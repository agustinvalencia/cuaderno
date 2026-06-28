//! Frontmatter normalisation (#233).
//!
//! Reorders a note's frontmatter keys into the canonical per-type order
//! ([`NoteType::frontmatter_order`]) without touching values. Line-based
//! and value-preserving, in the same spirit as
//! `rewrite_field_in_frontmatter`: it never re-emits YAML from a parsed
//! model, so quoting, `null`s, and any unknown keys survive verbatim.
//!
//! Exposed as an explicit pass (`cdno normalise`), never run on an
//! ordinary write — notes cdno creates are already canonical (their
//! templates define the order); this is for hand-authored or migrated
//! notes, so it shouldn't churn diffs unless asked.

use std::str::FromStr;

use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use super::Vault;
use super::index_entry::build_index_entry_for;
use crate::error::DomainError;
use crate::note_type::NoteType;

/// Outcome of a normalisation pass.
#[derive(Debug, Default, Clone)]
pub struct NormaliseReport {
    /// Notes examined (indexed notes with a known type).
    pub checked: usize,
    /// Notes skipped because their `type` isn't a known variant — lint
    /// reports those; the normaliser can't pick an order for them.
    pub skipped: usize,
    /// Notes whose frontmatter was reordered (or would be, in a dry run).
    pub changed: Vec<VaultPath>,
    /// Notes that couldn't be read, recorded rather than aborting.
    pub errors: Vec<(VaultPath, String)>,
}

impl Vault {
    /// Reorder every note's frontmatter into canonical key order.
    ///
    /// With `dry_run`, nothing is written — `changed` lists the notes
    /// that *would* be reordered (the `--check` mode). Otherwise the
    /// rewrites are staged onto one transaction and committed together.
    /// Notes with an unknown `type` are skipped (lint reports those);
    /// a note that fails to read is recorded in `errors` and the pass
    /// continues.
    pub fn normalise_notes(&self, dry_run: bool) -> Result<NormaliseReport, DomainError> {
        let paths = self.index.list_all_paths()?;
        let mut report = NormaliseReport::default();
        // One transaction for the whole pass: the rewrites commit
        // all-or-nothing and hold the write lock once. Fine at the vault
        // sizes cdno targets; if a vault ever grew large enough that the
        // single lock-hold or transaction size mattered, this is the
        // line to switch to per-note commits.
        let mut tx = if dry_run {
            None
        } else {
            Some(self.transaction()?)
        };

        for path in paths {
            // A concurrent writer could drop the note between listing
            // and lookup; treat that as nothing-to-do.
            let Some(entry) = self.index.find_by_path(&path)? else {
                continue;
            };
            let Ok(note_type) = NoteType::from_str(&entry.note_type) else {
                report.skipped += 1; // unknown type: lint's job, not ours
                continue;
            };
            report.checked += 1;

            let raw = match self.store.read_file(&path) {
                Ok(raw) => raw,
                Err(e) => {
                    report.errors.push((path, e.to_string()));
                    continue;
                }
            };

            let order = self.canonical_frontmatter_order(note_type, &raw)?;

            let Some(new_raw) = reorder_frontmatter(&raw, &order) else {
                continue; // no frontmatter, or already canonical
            };
            report.changed.push(path.clone());

            if let Some(tx) = tx.as_mut() {
                let meta = build_index_entry_for(&path, &new_raw, note_type.as_str())?;
                tx.write_file(path.clone(), new_raw);
                tx.upsert_note(meta);
            }
        }

        if let Some(tx) = tx {
            tx.commit()?;
        }
        Ok(report)
    }

    /// The canonical frontmatter key order for a note of `note_type`
    /// whose raw content is `raw`. Derived from the *effective* template
    /// (custom `.cuaderno/templates/` override if present, else the
    /// built-in), so it respects a custom template's field order rather
    /// than a hardcoded one. Tracking's order is variant-specific, keyed
    /// by the note's `activity`, so `raw` is parsed to pick the variant.
    ///
    /// Shared by `normalise_notes` (which reorders to this) and the lint
    /// frontmatter-order rule (#236, which flags deviation from it).
    pub(in crate::vault) fn canonical_frontmatter_order(
        &self,
        note_type: NoteType,
        raw: &str,
    ) -> Result<Vec<String>, DomainError> {
        let variant = if note_type == NoteType::Tracking {
            Frontmatter::parse(raw)
                .ok()
                .and_then(|(fm, _)| fm.optional_field::<String>("activity").ok().flatten())
        } else {
            None
        };
        let template = self.resolve_template_content(note_type.as_str(), variant.as_deref())?;
        Ok(frontmatter_key_order(&template))
    }
}

/// Reorder the frontmatter block of `raw` so its keys follow `order`
/// (the listed keys first, in that order; any other keys after, in
/// their original relative order). Values and formatting are preserved
/// verbatim — only whole `key:` line-groups move.
///
/// Returns `Some(new_raw)` when the order actually changed, and `None`
/// when the note has no frontmatter or is already canonical, so callers
/// skip the write and the pass stays idempotent.
pub(in crate::vault) fn reorder_frontmatter(raw: &str, order: &[String]) -> Option<String> {
    let opening = "---\n";
    if !raw.starts_with(opening) {
        return None;
    }
    let after_open = opening.len();
    // Locate the close with the same naive `\n---` scan as
    // `rewrite_field_in_frontmatter`. It's looser than core's own-line
    // `split_at_closing_delim`, but every note reaching here was already
    // parsed by the index (so serde_yaml accepted its frontmatter),
    // which rules out the inputs where the two would disagree. `+1`
    // keeps the trailing newline of the last frontmatter line in `yaml`.
    let yaml_end = after_open + raw[after_open..].find("\n---")? + 1;
    let yaml = &raw[after_open..yaml_end];

    // Group lines: a new group starts at each top-level `key:` line;
    // continuation lines (indented, list items, blanks, comments)
    // attach to the current group so a multi-line YAML value moves as
    // one unit.
    let mut groups: Vec<(Option<&str>, String)> = Vec::new();
    for line in yaml.split_inclusive('\n') {
        match top_level_key(line) {
            Some(key) => groups.push((Some(key), line.to_owned())),
            None => match groups.last_mut() {
                Some(last) => last.1.push_str(line),
                // Lines before any key (unusual) stay first, keyless.
                None => groups.push((None, line.to_owned())),
            },
        }
    }

    // Reassemble: keyless preamble first (original order), then the
    // listed keys in canonical order, then the rest in original order.
    let mut used = vec![false; groups.len()];
    let mut new_yaml = String::with_capacity(yaml.len());

    // Keyless preamble (lines before any key — unusual but preserved).
    for (group, used) in groups.iter().zip(used.iter_mut()) {
        if group.0.is_none() {
            new_yaml.push_str(&group.1);
            *used = true;
        }
    }
    // Known keys, in canonical order.
    for key in order {
        for (group, used) in groups.iter().zip(used.iter_mut()) {
            if !*used && group.0 == Some(key.as_str()) {
                new_yaml.push_str(&group.1);
                *used = true;
            }
        }
    }
    // Anything left (unknown keys), in original order.
    for (group, used) in groups.iter().zip(used.iter_mut()) {
        if !*used {
            new_yaml.push_str(&group.1);
            *used = true;
        }
    }

    if new_yaml == yaml {
        return None;
    }
    let mut result = String::with_capacity(raw.len());
    result.push_str(opening);
    result.push_str(&new_yaml);
    result.push_str(&raw[yaml_end..]); // closing `---` delimiter onward
    Some(result)
}

/// The top-level frontmatter keys of `raw`, in document order. Used to
/// derive a template's canonical key order (an empty vec when there's no
/// frontmatter block — the reorderer then treats every key as unknown
/// and leaves the note untouched).
fn frontmatter_key_order(raw: &str) -> Vec<String> {
    let opening = "---\n";
    if !raw.starts_with(opening) {
        return Vec::new();
    }
    let after_open = opening.len();
    let Some(rel) = raw[after_open..].find("\n---") else {
        return Vec::new();
    };
    let yaml = &raw[after_open..after_open + rel + 1];
    yaml.split_inclusive('\n')
        .filter_map(|line| top_level_key(line).map(str::to_owned))
        .collect()
}

/// The top-level YAML key a line declares, or `None` when the line is a
/// continuation (indented / list item / blank / comment / no colon).
fn top_level_key(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None; // indentation -> continuation
    }
    let key = line.split(':').next()?.trim_end();
    if key.is_empty() || key.contains(char::is_whitespace) || !line.contains(':') {
        return None;
    }
    Some(key)
}
