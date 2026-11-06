//! A `MemoryIndex` that, once armed, fails the next `replace_milestones`
//! (and only that one): an index update failing after every file of a
//! commit landed, which the commit reports as `IndexStale`. Every other
//! call delegates. The same double as `cdno-domain`'s unit suite uses.

use std::sync::{Arc, Mutex};

use cdno_core::error::IndexError;
use cdno_core::index::{
    ArchivalSnapshot, DeadlineEntry, LinkEntry, LinkResolution, LinkResolutionUpdate, MemoryIndex,
    MilestoneEntry, NoteCandidate, NoteEntry, SearchHit, VaultIndex,
};
use cdno_core::path::VaultPath;

/// Wraps a `MemoryIndex` and, once armed, fails the next
/// `replace_milestones` (and only that one): an index update failing after
/// every file of a commit landed, which the commit reports as
/// `IndexStale`. Every other call delegates.
pub struct FlakyIndex {
    inner: Arc<MemoryIndex>,
    armed: Mutex<bool>,
}

impl FlakyIndex {
    pub fn new(inner: Arc<MemoryIndex>) -> Self {
        Self {
            inner,
            armed: Mutex::new(false),
        }
    }

    pub fn arm(&self) {
        *self.armed.lock().unwrap() = true;
    }
}

impl VaultIndex for FlakyIndex {
    fn upsert_note(&self, entry: &NoteEntry) -> Result<(), IndexError> {
        self.inner.upsert_note(entry)
    }
    fn remove_note(&self, path: &VaultPath) -> Result<(), IndexError> {
        self.inner.remove_note(path)
    }
    fn find_by_path(&self, path: &VaultPath) -> Result<Option<NoteEntry>, IndexError> {
        self.inner.find_by_path(path)
    }
    fn list_by_type(&self, note_type: &str) -> Result<Vec<NoteEntry>, IndexError> {
        self.inner.list_by_type(note_type)
    }
    fn list_all_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.list_all_paths()
    }
    fn list_aliases(&self) -> Result<Vec<(String, VaultPath)>, IndexError> {
        self.inner.list_aliases()
    }
    fn list_candidates(&self) -> Result<Vec<NoteCandidate>, IndexError> {
        self.inner.list_candidates()
    }
    fn replace_deadlines(
        &self,
        path: &VaultPath,
        deadlines: &[DeadlineEntry],
    ) -> Result<(), IndexError> {
        self.inner.replace_deadlines(path, deadlines)
    }
    fn deadlines_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, DeadlineEntry)>, IndexError> {
        self.inner.deadlines_between(from, to)
    }
    fn replace_links(&self, path: &VaultPath, links: &[LinkEntry]) -> Result<(), IndexError> {
        self.inner.replace_links(path, links)
    }
    fn find_backlinks(&self, path: &VaultPath) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.find_backlinks(path)
    }
    fn find_outgoing_links(&self, path: &VaultPath) -> Result<Vec<LinkEntry>, IndexError> {
        self.inner.find_outgoing_links(path)
    }
    fn link_resolutions(&self) -> Result<Vec<LinkResolution>, IndexError> {
        self.inner.link_resolutions()
    }
    fn set_link_resolutions(&self, updates: &[LinkResolutionUpdate]) -> Result<usize, IndexError> {
        self.inner.set_link_resolutions(updates)
    }
    fn replace_tags(&self, path: &VaultPath, tags: &[String]) -> Result<(), IndexError> {
        self.inner.replace_tags(path, tags)
    }
    fn find_by_tag(&self, tag: &str) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.find_by_tag(tag)
    }
    fn replace_milestones(
        &self,
        path: &VaultPath,
        milestones: &[MilestoneEntry],
    ) -> Result<(), IndexError> {
        let mut armed = self.armed.lock().unwrap();
        if *armed {
            *armed = false;
            return Err(IndexError::Update("forced test failure".to_owned()));
        }
        self.inner.replace_milestones(path, milestones)
    }
    fn milestones_for_project(&self, path: &VaultPath) -> Result<Vec<MilestoneEntry>, IndexError> {
        self.inner.milestones_for_project(path)
    }
    fn milestones_between(
        &self,
        from: &str,
        to: &str,
    ) -> Result<Vec<(VaultPath, MilestoneEntry)>, IndexError> {
        self.inner.milestones_between(from, to)
    }
    fn replace_fts(
        &self,
        path: &VaultPath,
        title: Option<&str>,
        body: &str,
    ) -> Result<(), IndexError> {
        self.inner.replace_fts(path, title, body)
    }
    fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>, IndexError> {
        self.inner.search(query, limit)
    }
    fn fts_indexed_paths(&self) -> Result<Vec<VaultPath>, IndexError> {
        self.inner.fts_indexed_paths()
    }
    fn record_archival_snapshot(
        &self,
        path: &VaultPath,
        snapshot: &ArchivalSnapshot,
    ) -> Result<(), IndexError> {
        self.inner.record_archival_snapshot(path, snapshot)
    }
    fn find_archival_snapshot(
        &self,
        path: &VaultPath,
    ) -> Result<Option<ArchivalSnapshot>, IndexError> {
        self.inner.find_archival_snapshot(path)
    }
    fn list_archival_snapshots(&self) -> Result<Vec<(VaultPath, ArchivalSnapshot)>, IndexError> {
        self.inner.list_archival_snapshots()
    }
}
