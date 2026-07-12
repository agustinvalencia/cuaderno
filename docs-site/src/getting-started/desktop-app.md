# Desktop app

Cuaderno ships a macOS desktop app — a **lens over the vault, not an editor**. It answers "where
am I, what's next, what did I promise?" without a terminal, lets you tick, log, and capture, and
hands deep editing off to your editor. The markdown files stay the source of truth; the app is a
view that refreshes live when anything else (nvim, the CLI, Claude via MCP) touches the vault.

Apple Silicon only for now (an Intel build can join when there is a taker).

## Install

### Homebrew cask (recommended)

```bash
brew install --cask agustinvalencia/tap/cuaderno-app
xattr -dr com.apple.quarantine /Applications/cuaderno.app
```

The `xattr` step matters: the app is **ad-hoc signed, not notarized**, so Gatekeeper (recent
Homebrew removed the `--no-quarantine` install flag)
blocks the first launch.

### Manual `.dmg`

Download `cuaderno-app-<version>-aarch64-apple-darwin.dmg` from the
[releases page](https://github.com/agustinvalencia/cuaderno/releases), copy the app to
`/Applications`, then strip the quarantine flag (right-click → Open no longer works on macOS 15+):

```bash
xattr -dr com.apple.quarantine /Applications/cuaderno.app
```

(Or use System Settings → Privacy & Security → "Open Anyway" after the first blocked attempt.)

## Before first launch: two notes

1. **Gatekeeper** — covered above: strip the quarantine
   attribute from a manual install. There is no notarization and no auto-updater yet; upgrades go
   through `brew upgrade --cask cuaderno-app` or a fresh `.dmg`.

2. **Vault discovery** — on first launch the app asks for your vault folder with a native picker,
   validates that it is a cuaderno vault (a `.cuaderno/` marker — pick the vault root, or run
   `cdno init` first if you have not created one), and remembers it for next time. If the folder
   you pick isn't a vault it re-asks; if you cancel, it explains that it needs a vault and exits.

   The `CUADERNO_VAULT_PATH` environment variable remains an **override** for terminals and dev,
   same as the CLI and the MCP server. A Finder-launched app inherits no shell environment, so if
   you want to pin the vault without the picker you can still set it for GUI apps once per login:

   ```bash
   launchctl setenv CUADERNO_VAULT_PATH "$HOME/Documents/notebook"
   ```

   or for a one-off run from a terminal:

   ```bash
   CUADERNO_VAULT_PATH=~/Documents/notebook open -a cuaderno
   ```

   When set, the variable wins over the remembered folder (and an invalid override fails loudly,
   rather than falling back to the picker).

## A quick tour

The sidebar leads with **Today** — the morning orientation: commitments due soon, a card per
active project with its current state and a next action filtered by your energy, and any quietly
lapsed habits. **Actions** is the cross-project list of next actions; **Calendar** a month grid
of your journal — days with a daily note are marked, and clicking one opens it in an embedded
panel that reads read-only and jumps to the previous or next day, the day's week, or its month;
**Commitments** a chronological timeline of everything promised (context-coloured, never red);
**Weekly** a guided, stop-anywhere five-step review; **Strategic** the monthly view — questions
grid, five-slot project allocator, portfolio health, stewardship trends. Below those, each active
project has a full map (`/projects/<slug>`), and **Portfolios** and **Stewardships** browse the
knowledge and responsibility layers. A stewardship's detail draws each tracked series as a calm
trend — counts and volumes (reps, laps, sessions) as columns, continuous measures (a weight, a
pace) as lines, always in the context hue and never as a target to hit. **Config** edits
`.cuaderno/config.toml` in the app — a Raw text view and a structured Form for note types and
schemas, every save validated before it touches disk, and a live reload whenever the config changes
on disk. See [Editing the config in the app](config-editor.md) for the full walkthrough. Everywhere:
`⌘K` opens search-and-jump, `⌘⇧C` summons the
global capture window from any app (Enter files to the inbox, `⌘Enter` appends to today's log),
and the menu-bar tray keeps Quick capture / Open / Quit reachable even with every window closed.
`⌘[` and `⌘]` (or the mouse's back and forward side buttons) step backward and forward through
your view history, just like a browser.
