//! Resolving and running the editor `cdno open` hands a note to.
//!
//! Lives in `cdno-cli` and nowhere else. This is the only place in the
//! codebase where vault-supplied data becomes an executed program, and the
//! layering keeps it that way: `cdno-core` *parses* the `[editor]` config,
//! `cdno-cli` acts on it, and `cdno-mcp` never resolves or runs an editor at
//! all — an MCP tool that spawned one would turn a config write into
//! arbitrary code execution.
//!
//! ## Why the editor never comes from the vault
//!
//! It would be natural to put this in `.cuaderno/config.toml` — a research
//! vault that belongs in Obsidian, a code vault that belongs in your editor.
//! That is exactly what this deliberately does **not** do.
//!
//! A vault is a git repository. `--vault` and `CUADERNO_VAULT_PATH` exist so
//! cdno can be pointed at one you did not create, and vaults get cloned and
//! synced between machines and people. A setting that says *which program to
//! run* cannot live in data that travels like that: it is the
//! `.git/config core.editor` class of bug, and cloning a vault would be
//! enough to execute its author's choice of program.
//!
//! Constraining the *shape* of such a setting does not rescue it. Reducing it
//! to a bare binary name with no arguments still leaves `command = "sh"`,
//! which runs `sh <the note>` — and the note is the attacker's too, so its
//! contents execute. The same holds for `bash`, `perl`, `ruby`, `node`, and
//! every other interpreter that takes a script positionally. The problem is
//! not the value's shape, it is its provenance.
//!
//! So every source here is one the person at the keyboard controls: a flag
//! they typed, or an environment variable from their own shell. Both travel
//! with the *user*, not with the data.
//!
//! (If a per-vault editor is wanted later, it needs a trust record kept
//! *outside* the vault — git's `safe.directory` shape — so that consent
//! cannot be shipped inside the artefact it authorises. That is a design of
//! its own, not a config key.)
//!
//! ## The metacharacter check is ergonomics, not security
//!
//! [`first_unquoted_metachar`] rejects an unquoted `;`, `&&`, `|` and so on.
//! That is there so `vim {path} && echo done` fails loudly instead of opening
//! a file called `&&` — it is **not** a security boundary, and it could not
//! be one: a template may always name a shell itself. Since every source is
//! trusted, that is fine, and it must stay fine — do not reintroduce an
//! untrusted source on the strength of this check.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow, bail};

/// The placeholder a template uses to say where the path goes.
pub const PATH_PLACEHOLDER: &str = "{path}";

/// Characters that only mean something to a shell.
///
/// We never invoke one, so an unquoted occurrence is always a mistake — the
/// user expected `sh -c` semantics they will not get. Rejecting is louder and
/// kinder than the alternative, which is silently opening a file named `&&`.
/// It also makes "this is not a shell" an enforced contract rather than a
/// convention someone later relaxes.
const SHELL_METACHARS: &[char] = &[';', '|', '&', '$', '`', '>', '<', '(', ')', '\n', '\r'];

/// Where an editor setting came from.
///
/// Carried through resolution purely so a failure can name the thing to go and
/// fix. With five possible sources, "editor not found" on its own sends the
/// user hunting through their shell profile and two config files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorSource {
    Flag,
    EnvCuaderno,
    EnvVisual,
    EnvEditor,
}

impl EditorSource {
    pub fn describe(self) -> &'static str {
        match self {
            Self::Flag => "--editor",
            Self::EnvCuaderno => "$CUADERNO_EDITOR",
            Self::EnvVisual => "$VISUAL",
            Self::EnvEditor => "$EDITOR",
        }
    }
}

/// A resolved editor, ready to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorCommand {
    /// A real program plus its argv. Executed directly, never through a shell.
    Argv {
        argv: Vec<String>,
        wait: bool,
        source: EditorSource,
    },
    /// A URI template (`obsidian://…`). Handed whole to the OS handler; never
    /// tokenised, because a URI cannot be split into argv meaningfully.
    Uri {
        template: String,
        source: EditorSource,
    },
    /// Nothing configured anywhere: let the OS decide from the file
    /// association, which beats guessing at `vi` or `notepad`.
    PlatformDefault,
}

/// Resolve which editor to use.
///
/// `env` is injected rather than read from the process. Edition 2024 makes
/// `std::env::set_var` `unsafe` and it is racy across cargo's test threads, so
/// reading the real environment here would make every precedence rule
/// untestable.
///
/// Order: `--editor` → `$CUADERNO_EDITOR` → `$VISUAL` → `$EDITOR` → the
/// platform default. Every one of these is set by the person running the
/// command; nothing is read from the vault (see the module docs).
pub fn resolve(
    flag: Option<&str>,
    env: &dyn Fn(&str) -> Option<String>,
    wait_override: Option<bool>,
) -> Result<EditorCommand> {
    // Every source is trim-and-drop-if-empty. `EDITOR=""` is common in
    // stripped CI images, and treating it as set would yield an empty argv
    // rather than falling through to something that works.
    let nonblank = |s: String| {
        let t = s.trim().to_owned();
        (!t.is_empty()).then_some(t)
    };

    let candidates: [(Option<String>, EditorSource); 4] = [
        (
            flag.map(str::to_owned).and_then(nonblank),
            EditorSource::Flag,
        ),
        (
            env("CUADERNO_EDITOR").and_then(nonblank),
            EditorSource::EnvCuaderno,
        ),
        (env("VISUAL").and_then(nonblank), EditorSource::EnvVisual),
        (env("EDITOR").and_then(nonblank), EditorSource::EnvEditor),
    ];

    for (value, source) in candidates {
        if let Some(template) = value {
            return parse_template(&template, source, wait_override);
        }
    }
    Ok(EditorCommand::PlatformDefault)
}

/// Turn a template into something runnable.
pub fn parse_template(
    raw: &str,
    source: EditorSource,
    wait_override: Option<bool>,
) -> Result<EditorCommand> {
    let trimmed = raw.trim();

    // Syntactic, not a guess: no executable name contains `://`. This must be
    // checked before tokenising, because `obsidian://open?path=/x` has no
    // sensible argv decomposition at all.
    if trimmed
        .split_whitespace()
        .next()
        .is_some_and(|w| w.contains("://"))
    {
        // The `Argv` branch can append a missing placeholder; a URI cannot —
        // no position reliably means "the file" across schemes. Without the
        // placeholder the OS handler is invoked with a URI naming no note, so
        // cdno would report success having opened nothing in particular.
        // Refuse rather than pretend.
        if !trimmed.contains(PATH_PLACEHOLDER) {
            bail!(
                "editor URI from {} has no `{PATH_PLACEHOLDER}` placeholder, so it \
                 names no note: {trimmed}",
                source.describe(),
            );
        }
        return Ok(EditorCommand::Uri {
            template: trimmed.to_owned(),
            source,
        });
    }

    if let Some(c) = first_unquoted_metachar(trimmed) {
        bail!(
            "editor command from {} contains `{c}`, which cdno will not run: the \
             template is executed directly, not through a shell. Put the pipeline \
             in a script and point the editor at that.",
            source.describe(),
        );
    }

    let argv = shlex::split(trimmed).ok_or_else(|| {
        anyhow!(
            "editor command from {} has unbalanced quotes: {trimmed}",
            source.describe()
        )
    })?;
    if argv.is_empty() {
        bail!("editor command from {} is empty", source.describe());
    }
    Ok(EditorCommand::Argv {
        argv,
        wait: wait_override.unwrap_or(true),
        source,
    })
}

/// The first shell metacharacter sitting outside quotes, if any.
///
/// Quote-aware on purpose: `"My Editor (beta)"` is a legitimate program name,
/// and a plain `contains()` would reject it for the parenthesis.
fn first_unquoted_metachar(s: &str) -> Option<char> {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match (quote, c) {
            (None, '\\') => escaped = true,
            (None, '\'') | (None, '"') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            // Inside quotes a metacharacter is just text; shlex will hand it
            // to the editor as part of one argument, which is what the user
            // asked for by quoting it.
            (Some(_), _) => {}
            (None, c) if SHELL_METACHARS.contains(&c) => return Some(c),
            (None, _) => {}
        }
    }
    None
}

/// Substitute the note's path into an argv.
///
/// `{path}` is replaced in every token containing it. When no token carries
/// the placeholder, the path is appended as a final argument — which is what
/// makes a bare `EDITOR=nvim` work without the user ever learning the
/// placeholder exists. `EDITOR` values in the wild never carry one, and this
/// mirrors how git treats `core.editor`.
pub fn substitute_path(argv: &[String], path: &Path) -> Vec<String> {
    let p = path.to_string_lossy();
    if argv.iter().any(|a| a.contains(PATH_PLACEHOLDER)) {
        argv.iter()
            .map(|a| a.replace(PATH_PLACEHOLDER, &p))
            .collect()
    } else {
        let mut out = argv.to_vec();
        out.push(p.into_owned());
        out
    }
}

/// Percent-encode a path for embedding in a URI's query string.
///
/// The set is deliberately conservative — anything outside unreserved
/// characters is escaped. Without this, `obsidian://open?path={path}` breaks
/// on the first vault under `~/Google Drive/`, and an `&` in a filename would
/// silently truncate the parameter.
fn encode_for_uri(path: &Path) -> String {
    const UNRESERVED_EXTRA: &[char] = &['-', '_', '.', '~'];
    let mut out = String::new();
    for byte in path.to_string_lossy().bytes() {
        let c = byte as char;
        if c.is_ascii_alphanumeric() || UNRESERVED_EXTRA.contains(&c) {
            out.push(c);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

impl EditorCommand {
    /// The URI this would hand to the OS for `path`, or `None` for a variant
    /// that runs a program instead.
    ///
    /// Split out from [`spawn`](Self::spawn) so the percent-encoding can be
    /// asserted without actually launching the OS handler — which in a test
    /// would open a real application.
    pub fn uri_for(&self, path: &Path) -> Option<String> {
        match self {
            Self::Uri { template, .. } => {
                Some(template.replace(PATH_PLACEHOLDER, &encode_for_uri(path)))
            }
            _ => None,
        }
    }

    /// Run the editor against `path` (absolute).
    ///
    /// Returns the editor's exit code when it ran and produced one, and `None`
    /// when the work was handed off (detached, or to the OS).
    ///
    /// **Always waits, with inherited stdio, unless told otherwise.** Deciding
    /// terminal-vs-GUI automatically is not possible: `EDITOR=vim` is
    /// terminal, `code -w` is GUI-and-blocking, `code` is GUI-and-returns, and
    /// a program-name heuristic would be wrong in a way the user could not
    /// override. Waiting is correct for every terminal editor — the case that
    /// fails catastrophically otherwise, since a detached `vim` fights the
    /// parent shell for the tty — and indistinguishable from detaching for a
    /// GUI editor that returns immediately.
    pub fn spawn(&self, path: &Path) -> Result<Option<i32>> {
        match self {
            Self::PlatformDefault => {
                open::that_detached(path).with_context(|| {
                    format!(
                        "opening {} with the platform default handler",
                        path.display()
                    )
                })?;
                Ok(None)
            }
            Self::Uri { source, .. } => {
                let uri = self.uri_for(path).expect("Uri variant builds a URI");
                open::that_detached(&uri).with_context(|| {
                    format!(
                        "handing {uri} to the OS (editor from {})",
                        source.describe()
                    )
                })?;
                Ok(None)
            }
            Self::Argv { argv, wait, source } => {
                let argv = substitute_path(argv, path);
                let mut cmd = Command::new(&argv[0]);
                cmd.args(&argv[1..]);
                if *wait {
                    // Inherited stdio is the whole point: a terminal editor
                    // needs the tty, and without this `vim` cannot draw.
                    let status = cmd
                        .status()
                        .map_err(|e| spawn_error(e, &argv[0], *source))?;
                    // `code()` is None when the child died on a signal, and
                    // returning that would be indistinguishable from "handed
                    // off successfully" — an editor killed by SIGTERM would
                    // report success to a wrapper script. Unix convention
                    // maps a signal death to 128 + signum; 128 alone still
                    // says "not a clean exit".
                    Ok(Some(status.code().unwrap_or(128)))
                } else {
                    cmd.stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null());
                    cmd.spawn().map_err(|e| spawn_error(e, &argv[0], *source))?;
                    Ok(None)
                }
            }
        }
    }
}

fn spawn_error(e: std::io::Error, program: &str, source: EditorSource) -> anyhow::Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        anyhow!(
            "editor `{program}` was not found on PATH (configured by {})",
            source.describe()
        )
    } else {
        anyhow::Error::new(e).context(format!(
            "running editor `{program}` (configured by {})",
            source.describe()
        ))
    }
}
