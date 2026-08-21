# CLI ergonomics convention

This document specifies how Cuaderno CLI commands handle missing
arguments. The convention is **flags-and-prompts**: every required
argument is a clap flag declared `Option<T>`; missing flags are
gathered interactively when stdout is a TTY, and surfaced as clear
errors otherwise.

The action commands (`cdno action add / promote / complete / list`)
implement the convention as of #113. The retrofit to `cdno project` /
`cdno commit` is tracked under #114.

## Why

Three audiences hit the same dispatcher with different expectations:

1. **Humans typing interactively** — don't remember every slug, want a
   fuzzy picker, want confirmation before a write.
2. **Humans scripting** — invoke from `.zshrc` aliases, `make`
   targets, or one-liners; want the command to fail-fast on missing
   args, never hang waiting for stdin.
3. **Agentic clients** (MCP, Tauri) — always supply full args at the
   transport boundary; never run in a TTY. The transport layer
   collapses to the same code path as the scripted human.

A single convention covers all three by routing on `Option` + TTY
detection rather than splitting the command into interactive and
non-interactive variants.

## Rules

### 1. Promptable fields are `Option<T>` in clap

Every required argument that could be prompted for is declared as a
clap optional flag, never as a positional or as a required flag:

```rust
ActionCommands::Add {
    #[arg(long)] project: Option<String>,
    #[arg(long)] title:   Option<String>,
    #[arg(long)] energy:  Option<EnergyLevel>,
    #[arg(long)] note:    bool,
}
```

`note` stays a plain bool because clap flags default to `false`; the
absence of the flag is itself a valid value. Required *fields* (the
ones a user must supply or be prompted for) get `Option<T>`.

### 2. Handler folds each `Option` with a single helper

The shared `gather` helper enforces the three-way decision:

```rust
let project = gather(project, "project", interactive, &mut prompted, || {
    prompt::prompt_project(vault)
})?;
```

- `Some(v)` → use it.
- `None` and `interactive` → call the prompt; set `prompted = true`.
- `None` and not interactive → return `missing_flag("project")`, a
  clear "missing required flag: --project" error.

### 3. Confirm only when something was prompted

If `prompted` is `false`, the user provided every field as a flag and
clearly knows what they want — run straight through without
confirmation. If `prompted` is `true`, render a one-block preview of
the gathered values and call `confirm_preview`:

```
About to add to project 'surrogate':
  title:  Run ablation
  energy: deep
  note:   no

Proceed? [Y/n]
```

This matches the agentic shape — MCP and Tauri always supply full
args, so they never see the confirm step.

### 4. `is_interactive` combines `--no-interactive` and TTY detection

```rust
pub fn is_interactive(no_interactive: bool) -> bool {
    !no_interactive && std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}
```

**Both** streams are checked. Prompts read stdin and write stdout, so a
caller with a terminal on only one end — `cdno orient < /dev/null`, a
background job, a wrapper that allocates a pty for output only — must not
be offered one. Testing stdout alone lets such a caller reach the prompt
and fail with inquire's `NotTTY`, which for a read-only listing means an
error and a non-zero exit where there was neither before.

- A TTY on both ends without `--no-interactive` → prompts are available.
- Piped output, redirected stdout, CI, MCP transport → no prompts;
  missing flags error.
- A TTY *with* `--no-interactive` → explicit opt-out; missing flags
  error. Useful inside aliases or scripts that should fail-fast even
  when invoked from a terminal.

`--no-interactive` is declared `global = true` on the root `Cli` so
every subcommand respects it without per-command plumbing.

### 5. Read verbs may prompt *after* rendering (drill-down)

Rules 1-4 cover prompting for a value the command needs *before* it can
act. A read verb has a second, different opportunity: once its listing is
on screen, the reader can be offered a way into one of the rows.

```rust
print!("{}", render_list(&summaries));
prompt::drill_down(
    &summaries,
    "Inspect a project",
    interactive,
    |s| format!("{} ({})", s.slug, s.context.as_str()),
    |s| { print!("{}", render_show(s)); Ok(()) },
)?;
```

- **The listing renders first, unconditionally.** The prompt is purely
  additive. A reader who pipes the output, passes `--no-interactive`, or
  hits Esc immediately sees exactly what they saw before drill-down
  existed.
- **Gated on the same expression as every write verb** —
  `prompt::reports_interactively(no_interactive, json)`. `--json`,
  `--no-interactive`, and either stream not being a terminal are excluded
  by construction. (Both streams matter: the listing is written to stdout
  but the picker reads stdin.)
- **No confirm step**, for the reason in "What is not part of the
  convention" below: nothing is being mutated.
- **Esc and Ctrl-C exit silently with status 0.** Contrast `cdno triage`,
  which prints `Triage stopped.` — there a mutating drain was abandoned
  part-way and saying so earns its line. Here nothing happened.
- **The detail shown is the `show` verb's own renderer**, never a
  bespoke rendering, so the two surfaces cannot drift.

### Exception: a read verb may take one trailing optional positional

Rule 1 exists to keep *mutating* verbs unambiguous, where several promptable
fields would otherwise compete for position. A read verb whose only
promptable argument is an identifier has no such ambiguity, so it may
declare it as a trailing optional positional:

```rust
Show {
    #[arg(add = ArgValueCompleter::new(completions::complete_any_project))]
    slug: Option<String>,
},
```

**The rule for new code:** a read-only `show`-style verb takes its
identifier as a trailing optional positional, because `cdno project show
alpha` is what people type. Missing and non-interactive, it errors with
`missing_positional`, not `missing_flag` — naming a `--slug` that does not
exist sends the reader to `--help` for nothing.

`portfolio show` and `stewardship show` currently take `--portfolio` /
`--slug` flags and are **grandfathered**. Adding a trailing positional to
them later would be additive and is worth doing when one of them is next
touched; their existing flags keep working either way. Until then the two
shapes coexist, and that is a known inconsistency rather than a choice.

## Implementation template

```rust
fn add(
    vault: &Vault,
    at: NaiveDateTime,
    project: Option<String>,
    title: Option<String>,
    energy: Option<EnergyLevel>,
    note: bool,
    interactive: bool,
) -> Result<()> {
    let mut prompted = false;

    // 1. Gather missing fields.
    let project = gather(project, "project", interactive, &mut prompted, || {
        prompt::prompt_project(vault)
    })?;
    let title = gather(title, "title", interactive, &mut prompted, || {
        prompt::prompt_text("Title")
    })?;
    let energy = gather(energy, "energy", interactive, &mut prompted, || {
        prompt::prompt_energy()
    })?;
    let note = if prompted {
        prompt::prompt_confirm("Promote on creation?", note)?
    } else {
        note
    };

    // 2. Confirm-on-prompt only.
    if prompted && !prompt::confirm_preview(&preview_string(...))? {
        println!("Aborted.");
        return Ok(());
    }

    // 3. Execute the domain call.
    vault.add_action(at, &project, &title, energy)?;
    Ok(())
}
```

## What is not part of the convention

- **Read-only commands** (`cdno action list`, `cdno orient`,
  `cdno status`) don't render a confirm step even when they prompt
  for a missing project, or when they offer a drill-down (rule 5) —
  nothing is being mutated.
- **Defaults** (`note: false`, `--weeks 2` on commitments) stay clap
  defaults rather than being prompted for; if the user didn't pass
  the flag and there's a sensible default, use the default.
- **Domain layer** never sees the prompts. `cdno-domain` stays pure
  and synchronous; every prompt happens before the domain call.

## Library

[`inquire`](https://crates.io/crates/inquire) 0.7. Fuzzy-by-default
`Select`, built-in `DateSelect`, and `Confirm` cover every prompt the
action verb needs. Added at the workspace level so future CLI
subcommands can import without per-crate Cargo edits.

New pickers should use `Select::raw_prompt()`, which returns the chosen
*index*. The older pickers in `prompt.rs` recover the index by searching
the label list for the returned string, which silently selects the wrong
row when two labels are equal — a real hazard for search hits and
question text. `drill_down` and `prompt_any_project` already do; the rest
are worth migrating.

## Status

| Verb | Convention applied |
|---|---|
| `cdno action add / promote / complete / list` | #113 |
| `cdno project create / state / park / activate / milestone add+done / waiting add+resolve` | #114 (split across two PRs) |
| `cdno commit create / done` | #114 |
| `cdno orient` (`--energy` already optional) | covered ad-hoc |
| `cdno project show` (slug now an optional positional) | rule 5 exception |

**Drill-down (rule 5) applied**: `project list`, `portfolio list`,
`stewardship list`, `orient`, `status`. Deferred where no `show` verb
exists to open: `questions`, `commitments`, `search`, `action list` —
`prompt::drill_down` is generic, so these need no redesign when one does.

**Picker prompts available**: project (active), any project (active + parked, for read verbs), parked project, action bullet, open milestone, energy, life-domain context, date, hard/soft.
**Plain text prompts** (fuzzy pickers deferred): `waiting resolve` query, `commit done` slug — both pending the matching domain queries (open waiting items per project, active commitments listing).
