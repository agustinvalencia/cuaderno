//! `cdno watch` — keep the index honest while the vault changes under it.
//!
//! The index is a cache of the markdown on disk, rebuilt by reconciliation.
//! Every other CLI verb reconciles once, at vault open, and exits. That is
//! enough when the process is short-lived, but it means an edit made in
//! another editor is invisible to `cdno search` until the next command
//! happens to run. This watches for those edits and reconciles.
//!
//! Saves `cdno-core/src/watcher.rs`, whose only consumer was the desktop's
//! watcher thread (#597, #600).
//!
//! ## A feature build, not a port
//!
//! The desktop's watcher was coupled to app state — a write journal that
//! stops the app hearing its own writes, and react-query invalidation.
//! None of that transfers, so there is no reference implementation to diff
//! against and correctness cannot be established by comparison. What the
//! CLI needs is narrower: on a debounced external change, reconcile.
//!
//! ## The self-echo is via DIRECTORIES, not via the index
//!
//! This was measured, not reasoned about, and the first guess was wrong.
//! The obvious echo is the index: reconciliation writes
//! `.cuaderno/index.db`, so reacting to that would re-trigger itself.
//! Filtering `.cuaderno/` alone does NOT stop the loop, and a build that
//! did only that spun 18 reconcile passes in 6 seconds on a freshly
//! initialised vault with nobody touching it.
//!
//! The real source is the walk. `reconcile` calls `store.walk_dir` over
//! the whole vault, reading every directory; inotify reports those reads,
//! so each pass emits a `Changed` event for `projects`, `journal/2026`,
//! `stewardships` and every other folder — which triggers the next pass.
//! The index was never the problem, because a pass that finds nothing new
//! still stirs the directories it looked in.
//!
//! So relevance is decided on the FILES that can actually change the
//! index: a path under `.cuaderno/` is ignored (our own writes, and
//! template edits that the index does not care about), and everything
//! else must be a note file. A directory event carries no note
//! extension, so the walk's own footprints are filtered by the same rule
//! rather than by a special case.
//!
//! This is also why the desktop's write journal does not transfer: it
//! exists because the app writes NOTES and must not hear itself. This
//! process writes nothing but the index, so it needs no journal — but it
//! does need to not hear its own reads, which the journal never covered.
//!
//! ## A `.typ` file is read on every pass
//!
//! The classifier reads each loose `.typ` to look for its label, and
//! inotify reports that read as a change to the file, so a `.typ` event
//! cannot be taken at its word the way a `.md` one is. [`TypstEchoFilter`]
//! lets through only one whose mtime or size moved.
//!
//! ## What the note-only rule gives up
//!
//! Only notes are indexed, so a PDF appearing changes no row by itself.
//! Filing an attachment writes or updates its `.md` stub as well, and
//! that is caught. A directory removed takes its notes with it, and each
//! of those arrives as a `Removed` `.md` path in the same batch. The case
//! genuinely missed is a bare directory rename with no note touched,
//! which no reconcile would change either. `Rescan` remains the backstop
//! whenever the backend admits it dropped events.
//!
//! A `config.toml` edit is ignored too. See `Vault::reconcile` —
//! honouring new `ignore` globs means rebuilding the vault, not
//! reconciling it, so `cdno watch` names the boundary and asks to be
//! restarted rather than quietly using stale globs.
//!
//! ## Bursts are coalesced
//!
//! The watcher already debounces (400ms) to absorb an editor's atomic-save
//! storm. A `git checkout` across hundreds of notes still arrives as
//! several batches, so every batch waiting in the channel is drained
//! before reconciling — one pass for the burst rather than one per batch.
//! Events that land DURING a pass queue up and are drained by the next
//! iteration, which is also what keeps this process from ever running two
//! passes at once (#459 is about passes interleaving; this loop is
//! single-threaded and synchronous, so it cannot race itself).

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::channel;

use anyhow::{Context, Result, bail};

use cdno_core::path::VaultPath;
use cdno_core::paths::{self, NoteFormat, note_format};
use cdno_core::reconcile::ReconciliationReport;
use cdno_core::store::{FsVaultStore, VaultStore};
use cdno_core::watcher::{FileEvent, FileWatcher, FsFileWatcher};

use crate::bootstrap;

/// Whether a watched path should trigger a reconcile.
///
/// Its own function, and `pub`, because this is the predicate the
/// self-echo argument above rests on — a test can hold it to that
/// directly, rather than trying to observe a spin loop.
pub fn is_relevant(event: &FileEvent) -> bool {
    match event {
        // The backend says it may have dropped events, so the batch
        // cannot be trusted to be complete: reconcile regardless of what
        // else is in it.
        FileEvent::Rescan => true,
        FileEvent::Changed(path) | FileEvent::Removed(path) => {
            // `.cuaderno/` holds our own index writes, plus templates
            // whose contents the index does not track. `Path::starts_with`
            // compares whole components, which is why this goes through
            // `as_path()` rather than a string prefix: a sibling directory
            // named `.cuadernoX/` shares the textual prefix but is an
            // ordinary part of the vault, and ignoring it would be a
            // missed reconcile.
            if path.as_path().starts_with(paths::CUADERNO_DIR) {
                return false;
            }
            // Note files only. This is what stops the walk's own directory
            // reads from re-triggering the pass that made them — see the
            // module docs; a directory event has no note extension, so it
            // falls out here rather than needing a rule of its own.
            note_format(path).is_some()
        }
    }
}

/// Tells a real change to a `.typ` from the classifier's own read of it, by
/// the mtime and size last seen for the path. A path seen for the first time
/// passes, so the worst a read can cost is one extra pass per file.
#[derive(Debug, Default)]
pub struct TypstEchoFilter {
    seen: HashMap<VaultPath, (u64, u64)>,
}

impl TypstEchoFilter {
    /// Whether `event` still warrants a pass. `stamp` is the path's current
    /// `(mtime_ns, size)`, `None` when it cannot be had; only a `Changed`
    /// event on a `.typ` that matches the last stamp is an echo.
    pub fn is_news(&mut self, event: &FileEvent, stamp: Option<(u64, u64)>) -> bool {
        match event {
            FileEvent::Changed(path) if note_format(path) == Some(NoteFormat::Typst) => {
                let Some(stamp) = stamp else { return true };
                self.seen.insert(path.clone(), stamp) != Some(stamp)
            }
            FileEvent::Removed(path) => {
                self.seen.remove(path);
                true
            }
            _ => true,
        }
    }
}

/// Format a completed pass for the human watching the terminal.
///
/// Reports the counts that changed rather than a bare "reconciled": the
/// point of leaving this running is to see that an edit landed, and
/// `0 added, 0 updated` is exactly the case where something did not.
pub fn describe_pass(report: &ReconciliationReport) -> String {
    let mut parts = Vec::new();
    if report.added > 0 {
        parts.push(format!("{} added", report.added));
    }
    if report.updated > 0 {
        parts.push(format!("{} updated", report.updated));
    }
    if report.removed > 0 {
        parts.push(format!("{} removed", report.removed));
    }
    if parts.is_empty() {
        parts.push("no index change".to_owned());
    }
    let mut line = format!("reconciled: {}", parts.join(", "));
    if !report.errors.is_empty() {
        // Per-file failures do not abort the pass, so they would be
        // invisible without this — a note that stopped being indexed
        // because of a typo in its frontmatter is precisely what someone
        // watching wants to be told.
        line.push_str(&format!(
            " ({} file(s) could not be indexed)",
            report.errors.len()
        ));
    }
    line
}

pub fn run(root: &Path) -> Result<()> {
    // Opening the vault reconciles once, so the index is correct before
    // the first event rather than only after it.
    let (vault, report) = bootstrap::open_vault(root)?;
    println!(
        "Watching {} — {}. Press Ctrl-C to stop.",
        root.display(),
        describe_pass(&report)
    );
    println!("Note: a change to .cuaderno/config.toml needs a restart to take effect.");

    let (sender, receiver) = channel();
    let mut watcher = FsFileWatcher::new(root);
    watcher
        .watch(sender)
        .context("starting the filesystem watcher")?;

    // Ctrl-C terminates the process by default, which is the whole of the
    // stop story: nothing here needs unwinding. A pass interrupted partway
    // leaves the index incomplete, and that is safe — it is a cache, the
    // markdown is untouched, and the next open reconciles it back.
    let store = FsVaultStore::new(root);
    let mut echoes = TypstEchoFilter::default();
    let mut warrants_pass = |event: &FileEvent| {
        if !is_relevant(event) {
            return false;
        }
        let stamp = match event {
            FileEvent::Changed(path) => store
                .metadata(path)
                .ok()
                .map(|meta| (meta.mtime_ns(), meta.size)),
            _ => None,
        };
        echoes.is_news(event, stamp)
    };
    loop {
        // `recv` fails only when every sender is gone, and this process
        // never drops its own: that means the debouncer's worker thread
        // died. Falling out of the loop would have returned `Ok(())`, so
        // `cdno watch` exited 0 while nothing was being watched — the one
        // silent failure in a loop that keeps every other one loud.
        let batch = match receiver.recv() {
            Ok(batch) => batch,
            Err(_) => bail!(
                "the filesystem watcher stopped unexpectedly, so changes are no longer \
                 being seen. Re-run `cdno watch`; if it keeps happening, `cdno reindex` \
                 rebuilds the index from the notes on disk."
            ),
        };
        {
            let mut relevant = batch.iter().fold(false, |any, e| warrants_pass(e) | any);
            // Drain whatever else is already queued so a burst becomes one
            // pass. `try_recv` is non-blocking, so this stops as soon as the
            // channel is empty rather than waiting for a quiet period the
            // debouncer has already waited for.
            while let Ok(extra) = receiver.try_recv() {
                relevant = extra.iter().fold(relevant, |any, e| warrants_pass(e) | any);
            }
            if !relevant {
                continue;
            }
            match vault.reconcile() {
                Ok(report) => println!("{}", describe_pass(&report)),
                // A failed pass must not end the watch: the usual cause is
                // transient (a file half-written by another process), and the
                // next event reconciles again. Exiting here would leave the
                // index stale with nothing watching it.
                Err(err) => eprintln!("reconcile failed, still watching: {err}"),
            }
        }
    }
}
