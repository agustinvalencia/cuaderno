//! End-to-end test for the git-checkpoint recoverability loop
//! (GH #303): a dirty vault served by `cdno-mcp-server` gains a
//! `cdno-mcp checkpoint` commit within one checkpoint interval.

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
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cdno-mcp-server");

    // Dirty the vault out of band, as a remote write (or sync tool)
    // would, then wait for the sweep to commit it.
    std::fs::write(dir.path().join("inbox/checkpoint-probe.md"), "dirty").unwrap();

    let deadline = Instant::now() + Duration::from_secs(15);
    let committed = loop {
        let log = git(dir.path(), &["log", "--oneline"]);
        let text = String::from_utf8_lossy(&log.stdout).into_owned();
        if text.contains("cdno-mcp checkpoint") {
            break text;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("no checkpoint commit within 15s; git log:\n{text}");
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
