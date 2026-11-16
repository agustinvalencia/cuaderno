//! Subprocess-based end-to-end tests for the `cdno-mcp` stdio
//! binary. Spawns the actual binary, speaks JSON-RPC at it through
//! stdin / stdout, and asserts on the responses.
//!
//! This is the protocol-correctness counterpart to the in-process
//! handler tests in `handlers_context.rs` / `handlers_operations.rs`.
//! Those exercise our handler bodies; these exercise rmcp's
//! initialisation handshake, the `tools/list` dispatch, the
//! `tools/call` round-trip, and our binary's lifecycle. Failures
//! here would catch a broken protocol surface that the in-process
//! tests can't see.
//!
//! Per `Cargo`'s integration-test conventions, `cargo test` already
//! builds the binary before running, and the path is in
//! `env!("CARGO_BIN_EXE_cdno-mcp")`.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use cdno_mcp::index_health::SWEEP_INTERVAL_ENV;
use serde_json::{Value, json};

#[path = "support/process.rs"]
mod process;
use process::exit_within_deadline;
use tempfile::TempDir;

/// Helper: spawn `cdno-mcp` against `vault_root` with stdin/stdout
/// piped, plus a stderr drain so the child doesn't block on a full
/// pipe.
///
/// `stdin` is held in an `Option` so `Drop` can explicitly take it
/// before waiting on the child. Closing the write end of the pipe
/// is what signals EOF to rmcp's stdio loop; without that the
/// service keeps blocking on stdin and `child.wait()` hangs
/// forever. Rust drops struct fields in declaration order
/// implicitly, but Drop runs *before* any field drops, so the
/// implicit close happens too late.
struct McpSubprocess {
    child: Child,
    stdin: Option<ChildStdin>,
    // Each line of stdout pushes onto this receiver; reader thread
    // owns the actual ChildStdout.
    rx: Receiver<String>,
    // Every line the child wrote to stdout so far, read or not.
    stdout_lines: Arc<Mutex<Vec<String>>>,
    // Everything the child wrote to stderr so far, collected by a drain
    // thread so the child never blocks on a full pipe.
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl McpSubprocess {
    fn spawn(vault_root: &Path) -> Self {
        Self::spawn_with_index(vault_root, None)
    }

    /// [`spawn`](Self::spawn) with `CUADERNO_INDEX_PATH` set to `index`,
    /// or removed when `None` so no test inherits the developer's value.
    fn spawn_with_index(vault_root: &Path, index: Option<&Path>) -> Self {
        Self::spawn_with_env(vault_root, index, &[])
    }

    /// [`spawn_with_index`](Self::spawn_with_index) with extra environment.
    /// The sweep interval variable is removed unless `env` sets it, so every
    /// other test runs on the default interval, which no test outlasts.
    fn spawn_with_env(vault_root: &Path, index: Option<&Path>, env: &[(&str, &str)]) -> Self {
        let bin = env!("CARGO_BIN_EXE_cdno-mcp");
        let mut cmd = Command::new(bin);
        match index {
            Some(path) => cmd.env(cdno_core::paths::INDEX_PATH_ENV, path),
            None => cmd.env_remove(cdno_core::paths::INDEX_PATH_ENV),
        };
        cmd.env_remove(SWEEP_INTERVAL_ENV)
            .env("CUADERNO_VAULT_PATH", vault_root)
            // Quiet logs in tests; the binary writes to stderr so
            // RUST_LOG=off keeps test output clean.
            .env("RUST_LOG", "off");
        for (key, value) in env {
            cmd.env(key, value);
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn cdno-mcp");

        let stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");
        let stderr = child.stderr.take().expect("piped stderr");

        // Drain stderr in the background so the child can write
        // unbounded log output without us blocking.
        let collected = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&collected);
        thread::spawn(move || {
            use std::io::Read;
            let mut s = stderr;
            let mut buf = [0u8; 4096];
            while let Ok(n) = s.read(&mut buf) {
                if n == 0 {
                    break;
                }
                sink.lock().unwrap().extend_from_slice(&buf[..n]);
            }
        });

        let stdout_lines = Arc::new(Mutex::new(Vec::new()));
        let rx = spawn_line_reader(stdout, Arc::clone(&stdout_lines));

        Self {
            child,
            stdin: Some(stdin),
            rx,
            stdout_lines,
            stderr: collected,
        }
    }

    /// What the child has written to stderr so far.
    fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr.lock().unwrap()).into_owned()
    }

    /// Close stdin, as a client that disconnects does, and wait at most
    /// `deadline` for the process to exit. Returns how long it took and how
    /// it exited; panics, after killing it, if it overstays.
    fn close_and_wait(&mut self, deadline: Duration) -> (Duration, std::process::ExitStatus) {
        drop(self.stdin.take());
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().expect("try_wait") {
                return (start.elapsed(), status);
            }
            if start.elapsed() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("cdno-mcp did not exit within {deadline:?} of stdin closing");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    /// Send one JSON-RPC message (one newline-terminated line).
    fn send(&mut self, msg: &Value) {
        let mut line = serde_json::to_string(msg).expect("serialise message");
        line.push('\n');
        let stdin = self.stdin.as_mut().expect("stdin already closed");
        stdin.write_all(line.as_bytes()).expect("write to stdin");
        stdin.flush().expect("flush stdin");
    }

    /// Read the next response with `id == expected_id`, with a 5s
    /// hard timeout to keep a hung subprocess from hanging CI.
    /// Notifications (no `id`) are skipped.
    fn read_response(&self, expected_id: u64) -> Value {
        self.read_response_within(expected_id, Duration::from_secs(5))
    }

    /// [`read_response`](Self::read_response) with its own timeout, for a
    /// reply that follows slow work at start (a checked open that rebuilds
    /// the index).
    fn read_response_within(&self, expected_id: u64, deadline: Duration) -> Value {
        loop {
            match self.rx.recv_timeout(deadline) {
                Ok(line) => {
                    let value: Value = serde_json::from_str(&line)
                        .unwrap_or_else(|e| panic!("non-JSON line `{line}`: {e}"));
                    if value.get("id").and_then(|v| v.as_u64()) == Some(expected_id) {
                        return value;
                    }
                    // Otherwise it's a notification or a stray
                    // response for a different id — loop to find ours.
                }
                Err(RecvTimeoutError::Timeout) => {
                    panic!("timed out waiting for response with id {expected_id}");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    panic!("subprocess closed stdout before responding to id {expected_id}");
                }
            }
        }
    }
}

impl Drop for McpSubprocess {
    fn drop(&mut self) {
        // Close stdin first so the rmcp service loop sees EOF and
        // exits on its own. If we just called child.wait() with
        // stdin still open, the subprocess would block forever
        // reading from a pipe whose write end nobody's closed.
        drop(self.stdin.take());

        // Poll for clean exit with a short deadline; kill if it
        // overstays. `try_wait` is non-blocking; we sleep a few ms
        // between checks. 500ms total is plenty for a stdio rmcp
        // service to drain after EOF.
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if std::time::Instant::now() >= deadline => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
                Ok(None) => thread::sleep(Duration::from_millis(20)),
                Err(_) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
            }
        }
    }
}

/// Spawn a background thread that pulls lines off `stdout` and
/// pushes them onto a channel. Lets the test's reads be
/// timeout-bounded. Every line is also kept in `transcript`, bytes that
/// are not UTF-8 included (lossily), so a test can inspect all of stdout
/// after the reads have consumed it.
fn spawn_line_reader(stdout: ChildStdout, transcript: Arc<Mutex<Vec<String>>>) -> Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let line = String::from_utf8_lossy(&buf)
                .trim_end_matches(['\n', '\r'])
                .to_owned();
            transcript.lock().unwrap().push(line.clone());
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    rx
}

/// Initialise a vault on disk so the spawned subprocess has
/// something to open. Uses cdno-cli's `init::run` directly rather
/// than shelling out — keeps test setup fast and dependency-free.
fn make_vault(dir: &Path) {
    cdno_cli::commands::init::run(dir).expect("cdno init");
}

/// The MCP initialisation dance: send `initialize`, read the
/// response, send the `initialized` notification. Returns the
/// `initialize` result for shape assertions.
fn initialise(mcp: &mut McpSubprocess) -> Value {
    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "cdno-mcp e2e", "version": "0.0" }
        }
    }));
    let init_response = mcp.read_response(1);
    mcp.send(&json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));
    init_response
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[test]
fn initialize_returns_server_info_and_tools_capability() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    let init = initialise(&mut mcp);

    let result = &init["result"];
    assert_eq!(result["serverInfo"]["name"], "cdno-mcp");
    assert!(
        result["capabilities"]["tools"].is_object(),
        "tools capability must be advertised: {init}"
    );
}

#[test]
fn tools_list_returns_all_advertised_tools() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    mcp.send(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }));
    let response = mcp.read_response(2);

    let tools = response["result"]["tools"].as_array().expect("tools array");
    assert_eq!(
        tools.len(),
        68,
        "expected the full catalogue advertised over stdio; tests/server.rs pins the names, got {}",
        tools.len()
    );
}

/// Pause, switch and resume over the real binary: three calls, and the
/// daily note's lines in the order they were made.
#[test]
fn pause_switch_resume_over_stdio() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    let mut id = 100;
    let mut call = |mcp: &mut McpSubprocess, name: &str, arguments: Value| -> Value {
        id += 1;
        mcp.send(&json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }));
        mcp.read_response(id)
    };

    let created = call(
        &mut mcp,
        "create_project",
        json!({ "title": "Surrogate Model", "context": "university" }),
    );
    let slug = slug_of(&created);
    for title in ["Draft methods section", "Run ablation"] {
        call(
            &mut mcp,
            "add_action",
            json!({ "project": slug, "title": title, "energy": "deep" }),
        );
    }
    call(
        &mut mcp,
        "start_action",
        json!({ "project": slug, "query": "Draft methods" }),
    );

    // 1. switch to the other bullet (pauses the first).
    let switched = call_payload(&call(
        &mut mcp,
        "switch_action",
        json!({ "project": slug, "query": "Run ablation", "next": "section 3" }),
    ));
    assert_eq!(switched["paused"]["action"], "Draft methods section (deep)");
    assert_eq!(switched["started"]["action"], "Run ablation (deep)");

    // 2. pause that one.
    let paused = call_payload(&call(&mut mcp, "pause_action", json!({})));
    assert_eq!(paused["paused"]["action"], "Run ablation (deep)");

    // 3. resume the latest pause.
    let resumed = call_payload(&call(&mut mcp, "resume_action", json!({})));
    assert_eq!(resumed["resumed"]["action"], "Run ablation (deep)");
    assert_eq!(resumed["resumed_from"]["kind"], "paused");

    let path = resumed["path"].as_str().expect("daily path");
    let body = std::fs::read_to_string(dir.path().join(path)).expect("daily note");
    let heads = [
        "started [[".to_owned() + &slug + "]] \u{2014} Draft methods section (deep)",
        "action paused on [[".to_owned() + &slug + "]] \u{2014} Draft methods section (deep)",
        "started [[".to_owned() + &slug + "]] \u{2014} Run ablation (deep)",
        "action paused on [[".to_owned() + &slug + "]] \u{2014} Run ablation (deep)",
        "resumed [[".to_owned() + &slug + "]] \u{2014} Run ablation (deep)",
    ];
    let mut from = 0;
    for head in &heads {
        let at = body[from..].find(head.as_str()).unwrap_or_else(|| {
            panic!("`{head}` missing or out of order after byte {from}:\n{body}")
        });
        from += at + head.len();
    }
}

/// A write result carries the focus over the real binary (RFC 0005 §5.5):
/// the start's own result, a log line written while it is open, and a
/// completion that closes it, which reports `focus: null`.
#[test]
fn focus_is_on_the_write_result_over_stdio() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    let mut id = 100;
    let mut call = |mcp: &mut McpSubprocess, name: &str, arguments: Value| -> Value {
        id += 1;
        mcp.send(&json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }));
        mcp.read_response(id)
    };

    let created = call(
        &mut mcp,
        "create_project",
        json!({ "title": "Surrogate Model", "context": "university" }),
    );
    let slug = slug_of(&created);
    call(
        &mut mcp,
        "add_action",
        json!({ "project": slug, "title": "Draft methods section", "energy": "deep" }),
    );

    let started = call_payload(&call(
        &mut mcp,
        "start_action",
        json!({ "project": slug, "query": "Draft methods" }),
    ));
    let focus = &started["focus"];
    assert_eq!(focus["project"], slug.as_str(), "{started}");
    assert_eq!(focus["action"], "Draft methods section (deep)", "{started}");
    assert_eq!(focus["carried"], false, "{started}");

    let logged = call_payload(&call(
        &mut mcp,
        "append_to_log",
        json!({ "text": "read a related paper" }),
    ));
    assert_eq!(&logged["focus"], focus, "{logged}");
    let current = call_payload(&call(&mut mcp, "current_focus", json!({})));
    assert_eq!(&current, focus, "the same as `current_focus`");

    let completed = call_payload(&call(
        &mut mcp,
        "complete_action",
        json!({ "project": slug, "query": "Draft methods" }),
    ));
    let closed = completed
        .as_object()
        .and_then(|o| o.get("focus"))
        .unwrap_or_else(|| panic!("`focus` present, null: {completed}"));
    assert!(closed.is_null(), "{completed}");
}

#[test]
fn tools_call_get_orientation_against_empty_vault_returns_empty_arrays() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get_orientation",
            "arguments": {}
        }
    }));
    let response = mcp.read_response(3);

    let content = response["result"]["content"]
        .as_array()
        .expect("content array");
    assert_eq!(content.len(), 1);
    let text = content[0]["text"].as_str().expect("text payload");
    let parsed: Value = serde_json::from_str(text).expect("JSON payload");
    assert!(parsed["commitments"].as_array().unwrap().is_empty());
    assert!(parsed["projects"].as_array().unwrap().is_empty());
    assert!(parsed["lapsed_habits"].as_array().unwrap().is_empty());
}

#[test]
fn tools_call_append_to_log_writes_into_today_daily() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "append_to_log",
            "arguments": { "text": "e2e: hello from the subprocess test" }
        }
    }));
    let response = mcp.read_response(4);
    let content = response["result"]["content"][0]["text"]
        .as_str()
        .expect("text payload");
    let parsed: Value = serde_json::from_str(content).expect("JSON payload");
    let path = parsed["path"].as_str().expect("path field");
    assert!(path.contains("journal/") && path.ends_with(".md"));

    // Drop the subprocess so it flushes / releases the SQLite file
    // before we read the on-disk artefact ourselves.
    drop(mcp);

    let body = std::fs::read_to_string(dir.path().join(path)).expect("read daily note");
    assert!(
        body.contains("e2e: hello from the subprocess test"),
        "daily note didn't pick up the appended log:\n{body}"
    );
}

#[test]
fn tools_call_with_unknown_tool_name_returns_jsonrpc_error() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "no_such_tool",
            "arguments": {}
        }
    }));
    let response = mcp.read_response(5);
    assert!(
        response.get("error").is_some(),
        "expected an `error` field on the response: {response}"
    );
    // The other half of the #560 split, and the pass-through branch of
    // `rejection::decode`: a MALFORMED CALL stays a protocol error. Only
    // errors raised inside a successful invocation become tool results,
    // so an unknown tool must not acquire a `result`.
    assert!(
        response.get("result").is_none(),
        "an unknown tool is a protocol error, not a tool result: {response}"
    );
}

// ---------------------------------------------------------------------
// Domain rejections on the wire (GH #560)
// ---------------------------------------------------------------------
//
// These are the cases the handler tests structurally cannot see. A
// rejection becomes a tool result in `CuadernoServer::call_tool`, which
// only runs when a call arrives through the router — a handler test calls
// the method body directly and still gets an `Err`. So the *client-visible
// shape* has no coverage anywhere but here.

/// Extract a created note's slug from a write result, rather than
/// assuming the slugifier's output. A test that hard-codes the slug
/// fails for the wrong reason the day slugification changes.
fn slug_of(write_result: &Value) -> String {
    let content = write_result["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("setup call returned no text payload: {write_result}"));
    let parsed: Value = serde_json::from_str(content).expect("JSON payload");
    let path = parsed["path"].as_str().expect("path field");
    Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .expect("slug from path")
        .to_owned()
}

/// Read the rejection payload out of a tool result, asserting the two
/// properties that #560 is about: it is **not** a JSON-RPC protocol
/// error, and it is flagged `isError` so a client renders it as a failure
/// rather than as a successful answer.
fn rejection_payload(response: &Value) -> Value {
    assert!(
        response.get("error").is_none(),
        "a caller-actionable rejection must not come back as a JSON-RPC protocol error \
         — that is the shape the client in #560 renders as a bare \"Tool execution failed\": \
         {response}"
    );
    assert_eq!(
        response["result"]["isError"],
        json!(true),
        "a rejection must set isError, or a client reads it as success: {response}"
    );
    let content = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("rejection carried no text payload: {response}"));
    serde_json::from_str(content).expect("rejection payload is JSON")
}

#[test]
fn a_domain_rejection_arrives_as_a_tool_result_carrying_its_message() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 10, "method": "tools/call",
        "params": { "name": "create_project", "arguments": {
            "title": "Surrogate Model", "context": "university"
        }}
    }));
    let created = mcp.read_response(10);
    let slug = slug_of(&created);

    // 501 characters: one past the documented default cap, which is the
    // exact boundary #560 established by binary search over the wire.
    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 11, "method": "tools/call",
        "params": { "name": "update_project_state", "arguments": {
            "project": slug, "new_state": "a".repeat(501)
        }}
    }));
    let payload = rejection_payload(&mcp.read_response(11));

    assert_eq!(payload["code"], json!("state_too_long"));
    assert_eq!(payload["details"]["chars"], json!(501));
    assert_eq!(payload["details"]["max"], json!(500));

    // The message that used to reach the model as nothing at all. Both
    // numbers asserted separately so half a break still fails.
    let message = payload["message"].as_str().expect("message field");
    assert!(
        message.contains("summarise"),
        "the advice that makes `reject` mode work is missing: {message}"
    );
    assert!(
        message.contains("501") && message.contains("500"),
        "the message must name both the actual length and the limit: {message}"
    );
}

#[test]
fn an_ambiguous_action_delivers_its_candidates_as_data_not_prose() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 20, "method": "tools/call",
        "params": { "name": "create_project", "arguments": {
            "title": "Ablation Study", "context": "university"
        }}
    }));
    let slug = slug_of(&mcp.read_response(20));

    for (id, title) in [
        (21, "Draft the methods section"),
        (22, "Revise the methods appendix"),
    ] {
        mcp.send(&json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "add_action", "arguments": {
                "project": slug, "title": title, "energy": "deep", "with_note": false
            }}
        }));
        let added = mcp.read_response(id);
        assert!(
            added.get("error").is_none() && added["result"]["isError"] != json!(true),
            "setup add_action failed: {added}"
        );
    }

    // "methods" matches both bullets.
    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 23, "method": "tools/call",
        "params": { "name": "complete_action", "arguments": {
            "project": slug, "query": "methods"
        }}
    }));
    let payload = rejection_payload(&mcp.read_response(23));

    assert_eq!(payload["code"], json!("ambiguous_action"));
    assert_eq!(payload["details"]["query"], json!("methods"));

    // The point of the whole change, and the prerequisite #602 is
    // blocked on: a railguard must be able to ACT on candidates. An
    // array of strings is actionable; the same list rendered into the
    // message is not.
    let candidates = payload["details"]["candidates"]
        .as_array()
        .unwrap_or_else(|| panic!("candidates must be a JSON array, got: {payload}"));
    assert_eq!(candidates.len(), 2, "both matching bullets: {payload}");
    let texts: Vec<&str> = candidates
        .iter()
        .map(|c| c.as_str().expect("candidate string"))
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("methods section"))
            && texts.iter().any(|t| t.contains("methods appendix")),
        "candidates should name both bullets verbatim: {texts:?}"
    );
}

#[test]
fn a_slug_that_matches_nothing_is_a_rejection_that_lists_the_real_ones() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 30, "method": "tools/call",
        "params": { "name": "create_project", "arguments": {
            "title": "Surrogate Model", "context": "university"
        }}
    }));
    let slug = slug_of(&mcp.read_response(30));

    // A typo, which is the commonest mistake an agent makes. The first
    // review of #560 caught this arriving as a -32603 because the domain
    // raises it as `StoreError::NotFound` — a core forward — even though
    // it hand-builds the message WITH the list of valid projects.
    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 31, "method": "tools/call",
        "params": { "name": "update_project_state", "arguments": {
            "project": "surrogat-model", "new_state": "Anything."
        }}
    }));
    let payload = rejection_payload(&mcp.read_response(31));

    assert_eq!(payload["code"], json!("not_found"));
    let message = payload["message"].as_str().expect("message field");
    assert!(
        message.contains(&slug),
        "the hint naming the real project must reach the client, or the agent \
         has nothing to correct towards — got: {message}"
    );
}

// ---------------------------------------------------------------------
// Read, then revise (RFC 0002 T9, #622)
// ---------------------------------------------------------------------

/// The parsed JSON payload of a successful `tools/call`.
fn call_payload(response: &Value) -> Value {
    assert_ne!(
        response["result"]["isError"],
        json!(true),
        "tool returned an error result: {response}"
    );
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_else(|| panic!("text payload: {response}"));
    serde_json::from_str(text).expect("JSON payload")
}

/// `read_note` over the wire, returning its payload.
fn read_concept(mcp: &mut McpSubprocess, id: u64) -> Value {
    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": "read_note",
            "arguments": { "note": "concept:woodbury-identity" }
        }
    }));
    call_payload(&mcp.read_response(id))
}

#[test]
fn read_note_then_revise_note_round_trips_through_the_content_hash() {
    let dir = TempDir::new().unwrap();
    // `cdno init` declares the `concept` type, so a concept is revisable.
    make_vault(dir.path());
    std::fs::create_dir_all(dir.path().join("concepts")).unwrap();
    std::fs::write(
        dir.path().join("concepts/woodbury-identity.md"),
        "---\ntype: concept\ncreated: 2026-09-01\n---\n\n# Woodbury identity\n\nFirst account.\n",
    )
    .unwrap();
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    let before = read_concept(&mut mcp, 10);
    let hash = before["content_hash"].as_str().expect("content_hash");

    let new_body = "\n# Woodbury identity\n\nSecond, sharper account.\n";
    mcp.send(&json!({
        "jsonrpc": "2.0",
        "id": 11,
        "method": "tools/call",
        "params": {
            "name": "revise_note",
            "arguments": {
                "note": "concepts/woodbury-identity",
                "expected_hash": hash,
                "body": new_body,
                "reason": "sharpened the account"
            }
        }
    }));
    let revised = call_payload(&mcp.read_response(11));
    assert_eq!(revised["changed"], json!(true), "{revised}");
    assert_eq!(revised["path"], "concepts/woodbury-identity.md");

    let after = read_concept(&mut mcp, 12);
    assert_eq!(after["body"], new_body, "{after}");
    assert_ne!(after["content_hash"], before["content_hash"]);
    assert_eq!(after["content_hash"], revised["new_hash"], "{revised}");
}

/// Filing a Typst evidence note over the wire, then reading it back: the
/// `format` field reaches the handler and `read_note` reports the format.
#[test]
fn file_to_portfolio_typst_then_read_note_over_stdio() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    let mut call = |id: u64, name: &str, arguments: Value| -> Value {
        mcp.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }));
        mcp.read_response(id)
    };

    call_payload(&call(
        30,
        "create_portfolio",
        json!({ "question": "Surrogate error budget" }),
    ));
    let body =
        "= Setting\n\nThe surrogate's error splits as in [[concepts/bias-variance-split]].\n";
    let filed = call_payload(&call(
        31,
        "file_to_portfolio",
        json!({
            "portfolio": "surrogate-error-budget",
            "source": "Error budget derivation",
            "origin": "projects/surrogate-model",
            "content": body,
            "format": "typst"
        }),
    ));
    let path = filed["path"].as_str().expect("path").to_owned();
    assert!(path.ends_with("-error-budget-derivation.typ"), "{filed}");

    let read = call_payload(&call(32, "read_note", json!({ "note": path })));
    assert_eq!(read["format"], "typst", "{read}");
    assert_eq!(read["body"].as_str().unwrap().trim_start(), body, "{read}");
}

/// Adopting a `.typ` with its folder over the wire: the result lists the rewrites.
#[test]
fn file_to_portfolio_adopts_a_typst_file_over_stdio() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let src = TempDir::new().unwrap();
    std::fs::write(src.path().join("deck.typ"), "#image(\"figs/a.png\")\n").unwrap();
    std::fs::create_dir(src.path().join("figs")).unwrap();
    std::fs::write(src.path().join("figs/a.png"), b"png").unwrap();
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    let mut call = |id: u64, name: &str, arguments: Value| -> Value {
        mcp.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }));
        mcp.read_response(id)
    };
    call_payload(&call(
        40,
        "create_portfolio",
        json!({ "question": "Surrogate error budget" }),
    ));
    let filed = call_payload(&call(
        41,
        "file_to_portfolio",
        json!({
            "portfolio": "surrogate-error-budget",
            "source": "Error budget derivation",
            "origin": "projects/surrogate-model",
            "format": "typst",
            "attach": src.path().join("deck.typ").to_string_lossy()
        }),
    ));
    let path = filed["path"].as_str().expect("path").to_owned();
    assert_eq!(filed["rewrites"].as_array().unwrap().len(), 1, "{filed}");
    let stem = path.strip_suffix(".typ").unwrap();
    assert!(dir.path().join(stem).join("figs/a.png").is_file());
}

/// `rename_note` over the wire: preview, write the plan that was shown, then
/// the old name still resolves (RFC 0007 §6.4, T26).
#[test]
fn rename_note_over_stdio() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    std::fs::write(
        dir.path().join("projects/surrogate-model.md"),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n\n## Milestones\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("inbox/2026-05-08-link.md"),
        "---\ntype: inbox\ncreated: 2026-05-08\n---\n\nSee [[surrogate-model]].\n",
    )
    .unwrap();
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    let mut call = |id: u64, name: &str, arguments: Value| -> Value {
        mcp.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": { "name": name, "arguments": arguments }
        }));
        mcp.read_response(id)
    };

    let preview = call_payload(&call(
        20,
        "rename_note",
        json!({ "note": "surrogate-model", "to": "emulator-study", "dry_run": true }),
    ));
    assert_eq!(preview["written"], json!(false), "{preview}");
    assert!(dir.path().join("projects/surrogate-model.md").exists());
    let hash = preview["plan_hash"].as_str().expect("plan_hash").to_owned();

    // A plan that moved is a rejection the agent can read, not a transport error.
    let stale = call(
        21,
        "rename_note",
        json!({ "note": "surrogate-model", "to": "emulator-study", "expected_plan": "stale" }),
    );
    assert_eq!(stale["result"]["isError"], json!(true), "{stale}");
    let rejection: Value =
        serde_json::from_str(stale["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(rejection["code"], "stale_plan", "{rejection}");
    assert_eq!(rejection["details"]["actual"], json!(hash));

    let written = call_payload(&call(
        22,
        "rename_note",
        json!({ "note": "surrogate-model", "to": "emulator-study", "expected_plan": hash }),
    ));
    assert_eq!(written["written"], json!(true), "{written}");
    assert!(dir.path().join("projects/emulator-study.md").exists());
    assert!(!dir.path().join("projects/surrogate-model.md").exists());

    let read = call_payload(&call(23, "read_note", json!({ "note": "surrogate-model" })));
    assert_eq!(read["path"], "projects/emulator-study.md", "{read}");
    assert_eq!(read["found_by_former_name"], "surrogate-model", "{read}");
    let inbox = std::fs::read_to_string(dir.path().join("inbox/2026-05-08-link.md")).unwrap();
    assert!(inbox.contains("[[emulator-study]]"), "{inbox}");
}

/// The binary hands the vault root to the server: a merge left paused in
/// the vault's git repository refuses the rename, a preview included, with
/// a code the agent can read. A server with no root would skip the check.
#[test]
fn rename_note_over_stdio_is_refused_while_a_merge_is_paused() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    std::fs::write(
        dir.path().join("projects/surrogate-model.md"),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n\n## Milestones\n",
    )
    .unwrap();
    let init = std::process::Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .args(["init", "-q"])
        .output()
        .expect("git init");
    assert!(init.status.success());
    std::fs::write(dir.path().join(".git/MERGE_HEAD"), "0".repeat(40) + "\n").unwrap();
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);

    for (id, dry_run) in [(30u64, true), (31, false)] {
        mcp.send(&json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "rename_note", "arguments": {
                "note": "surrogate-model", "to": "emulator-study",
                "dry_run": dry_run, "skip_sync_check": true
            } }
        }));
        let response = mcp.read_response(id);
        assert_eq!(response["result"]["isError"], json!(true), "{response}");
        let rejection: Value =
            serde_json::from_str(response["result"]["content"][0]["text"].as_str().unwrap())
                .unwrap();
        assert_eq!(
            rejection["code"], "git_operation_in_progress",
            "{rejection}"
        );
    }
    assert!(dir.path().join("projects/surrogate-model.md").exists());
}

// ---------------------------------------------------------------------
// CUADERNO_INDEX_PATH (#998)
// ---------------------------------------------------------------------

/// Names under `<vault>/.cuaderno/` that belong to an index: the database
/// and any sidecar or sibling file of it.
fn index_files_in_meta_dir(vault: &Path) -> Vec<String> {
    std::fs::read_dir(vault.join(cdno_core::paths::CUADERNO_DIR))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("index"))
        .collect()
}

/// With the variable set, the stdio server keeps its index at that file,
/// creates nothing for it under `.cuaderno/`, and its index-backed tools
/// (a write, then a full-text search over it) work.
#[test]
fn the_index_path_variable_moves_the_stdio_servers_index() {
    let dir = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let index = elsewhere.path().join("cache").join("index.db");
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn_with_index(dir.path(), Some(&index));
    initialise(&mut mcp);

    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "create_project",
                    "arguments": { "title": "Surrogate model", "context": "university" } }
    }));
    call_payload(&mcp.read_response(2));
    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": { "name": "search_notes", "arguments": { "query": "surrogate" } }
    }));
    let found = mcp.read_response(3);
    assert!(
        found.to_string().contains("surrogate-model"),
        "the search must find the project through the relocated index: {found}"
    );
    drop(mcp);

    assert!(index.is_file(), "the index lives at the override");
    assert_eq!(index_files_in_meta_dir(dir.path()), Vec::<String>::new());
}

/// The self-repair of #526 at an overridden index, end to end: the real
/// binary finds the damage at start, repairs the file in place beside the
/// override, reports it through `index_status`, and leaves the file bound
/// to this vault with nothing of the index under `.cuaderno/`.
#[test]
fn the_stdio_server_repairs_a_damaged_overridden_index_in_place() {
    let dir = TempDir::new().unwrap();
    let elsewhere = TempDir::new().unwrap();
    let index = elsewhere.path().join("surrogate-model.db");
    make_vault(dir.path());
    // Enough notes that the index spans many pages, so its last page is
    // data and damaging it spares the header, the schema and the binding.
    let padding = "Surrogate model notes. ".repeat(30);
    for day in 1..=28 {
        for month in 1..=3 {
            let folder = dir.path().join("journal/2025/daily");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(
                folder.join(format!("2025-{month:02}-{day:02}.md")),
                format!("---\ntype: daily\n---\n# 2025-{month:02}-{day:02}\n\n## Logs\n\n- **09:00**: {padding}\n"),
            )
            .unwrap();
        }
    }
    drop(
        cdno_mcp::bootstrap::open_vault_with_index(dir.path(), Some(index.as_os_str()))
            .expect("first open builds the overridden index"),
    );
    let page_size: usize = rusqlite::Connection::open(&index)
        .unwrap()
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .unwrap();
    let mut bytes = std::fs::read(&index).unwrap();
    let start = bytes.len() - page_size;
    bytes[start..].fill(0xA5);
    std::fs::write(&index, bytes).unwrap();

    let mut mcp = McpSubprocess::spawn_with_index(dir.path(), Some(&index));
    // The server rebuilds the index before it answers `initialize`, which
    // can take far longer than a normal reply on a loaded runner.
    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "cdno-mcp e2e", "version": "0.0" }
        }
    }));
    mcp.read_response_within(1, Duration::from_secs(120));
    mcp.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "index_status", "arguments": {} }
    }));
    let status = call_payload(&mcp.read_response(2));
    drop(mcp);

    assert_eq!(status["healthy"], json!(true), "{status}");
    assert_eq!(status["index_path"], json!(index.display().to_string()));
    assert!(
        status["damage_at_open"]
            .as_array()
            .is_some_and(|d| !d.is_empty()),
        "the damage found at start is reported: {status}"
    );
    let mut previous = index.clone().into_os_string();
    previous.push(cdno_core::index::PREVIOUS_SUFFIX);
    assert!(
        Path::new(&previous).is_file(),
        "the damaged copy sits beside the override"
    );
    assert_eq!(
        cdno_core::index::SqliteIndex::bound_vault_root(&index).unwrap(),
        Some(cdno_core::index::canonical_vault_root(dir.path())),
        "the repaired index is still bound to this vault"
    );
    assert_eq!(index_files_in_meta_dir(dir.path()), Vec::<String>::new());
}

/// A relative value stops the stdio server at start, before anything is
/// opened or created.
#[test]
fn the_stdio_server_refuses_a_relative_index_path() {
    let dir = TempDir::new().unwrap();
    let cwd = TempDir::new().unwrap();
    make_vault(dir.path());
    let (status, stderr) = exit_within_deadline(
        Command::new(env!("CARGO_BIN_EXE_cdno-mcp"))
            .current_dir(cwd.path())
            .env("CUADERNO_VAULT_PATH", dir.path())
            .env(cdno_core::paths::INDEX_PATH_ENV, "cache/index.db")
            .env("RUST_LOG", "off"),
    );
    assert!(!status.success(), "a relative index path must refuse");
    assert!(
        stderr.contains("CUADERNO_INDEX_PATH") && stderr.contains("absolute"),
        "{stderr}"
    );
    assert!(!cwd.path().join("cache").exists());
    assert_eq!(index_files_in_meta_dir(dir.path()), Vec::<String>::new());
}

// ---------------------------------------------------------------------
// The periodic sweep (#1002)
// ---------------------------------------------------------------------

/// An inbox note with `body`, at `inbox/<stem>.md`.
fn write_inbox_note(vault: &Path, stem: &str, body: &str) {
    std::fs::write(
        vault.join("inbox").join(format!("{stem}.md")),
        format!("---\ntype: inbox\ncreated: 2026-05-08\n---\n\n{body}\n"),
    )
    .unwrap();
}

/// One `tools/call`, returning the whole response.
fn call_tool(mcp: &mut McpSubprocess, id: u64, name: &str, arguments: Value) -> Value {
    mcp.send(&json!({
        "jsonrpc": "2.0", "id": id, "method": "tools/call",
        "params": { "name": name, "arguments": arguments }
    }));
    mcp.read_response(id)
}

/// Whether `search_notes` for `word` names the note `stem`.
fn search_finds(mcp: &mut McpSubprocess, id: u64, word: &str, stem: &str) -> bool {
    call_tool(mcp, id, "search_notes", json!({ "query": word }))
        .to_string()
        .contains(stem)
}

/// Poll `check` every 200ms until it holds or `deadline` passes, so a test
/// waits only as long as the sweep takes and never longer than the bound.
fn eventually(deadline: Duration, mut check: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    loop {
        if check() {
            return true;
        }
        if start.elapsed() >= deadline {
            return false;
        }
        thread::sleep(Duration::from_millis(200));
    }
}

/// Long enough for a one-second sweep to have run on a loaded runner, short
/// enough that a sweep that never runs fails in seconds.
const SWEEP_DEADLINE: Duration = Duration::from_secs(15);

/// The acceptance test of #1002: a note created, a note deleted and a note
/// edited on disk under a running stdio server all reach `search_notes` and
/// `lint` through the sweep, with no tool call that writes.
#[test]
fn the_stdio_server_sweeps_out_of_band_changes_into_the_index() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    write_inbox_note(
        dir.path(),
        "2026-05-08-edited",
        "Surrogate model calibration.",
    );
    write_inbox_note(dir.path(), "2026-05-08-deleted", "Marmalade residuals.");
    let mut mcp = McpSubprocess::spawn_with_env(dir.path(), None, &[(SWEEP_INTERVAL_ENV, "1")]);
    initialise(&mut mcp);

    // What the index holds at open.
    assert!(search_finds(&mut mcp, 2, "marmalade", "2026-05-08-deleted"));
    assert!(!search_finds(&mut mcp, 3, "zephyr", "2026-05-09-created"));
    assert!(!search_finds(&mut mcp, 4, "quinoa", "2026-05-08-edited"));

    // Out of band: one note arrives, one goes, one is edited. The edit's
    // mtime is moved on explicitly so it differs from the indexed one even
    // on a filesystem with coarse timestamps.
    write_inbox_note(dir.path(), "2026-05-09-created", "Zephyr ablation.");
    std::fs::remove_file(dir.path().join("inbox/2026-05-08-deleted.md")).unwrap();
    write_inbox_note(
        dir.path(),
        "2026-05-08-edited",
        "Quinoa sweep, recalibrated.",
    );
    std::fs::File::options()
        .write(true)
        .open(dir.path().join("inbox/2026-05-08-edited.md"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(10))
        .unwrap();

    let mut id = 10;
    let checks: [(&str, &str, bool, &str); 3] = [
        (
            "zephyr",
            "2026-05-09-created",
            true,
            "a note created on disk was never indexed",
        ),
        (
            "marmalade",
            "2026-05-08-deleted",
            false,
            "a note deleted on disk stayed searchable",
        ),
        (
            "quinoa",
            "2026-05-08-edited",
            true,
            "a note edited on disk was never reindexed",
        ),
    ];
    for (word, stem, present, failure) in checks {
        assert!(
            eventually(SWEEP_DEADLINE, || {
                id += 1;
                search_finds(&mut mcp, id, word, stem) == present
            }),
            "{failure}"
        );
    }
    // The fault the issue names: a deleted note's rows make `lint` report
    // a note it cannot read. After the sweep it names nothing of it.
    let lint = call_payload(&call_tool(&mut mcp, 100, "lint", json!({})));
    assert!(
        !lint.to_string().contains("2026-05-08-deleted"),
        "lint still reports the deleted note: {lint}"
    );
    let status = call_payload(&call_tool(&mut mcp, 101, "index_status", json!({})));
    assert!(status["last_sweep"].is_object(), "{status}");
    assert_eq!(status["healthy"], json!(true), "{status}");
}

/// `0` turns the sweep off: a note created on disk stays out of the index
/// over several would-be intervals, and no sweep is ever recorded.
#[test]
fn a_zero_interval_disables_the_stdio_sweep() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn_with_env(dir.path(), None, &[(SWEEP_INTERVAL_ENV, "0")]);
    initialise(&mut mcp);

    write_inbox_note(dir.path(), "2026-05-09-created", "Zephyr ablation.");
    thread::sleep(Duration::from_secs(3));
    assert!(!search_finds(&mut mcp, 2, "zephyr", "2026-05-09-created"));
    let status = call_payload(&call_tool(&mut mcp, 3, "index_status", json!({})));
    assert!(status["last_sweep"].is_null(), "{status}");
}

/// A value that is not a number of seconds stops the server at start,
/// naming the variable, rather than running on the default.
#[test]
fn the_stdio_server_refuses_a_malformed_sweep_interval() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let (status, stderr) = exit_within_deadline(
        Command::new(env!("CARGO_BIN_EXE_cdno-mcp"))
            .env("CUADERNO_VAULT_PATH", dir.path())
            .env_remove(cdno_core::paths::INDEX_PATH_ENV)
            .env(SWEEP_INTERVAL_ENV, "5m")
            .env("RUST_LOG", "off"),
    );
    assert!(!status.success(), "a malformed interval must refuse");
    assert!(stderr.contains(SWEEP_INTERVAL_ENV), "{stderr}");
}

/// Stdout is the JSON-RPC channel. With every log level on and sweeps that
/// apply changes and warn about a note that does not parse, every line on
/// stdout is still a JSON-RPC message, and the sweeps' logs are on stderr.
#[test]
fn the_stdio_sweep_writes_nothing_but_json_rpc_to_stdout() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn_with_env(
        dir.path(),
        None,
        &[(SWEEP_INTERVAL_ENV, "1"), ("RUST_LOG", "trace")],
    );
    initialise(&mut mcp);

    // A note that never parses makes every sweep warn, naming it; new notes
    // make sweeps apply changes (logged in the same warning while the broken
    // note is there).
    std::fs::write(
        dir.path().join("inbox/2026-05-08-broken.md"),
        "---\ntype: [unclosed\n---\n",
    )
    .unwrap();
    for n in 0..3 {
        write_inbox_note(dir.path(), &format!("2026-05-1{n}-arrived"), "Zephyr.");
        thread::sleep(Duration::from_millis(1200));
    }
    assert!(
        eventually(SWEEP_DEADLINE, || {
            let stderr = mcp.stderr_text();
            stderr.contains("periodic reconciliation could not index some notes")
                && stderr.contains("2026-05-08-broken")
        }),
        "no sweep logged the broken note to stderr: {}",
        mcp.stderr_text()
    );
    assert!(
        eventually(SWEEP_DEADLINE, || search_finds(
            &mut mcp,
            2,
            "zephyr",
            "2026-05-12-arrived"
        )),
        "the sweeps never applied the new notes"
    );
    call_payload(&call_tool(&mut mcp, 3, "get_orientation", json!({})));
    let (_, status) = mcp.close_and_wait(Duration::from_secs(15));
    assert!(status.success(), "{status:?}");

    // The reader thread disconnects the channel at the child's EOF, by
    // which point the transcript holds every line stdout carried.
    while mcp.rx.recv_timeout(Duration::from_secs(5)).is_ok() {}
    let lines = mcp.stdout_lines.lock().unwrap().clone();
    assert!(
        lines.len() >= 3,
        "initialize, a search and get_orientation at least: {lines:?}"
    );
    for line in &lines {
        let value: Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("a non-JSON line on stdout `{line}`: {e}"));
        assert_eq!(value["jsonrpc"], "2.0", "not a JSON-RPC message: {line}");
    }
}

/// A client that disconnects while no sweep runs gets a prompt exit, with
/// the sweep loop on its default interval.
#[test]
fn the_stdio_server_exits_promptly_when_stdin_closes_between_sweeps() {
    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn(dir.path());
    initialise(&mut mcp);
    call_payload(&call_tool(&mut mcp, 2, "get_orientation", json!({})));

    let (took, status) = mcp.close_and_wait(Duration::from_secs(10));
    assert!(status.success(), "{status:?}");
    // Normally a few milliseconds. The bound is the sweep grace: an idle
    // server must not spend it, and a runner slowed by instrumentation
    // still exits well inside it.
    assert!(
        took < Duration::from_secs(5),
        "exit took {took:?} with nothing running"
    );
}

/// A client that disconnects while a sweep runs gets an exit within the
/// shutdown grace (5s), not when the sweep ends. The sweep is held running
/// deterministically: this test holds the vault write lock, so the sweep's
/// first new note waits out the 5s lock timeout, and each of the eight
/// after it waits again, some 45s in all.
#[test]
fn the_stdio_server_exits_within_the_grace_when_stdin_closes_during_a_sweep() {
    use cdno_core::store::{FsVaultStore, VaultStore};

    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    let mut mcp = McpSubprocess::spawn_with_env(dir.path(), None, &[(SWEEP_INTERVAL_ENV, "1")]);
    initialise(&mut mcp);

    let store = FsVaultStore::new(dir.path());
    let lock = store.acquire_write_lock().expect("the vault lock");
    for n in 0..9 {
        write_inbox_note(dir.path(), &format!("2026-05-1{n}-arrived"), "Zephyr.");
    }
    // Past the next one-second tick, so the sweep is running and waiting
    // on the lock when stdin closes.
    thread::sleep(Duration::from_millis(2500));

    let (took, status) = mcp.close_and_wait(Duration::from_secs(30));
    drop(lock);
    assert!(status.success(), "{status:?}");
    // No tool call is in flight, so rmcp's drain and the tool-call drain
    // are immediate and the exit is the 5s grace: at least most of it
    // (the sweep was holding the exit), and far less than the sweep's ~40s.
    assert!(
        took >= Duration::from_secs(4),
        "exit took {took:?}: the grace was not exercised, so no sweep was running"
    );
    assert!(
        took < Duration::from_secs(9),
        "exit took {took:?}; the grace is 5s and the sweep had ~40s left"
    );
}

/// A write still running when the client disconnects completes, every file
/// of it and its index rows, before the process exits (#1002). The write is
/// a rename that rewrites references in three notes, made slow
/// deterministically: `git` on the server's PATH is a wrapper that sleeps
/// 1.5s per call (under the 2s probe timeout), and the rename probes git
/// four times before taking the lock and four again under it, so the call
/// runs some 12s after stdin closes: past rmcp's 5s drain and the 5s sweep
/// grace together, which is what used to cut it.
#[cfg(unix)]
#[test]
fn a_write_in_flight_at_disconnect_completes_before_the_stdio_server_exits() {
    use std::os::unix::fs::PermissionsExt;

    let real_git = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join("git"))
        .find(|path| path.is_file())
        .expect("git on PATH");
    let fake_bin = TempDir::new().unwrap();
    let wrapper = fake_bin.path().join("git");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nsleep 1.5\nexec '{}' \"$@\"\n",
            real_git.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(fake_bin.path().to_path_buf()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();

    let dir = TempDir::new().unwrap();
    make_vault(dir.path());
    std::fs::write(
        dir.path().join("projects/surrogate-model.md"),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n\n## Milestones\n",
    )
    .unwrap();
    let linking = ["2026-05-08-one", "2026-05-08-two", "2026-05-08-three"];
    for stem in linking {
        write_inbox_note(dir.path(), stem, "See [[surrogate-model]].");
    }
    let init = Command::new(&real_git)
        .arg("-C")
        .arg(dir.path())
        .args(["init", "-q"])
        .output()
        .expect("git init");
    assert!(init.status.success());

    let path = path.to_string_lossy().into_owned();
    let mut mcp = McpSubprocess::spawn_with_env(dir.path(), None, &[("PATH", &path)]);
    initialise(&mut mcp);
    mcp.send(&json!({
        "jsonrpc": "2.0", "id": 2, "method": "tools/call",
        "params": { "name": "rename_note", "arguments": {
            "note": "surrogate-model", "to": "emulator-study"
        } }
    }));
    // Let rmcp read the request before the input ends.
    thread::sleep(Duration::from_millis(300));
    let (took, status) = mcp.close_and_wait(Duration::from_secs(90));
    assert!(status.success(), "{status:?}");
    assert!(
        took > Duration::from_secs(10),
        "exit took {took:?}: the rename was meant to outlast rmcp's drain and the grace"
    );

    // Every file of the rename landed.
    assert!(dir.path().join("projects/emulator-study.md").is_file());
    assert!(!dir.path().join("projects/surrogate-model.md").exists());
    for stem in linking {
        let body = std::fs::read_to_string(dir.path().join(format!("inbox/{stem}.md"))).unwrap();
        assert!(body.contains("[[emulator-study]]"), "{stem}: {body}");
    }
    let today = chrono::Local::now().date_naive();
    let daily = dir
        .path()
        .join(format!("journal/{}/daily/{today}.md", today.format("%Y")));
    let log = std::fs::read_to_string(&daily).unwrap_or_default();
    assert!(log.contains("renamed [[emulator-study]]"), "{log}");

    // And its index update: the rows already name the new path, read
    // straight from the file, with no open that would reconcile first.
    let index = rusqlite::Connection::open_with_flags(
        dir.path().join(cdno_core::paths::INDEX_DB),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let paths: Vec<String> = index
        .prepare("SELECT path FROM notes WHERE path LIKE 'projects/%'")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(paths, vec!["projects/emulator-study.md".to_owned()]);
}
