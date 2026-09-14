//! Read-back verification on the mutating tools (GH #539).
//!
//! The failure this closes is a write that reports success without
//! having landed, so the interesting cases are the negative ones: a
//! store that accepts a write and keeps nothing must surface as a tool
//! **error**, never as a `WriteResultDto`.
//!
//! Same in-process pattern as `handlers_operations.rs` — call the
//! handler method directly and decode the JSON payload — plus a
//! sabotaging [`VaultStore`] decorator that silently drops writes under
//! a chosen prefix.

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
use cdno_mcp::server::{
    AppendToLogInput, CaptureInput, CreateProjectInput, DiscardInboxItemInput, StartActionInput,
    StartUnplannedActionInput,
};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, RawContent};
use serde_json::Value;

// ---------------------------------------------------------------------
// A store that loses writes silently — the bug being defended against
// ---------------------------------------------------------------------

/// Wraps [`MemoryVaultStore`] and throws away any `write_file` whose
/// path starts with `swallow`, reporting success. Everything else
/// delegates. This is what a silently-failing write path looks like
/// from the domain's side: the call returns `Ok`, and nothing is there
/// afterwards.
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

// ---------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------

fn server_over(store: Arc<dyn VaultStore>) -> CuadernoServer {
    let index: Arc<dyn VaultIndex> = Arc::new(MemoryIndex::new());
    let (vault, _report) = Vault::new(store, index, VaultConfig::default()).expect("open vault");
    CuadernoServer::new(Arc::new(vault))
}

fn healthy_server() -> CuadernoServer {
    server_over(Arc::new(MemoryVaultStore::new()))
}

fn decode(result: &CallToolResult) -> Value {
    assert_eq!(result.is_error, Some(false), "tool errored: {result:?}");
    let RawContent::Text(text) = &result.content.first().expect("one content item").raw else {
        panic!("expected text content: {result:?}");
    };
    serde_json::from_str(&text.text).expect("payload is JSON")
}

fn verification(result: &CallToolResult) -> Value {
    decode(result)["verification"].clone()
}

// ---------------------------------------------------------------------
// A write that landed
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_verified_write_reports_bytes_and_a_content_hash() {
    let server = healthy_server();
    let result = server
        .append_to_log(Parameters(AppendToLogInput {
            text: "hooked the surrogate-model baseline up to the sweep".to_owned(),
        }))
        .await
        .expect("append_to_log succeeds");

    let v = verification(&result);
    assert_eq!(v["verified"], "content");
    let bytes = v["bytes_written"].as_u64().expect("bytes_written is a u64");
    assert!(bytes > 0, "a written note is not empty");

    let hash = v["content_hash"]
        .as_str()
        .expect("content_hash is a string");
    assert_eq!(hash.len(), 16, "xxh3-64 renders as 16 hex chars: {hash}");
    assert!(
        hash.chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase()),
        "the hash is lowercase hex: {hash}"
    );
}

#[tokio::test]
async fn the_hash_matches_the_bytes_that_are_actually_on_disk() {
    // The point of the field: a caller can recompute it from the note
    // and know it is looking at the same content the server saw.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    let server = server_over(Arc::clone(&store));
    let result = server
        .append_to_log(Parameters(AppendToLogInput {
            text: "first line".to_owned(),
        }))
        .await
        .expect("append_to_log succeeds");

    let payload = decode(&result);
    let path = VaultPath::new(payload["path"].as_str().unwrap()).unwrap();
    let on_disk = store.read_file(&path).expect("the note exists");

    assert_eq!(
        payload["verification"]["content_hash"].as_str().unwrap(),
        cdno_core::hash::content_hash(&on_disk)
    );
    assert_eq!(
        payload["verification"]["bytes_written"].as_u64().unwrap(),
        on_disk.len() as u64
    );
}

#[tokio::test]
async fn an_append_shaped_write_returns_the_tail_that_landed() {
    let server = healthy_server();
    let result = server
        .append_to_log(Parameters(AppendToLogInput {
            text: "a distinctive marker".to_owned(),
        }))
        .await
        .expect("append_to_log succeeds");

    let tail = verification(&result)["appended_tail"]
        .as_str()
        .expect("an append-shaped write carries its tail")
        .to_owned();
    assert!(
        tail.contains("a distinctive marker"),
        "the tail must show the line that landed: {tail}"
    );
}

/// A custom daily template whose LAST `##` heading is not `Logs`.
///
/// The domain pins the template's last section to the bottom of the
/// note, whichever it is — `cdno-domain`'s `daily_anchor_section`, and
/// the case `daily_anchor_follows_a_custom_templates_last_section` in
/// `cdno-domain/tests/unit/templating_tests.rs` is written against this
/// exact shape. So with this template the appended log line lands in
/// the MIDDLE of the file, and the last N bytes of the file are the
/// Reflection filler below.
const DAILY_TEMPLATE_ANCHORED_ELSEWHERE: &str = concat!(
    "---\ntype: daily\ndate: {{date}}\n---\n\n# {{heading}}\n\n## Logs\n\n## Reflection\n\n",
    // ~1 KB of filler, so the trailing window cannot reach back into
    // the Logs section by accident and mask the bug.
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
    "reflection filler reflection filler reflection filler reflection filler\n",
);

#[tokio::test]
async fn the_tail_is_the_appended_section_not_the_end_of_the_file() {
    // Regression (PR #549 review): the tail used to be the last N bytes
    // of the file, on the assumption that a log line always lands at
    // EOF. That holds only for the built-in template. With the daily
    // note anchored on a different section the line lands mid-file, and
    // the old tail showed the Reflection filler while claiming to be
    // "the text that landed" — a silently wrong answer, which is worse
    // than no answer.
    let store: Arc<dyn VaultStore> = Arc::new(MemoryVaultStore::new());
    store
        .write_file(
            &VaultPath::new(".cuaderno/templates/daily.md").unwrap(),
            DAILY_TEMPLATE_ANCHORED_ELSEWHERE,
        )
        .expect("seed the custom daily template");
    let server = server_over(Arc::clone(&store));

    let result = server
        .append_to_log(Parameters(AppendToLogInput {
            text: "a distinctive marker".to_owned(),
        }))
        .await
        .expect("append_to_log succeeds");

    // The write itself is fine either way — this is about the evidence.
    let payload = decode(&result);
    let path = VaultPath::new(payload["path"].as_str().unwrap()).unwrap();
    let on_disk = store.read_file(&path).expect("the note exists");
    assert!(
        on_disk.contains("a distinctive marker"),
        "the line must have landed:\n{on_disk}"
    );
    assert!(
        on_disk.trim_end().ends_with("reflection filler"),
        "this template must anchor on Reflection, or the test proves nothing:\n{on_disk}"
    );

    let tail = payload["verification"]["appended_tail"]
        .as_str()
        .expect("an append-shaped write still carries a tail")
        .to_owned();
    assert!(
        tail.contains("a distinctive marker"),
        "the tail must show the line that landed, not the end of the file: {tail:?}"
    );
    assert!(
        !tail.contains("reflection filler"),
        "the tail must be scoped to the section written to: {tail:?}"
    );
}

#[tokio::test]
async fn a_rewrite_shaped_write_carries_no_tail() {
    // The tail is only meaningful where the change is at EOF; offering
    // one elsewhere would point the caller at the wrong bytes.
    let server = healthy_server();
    let result = server
        .capture(Parameters(CaptureInput {
            text: "triage me later".to_owned(),
        }))
        .await
        .expect("capture succeeds");

    let v = verification(&result);
    assert_eq!(v["verified"], "content");
    assert!(
        v["appended_tail"].is_null(),
        "no tail on a whole-file write"
    );
}

#[tokio::test]
async fn a_delete_is_verified_as_removed_not_as_content() {
    let server = healthy_server();
    let captured = server
        .capture(Parameters(CaptureInput {
            text: "throwaway".to_owned(),
        }))
        .await
        .expect("capture succeeds");
    let path = decode(&captured)["path"].as_str().unwrap().to_owned();
    let slug = Path::new(&path)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let result = server
        .discard_inbox_item(Parameters(DiscardInboxItemInput { slug }))
        .await
        .expect("discard succeeds");

    let v = verification(&result);
    assert_eq!(v["verified"], "removed");
    assert_eq!(v["bytes_written"], 0);
    assert!(v["content_hash"].is_null(), "a removed file has no hash");
}

// ---------------------------------------------------------------------
// A write that did not land
// ---------------------------------------------------------------------

#[tokio::test]
async fn a_write_that_silently_vanished_is_an_error_not_a_success() {
    // `capture` writes under `inbox/`; the store accepts that write and
    // keeps nothing. Before verification this reported success with a
    // path pointing at a file that was never there.
    let server = server_over(Arc::new(SwallowingStore::new("inbox/")));
    let outcome = server
        .capture(Parameters(CaptureInput {
            text: "this will be swallowed".to_owned(),
        }))
        .await;

    let err = outcome.expect_err("an unverifiable write must not report success");
    let message = err.message.to_string();
    assert!(
        message.contains("could not be verified"),
        "the error must say the write is unverified, not merely that a file is missing: {message}"
    );
    assert!(
        message.contains("re-read"),
        "the error must tell the caller what to do next: {message}"
    );
}

#[tokio::test]
async fn a_swallowed_append_is_an_error_too() {
    // The append shape takes a different branch in the domain (the
    // daily note is scaffolded, then rewritten), so it gets its own
    // sabotage rather than trusting the `capture` case to cover it.
    let server = server_over(Arc::new(SwallowingStore::new("journal/")));
    let outcome = server
        .append_to_log(Parameters(AppendToLogInput {
            text: "into the void".to_owned(),
        }))
        .await;

    let err = outcome.expect_err("an unverifiable append must not report success");
    assert!(err.message.contains("could not be verified"), "{err:?}");
}

// --- the start verbs' write shapes (#568) -------------------------------
//
// Both choices are load-bearing and neither was pinned: swapping them
// left every operations, context, server AND verification test green.
// The failure is quiet rather than loud — `Rewritten` on `start_action`
// drops `appended_tail`, which is the only thing that shows an agent
// which log line actually landed, the stated purpose of #539.

/// A server whose vault already holds one active project with one open
/// bullet, so both start verbs have something to act on.
async fn server_with_a_started_bullet() -> CuadernoServer {
    let server = healthy_server();
    server
        .create_project(Parameters(CreateProjectInput {
            title: "Alpha".to_owned(),
            context: "work".to_owned(),
            core_question: None,
            vars: None,
        }))
        .await
        .expect("create_project");
    server
}

#[tokio::test]
async fn start_action_is_append_shaped_and_shows_the_log_line_that_landed() {
    let server = server_with_a_started_bullet().await;
    server
        .start_unplanned_action(Parameters(StartUnplannedActionInput {
            project: "alpha".to_owned(),
            title: "Draft methods".to_owned(),
            energy: "deep".to_owned(),
        }))
        .await
        .expect("seed a bullet to start");

    let result = server
        .start_action(Parameters(StartActionInput {
            project: "alpha".to_owned(),
            query: "Draft methods".to_owned(),
        }))
        .await
        .expect("start_action");

    let tail = verification(&result)["appended_tail"]
        .as_str()
        .expect("start_action is append-shaped, so it carries its tail")
        .to_owned();
    assert!(
        tail.contains("started [[alpha]]") && tail.contains("Draft methods (deep)"),
        "the tail must show the line that landed: {tail}"
    );
}

#[tokio::test]
async fn start_unplanned_action_is_rewrite_shaped_because_it_returns_the_map() {
    // Its `.primary` is the project map, not the daily note, so an
    // append shape would look for a tail in the wrong file.
    let server = server_with_a_started_bullet().await;
    let result = server
        .start_unplanned_action(Parameters(StartUnplannedActionInput {
            project: "alpha".to_owned(),
            title: "Fix the CI badge".to_owned(),
            energy: "light".to_owned(),
        }))
        .await
        .expect("start_unplanned_action");

    // NOTE, deliberately: this does NOT pin the WriteShape, and cannot.
    // `AppendedToSection` degrades to a null tail when the heading is
    // absent (verify.rs `section_tail`), and the project map has no
    // `## Logs`, so swapping this verb to the append shape produces a
    // byte-identical payload. Verified by mutation: the swap leaves this
    // whole target green, and nothing observable distinguishes them.
    //
    // The consequential direction IS pinned, by the sibling test above:
    // swapping `start_action` to `Rewritten` drops the tail that shows
    // an agent which log line landed, and that fails loudly. What is
    // asserted here is the payload a rewrite actually carries.
    let v = verification(&result);
    assert_eq!(v["verified"], "content", "a rewrite re-reads the file: {v}");
    assert!(
        v["bytes_written"].as_u64().is_some_and(|b| b > 0),
        "and reports the size it wrote: {v}"
    );
    assert!(
        v["content_hash"].as_str().is_some(),
        "and fingerprints it: {v}"
    );
    assert!(
        v.get("appended_tail").is_none() || v["appended_tail"].is_null(),
        "with no appended tail: {v}"
    );
    let payload = decode(&result);
    assert!(
        payload["path"]
            .as_str()
            .is_some_and(|p| p.starts_with("projects/")),
        "and reports the map it rewrote: {}",
        payload["path"]
    );
}
