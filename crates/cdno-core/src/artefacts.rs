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
//! This module lives in `cdno-core` because reconciliation — which is here
//! — has to apply the rule before any domain type exists, and it is
//! deliberately the only place core encodes the stub contract.

use std::collections::HashSet;
use std::sync::Arc;

use crate::frontmatter::Frontmatter;
use crate::path::VaultPath;
use crate::paths::PORTFOLIOS;
use crate::store::VaultStore;

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
    let Ok((frontmatter, _body)) = Frontmatter::parse(&raw) else {
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

/// The attachment stubs among `md_paths`, resolved with one read each.
///
/// Only files that could actually own something are read: a stub must sit
/// under `portfolios/` and have a sibling directory of the same stem. The
/// read count is therefore bounded by the number of `<dir>.md` / `<dir>/`
/// pairs in the vault — the artefact folders themselves, plus any
/// accidental namesake — not by the note count, so a portfolio full of
/// ordinary evidence notes costs no reads at all. These reads sit outside
/// the mtime+size fast path (#94), which is why the candidate set is kept
/// this narrow.
pub fn attachment_stubs(
    store: &Arc<dyn VaultStore>,
    md_paths: &HashSet<VaultPath>,
    dirs: &HashSet<VaultPath>,
) -> HashSet<VaultPath> {
    dirs.iter()
        .filter(|dir| owner_depth_ok(dir))
        .filter_map(sibling_stub_of)
        .filter(|candidate| md_paths.contains(candidate))
        .filter(|candidate| is_attachment_stub(store, candidate))
        .collect()
}

/// The evidence stub that owns `path` as an attachment artefact, if any.
///
/// The walk goes from the nearest ancestor outwards rather than testing a
/// fixed depth, so the rule survives any intervening folder — which is what
/// lets a portfolio grow grouping subfolders without this changing. It also
/// means an artefact nested inside an artefact folder (a filed directory
/// tree keeps its internal structure) resolves to the same owning stub as
/// its siblings. A candidate that is not an attachment stub does not stop
/// the walk: an outer stub can still own the file.
///
/// Only ancestors strictly inside `portfolios/` count. The
/// stub-beside-folder shape is a portfolio convention, and applying it
/// vault-wide would be actively harmful: an expanded stewardship is
/// `stewardships/<slug>/` and a flat one is `stewardships/<slug>.md`, so a
/// vault holding both spellings of one slug would see the expanded folder's
/// notes silently vanish from the index.
///
/// `is_stub` decides whether a candidate is a real attachment stub. It
/// is `FnMut` so a caller can memoise: the walk probes the same candidate
/// once per file beneath it, and each probe costs a read and a parse.
/// Reconciliation passes a set resolved once by [`attachment_stubs`].
pub fn owning_artefact_stub(
    path: &VaultPath,
    mut is_stub: impl FnMut(&VaultPath) -> bool,
) -> Option<VaultPath> {
    let mut ancestor = path.as_path().parent();
    while let Some(dir) = ancestor {
        if !owner_depth_ok_path(dir) {
            return None;
        }
        if let Some(stub) = sibling_stub_of_path(dir)
            && is_stub(&stub)
        {
            return Some(stub);
        }
        ancestor = dir.parent();
    }
    None
}

/// `<dir>.md` — the stub a directory would pair with.
///
/// Built with `with_file_name`, never `with_extension`: the latter
/// *replaces* a trailing dotted segment, so a folder named `run-v1.2` would
/// yield `run-v1.md` and pair the artefact with an unrelated note.
fn sibling_stub_of_path(dir: &std::path::Path) -> Option<VaultPath> {
    let name = dir.file_name()?.to_str()?;
    VaultPath::new(dir.with_file_name(format!("{name}.md"))).ok()
}

fn sibling_stub_of(dir: &VaultPath) -> Option<VaultPath> {
    sibling_stub_of_path(dir.as_path())
}

fn owner_depth_ok_path(dir: &std::path::Path) -> bool {
    dir.starts_with(PORTFOLIOS) && dir.components().count() >= MIN_OWNER_DEPTH
}

fn owner_depth_ok(dir: &VaultPath) -> bool {
    owner_depth_ok_path(dir.as_path())
}
