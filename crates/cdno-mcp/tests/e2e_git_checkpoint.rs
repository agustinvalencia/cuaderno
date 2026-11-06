//! End-to-end tests for the git-checkpoint recoverability loop
//! (GH #303): a dirty vault served by `cdno-mcp-server` gains a
//! `cdno-mcp checkpoint` commit within one checkpoint interval — and,
//! in nudge-only mode (GH #541), gains a sentinel touch and no commit
//! at all.
//!
//! The mode's logic is unit-tested in `checkpoint.rs`; what these cover
//! is the wiring — that the flag reaches the sweep and that the
//! sentinel the sweep touches is the one the operator configured.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        // Identity + no signing, independent of host git config.
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .expect("run git")
}

#[test]
fn dirty_vault_gets_a_checkpoint_commit() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    assert!(git(dir.path(), &["init", "-q"]).status.success());
    assert!(git(dir.path(), &["add", "-A"]).status.success());
    assert!(
        git(dir.path(), &["commit", "-q", "-m", "baseline"])
            .status
            .success()
    );

    // Spawn the server with a 1s checkpoint interval; reconciliation
    // off to keep the test focused.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let bin = env!("CARGO_BIN_EXE_cdno-mcp-server");
    let mut child = Command::new(bin)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--read-only",
            "--reconcile-interval-secs",
            "0",
            "--git-checkpoint-interval-secs",
            "1",
        ])
        .env("CUADERNO_VAULT_PATH", dir.path())
        .env("RUST_LOG", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // Captured, not discarded: every startup failure in this binary
        // (bind refused, vault open error, bad flag combination) prints its
        // reason here and exits, and this test never waits on the process,
        // so discarding it left a dead server indistinguishable from a slow
        // checkpoint sweep (#458).
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cdno-mcp-server");

    // Dirty the vault out of band, as a remote write (or sync tool)
    // would, then wait for the sweep to commit it.
    std::fs::write(dir.path().join("inbox/checkpoint-probe.md"), "dirty").unwrap();

    // This harness never probes for readiness, so a child that never bound
    // is invisible from the first instant; `try_wait` each pass is what makes
    // it visible at all. Checked before the git log so a dead server is
    // reported as one rather than as a checkpoint that never arrived.
    let started = Instant::now();
    let deadline = started + Duration::from_secs(15);
    let mut passes = 0usize;
    let committed = loop {
        passes += 1;
        if let Some(status) = child.try_wait().expect("try_wait") {
            let mut err = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut err);
            }
            panic!(
                "cdno-mcp-server exited with {status} after {:?} ({passes} passes) instead of \
                 checkpointing; stderr:\n{}",
                started.elapsed(),
                if err.trim().is_empty() {
                    "<empty>"
                } else {
                    err.trim()
                }
            );
        }
        let log = git(dir.path(), &["log", "--oneline"]);
        let text = String::from_utf8_lossy(&log.stdout).into_owned();
        if text.contains("cdno-mcp checkpoint") {
            break text;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "no checkpoint commit after {:?} ({passes} passes at a 1s checkpoint interval, \
                 server still running); git log:\n{text}",
                started.elapsed()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    };
    let _ = child.kill();
    let _ = child.wait();

    // The tree is clean again after the checkpoint (everything staged
    // and committed).
    let status = git(dir.path(), &["status", "--porcelain"]);
    assert!(
        status.stdout.is_empty(),
        "tree should be clean after the checkpoint; log was:\n{committed}"
    );
}

#[test]
fn nudge_only_mode_signals_the_agent_and_never_commits() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    assert!(git(dir.path(), &["init", "-q"]).status.success());
    assert!(git(dir.path(), &["add", "-A"]).status.success());
    assert!(
        git(dir.path(), &["commit", "-q", "-m", "baseline"])
            .status
            .success()
    );
    let baseline = String::from_utf8_lossy(&git(dir.path(), &["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_owned();

    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let sentinel = dir.path().join(".git").join("cdno-sync.nudge");
    let bin = env!("CARGO_BIN_EXE_cdno-mcp-server");
    let mut child = Command::new(bin)
        .args([
            "--bind",
            &format!("127.0.0.1:{port}"),
            "--read-only",
            "--reconcile-interval-secs",
            "0",
            "--git-checkpoint-interval-secs",
            "1",
            "--git-checkpoint-mode",
            "nudge-only",
        ])
        .env("CUADERNO_VAULT_PATH", dir.path())
        .env("RUST_LOG", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn cdno-mcp-server");

    std::fs::write(dir.path().join("inbox/nudge-probe.md"), "dirty").unwrap();

    let started = Instant::now();
    let deadline = started + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            let mut err = String::new();
            if let Some(mut pipe) = child.stderr.take() {
                let _ = pipe.read_to_string(&mut err);
            }
            panic!("cdno-mcp-server exited with {status} instead of nudging; stderr:\n{err}");
        }
        if sentinel.exists() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "no sentinel at {} after {:?} at a 1s sweep interval",
                sentinel.display(),
                started.elapsed()
            );
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    // Give the sweep a couple more ticks: a mode that commits would
    // have done so by now, so this is what makes "never commits" mean
    // more than "had not got round to it yet".
    std::thread::sleep(Duration::from_millis(2500));
    let _ = child.kill();
    let _ = child.wait();

    let head = String::from_utf8_lossy(&git(dir.path(), &["rev-parse", "HEAD"]).stdout)
        .trim()
        .to_owned();
    assert_eq!(head, baseline, "nudge-only mode must create no commit");

    // Still dirty: nothing was staged either, so the external agent
    // finds the change exactly as it was left. (`inbox/` is empty at
    // `init`, so git never tracked it and reports the collapsed
    // `?? inbox/` rather than the file — hence the directory, not the
    // filename, is what to look for.)
    let status = git(dir.path(), &["status", "--porcelain"]);
    let status = String::from_utf8_lossy(&status.stdout);
    assert!(
        status.contains("inbox/"),
        "nudge-only mode must leave the change for the external agent; status was:\n{status}"
    );
}

/// RFC 0007 §5.8 (2), T26: the `rename_note` tool never fetches. The vault's
/// upstream is an unreachable remote the branch is recorded as ahead of: a
/// tool that fetched would hit it, and say so in a warning. This one reads
/// the upstream as last fetched, so it neither warns about a fetch nor
/// blocks, and git is left with no `FETCH_HEAD`.
#[test]
fn rename_note_never_fetches() {
    use std::io::{BufRead, BufReader, Write};

    let dir = tempfile::TempDir::new().expect("tempdir");
    cdno_cli::commands::init::run(dir.path()).expect("cdno init");
    std::fs::write(
        dir.path().join("projects/surrogate-model.md"),
        "---\ntype: project\ncontext: work\nstatus: active\ncreated: 2026-04-01\n---\n\n# Surrogate model\n\n## Current State\nCalibrating.\n\n## Next Actions\n\n## Milestones\n",
    )
    .unwrap();
    let ok = |args: &[&str]| {
        let out = git(dir.path(), args);
        assert!(out.status.success(), "git {args:?}: {out:?}");
    };
    ok(&["init", "-q", "-b", "main"]);
    ok(&["add", "-A"]);
    ok(&["commit", "-q", "-m", "baseline"]);
    // An upstream that cannot be reached, recorded as one commit behind
    // HEAD: ahead 1, behind 0, as a fetch last left it.
    let unreachable = dir.path().join("no-such-remote");
    ok(&["remote", "add", "origin", unreachable.to_str().unwrap()]);
    ok(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    ok(&["config", "branch.main.remote", "origin"]);
    ok(&["config", "branch.main.merge", "refs/heads/main"]);
    ok(&[
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "ahead of the upstream",
    ]);

    let mut child = Command::new(env!("CARGO_BIN_EXE_cdno-mcp"))
        .env("CUADERNO_VAULT_PATH", dir.path())
        .env("RUST_LOG", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cdno-mcp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut send = |msg: serde_json::Value| {
        writeln!(stdin, "{msg}").unwrap();
        stdin.flush().unwrap();
    };
    let mut read = |id: u64| -> serde_json::Value {
        loop {
            let mut line = String::new();
            assert!(stdout.read_line(&mut line).unwrap() > 0, "server closed");
            let value: serde_json::Value = serde_json::from_str(&line).unwrap();
            if value["id"] == id {
                return value;
            }
        }
    };
    send(serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05", "capabilities": {},
            "clientInfo": { "name": "never-fetches", "version": "0" }
        }
    }));
    read(1);
    send(serde_json::json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));

    let mut plan_hash = String::new();
    for (id, dry_run) in [(2u64, true), (3, false)] {
        let mut arguments = serde_json::json!({
            "note": "surrogate-model", "to": "emulator-study", "dry_run": dry_run
        });
        if !dry_run {
            arguments["expected_plan"] = serde_json::json!(plan_hash);
        }
        send(serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": "tools/call",
            "params": { "name": "rename_note", "arguments": arguments }
        }));
        let response = read(id);
        assert_ne!(
            response["result"]["isError"],
            serde_json::json!(true),
            "an upstream recorded as ahead must not block: {response}"
        );
        let text = response["result"]["content"][0]["text"].as_str().unwrap();
        let payload: serde_json::Value = serde_json::from_str(text).unwrap();
        let fetch_warnings: Vec<&serde_json::Value> = payload["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|w| w["message"].as_str().is_some_and(|m| m.contains("fetch")))
            .collect();
        assert!(fetch_warnings.is_empty(), "{payload}");
        if dry_run {
            plan_hash = payload["plan_hash"].as_str().unwrap().to_owned();
        } else {
            assert_eq!(payload["written"], serde_json::json!(true), "{payload}");
        }
    }
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        !dir.path().join(".git/FETCH_HEAD").exists(),
        "git fetch leaves FETCH_HEAD; the tool never ran one"
    );
    assert!(dir.path().join("projects/emulator-study.md").exists());
}
