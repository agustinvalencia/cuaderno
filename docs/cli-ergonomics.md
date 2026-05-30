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
    !no_interactive && std::io::stdout().is_terminal()
}
```

- A TTY without `--no-interactive` → prompts are available.
- Piped output, redirected stdout, CI, MCP transport → no prompts;
  missing flags error.
- A TTY *with* `--no-interactive` → explicit opt-out; missing flags
  error. Useful inside aliases or scripts that should fail-fast even
  when invoked from a terminal.

`--no-interactive` is declared `global = true` on the root `Cli` so
every subcommand respects it without per-command plumbing.

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
  for a missing project — nothing is being mutated.
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

## Status

| Verb | Convention applied |
|---|---|
| `cdno action add / promote / complete / list` | #113 |
| `cdno project create / state` | #114 (this PR) |
| `cdno commit create / done` | #114 (this PR) |
| `cdno project park / activate / milestone add+done / waiting add+resolve` | #114 follow-up — convention applies, picker prompts (active commitments, open milestones, open waitings) wait for the matching domain queries |
| `cdno orient` (`--energy` already optional) | covered ad-hoc |
