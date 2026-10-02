# RFC 0005 — Focus: a deliberate, persistent "what I am on" that agents respect

| | |
|---|---|
| **Status** | Draft, amended after review — 2026-10-02 (three-seat review, three rounds, on PR #718; all seats approve for amendment; §9 names what the maintainer still confirms) |
| **Tracked by** | PR #718 |
| **Affects** | `cdno-domain` (`vault/context.rs`, `vault/projects/actions.rs`, `vault/projects/closing.rs`, `vault/orient.rs`, `vault/lint.rs`, config), `cdno-cli` (`cdno action`, `cdno now`), `cdno-mcp` (four tools, server instructions, one field on write results, two rejection codes), `docs/cli-ergonomics.md`, `examples/` (skills, a Claude Code hook), `docs-site` |
| **Related** | #568 (`start_unplanned_action`, the typo-becomes-an-action failure), `a_promotion_between_start_and_close_strands_the_focus` (the known stranding), #564 (the `reason:` continuation line), #560 (caller-actionable rejections), #196 (the write lock), RFC 0004 (closure recipe, rejection shapes) |

> **Amended after review.** The first draft was reviewed on the PR by three seats (CLI and MCP
> surface; method and user; safety, tests and maintenance), three rounds, seats replying to each
> other until they agreed. The review removed the index cache entirely (§3.3: measured, it saves
> nothing a hook can feel, and it had two coherence holes), replaced the promotion fix with a
> fold rule over the line promotion already writes (§5.4), added `resume` and the `resumed`
> marker (§5.3), renamed `because:` to `reason:` (#564 family), made the `next:` hint a
> skippable prompt (§5.1, D8), moved `last_paused` onto `get_orientation` and
> `get_project_context` with its own look-back (§5.5), rewrote the detour protocol with a
> "do it as an aside" path, once-per-topic, exemptions and a consent rule (§5.6), and
> restated the cross-day stopping rule (§5.2). The text below is the amended version; the
> record is on the PR.

> **Authorship.** Drafted by Claude (Anthropic) from a design conversation with the maintainer on
> 2026-10-01/02. The goal in §2, the decision that setting and unsetting a focus must be
> deliberate, and the carry-over default are the maintainer's; the survey in §3, the marker design
> in §5 and the staging in §8 are Claude's, amended by the review. Where the text says "the
> maintainer confirms", the decision is open and §9 names it.

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
  hint) and `switch` (with an optional reason), so every change of focus is one explicit line in
  the log. A second `start` while a focus is open is refused and names the remedy.
- **Persistent across midnight, briefly, and resumable.** A focus started yesterday and not
  closed is still the focus this morning (§5.2); the look-back window is configurable, default
  one day: carry overnight, expire after that. `resume` (§5.3) re-anchors a carried focus, or
  the most recent pause, to today, so work that continues keeps its focus and work that stopped
  lets it lapse.
- **Cheap to read, with no cache.** `current_focus` replays at most `carry_over_days + 1`
  daily notes. Measured on a 2 100-note vault, a warm `cdno now` is 60–100 ms end to end, of
  which the replay is unmeasurable (§3.3). The log is the only store.
- **Robust to promotion.** `promote_action` rewrites the bullet an open start points at, which
  today strands the focus for the rest of the day. §5.4 teaches the reader to follow the line
  promotion already writes; nothing new is logged.
- **Visible to agents without being asked.** Every MCP write result carries the current focus
  (§5.5), `get_orientation` carries it with each project's last pause, the server instructions
  state the detour protocol (§5.6), and a `UserPromptSubmit` hook example injects
  `cdno now --line` into every Claude Code turn (§6.6).

What this RFC deliberately does **not** do: the server never refuses a write because it is
outside the focus (§5.6), no `focus:` field is added to any note, index or config, and nothing
in the log, in a tool description or in a skill ever says "drift".

### 1.1 How you will use it

Tuesday, 09:10. You tell Claude "let's get the methods section drafted". That names the work,
so the agent calls `start_action` (or you run
`cdno action start --project thesis --query "methods"`). The log gets:

```
- **09:10**: started [[thesis]] — Draft methods section (deep)
```

At 10:40 you ask the agent to look into why the CI on another repo is red. It sees the focus on
every write result and in the hook-injected line, and answers in one sentence:

> That's outside *Draft methods*. I'll capture "CI red on cuaderno" and we stay on methods,
> unless you want it now or want to move over to it.

"Capture it" writes an inbox item tagged with where your attention was, and the agent gives you
one return cue. "Just look, then back" is an aside: the agent looks, nothing is logged, and it
does not ask again about that thread. "Move over, Anna is blocked on it" is the decision, so the
agent calls `switch_action`, drafting the `next:` from what it saw you do:

```
- **10:40**: action paused on [[thesis]] — Draft methods section (deep)
  next: related-work paragraph half done; pick up at "Prior approaches"
  reason: Anna is blocked on CI
- **10:40**: started [[cuaderno]] — Fix red CI on main (medium)
```

You finish at 12:30 (`complete_action`), the focus clears, and at 14:05 you say "back to the
methods". That is a resume of the most recent pause:

```
- **14:05**: resumed [[thesis]] — Draft methods section (deep)
```

and `cdno now` says `On thesis — Draft methods section (deep), picked up 14:05.` You never close
it that evening.

Wednesday, 08:50. `daily-orientation` greets you with the day and date, leads with yesterday's
win (the CI fix), lists what is due, and then offers the one recommended pick:

> Yesterday you were mid-way through *Draft methods* on `thesis` (since 14:05). I suggest
> picking it up there (Recommended), or pausing it with a note on where you got to.

"Yes" writes `resumed [[thesis]] — Draft methods section (deep)` at 08:50, and the focus is
anchored to today. Thursday, had you said nothing on Wednesday, the Tuesday start would be
outside the one-day window: expired, and orientation would mention it once as last worked on.

---

## 2. Motivation

The maintainer's goal, in their words: use the focus mechanism "to allow agents to have
guardrails and avoid an ADHD person drifting from what they should be working on", with
"setting and unsetting a focus" as "a deliberate action".

Two ADHD failure modes bracket the design. **The detour**: a tangent arrives, the agent follows
it helpfully, and an hour later the morning's intention has not been touched. **Hyperfocus**:
the same person stays on one thing past a commitment due today. Both are visible in the log if
the log is kept, and both are things an agent can speak up about if it knows the focus. Today it
does not know, because nothing injects it, and the mechanism it would read has holes (§3.2)
that would make an agent acting on it confidently wrong.

The non-goal is as important as the goal. A guardrail that blocks, nags or shames gets switched
off, and then there is no guardrail. So the rule throughout is: **the override is one sentence,
never a dead end**, and the record is neutral.

---

## 3. Background — the focus mechanism today

### 3.1 What exists

| Piece | Where | Behaviour |
|---|---|---|
| Open marker | `format_started_log_entry`, `vault/projects/actions.rs` | `started [[slug]] — <resolved bullet text>` |
| Close markers | same file | `action done on [[slug]] — text`, `action dropped on [[slug]] — text` (+ indented `reason:`, #564) |
| Promotion line | `promote_action_with_vars`, same file | `action promoted on [[slug]] — "title" -> [[actions/<new-slug>]]` (no reader parses it) |
| Reader | `Vault::current_focus`, `vault/context.rs` | Replays **today's** `## Logs`; most recent start with no matching close wins; several starts in a day are normal |
| Parser | `parse_focus_marker` | Requires the `- **HH:MM**: ` stamp and the em dash U+2014; prose never matches |
| Lint | `focus_marker_issues`, `vault/lint.rs` | Warns on a near-miss marker the reader will skip, naming the cause |
| CLI | `cdno action start [--unplanned]`, `cdno now [--json]` | `now --json` emits `{project, action, started}`, all null when nothing is open; `elapsed_since` assumes a same-day start |
| MCP | `start_action`, `start_unplanned_action` (write); `current_focus` (read, returns the DTO or `null`) | 60 tools pinned in `tests/server.rs` and `e2e_stdio.rs` |

The only callers of `current_focus` are `cdno now` and the MCP tool. `get_orientation`, the
reviews and every skill in `examples/skills/` ignore it; `daily-orientation`'s surface notes say
so explicitly, and its step 9 logs a non-marker line that uses the word "focus".

### 3.2 The holes

**H1 — There is no honest way to stop.** A focus ends by `complete`, `drop`, or midnight. "I am
stopping, this is neither done nor abandoned" has no verb, so the person either drops work they
mean to resume, or leaves the focus pinned to something they stopped hours ago. An agent acting
on a pinned stale focus is worse than no agent.

**H2 — A second start stacks silently.** `start_action` while another start is open writes a
second `started` line. The reader takes the most recent, so it works, but the log never records
that a switch happened or why, and if the second is closed the reader falls back to the first.
The switch, which is the single most useful event for a weekly review to count, is invisible.

**H3 — Promotion strands the focus.** `promote_action` rewrites the matched bullet to
`[[actions/<slug>]] (energy)`. The open `started` line carries the old text; the eventual close
carries the new one; they never pair, and the focus names the old text until midnight. Pinned by
`a_promotion_between_start_and_close_strands_the_focus` and documented on every surface as a
limitation — even though the log already carries a line that says exactly what was renamed to
what.

**H4 — Midnight is an accident, not a decision.** The reader replays one day, so a focus ends
at 00:00 because the implementation reads one file, not because anyone decided a focus should
not survive sleep.

**H5 — Nothing injects it.** An agent only knows the focus if it calls `current_focus`, and
nothing makes it. The cost of asking is one tool round trip per turn, which is why nobody asks.

### 3.3 Why there is no cache, and no `.active-focus` file

The maintainer raised a file: "wouldn't a short `.active-focus` be more agile and direct?" The
first draft answered with a cached row in `index.db`, rebuilt from the log. The review removed
it, on two grounds.

**It saves nothing.** The agent never reads the log; it reads the thirty-token result
`current_focus` computes, the same size whichever store backs it. For the hook, the review
measured `cdno now` on a synthetic 2 102-note vault (release build): cold open with no
`index.db`, 14.2 s; warm open, 60–100 ms end to end, of which process start is 4 ms and the
replay of one or two daily notes is unmeasurable. The rest is startup reconciliation's stat
walk, which a cache does not skip. A self-validating row (stamps of the window's notes beside
it) would cost one stat per note, the same as reading them.

**It would have been wrong.** `cdno log` and `append_to_log` stage the daily note through
`stage_daily_logs` and restamp its row on commit, so a marker-shaped line written that way
changes the replay answer, stages no cache update, and leaves a row the reconcile fast path
never revisits. And the rebuild placed in `Vault::new` never runs in the long-lived
`cdno-mcp-server`, which reconciles on an interval through a different seam.

A file as the source of truth fails for the reasons the first draft gave: two stores that can
disagree; a hot single-line file that sync tools fight over, where the append-only log merges;
the log line becoming optional, which is the data every review metric is made of; and nothing
for lint to check.

So: the log is the only store, `current_focus` replays at most `carry_over_days + 1` notes, and
if a hook ever measures lag the follow-up is a `Vault::open_unreconciled` seam for file-only
reads (there is none today; `Vault::new` always reconciles), not a cache.

---

## 4. Terminology

- **Focus** — the most recent open marker (`started` or `resumed`), within the look-back
  window, with no matching close. At most one at a time by construction (§5.1).
- **Close** — any of `action done on`, `action dropped on`, `action paused on` whose
  `[[slug]] — text` pairs with an open marker.
- **Window** — how many days back from today the reader walks before giving up; `0` means
  today only (the current behaviour), `1` means today and yesterday (the new default).
- **Carried** — a focus whose open marker is dated before today.
- **Detour** — a request to an agent that does not belong to the focus's project. An internal
  term: agents do not say it to the person.

---

## 5. Proposal

### 5.1 Two verbs: `pause` and `switch`

```bash
cdno action pause   [--next "<re-entry hint>"] [--reason "<why>"]
cdno action switch  --project <slug> --query <text> [--next "<hint>"] [--reason "<why>"]
cdno action switch  --project <slug> --unplanned --title <t> --energy <e> [--next …] [--reason …]
```

with `pause_action`, `switch_action` and `switch_unplanned_action` over MCP. Neither verb takes a
project or query for the thing being paused: there is exactly one focus, and it is the one
being paused. **`pause` resolves nothing against the map** — the text it logs is the open
focus's own text, so a focus on a since-parked project can still be paused. Pausing with
nothing open is `NoFocus`, a hard error in the CLI (worded gently: `Nothing started — nothing to
pause.`, non-zero exit) and a caller-actionable rejection over MCP; a skill that pauses blindly
is a bug, not a no-op.

**`pause`** writes one entry:

```
- **HH:MM**: action paused on [[slug]] — <focus text>
  next: <hint>          (when given)
  reason: <why>         (when given)
```

The bullet stays on the map untouched; an attached action note stays `active`. `paused` is a
new close marker (`LOG_ACTION_PAUSED_PREFIX`), added to the list the reader and lint already
share. The continuations are indented two spaces like every `reason:` line today
(`format_action_dropped_log_entry`), `reason:` reuses `LOG_REASON_KEY` (#564) so T8 parses one
key, and `next:` is a new key beside it. Neither is parsed by the focus reader; both are read
back by `last_paused` (§5.5).

**`switch`** is `pause` of the open focus and `start` of the new one, in **one transaction**,
with the same `--unplanned` split `start` has (#568: a typo must not create an action) and the
same `conflicts_with`/`requires` wiring at the parser. Over MCP the two forms are two tools,
`switch_action {project, query, next?, reason?}` and
`switch_unplanned_action {project, title, energy, next?, reason?}`, for the reason
`start_unplanned_action` is its own tool: one tool inferring the mode from optional fields is
the fallback #568 removed. The log is the pair of lines above, so no new marker is needed: *a
pause immediately followed by a start is a switch*, and a review that wants to count switches
counts exactly that. Switching with nothing open is a plain start; a supplied `--next` then has
nothing to attach to and the CLI says so rather than dropping it silently. The write lock is not
re-entrant, so `switch` is composed from the `stage_*` helpers inside one transaction, never by
chaining `pause_action` and `start_action`; a failing start therefore leaves no pause line
because the pause was only staged.

**`start` with a focus already open is refused** with `FocusOpen { project, action }`, naming
the open one and the remedy. This is the one new refusal, and it is the deliberate act the
maintainer asked for: you can always switch, but you say so. Precedence: project and bullet
resolution errors (`ActionNotFound`, `AmbiguousAction`) win over `FocusOpen`, so a typo is
reported as a typo; and in `start_unplanned_action` the check precedes the add, so a refusal
creates nothing. `start` on the bullet that *is* the focus is refused too, with
`same_action: true` and the remedy `already_focused` (started today) or `resume_action`
(carried) — `start` never writes a different marker depending on the date (§5.3). Over MCP:

```json
{"code": "focus_open",
 "message": "An action is already in focus. Ask the person before switching; do not retry.",
 "details": {"focus": {"project": "thesis", "action": "Draft methods section (deep)",
                       "started": "09:10", "date": "2026-10-01", "carried": false},
             "attempted": {"project": "cuaderno", "query": "CI"},
             "same_action": false,
             "remedy": "switch_action"}}
```

`RejectionCode` gains `FocusOpen` and `NoFocus`; `classify` is exhaustive on purpose, so the
compile error is the checklist. The CLI emits the same object under `--json`, as RFC 0004 §6.3
does for `ProjectHasOpenItems`, and in a terminal offers to run the switch through
`reports_interactively` (so `--json`, `--no-interactive` and pipes fail fast), confirm default
no, without asking for `--next` or `--reason`.

**`complete` and `drop` are unchanged**, including on a paused bullet: a paused action that is
later completed logs `action done on`, and the reader, which saw the pause as the close, simply
has nothing to pair it with. Lint does not warn on that; a close with no open marker is
ordinary. `drop_project`'s cascade (`OpenItems::Drop`, RFC 0004 §5.3) already logs
`action dropped on` for every open bullet, so a focus on a dropped project clears by the
existing line; `park_project` closes nothing, and the focus stays until paused.

### 5.2 The focus survives midnight, within a window

The reader stops replaying one day and walks back from today:

```toml
[focus]
carry_over_days = 1   # 0 = today only (pre-RFC behaviour); default 1
```

The rule, precisely: **the focus is the most recent open marker with no matching close, among
the daily notes from `today - carry_over_days` to `today`.** An open marker older than the
window is not a focus, whatever its state. The walk reads newest first, folding everything read
so far oldest-to-newest exactly as the one-day reader does today, and **stops when that fold
leaves an open marker standing, else reads the next older note** until the window ends. The
stop is sound because an older note can only add *older* open markers, which the fold's
"most recent wins" never returns while a newer one stands; judging a day in isolation is not
sound (window 2: D-2 `started Z`, D-1 `started Y`, D0 `done Y` — stopping at D-1 misses Z). For
the default window the two rules coincide, which is why the correct one is stated now.

The adversarial cases the review walked, all handled by the existing fold once it spans days: a
start yesterday closed today pairs; two starts yesterday and one close today leave the earlier
standing (as `completing_one_action_leaves_an_earlier_start_standing` already pins within a
day); a close with no open marker is dropped; a pause then a complete of the same text pairs the
complete with nothing; the same text on two projects is keyed by `(project, action)`.

`CurrentFocus` gains `date: NaiveDate`, and `cdno now`'s `elapsed_since` moves from two
`NaiveTime`s to `NaiveDateTime`, so a start at 08:00 yesterday rendered at 10:00 reads 26 h, not
2 h, and a start at 14:05 yesterday rendered at 09:00 reads 19 h, not nothing. The module doc in
`now.rs` that relies on a same-day start is rewritten.

### 5.3 `resume`: re-anchoring a carried or paused focus

A carried focus has, without this verb, no honest way to continue: `start` is refused (§5.1),
`switch` to the same action would log a pause nobody did, and writing nothing lets a Tuesday
start that was worked all Wednesday expire on Thursday.

```bash
cdno action resume [--project <slug>]
```

with `resume_action {project?}` over MCP. It takes **no `--query` or `--title`**: the text comes
from the log, never from the map, so there is no resolve-or-create ambiguity of the kind #568
removed. `project` is optional and never prompted.

- With no `project`: resumes the carried focus if there is one, else the most recent pause
  within the paused look-back (§5.5).
- With `project`: resumes that project's most recent pause — the one `get_orientation` shows
  beside its `top_action`, so an agent resumes what the person said yes to.
- Nothing resumable → `NoFocus`. A different focus open today → `FocusOpen` (no auto-switch,
  D5). The focus already resumed today → `FocusOpen`, `remedy: already_focused`.

It writes one entry, `resumed [[slug]] — <text>`, a new **open** marker
(`LOG_RESUMED_PREFIX`) the fold treats as **close-plus-reopen at the resume stamp**: any open
marker of the same `(project, text)` is cleared and a new one opened at this line's time and
date. That re-stamp is what fixes the Thursday expiry: the window counts from the resume. A
hand-written `resumed` with no open marker reads as a plain start. Pause then resume therefore
round-trips without a second verb, and `last_paused` (§5.5) treats a pause followed by a
`resumed` or `started` of the same text as resumed.

`cdno now` shows today's anchor and keeps the origin:
`On thesis — Draft methods section (deep), picked up 08:50 today (started Tuesday 14:05).` The
hyperfocus check (§5.6) counts from the resume.

`resume_action`'s result carries `resumed_from: {kind: "carried" | "paused", date, next, reason}`
so the agent can read the `next:` hint back to the person on re-entry without a second call.

### 5.4 Promotion no longer strands the focus — nothing new is written

`promote_action` already logs `action promoted on [[slug]] — "title" -> [[actions/<new-slug>]]`
in the same transaction as the rewrite. The fold learns to read that line as a **rename**:

- On an `action promoted on [[p]] — "title" -> [[actions/x]]` head, find the open marker
  `(p, a)` with `strip_energy_suffix(a).trim() == title`.
- Replace its text with `[[actions/x]] (energy)`, where the energy comes from the open marker's
  own suffix (`parse_bullet_energy(a)`), never from the map — promotion refuses a bullet without
  one (`BulletMissingEnergy`), so an open marker lacking it cannot be the subject. The new text
  is exactly what `complete_action` later logs, since that is `parse_open_action_text` of the
  rewritten bullet.
- Keep its start time and date. The person never stopped.
- Parse the title by stripping the leading `"` and splitting on the last `" -> [[`, so a title
  containing `->` survives. No match → the line is ignored, like any close with no open marker.

The parser is as strict as `parse_focus_marker` (stamp plus em dash), `promoted` joins
`FOCUS_MARKER_PREFIXES` so lint reports a near-miss, and old logs pair retroactively (§7). No
`paused`, no `started`, no reset of the start time, no pollution of the T8 switch count, and
`last_paused` never offers a pre-promotion bullet that no longer exists in that form. The test
`a_promotion_between_start_and_close_strands_the_focus` is renamed to assert the opposite, a
test covers a promotion line written by hand, and every "promotion strands the focus" paragraph
is deleted: `now.rs` module doc, the `Start` doc comment shown in `--help`
(`cli/commands/action.rs`), the `start_action`, `start_unplanned_action`, `current_focus` and
`lint` tool descriptions, `cli/action.md`, `cli/now.md` and `troubleshooting.md`.

### 5.5 Reads: write results, orientation, project context

**Every MCP write result carries the focus.** There is no single `WriteResult` type — the
verified-write payloads are built per handler through `verified_write_with`, `ProjectClosureDto`
builds its own, and `NoteToDailyResponse` carries its verification differently — so the
mechanism is: read `current_focus` **inside the same `with_vault` closure** as the write's
`verify()`, after the commit, and thread it into the payload's `build`; never a second
`spawn_blocking`. Which DTOs get it: every one returned by a tool that writes a daily-log line
or changes a project map (`start_*`, `switch_*`, `pause_action`, `resume_action`,
`complete_action`, `drop_action`, `promote_action`, `add_action`, `append_to_log`, `capture`,
`note_to_daily`, the project lifecycle tools). Two rules: a failure to read the focus after a
committed write yields `focus: null`, never a tool error, because an error would tell the agent
a successful write failed; and `focus: null` means exactly what `current_focus` returning `null`
means — nothing open — and the schema doc says so. The DTO is the existing `CurrentFocusDto`
with `date` and `carried` added, not a new type.

**`current_focus` is otherwise unchanged**: it returns the DTO or `null`, as skills test for
today.

**`get_orientation` gains `focus`** (same DTO or null) **and, per project, `last_paused`**
beside `top_action`: `{action, at, date, next, reason}` or null, the project's most recent pause
not followed by a `started` or `resumed` of the same text. **`get_project_context` gets the same
`last_paused`** for its project, so the hint appears when the person opens the project, not only
in the morning. The look-back for pauses is its own setting, independent of the focus window,
because a Friday pause must survive the weekend and most holidays:

```toml
[focus]
carry_over_days     = 1
paused_lookback_days = 14
```

`Vault::last_paused(window)` is one pass over at most `paused_lookback_days + 1` daily notes
yielding every project's last pause (not one scan per project), in the shape
`daily_log_mentions` and `weekly_logs` already use; days with no note cost nothing. Fifteen
reads of entry heads is milliseconds and sits on the orientation path, not the hook path. There
is no index-backed alternative that stays non-authoritative without the machinery §3.3 removed;
none is built.

### 5.6 Agents: the detour protocol

The server **never refuses a write for being outside the focus.** Most detours are not vault
writes, and the writes that do happen during one — `capture`, `note_to_daily`, `append_to_log`
— are the release valve: "park the thought and get back" is the ADHD-friendly move, and
blocking it makes the detour longer. The guardrail is behaviour, stated once in the **server
instructions** (the `with_instructions` text every client receives) with one-line pointers from
the `current_focus`, `start_action` and `switch_action` descriptions, rather than repeated in
five descriptions that cost tokens on every `tools/list`.

**The protocol**, as the instructions will word it:

1. *Compare at project level.* A request that belongs to the focus's project, or to a
   portfolio or question linked to it (a judgement from `get_project_context`, not a rule the
   server checks), is not a detour. Captures, stewardship tracking, commitments, reviews and
   orientation, and reads are never detours.
2. *One sentence, only on a mismatch.* Never restate the focus on a turn that matches it; the
   hook puts it in front of you every turn, the person does not need it back.
3. *Recommend, do not ask open-endedly.* "That's outside *X*. I'll capture it and we stay on
   X, unless you want it now or want to move over to it." Three answers are accepted: capture;
   do it now as an aside (no log line, no switch); move over. Any reply that is not a move
   means carry on.
4. *Ask at most once per topic per focus.* After "just this, then back", do not raise that
   thread again. The server cannot enforce this; it is yours to track in the conversation.
5. *Never ask why.* Record a `reason:` only when the person volunteers one.
6. *A return cue after a capture or an aside*, taken from the focus and its `next:` if any:
   "Back to methods — you were at 'Prior approaches'."
7. *Consent.* If the person explicitly names other work to do now ("let's work on Y", "switch
   to Y"), that is their decision: call `switch_action` directly, drafting `next:` from what
   you saw them do and never inventing one. If a request only touches other work without saying
   to move to it, or `start_action` returns `focus_open`, do not retry: propose the move in one
   sentence and wait. The same holds for setting a focus: "let's work on X" is consent to
   `start_action`; anything weaker gets a proposal. Never call `pause_action` or
   `resume_action` without the person's yes.
8. *Hyperfocus, narrowly.* At most once per focus per day, only when an overdue or due-today
   commitment exists outside the focus's project (`get_orientation.commitments`; commitments
   are date-only, so nothing time-of-day is computed), phrased as information: "Heads up: *X*
   is due today." Elapsed-time nudges are T8's to design.

**`capture`, `note_to_daily` and `append_to_log` tag the detour.** When a focus is open, an
inbox item's frontmatter gets `captured_during: <slug>`; a `## Notes` entry's log line and an
`append_to_log` line get an indented `during: [[slug]]` continuation. The tag must survive
triage: `discard_inbox_item` and the routing verbs carry `during:` on the log line they write,
so T8's "where captures came from" can see inbox captures after the item is gone. The
`quick-capture` skill uses `append_to_log` (and wrongly says the inbox is not exposed over MCP;
`capture` exists), so tagging `append_to_log` is what covers the path a person actually uses.

**Skills.** `daily-orientation` keeps its order — greeting with day and date, wins, due-soon —
and then offers a carried focus as the one recommended pick, framed as continuity, with
"pause it" as the alternative; a `next:` on the recommended project is quoted; a deep focus
on a light day suggests the pause. On "yes" it calls `resume_action`. Its step 9 line
("Started the day — focus [[slug]]: …") is replaced by a real `start_action` or
`resume_action` once the person says go, and its "no stored focus" notes go. `quick-capture`
gains the return cue. `weekly-review` is §8 T8.

**Wording.** The log says `paused`, `resumed`, `started`, `during`. Tool descriptions, server
instructions and skill text — everything an agent paraphrases to the person — use "move over",
"pick up", "capture"; never "drift", "distraction", "off-task", "leaked" or "enforce". The
reviews may editorialise; the record and the agent's mouth do not. T8 is bound now to a
wins-first framing ("focus time on This Week's Goal"), not "14 switches on Tuesday".

### 5.7 CLI surface

`cdno now`, the three states:

```
$ cdno now
On thesis — Draft methods section (deep), since 09:10 (2h 15m).

$ cdno now
On thesis — Draft methods section (deep), picked up 08:50 today (started Tuesday 14:05).

$ cdno now
Nothing started.
Last paused: thesis — Draft methods section (10:40), next: pick up at "Prior approaches"
```

`cdno now --json`, nulls-not-missing so a caller can test one field:

```json
{"project": "thesis", "action": "[[actions/draft-methods]] (deep)",
 "title": "Draft methods section", "note": "actions/draft-methods", "energy": "deep",
 "started": "08:50", "started_at": "2026-10-02T08:50", "date": "2026-10-02",
 "carried": false, "origin": {"started_at": "2026-09-30T14:05"},
 "elapsed_minutes": 135,
 "last_paused": null}
```

`action` stays the raw bullet text (what `complete_action` expects back); `title` strips the
link and suffix for display and `note` is the attached slug or null. `started` (HH:MM) stays
for compatibility. `origin` is null unless the focus was resumed.

`cdno now --line` emits one sanitised, length-capped line for prompt segments and the hook —
`Focus: thesis — Draft methods section (since 08:50)` or
`Focus: none (last paused: thesis — Draft methods section, next: …)` — through the same
`sanitise` the CLI already applies to bullet text, and exits 0 printing nothing when no vault is
found or `cdno` fails. `cdno now --replay` is gone with the cache.

`cdno action pause` and `switch` in a terminal: if `--next` is absent, **one prompt for the
hint, Enter skips**, and that prompt does not set `prompted` and triggers no confirm — a named
exception added to `docs/cli-ergonomics.md`'s "What is not part of the convention", with
`action pause` as the example: the written line is a single cheap log entry and a confirm on top
is the friction. `--reason` is never prompted. On `switch`, if other fields were prompted
(`--project`, `--query`), the normal confirm runs and its preview names both sides ("pause X,
start Y") and shows the typed `next:`; the picker never offers the bullet that is the focus.

---

## 6. Detailed design

### 6.1 `cdno-core`

- `VaultConfig`: `[focus]` with `carry_over_days: u32` (default 1) and
  `paused_lookback_days: u32` (default 14), `deny_unknown_fields` like `[tracking]`.
- Nothing else. No migration, no trait change, no transaction op.

### 6.2 `cdno-domain`

- `projects/actions.rs`: `LOG_ACTION_PAUSED_PREFIX`, `LOG_RESUMED_PREFIX`, `LOG_NEXT_KEY`;
  `format_action_paused_log_entry` (optional `next:` then `reason:` continuations, two-space
  indent, whitespace flattened like `reason:`); `pause_action`, `switch_action`,
  `switch_unplanned_action`, `resume_action`, each composed from `stage_*` helpers in one
  transaction; the `FocusOpen` check in `start_action` / `start_unplanned_action`, placed after
  resolution and before any add. `promote_action_with_vars` is unchanged.
- `context.rs`: the walk (§5.2) with the stated stopping rule; the fold gains three arms —
  `paused` as a close, `resumed` as close-plus-reopen, `promoted` as a rename;
  `CurrentFocus { date, origin }`; `last_paused(window)` as one pass; `parse_focus_marker`
  unchanged; a `parse_promotion_marker` beside it.
- `lint.rs`: `paused`, `resumed` and `promoted` join `FOCUS_MARKER_PREFIXES`; `lint_all_notes`
  does not need `today` after all (no cache to compare), so its signature is unchanged.
- `orient.rs`: `orientation_context` carries `focus` and per-project `last_paused`;
  `get_project_full` carries `last_paused`.
- `DomainError`: `NoFocus`, `FocusOpen { project, action, same_action, carried }`.
- Tests (`tests/unit/context_tests.rs`, `actions_tests.rs`, `lint_tests.rs`,
  `orient_tests.rs`): the walk across a window boundary and the D-2/D-1/D0 case with window 2;
  `carry_over_days = 0` reproduces today's behaviour exactly; a paused start is not a focus;
  resume re-stamps and clears the older marker; switch is atomic; promotion follows the note at
  the original time and a hand-written promotion line pairs; `last_paused` returns the
  continuations and skips a resumed pause; the drop cascade clears a focus; a focus on a parked
  project can be paused.

### 6.3 `cdno-cli`

- `cdno action pause`, `switch`, `resume` (flags-and-prompts; the skippable `next:` prompt per
  §5.7; `--reason` silent, as `drop --reason` is).
- `cdno now`: the §5.7 shapes, `--line`, date-aware `elapsed_since`.
- `start` refusal: message, `--json` object, interactive offer to switch.
- `docs/cli-ergonomics.md`: the named skippable-prompt exception.
- Tests in `tests/action.rs`, `tests/now.rs`: wiring only.

### 6.4 `cdno-mcp`

- `pause_action`, `switch_action`, `switch_unplanned_action`, `resume_action` (write).
  Catalogue 60 → 64; the pins in `tests/server.rs` and `e2e_stdio.rs` move.
- `CurrentFocusDto` gains `date`, `carried`, `origin`; `focus` on the write payloads per §5.5;
  `resumed_from` on `resume_action`'s payload; `focus` and `last_paused` on `get_orientation`,
  `last_paused` on `get_project_context`.
- `RejectionCode::{FocusOpen, NoFocus}` with the §5.1 details.
- Server instructions carry §5.6; `current_focus`, `start_action`, `switch_action` point at it;
  `lint`'s description lists the three new prefixes; every stranding sentence removed.
- `tests/handlers_operations.rs`, `handlers_context.rs`, `e2e_stdio.rs`: the rejection shapes,
  the `focus` field after a write and after a failed focus read, pause / switch / resume round
  trips.

### 6.5 Documentation

- `docs-site/src/reference/cli/action.md`, `now.md`, `configuration.md` (`[focus]`),
  `troubleshooting.md` ("names the wrong action after a promotion" is deleted; new entries:
  "says I'm still on yesterday's thing" → the window; `resume` or `pause` it, or
  `carry_over_days = 0`; and "`action start` refuses with `focus_open`" → `switch`, or `pause`
  first), `reference/mcp/reads.md`, `writes.md`, `concepts/contexts-and-energy.md` (a short
  "focus" section), `tutorials/daily-loop.md`.
- `docs/design.md`: the marker family gains `paused` and `resumed`; `CLAUDE.md`'s
  history-preservation paragraph lists them and names `promoted` as read back.
- `CHANGELOG.md` under `[Unreleased]` **in every stage** (a new refusal and new verbs are
  behavioural); `STATUS.md` at T7.

### 6.6 `examples/`

- `examples/hooks/claude-code/README.md` + `focus.sh` + a `settings.json` snippet to merge: a
  `UserPromptSubmit` hook that runs `cdno now --line --vault "$CUADERNO_VAULT_PATH"` (CLI
  discovery is upward from the session's cwd, so the vault must be named), exits 0 and prints
  nothing on any failure, and injects the one sanitised line. The README states that this is
  what turns §5.6 from advice into behaviour, and that it costs one warm `cdno` run per turn
  (§3.3's measurement).
- `examples/skills/daily-orientation/SKILL.md` and `quick-capture/SKILL.md` per §5.6.
- `examples/skills/references/ADHD-PRINCIPLES.md`: a "Focus and moving over" section with the
  protocol and the wording rule.

---

## 7. Compatibility

- Existing logs parse unchanged; `paused` and `resumed` are additive. **`promoted` lines in old
  logs now pair retroactively** (§5.4): a day with an un-closed start whose bullet was promoted
  and then completed reads as closed where it used to read as open. That is a correction.
- **The upgrade morning.** With the one-day window and no historical `pause`, a vault whose
  last start yesterday was never closed carries it today, and the first `start` after upgrade
  is refused with `focus_open` where it used to stack silently. The refusal names `switch`,
  `resume` or `pause`, and the troubleshooting entry covers it. Set `carry_over_days = 0` to
  keep the old behaviour.
- `start_action` with a focus open was a silent second start; it is now a refusal. Any script
  or skill that relied on stacking must call `switch_action`. No shipped skill does.
- `cdno now --json` adds fields and removes none; `current_focus`'s null contract is unchanged.
- The desktop app is not affected; it is retired.

---

## 8. Implementation plan — staged

Each stage is one PR, green on its own, each adding to `CHANGELOG.md`.

| | Stage | Depends on |
|---|---|---|
| T0 | `paused` marker: constants, formatter, reader close-arm, lint prefix, `pause_action` domain verb (resolves nothing against the map) + tests | — |
| T1 | Promotion read as a rename (§5.4): `parse_promotion_marker`, fold arm, lint prefix, test inverted, stranding paragraphs deleted everywhere | — |
| T2 | `switch_action` (+ unplanned), `FocusOpen` refusal on `start` with precedence, `NoFocus` on pause; composed from `stage_*` helpers | T0 |
| T3 | Window: `[focus] carry_over_days`, the walk with the stated stopping rule, `CurrentFocus.date`, date-aware `elapsed_since` | T0 |
| T4 | `resumed` marker, `resume_action`, `CurrentFocus.origin`; `paused_lookback_days`, `last_paused` one-pass | T2, T3 |
| T5 | CLI (`action pause/switch/resume`, `now` shapes, `--line`, skippable `next:` prompt + cli-ergonomics exception) and MCP (four tools, rejection codes, `focus` on write payloads, `get_orientation` / `get_project_context` fields, `resumed_from`) | T4 |
| T6 | Server instructions and tool descriptions (§5.6), the hook example, `captured_during` / `during:` on `capture`, `note_to_daily`, `append_to_log` and the triage verbs, the two skills and ADHD-PRINCIPLES | T5 |
| T7 | Docs-site, `design.md`, `CLAUDE.md`, `STATUS.md` | T5 |
| T8 | Weekly review reads the log: focus time per project against `This Week's Goal`, pauses and resumes, top `during:` sources, a `resumed` counted as a resume and a promotion as neither — **separate RFC**, wins-first by §5.6, once a few weeks of log exist | T6 |

T0, T1 and T3 can land in parallel.

---

## 9. Decisions and open questions

- **D1 — The focus survives midnight; default window one day.** Maintainer's decision
  (2026-10-02): "carry overnight and expire after that", configurable. Calendar days, not
  rolling hours: the journal's unit, and `resume` makes the night-owl case a one-word fix.
- **D2 — No cache.** The first draft's cached row was removed by the review on measurement and
  on two coherence holes (§3.3). The log is the only store. A `Vault::open_unreconciled` seam is
  the follow-up if a hook ever measures lag; a cache is not.
- **D3 — `pause`, `switch`, `resume` live under `cdno action`.** They share the marker family
  and the bullet text with `start`, `complete` and `drop`; `cdno focus` plus `cdno now` plus
  `cdno action start` would split one concept three ways. The asymmetry that `pause` and
  `resume` take no `--project/--query` while every sibling does is real: the help text and the
  MCP descriptions open with "acts on the CURRENT focus; takes no project or query", and
  `cdno now` hints the verbs. `ActionStatus::Blocked` on attached notes is a status, not a
  pause; `pause` writes a log line only.
- **D4 — A switch is a pause followed by a start; no `switched` marker.** Promotion no longer
  writes that pair, so the T8 count stays honest.
- **D5 — A second `start` is refused, not stacked, and never auto-switches.** This is the
  "deliberate" requirement made concrete, and it is only a speed bump by design: `complete` then
  `start` gets through, and `switch_action` is callable on the person's word. D7 and D5 are
  consistent because starting is a focus verb.
- **D6 — Project-level focus (a start with no bullet) is out of scope.** `start_unplanned`
  makes a bullet cheaply, and a focus without a bullet has no close verb.
- **D7 — The server never refuses an out-of-focus write.** §5.6. Not up for a flag.
- **D8 — `reason` is never prompted; `next` is one skippable prompt.** A prompt for a reason
  at the moment of switching is the friction that makes people not record the switch. The
  re-entry hint is the value of `pause` and is cheapest at the moment of stopping, so it is
  asked once, Enter skips, nothing is confirmed, and `docs/cli-ergonomics.md` names the shape.
- **D9 — `resume` is its own verb and marker, not `start` on the focused bullet.** The one
  deliberate verb must not write a different marker depending on the date, and `resume` on a
  pause is what brings the `next:` hint back. Cost accepted: a fifth marker in the family, a
  fold arm, a lint prefix, a verb and a tool.
- **D10 — `detour_budget` (suggest a pause after N captures during one focus) is deferred** to
  T8's RFC, with the metrics that would justify a number.
- **Open: the hook's placement.** `examples/hooks/` is new. **The maintainer confirms**, or
  prefers it under `examples/skills/` beside the skills it serves.
- **Open: `paused_lookback_days` default.** 14 covers a weekend and most holidays; 30 covers
  a conference month at the cost of a few more reads on the orientation path only. **The
  maintainer confirms** 14.

---

## 10. Verification

- `cargo test -p cdno-domain --test unit -- unit::context_tests`, `unit::actions_tests`,
  `unit::orient_tests` cover §6.2; `-- unit::lint_tests` the three prefixes.
- `cargo test -p cdno-cli --test action` and `--test now` cover the wiring, including
  `carry_over_days = 0` in a temp vault's `config.toml`, the `--line` sanitisation (a bullet
  with control characters and an over-long `next:`), and the skippable prompt under
  `--no-interactive` and a pipe.
- `cargo test -p cdno-mcp --test handlers_operations`, `--test handlers_context` and
  `--test e2e_stdio` cover the rejections, the `focus` field, the four tools and the catalogue
  pin.
- Manual, in the repo's own dev vault: start, promote, complete; start, write yesterday's note by
  hand with an open start, `cdno now` says carried; `resume`, `cdno now` says picked up today;
  edit today's log by hand, `cdno now` sees it; `cdno lint` clean throughout, and dirty after
  writing a `paused` or `resumed` line with an ASCII hyphen.
