//! `cdno migrate [--dry-run]` — move the vault to the method version this
//! cdno implements (RFC 0006 §8.4, plan T9).
//!
//! A thin layer over [`Vault::plan_migration`] (the dry run) and
//! [`Vault::migrate`]: it passes [`METHOD_VERSION`] and [`FILE_MIGRATIONS`]
//! and formats the [`MigrationReport`]. Nothing here decides what changes.
//!
//! The verb has no promptable argument, so it prompts and confirms nothing:
//! `--dry-run` is the look before the leap. It is CLI-only (no MCP tool) and
//! never runs at open; the person chooses when the vault moves.

use std::path::Path;

use anyhow::Result;
use chrono::NaiveDateTime;
use serde_json::json;

use cdno_domain::{
    CommitmentSource, FILE_MIGRATIONS, METHOD_VERSION, MigrationReport, describe_method_version,
};

use crate::bootstrap;

/// Run `cdno migrate`. `at` stamps the `vault migrated` daily-log entry; a
/// dry run uses it only to name the daily note the entry would go to.
pub fn run(root: &Path, at: NaiveDateTime, dry_run: bool, json: bool) -> Result<()> {
    let (vault, _report) = bootstrap::open_vault(root)?;
    let report = if dry_run {
        vault.plan_migration(at.date(), METHOD_VERSION, FILE_MIGRATIONS)?
    } else {
        vault.migrate(at, METHOD_VERSION, FILE_MIGRATIONS)?
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&to_json(&report, dry_run))?
        );
    } else {
        print!("{}", render(&report, dry_run, at));
    }
    Ok(())
}

/// What the run did or would do, as one word for `--json`.
fn outcome(report: &MigrationReport, dry_run: bool) -> &'static str {
    match (report.stamps(), dry_run) {
        (false, _) => "nothing_to_do",
        (true, true) => "planned",
        (true, false) => "migrated",
    }
}

/// The `--json` shape: the same content the text carries.
fn to_json(report: &MigrationReport, dry_run: bool) -> serde_json::Value {
    json!({
        "dry_run": dry_run,
        "outcome": outcome(report, dry_run),
        "vault": report.from,
        "vault_described": describe_method_version(report.from),
        "binary": report.to,
        "migrations": report.steps,
        "config_line": report.stamp_line(),
        "log_entry": report.log_entry(),
        "read_through": report.read_through,
    })
}

/// The human-readable report.
fn render(report: &MigrationReport, dry_run: bool, at: NaiveDateTime) -> String {
    let mut out = String::new();
    if !report.stamps() {
        out.push_str(&format!("vault is at RLM {}; nothing to do\n", report.to));
        // A vault at the binary's version but below RLM 1.1 still has its
        // read-through: reachable only from a cdno implementing RLM 1.0.
        if !report.read_through.is_empty() {
            render_read_through(report, &mut out);
        }
        return out;
    }
    if dry_run {
        out.push_str("Dry run: nothing written.\n");
    } else {
        out.push_str(&format!("Migrated the vault to RLM {}.\n", report.to));
    }
    out.push_str(&format!(
        "  vault:      {}\n",
        describe_method_version(report.from)
    ));
    out.push_str(&format!("  this cdno:  RLM {}\n", report.to));

    if report.steps.is_empty() {
        out.push_str("  migrations: none: additive\n");
    } else {
        out.push_str("  migrations:\n");
        for step in &report.steps {
            out.push_str(&format!(
                "    to RLM {}.0: {}\n",
                step.to_major, step.description
            ));
            if step.writes.is_empty() {
                out.push_str("      (every file already migrated)\n");
            }
            for path in &step.writes {
                out.push_str(&format!("      {path}\n"));
            }
        }
    }

    let (config_verb, log_verb) = if dry_run {
        ("would write", "would append")
    } else {
        ("wrote", "appended")
    };
    if let Some(line) = report.stamp_line() {
        out.push_str(&format!(
            "  config:     {config_verb} {line} under [vault]\n"
        ));
    }
    if let Some(entry) = report.log_entry() {
        out.push_str(&format!(
            "  log:        {log_verb} to the daily note for {}:\n",
            at.date()
        ));
        for line in entry.lines() {
            out.push_str(&format!("                {line}\n"));
        }
    }

    if report.read_through.is_empty() {
        out.push_str("  read-through: none\n");
    } else {
        render_read_through(report, &mut out);
    }
    out
}

/// The read-through section: every hard date on a parked map, overdue first.
fn render_read_through(report: &MigrationReport, out: &mut String) {
    out.push_str(
        "  read-through (hard dates on parked maps, overdue first; drop or re-date each):\n",
    );
    for entry in &report.read_through {
        let (kind, map) = match &entry.source {
            CommitmentSource::ProjectMilestone(slug) => ("hard milestone", slug),
            CommitmentSource::ActionNote(slug) => ("action due", slug),
            CommitmentSource::Stewardship(slug) | CommitmentSource::StandaloneCommitment(slug) => {
                ("date", slug)
            }
        };
        let overdue = if entry.is_overdue { ", overdue" } else { "" };
        out.push_str(&format!(
            "    {}  {}  ({kind} on [[{map}]]{overdue})\n",
            entry.date, entry.title
        ));
    }
}
