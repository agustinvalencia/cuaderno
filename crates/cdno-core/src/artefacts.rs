//! Attachment artefacts inside a portfolio.
//!
//! Filing a document into a portfolio writes an *evidence stub* beside a
//! folder of the same stem, and everything inside that folder belongs to
//! the stub:
//!
//! ```text
//! portfolios/<portfolio>/<stem>.md   <- the evidence stub (kind: <kind>)
//! portfolios/<portfolio>/<stem>/…    <- the artefacts it owns
//! ```
//!
//! Membership is by *location*, not extension: a markdown file inside an
//! artefact folder is a document that was filed, not a note. It carries no
//! frontmatter contract and its searchable abstract lives in the stub, so
//! indexing it can only ever fail.
//!
//! **Ownership is established positively.** It is not enough for a sibling
//! `<stem>.md` to exist — it must actually be an attachment stub. Inferring
//! ownership from the path shape alone would mean an ordinary note that
//! happens to share a name with a sibling folder swallows that folder's
//! real notes out of the index, silently: `portfolios/climate/2026-Q2.md`
//! written as a quarter summary beside a hand-made `portfolios/climate/2026-Q2/`
//! holding eight evidence notes would evict all eight from search,
//! backlinks and portfolio contents, with the files untouched on disk and
//! nothing naming them. The frontmatter check is what makes that
//! impossible.
//!
//! A Typst note owns a folder the same way (RFC 0008 §5.7): `<stem>.typ`
//! beside `<stem>/`, in `portfolios/` or in the folder of a type that
//! admits Typst. It owns it only if it is a note, which is established by
//! reading: the `<cdno>` label and a `.typ` extension.
//!
//! This module lives in `cdno-core` because reconciliation — which is here
//! — has to apply the rule before any domain type exists, and it is
//! deliberately the only place core encodes the stub contract.

use std::collections::HashSet;
use std::sync::Arc;

use crate::path::VaultPath;
use crate::paths::PORTFOLIOS;
use crate::paths::{NoteFormat, note_format};
use crate::store::VaultStore;
use crate::typst;

/// The note type an attachment stub carries.
const EVIDENCE_TYPE: &str = "evidence";
/// The frontmatter field carrying a note's type.
const TYPE_FIELD: &str = "type";
/// The frontmatter field whose presence marks an evidence note as a stub
/// standing in for a filed artefact (`pdf`, `image`, `file`, …).
const KIND_FIELD: &str = "kind";
/// `portfolios/<portfolio>/<stem>` — the shallowest folder that can own
/// artefacts, and so the fewest components an owning directory can have.
const MIN_OWNER_DEPTH: usize = 3;

/// Whether the note at `path` is an attachment stub: an evidence note
/// carrying a non-empty `kind`.
///
/// An unreadable or unparseable file is not a stub. Being wrong in that
/// direction merely leaves a file indexed (where a parse error is reported
/// normally); being wrong in the other direction removes notes from the
/// index without a word.
pub fn is_attachment_stub(store: &Arc<dyn VaultStore>, path: &VaultPath) -> bool {
    let Ok(raw) = store.read_file(path) else {
        return false;
    };
    let Ok((frontmatter, _body)) = crate::frontmatter::parse_note(path, &raw) else {
        return false;
    };
    let is_evidence = frontmatter
        .optional_field::<String>(TYPE_FIELD)
        .ok()
        .flatten()
        .is_some_and(|t| t == EVIDENCE_TYPE);
    let has_kind = frontmatter
        .optional_field::<String>(KIND_FIELD)
        .ok()
        .flatten()
        .is_some_and(|k| !k.trim().is_empty());
    is_evidence && has_kind
}

/// Whether the file at `path` is a Typst note: a `.typ` carrying the label.
///
/// Like [`is_attachment_stub`], an unreadable file is not an owner, since
/// being wrong in the other direction evicts notes without a word.
pub fn is_typst_note(store: &Arc<dyn VaultStore>, path: &VaultPath) -> bool {
    if note_format(path) != Some(NoteFormat::Typst) {
        return false;
    }
    let Ok(bytes) = store.read_bytes(path) else {
        return false;
    };
    std::str::from_utf8(&bytes).is_ok_and(typst::has_label)
}

/// The notes among `files` that own a folder, resolved with one read each.
///
/// A stub can own `<dir>/` only under `portfolios/`; a Typst note can own it
/// under `portfolios/` or any of `typst_folders`. Only files that could own
/// something are read: those with a sibling directory of the same stem, so
/// the read count is bounded by the number of `<dir>.md|typ` / `<dir>/`
/// pairs, not by the note count. These reads sit outside the mtime+size fast
/// path (#94), which is why the candidate set is kept this narrow.
///
/// Folders are visited shallowest first and a folder inside an owned folder
/// is skipped, so a labelled file inside an assets folder is never opened.
/// A folder claimed by both a stub and a Typst note yields both; see
/// [`owning_note`] for which one wins.
pub fn folder_owners(
    store: &Arc<dyn VaultStore>,
    files: &HashSet<VaultPath>,
    dirs: &HashSet<VaultPath>,
    typst_folders: &[String],
) -> HashSet<VaultPath> {
    let mut candidates: Vec<&VaultPath> = dirs.iter().collect();
    candidates.sort_by_key(|dir| dir.as_path().components().count());

    let mut owned_dirs: Vec<&VaultPath> = Vec::new();
    let mut owners = HashSet::new();
    for dir in candidates {
        if owned_dirs
            .iter()
            .any(|owned| dir.as_path().starts_with(owned.as_path()))
        {
            continue;
        }
        let mut claimed = false;
        if stub_depth_ok(dir.as_path())
            && let Some(stub) = sibling_with(dir.as_path(), "md")
            && files.contains(&stub)
            && is_attachment_stub(store, &stub)
        {
            owners.insert(stub);
            claimed = true;
        }
        if typst_depth_ok(dir.as_path(), typst_folders)
            && let Some(note) = sibling_with(dir.as_path(), "typ")
            && files.contains(&note)
            && is_typst_note(store, &note)
        {
            owners.insert(note);
            claimed = true;
        }
        if claimed {
            owned_dirs.push(dir);
        }
    }
    owners
}

/// The note that owns `path` as an attachment artefact, if any.
///
/// The walk goes from the outermost ancestor inwards, so owners resolve
/// shallow-first: once a folder is owned nothing inside it is probed, and an
/// artefact nested inside an artefact folder (a filed directory tree keeps
/// its internal structure) resolves to the same owner as its siblings. A
/// candidate that is not an owner does not stop the walk.
///
/// A stub-beside-folder under `portfolios/` is the portfolio convention, and
/// applying it vault-wide would be actively harmful: an expanded stewardship
/// is `stewardships/<slug>/` and a flat one is `stewardships/<slug>.md`, so a
/// vault holding both spellings of one slug would see the expanded folder's
/// notes silently vanish from the index. A Typst note owns a folder only
/// under `portfolios/` or `typst_folders`, for the same reason.
///
/// Within one folder the `.md` candidate is asked first, so a stub and a
/// Typst note sharing a stem resolve to the stub, deterministically.
///
/// `is_owner` decides whether a candidate really is one. It is `FnMut` so a
/// caller can memoise: each probe costs a read and a parse. Reconciliation
/// passes a set resolved once by [`folder_owners`].
pub fn owning_note(
    path: &VaultPath,
    typst_folders: &[String],
    mut is_owner: impl FnMut(&VaultPath) -> bool,
) -> Option<VaultPath> {
    let mut ancestors: Vec<&std::path::Path> = path.as_path().ancestors().skip(1).collect();
    ancestors.reverse();
    for dir in ancestors {
        if stub_depth_ok(dir)
            && let Some(stub) = sibling_with(dir, "md")
            && is_owner(&stub)
        {
            return Some(stub);
        }
        if typst_depth_ok(dir, typst_folders)
            && let Some(note) = sibling_with(dir, "typ")
            && is_owner(&note)
        {
            return Some(note);
        }
    }
    None
}

/// `<dir>.<extension>` — the note a directory would pair with.
///
/// Built with `with_file_name`, never `with_extension`: the latter
/// *replaces* a trailing dotted segment, so a folder named `run-v1.2` would
/// yield `run-v1.md` and pair the artefact with an unrelated note.
fn sibling_with(dir: &std::path::Path, extension: &str) -> Option<VaultPath> {
    let name = dir.file_name()?.to_str()?;
    VaultPath::new(dir.with_file_name(format!("{name}.{extension}"))).ok()
}

fn stub_depth_ok(dir: &std::path::Path) -> bool {
    dir.starts_with(PORTFOLIOS) && dir.components().count() >= MIN_OWNER_DEPTH
}

/// `portfolios` keeps its `<portfolio>/<stem>` depth; any other folder of an
/// admitting type owns at `<folder>/<stem>`.
fn typst_depth_ok(dir: &std::path::Path, typst_folders: &[String]) -> bool {
    let depth = dir.components().count();
    typst_folders.iter().any(|folder| {
        let root = std::path::Path::new(folder);
        if root == std::path::Path::new(PORTFOLIOS) {
            stub_depth_ok(dir)
        } else {
            dir.starts_with(root) && depth > root.components().count()
        }
    })
}
