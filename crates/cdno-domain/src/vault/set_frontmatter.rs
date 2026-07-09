//! `set_frontmatter`: the generic, schema-driven frontmatter setter (#301).
//!
//! Toggling a typed frontmatter flag (the daily `meds`/`workout`/`closed`
//! fields, a project's custom `phase`, …) used to force a hand-edit — which
//! desyncs `.cuaderno/index.db`, the one thing the design forbids. This
//! operation writes the field through the same single-transaction
//! read-modify-write every lifecycle mutation uses, so the file and its index
//! row commit together and can never drift.
//!
//! It is deliberately generic-but-validated: nothing about a specific field is
//! baked into the engine. The vault's `[schemas.<type>.fields.<name>]` config
//! declares which fields exist, their types, and whether they may be set — this
//! operation only enforces that declared contract.

use chrono::{NaiveDate, NaiveDateTime};
use serde_json::Value;

use cdno_core::config::FieldType;
use cdno_core::error::StoreError;
use cdno_core::frontmatter::Frontmatter;
use cdno_core::path::VaultPath;

use crate::error::DomainError;
use crate::note_type::NoteType;
use crate::type_registry::hard_reserved_fields;

use super::Vault;
use super::WriteOutcome;
use super::index_entry::build_index_entry_for;
use super::log::daily_note_path;
use super::projects::rewrite_field_in_frontmatter;

impl Vault {
    /// Set a single typed frontmatter field on a note, keeping the SQLite
    /// index in lock-step with the file (#301).
    ///
    /// `note` resolves to a file (see [`resolve_frontmatter_note`]); `key` is a
    /// field declared under `[schemas.<type>.fields.<key>]` and marked
    /// `settable = true`; `value` is the new value as a string, coerced to the
    /// field's declared type.
    ///
    /// The write is validated hard before it lands:
    /// - the field must be **declared** in the note type's schema
    ///   ([`DomainError::UndeclaredSchemaField`]);
    /// - it must be explicitly **settable** — default-deny, so an absent or
    ///   `false` `settable` rejects ([`DomainError::FieldNotSettable`]);
    /// - it must not be an engine-**reserved** key (`type`, `status`, or the
    ///   calendar period key), regardless of config
    ///   ([`DomainError::ReservedSchemaField`]) — the lifecycle tools stay the
    ///   sole writers of those, so their auto-logging and index invariants are
    ///   never bypassed;
    /// - the coerced value must **type-check** against the declared type and
    ///   any `values` constraint ([`DomainError::InvalidFieldValue`]).
    ///
    /// When the new value equals the current one the call is a silent no-op:
    /// nothing is written or logged and the returned [`WriteOutcome`] carries an
    /// empty `paths` (mirrors `update_project_state`). When
    /// `log_on_change = true` and the value did change, a `key: old → new`
    /// line is stamped into today's daily note in the same commit.
    ///
    /// Strict-exists contract (v1): a key absent from the note's frontmatter
    /// errors ([`DomainError::MissingFrontmatterField`]) rather than being
    /// appended — the daily flags exist via the template default, so this holds
    /// for the intended use. Ordered-insert of a missing key is a follow-up.
    ///
    /// `at` is a parameter so tests can pin the log timestamp and the
    /// daily-note date; production callers pass
    /// `chrono::Local::now().naive_local()`.
    pub fn set_frontmatter(
        &self,
        at: NaiveDateTime,
        note: &str,
        key: &str,
        value: &str,
    ) -> Result<WriteOutcome, DomainError> {
        // Acquire the write lock up front so the whole read-modify-write
        // serialises against other writers (#196) — the same seam as
        // `update_project_state` / `park_project`.
        let mut tx = self.transaction()?;

        // 1. Resolve the note reference to an existing file, then read it.
        let path = self.resolve_frontmatter_note(note, at.date())?;
        let raw = self.store.read_file(&path)?;
        let (fm, _body) = Frontmatter::parse(&raw)?;

        // 2. Determine the note's type: the reconciled index row is
        //    authoritative; fall back to the `type:` frontmatter marker.
        let note_type = self.note_type_of(&fm, &path)?;

        // 3. Validate against the declared schema. The field must be declared
        //    under `[schemas.<type>.fields]` (or the desugared `extra_required`
        //    view) and explicitly `settable = true`. Default-deny: an absent or
        //    `false` `settable` rejects, so a field is only ever writable when
        //    the vault opted it in.
        let declared = self
            .config
            .schema_for(&note_type)
            .map(|s| s.declared_fields())
            .unwrap_or_default();
        let Some(spec) = declared.get(key) else {
            return Err(DomainError::UndeclaredSchemaField {
                note_type,
                field: key.to_owned(),
            });
        };
        if spec.settable != Some(true) {
            return Err(DomainError::FieldNotSettable {
                note_type,
                field: key.to_owned(),
            });
        }

        // 4. Reserved block — stricter than the create-time check. Beyond the
        //    per-type hard-reserved set (`type` plus the calendar period key
        //    `date`/`week`/`month`), `status` is reserved for *every* type so
        //    the lifecycle tools (`park_project`, `set_question_status`, …) stay
        //    the sole writers of state, carrying the auto-logging and index
        //    invariants a raw field-set would bypass. Enforced here regardless
        //    of config, even if a vault declared the field `settable`.
        if is_reserved_key(&note_type, key) {
            return Err(DomainError::ReservedSchemaField {
                note_type,
                field: key.to_owned(),
            });
        }

        // 5. Coerce the incoming string into a JSON value of the declared type,
        //    then type-check it (enum membership, calendar-date validity) via
        //    the canonical `FieldSpec::check_value` the lint layer also uses. A
        //    parse failure (e.g. `meds=maybe` for a bool) or a failed
        //    constraint is an `InvalidFieldValue`.
        let new_json = coerce_value(key, &note_type, spec.ty, value)?;
        if let Some(reason) = spec.check_value(&new_json) {
            return Err(DomainError::InvalidFieldValue {
                note_type,
                field: key.to_owned(),
                reason,
            });
        }
        // Serialise back to the scalar the frontmatter line carries. `bool`/
        // `int`/`date` stay bare (`meds: true`, `count: 3`, `when: 2026-07-09`);
        // a `string` (incl. an enum value) is emitted YAML-safe so a bareword
        // like `true`/`null`/a number, or a value containing `:`/`#`/a leading
        // `-`, is quoted rather than silently re-parsed as a non-string on the
        // index rebuild.
        let new_scalar = write_scalar(&new_json, spec.ty);

        // 6. No-change → silent no-op. Compare the coerced value against the
        //    note's current frontmatter value; if equal, write nothing and log
        //    nothing (mirrors `update_project_state`). `paths` stays empty so
        //    the desktop echo journal skips it (#315).
        let old_value = fm.as_json().get(key).cloned();
        if old_value.as_ref() == Some(&new_json) {
            return Ok(WriteOutcome::noop(path));
        }

        // 7. Rewrite the single field line in place, preserving key order,
        //    comments, and the body. Strict-exists: a key absent from the
        //    frontmatter errors here rather than being appended out of order.
        let new_content = rewrite_field_in_frontmatter(&raw, key, &new_scalar)?;

        // 8. Optionally auto-log, then commit. `log_on_change = true` stamps a
        //    `key: old → new` line into today's daily note.
        if spec.log_on_change == Some(true) {
            let log_entry =
                format_field_change_log_entry(&path, key, old_value.as_ref(), &new_json);
            let today_daily = daily_note_path(at.date())?;
            if today_daily == path {
                // The field lives on *today's* daily note. `stage_daily_log`
                // would re-read the pre-change file from the store and clobber
                // this field edit, so instead fold the log line into the same
                // in-flight content and write the file exactly once. This is
                // the index-consistency seam: one file write, one index row, no
                // self-clobber.
                let folded = self.fold_daily_log_line(at.time(), new_content, &log_entry)?;
                let entry_meta = build_index_entry_for(&path, &folded, &note_type)?;
                tx.write_file(path.clone(), folded);
                tx.upsert_note(entry_meta);
            } else {
                // The field lives elsewhere (a project note, a past daily): the
                // field edit and the daily-log write target different paths, so
                // stage them independently — no read-after-write hazard.
                let entry_meta = build_index_entry_for(&path, &new_content, &note_type)?;
                tx.write_file(path.clone(), new_content);
                tx.upsert_note(entry_meta);
                self.stage_daily_log(at, &log_entry, &mut tx)?;
            }
        } else {
            let entry_meta = build_index_entry_for(&path, &new_content, &note_type)?;
            tx.write_file(path.clone(), new_content);
            tx.upsert_note(entry_meta);
        }

        let touched = tx.commit()?;
        Ok(WriteOutcome::written(path, touched))
    }

    /// Resolve the `note` reference to an existing vault file. `today` and a
    /// bare `YYYY-MM-DD` map to the daily note for that date; anything else is
    /// treated as a vault-relative path. Errors [`StoreError::NotFound`] when
    /// the resolved file is absent.
    ///
    /// Slug resolution for projects/questions is a deferred follow-up — v1
    /// covers the daily loop (`today`/date) and explicit note paths, which is
    /// the toggle-a-daily-flag case the setter exists for.
    fn resolve_frontmatter_note(
        &self,
        note: &str,
        today: NaiveDate,
    ) -> Result<VaultPath, DomainError> {
        let path = if note == "today" {
            daily_note_path(today)?
        } else if let Ok(date) = NaiveDate::parse_from_str(note, "%Y-%m-%d") {
            daily_note_path(date)?
        } else {
            VaultPath::new(note)?
        };
        if !self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::NotFound(path.to_string())));
        }
        Ok(path)
    }

    /// The note's declared type. The reconciled index row is authoritative
    /// (reconciliation runs at [`Vault::new`], so an on-disk note has a row);
    /// the `type:` frontmatter marker is the fallback.
    fn note_type_of(&self, fm: &Frontmatter, path: &VaultPath) -> Result<String, DomainError> {
        if let Some(entry) = self.index.find_by_path(path)? {
            return Ok(entry.note_type);
        }
        Ok(fm.require_field::<String>("type")?)
    }
}

/// Whether `key` is engine-owned for `note_type` and therefore never settable
/// through this generic path. The set is the per-type hard-reserved fields
/// (`type` plus the calendar period key) unioned with `status`, which the
/// lifecycle tools own for every type.
fn is_reserved_key(note_type: &str, key: &str) -> bool {
    if key == "type" || key == "status" {
        return true;
    }
    // A config-defined custom type has no built-in period key; only the
    // universal `type`/`status` above apply to it.
    if let Ok(nt) = note_type.parse::<NoteType>() {
        return hard_reserved_fields(nt).contains(&key);
    }
    false
}

/// Coerce the incoming `value` string into a `serde_json::Value` of the field's
/// declared `ty`. `bool`/`int` parse strictly (a parse failure *is* the type
/// mismatch); `string`/`date` carry the text verbatim for `check_value` to
/// validate (enum membership, calendar-date validity).
fn coerce_value(
    field: &str,
    note_type: &str,
    ty: FieldType,
    value: &str,
) -> Result<Value, DomainError> {
    let invalid = |reason: &str| DomainError::InvalidFieldValue {
        note_type: note_type.to_owned(),
        field: field.to_owned(),
        reason: reason.to_owned(),
    };
    match ty {
        FieldType::Bool => value
            .parse::<bool>()
            .map(Value::Bool)
            .map_err(|_| invalid("is not a valid bool")),
        FieldType::Int => value
            .parse::<i64>()
            .map(Value::from)
            .map_err(|_| invalid("is not a valid int")),
        FieldType::String | FieldType::Date => Ok(Value::String(value.to_owned())),
    }
}

/// Render a coerced scalar back to the frontmatter text for the field's
/// declared type. `bool`/`int`/`date` are written **bare** (`meds: true`,
/// `count: 3`, `when: 2026-07-09` — the date has already been validated as a
/// real `YYYY-MM-DD`, so it is safe unquoted). A `string` (including an enum
/// value) is written as a YAML-safe scalar via [`yaml_string_scalar`], so a
/// bareword or a value carrying special characters is quoted rather than
/// re-parsed as a non-string on the index rebuild.
fn write_scalar(value: &Value, ty: FieldType) -> String {
    match ty {
        FieldType::Bool | FieldType::Int | FieldType::Date => display_value(value),
        FieldType::String => yaml_string_scalar(value),
    }
}

/// A plain, human-readable rendering of a scalar JSON value: bool → `true`/
/// `false`, number → digits, string → its text verbatim (no quoting). Used for
/// the bare types' write-back and for the daily-log display.
fn display_value(value: &Value) -> String {
    match value {
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        // Coercion only ever produces the scalar shapes above; a non-scalar
        // here would be a bug, so stringify defensively.
        other => other.to_string(),
    }
}

/// Emit a `string`/enum value as a single YAML scalar, quoting it only when the
/// YAML emitter needs to (a bareword like `true`/`false`/`null`/a number, or a
/// value containing `:`/`#`/a leading `-`/quotes). Delegating to `serde_yaml`
/// covers every quoting edge case rather than hand-rolling the rules; the
/// emitter's trailing newline is stripped so the result is one line.
fn yaml_string_scalar(value: &Value) -> String {
    let s = match value {
        Value::String(s) => s.as_str(),
        // `coerce_value` only produces `Value::String` for a String/Date field,
        // and `write_scalar` routes non-strings elsewhere; defensive fallback.
        other => return display_value(other),
    };
    serde_yaml::to_string(&s)
        .map(|y| y.trim_end_matches('\n').to_owned())
        .unwrap_or_else(|_| s.to_owned())
}

/// Build the daily-log line recording a field change: `key: old → new on
/// [[note]]`. The note is wikilinked by its vault path (sans `.md`) so the link
/// resolves from the daily note; a previously-unset or `null` value renders as
/// `(unset)`. Both values use the plain [`display_value`] form (not the
/// YAML-quoted write-back), so the log reads naturally.
fn format_field_change_log_entry(
    path: &VaultPath,
    key: &str,
    old: Option<&Value>,
    new: &Value,
) -> String {
    let old_display = match old {
        None | Some(Value::Null) => "(unset)".to_owned(),
        Some(v) => display_value(v),
    };
    let new_display = display_value(new);
    let link = path.to_string();
    let link = link.strip_suffix(".md").unwrap_or(&link);
    format!("{key}: {old_display} \u{2192} {new_display} on [[{link}]]")
}
