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
use std::sync::Arc;

use crate::artefacts::{attachment_stubs, owning_artefact_stub};
use crate::config::IgnoreSet;
use crate::error::StoreError;
use crate::path::VaultPath;
use crate::paths::{CUADERNO_DIR, NoteFormat, note_format};
use crate::store::VaultStore;

/// What the classifier needs to know about a vault. Core cannot tell which
/// note types admit Typst, so the domain says where they live.
#[derive(Debug, Clone)]
pub struct NoteScope {
    pub ignore: IgnoreSet,
    /// Folders in which a `.typ` file may be a note.
    pub typst_folders: Vec<String>,
}

impl NoteScope {
    /// A scope with these `ignore` globs and no Typst folders.
    pub fn new(ignore: IgnoreSet) -> Self {
        Self {
            ignore,
            typst_folders: Vec::new(),
        }
    }

    /// A scope that excludes nothing and admits no Typst.
    pub fn empty() -> Self {
        Self::new(IgnoreSet::empty())
    }
}

/// Why a file that could have been a note is not one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    Attachment,
    Ignored,
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
    /// Markdown files matched by the `ignore` globs.
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

    // Over the full Markdown set, ignored files included: an ignored stub
    // still owns its folder.
    let markdown: HashSet<VaultPath> = files
        .iter()
        .filter(|p| note_format(p) == Some(NoteFormat::Markdown))
        .cloned()
        .collect();
    let stubs = attachment_stubs(store, &markdown, &dirs);

    let mut classified = Classified::default();
    for path in &files {
        let is_markdown = markdown.contains(path);
        if owning_artefact_stub(path, |stub| stubs.contains(stub)).is_some() {
            classified.artefacts += usize::from(is_markdown);
            continue;
        }
        if !is_markdown {
            continue;
        }
        // Counted rather than silently filtered: an over-broad pattern
        // evicting notes from retrieval has to be observable.
        if scope.ignore.is_match(path.as_path()) {
            classified.ignored += 1;
            classified.skipped.push(Skipped {
                path: path.clone(),
                reason: SkipReason::Ignored,
            });
        } else {
            classified.notes.insert(path.clone());
        }
    }
    classified.files = files;
    Ok(classified)
}
