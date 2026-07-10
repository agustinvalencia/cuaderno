# Editing the config in the app

The [desktop app](desktop-app.md)'s **Config** view edits `.cuaderno/config.toml` without leaving
the app. It reads the file, edits it two ways, validates every change before it touches disk, and
reloads the live vault the moment the config changes — whether the change came from the app or from
your editor.

This page covers using the editor. For *what* each key means, see
[Configuration](../concepts/configuration.md) and the
[Configuration reference](../reference/configuration.md).

## Raw and Form

Config opens on a **Raw / Form** toggle:

- **Raw** is the whole `config.toml` in a text editor. Everything is editable here, byte for byte —
  it is the escape hatch for anything the Form doesn't cover.
- **Form** is a structured view of the two parts that have a fixed shape: **note types** and their
  **schema extensions**. You add, rename, and remove custom note types and required frontmatter
  fields with inputs and toggles instead of hand-writing TOML.

Switch freely between them; both edit the same file. What the Form can't represent stays in Raw
(see [What the Form doesn't edit](#what-the-form-doesnt-edit) below).

## The never-brick save

A vault whose `config.toml` fails to parse or validate won't open — so the editor is built so you
cannot save it into that state from the app.

Every save, from either view, runs the **exact validation the app runs when it opens a vault**:
the TOML is parsed, the ignore globs are compiled, and the type registry is validated. Only if all
three pass does the save proceed. A failure is reported inline — with the line and column for a
syntax slip — and **nothing is written**. The file on disk is never the broken version.

The Raw view also has a **Check** button that dry-runs the same validation without saving, so you
can confirm a hand-edit before you commit to it.

### Edits are surgical

Saving from the Form does **not** rewrite the whole file. It applies a targeted edit to just the
table you changed, so your comments, key order, and formatting elsewhere survive untouched. Renaming
a note type or adding a required field rewrites only that one table.

### Conflict detection

If the file changed on disk between the app reading it and your save — say you edited it in your
editor in the meantime — the save is refused rather than silently overwriting the other change. The
app tells you the file moved under it; reload to pick up the on-disk version, then reapply your edit.

## Live reload

The app watches `.cuaderno/config.toml`. When it changes on disk — you edited it in nvim, ran a CLI
command, or another tool touched it — the app **rebuilds the vault against the new config and
refreshes every view**, no restart needed. The status line confirms the reload.

If an external edit leaves the config invalid, the app keeps the last good vault live and shows the
validation error instead of the reload, so a bad hand-edit never takes the app down with it — fix
the file and the next save reloads cleanly.

## What the Form doesn't edit

The Form covers note types and schema extensions. A few things stay Raw-only by design:

- **The `[variables]` and `[variables.prompt]` blocks** — static and prompted template variables.
  Edit them in Raw; the surgical writer preserves them untouched when you save Form changes.
- **`[vault]` keys** like `max_active_projects` and `ignore` globs — edit these in Raw.

Everything Raw-only is still covered by the never-brick save and live reload; only the structured
inputs are absent.

---

Next: the concepts behind these keys in [Configuration](../concepts/configuration.md), or the
hands-on [Customising templates and frontmatter](../tutorials/templates-and-frontmatter.md) tutorial.
