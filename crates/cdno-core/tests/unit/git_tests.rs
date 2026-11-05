//! Tests for `cdno_core::git` on real temporary repositories.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use cdno_core::git::{GitError, GitState, Upstream, fetch_quietly, git_state};
use tempfile::TempDir;

fn have_git() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

macro_rules! require_git {
    () => {
        if !have_git() {
            eprintln!("skipping: git is not on PATH");
            return;
        }
    };
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "protocol.file.allow=always",
        ])
        .args(args)
        .output()
        .expect("run git")
}

fn ok(dir: &Path, args: &[&str]) {
    let out = git(dir, args);
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn commit_file(dir: &Path, name: &str, content: &str) {
    std::fs::write(dir.join(name), content).unwrap();
    ok(dir, &["add", "-A"]);
    ok(dir, &["commit", "-q", "-m", name]);
}

fn init_repo(dir: &Path) {
    ok(dir, &["init", "-q"]);
    commit_file(dir, "a.md", "one\n");
}

/// A bare remote plus two clones, both at the same first commit.
fn remote_and_clones() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let remote = tmp.path().join("remote.git");
    let a = tmp.path().join("a");
    let b = tmp.path().join("b");
    std::fs::create_dir_all(&remote).unwrap();
    ok(&remote, &["init", "-q", "--bare"]);
    std::fs::create_dir_all(&a).unwrap();
    ok(&a, &["init", "-q"]);
    ok(&a, &["remote", "add", "origin", remote.to_str().unwrap()]);
    commit_file(&a, "a.md", "one\n");
    ok(&a, &["push", "-q", "-u", "origin", "HEAD"]);
    ok(
        tmp.path(),
        &["clone", "-q", remote.to_str().unwrap(), b.to_str().unwrap()],
    );
    (tmp, a, b)
}

fn upstream(dir: &Path) -> Option<Upstream> {
    match git_state(dir) {
        GitState::Repository { upstream, .. } => upstream,
        other => panic!("expected a repository, got {other:?}"),
    }
}

#[test]
fn a_plain_dir_is_not_a_repository() {
    require_git!();
    let dir = TempDir::new().unwrap();
    assert_eq!(git_state(dir.path()), GitState::NotARepository);
}

#[test]
fn a_repo_with_no_upstream_reports_none() {
    require_git!();
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());
    assert_eq!(
        git_state(dir.path()),
        GitState::Repository {
            paused: None,
            upstream: None
        }
    );
}

#[test]
fn behind_is_counted_after_a_fetch() {
    require_git!();
    let (_tmp, a, b) = remote_and_clones();
    commit_file(&a, "two.md", "two\n");
    ok(&a, &["push", "-q"]);

    assert_eq!(
        upstream(&b),
        Some(Upstream {
            ahead: 0,
            behind: 0
        })
    );
    fetch_quietly(&b, Duration::from_secs(20)).unwrap();
    assert_eq!(
        upstream(&b),
        Some(Upstream {
            ahead: 0,
            behind: 1
        })
    );
}

#[test]
fn ahead_is_counted() {
    require_git!();
    let (_tmp, _a, b) = remote_and_clones();
    commit_file(&b, "local.md", "local\n");
    assert_eq!(
        upstream(&b),
        Some(Upstream {
            ahead: 1,
            behind: 0
        })
    );
}

/// Leave a conflicting merge in place on a fresh repo.
fn conflicted_merge(dir: &Path) {
    ok(dir, &["init", "-q"]);
    commit_file(dir, "a.md", "base\n");
    ok(dir, &["checkout", "-q", "-b", "feature"]);
    commit_file(dir, "a.md", "feature\n");
    ok(dir, &["checkout", "-q", "-"]);
    commit_file(dir, "a.md", "main side\n");
    let merge = git(dir, &["merge", "feature"]);
    assert!(!merge.status.success(), "merge should conflict");
}

#[test]
fn a_paused_merge_is_reported() {
    require_git!();
    let dir = TempDir::new().unwrap();
    conflicted_merge(dir.path());
    match git_state(dir.path()) {
        GitState::Repository {
            paused: Some(op), ..
        } => assert_eq!(op.marker, Some("MERGE_HEAD")),
        other => panic!("expected a paused merge, got {other:?}"),
    }
}

#[test]
fn unmerged_paths_without_a_marker_are_reported() {
    require_git!();
    let dir = TempDir::new().unwrap();
    conflicted_merge(dir.path());
    std::fs::remove_file(dir.path().join(".git/MERGE_HEAD")).unwrap();
    match git_state(dir.path()) {
        GitState::Repository {
            paused: Some(op), ..
        } => {
            assert_eq!(op.marker, None);
            assert!(op.reason.contains("unmerged"), "{}", op.reason);
        }
        other => panic!("expected unmerged paths, got {other:?}"),
    }
}

#[test]
fn probes_never_fetch() {
    require_git!();
    let (_tmp, _a, b) = remote_and_clones();
    ok(&b, &["remote", "set-url", "origin", "file:///nonexistent"]);
    let start = Instant::now();
    let got = upstream(&b);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(
        got,
        Some(Upstream {
            ahead: 0,
            behind: 0
        })
    );
}

#[test]
fn fetch_times_out() {
    require_git!();
    let (_tmp, _a, b) = remote_and_clones();
    // Accepts connections and never replies.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let held = std::thread::spawn(move || {
        let mut conns = Vec::new();
        while let Ok((s, _)) = listener.accept() {
            conns.push(s);
        }
    });
    ok(
        &b,
        &[
            "remote",
            "set-url",
            "origin",
            &format!("git://127.0.0.1:{port}/x"),
        ],
    );
    let start = Instant::now();
    let err = fetch_quietly(&b, Duration::from_secs(1)).unwrap_err();
    assert!(matches!(err, GitError::TimedOut { .. }), "{err:?}");
    assert!(start.elapsed() < Duration::from_secs(3));
    drop(held); // detached: the listener thread ends with the process
}
