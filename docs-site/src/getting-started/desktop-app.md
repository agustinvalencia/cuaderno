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

The sidebar is grouped the way the method is: a **rhythm**, and the two tracks it binds.

**Rhythm** is the cadence. **Today** is the day's own note, with the morning orientation above it: a
**Now** band naming whatever you started and have not finished (read back from the day's log, so a
`cdno` start from the terminal counts too), a one-line log composer, commitments due soon, and an
energy-filtered shortlist of one action per project to pick from. Any quietly lapsed habits sit at
the foot. **Calendar** is a month grid of your journal — days with a daily note are marked, today carries a
ring wherever you have paged to, and clicking a day opens it in a panel beside the grid that reads
read-only and jumps to the previous or next day, the day's week, or its month. On a narrower window
the grid collapses behind a **The month** toggle instead of sitting alongside. **Weekly** is a guided, stop-anywhere five-step review with a labelled stepper and Back/Next; its
wins step hands you the week's completions and log lines as cards you tick and reshuffle rather than a
blank box to compose in. **Monthly** is the review at the highest altitude — "am I still pointed at the right questions?" —
stepped like the weekly: questions, portfolio health, the five-slot project allocator, stewardship
trends, the six-week lookahead, and a focus step that writes wins, themes and next month's focus to
the monthly note. It is the one review that leaves an artefact, which is what gives the monthly
cadence a reason to come back.

**Operations** is delivery. **Projects** heads the group and says how many of your slots are taken
("3 of 5"), with each active project listed beneath it; every one has a full map
(`/projects/<slug>`) leading with its current state, then the next actions, blockers and milestones,
with backlinks and recent log mentions kept to a few and the rest a click away. **Actions** is the
cross-project list of next actions: a project rail with counts down the left, and a filter bar of
context and energy chips plus a text filter across the top. Every chip carries its own count, and
one of the energy chips is **untagged** — most bullets carry no `(deep|medium|light)` suffix, and
they are work like any other. Filtering says how much it is hiding rather than just showing less. **Commitments** is everything promised — what someone else is counting on, as
against what you merely decided to do (context-coloured, never red). It reads as a timeline banded
into **This week** / **Next week** / by month, or as a month grid with a dot per promise; the
horizon is yours to set, from a fortnight up to everything. **Stewardships** are the perpetual responsibilities. They never
complete, so the list shows status rather than progress: quietest first, with a count of how many
have gone quiet and a filter down to just those, and freshness read as ink emphasis rather than a
colour. Logging is one click from a row. A stewardship's detail leads with **Log entry** in its
header and draws each tracked series as a calm trend — counts and volumes (reps, laps, sessions) as
columns, continuous measures (a weight, a pace) as lines, always in the context hue and never as a
target to hit; charts sit two-up behind an activity filter.

**Inquiry** is investigation. **Questions** is the important-questions list that sits above any one
project, grouped into research and life, each showing what is pointed at it and each movable between
active, parked, answered and retired in place. **Portfolios** are the evidence dossiers those
questions accumulate.

Everywhere: `⌘K` opens search-and-jump, `⌘⇧C` summons the global capture window from any app (Enter
files to the inbox, `⌘Enter` appends to today's log), and the menu-bar tray keeps Quick capture /
Open / Quit reachable even with every window closed. `⌘[` and `⌘]` (or the mouse's back and forward
side buttons) step backward and forward through your view history, just like a browser.

`⌘,` opens **Settings**, which holds everything that configures the app or the vault rather than
living in it: Appearance and Reading, a metrics toggle under General, custom CSS under Advanced,
and two full editors — **Vault config**, which edits `.cuaderno/config.toml` in a Raw text view and
a structured Form for note types and schemas (every save validated before it touches disk, with a
live reload whenever the config changes underneath), and **Templates**, the per-note-type template
browser and editor. Both hold real drafts, so Settings will not close over unsaved changes without
asking. See [Editing the config in the app](config-editor.md) for the full walkthrough.
