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
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};
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
}

impl McpSubprocess {
    fn spawn(vault_root: &Path) -> Self {
        let bin = env!("CARGO_BIN_EXE_cdno-mcp");
        let mut child = Command::new(bin)
            .env("CUADERNO_VAULT_PATH", vault_root)
            // Quiet logs in tests; the binary writes to stderr so
            // RUST_LOG=off keeps test output clean.
            .env("RUST_LOG", "off")
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
        thread::spawn(move || {
            use std::io::Read;
            let mut sink = Vec::new();
            let mut s = stderr;
            let _ = s.read_to_end(&mut sink);
        });

        let rx = spawn_line_reader(stdout);

        Self {
            child,
            stdin: Some(stdin),
            rx,
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
        let deadline = Duration::from_secs(5);
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
/// timeout-bounded.
fn spawn_line_reader(stdout: ChildStdout) -> Receiver<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
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
        46,
        "expected the full catalogue: 45 prior + the list_note_types discovery read, got {}",
        tools.len()
    );
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
}
