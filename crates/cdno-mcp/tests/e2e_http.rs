//! Subprocess-based end-to-end tests for the `cdno-mcp-server`
//! Streamable HTTP binary (GH #60/#61). The HTTP counterpart of
//! `e2e_stdio.rs`: spawns the actual binary, speaks MCP JSON-RPC at
//! it over HTTP, and asserts on the responses.
//!
//! Covers the surface the stdio tests can't: the stateless
//! Streamable HTTP framing, the `--smoke` / `--read-only` modes, the
//! non-loopback safety interlock, and the periodic index
//! reconciliation that keeps a long-running server honest about
//! out-of-band edits.

use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tempfile::TempDir;

/// Spawn `cdno-mcp-server` with the given extra args, wait until the
/// port accepts connections, and hand back the child (killed on drop).
struct HttpServer {
    child: Child,
    port: u16,
}

impl HttpServer {
    fn spawn(vault_root: Option<&Path>, extra_args: &[&str]) -> Self {
        let port = free_port();
        let bin = env!("CARGO_BIN_EXE_cdno-mcp-server");
        let mut cmd = Command::new(bin);
        cmd.arg("--bind")
            .arg(format!("127.0.0.1:{port}"))
            .args(extra_args)
            .env("RUST_LOG", "off")
            // Never inherit a vault from the invoking environment —
            // the smoke test asserts the binary works with NO vault.
            .env_remove("CUADERNO_VAULT_PATH")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(root) = vault_root {
            cmd.env("CUADERNO_VAULT_PATH", root);
        }
        let child = cmd.spawn().expect("spawn cdno-mcp-server");

        // Readiness: poll until the listener accepts. The binary
        // reconciles the vault at open, so allow a generous deadline.
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "cdno-mcp-server did not start listening on port {port} within 15s"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        Self { child, port }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Ask the OS for a free port, then release it for the child to take.
/// (Small race by construction; fine at test scale.)
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind :0");
    listener.local_addr().expect("local addr").port()
}

/// Seed a vault the same way the stdio e2e does.
fn make_vault(dir: &Path) {
    cdno_cli::commands::init::run(dir).expect("cdno init");
}

/// POST one JSON-RPC message to `/mcp`. The Streamable HTTP spec
/// requires the client to accept both `application/json` and
/// `text/event-stream`; in the server's stateless JSON mode the
/// response body is plain JSON.
async fn post_mcp(client: &reqwest::Client, port: u16, body: &Value) -> (u16, Value) {
    let resp = client
        .post(format!("http://127.0.0.1:{port}/mcp"))
        .header("Accept", "application/json, text/event-stream")
        .json(body)
        .send()
        .await
        .expect("POST /mcp");
    let status = resp.status().as_u16();
    let text = resp.text().await.expect("response body");
    let value = if text.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("non-JSON body `{text}`: {e}"))
    };
    (status, value)
}

/// `tools/list` and return the sorted tool names.
async fn list_tool_names(client: &reqwest::Client, port: u16) -> Vec<String> {
    let (status, resp) = post_mcp(
        client,
        port,
        &json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/list" }),
    )
    .await;
    assert_eq!(status, 200, "tools/list should succeed: {resp}");
    let mut names: Vec<String> = resp["result"]["tools"]
        .as_array()
        .unwrap_or_else(|| panic!("tools/list shape: {resp}"))
        .iter()
        .map(|t| t["name"].as_str().expect("tool name").to_string())
        .collect();
    names.sort();
    names
}

// ---------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------

#[tokio::test]
async fn full_catalogue_and_tool_call_over_http() {
    let dir = TempDir::new().expect("tempdir");
    make_vault(dir.path());
    let server = HttpServer::spawn(Some(dir.path()), &[]);
    let client = reqwest::Client::new();

    let names = list_tool_names(&client, server.port).await;
    // Same pin as e2e_stdio.rs / tests/server.rs: the HTTP transport
    // must serve the identical catalogue, not a subset that happens
    // to look plausible.
    assert_eq!(
        names.len(),
        46,
        "HTTP catalogue diverged from the stdio pin: {names:?}"
    );
    assert!(names.iter().any(|n| n == "get_orientation"), "{names:?}");
    assert!(names.iter().any(|n| n == "append_to_log"), "{names:?}");

    // A real tools/call round-trip through the vault.
    let (status, resp) = post_mcp(
        &client,
        server.port,
        &json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "get_orientation", "arguments": {} }
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(resp["result"]["isError"], json!(false), "{resp}");
}

#[tokio::test]
async fn read_only_mode_hides_every_mutating_tool() {
    let dir = TempDir::new().expect("tempdir");
    make_vault(dir.path());
    let server = HttpServer::spawn(Some(dir.path()), &["--read-only"]);
    let client = reqwest::Client::new();

    let names = list_tool_names(&client, server.port).await;
    // The read-only surface is exactly the 16 context-router tools.
    assert_eq!(names.len(), 16, "read-only catalogue drifted: {names:?}");
    assert!(names.iter().any(|n| n == "get_orientation"), "{names:?}");
    assert!(names.iter().any(|n| n == "search_notes"), "{names:?}");
    for mutating in [
        "append_to_log",
        "capture",
        "create_project",
        "update_project_state",
        "park_project",
        "file_to_portfolio",
    ] {
        assert!(
            !names.iter().any(|n| n == mutating),
            "read-only catalogue must not advertise `{mutating}`: {names:?}"
        );
    }

    // Hidden means undispatchable, not merely unadvertised: calling a
    // mutating tool by name must be rejected by the router (rmcp maps
    // an unknown tool to an invalid-params JSON-RPC error).
    let (status, resp) = post_mcp(
        &client,
        server.port,
        &json!({
            "jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": { "name": "append_to_log", "arguments": { "text": "must not land" } }
        }),
    )
    .await;
    assert_eq!(status, 200, "JSON-RPC errors still ride a 200: {resp}");
    assert!(
        resp.get("error").is_some(),
        "calling a mutating tool in read-only mode must error: {resp}"
    );
}

#[tokio::test]
async fn smoke_mode_serves_echo_and_never_opens_a_vault() {
    // Deliberately NO vault anywhere near the process: an empty
    // tempdir is the cwd and CUADERNO_VAULT_PATH is unset. If smoke
    // mode ever tried to open a vault this would fail to start.
    let server = HttpServer::spawn(None, &["--smoke"]);
    let client = reqwest::Client::new();

    let names = list_tool_names(&client, server.port).await;
    assert_eq!(names, vec!["echo".to_string()]);

    let (status, resp) = post_mcp(
        &client,
        server.port,
        &json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": { "name": "echo", "arguments": { "message": "auth pipeline live" } }
        }),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        resp["result"]["content"][0]["text"],
        json!("auth pipeline live"),
        "{resp}"
    );
}

/// Send a raw HTTP/1.1 request with full control over the `Host`
/// header (HTTP clients normally overwrite it), returning the status
/// line. Used for the DNS-rebinding-protection assertions.
fn raw_request(port: u16, method: &str, host_header: &str, body: Option<&str>) -> String {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let body = body.unwrap_or("");
    let req = format!(
        "{method} /mcp HTTP/1.1\r\nHost: {host_header}\r\n\
         Accept: application/json, text/event-stream\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(req.as_bytes()).expect("write request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response.lines().next().unwrap_or("").to_string()
}

const TOOLS_LIST: &str = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;

#[tokio::test]
async fn foreign_host_header_is_rejected_unless_allowed() {
    // Default allowlist (loopback names only): a rebound hostname is
    // refused — this is rmcp's DNS-rebinding protection doing its job.
    let server = HttpServer::spawn(None, &["--smoke"]);
    let status = raw_request(server.port, "POST", "evil.example:8787", Some(TOOLS_LIST));
    assert!(
        status.contains("403"),
        "foreign Host must be rejected by default: {status}"
    );

    // The same hostname passed via --allowed-host is admitted —
    // extending, not replacing, the loopback defaults.
    let allowed = HttpServer::spawn(None, &["--smoke", "--allowed-host", "evil.example:8787"]);
    let status = raw_request(allowed.port, "POST", "evil.example:8787", Some(TOOLS_LIST));
    assert!(
        status.contains("200"),
        "--allowed-host must admit the listed hostname: {status}"
    );
    // ... and loopback still works (extend semantics).
    let status = raw_request(
        allowed.port,
        "POST",
        &format!("127.0.0.1:{}", allowed.port),
        Some(TOOLS_LIST),
    );
    assert!(
        status.contains("200"),
        "loopback must stay allowed: {status}"
    );
}

#[tokio::test]
async fn get_and_delete_are_rejected_in_stateless_mode() {
    // No sessions exist in stateless mode: GET (SSE resume) and
    // DELETE (session teardown) must be 405, not a hang or panic.
    let server = HttpServer::spawn(None, &["--smoke"]);
    for method in ["GET", "DELETE"] {
        let status = raw_request(server.port, method, "127.0.0.1", None);
        assert!(
            status.contains("405"),
            "{method} must be 405 in stateless mode: {status}"
        );
    }
}

#[test]
fn refuses_non_loopback_bind_without_origin_auth() {
    // No readiness loop here — the process must exit, fast, with an
    // error explaining the interlock.
    let bin = env!("CARGO_BIN_EXE_cdno-mcp-server");
    let output = Command::new(bin)
        .args(["--bind", "0.0.0.0:0", "--smoke"])
        .env("RUST_LOG", "off")
        .output()
        .expect("run cdno-mcp-server");
    assert!(
        !output.status.success(),
        "non-loopback bind must be refused until GH #302 lands"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("refusing to bind non-loopback"),
        "interlock message missing from stderr: {stderr}"
    );
}

#[tokio::test]
async fn periodic_reconciliation_picks_up_out_of_band_edits() {
    let dir = TempDir::new().expect("tempdir");
    make_vault(dir.path());
    let server = HttpServer::spawn(Some(dir.path()), &["--reconcile-interval-secs", "1"]);
    let client = reqwest::Client::new();

    // Write a note directly to disk — the exact shape `cdno capture`
    // produces (`inbox/<date>-<slug>.md`, `type: inbox`) — bypassing
    // the server entirely, as a sync tool or editor would.
    let token = "oobprobe7391";
    let inbox = dir.path().join("inbox");
    std::fs::create_dir_all(&inbox).expect("mkdir inbox");
    std::fs::write(
        inbox.join("2026-01-02-oob-probe.md"),
        format!(
            "---\ntype: inbox\ncreated: 2026-01-02T09:00:00\n---\n\n{token} appeared out of band\n"
        ),
    )
    .expect("write out-of-band note");

    // The reconciliation loop runs every second; poll search until
    // the token surfaces (hard deadline well past several passes).
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let (status, resp) = post_mcp(
            &client,
            server.port,
            &json!({
                "jsonrpc": "2.0", "id": 3, "method": "tools/call",
                "params": { "name": "search_notes", "arguments": { "query": token } }
            }),
        )
        .await;
        assert_eq!(status, 200);
        let body = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
        if body.contains(token) {
            return; // reconciled: the out-of-band note is searchable
        }
        assert!(
            Instant::now() < deadline,
            "out-of-band note never appeared in search results; last response: {resp}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}
