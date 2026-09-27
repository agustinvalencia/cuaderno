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
    /// is ignored and the engine value is used instead. Passing the check
    /// does not guarantee the rendered note has a non-empty value: a
    /// template that omits the placeholder can still produce a note that
    /// `cdno lint` reports as missing the field.
    ///
    /// `body` (RFC 0002 §6.2) is the note's prose, written verbatim. When the
    /// type's template references `{{body}}` it renders there; otherwise (a
    /// template without the placeholder, or the synthesised note) it is
    /// inserted after the note's first H1, or appended when there is none, so
    /// the H1 stays the title. With no body, a `{{body}}` placeholder renders
    /// empty rather than literally. A whitespace-only body counts as absent.
    ///
    /// `origin` (RFC 0002 §5.5) is one string of wikilinks to where the note
    /// came from — promotion is create-with-`origin`. It is written as a plain
    /// frontmatter string, exactly as given, through the same field map as
    /// `fields`, so a type that does not declare `origin` refuses it with
    /// [`DomainError::UnknownField`]. When `fields` also carries an `origin`
    /// key, this parameter wins. A whitespace-only origin counts as absent.
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
        if let Some(origin) = origin.filter(|o| !o.trim().is_empty()) {
            fields.insert(ORIGIN.to_owned(), origin.to_owned());
        }
        let fields = &fields;
        let body = body.map(str::trim).filter(|b| !b.is_empty());

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
        // A template with a `{{body}}` placeholder takes the body there; with
        // no body it renders empty (the engine would otherwise leave an
        // unknown placeholder in literally).
        let body_in_template = self.custom_template_references(&template_name, BODY)?;
        if body_in_template {
            ctx.set_contextual(BODY, body.unwrap_or(""));
        }

        let field_order = descriptor
            .custom_frontmatter_order()
            .expect("a custom descriptor always yields an order");
        let mut content =
            self.scaffold_custom(type_name, &template_name, &field_order, &mut ctx)?;
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

/// Insert `body` after the first H1 line of the rendered note `content`
/// (outside the frontmatter and fenced code), with one blank line before and
/// after it; when the note has no H1, append it at the end. The result ends
/// with exactly one newline.
fn insert_body_after_h1(content: &str, body: &str) -> String {
    let mut offset = 0;
    let mut in_frontmatter = false;
    let mut in_fence = false;
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
        } else if text.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence && (text.starts_with("# ") || text == "#") {
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
