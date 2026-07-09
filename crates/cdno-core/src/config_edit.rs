//! Surgical, comment-preserving edits to `.cuaderno/config.toml` (#365,
//! PR5b) — the write half of the desktop Config *form*.
//!
//! The Config save seam is **string-in, string-out**: the domain's
//! `save_config_raw` validates a candidate buffer and, if it passes,
//! writes it *verbatim*. The form must therefore hand that gate a
//! candidate string that differs from the current one in exactly the one
//! table the user touched — comments, key order, the `[variables]` block,
//! and every other note type / schema left byte-for-byte intact. A naive
//! `toml::to_string(&VaultConfig)` would re-serialise the whole document
//! and *drop every comment and all ordering* — it fights the seam. So
//! these functions edit the parsed `toml_edit::DocumentMut` in place,
//! mutating only the one dotted table (`[note_types.<name>]` or
//! `[schemas.<type>.fields.<field>]`) and returning the re-rendered
//! string.
//!
//! **No validation happens here.** These writers are deliberately dumb:
//! they produce a candidate string, and the save gate
//! (`Vault::save_config_raw` -> `validate_config_str`) remains the single
//! authority on whether that candidate may persist. A form pre-check may
//! block obviously-bad input for a calmer UX, but the server error is
//! always the one that decides.
//!
//! One caveat to "comments preserved": a comment attached to a key the
//! edit REMOVES goes with it — a `# why this default` line above a
//! `default` the form clears has no surviving key to anchor to, so it is
//! dropped. Comments on untouched keys, on the table header, and on every
//! other table are kept. This is inherent to surgical key removal, not a
//! bug; a form that only ever sets keys never hits it.
//!
//! Minimal keys by design: a `set_*` writes only the keys that carry
//! meaning (`folder` always; `required`/`optional`/`values` only when
//! non-empty; `template`/`title_field`/`date_field`/`default` only when
//! `Some`; `append_only`/`required` only when `true`) and *removes* a key
//! the current model no longer sets. The reserved field-spec keys
//! (`list`, `settable`, `log_on_change`) are never written and never
//! touched, so a hand-set reserved key survives a form edit of the same
//! field.

use toml_edit::{Array, DocumentMut, Item, Table, value};

use crate::config::{CustomNoteType, FieldSpec};
use crate::error::ConfigEditError;

/// Parse a config buffer into an editable document, mapping a syntax
/// error to [`ConfigEditError::Parse`] (which carries `toml_edit`'s own
/// line/column-bearing message).
fn parse(content: &str) -> Result<DocumentMut, ConfigEditError> {
    content
        .parse::<DocumentMut>()
        .map_err(|err| ConfigEditError::Parse(err.to_string()))
}

/// Borrow the child table at `key` under `parent`, creating an empty one
/// if it is absent. A newly created intermediate table is marked
/// `implicit` when `implicit_if_new` is set, so only the leaf table that
/// actually carries keys renders a `[header]` (an implicit ancestor
/// suppresses its own header). An existing table is returned untouched —
/// crucially, its `implicit` flag is left as-is, so a real
/// `[schemas.<type>]` that already holds `extra_required` keeps its
/// header. A key that exists but is not a table is a hard
/// [`ConfigEditError::NotATable`]: overwriting it would silently drop the
/// user's value.
fn table_entry<'a>(
    parent: &'a mut Table,
    key: &str,
    implicit_if_new: bool,
) -> Result<&'a mut Table, ConfigEditError> {
    // A key that is present and is neither a table nor a null placeholder
    // is the wrong shape — refuse rather than clobber.
    if parent
        .get(key)
        .is_some_and(|item| !item.is_none() && !item.is_table())
    {
        return Err(ConfigEditError::NotATable(key.to_string()));
    }
    // Absent (or a null placeholder) — vivify an empty table.
    if parent.get(key).map(Item::is_none).unwrap_or(true) {
        let mut table = Table::new();
        if implicit_if_new {
            table.set_implicit(true);
        }
        parent.insert(key, Item::Table(table));
    }
    Ok(parent
        .get_mut(key)
        .and_then(Item::as_table_mut)
        .expect("entry was just inserted or verified as a table"))
}

/// Set `key` to a TOML string array when `items` is non-empty; remove it
/// otherwise. The "remove when empty" arm is what lets an edit that
/// clears a note type's `required` list drop the key rather than leave an
/// empty `[]` behind.
fn set_or_remove_string_array(table: &mut Table, key: &str, items: &[String]) {
    if items.is_empty() {
        table.remove(key);
    } else {
        let array: Array = items.iter().map(String::as_str).collect();
        table.insert(key, value(array));
    }
}

/// Set `key` to a TOML string when `opt` is `Some`; remove it when `None`
/// — the scalar analogue of [`set_or_remove_string_array`].
fn set_or_remove_opt_string(table: &mut Table, key: &str, opt: &Option<String>) {
    match opt {
        Some(text) => {
            table.insert(key, value(text.as_str()));
        }
        None => {
            table.remove(key);
        }
    }
}

/// Map a `toml::Value` default (as parsed from the model) into a
/// `toml_edit::Item` for surgical insertion. The realistic cases are the
/// four scalars a field default may hold (`bool`/`int`/`string`, and a
/// `date` authored as a quoted `YYYY-MM-DD` string). A bare TOML datetime
/// is rendered back as its text, and any non-scalar (array/table) — which
/// the save gate's `validate_schemas` rejects before it could ever
/// persist — is stringified defensively, so this mapping stays total
/// rather than panicking on an unreachable shape.
fn default_item(default: &toml::Value) -> Item {
    match default {
        toml::Value::String(s) => value(s.as_str()),
        toml::Value::Integer(i) => value(*i),
        toml::Value::Float(f) => value(*f),
        toml::Value::Boolean(b) => value(*b),
        toml::Value::Datetime(dt) => value(dt.to_string()),
        other => value(other.to_string()),
    }
}

/// Insert or replace `[note_types.<name>]`, writing only the meaningful
/// keys and removing any the model no longer sets. Every other table,
/// comment, and the key's own header comment are preserved; a from-scratch
/// insert appends a fresh `[note_types.<name>]` table. Returns the
/// re-rendered document — the candidate string the save gate validates.
pub fn set_note_type(
    content: &str,
    name: &str,
    note_type: &CustomNoteType,
) -> Result<String, ConfigEditError> {
    let mut doc = parse(content)?;
    let note_types = table_entry(doc.as_table_mut(), "note_types", true)?;
    let table = table_entry(note_types, name, false)?;

    table.insert("folder", value(note_type.folder.as_str()));
    set_or_remove_string_array(table, "required", &note_type.required);
    set_or_remove_string_array(table, "optional", &note_type.optional);
    set_or_remove_opt_string(table, "template", &note_type.template);
    if note_type.append_only {
        table.insert("append_only", value(true));
    } else {
        table.remove("append_only");
    }
    set_or_remove_opt_string(table, "title_field", &note_type.title_field);
    set_or_remove_opt_string(table, "date_field", &note_type.date_field);

    Ok(doc.to_string())
}

/// Remove `[note_types.<name>]` if present. Idempotent: removing an absent
/// type (or when `[note_types]` isn't even a table) is a no-op success, so
/// a double-remove or a stale delete never errors. Every other table and
/// comment is preserved.
pub fn remove_note_type(content: &str, name: &str) -> Result<String, ConfigEditError> {
    let mut doc = parse(content)?;
    if let Some(note_types) = doc
        .as_table_mut()
        .get_mut("note_types")
        .and_then(Item::as_table_mut)
    {
        note_types.remove(name);
    }
    Ok(doc.to_string())
}

/// Insert or replace `[schemas.<note_type>.fields.<field>]`, writing only
/// the form-controlled keys (`type` always; `default`/`required`/`values`
/// per the spec) and removing those the model no longer sets. The reserved
/// keys `list`/`settable`/`log_on_change` are deliberately left untouched,
/// so a hand-authored reserved key on the same field survives the edit.
/// Sibling fields, the schema's `extra_required`, and every other table and
/// comment are preserved. Returns the re-rendered candidate string.
pub fn set_schema_field(
    content: &str,
    note_type: &str,
    field: &str,
    spec: &FieldSpec,
) -> Result<String, ConfigEditError> {
    let mut doc = parse(content)?;
    let schemas = table_entry(doc.as_table_mut(), "schemas", true)?;
    let ty_table = table_entry(schemas, note_type, true)?;
    let fields = table_entry(ty_table, "fields", true)?;
    let table = table_entry(fields, field, false)?;

    table.insert("type", value(spec.ty.as_str()));
    match &spec.default {
        Some(default) => {
            table.insert("default", default_item(default));
        }
        None => {
            table.remove("default");
        }
    }
    if spec.required {
        table.insert("required", value(true));
    } else {
        table.remove("required");
    }
    set_or_remove_string_array(table, "values", spec.values.as_deref().unwrap_or(&[]));

    Ok(doc.to_string())
}

/// Remove `[schemas.<note_type>.fields.<field>]` if present. Idempotent,
/// like [`remove_note_type`]: an absent field (or a missing schema /
/// `fields` table) is a no-op success. Only the one field table is
/// dropped; the schema's other fields, its `extra_required`, and every
/// other table and comment are preserved.
pub fn remove_schema_field(
    content: &str,
    note_type: &str,
    field: &str,
) -> Result<String, ConfigEditError> {
    let mut doc = parse(content)?;
    let fields = doc
        .as_table_mut()
        .get_mut("schemas")
        .and_then(Item::as_table_mut)
        .and_then(|schemas| schemas.get_mut(note_type))
        .and_then(Item::as_table_mut)
        .and_then(|ty_table| ty_table.get_mut("fields"))
        .and_then(Item::as_table_mut);
    if let Some(fields) = fields {
        fields.remove(field);
    }
    Ok(doc.to_string())
}
