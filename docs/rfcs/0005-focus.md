# RFC 0005 — Focus: a deliberate, persistent "what I am on" that agents respect

| | |
|---|---|
| **Status** | Draft — 2026-10-02 |
| **Tracked by** | (to be filed) |
| **Affects** | `cdno-core` (one index migration, reconciliation), `cdno-domain` (`vault/context.rs`, `vault/projects/actions.rs`, `vault/lint.rs`, config), `cdno-cli` (`cdno action`, `cdno now`), `cdno-mcp` (two tools, one field on every write result, tool descriptions), `examples/` (skills, a Claude Code hook), `docs-site` |
| **Related** | #568 (`start_unplanned_action`, the typo-becomes-an-action failure), `a_promotion_between_start_and_close_strands_the_focus` (the known stranding), #564 (the `reason:` continuation line), #601 (raw-write exceptions), RFC 0004 (closure recipe, `reason:` on a cascade) |

> **Authorship.** Drafted by Claude (Anthropic) from a design conversation with the maintainer on
> 2026-10-01/02. The goal in §2, the decision that setting and unsetting a focus must be
> deliberate, the cache, and the carry-over default are the maintainer's; the survey in §3, the
> marker design in §5 and the staging in §8 are Claude's. Where the text says "the maintainer
> confirms", the decision is open and §9 names it.

---

## 1. Summary

The vault already knows what you are in the middle of. `start_action` writes
`- **HH:MM**: started [[slug]] — text` into today's `## Logs`, `complete_action` and
`drop_action` write the matching close, and `Vault::current_focus` replays the day to answer
"what am I on". `cdno now` and the `current_focus` MCP tool expose it. Nothing else uses it.

This RFC turns that read into a **guardrail for agents**: the thing an ADHD person asked an
agent to help with is the thing the agent helps with, and a change of subject is noticed, named
and recorded rather than silently followed. To carry that weight the focus has to become:

- **Deliberate at both ends.** Starting is already a verb. Stopping without finishing is not:
  the only exits today are `complete`, `drop`, and midnight. §5.1 adds `pause` (with a re-entry
  hint) and `switch` (with a reason), so every change of focus is one explicit line in the log.
- **Persistent across midnight, briefly.** A focus started yesterday and not closed is still
  the focus this morning (§5.2). The look-back window is configurable; the default is one day:
  carry overnight, expire after that.
- **Cheap to read.** The current focus is cached in the SQLite index, written in the same
  transaction as the log line and rebuilt from the log whenever the index is reconciled (§5.3).
  The log stays the source of truth; the cache is a cache, by the same contract `index.db`
  already has. `cdno now --json` reads the cache when present and replays when absent, so a
  prompt segment or a Claude Code hook can call it on every turn.
- **Robust to promotion.** `promote_action` rewrites the bullet an open start points at, which
  today strands the focus for the rest of the day. §5.4 fixes it before anything depends on it.
- **Visible to agents without being asked.** Every MCP write result carries the current focus
  (§5.5), the tool descriptions and the skills state the detour protocol (§5.6), and a
  `UserPromptSubmit` hook example injects `cdno now --json` into every Claude Code turn (§6.6).

What this RFC deliberately does **not** do: the server never refuses a write because it is
outside the focus (§5.6), no `focus:` field is added to any note or config, and nothing in the
log ever says "drifted".

### 1.1 How you will use it

Tuesday, 09:10. You tell Claude "let's get the methods section drafted". The agent calls
`start_action` (or you run `cdno action start --project thesis --query "methods"`). The log gets:

```
- **09:10**: started [[thesis]] — Draft methods section (deep)
```

At 10:40 you ask the agent to look into why the CI on another repo is red. It sees the focus on
every write result and in the hook-injected line, and answers:

> You're on *Draft methods section* for `thesis`. Shall I capture "CI red on cuaderno" to the
> inbox and carry on, or is this a switch?

"Capture it" writes an inbox item tagged with where your attention was; you stay on the
methods. "Switch, Anna is blocked on it" writes:

```
- **10:40**: action paused on [[thesis]] — Draft methods section (deep)
    next: the related-work paragraph is half done; pick up at "Prior approaches"
    because: Anna is blocked on CI
- **10:40**: started [[cuaderno]] — Fix red CI on main (medium)
```

You finish at 12:30 (`complete_action`), the focus clears, and `cdno now` says
`Nothing started. Last paused: thesis — Draft methods section (10:40), next: …`.

Wednesday, 08:50. You forgot to close anything last night and the thesis start from Tuesday
afternoon is still open. `daily-orientation` opens with:

> You're still on *Draft methods section* (`thesis`) from yesterday 14:05. Resume, or pause it
> with a note on where you got to?

Thursday, same situation, but the start was Tuesday: outside the one-day window, so it has
expired. Orientation says nothing is in focus and suggests a project as it does today.

---

## 2. Motivation

The maintainer's goal, in their words: use the focus mechanism "to allow agents to have
guardrails and avoid an ADHD person drifting from what they should be working on", with
"setting and unsetting a focus" as "a deliberate action".

Two ADHD failure modes bracket the design. **Drift**: a tangent arrives, the agent follows it
helpfully, and an hour later the morning's intention has not been touched. **Hyperfocus**: the
same person stays on one thing through a commitment due at 14:00. Both are visible in the log if
the log is kept, and both are things an agent can speak up about if it knows the focus. Today it
does not know, because nothing injects it, and the mechanism it would read has three holes
(§3.2) that would make an enforcing agent confidently wrong.

The non-goal is as important as the goal. A guardrail that blocks, nags or shames gets switched
off, and then there is no guardrail. So the rule throughout is: **the override is one sentence,
never a dead end**, and the record is neutral.

---

## 3. Background — the focus mechanism today

### 3.1 What exists

| Piece | Where | Behaviour |
|---|---|---|
| Open marker | `format_started_log_entry`, `vault/projects/actions.rs` | `started [[slug]] — <resolved bullet text>` |
| Close markers | same file | `action done on [[slug]] — text`, `action dropped on [[slug]] — text` (+ indented `reason:`) |
| Reader | `Vault::current_focus`, `vault/context.rs` | Replays **today's** `## Logs`; most recent start with no matching close wins; several starts in a day are normal |
| Parser | `parse_focus_marker` | Requires the `- **HH:MM**: ` stamp and the em dash U+2014; prose never matches |
| Lint | `focus_marker_issues`, `vault/lint.rs` | Warns on a near-miss marker the reader will skip, naming the cause |
| CLI | `cdno action start [--unplanned]`, `cdno now [--json]` | `now --json` emits `{project, action, started}`, all null when nothing is open |
| MCP | `start_action`, `start_unplanned_action` (write); `current_focus` (read) | The read sits on the read-only surface |

The only callers of `current_focus` are `cdno now` and the MCP tool. `get_orientation`, the
reviews and every skill in `examples/skills/` ignore it; `daily-orientation`'s surface notes say
so explicitly ("No stored focus … it does not read or write a focus marker").

### 3.2 The holes

**H1 — There is no honest way to stop.** A focus ends by `complete`, `drop`, or midnight. "I am
stopping, this is neither done nor abandoned" has no verb, so the person either drops work they
mean to resume, or leaves the focus pinned to something they stopped hours ago. An agent
enforcing a pinned stale focus is worse than no agent.

**H2 — A second start stacks silently.** `start_action` while another start is open writes a
second `started` line. The reader takes the most recent, so it works, but the log never records
that a switch happened or why, and if the second is closed the reader falls back to the first.
The switch, which is the single most useful event for a weekly review to count, is invisible.

**H3 — Promotion strands the focus.** `promote_action` rewrites the matched bullet to
`[[actions/<slug>]] (energy)`. The open `started` line carries the old text; the eventual close
carries the new one; they never pair, and the focus names the old text until midnight. Pinned by
`a_promotion_between_start_and_close_strands_the_focus` and documented on every surface as a
limitation.

**H4 — Midnight is an accident, not a decision.** The reader replays one day, so a focus ends
at 00:00 because the implementation reads one file, not because anyone decided a focus should
not survive sleep.

**H5 — Nothing injects it.** An agent only knows the focus if it calls `current_focus`, and
nothing makes it. The cost of asking is one tool round trip per turn, which is why nobody asks.

### 3.3 Why not a `.active-focus` file

The maintainer raised it: "wouldn't a short `.active-focus` be more agile and direct?" The
token argument does not hold — the agent never reads the log, it reads the thirty-token result
`current_focus` computes, which is the same size whichever store backs it. The latency argument
holds for one case only: a prompt segment or a hook that spawns `cdno` on every keystroke should
not pay for a full vault open. A file as the **source of truth**, though, breaks four things the
project is built on: two stores that can disagree (hand-edit the log, forget the file); a hot
single-line file that sync tools fight over, where the append-only log merges; the log line
becoming optional, which is the data every review metric is made of; and nothing for lint to
check, since a file is simply right or silently wrong.

So: the log is the truth, and there is a cache. §5.3.

---

## 4. Terminology

- **Focus** — the most recent `started` marker, within the look-back window, with no matching
  close. At most one at a time by construction (§5.1 makes a second start a `switch`).
- **Close** — any of `action done on`, `action dropped on`, `action paused on` whose
  `[[slug]] — text` pairs with an open start.
- **Window** — how many days back from today the reader walks before giving up; `0` means
  today only (the current behaviour), `1` means today and yesterday (the new default).
- **Detour** — a request to an agent that does not belong to the focus's project.

---

## 5. Proposal

### 5.1 Two verbs: `pause` and `switch`

```bash
cdno action pause   [--next "<re-entry hint>"] [--because "<reason>"]
cdno action switch  --project <slug> --query <text> [--because "<reason>"] [--next "<hint>"]
cdno action switch  --project <slug> --unplanned --title <t> --energy <e> [--because …] [--next …]
```

with `pause_action` and `switch_action` over MCP. Neither takes a project or query for the thing
being paused: there is exactly one focus, and it is the one being paused. Pausing with nothing
open is an error (`NoFocus`), not a no-op, because a skill that pauses blindly is a bug.

**`pause`** writes one entry:

```
- **HH:MM**: action paused on [[slug]] — <resolved bullet text>
    next: <hint>          (when given)
    because: <reason>     (when given)
```

The bullet stays on the map untouched; an attached action note stays `active`. `paused` is a
new close marker (`LOG_ACTION_PAUSED_PREFIX`), added to the pair the reader and lint already
share. It joins the `reason:` family of continuation lines (#564): indented, `key: value`, never
parsed back by the focus reader, readable by the reviews.

**`switch`** is `pause` of the open focus and `start` of the new one, in **one transaction**,
with the same `--unplanned` split `start` has (#568: a typo must not create an action). The log
is the pair of lines above, so no new marker is needed: *a pause immediately followed by a start
is a switch*, and a review that wants to count switches counts exactly that. Switching with
nothing open is a plain start; the verb exists so an agent can express intent in one call.

**`start` with a focus already open is refused** with `FocusOpen { project, action }`, naming
the open one and `switch` as the remedy. This is the one new refusal, and it is the deliberate
act the maintainer asked for: you can always switch, but you say so. Over MCP it is a
caller-actionable rejection (#560), not `INTERNAL_ERROR`.

**`complete` and `drop` are unchanged**, including on a paused bullet: a paused action that is
later completed logs `action done on`, and the reader (which saw the pause as the close) simply
has nothing to pair it with. Lint does not warn on that; a close with no open start is ordinary.

### 5.2 The focus survives midnight, within a window

The reader stops replaying one day and walks back from today, newest first, until it finds a
start or a close, or runs out of window:

```toml
[focus]
carry_over_days = 1   # 0 = today only (pre-RFC behaviour); default 1
```

The rule, precisely: **the focus is the most recent `started` marker with no matching close,
among the daily notes from `today - carry_over_days` to `today`.** A start older than the window
is not a focus, whatever its state. The walk reads at most `carry_over_days + 1` notes, newest
first, folding each day's entries exactly as the one-day reader does today, and stops as soon as
a day yields an open start. A day that contains only closes does not settle the question on its
own: those closes may pair with a start on the day before, so the walk continues. A close on a
later day always pairs with an earlier start of the same text, which is why the fold is run
oldest-to-newest *within* the notes the walk has read, not per day in isolation.

`CurrentFocus` gains a `date: NaiveDate`, and `cdno now` says `since yesterday 14:05` when it is
not today. `--json` adds `"date"`.

A paused focus is not a focus, but orientation wants it. `Vault::last_paused(window)` returns
the most recent `action paused on` within the same window, with its `next:` and `because:`
lines, so the morning can offer it back (§5.6). It is a read of the same notes the focus walk
already opened.

### 5.3 The cache

Migration `003_focus.sql` adds a one-row table:

```sql
CREATE TABLE focus (
    id       INTEGER PRIMARY KEY CHECK (id = 1),
    project  TEXT NOT NULL,
    action   TEXT NOT NULL,
    started  TEXT NOT NULL,   -- RFC 3339 local datetime
    date     TEXT NOT NULL    -- the daily note it was read from
);
```

Empty means no focus. The `VaultIndex` trait gains `focus()`, `set_focus(Option<&FocusEntry>)`;
`MemoryIndex` holds an `Option`.

- Every focus write — `start`, `start_unplanned`, `pause`, `switch`, `complete`, `drop`, and
  the promotion fix in §5.4 — stages `set_focus` in the **same `VaultTransaction`** as the log
  line, so the index step runs only after the file write succeeded, and an index failure is
  `IndexStale` as for every other write. The lock covers both.
- `current_focus` reads the cache first. **A cache hit is trusted only if its `date` is within
  the window**; a hit from outside the window is treated as empty (the focus expired while
  nobody wrote). A miss replays per §5.2 and fills the cache. There is no "verify on every read":
  that would make the cache pointless.
- **Startup reconciliation rebuilds the cache from the log** whenever any daily note in the
  window was reconciled (changed, added or removed). A hand edit to today's log is therefore
  picked up by the next `cdno` or MCP start, the same way an edited project map is. Deleting
  `.cuaderno/` loses nothing; `cdno reindex` rebuilds it.
- **`lint` always replays and never reads the cache**, and reports a cache that disagrees with
  the replay as a warning naming `cdno reindex`. This is the check that keeps the cache a cache.
- `cdno now` adds `--replay` to force the walk (what lint does), for debugging a disagreement.

Nothing new is written outside `.cuaderno/index.db`, and the raw-write exceptions in `CLAUDE.md`
(#601) gain no new entry.

### 5.4 Promotion no longer strands the focus

`promote_action`, when the bullet it matched is the current focus, writes in the same
transaction, after the rewrite:

```
- **HH:MM**: action paused on [[slug]] — <old bullet text>
    because: promoted to [[actions/<new-slug>]]
- **HH:MM**: started [[slug]] — [[actions/<new-slug>]] (energy)
```

and sets the cache to the new text. The close verbs then match, the focus follows the note, and
the log shows the rename as what it is. This is §5.1's `switch` machinery with a fixed reason,
which is why `pause` lands before the fix (§8). The test
`a_promotion_between_start_and_close_strands_the_focus` is renamed to assert the opposite, and
the "one exception" paragraphs in `now.rs`, the tool descriptions, `cli/action.md`, `cli/now.md`
and `troubleshooting.md` are deleted.

### 5.5 Every MCP write result carries the focus

`WriteResult` (`dto.rs`) gains `focus: Option<FocusDto>` — `{project, action, started, date}` —
filled from the cache after the write committed. `appended_tail` set the precedent: a write
returns read-back proof; this returns the one piece of state the next decision needs. The cost is
thirty tokens per write, which is the whole point of §3.3: an agent that never calls
`current_focus` still cannot miss it.

### 5.6 Agents: the detour protocol

The server **never refuses a write for being outside the focus.** Drift is mostly not a vault
write, and the writes that do happen during a detour — `capture`, `note_to_daily` — are the
release valve: "park the thought and get back" is the ADHD-friendly move, and blocking it makes
drift worse. The guardrail is behaviour, stated where agents read:

**Tool descriptions** (`current_focus`, `start_action`, `switch_action`, `pause_action`,
`capture`) state the protocol in the imperative:

1. When a request does not belong to the focus's **project** (compare at project level; moving
   between bullets of one project is not a detour), say so in one sentence.
2. Offer to `capture` it and continue.
3. Ask whether this is a switch. Never call `switch_action` or `pause_action` on your own
   judgement; the person says so. Never silently start a second thing.
4. If the focus has been open longer than a commitment that is due today allows, say that too
   (`get_orientation.commitments` is already in hand). Hyperfocus gets the same one sentence.

**`capture` and `note_to_daily` tag the detour.** When a focus is open, the inbox item's
frontmatter gets `captured_during: <slug>` and a `## Notes` entry's log line gets an indented
`during: [[slug]]` continuation. Triage later shows where attention leaked and from what, with
no new state.

**Skills.** `daily-orientation` reads `current_focus` first and, if one is open from yesterday,
opens with resume-or-pause before anything else; if `last_paused` has a `next:`, it offers that
as the suggested first action instead of a fresh pick. `complete-action` and `add-action` read
the focus from the write result and apply step 1 above. `weekly-review` is §8 T8.

**Wording.** The log says `paused`, `started`, `during`. The reviews may editorialise; the
record does not. No marker, field or message in this RFC uses "drift", "distraction" or
"off-task".

### 5.7 CLI surface

`cdno now` output, both states:

```
$ cdno now
On thesis — Draft methods section (deep), since yesterday 14:05.

$ cdno now
Nothing started.
Last paused: thesis — Draft methods section (10:40), next: pick up at "Prior approaches"
```

`cdno now --json`:

```json
{"project":"thesis","action":"Draft methods section (deep)","started":"14:05","date":"2026-10-01",
 "last_paused":null}
```

`last_paused` is `{project, action, at, date, next, because}` or null. The nulls-not-missing
convention of the existing shape is kept so a caller can test one field.

`cdno action start` with a focus open prints the refusal and the exact `switch` command to run,
and in a terminal offers to run it.

---

## 6. Detailed design

### 6.1 `cdno-core`

- `migrations/003_focus.sql` and its `MIGRATIONS` entry (append-only).
- `VaultIndex`: `focus()`, `set_focus()`. `SqliteIndex` and `MemoryIndex` implement both.
- `VaultTransaction`: an index op `SetFocus(Option<FocusEntry>)`, staged like any other.
- `reconcile`: returns whether any path under `journal/*/daily/` changed; the domain uses that
  to decide whether to rebuild the cache at startup. Core does not know the window.
- `VaultConfig`: `[focus] carry_over_days: u32`, default 1, `deny_unknown_fields` like
  `[tracking]`.

### 6.2 `cdno-domain`

- `projects/actions.rs`: `LOG_ACTION_PAUSED_PREFIX`, `format_action_paused_log_entry` (with the
  optional `next:` / `because:` continuations, in that order), `pause_action`, `switch_action`
  (and `_unplanned`), the `FocusOpen` refusal in `start_action` / `start_unplanned_action`, the
  §5.4 branch in `promote_action_with_vars`. Every focus write stages `SetFocus`.
- `context.rs`: `current_focus` becomes cache-first with the window check; `replay_focus(window)`
  is the walk; `last_paused(window)`; `CurrentFocus.date`; `parse_focus_marker` unchanged.
  The fold already clears a start on any close marker; `paused` is added to the list of close
  prefixes it recognises, next to done and dropped.
- `mod.rs`: `Vault::new` rebuilds the cache after reconciliation when a daily note changed, or
  when the cache's `date` is outside the window.
- `lint.rs`: `paused` joins `FOCUS_MARKER_PREFIXES`; a new rule compares cache and replay.
- `DomainError`: `NoFocus`, `FocusOpen { project, action }`.
- Tests (`tests/unit/context_tests.rs`, `actions_tests.rs`, `lint_tests.rs`): the walk across a
  window boundary; `carry_over_days = 0` reproduces today's behaviour exactly; a paused start is
  not a focus; switch is atomic (a failing start leaves no pause line); promotion follows the
  note; cache hit outside the window reads as empty; reconcile after a hand edit rebuilds;
  `last_paused` returns the continuations.

### 6.3 `cdno-cli`

- `cdno action pause`, `cdno action switch` (flags-and-prompts; `--because` and `--next` are
  optional and never prompted — a prompt for a reason is friction, the flag is there when the
  person has one).
- `cdno now`: the §5.7 shape, `--replay`.
- Tests in `tests/action.rs`, `tests/now.rs`: wiring only.

### 6.4 `cdno-mcp`

- `pause_action`, `switch_action` (write); `current_focus` gains `last_paused`. Catalogue
  61 → 63.
- `FocusDto`, `focus` on `WriteResult`.
- `FocusOpen` maps to a caller-actionable rejection carrying the open focus and the remedy.
- Tool descriptions per §5.6; every "promotion strands the focus" sentence removed.
- `tests/handlers_operations.rs`, `handlers_context.rs`, `e2e_stdio.rs`: the refusal shape, the
  `focus` field after a write, pause and switch round trips.

### 6.5 Documentation

- `docs-site/src/reference/cli/action.md`, `now.md`, `configuration.md` (`[focus]`),
  `troubleshooting.md` (the "names the wrong action" entry becomes "I promoted it" → nothing to
  do, and a new "says I'm still on yesterday's thing" → that is the window; pause it or set
  `carry_over_days = 0`), `reference/mcp/reads.md`, `writes.md`, `concepts/contexts-and-energy.md`
  (a short "focus" section), `tutorials/daily-loop.md`.
- `docs/design.md`: the marker family gains `paused`; `CLAUDE.md` history-preservation
  paragraph lists it.
- `CHANGELOG.md`, `STATUS.md`.

### 6.6 `examples/`

- `examples/hooks/claude-code/focus.json` + `focus.sh`: a `UserPromptSubmit` hook that runs
  `cdno now --json` and emits one line, `Focus: thesis — Draft methods section (since 14:05)`
  or `Focus: none (last paused: …)`. The README states that the hook is what turns §5.6 from
  advice into behaviour, and that it costs one cached read per turn.
- `examples/skills/daily-orientation/SKILL.md`: the surface note "No stored focus" is replaced
  by the §5.6 obligation; `current_focus` joins the tools table and step 1.
- `examples/skills/references/ADHD-PRINCIPLES.md`: a "Focus and detours" section with the
  protocol and the wording rule.

---

## 7. Compatibility

- Existing vaults: migration 003 runs on first open; the cache fills from the replay. A vault
  with a start left open **yesterday** will, on upgrade, report it as the focus — that is the
  new default working as intended, and `cdno now` says `since yesterday`. Set
  `carry_over_days = 0` to keep the old behaviour.
- Existing logs parse unchanged; `paused` is additive.
- `start_action` with a focus open was a silent second start; it is now a refusal. Any script or
  skill that relied on stacking starts must call `switch_action`. No shipped skill does.
- `cdno now --json` adds fields and removes none.
- The desktop app is not affected; it is retired.

---

## 8. Implementation plan — staged

Each stage is one PR, green on its own, in this order because each depends on the one before.

| | Stage | Depends on |
|---|---|---|
| T0 | `paused` marker: constant, formatter, reader close-prefix, lint prefix, `pause_action` domain verb + tests | — |
| T1 | Promotion follows the note (§5.4), test renamed and inverted, limitation paragraphs deleted everywhere | T0 |
| T2 | `switch_action` (+ unplanned), `FocusOpen` refusal on `start`, `NoFocus` on pause | T0 |
| T3 | Window: `[focus] carry_over_days`, the walk, `CurrentFocus.date`, `last_paused` | T0 |
| T4 | Cache: migration 003, trait methods, `SetFocus` op staged by every focus write, startup rebuild, lint disagreement rule, `cdno now --replay` | T2, T3 |
| T5 | CLI (`action pause`, `action switch`, `now` shape) and MCP (two tools, `FocusDto` on `WriteResult`, rejection mapping) | T4 |
| T6 | Tool descriptions and skills (§5.6), the Claude Code hook example, `captured_during` / `during:` on `capture` and `note_to_daily` | T5 |
| T7 | Docs-site, `design.md`, `CLAUDE.md`, `CHANGELOG.md`, `STATUS.md` | T5 |
| T8 | Weekly review reads the log for switches per day, focus time per project against `This Week's Goal`, top `during:` sources — **separate RFC**, once a few weeks of log exist | T6 |

T0 and T3 can land in parallel; T1 and T2 both wait on T0 only.

---

## 9. Decisions and open questions

- **D1 — The focus survives midnight; default window one day.** Maintainer's decision
  (2026-10-02): "carry overnight and expire after that", configurable.
- **D2 — The cache lives in `index.db`, never authoritative.** Maintainer agreed to the cache;
  the placement in the index rather than a new file is the drafter's, on the grounds that the
  index already has the rebuilt-never-authoritative contract and the lock. **The maintainer
  confirms** the placement, or asks for the `jq`-able `.cuaderno/focus.json` sidecar as an
  *additional* cache written from the same transaction (§3.3 names the one case it serves).
- **D3 — `pause` and `switch`, not `focus set/clear`.** The verbs live under `cdno action`
  because a focus *is* a started action, and `start` already lives there. A `cdno focus`
  namespace would duplicate `action start` under a second name.
- **D4 — A switch is a pause followed by a start; no `switched` marker.** Keeps the reader's
  marker family at four and makes "count switches" a trivial log query.
- **D5 — A second `start` is refused, not stacked.** This is the "deliberate" requirement made
  concrete. The remedy is always named.
- **D6 — Project-level focus (a start with no bullet) is out of scope.** `start_unplanned`
  makes a bullet cheaply, and a focus without a bullet has no close verb. Revisit if the
  bullet turns out to be the friction.
- **D7 — The server never refuses an out-of-focus write.** §5.6. Not up for a flag.
- **D8 — `--because` and `--next` are never prompted.** A prompt for a reason at the moment of
  switching is exactly the friction that makes people not record the switch.
- **D9 — `detour_budget` (suggest a pause after N captures during one focus) is deferred** to
  T8's RFC, with the metrics that would justify a number.
- **Open: the window unit.** `carry_over_days` counts calendar days, so a start at 23:50 carries
  into the next ten minutes and the following whole day. A rolling 24 or 36 hours would be
  more intuitive for a night owl. **The maintainer confirms** days (simple, matches the
  journal's unit) or asks for hours.
- **Open: the hook's placement.** `examples/hooks/` is new. **The maintainer confirms**, or
  prefers it under `examples/skills/` beside the skills it serves.

---

## 10. Verification

- `cargo test -p cdno-domain --test unit -- unit::context_tests` and `unit::actions_tests` cover
  §6.2; `-- unit::lint_tests` the two lint rules.
- `cargo test -p cdno-cli --test action` and `--test now` cover the wiring, including
  `carry_over_days = 0` in a temp vault's `config.toml`.
- `cargo test -p cdno-mcp --test handlers_operations` and `--test e2e_stdio` cover the refusal,
  the `focus` field and the two new tools.
- Manual, in the repo's own dev vault: start, promote, complete; start, wait for the date to
  roll (or write yesterday's note by hand), `cdno now`; delete `.cuaderno/index.db`, `cdno now`
  again, same answer; edit today's log by hand, `cdno now`, the edit is seen; `cdno lint` clean
  throughout, and dirty after writing a `paused` line with an ASCII hyphen.
