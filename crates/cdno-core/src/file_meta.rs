use std::fs::Metadata;
use std::io;
use std::time::{SystemTime, UNIX_EPOCH};

/// Snapshot of a file's modification time and size.
///
/// Used by the index layer to detect stale cache entries via mtime
/// comparison without opening the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    pub mtime: SystemTime,
    pub size: u64,
}

impl FileMeta {
    pub fn new(mtime: SystemTime, size: u64) -> Self {
        Self { mtime, size }
    }

    /// `mtime` as nanoseconds since the UNIX epoch — the form stored in
    /// `NoteEntry::mtime_ns` and compared by the reconcile fast-path.
    /// Pre-epoch times (which shouldn't occur on a live filesystem) clamp
    /// to 0.
    pub fn mtime_ns(&self) -> u64 {
        self.mtime
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0)
    }
}

impl TryFrom<Metadata> for FileMeta {
    type Error = io::Error;

    fn try_from(meta: Metadata) -> Result<Self, Self::Error> {
        Ok(Self {
            mtime: meta.modified()?,
            size: meta.len(),
        })
    }
}
