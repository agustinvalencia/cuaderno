//! Tests for `cdno watch` (#600).
//!
//! Split deliberately. The predicate that decides whether an event is worth
//! reconciling is a pure function and is tested as one — that is where the
//! only real bug in this verb lived, and a unit test pins it precisely. The
//! claim that an *external* edit reaches the index THROUGH the watcher
//! cannot be established that way, so one test spawns the real binary.

use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use cdno_cli::commands::{init, watch};
use cdno_core::path::VaultPath;
use cdno_core::reconcile::{ReconciliationIssue, ReconciliationReport};
use cdno_core::watcher::FileEvent;
use tempfile::tempdir;

fn seed(root: &Path) {
    init::run(root).expect("init");
}

fn changed(path: &str) -> FileEvent {
    FileEvent::Changed(VaultPath::new(path).expect("valid vault path"))
}

fn removed(path: &str) -> FileEvent {
    FileEvent::Removed(VaultPath::new(path).expect("valid vault path"))
}

#[test]
fn a_directory_event_is_ignored_because_our_own_walk_causes_it() {
    // THE bug this verb had, and it took a measurement rather than
    // reasoning to find. `reconcile` walks the whole vault, and inotify
    // reports every directory it reads as changed — so reacting to
    // directory events means each pass triggers the next. A build that
    // filtered only `.cuaderno/` ran 18 passes in 6 seconds on a freshly
    // initialised vault that nobody was touching.
    //
    // Directories are excluded by the same `.md` rule as everything else
    // rather than by a special case, which is why these assertions are
    // about extensions rather than about "is a directory".
    assert!(!watch::is_relevant(&changed("projects")));
    assert!(!watch::is_relevant(&changed("journal/2026")));
    assert!(!watch::is_relevant(&changed("journal/2026/daily")));
    assert!(!watch::is_relevant(&changed("projects/_parked")));
}

#[test]
fn our_own_index_writes_are_ignored() {
    // The echo that was guessed first. It is real, just not sufficient on
    // its own: reconciliation writes the index, so reacting to it would
    // also self-trigger. The directory rule above happens to cover
    // `.cuaderno/` too, but this is asserted separately because the two
    // are independent reasons and a future change to either should not
    // silently remove the other's protection.
    assert!(!watch::is_relevant(&changed(".cuaderno/index.db")));
    assert!(!watch::is_relevant(&changed(".cuaderno/index.db-wal")));
    assert!(!watch::is_relevant(&changed(".cuaderno/index.db-shm")));
    assert!(!watch::is_relevant(&changed(".cuaderno/.lock")));
    assert!(!watch::is_relevant(&changed(".cuaderno/config.toml")));
    // Templates are markdown, so only the `.cuaderno/` rule excludes
    // them — and it must, because their content is not indexed.
    assert!(!watch::is_relevant(&changed(
        ".cuaderno/templates/project.md"
    )));
}

#[test]
fn a_directory_that_merely_starts_with_the_same_text_is_not_ignored() {
    // Why the predicate compares path COMPONENTS rather than a string
    // prefix: `.cuadernoX/` shares the textual prefix with `.cuaderno/`
    // but is an ordinary part of the vault, and skipping it would be a
    // silently missed reconcile.
    assert!(watch::is_relevant(&changed(".cuadernoX/note.md")));
}

#[test]
fn markdown_changes_and_removals_are_what_trigger_a_pass() {
    assert!(watch::is_relevant(&changed("projects/alpha.md")));
    assert!(watch::is_relevant(&removed("projects/alpha.md")));
    assert!(watch::is_relevant(&changed(
        "journal/2026/daily/2026-09-25.md"
    )));
    // Case is not a contract of the filesystem, so it is not one here.
    assert!(watch::is_relevant(&changed("inbox/Note.MD")));
    // Non-markdown carries no frontmatter and changes no row on its own;
    // filing an attachment writes its `.md` stub too, which is caught.
    assert!(!watch::is_relevant(&changed("portfolios/p/paper.pdf")));
}

#[test]
fn a_rescan_always_triggers_a_pass() {
    // The backend is saying it may have dropped events, so the batch
    // cannot be trusted to be complete — this is the one case where
    // reconciling despite knowing nothing about the paths is correct.
    assert!(watch::is_relevant(&FileEvent::Rescan));
}

#[test]
fn a_pass_reports_what_changed_rather_than_merely_that_it_ran() {
    // The point of leaving this running is to see that an edit landed,
    // so "no index change" has to be distinguishable from "1 updated" —
    // the former is exactly the case where something did not work.
    let mut report = ReconciliationReport::default();
    assert!(watch::describe_pass(&report).contains("no index change"));

    report.added = 30;
    assert!(watch::describe_pass(&report).contains("30 added"));

    let mut report = ReconciliationReport {
        removed: 2,
        ..Default::default()
    };
    assert!(watch::describe_pass(&report).contains("2 removed"));
    assert!(
        !watch::describe_pass(&report).contains("no index change"),
        "a pass that removed rows is a change"
    );

    // Per-file failures do not abort a pass, so without this they would
    // be invisible — a note that dropped out of the index because of a
    // typo in its frontmatter is precisely what a watcher should say.
    report.errors.push(ReconciliationIssue {
        path: VaultPath::new("inbox/broken.md").unwrap(),
        reason: "missing frontmatter".to_owned(),
    });
    assert!(watch::describe_pass(&report).contains("could not be indexed"));
}

/// Poll `check` until it holds or the deadline passes.
///
/// A fixed sleep is what #458 blames for the flaky reconciliation test in
/// `cdno-mcp`: a budget that is generous when a test runs alone expires
/// under a full parallel workspace run, and the assertion then fails for
/// load rather than for behaviour. Polling to a deadline fails only when
/// the state never arrives.
fn wait_until(deadline: Duration, mut check: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    check()
}

#[test]
fn an_external_edit_reaches_the_index_through_the_watcher() {
    // #600's own probe, and the reason it insists on being behavioural:
    // nothing but an out-of-band write and a subsequent read establishes
    // that the loop actually runs. The evidence is the watcher's own
    // report of the pass — reading the vault back with another `cdno`
    // command would prove nothing, because opening a vault reconciles,
    // so the read would repair the index itself and pass either way.
    use std::process::{Command, Stdio};

    let dir = tempdir().unwrap();
    seed(dir.path());
    let note = dir.path().join("inbox").join("external.md");

    let log = dir.path().join("watch.log");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cdno"))
        .env_remove("CUADERNO_VAULT_PATH")
        .arg("--vault")
        .arg(dir.path())
        .arg("watch")
        .stdout(Stdio::from(fs::File::create(&log).unwrap()))
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cdno watch");

    let read_log = || fs::read_to_string(&log).unwrap_or_default();
    assert!(
        wait_until(Duration::from_secs(20), || read_log().contains("Watching")),
        "the watcher never started; log was:\n{}",
        read_log()
    );

    // The edit itself: a note written by nothing that knows about cdno.
    fs::write(
        &note,
        "---\ntype: inbox\ncreated: 2026-09-25\n---\n\n# External\n",
    )
    .unwrap();

    let landed = wait_until(Duration::from_secs(30), || read_log().contains("1 added"));
    let final_log = read_log();
    let _ = child.kill();
    let _ = child.wait();

    assert!(
        landed,
        "an externally written note never reached the index; watcher log was:\n{final_log}"
    );
}

#[test]
fn the_watcher_does_not_reconcile_when_nothing_is_happening() {
    // The regression guard for the spin loop, at the level where it was
    // actually observed. The predicate tests above pin the rule; this
    // pins the consequence, because the bug was not in any one event's
    // classification but in the loop feeding itself.
    use std::process::{Command, Stdio};

    let dir = tempdir().unwrap();
    seed(dir.path());
    let log = dir.path().join("watch.log");
    let mut child = Command::new(env!("CARGO_BIN_EXE_cdno"))
        .env_remove("CUADERNO_VAULT_PATH")
        .arg("--vault")
        .arg(dir.path())
        .arg("watch")
        .stdout(Stdio::from(fs::File::create(&log).unwrap()))
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn cdno watch");

    let read_log = || fs::read_to_string(&log).unwrap_or_default();
    assert!(
        wait_until(Duration::from_secs(20), || read_log().contains("Watching")),
        "the watcher never started"
    );

    // Deliberately a plain sleep rather than a deadline poll: the claim
    // is that NOTHING happens, and the only way to observe nothing is to
    // let time pass. The startup line is excluded because it reports the
    // open-time pass, which is expected.
    std::thread::sleep(Duration::from_secs(3));
    let passes = read_log()
        .lines()
        .filter(|line| line.starts_with("reconciled:"))
        .count();
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(
        passes, 0,
        "an idle vault must provoke no reconcile passes; the earlier build ran 18 in 6 seconds"
    );
}
