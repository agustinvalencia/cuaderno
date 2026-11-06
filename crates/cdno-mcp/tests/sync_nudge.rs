//! The post-write sync-nudge sentinel (GH #540).
//!
//! Three properties, and the negative ones matter most: an agent that
//! is woken by writes that did not happen learns to ignore the signal.
//!
//! 1. off unless explicitly configured;
//! 2. touched after a write that verified;
//! 3. **not** touched when the write could not be verified.
//!
//! The sabotage is the same trick `handlers_verification.rs` uses — a
//! store that accepts a write under a prefix and keeps nothing — so an
//! unverifiable write is reachable without a real filesystem fault.

use std::path::Path;
use std::sync::Arc;

use cdno_core::config::VaultConfig;
use cdno_core::error::StoreError;
use cdno_core::file_meta::FileMeta;
use cdno_core::index::{MemoryIndex, VaultIndex};
use cdno_core::path::VaultPath;
use cdno_core::store::{MemoryVaultStore, VaultStore, VaultWriteLock};
use cdno_domain::Vault;
use cdno_mcp::CuadernoServer;
use cdno_mcp::nudge::SyncNudge;
use cdno_mcp::server::{AppendToLogInput, CaptureInput, RenameNoteInput};
use rmcp::handler::server::wrapper::Parameters;
use tempfile::TempDir;

/// Accepts writes under `swallow` and keeps nothing, so the domain call
/// returns `Ok` for a change that is not there.
struct SwallowingStore {
    inner: MemoryVaultStore,
    swallow: &'static str,
}

impl SwallowingStore {
    fn new(swallow: &'static str) -> Self {
        Self {
            inner: MemoryVaultStore::new(),
            swallow,
        }
    }
    fn swallows(&self, path: &VaultPath) -> bool {
        path.to_string().starts_with(self.swallow)
    }
}

impl VaultStore for SwallowingStore {
    fn read_file(&self, path: &VaultPath) -> Result<String, StoreError> {
        self.inner.read_file(path)
    }
    fn read_bytes(&self, path: &VaultPath) -> Result<Vec<u8>, StoreError> {
        self.inner.read_bytes(path)
    }
    fn write_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        if self.swallows(path) {
            return Ok(());
        }
        self.inner.write_file(path, content)
    }
    fn append_to_file(&self, path: &VaultPath, content: &str) -> Result<(), StoreError> {
        if self.swallows(path) {
            return Ok(());
        }
        self.inner.append_to_file(path, content)
    }
    fn move_file(&self, src: &VaultPath, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.move_file(src, dest)
    }
    fn delete_file(&self, path: &VaultPath) -> Result<(), StoreError> {
        self.inner.delete_file(path)
    }
    fn exists(&self, path: &VaultPath) -> Result<bool, StoreError> {
        self.inner.exists(path)
    }
    fn list_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.list_dir(path)
    }
    fn walk_dir(&self, path: &VaultPath) -> Result<Vec<VaultPath>, StoreError> {
        self.inner.walk_dir(path)
    }
    fn metadata(&self, path: &VaultPath) -> Result<FileMeta, StoreError> {
        self.inner.metadata(path)
    }
    fn import_external(&self, src: &Path, dest: &VaultPath) -> Result<(), StoreError> {
        self.inner.import_external(src, dest)
    }
    fn acquire_write_lock(&self) -> Result<VaultWriteLock, StoreError> {
        self.inner.acquire_write_lock()
    }
}

fn server_over(store: Arc<dyn VaultStore>) -> CuadernoServer {
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("open vault");
    CuadernoServer::new(Arc::new(vault), scratch_root())
}

#[tokio::test]
async fn the_sentinel_is_off_unless_it_is_configured() {
    // A server built the ordinary way must write nothing anywhere near
    // the sentinel's default location. Enabling is explicit.
    let dir = TempDir::new().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    let sentinel = SyncNudge::default_path(dir.path());

    let server = server_over(Arc::new(MemoryVaultStore::new()));
    server
        .append_to_log(Parameters(AppendToLogInput {
            text: "a line".to_owned(),
        }))
        .await
        .expect("the write succeeds");

    assert!(
        !sentinel.exists(),
        "no sentinel may appear without --sync-nudge"
    );
}

#[tokio::test]
async fn a_verified_write_touches_the_sentinel() {
    let dir = TempDir::new().unwrap();
    let sentinel = dir.path().join("cdno-sync.nudge");
    let server = server_over(Arc::new(MemoryVaultStore::new()))
        .with_sync_nudge(Arc::new(SyncNudge::new(sentinel.clone())));

    server
        .append_to_log(Parameters(AppendToLogInput {
            text: "a line".to_owned(),
        }))
        .await
        .expect("the write succeeds");

    assert!(sentinel.exists(), "a verified write must nudge");
    let payload = std::fs::read_to_string(&sentinel).unwrap();
    assert!(
        payload.trim().parse::<u64>().is_ok(),
        "the sentinel carries a timestamp, never vault content: {payload:?}"
    );
}

#[tokio::test]
async fn an_unverified_write_does_not_touch_the_sentinel() {
    // The signal means "something landed". A write the server could not
    // confirm must not wake the agent, or the agent commits nothing and
    // stops trusting the sentinel.
    let dir = TempDir::new().unwrap();
    let sentinel = dir.path().join("cdno-sync.nudge");
    let server = server_over(Arc::new(SwallowingStore::new("inbox/")))
        .with_sync_nudge(Arc::new(SyncNudge::new(sentinel.clone())));

    let outcome = server
        .capture(Parameters(CaptureInput {
            text: "swallowed".to_owned(),
        }))
        .await;

    assert!(outcome.is_err(), "the write must not verify");
    assert!(
        !sentinel.exists(),
        "an unverified write must leave the sentinel untouched"
    );
}

#[tokio::test]
async fn an_unwritable_sentinel_never_fails_the_write() {
    // The sentinel is a hint to an optional process. Failing a write
    // that already landed because the hint could not be delivered would
    // be strictly worse than the polling it replaces.
    let dir = TempDir::new().unwrap();
    let unreachable = dir.path().join("no-such-dir").join("cdno-sync.nudge");
    let server = server_over(Arc::new(MemoryVaultStore::new()))
        .with_sync_nudge(Arc::new(SyncNudge::new(unreachable.clone())));

    server
        .append_to_log(Parameters(AppendToLogInput {
            text: "a line".to_owned(),
        }))
        .await
        .expect("a write must succeed even when the sentinel cannot be written");
    assert!(!unreachable.exists());
}

#[test]
fn the_default_sentinel_hides_under_dot_git() {
    // Under `.git/` on purpose: git will not track it and no working-tree
    // mirror will carry it, so the signal can never become content.
    let path = SyncNudge::default_path(Path::new("/vault"));
    assert_eq!(path, Path::new("/vault/.git/cdno-sync.nudge"));
}

/// RFC 0007 §5.8 (3), T26: a written rename wakes the sync agent once, and a
/// preview, which writes nothing, never does (a signal for a write that did
/// not happen teaches the agent to ignore the signal).
#[tokio::test]
async fn a_written_rename_nudges_and_a_dry_run_does_not() {
    let dir = TempDir::new().unwrap();
    let sentinel = dir.path().join("cdno-sync.nudge");
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    store
        .write_file(
            &VaultPath::new("projects/surrogate-model.md").unwrap(),
            "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n\n## Milestones\n",
        )
        .unwrap();
    let server = server_over(store).with_sync_nudge(Arc::new(SyncNudge::new(sentinel.clone())));
    let input = |dry_run: bool, expected_plan: Option<String>| RenameNoteInput {
        note: "surrogate-model".to_owned(),
        to: Some("emulator-study".to_owned()),
        relink: None,
        reason: None,
        dry_run: Some(dry_run),
        expected_plan,
        skip_sync_check: None,
    };

    let preview = server
        .rename_note(Parameters(input(true, None)))
        .await
        .expect("a preview");
    assert!(
        !sentinel.exists(),
        "a dry run writes nothing, so it must not nudge"
    );

    let text = match &preview.content[0].raw {
        rmcp::model::RawContent::Text(t) => t.text.clone(),
        other => panic!("{other:?}"),
    };
    let hash = serde_json::from_str::<serde_json::Value>(&text).unwrap()["plan_hash"]
        .as_str()
        .unwrap()
        .to_owned();
    server
        .rename_note(Parameters(input(false, Some(hash))))
        .await
        .expect("the previewed plan is written");
    assert!(sentinel.exists(), "a written rename must nudge");
}

/// A fresh empty directory as the server's vault root: the in-memory vault
/// has no folder of its own, and the root must not be a place git could find
/// a repository above (a developer's temp dir may sit inside one).
fn scratch_root() -> std::path::PathBuf {
    tempfile::TempDir::new().expect("tempdir").keep()
}
