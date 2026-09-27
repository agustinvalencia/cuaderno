//! The generic create/list path for config-defined custom note types.
//!
//! Built-in types each have a bespoke `create_*` carrying their behaviour;
//! a custom type has none, so this one generic path serves them all: validate
//! the supplied fields against the type's declared schema, render its template
//! (or synthesise a minimal note when it ships none), and write it. Modelled on
//! [`create_question_with_vars`](Vault::create_question_with_vars).

use std::collections::HashMap;

use chrono::NaiveDateTime;

use cdno_core::error::StoreError;
use cdno_core::frontmatter::split_frontmatter;
use cdno_core::path::VaultPath;
use cdno_core::template::VariableContext;

use super::Vault;
use super::index_entry::build_index_entry_for;
use super::slug::slugify;
use super::templating::custom_template_filename;
use crate::error::DomainError;

/// The contextual placeholder a custom template uses for the note's body.
const BODY: &str = "body";

/// The frontmatter field the `origin` parameter is written to.
const ORIGIN: &str = "origin";

/// The contextual placeholders the create path fills itself, before any
/// caller-supplied `fields`: `{{title}}`, `{{slug}}`, `{{created}}`,
/// `{{date}}` (`created` and `date` share one value — the creation date).
/// A declared `required` field whose name is in this set is satisfied by
/// that engine value even when the caller supplies nothing for it — the
/// engine, not the caller, is the one who "supplies" it, so demanding it
/// in `fields` too would ask the caller to repeat what create already
/// knows. A `required` field outside this set is unaffected: it must
/// still come from the caller.
const ENGINE_SUPPLIED_FIELDS: &[&str] = &["title", "slug", "created", "date"];

impl Vault {
    /// Create a note of a config-defined custom type `type_name`. Convenience
    /// wrapper over [`create_custom_note_with_vars`](Self::create_custom_note_with_vars)
    /// with no prompted-variable values.
    pub fn create_custom_note(
        &self,
        at: NaiveDateTime,
        type_name: &str,
        title: &str,
        fields: &HashMap<String, String>,
    ) -> Result<VaultPath, DomainError> {
        self.create_custom_note_with_vars(at, type_name, title, fields, &HashMap::new(), None, None)
    }

    /// Whether the template a custom type renders from references a
    /// `{{body}}` placeholder. `false` when the type ships no template file
    /// (the synthesised note has none). The CLI asks this to decide whether a
    /// body is something to prompt for; the create path asks it to decide
    /// between filling the placeholder and inserting the body after the H1.
    ///
    /// Errors as [`create_custom_note_with_vars`](Self::create_custom_note_with_vars)
    /// does for a type that is unknown or built-in.
    pub fn custom_template_has_body(&self, type_name: &str) -> Result<bool, DomainError> {
        let registry = self.type_registry();
        let Some(descriptor) = registry.resolve(type_name) else {
            return Err(DomainError::UnknownNoteType {
                note_type: type_name.to_owned(),
            });
        };
        let Some(def) = descriptor.as_custom() else {
            return Err(DomainError::BuiltinTypeNotCustom {
                note_type: type_name.to_owned(),
            });
        };
        self.custom_template_references(&custom_template_filename(type_name, def), BODY)
    }

    /// Create a note of a config-defined custom type, with caller-supplied
    /// prompted-variable values (`[variables.prompt]`).
    ///
    /// The note is written to `<folder>/<slug(title)>.md`, its frontmatter shaped
    /// by the type's declared fields. `fields` maps frontmatter field → value;
    /// every key must be a declared `required`/`optional` field, and every
    /// `required` field satisfies the create-time check — either supplied
    /// by the caller, or, for a name in [`ENGINE_SUPPLIED_FIELDS`] (`title`,
    /// `slug`, `created`, `date`), filled from the value the create path
    /// itself computes. A vault declaring `required = ["created"]` is
    /// declaring "stamp this", not "make the caller repeat today's date".
    /// For an engine-supplied name, a non-blank caller value still wins over
    /// the engine's own (backdating a `created` is deliberate); a blank one
    /// is ignored and the engine value is used instead.
    ///
    /// **Frontmatter reconciliation.** Whatever the template renders, the
    /// written frontmatter carries `type: <type_name>` and every non-blank
    /// caller-supplied field (`fields`, plus `origin`) carrying the supplied
    /// value: a YAML string equal to it, or a plain scalar (number, boolean,
    /// null) whose canonical text equals it — `priority: {{priority}}` with
    /// `5` stays the number `5`, `done: {{done}}` with `true` stays `true`.
    /// After rendering, the frontmatter is parsed; if it already satisfies
    /// that, the rendered text is written byte for byte. Otherwise (a missing
    /// key, a sequence or mapping, a differing string) the mapping is
    /// repaired — a wrong or missing `type` and each such field are set as
    /// strings, fields that already carry their value keep it, existing keys keep their
    /// order, missing ones are appended in declared order — and the
    /// frontmatter is re-serialised with `serde_yaml` (so `[[a]] [[b]]` is
    /// quoted), the body after it left untouched. A template that forgets a
    /// placeholder, or pastes `origin: {{origin}}` unquoted, therefore still
    /// yields the field as a string, and a template that renders no
    /// frontmatter at all gets one. A repair re-serialises the whole block,
    /// which drops the template's YAML comments and quoting style; a block
    /// that needs none is never rewritten. Engine-supplied values the
    /// caller did not pass (`created`, …) are not reconciled: a template
    /// that omits them still produces a note `cdno lint` reports.
    ///
    /// `body` (RFC 0002 §6.2) is the note's prose, without the title heading
    /// (the engine writes the H1). It is kept verbatim except that leading
    /// and trailing blank lines and trailing whitespace are stripped (a
    /// first-line indent, such as an indented code block, survives), and a
    /// first line that is an ATX H1 equal to `title` is dropped with the
    /// blank lines after it. A body that is then blank counts as absent.
    /// When the type's template references `{{body}}` the body renders
    /// there; otherwise (a template without the placeholder, or the
    /// synthesised note) it is inserted after the note's first H1, or
    /// appended when there is none, so the H1 stays the title.
    ///
    /// `{{body}}` precedence: this parameter wins; without it, the engine's
    /// normal order applies (a declared `body` field in `fields`, then a
    /// `[variables]` value, then a `[variables.prompt]` value in `prompted`,
    /// whose unanswered prompt is reported as usual); only when none of those
    /// resolves `body` does the placeholder render empty rather than
    /// literally. The body is substituted as raw text, so a `{{body}}` inside
    /// the template's frontmatter can add or break YAML keys; keep the
    /// placeholder in the note body.
    ///
    /// `origin` (RFC 0002 §5.5) is one string of wikilinks to where the note
    /// came from — promotion is create-with-`origin`. It is stored trimmed
    /// as a frontmatter string (guaranteed by the reconciliation above),
    /// through the same field map as `fields`, so a type that does not
    /// declare `origin` refuses it with [`DomainError::UnknownField`]. When
    /// `fields` also carries an `origin` key, this parameter wins. A
    /// whitespace-only origin counts as absent.
    ///
    /// Errors:
    /// - [`DomainError::UnknownNoteType`] — `type_name` isn't a config type
    ///   (built-in types have their own create paths).
    /// - [`DomainError::EmptyField`] — `title` is whitespace-only.
    /// - [`DomainError::UnknownField`] — a `fields` key isn't declared.
    /// - [`DomainError::MissingRequiredField`] — a declared `required` field is
    ///   absent or empty.
    /// - [`StoreError::AlreadyExists`] — a note with the same slug exists.
    #[allow(clippy::too_many_arguments)] // signature fixed by RFC 0002 T6: two trailing options
    pub fn create_custom_note_with_vars(
        &self,
        at: NaiveDateTime,
        type_name: &str,
        title: &str,
        fields: &HashMap<String, String>,
        prompted: &HashMap<String, String>,
        body: Option<&str>,
        origin: Option<&str>,
    ) -> Result<VaultPath, DomainError> {
        let mut tx = self.transaction()?; // lock held across the read-modify-write (#196)

        let title = title.trim();
        if title.is_empty() {
            return Err(DomainError::EmptyField { field: "title" });
        }

        // Resolve to a Custom descriptor — built-in types are handled by their
        // own bespoke create paths, not here.
        let registry = self.type_registry();
        let Some(descriptor) = registry.resolve(type_name) else {
            return Err(DomainError::UnknownNoteType {
                note_type: type_name.to_owned(),
            });
        };
        // A known built-in is a different error from an unknown type: steer the
        // user to its own create command rather than claiming it's "unknown".
        let Some(def) = descriptor.as_custom() else {
            return Err(DomainError::BuiltinTypeNotCustom {
                note_type: type_name.to_owned(),
            });
        };

        // `origin` joins the caller's field map (the parameter winning over a
        // `fields["origin"]`), so the declared-field check below refuses it
        // on a type that does not declare it, before anything is written.
        let mut fields = fields.clone();
        if let Some(origin) = origin.map(str::trim).filter(|o| !o.is_empty()) {
            fields.insert(ORIGIN.to_owned(), origin.to_owned());
        }
        let fields = &fields;
        let body = body
            .and_then(normalise_body)
            .map(|b| strip_title_h1(b, title))
            .filter(|b| !b.is_empty());

        // Every supplied field must be declared.
        for key in fields.keys() {
            if !def.required.contains(key) && !def.optional.contains(key) {
                return Err(DomainError::UnknownField {
                    note_type: type_name.to_owned(),
                    field: key.clone(),
                });
            }
        }

        // Computed ahead of the required-field check (rather than after, as
        // in the original ordering) so an engine-supplied name can be
        // resolved against its real value below.
        let date = at.date().format("%Y-%m-%d").to_string();
        // Globally-unique stem (#225) so backlinks stay resolvable.
        let slug = self.unique_slug(&slugify(title))?;

        // Every `required` field must end up with a non-empty value: the
        // caller's, or, for a name in `ENGINE_SUPPLIED_FIELDS`, the value
        // computed above.
        let engine_value = |name: &str| -> Option<&str> {
            match name {
                "title" => Some(title),
                "slug" => Some(slug.as_str()),
                "created" | "date" => Some(date.as_str()),
                _ => None,
            }
        };
        debug_assert!(
            ENGINE_SUPPLIED_FIELDS
                .iter()
                .all(|name| engine_value(name).is_some()),
            "ENGINE_SUPPLIED_FIELDS and engine_value must stay in lock-step"
        );
        for req in &def.required {
            let caller_supplied = fields.get(req).is_some_and(|v| !v.trim().is_empty());
            let engine_supplied = engine_value(req).is_some();
            if !caller_supplied && !engine_supplied {
                return Err(DomainError::MissingRequiredField {
                    note_type: type_name.to_owned(),
                    field: req.clone(),
                });
            }
        }

        let path = VaultPath::new(format!("{}/{slug}.md", def.folder))?;
        if self.store.exists(&path)? {
            return Err(DomainError::Store(StoreError::AlreadyExists(
                path.to_string(),
            )));
        }

        let mut ctx = VariableContext::new();
        ctx.set_contextual("title", title);
        ctx.set_contextual("slug", &slug);
        ctx.set_contextual("created", date.as_str());
        ctx.set_contextual("date", date.as_str());
        for (k, v) in fields {
            // A blank value for an engine-supplied name is treated as
            // absent, so the engine's own value stands rather than being
            // overwritten with blank; a non-blank value still wins, so
            // backdating `created` works. For a caller-only field a blank
            // value is already rejected above by the required-field check.
            if engine_value(k).is_some() && v.trim().is_empty() {
                continue;
            }
            ctx.set_contextual(k, v.as_str());
        }
        for (k, v) in prompted {
            ctx.set_prompted(k, v);
        }

        let template_name = custom_template_filename(type_name, def);
        // A template with a `{{body}}` placeholder takes the body there. The
        // parameter wins; otherwise the engine's own order (declared field,
        // `[variables]`, prompted) stands, and only when nothing resolves
        // `body` — and no prompt is declared for it — does it render empty
        // (the engine would otherwise leave the placeholder in literally).
        let body_in_template = self.custom_template_references(&template_name, BODY)?;
        if body_in_template {
            // `scaffold_custom` loads `[variables]` too; loading it here
            // first lets the fallback below see a vault-level `body`.
            ctx.load_from_config(self.config());
            match body {
                Some(b) => ctx.set_contextual(BODY, b),
                None if ctx.resolve(BODY).is_none()
                    && !self.config().variables.prompt.contains_key(BODY) =>
                {
                    ctx.set_contextual(BODY, "");
                }
                None => {}
            }
        }

        let field_order = descriptor
            .custom_frontmatter_order()
            .expect("a custom descriptor always yields an order");
        let content = self.scaffold_custom(type_name, &template_name, &field_order, &mut ctx)?;
        // Caller-supplied, non-blank declared fields in declared order: the
        // values the written frontmatter must carry as strings.
        let supplied: Vec<(&str, &str)> = field_order
            .iter()
            .filter(|k| k.as_str() != "type")
            .filter_map(|k| {
                fields
                    .get(k)
                    .filter(|v| !v.trim().is_empty())
                    .map(|v| (k.as_str(), v.as_str()))
            })
            .collect();
        let mut content = self.reconcile_custom_frontmatter(
            content,
            type_name,
            &template_name,
            &field_order,
            &supplied,
            &mut ctx,
        )?;
        if let Some(body) = body.filter(|_| !body_in_template) {
            content = insert_body_after_h1(&content, body);
        }

        let entry = build_index_entry_for(&path, &content, type_name)?;
        tx.write_file(path.clone(), content);
        tx.upsert_note(entry);

        // Creation is logged for every custom type (RFC 0002 §6.2, ruling
        // 3), sharing the one creation-line helper the vault already uses
        // for commitments (`vault/commitments.rs`).
        self.stage_created_line(&mut tx, at, type_name, &path, title, None)?;

        tx.commit()?;

        Ok(path)
    }

    /// Every note of a config-defined custom type, by path, sorted. Thin
    /// wrapper over the index's type filter — the generic counterpart to the
    /// built-in list queries. (Custom notes carry their title in the body H1,
    /// like built-ins, so the structured index title is not surfaced here;
    /// richer display is a later phase.)
    pub fn list_custom_notes(&self, type_name: &str) -> Result<Vec<VaultPath>, DomainError> {
        // Symmetric with the create side: `list` is for custom types only.
        match self.type_registry().resolve(type_name) {
            Some(d) if d.is_custom() => {}
            Some(_) => {
                return Err(DomainError::BuiltinTypeNotCustom {
                    note_type: type_name.to_owned(),
                });
            }
            None => {
                return Err(DomainError::UnknownNoteType {
                    note_type: type_name.to_owned(),
                });
            }
        }
        let mut paths: Vec<VaultPath> = self
            .index
            .list_by_type(type_name)?
            .into_iter()
            .map(|e| e.path)
            .collect();
        paths.sort_by_key(|p| p.to_string());
        Ok(paths)
    }
}

impl Vault {
    /// Reconcile the frontmatter of a rendered custom note (see the
    /// "Frontmatter reconciliation" paragraph on
    /// [`create_custom_note_with_vars`](Self::create_custom_note_with_vars)).
    /// Returns `rendered` unchanged when no repair is needed.
    ///
    /// When the rendered frontmatter does not parse as YAML at all (an
    /// unquoted `origin: [[a]] [[b]]`), the note is rendered once more with
    /// each supplied field replaced by an inert sentinel, so the template's
    /// own frontmatter can be parsed and repaired; the sentinels are then
    /// swapped back for the real values. If even that does not parse, the
    /// original text is returned and the index-entry build reports the
    /// parse error, as before.
    fn reconcile_custom_frontmatter(
        &self,
        rendered: String,
        type_name: &str,
        template_name: &str,
        field_order: &[String],
        supplied: &[(&str, &str)],
        ctx: &mut VariableContext,
    ) -> Result<String, DomainError> {
        let (block, body) = match split_frontmatter(&rendered) {
            Some((block, body)) => (Some(block), body),
            None => (None, rendered.as_str()),
        };
        if let Some(mapping) = parse_frontmatter_block(block.unwrap_or("")) {
            if frontmatter_is_reconciled(&mapping, type_name, supplied) {
                return Ok(rendered);
            }
            return Ok(assemble_note(
                &repair_frontmatter(mapping, type_name, supplied),
                block.is_some(),
                body,
            ));
        }

        // Unparseable: re-render with sentinels in place of the supplied
        // values. A sentinel is set only where the context currently
        // resolves the key to the supplied value, so a key another source
        // shadows (a builtin, the `body` parameter) renders as before.
        let mut swaps: Vec<(String, &str)> = Vec::new();
        for (i, (key, value)) in supplied.iter().enumerate() {
            if ctx.resolve(key) != Some(*value) {
                continue;
            }
            let sentinel = format!("cdnoreconcilesentinel{i}x");
            ctx.set_contextual(*key, sentinel.as_str());
            if ctx.resolve(key) == Some(sentinel.as_str()) {
                swaps.push((sentinel, value));
            }
        }
        let probe = self.scaffold_custom(type_name, template_name, field_order, ctx)?;
        let (probe_block, probe_body) = match split_frontmatter(&probe) {
            Some((block, body)) => (Some(block), body),
            None => (None, probe.as_str()),
        };
        let Some(mut mapping) = parse_frontmatter_block(probe_block.unwrap_or("")) else {
            return Ok(rendered);
        };
        let unswap = |s: &str| {
            swaps.iter().fold(s.to_owned(), |acc, (sentinel, value)| {
                acc.replace(sentinel.as_str(), value)
            })
        };
        for (_, v) in mapping.iter_mut() {
            if let serde_yaml::Value::String(s) = v {
                *s = unswap(s);
            }
        }
        Ok(assemble_note(
            &repair_frontmatter(mapping, type_name, supplied),
            probe_block.is_some(),
            &unswap(probe_body),
        ))
    }
}

/// Parse a frontmatter YAML block as a mapping; an empty or `null` block is
/// an empty mapping. `None` when the block is not YAML or not a mapping.
fn parse_frontmatter_block(block: &str) -> Option<serde_yaml::Mapping> {
    if block.trim().is_empty() {
        return Some(serde_yaml::Mapping::new());
    }
    match serde_yaml::from_str::<serde_yaml::Value>(block).ok()? {
        serde_yaml::Value::Mapping(m) => Some(m),
        serde_yaml::Value::Null => Some(serde_yaml::Mapping::new()),
        _ => None,
    }
}

/// Whether `mapping` holds `key` as exactly the string `value`.
fn holds_string(mapping: &serde_yaml::Mapping, key: &str, value: &str) -> bool {
    matches!(mapping.get(key), Some(serde_yaml::Value::String(s)) if s == value)
}

/// Whether a parsed frontmatter value faithfully carries the supplied
/// `value`: a string equal to it, or a plain scalar (number, boolean,
/// null) whose canonical text equals it, so `priority: {{priority}}` with
/// `5` stays the number `5`. A sequence, a mapping or a differing string
/// does not.
fn carries_supplied(value: &serde_yaml::Value, supplied: &str) -> bool {
    use serde_yaml::Value;
    match value {
        Value::String(s) => s == supplied,
        Value::Number(n) => n.to_string() == supplied,
        Value::Bool(b) => b.to_string() == supplied,
        Value::Null => supplied == "null",
        _ => false,
    }
}

fn frontmatter_is_reconciled(
    mapping: &serde_yaml::Mapping,
    type_name: &str,
    supplied: &[(&str, &str)],
) -> bool {
    holds_string(mapping, "type", type_name)
        && supplied.iter().all(|(k, v)| {
            mapping
                .get(*k)
                .is_some_and(|value| carries_supplied(value, v))
        })
}

/// Set `type` and every supplied field that does not already carry its
/// value (see [`carries_supplied`]) as strings: existing keys keep their
/// position, a missing `type` goes first, and missing supplied fields are
/// appended in the (declared) order of `supplied`.
fn repair_frontmatter(
    mapping: serde_yaml::Mapping,
    type_name: &str,
    supplied: &[(&str, &str)],
) -> serde_yaml::Mapping {
    use serde_yaml::Value;
    let mut out = serde_yaml::Mapping::new();
    if !mapping.contains_key("type") {
        out.insert("type".into(), type_name.into());
    }
    for (key, value) in mapping {
        let replacement = match key.as_str() {
            Some("type") => Some(type_name),
            Some(k) => supplied
                .iter()
                .find(|(s, _)| *s == k)
                .map(|(_, v)| *v)
                .filter(|v| !carries_supplied(&value, v)),
            None => None,
        };
        let value = replacement.map_or(value, |v| Value::String(v.to_owned()));
        out.insert(key, value);
    }
    for (key, value) in supplied {
        if !out.contains_key(*key) {
            out.insert((*key).into(), (*value).into());
        }
    }
    out
}

/// A note from a re-serialised frontmatter mapping and the untouched body.
/// When the rendered text had no frontmatter, one blank line separates the
/// new block from the body.
fn assemble_note(mapping: &serde_yaml::Mapping, had_block: bool, body: &str) -> String {
    // Infallible for a mapping of strings and parsed YAML values.
    let yaml = serde_yaml::to_string(mapping).unwrap_or_default();
    let sep = if had_block || body.is_empty() || body.starts_with('\n') {
        ""
    } else {
        "\n"
    };
    format!("---\n{yaml}---\n{sep}{body}")
}

/// Normalise a caller's body: `None` when blank, otherwise the text with
/// leading blank lines, trailing blank lines and trailing whitespace
/// stripped. Indentation of the first non-blank line is kept.
fn normalise_body(raw: &str) -> Option<&str> {
    if raw.trim().is_empty() {
        return None;
    }
    let start: usize = raw
        .split_inclusive('\n')
        .take_while(|line| line.trim().is_empty())
        .map(str::len)
        .sum();
    Some(raw[start..].trim_end())
}

/// Drop a first line that is an ATX H1 whose text equals `title`, and the
/// blank lines after it: the engine writes the title heading itself.
fn strip_title_h1<'a>(body: &'a str, title: &str) -> &'a str {
    let first_end = body.find('\n').unwrap_or(body.len());
    if atx_h1_text(&body[..first_end]) != Some(title) {
        return body;
    }
    let rest = &body[first_end..];
    let skip: usize = rest
        .split_inclusive('\n')
        .take_while(|line| line.trim().is_empty())
        .map(str::len)
        .sum();
    &rest[skip..]
}

/// The text of an ATX H1 line (`# Title`, indented by at most three spaces,
/// an optional closing `#` sequence removed), or `None` for any other line.
fn atx_h1_text(line: &str) -> Option<&str> {
    let line = line.trim_end_matches(['\n', '\r']);
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = line[indent..].strip_prefix('#')?;
    if !(rest.is_empty() || rest.starts_with([' ', '\t'])) {
        return None;
    }
    let text = rest.trim();
    let unclosed = text.trim_end_matches('#');
    if unclosed.is_empty() || unclosed.ends_with([' ', '\t']) {
        return Some(unclosed.trim_end());
    }
    Some(text)
}

/// The fence a line opens or closes: its character (`` ` `` or `~`) and
/// run length, for a line indented by at most three spaces.
fn fence_run(line: &str) -> Option<(char, usize)> {
    let line = line.trim_end_matches(['\n', '\r']);
    let indent = line.len() - line.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let rest = &line[indent..];
    let ch = rest.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let run = rest.len() - rest.trim_start_matches(ch).len();
    (run >= 3).then_some((ch, run))
}

/// Insert `body` after the first H1 line of the rendered note `content`
/// (outside the frontmatter and fenced code), with one blank line before and
/// after it; when the note has no H1, append it at the end. The result ends
/// with exactly one newline.
///
/// Both backtick and `~~~` fences are skipped (a fence closes on a run of
/// the same character at least as long as its opener), and an ATX H1 may be
/// indented by up to three spaces. A setext H1 (`Title` over `===`) is not
/// recognised, so with only such a heading the body is appended at the end;
/// and line endings are not normalised, so a CRLF template gets LF lines
/// around the inserted body.
fn insert_body_after_h1(content: &str, body: &str) -> String {
    let mut offset = 0;
    let mut in_frontmatter = false;
    let mut fence: Option<(char, usize)> = None;
    let mut h1_end = None;
    for (i, line) in content.split_inclusive('\n').enumerate() {
        let end = offset + line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        if i == 0 && text == "---" {
            in_frontmatter = true;
        } else if in_frontmatter {
            if text == "---" {
                in_frontmatter = false;
            }
        } else if let Some((ch, run)) = fence {
            let closes = fence_run(text).is_some_and(|(c, r)| {
                c == ch && r >= run && text.trim_start_matches([' ', ch]).trim().is_empty()
            });
            if closes {
                fence = None;
            }
        } else if let Some(open) = fence_run(text) {
            fence = Some(open);
        } else if atx_h1_text(text).is_some() {
            h1_end = Some(end);
            break;
        }
        offset = end;
    }

    let mut out = String::with_capacity(content.len() + body.len() + 4);
    match h1_end {
        Some(end) => {
            out.push_str(content[..end].trim_end_matches(['\n', '\r']));
            out.push_str("\n\n");
            out.push_str(body);
            out.push('\n');
            let rest = content[end..].trim_start_matches(['\n', '\r']);
            if !rest.is_empty() {
                out.push('\n');
                out.push_str(rest);
            }
        }
        None => {
            out.push_str(content.trim_end_matches(['\n', '\r']));
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(body);
            out.push('\n');
        }
    }
    let trimmed_len = out.trim_end_matches(['\n', '\r']).len();
    out.truncate(trimmed_len);
    out.push('\n');
    out
}
