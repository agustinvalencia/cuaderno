//! What in a vault is a note.
//!
//! One walk decides, so reconciliation and every other reader of the note
//! set agree because they ask the same function. "Is a note" means
//! membership in [`Classified::notes`]; the index holds exactly that set
//! after a reconcile.
//!
//! A file is sorted in a fixed order: drop `.cuaderno/`, resolve the
//! attachment stubs over every Markdown file, set aside whatever a stub
//! owns, and only then look at the file's format and the `ignore` globs.
//! Ownership comes first so that the two exclusions stay independent
//! (ignoring a stub must not promote its artefacts into notes) and so that
//! no format-specific handling ever sees a file inside an owned folder.

use std::collections::{BTreeSet, HashSet};
use std::path::Path;
use std::sync::Arc;

use crate::artefacts::{folder_owners, owning_note};
use crate::config::IgnoreSet;
use crate::error::StoreError;
use crate::path::VaultPath;
use crate::paths::{CUADERNO_DIR, NoteFormat, note_format};
use crate::store::VaultStore;
use crate::typst;

/// What the classifier needs to know about a vault. Core cannot tell which
/// note types admit Typst, so the domain says where they live.
#[derive(Debug, Clone)]
pub struct NoteScope {
    pub ignore: IgnoreSet,
    /// Folders in which a `.typ` file may be a note.
    pub typst_folders: Vec<String>,
    /// Folders of the note types the `[typst]` switch leaves out. A labelled
    /// `.typ` here is reported, so turning Typst off hides no note silently.
    pub typst_excluded_folders: Vec<String>,
    /// The note types that admit Typst. A `.typ` whose block `type` is not listed is kept out of
    /// the index and reported by reconciliation. Empty means no check, as when no folder admits
    /// Typst.
    pub typst_types: Vec<String>,
}

impl NoteScope {
    /// A scope with these `ignore` globs and no Typst folders.
    pub fn new(ignore: IgnoreSet) -> Self {
        Self {
            ignore,
            typst_folders: Vec::new(),
            typst_excluded_folders: Vec::new(),
            typst_types: Vec::new(),
        }
    }

    /// A scope that excludes nothing and admits no Typst.
    pub fn empty() -> Self {
        Self::new(IgnoreSet::empty())
    }

    /// Whether `dir` is itself the folder of a declared type, in either list.
    /// No note owns such a folder: it is another type's root, not an attachment folder.
    pub(crate) fn is_type_root(&self, dir: &std::path::Path) -> bool {
        self.typst_folders
            .iter()
            .chain(&self.typst_excluded_folders)
            .any(|folder| std::path::Path::new(folder) == dir)
    }

    /// `Some(admitted)` for a path under either Typst list, `None` for a path
    /// under neither. Where the lists nest, the deepest folder decides.
    pub(crate) fn typst_admission(&self, path: &VaultPath) -> Option<bool> {
        let depth = |folders: &[String]| {
            folders
                .iter()
                .filter(|folder| path.as_path().starts_with(folder))
                .map(|folder| Path::new(folder).components().count())
                .max()
        };
        let admitted = depth(&self.typst_folders);
        let excluded = depth(&self.typst_excluded_folders);
        (admitted.is_some() || excluded.is_some()).then_some(admitted >= excluded)
    }
}

/// Why a file that could have been a note is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    Attachment,
    Ignored,
    /// A loose `.typ` in an admitted folder: neither note nor attachment.
    TypstUnlabelled,
    /// No `<cdno>`, but the bytes hold a metadata call with a `type` key.
    /// `line` is the 1-based line of the call.
    TypstLabelBroken {
        line: usize,
    },
    /// A labelled `.typ` in a folder the `[typst]` switch excludes.
    TypstOutsideScope,
}

/// A file the classifier left out, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub path: VaultPath,
    pub reason: SkipReason,
}

/// The classifier's answer for one walk of a vault.
#[derive(Debug, Default)]
pub struct Classified {
    /// What reconciliation indexes.
    pub notes: BTreeSet<VaultPath>,
    /// Every walked path outside `.cuaderno/`, whatever its extension.
    pub files: BTreeSet<VaultPath>,
    /// Files left out and why. Ignored files are listed as well as counted;
    /// attachments are only counted.
    pub skipped: Vec<Skipped>,
    /// Markdown files owned by an attachment stub.
    pub artefacts: usize,
    /// Candidate notes matched by the `ignore` globs.
    pub ignored: usize,
}

/// Walk the vault and sort its files. Fails only if the walk itself does;
/// a stub that cannot be read is simply not a stub.
pub fn classify(store: &Arc<dyn VaultStore>, scope: &NoteScope) -> Result<Classified, StoreError> {
    let walked = store.walk_dir(&VaultPath::root())?;

    // Every directory on the path to a walked file: the only places an
    // artefact folder can be, and the filter that keeps the stub lookup
    // from reading the whole vault. All ancestors, not just parents, since
    // a filed directory tree may hold no direct files at all. The empty
    // path that `ancestors` ends on is rejected by `VaultPath::new`.
    let dirs: HashSet<VaultPath> = walked
        .iter()
        .flat_map(|p| p.as_path().ancestors().skip(1))
        .filter_map(|d| VaultPath::new(d).ok())
        .collect();

    // `.cuaderno/` holds config, templates and the index database; indexing
    // it would surface e.g. an ejected daily template as a daily note.
    let files: BTreeSet<VaultPath> = walked
        .into_iter()
        .filter(|p| !p.as_path().starts_with(CUADERNO_DIR))
        .collect();

    // Over every file, ignored ones included: an ignored stub still owns its
    // folder.
    let all: HashSet<VaultPath> = files.iter().cloned().collect();
    let owners = folder_owners(store, &all, &dirs, scope);

    let mut classified = Classified::default();
    for path in &files {
        let format = note_format(path);
        if owning_note(path, scope, |owner| owners.contains(owner)).is_some() {
            classified.artefacts += usize::from(format == Some(NoteFormat::Markdown));
            continue;
        }
        match format {
            Some(NoteFormat::Markdown) => classified.candidate(path, scope),
            Some(NoteFormat::Typst) => classify_typst(store, path, scope, &mut classified),
            None => {}
        }
    }
    classified.files = files;
    Ok(classified)
}

impl Classified {
    /// A file that is a note unless the `ignore` globs say otherwise. Counted
    /// rather than silently filtered: an over-broad pattern evicting notes
    /// from retrieval has to be observable.
    fn candidate(&mut self, path: &VaultPath, scope: &NoteScope) {
        if scope.ignore.is_match(path.as_path()) {
            self.ignored += 1;
            self.skipped.push(Skipped {
                path: path.clone(),
                reason: SkipReason::Ignored,
            });
        } else {
            self.notes.insert(path.clone());
        }
    }
}

/// Sort a `.typ` that no stub owns (RFC 0008 §5.2). Byte searches only: the
/// parse belongs to reconciliation.
fn classify_typst(
    store: &Arc<dyn VaultStore>,
    path: &VaultPath,
    scope: &NoteScope,
    classified: &mut Classified,
) {
    let Some(admitted) = scope.typst_admission(path) else {
        return;
    };
    if admitted && scope.ignore.is_match(path.as_path()) {
        classified.candidate(path, scope);
        return;
    }
    let Ok(bytes) = store.read_bytes(path) else {
        // Left to reconciliation, which reports the failed read and leaves
        // any existing row in place. Dropping it here would lose it silently.
        if admitted {
            classified.notes.insert(path.clone());
        }
        return;
    };
    // Typst source is UTF-8, so bytes that do not decode hold no label.
    let source = std::str::from_utf8(&bytes).unwrap_or_default();
    let reason = match (admitted, typst::has_label(source)) {
        (true, true) => {
            classified.notes.insert(path.clone());
            return;
        }
        (false, true) => SkipReason::TypstOutsideScope,
        (false, false) => return,
        (true, false) => match typst::broken_label_line(source) {
            Some(line) => SkipReason::TypstLabelBroken { line },
            None => SkipReason::TypstUnlabelled,
        },
    };
    classified.skipped.push(Skipped {
        path: path.clone(),
        reason,
    });
}
