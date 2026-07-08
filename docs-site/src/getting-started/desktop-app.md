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

## Before first launch: two caveats

1. **Gatekeeper** — covered above: strip the quarantine
   attribute from a manual install. There is no notarization and no auto-updater yet; upgrades go
   through `brew upgrade --cask cuaderno-app` or a fresh `.dmg`.

2. **Vault discovery** — the app reads the vault path from the `CUADERNO_VAULT_PATH` environment
   variable, same as the CLI and the MCP server. A Finder-launched app inherits no shell
   environment, so set it for GUI apps once per login:

   ```bash
   launchctl setenv CUADERNO_VAULT_PATH "$HOME/Documents/notebook"
   ```

   then (re)launch the app. One-off alternative from a terminal:

   ```bash
   CUADERNO_VAULT_PATH=~/Documents/notebook open -a cuaderno
   ```

   Without the variable the app aborts on startup (the message is visible in Console.app).

## A quick tour

The sidebar leads with **Today** — the morning orientation: commitments due soon, a card per
active project with its current state and a next action filtered by your energy, and any quietly
lapsed habits. **Actions** is the cross-project list of next actions; **Commitments** a
chronological timeline of everything promised (context-coloured, never red); **Weekly** a guided,
stop-anywhere five-step review; **Strategic** the monthly view — questions grid, five-slot project
allocator, portfolio health, stewardship trends. Below those, each active project has a full map
(`/projects/<slug>`), and **Portfolios** and **Stewardships** browse the knowledge and
responsibility layers. Everywhere: `⌘K` opens search-and-jump, `⌘⇧C` summons the global capture
window from any app (Enter files to the inbox, `⌘Enter` appends to today's log), and the menu-bar
tray keeps Quick capture / Open / Quit reachable even with every window closed.
