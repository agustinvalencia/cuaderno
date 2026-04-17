use std::fs::Metadata;
use std::io;
use std::time::SystemTime;

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
