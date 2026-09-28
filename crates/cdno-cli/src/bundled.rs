//! Bundled note types: the custom types the binary ships with, and the one
//! function that installs them into a vault (RFC 0003).
//!
//! A bundled type is an ordinary `[note_types.<name>]` declaration plus a
//! template, carried verbatim by the binary. `cdno init` installs every
//! bundled type into a new vault, and `cdno config note-type install` installs
//! one into an existing vault. Both go through [`install_bundled`], so a new
//! vault and an upgraded one are identical by construction.
//!
//! The declaration block and the template are `include_str!`ed from
//! `examples/note-types/<name>/`, so there is exactly one copy of each on
//! disk and the examples cannot drift from what the binary writes.
//!
//! ## What an install does, in order
//!
//! 1. **Template.** Create `.cuaderno/templates/` if absent, then write the
//!    bundled template only when the file the declaration in force names is
//!    absent and is the bundled filename. A present file is never
//!    overwritten.
//! 2. **Folder.** Create the type's folder if absent. Empty folders are not
//!    indexed, so this is cosmetic, but it makes the install visible.
//! 3. **Declaration.** Append the bundled block as TEXT through the config
//!    gate (validate first, then compare-and-swap), only when the parsed
//!    config does not already declare the name. An existing declaration is
//!    never modified; it is compared field by field and reported.
//!
//! The order means a failure never leaves a declaration pointing at a
//! missing template. When the declaration step is refused, whatever the
//! first two steps created is removed again, so a refused install leaves
//! nothing behind.
//!
//! ## Why the block is appended as text
//!
//! `config_edit::set_note_type` rebuilds the table key by key through
//! `toml_edit`, which drops comments. The bundled block's comment is what
//! tells the owner the type is optional and deletable, so the block goes in
//! verbatim instead. "Declared" is still decided on the PARSED config, never
//! a text search: a commented-out `# [note_types.concept]` must not count,
//! and a dotted `note_types.concept.folder = …` key must.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow};
use cdno_core::config::{CustomNoteType, VaultConfig};
use cdno_core::paths;
use cdno_core::store::FsVaultStore;
use cdno_domain::ConfigSaveError;
use cdno_domain::vault::config::read_config_from;

use crate::commands::config::{EditOutcome, finish_edit};

/// One note type the binary ships with.
#[derive(Debug, Clone, Copy)]
pub struct BundledType {
    /// The `[note_types.<name>]` key.
    pub name: &'static str,
    /// One line saying what the type is for, shown by `install --list`.
    pub purpose: &'static str,
    /// The declaration, verbatim, comment included. Must declare exactly
    /// `[note_types.<name>]`.
    pub block: &'static str,
    /// The template filename under `.cuaderno/templates/` the block names.
    pub template_filename: &'static str,
    /// The template content, verbatim.
    pub template: &'static str,
}

/// Every bundled type. Bundled types are the note types `cdno init` knows,
/// nothing more; once installed, each is an ordinary custom declaration the
/// owner may edit or delete.
///
/// The names must match [`cdno_core::paths::BUNDLED_NOTE_TYPE_NAMES`], which
/// is how `cdno-mcp` names the install command without depending on this
/// crate; a test pins the two together.
pub const BUNDLED: &[BundledType] = &[BundledType {
    name: "concept",
    purpose: "a library of reusable understanding: theorems, definitions, techniques, procedures",
    block: include_str!("../../../examples/note-types/concept/config.toml"),
    template_filename: "concept.md",
    template: include_str!("../../../examples/note-types/concept/concept.md"),
}];

/// Look a bundled type up by name.
pub fn find(name: &str) -> Option<&'static BundledType> {
    BUNDLED.iter().find(|t| t.name == name)
}

impl BundledType {
    /// The block parsed as a config, i.e. the declaration exactly as it
    /// would read once installed.
    pub fn declaration(&self) -> Result<CustomNoteType> {
        let config: VaultConfig = toml::from_str(self.block)
            .with_context(|| format!("parsing the bundled `{}` declaration", self.name))?;
        config
            .note_types
            .get(self.name)
            .cloned()
            .ok_or_else(|| anyhow!("the bundled block does not declare `{}`", self.name))
    }

    /// The template section headings (`## …` lines), for `install --list`.
    pub fn section_headings(&self) -> Vec<&'static str> {
        self.template
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .map(str::trim)
            .collect()
    }
}

/// The template filename a declaration resolves to: its `template` key, or
/// `<name>.md` when it has none (as `templating.rs` resolves it).
pub fn template_in_force(name: &str, declaration: &CustomNoteType) -> String {
    declaration
        .template
        .clone()
        .unwrap_or_else(|| format!("{name}.md"))
}

/// What the declaration step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclarationOutcome {
    /// The bundled block was appended.
    Written,
    /// Already declared, and equal to the bundled declaration.
    KeptMatches,
    /// Already declared, and different; left untouched.
    KeptDiffers(Vec<Difference>),
}

/// One key on which an existing declaration differs from the bundled one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub key: &'static str,
    /// The bundled value, rendered for display.
    pub bundled: String,
    /// The vault's value, rendered for display.
    pub yours: String,
    /// The `cdno config note-type set` invocation that adopts the bundled
    /// value.
    pub adopt: String,
}

/// What the template step did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateAction {
    Written,
    /// A file was present and identical to the bundled template.
    KeptMatches,
    /// A file was present and differs from the bundled template.
    KeptCustomised,
    /// The declaration in force names a different file, so the bundled
    /// template was not installed.
    NotInstalled {
        declared: String,
    },
}

/// What the folder step did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderAction {
    Created,
    Present,
}

/// The outcome of one install, step by step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub name: String,
    pub declaration: DeclarationOutcome,
    /// The template filename under `.cuaderno/templates/`.
    pub template_file: String,
    pub template: TemplateAction,
    /// The vault-relative folder.
    pub folder_path: String,
    pub folder: FolderAction,
    /// Whether `.cuaderno/templates/` itself had to be created.
    pub templates_dir_created: bool,
    /// Whether this was a dry run: every outcome is what WOULD happen.
    pub dry_run: bool,
    /// On a dry run, the exact config text that would be appended.
    pub appended_block: Option<String>,
}

impl InstallReport {
    /// Whether anything was (or, on a dry run, would be) written.
    pub fn changed(&self) -> bool {
        self.declaration == DeclarationOutcome::Written
            || self.template == TemplateAction::Written
            || self.folder == FolderAction::Created
            || self.templates_dir_created
    }
}

/// Install the bundled type `name` into the vault at `root`.
///
/// See the module docs for the three steps and their order. Errors when the
/// name is not bundled, when the config cannot be read, parsed or validated, when the
/// gate refuses the appended declaration (with what the earlier steps
/// created removed again), or when a write fails.
pub fn install_bundled(root: &Path, name: &str) -> Result<InstallReport> {
    let bundled = find(name).ok_or_else(|| unknown_bundled(name))?;
    install(root, bundled, false)
}

/// The refusal for a name that is not bundled, listing those that are.
pub fn unknown_bundled(name: &str) -> anyhow::Error {
    let names: Vec<&str> = BUNDLED.iter().map(|t| t.name).collect();
    anyhow!(
        "'{name}' is not a bundled note type — bundled: {}",
        names.join(", ")
    )
}

/// [`install_bundled`] over an explicit [`BundledType`], so a test can drive
/// the gate with a block the `const` registry would never carry. With
/// `dry_run`, every step is decided and the candidate config is validated,
/// but nothing is written.
pub fn install(root: &Path, bundled: &BundledType, dry_run: bool) -> Result<InstallReport> {
    let name = bundled.name;
    let bundled_decl = bundled.declaration()?;

    // Read and parse the config first: every later decision depends on
    // whether the name is declared, and a config that does not parse is
    // refused before anything is written.
    let store = FsVaultStore::new(root);
    let original = read_config_from(&store).context("reading .cuaderno/config.toml")?;
    let model = crate::commands::config::read_model(&original.content)?;
    let existing = model.note_types.get(name);

    // `read_model` only proves the TOML parses. When the name is already
    // declared the declaration step never reaches the gate, so without this
    // an existing declaration that parses but does not validate (a folder
    // escaping the vault, say) would drive the folder step as it stands.
    // Refuse any config the vault would not open, before any write.
    cdno_domain::validate_config_str(&original.content)
        .map_err(|e| crate::commands::config::describe(ConfigSaveError::Validation(e)))?;

    // The declaration that will be in force once this install is done.
    let in_force = existing.unwrap_or(&bundled_decl);
    let template_file = template_in_force(name, in_force);
    let folder_path = in_force.folder.trim_end_matches('/').to_owned();

    let mut undo = Undo::default();
    let result = (|| -> Result<InstallReport> {
        // ---- 1. Template -------------------------------------------------
        let templates_dir = root.join(paths::TEMPLATES_DIR);
        let templates_dir_created = !templates_dir.is_dir();
        if templates_dir_created && !dry_run {
            undo.create_dirs(&templates_dir)?;
        }

        let template = if template_file != bundled.template_filename {
            TemplateAction::NotInstalled {
                declared: template_file.clone(),
            }
        } else {
            let dest = templates_dir.join(&template_file);
            if dest.exists() {
                let present = fs::read_to_string(&dest)
                    .with_context(|| format!("reading {}", dest.display()))?;
                if present == bundled.template {
                    TemplateAction::KeptMatches
                } else {
                    TemplateAction::KeptCustomised
                }
            } else {
                if !dry_run {
                    fs::write(&dest, bundled.template)
                        .with_context(|| format!("writing template {}", dest.display()))?;
                    undo.files.push(dest);
                }
                TemplateAction::Written
            }
        };

        // ---- 2. Folder ---------------------------------------------------
        let folder_dir = root.join(&folder_path);
        let folder = if folder_dir.is_dir() {
            FolderAction::Present
        } else {
            if !dry_run {
                undo.create_dirs(&folder_dir)?;
            }
            FolderAction::Created
        };

        // ---- 3. Declaration ----------------------------------------------
        let mut appended_block = None;
        let declaration = match existing {
            Some(current) => {
                let differences = compare(name, &bundled_decl, current);
                if differences.is_empty() {
                    DeclarationOutcome::KeptMatches
                } else {
                    DeclarationOutcome::KeptDiffers(differences)
                }
            }
            None => {
                let candidate = append_block(&original.content, bundled.block);
                if dry_run {
                    // The same validation the gate runs first, so a dry run
                    // reports the refusal a real run would hit.
                    cdno_domain::validate_config_str(&candidate).map_err(|err| {
                        translate(name, &folder_path, ConfigSaveError::Validation(err))
                    })?;
                    appended_block = Some(candidate[original.content.len()..].to_owned());
                } else {
                    match finish_edit(&store, &original, &candidate) {
                        Ok(EditOutcome::Saved { .. }) | Ok(EditOutcome::Unchanged) => {}
                        Err(err) => return Err(translate(name, &folder_path, err)),
                    }
                }
                DeclarationOutcome::Written
            }
        };

        Ok(InstallReport {
            name: name.to_owned(),
            declaration,
            template_file: template_file.clone(),
            template,
            folder_path: folder_path.clone(),
            folder,
            templates_dir_created,
            dry_run,
            appended_block,
        })
    })();

    match result {
        Ok(report) => Ok(report),
        Err(err) => {
            // Leave nothing behind: the template and folders this run
            // created go again, newest first.
            undo.rollback();
            Err(err)
        }
    }
}

/// The candidate config: the original text, then a newline if it does not
/// already end in one, then a blank line if it does not already end in one,
/// then the block verbatim (RFC 0003 §4.2.3).
///
/// The default config ends in `\n\n`, so on a fresh vault this adds nothing
/// between the two and `cdno init` writes the same bytes it always has.
pub fn append_block(original: &str, block: &str) -> String {
    let mut candidate = String::with_capacity(original.len() + block.len() + 2);
    candidate.push_str(original);
    // An empty config needs no separator: there is nothing to separate from.
    if !candidate.is_empty() {
        if !candidate.ends_with('\n') {
            candidate.push('\n');
        }
        if !candidate.ends_with("\n\n") {
            candidate.push('\n');
        }
    }
    candidate.push_str(block);
    candidate
}

/// Phrase a gate refusal of the appended declaration.
///
/// Two refusals get plain words because the gate's own message names the
/// symptom rather than the fix: a `note_types` inline table cannot take an
/// appended `[note_types.<name>]` header (TOML reports a duplicate key, or an
/// attempt to extend an inline table, depending on the shape), and
/// another type already owning the folder is reported as "both declare
/// folder". Everything else is the structured verbs' own message.
fn translate(name: &str, folder: &str, err: ConfigSaveError) -> anyhow::Error {
    if let ConfigSaveError::Validation(e) = &err {
        if e.message.contains("note_types")
            && (e.message.contains("duplicate key") || e.message.contains("inline table"))
        {
            return anyhow!(
                "`note_types` is declared inline; add the block with `cdno config edit`. \
                 Nothing was written."
            );
        }
        if e.message.contains("both declare folder") {
            return anyhow!(
                "another note type already uses the folder `{folder}`, so `{name}` cannot be \
                 installed alongside it:\n  {}\n\nNothing was written.",
                e.message
            );
        }
    }
    crate::commands::config::describe(err)
}

/// Compare an existing declaration with the bundled one, key by key, as
/// parsed values. `template` is compared as the filename each resolves to,
/// since an absent key and `<name>.md` name the same file.
pub fn compare(name: &str, bundled: &CustomNoteType, yours: &CustomNoteType) -> Vec<Difference> {
    let mut out = Vec::new();
    let set = |flag: String| format!("cdno config note-type set --name {name} {flag}");

    if bundled.folder != yours.folder {
        out.push(Difference {
            key: "folder",
            bundled: quoted(&bundled.folder),
            yours: quoted(&yours.folder),
            adopt: set(format!("--folder {}", shell_word(&bundled.folder))),
        });
    }
    if bundled.required != yours.required {
        out.push(Difference {
            key: "required",
            bundled: list(&bundled.required),
            yours: list(&yours.required),
            adopt: set(format!(
                "--required {}",
                shell_word(&bundled.required.join(","))
            )),
        });
    }
    if bundled.optional != yours.optional {
        out.push(Difference {
            key: "optional",
            bundled: list(&bundled.optional),
            yours: list(&yours.optional),
            adopt: set(format!(
                "--optional {}",
                shell_word(&bundled.optional.join(","))
            )),
        });
    }
    let bundled_template = template_in_force(name, bundled);
    let your_template = template_in_force(name, yours);
    if bundled_template != your_template {
        out.push(Difference {
            key: "template",
            bundled: quoted(&bundled_template),
            yours: quoted(&your_template),
            adopt: set(format!("--template {}", shell_word(&bundled_template))),
        });
    }
    if bundled.append_only != yours.append_only {
        out.push(Difference {
            key: "append_only",
            bundled: bundled.append_only.to_string(),
            yours: yours.append_only.to_string(),
            adopt: set(if bundled.append_only {
                "--append-only".to_owned()
            } else {
                "--no-append-only".to_owned()
            }),
        });
    }
    for (key, flag, b, y) in [
        (
            "title_field",
            "--title-field",
            &bundled.title_field,
            &yours.title_field,
        ),
        (
            "date_field",
            "--date-field",
            &bundled.date_field,
            &yours.date_field,
        ),
    ] {
        if b != y {
            out.push(Difference {
                key,
                bundled: opt(b),
                yours: opt(y),
                adopt: set(format!(
                    "{flag} {}",
                    shell_word(b.as_deref().unwrap_or_default())
                )),
            });
        }
    }
    out
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

fn list(values: &[String]) -> String {
    let inner: Vec<String> = values.iter().map(|v| quoted(v)).collect();
    format!("[{}]", inner.join(", "))
}

fn opt(value: &Option<String>) -> String {
    value.as_deref().map_or_else(|| "(none)".to_owned(), quoted)
}

/// Quote a flag value for a shell when it needs it; an empty value is `''`,
/// which is how the `set` verbs clear a key.
fn shell_word(value: &str) -> String {
    if !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ',' | '/'))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

/// What an install created, so a refused declaration can take it away again.
#[derive(Default)]
struct Undo {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
}

impl Undo {
    /// `create_dir_all`, remembering each directory that did not exist, so
    /// rollback removes exactly those and no pre-existing parent.
    fn create_dirs(&mut self, dir: &Path) -> Result<()> {
        let mut missing = Vec::new();
        let mut cursor = Some(dir);
        while let Some(path) = cursor {
            if path.exists() {
                break;
            }
            missing.push(path.to_path_buf());
            cursor = path.parent();
        }
        fs::create_dir_all(dir).with_context(|| format!("creating directory {}", dir.display()))?;
        // Outermost first, so rollback (which runs in reverse) removes the
        // innermost first.
        self.dirs.extend(missing.into_iter().rev());
        Ok(())
    }

    fn rollback(self) {
        for file in self.files.iter().rev() {
            let _ = fs::remove_file(file);
        }
        // `remove_dir` only removes an empty directory, so anything a
        // concurrent writer put there in the meantime survives.
        for dir in self.dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
    }
}

/// Where a vault stands with one bundled type, for `install --list`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstalledState {
    /// The config does not declare the name.
    NotInstalled,
    /// Declared exactly as bundled, with the bundled template in place.
    Matches,
    /// Declared, but differently from the bundled declaration.
    DeclarationDiffers,
    /// Declared as bundled, and the template file differs from the bundled
    /// one.
    TemplateCustomised,
    /// Declared as bundled, and the template file it names is absent. Not
    /// one of RFC 0003's four states: a vault that followed the old two-file
    /// recipe halfway lands here, and calling it "matches" would hide the
    /// one step `install` would still take.
    TemplateMissing,
}

impl InstalledState {
    /// The text `--list` prints.
    pub fn label(self) -> &'static str {
        match self {
            Self::NotInstalled => "not installed",
            Self::Matches => "installed (matches)",
            Self::DeclarationDiffers => "installed (declaration differs)",
            Self::TemplateCustomised => "installed (template customised)",
            Self::TemplateMissing => "installed (template missing)",
        }
    }

    /// The value `--list --json` carries.
    pub fn key(self) -> &'static str {
        match self {
            Self::NotInstalled => "not_installed",
            Self::Matches => "installed_matches",
            Self::DeclarationDiffers => "installed_declaration_differs",
            Self::TemplateCustomised => "installed_template_customised",
            Self::TemplateMissing => "installed_template_missing",
        }
    }
}

/// The state of `bundled` in the vault at `root`, decided exactly as
/// [`install`] decides it: on the parsed config, comparing field by field.
pub fn installed_state(root: &Path, bundled: &BundledType) -> Result<InstalledState> {
    let store = FsVaultStore::new(root);
    let original = read_config_from(&store).context("reading .cuaderno/config.toml")?;
    let model = crate::commands::config::read_model(&original.content)?;
    let Some(current) = model.note_types.get(bundled.name) else {
        return Ok(InstalledState::NotInstalled);
    };
    if !compare(bundled.name, &bundled.declaration()?, current).is_empty() {
        return Ok(InstalledState::DeclarationDiffers);
    }
    let path = root
        .join(paths::TEMPLATES_DIR)
        .join(template_in_force(bundled.name, current));
    if !path.exists() {
        return Ok(InstalledState::TemplateMissing);
    }
    let present =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    Ok(if present == bundled.template {
        InstalledState::Matches
    } else {
        InstalledState::TemplateCustomised
    })
}
