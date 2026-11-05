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
    let mut cmd = Command::new("git");
    // Inside a git hook GIT_DIR, GIT_INDEX_FILE, ... are exported and would
    // send the fixtures to the wrong repository.
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("GIT_") {
            cmd.env_remove(k);
        }
    }
    cmd.arg("-C")
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
    assert!(matches!(git_state(dir.path()), GitState::NotARepository));
}

#[test]
fn a_repo_with_no_upstream_reports_none() {
    require_git!();
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());
    match git_state(dir.path()) {
        GitState::Repository {
            paused: Ok(None),
            upstream: None,
        } => {}
        other => panic!("expected a clean repository, got {other:?}"),
    }
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
            paused: Ok(Some(op)),
            ..
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
            paused: Ok(Some(op)),
            ..
        } => {
            assert_eq!(op.marker, None);
            assert!(op.reason.contains("unmerged"), "{}", op.reason);
        }
        other => panic!("expected unmerged paths, got {other:?}"),
    }
}

#[test]
fn probes_do_not_see_an_unfetched_push() {
    require_git!();
    let (_tmp, a, b) = remote_and_clones();
    commit_file(&a, "two.md", "two\n");
    ok(&a, &["push", "-q"]);

    let start = Instant::now();
    let before = upstream(&b);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(
        before,
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

#[test]
fn a_failed_paused_probe_is_an_error_not_none() {
    require_git!();
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());
    // A corrupt index makes `git diff` fail while `rev-parse` still works.
    std::fs::write(dir.path().join(".git/index"), b"not an index").unwrap();
    match git_state(dir.path()) {
        GitState::Repository { paused, .. } => {
            assert!(
                paused.is_err(),
                "unknown must not read as not-paused: {paused:?}"
            )
        }
        other => panic!("expected a repository, got {other:?}"),
    }
}

#[test]
fn a_git_that_cannot_answer_is_unknown() {
    require_git!();
    let dir = TempDir::new().unwrap();
    init_repo(dir.path());
    // A malformed config makes git fail with something other than
    // "not a git repository".
    let cfg = dir.path().join(".git/config");
    let mut text = std::fs::read_to_string(&cfg).unwrap();
    text.push_str("\n[broken\n");
    std::fs::write(&cfg, text).unwrap();
    assert!(matches!(git_state(dir.path()), GitState::Unknown(_)));
}

fn resolve_and_stage(work: &Path, rel: &str) {
    std::fs::write(work.join(rel), "resolved\n").unwrap();
    ok(work, &["add", "-A"]);
}

#[test]
fn a_paused_merge_is_reported_from_a_subdirectory() {
    require_git!();
    let dir = TempDir::new().unwrap();
    let repo = dir.path();
    ok(repo, &["init", "-q"]);
    std::fs::create_dir_all(repo.join("vault")).unwrap();
    commit_file(repo, "vault/a.md", "base\n");
    ok(repo, &["checkout", "-q", "-b", "feature"]);
    commit_file(repo, "vault/a.md", "feature\n");
    ok(repo, &["checkout", "-q", "-"]);
    commit_file(repo, "vault/a.md", "main side\n");
    assert!(!git(repo, &["merge", "feature"]).status.success());
    resolve_and_stage(repo, "vault/a.md");

    match git_state(&repo.join("vault")) {
        GitState::Repository {
            paused: Ok(Some(op)),
            ..
        } => assert_eq!(op.marker, Some("MERGE_HEAD")),
        other => panic!("expected a paused merge, got {other:?}"),
    }
}

#[test]
fn a_paused_merge_is_reported_in_a_linked_worktree() {
    require_git!();
    let dir = TempDir::new().unwrap();
    let repo = dir.path().join("main");
    let wt = dir.path().join("wt");
    std::fs::create_dir_all(&repo).unwrap();
    ok(&repo, &["init", "-q"]);
    commit_file(&repo, "a.md", "base\n");
    ok(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "feature",
            wt.to_str().unwrap(),
        ],
    );
    commit_file(&wt, "a.md", "feature\n");
    commit_file(&repo, "a.md", "main side\n");
    let base = String::from_utf8(git(&repo, &["symbolic-ref", "--short", "HEAD"]).stdout)
        .unwrap()
        .trim()
        .to_string();
    assert!(!git(&wt, &["merge", &base]).status.success());
    resolve_and_stage(&wt, "a.md");

    assert!(wt.join(".git").is_file());
    match git_state(&wt) {
        GitState::Repository {
            paused: Ok(Some(op)),
            ..
        } => assert_eq!(op.marker, Some("MERGE_HEAD")),
        other => panic!("expected a paused merge, got {other:?}"),
    }
}

#[test]
fn a_timed_out_fetch_leaves_no_helper_behind() {
    require_git!();
    if Command::new("pgrep").arg("-V").output().is_err() {
        eprintln!("skipping: pgrep is not on PATH");
        return;
    }
    let (_tmp, _a, b) = remote_and_clones();
    // An http remote makes git spawn a transport helper that then waits
    // on a server that never answers.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut conns = Vec::new();
        while let Ok((s, _)) = listener.accept() {
            conns.push(s);
        }
    });
    let url = format!("http://127.0.0.1:{port}/x");
    ok(&b, &["remote", "set-url", "origin", &url]);
    let err = fetch_quietly(&b, Duration::from_secs(1)).unwrap_err();
    assert!(matches!(err, GitError::TimedOut { .. }), "{err:?}");

    let needle = format!("127.0.0.1:{port}");
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let found = Command::new("pgrep")
            .args(["-f", &needle])
            .output()
            .unwrap();
        if found.stdout.is_empty() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "helper survived: {}",
            String::from_utf8_lossy(&found.stdout)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
