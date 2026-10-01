//! `Vault::sync_template` (#699): bring a custom override of a built-in
//! template up to date with frontmatter keys a later release added to the
//! built-in, without touching anything the user wrote.
//!
//! A vault that customised `.cuaderno/templates/<type>.md` never receives
//! keys added to the built-in afterwards (`closed:` on `project`, RFC 0004,
//! was the first). Nothing breaks: notes made from the override lack the
//! key, as notes written before it existed do. But nothing said the
//! override had fallen behind either, and the only way to catch up was a
//! hand edit or `eject --force`, which throws the customisation away.
//!
//! The comparison is on the template TEXT, not on parsed YAML: templates
//! carry `{{placeholders}}` that are not valid YAML values, and the point
//! is to insert the built-in line verbatim (`closed: null`,
//! `core_question: {{core_question}}`) and leave every other byte alone.

use std::str::FromStr;

use serde::Serialize;

use crate::error::DomainError;
use crate::note_type::NoteType;

use super::Vault;
use super::templating::{builtin_defaults, template_path};

/// Where one custom override stands against its built-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TemplateSyncStatus {
    /// No custom override: the built-in is in effect, nothing to sync.
    NotCustomised,
    /// The override has every built-in key.
    UpToDate,
    /// Keys are missing and were not written (`--check`).
    Behind,
    /// Keys were missing and have been added.
    Synced,
    /// The override has no `---` frontmatter block, so there is nowhere to
    /// put a key. Reported, never rewritten.
    NoFrontmatter,
}

/// The outcome of syncing one built-in type's custom override.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TemplateSyncReport {
    pub note_type: String,
    /// Vault-relative path of the override (whether or not it exists).
    pub path: String,
    pub status: TemplateSyncStatus,
    /// Built-in keys the override lacked, in built-in order: added when
    /// `status` is `Synced`, missing when it is `Behind`.
    pub added: Vec<String>,
    /// The override's own keys that the built-in does not have. Kept as
    /// they are; listed so a reader sees nothing was dropped.
    pub kept: Vec<String>,
}

impl Vault {
    /// Compare the custom override of built-in `note_type` with the
    /// built-in template and, when `write`, insert each missing built-in
    /// frontmatter line verbatim (#699). Each goes right after the nearest
    /// preceding built-in key the override has, else right before the
    /// nearest following one, else last in the frontmatter. Nothing else
    /// changes: the body, the user's own keys, their values and their order
    /// stay byte for byte, and no key is ever removed.
    ///
    /// Writes through [`Vault::save_template`], the path `cdno templates
    /// save` uses, so this adds no raw-write path of its own. Only built-in
    /// types sync; a config custom type has no bundled template to compare
    /// with ([`DomainError::UnknownNoteType`]).
    pub fn sync_template(
        &self,
        note_type: &str,
        write: bool,
    ) -> Result<TemplateSyncReport, DomainError> {
        let nt = NoteType::from_str(note_type).map_err(|_| DomainError::UnknownNoteType {
            note_type: note_type.to_owned(),
        })?;
        let key = nt.as_str();
        let path = template_path(&format!("{key}.md"))?;
        let mut report = TemplateSyncReport {
            note_type: key.to_owned(),
            path: path.to_string(),
            status: TemplateSyncStatus::NotCustomised,
            added: Vec::new(),
            kept: Vec::new(),
        };
        if !self.store.exists(&path)? {
            return Ok(report);
        }
        let custom = self.store.read_file(&path)?;
        let builtin = builtin_template(nt);
        let Some(outcome) = sync_text(builtin, &custom) else {
            report.status = TemplateSyncStatus::NoFrontmatter;
            return Ok(report);
        };
        report.kept = outcome.kept;
        if outcome.added.is_empty() {
            report.status = TemplateSyncStatus::UpToDate;
            return Ok(report);
        }
        report.added = outcome.added;
        if write {
            self.save_template(key, None, &outcome.text)?;
            report.status = TemplateSyncStatus::Synced;
        } else {
            report.status = TemplateSyncStatus::Behind;
        }
        Ok(report)
    }

    /// [`Vault::sync_template`] over every built-in type that has a custom
    /// override, in [`NoteType::ALL`] order. Types with no override are left
    /// out: there is nothing of theirs to sync.
    pub fn sync_all_templates(&self, write: bool) -> Result<Vec<TemplateSyncReport>, DomainError> {
        let mut out = Vec::new();
        for nt in NoteType::ALL {
            let report = self.sync_template(nt.as_str(), write)?;
            if report.status != TemplateSyncStatus::NotCustomised {
                out.push(report);
            }
        }
        Ok(out)
    }

    /// The built-in keys the custom override of `nt` lacks, in built-in
    /// order; empty when there is no override, it is up to date, or it has
    /// no frontmatter. Read-only, for `templates list` and lint.
    pub(in crate::vault) fn stale_template_keys(
        &self,
        nt: NoteType,
    ) -> Result<Vec<String>, DomainError> {
        Ok(self.sync_template(nt.as_str(), false)?.added)
    }
}

/// The built-in template text for `nt`. Every built-in type ships one.
fn builtin_template(nt: NoteType) -> &'static str {
    builtin_defaults()
        .get(nt.as_str())
        .copied()
        .expect("every built-in note type has a built-in template")
}

/// What [`sync_text`] produced.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SyncedText {
    /// The override with every missing built-in line inserted; equal to the
    /// input when nothing was missing.
    pub text: String,
    /// Built-in keys that were missing, in built-in order.
    pub added: Vec<String>,
    /// Override keys the built-in does not have.
    pub kept: Vec<String>,
}

/// Insert every frontmatter key of `builtin` that `custom` lacks into
/// `custom`, verbatim, and report what was added and which of `custom`'s
/// keys the built-in lacks. `None` when `custom` has no frontmatter block.
pub(crate) fn sync_text(builtin: &str, custom: &str) -> Option<SyncedText> {
    let builtin_fm = FrontmatterLines::parse(builtin)?;
    let mut lines: Vec<String> = custom.split_inclusive('\n').map(str::to_owned).collect();
    let mut fm = FrontmatterLines::parse(custom)?;
    let crlf = custom.contains("\r\n");

    let builtin_keys = builtin_fm.keys();
    let kept: Vec<String> = fm
        .keys()
        .into_iter()
        .filter(|k| !builtin_keys.contains(k))
        .collect();

    let mut added = Vec::new();
    for (i, key) in builtin_keys.iter().enumerate() {
        if fm.position_of(key).is_some() {
            continue;
        }
        // After the nearest preceding built-in key the override has, else
        // before the nearest following one, else last.
        let insert_at = builtin_keys[..i]
            .iter()
            .rev()
            .find_map(|k| fm.block_end_of(k))
            .or_else(|| {
                builtin_keys[i + 1..]
                    .iter()
                    .find_map(|k| fm.position_of(k).map(|(line, _)| line))
            })
            .unwrap_or(fm.close);
        // Every insertion point is at or before the closing `---`, so the
        // line before it always ends in a newline and the block cannot run
        // into it.
        let block = builtin_fm.block_lines(key, crlf);
        let count = block.len();
        for (offset, line) in block.into_iter().enumerate() {
            lines.insert(insert_at + offset, line);
        }
        fm.insert(insert_at, key, count);
        added.push(key.clone());
    }

    Some(SyncedText {
        text: lines.concat(),
        added,
        kept,
    })
}

/// The frontmatter of a template, as line indices into its text split
/// with `split_inclusive('\n')`.
struct FrontmatterLines {
    /// Every line of the text, newline included.
    lines: Vec<String>,
    /// Index of the closing `---` line.
    close: usize,
    /// Each top-level key with the index of its line, in order.
    keys: Vec<(String, usize)>,
}

impl FrontmatterLines {
    /// The frontmatter between a first line `---` and the next `---` line.
    /// `None` when either is missing.
    fn parse(text: &str) -> Option<Self> {
        let lines: Vec<String> = text.split_inclusive('\n').map(str::to_owned).collect();
        if lines.first().map(|l| l.trim_end()) != Some("---") {
            return None;
        }
        let close = lines
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, l)| l.trim_end() == "---")
            .map(|(i, _)| i)?;
        let keys = (1..close)
            .filter_map(|i| top_level_key(&lines[i]).map(|k| (k.to_owned(), i)))
            .collect();
        Some(Self { lines, close, keys })
    }

    fn keys(&self) -> Vec<String> {
        self.keys.iter().map(|(k, _)| k.clone()).collect()
    }

    /// The line of `key` and its index among the keys.
    fn position_of(&self, key: &str) -> Option<(usize, usize)> {
        self.keys
            .iter()
            .enumerate()
            .find(|(_, (k, _))| k == key)
            .map(|(idx, (_, line))| (*line, idx))
    }

    /// The line after `key`'s block: its own line plus the indented or
    /// list continuation lines up to the next key or the closing `---`.
    fn block_end_of(&self, key: &str) -> Option<usize> {
        let (_, idx) = self.position_of(key)?;
        Some(
            self.keys
                .get(idx + 1)
                .map(|(_, line)| *line)
                .unwrap_or(self.close),
        )
    }

    /// `key`'s block, line by line, with line endings matched to the target
    /// file (`\r\n` when `crlf`).
    fn block_lines(&self, key: &str, crlf: bool) -> Vec<String> {
        let (start, _) = self
            .position_of(key)
            .expect("key comes from this frontmatter");
        let end = self
            .block_end_of(key)
            .expect("key comes from this frontmatter");
        self.lines[start..end]
            .iter()
            .map(|l| {
                let body = l.trim_end_matches(['\r', '\n']);
                format!("{body}{}", if crlf { "\r\n" } else { "\n" })
            })
            .collect()
    }

    /// Record that `count` lines for `key` were inserted at line `at`.
    fn insert(&mut self, at: usize, key: &str, count: usize) {
        for (_, line) in &mut self.keys {
            if *line >= at {
                *line += count;
            }
        }
        if self.close >= at {
            self.close += count;
        }
        let idx = self
            .keys
            .iter()
            .position(|(_, line)| *line > at)
            .unwrap_or(self.keys.len());
        self.keys.insert(idx, (key.to_owned(), at));
    }
}

/// The key of a top-level frontmatter line (`key: value` at column 0), or
/// `None` for an indented, list, comment or blank line.
fn top_level_key(line: &str) -> Option<&str> {
    let first = line.chars().next()?;
    if !(first.is_ascii_alphanumeric() || first == '_') {
        return None;
    }
    let (key, _) = line.split_once(':')?;
    key.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        .then_some(key)
}
