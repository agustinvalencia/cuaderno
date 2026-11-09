# Write tools

Tools that mutate the vault. Each returns a result describing what was written. The same business
rules as the CLI apply (append-only notes, auto-logged project state, the project cap).

## Every write is verified

A tool result that only said "success" could not be told apart from a write that silently never
landed — which over a remote connection is exactly how a lost note goes unnoticed. So every write
tool re-reads its target before answering, and the result carries the evidence:

```json
{
  "path": "journal/2026/daily/2026-08-26.md",
  "message": "Logged to journal/2026/daily/2026-08-26.md",
  "verification": {
    "verified": "content",
    "bytes_written": 412,
    "content_hash": "84bc0919e867576f",
    "appended_tail": "- **09:14**: baseline sweep finished\n"
  }
}
```

| Field | Meaning |
|-------|---------|
| `verified` | `content` — the file was re-read; or `removed`, for `discard_inbox_item`, where the check is that the file is gone |
| `bytes_written` | Size of the file on disk after the write. `0` for a removal |
| `content_hash` | The note's content hash (below) — `null` for a removal |
| `appended_tail` | For append-shaped writes (`append_to_log`, `start_action`, `note_to_daily`), the tail of the *section* the text went into — see [below](#the-appended-tail). `null` elsewhere |

**If the target cannot be read back, the tool returns an error rather than a success.** The wording
says the write is *unverified*, not failed: it may still have landed, so the right response is to
re-read the note, not to blindly repeat the write.

### The appended tail

`appended_tail` is scoped to the section the write targeted, not to the last bytes of the file.
`append_to_log` and `start_action` both write into `## Logs`, so that is the section you get back.
`note_to_daily` writes into both `## Notes` and `## Logs`; its tail is the last 512 bytes of
`## Notes`, which end with the entry just written (the pointer line is already in the result's
`log_line`).
(`start_unplanned_action` also logs, but it rewrites the project map in the same commit and so
verifies as a whole-file rewrite — its `appended_tail` is `null`.)

The distinction matters because `## Logs` is not always last. Cuaderno pins the *effective daily
template's* last `##` section to the bottom of the note — for the built-in template that is `## Logs`,
but a custom `.cuaderno/templates/daily.md` ending in, say, `## Reflection` keeps Reflection last and
leaves the log line in the middle of the file. Reading the end of the file would then show you
Reflection's text while claiming to be the line that landed.

If the section cannot be located — an unparseable note, a `## Logs` heading that is missing or
duplicated — the field is `null`. The write is still verified by `bytes_written` and `content_hash`;
only the extra evidence is withheld, because a window over the wrong bytes is worse than no window.

### The content hash

`content_hash` is the same non-cryptographic xxh3-64 fingerprint (16 lowercase hex characters) the
index uses for change detection, so a client can recompute it over a note it has read and compare.
Two uses in practice:

- confirm a note is byte-identical to the one the server saw;
- notice a **no-op**. `update_project_state` with a state that already matches deliberately writes
  nothing and still reports success — an unchanged `content_hash` across two calls is how you tell.

It is a change detector, not tamper evidence: it does not defend against someone who also chooses
the content.

## Logging, capture, triage

| Tool | Inputs | Effect |
|------|--------|--------|
| `append_to_log` | `text` | Append a line to today's daily note. ([`cdno log`](../cli/log.md)) |
| `capture` | `text` | Drop a raw note into `inbox/`. ([`cdno capture`](../cli/capture.md)) |
| `discard_inbox_item` | `slug` | Clear a triaged capture (slug from `triage_inbox`). |

When a focus is open, `append_to_log`, `capture`, `note_to_daily` and `discard_inbox_item` tag what
they write with it; see [The `during:` tag](#the-during-tag).

## Evidence

| Tool | Inputs | Effect |
|------|--------|--------|
| `file_to_portfolio` | `portfolio`, `source`, `origin`, `content?`, `attach?`, `vars?`, `format?` | File evidence into a portfolio; `attach` is a server-side path to a non-Markdown artefact (`vars` is ignored on the attach path). `format` is `markdown` (the default) or `typst`. `format: typst` files `content` as a Typst note: the complete Typst body, preamble included, with no `<cdno>` block (the engine writes it). Use it only when the person asks for Typst or the material is already Typst; a Typst sibling in the portfolio is where to copy the preamble from (`read_note`). The engine checks syntax only. A body it cannot take (a `<cdno>` label, a parse error, a `[[…]]` link with an unescaped character) is refused with a `remedy`; a missing space before `[[` after a `#…` expression or an `@ref` is not refused and silently loses the link. `content` is required with it unless `attach` is given (a blank one is refused, unlike a Markdown note, where it may be empty), and a Typst evidence body is frozen as written, so it cannot be fleshed out later. `vars` is refused. With `attach` set to a server-side `.typ` path, the file is **adopted** as the note: leave `content` empty, the engine writes the metadata block, every file under the directory the `.typ` sits in is copied beside the note (names starting with `.` are skipped; more than 200 files or 200 MiB, a symbolic link, or a directory overlapping the vault is refused as `adoption_refused`), each literal relative path in the file is rewritten to the copy, and the result carries `rewrites` (`was`/`now`). A path that leaves the folder, a `/`-rooted path or a non-literal path is refused (`asset_path_outside_folder`, `asset_path_not_literal`). See [Typst notes](../typst-notes.md). ([`cdno file`](../cli/file.md)) |

## Projects, actions, milestones, waiting-on

Every tool in this table takes an **open** project, active or parked: a parked map is a complete map,
so `add_action`, `update_project_state`, the milestone and waiting-on tools, `set_core_question`,
`promote_action`, `complete_action`, `drop_action` and `hand_off_project` write to it as it is.
Never activate a parked project just to write on it; the cap is checked only when a project begins
to count (`activate_project`, or a take-back of an active project). Only starting or picking up
work (`start_action`, `start_unplanned_action`, `switch_action`, `switch_unplanned_action`,
`resume_action`) needs an active map, as does [`park_project`](creation-and-lifecycle.md); on a
parked one they return `project_not_active` with `status: "parked"` (see the
[rejection codes](overview.md#when-a-call-is-rejected)). A completed or dropped project takes none of these writes.

| Tool | Inputs | Effect |
|------|--------|--------|
| `update_project_state` | `project`, `new_state` | Rewrite the Current State (auto-logs the previous). |
| `set_core_question` | `project`, `core_question?`, `clear?` | Set the project's core question (bare `questions/<domain>/<slug>` target, not `[[…]]`); `clear: true` detaches. Auto-logs the previous value. |
| `hand_off_project` | `project`, `holder?`, `take_back?` | Record whose move an open (active or parked) project is: `holder` writes `held_by: "<name>"` and the project stops counting against the cap; `take_back: true` writes `held_by: null`. Exactly one of the two. Logs `holder on [[slug]]` with `was:`/`now:` lines. Refused with `empty_field` for a name on the vault's `[projects] holder_sentinels`, with `project_in_focus` while an action on the project is in focus, and, for a take-back that would make an active project count at the cap, with `project_cap_reached`. Taking back a parked project only clears the name. |
| `add_action` | `project`, `title`, `energy`, `with_note?`, `slug?`, `vars?` | Append a next action; `with_note` also scaffolds a manifest note (`slug` and `vars` apply only then; without `with_note`, `slug` is ignored). |
| `promote_action` | `project`, `query`, `slug?`, `vars?` | Promote a bullet to a manifest note (substring match); `slug` names the new note's file ([the slug rule](../cli/overview.md#naming-a-note---slug)). |
| `start_action` | `project`, `query` | Log that work on an existing bullet is starting. Logs the **resolved** bullet text, so the later close pairs with it. Errors when `query` matches nothing — it will not create the action. Refused with `focus_open` while another action is in focus ([below](#rejections-focus_open-and-no_focus)). |
| `start_unplanned_action` | `project`, `title`, `energy` | Add the bullet **and** start it, in one commit, for work that was on no map. Separate from `start_action` on purpose: a fallback would turn a typo into a new action. |
| `pause_action`, `switch_action`, `switch_unplanned_action`, `resume_action` | see [Focus tools](#focus-tools) | Move the one focus slot: set it aside, move to another action, or pick work up again. |
| `complete_action` | `project`, `query` | Complete an action; archives its note if any. |
| `drop_action` | `project`, `query`, `reason?` | Close an action **without** recording it as done (superseded, abandoned, reprioritised); archives its note as `status: dropped`. |
| `add_milestone` | `project`, `title`, `target_date?`, `hard?` | Add a milestone; `hard` counts it in commitments and requires `target_date`. Omit `target_date` for a condition-gated milestone (`target: TBD`), which stays out of commitments. |
| `complete_milestone` | `project`, `query` | Complete a milestone (substring match). |
| `drop_milestone` | `project`, `query`, `reason?` | Remove a milestone, with the lines indented beneath it, **without** recording it as met (superseded, mis-typed, not happening); logs `milestone dropped on`. Completed bullets are never matched. |
| `add_waiting_on` | `project`, `description` | Add a waiting-on blocker. |
| `resolve_waiting_on` | `project`, `query` | Resolve a waiting-on item (substring match). |

## Focus tools

Four tools move the focus, the one "what I am on" slot that [`current_focus`](reads.md#focus-fields)
reads back. They are the MCP forms of [`cdno action pause`, `switch` and `resume`](../cli/action.md).
Each writes one entry in today's `## Logs` and, like every write, is [verified](#every-write-is-verified).

| Tool | Inputs | Effect |
|------|--------|--------|
| `pause_action` | `next?`, `reason?` | Sets the **current focus** aside without finishing it. Takes no project or query: it pauses whatever `current_focus` shows, with nothing matched against the map. Logs `action paused on [[slug]] — <text>` with optional indented `next:` and `reason:` lines. Result adds `paused` (the focus that was paused). |
| `switch_action` | `project`, `query`, `next?`, `reason?` | Pauses the current focus and starts an action already on the map, in one commit. `project` and `query` name the action to switch **to**; `next` and `reason` describe the one set aside. With nothing in focus it is a plain start and `paused` is `null`. |
| `switch_unplanned_action` | `project`, `title`, `energy`, `next?`, `reason?` | The same, for work on no map yet: adds the bullet and starts it. Separate from `switch_action` for the reason `start_unplanned_action` is separate from `start_action`: a query that matches nothing must never become a new action. |
| `resume_action` | `project?` | Re-anchors a focus carried over from an earlier day, or reopens the latest pause within `paused_lookback_days`; with `project`, that project's carried focus, else its latest pause. Logs `resumed [[slug]] — <text>`. |

`switch_action` and `switch_unplanned_action` return `paused` (the focus that was set aside, or
`null`) and `started` (the new focus). `resume_action` returns `resumed` (the focus now open) and
`resumed_from`:

```json
{"resumed": {"project": "surrogate-model", "action": "Draft the methods section (deep)",
             "started": "11:12", "date": "2026-10-04", "carried": false, "origin": null},
 "resumed_from": {"kind": "paused", "date": "2026-10-04",
                  "next": "pick up at Prior approaches", "reason": "call"}}
```

`resumed_from.kind` is `carried` or `paused`, `next` and `reason` are the pause's (always `null` for a
carried focus). With `project`, a carried focus wins over that project's pause, so read `kind` and
`resumed.action` back to the person rather than assuming the pause they were shown, and read
`next` out as the re-entry cue. `pause_action` and `resume_action` are for the person's word:
the tool descriptions tell an agent never to call them unasked.

### Rejections: `focus_open` and `no_focus`

Two refusals are returned as coded rejections, so an agent can act on them without parsing prose.
Nothing is written.

`focus_open` comes from `start_action`, `start_unplanned_action`, `switch_action`,
`switch_unplanned_action` and `resume_action`:

```json
{"code": "focus_open",
 "message": "An action is already in focus. Ask the person before switching; do not retry.",
 "details": {"focus": {"project": "surrogate-model", "action": "Draft the methods section (deep)",
                       "started": "09:10", "date": "2026-10-04", "carried": false},
             "attempted": {"project": "other-project", "query": "CI"},
             "same_action": false,
             "remedy": "switch_action"}}
```

| `details` key | Meaning |
|---------------|---------|
| `focus` | What is open: `project`, `action`, `started`, `date`, `carried`. |
| `attempted` | What the call asked for: `{project, query}`, or `{project, title}` for an unplanned form. Absent from `resume_action`'s rejection, which attempts no target. |
| `same_action` | `true` when the call named the action that is already in focus. |
| `remedy` | The tool to call, on the person's word: `switch_action` (something else is in focus), `resume_action` (the same action, carried over from an earlier day), or `already_focused` (the same action, started today: nothing to do). |

The message tells the agent to ask the person before switching and not to retry. The server never
switches on its own.

`no_focus` comes from `pause_action` (`Nothing is in focus to pause.`) and `resume_action` (`Nothing
to resume: no focus carried over and no pause.`), with empty `details`. Tell the person there is
nothing to act on rather than retrying.

### `focus` on write results

The success payload of these eighteen tools carries a `focus` field: `start_action`,
`start_unplanned_action`, `switch_action`, `switch_unplanned_action`, `pause_action`,
`resume_action`, `complete_action`, `drop_action`, `promote_action`, `add_action`, `append_to_log`,
`capture`, `note_to_daily`, `park_project`, `activate_project`, `complete_project`,
`drop_project` and `hand_off_project`. It is the focus as [`current_focus`](reads.md#focus-fields) reads it right after the
write, so an agent sees what is open without a second call: the same shape, and `null` means
nothing is open. It is read in the same call that verifies the write, and a focus that cannot be
read after a write has landed comes back as `null` rather than as an error, because an error would
tell the agent that a successful write failed. Every other write tool's result is unchanged.

The server never refuses a write for being outside the focus. How an agent should behave when a
request is, is stated once in the server's instructions rather than in each tool description: compare
at project level, say one sentence only on a mismatch, offer to capture it, and treat the person's
explicit "let's switch to Y" as consent. See [Focus](../../concepts/contexts-and-energy.md#focus).

### The `during:` tag

When a focus is open at write time, a carried one included, the writes that park a thought say which
project it was written during:

- `append_to_log` and `note_to_daily` append an indented continuation to the log line:

  ```text
  - **11:12**: checked the mesh
    during: [[surrogate-model]]
  ```

  (`note_to_daily` also echoes the tag on a second line of the `log_line` it returns.)
- `capture` adds `captured_during: surrogate-model` to the new inbox item's frontmatter.
- `discard_inbox_item` copies that tag from the item onto the log line it writes, so it outlives the
  file.

With no focus open nothing is written differently, and a focus that cannot be read writes untagged
rather than failing. The tag is written by the server; an agent does not add it to its own text.

## Commitments and tracking

| Tool | Inputs | Effect |
|------|--------|--------|
| `create_commitment` | `title`, `due`, `context`, `project?`, `stewardship?`, `slug?`, `vars?` | Create a standalone commitment note; `slug` names its file ([the slug rule](../cli/overview.md#naming-a-note---slug)). |
| `complete_commitment` | `commitment` (slug) | Mark a commitment done and archive it. |
| `drop_commitment` | `commitment`, `reason?` | End a commitment that was **not** kept; stamps `status: dropped`, clears `completed`, archives it. Never appears as completed work. |
| `reschedule_commitment` | `commitment`, `due` | Move an active commitment's due date, logging both the old and new dates. Refuses an unchanged date. |
| `complete_periodic` | `stewardship`, `title`, `at?` | Complete one occurrence of a stewardship's periodic commitment, rolling `next:` forward by that line's recurrence. Anchored to the due date, so completing early never drags the schedule earlier. |
| `create_tracking_entry` | `stewardship`, `activity`, `routine?`, `content?`, `vars?`, `metrics?`, `date?` | File a tracking note under an expanded stewardship. `metrics` is a JSON object merged into the entry's frontmatter — a scalar per reading (`{"balance": 1240.5}`), or an array of flat records when one entry holds several comparable items (`{"detail": [{"subject": "harmony", "minutes": 25}]}`); a scalar whose key is declared under `[schemas.tracking.fields]` is type-checked, and a key naming the note's identity (`type`, `stewardship`, `activity`, `date`) is refused. `date` files the entry for a past day (bounded to 50 years back, 1 year ahead). A second call for the same `(activity, date)` **merges** into the first: content appended, metrics folded in. Records carrying a stable `id` replace the record with that `id`; records without one append, so re-sending a payload without ids double-counts summed metrics. Either way the write is journalled to today's daily log. |

## Revising a note

| Tool | Inputs | Effect |
|------|--------|--------|
| `revise_note` | `note`, `reason`, and either `body` + `expected_hash` or `section` + `content` | Refine a mutable custom note (a concept, say) in place, logging the revision to today's daily note in the same write. ([`cdno note revise`](../cli/note.md#cdno-note-revise-note)) |

`revise_note` takes exactly one of two forms:

| Input | Meaning |
|-------|---------|
| `note` | Which note: any reference [`read_note`](reads.md) takes — a vault path with or without `.md`, a bare slug, or `type:slug`. |
| `body` | **Whole-body form.** The note's new body, everything after the frontmatter (which is kept as it is), written verbatim. A Typst note keeps the blank lines that follow its metadata block, so the new body's own leading blank lines give way to them, and a body identical to the current one is a no-op even if it would not parse. |
| `expected_hash` | Required with `body`: the `content_hash` `read_note` returned. A blank value counts as missing. Ignored with `section`. |
| `section` | **Section form.** Heading text of the section to upsert, as `read_note`'s `headings` lists it, without the `#` markers. An existing section is replaced together with its sub-sections; a missing one is appended as `## <section>`. No hash is needed. |
| `content` | The section's new text, without its heading. Only with `section`. |
| `reason` | Required. Why the note was revised, in a short clause; the assistant drafts it from what it changed. It becomes the daily-log line `revised [[<path>#<section>]] — <reason>`, or `revised [[<path>]] — <reason>` for a whole body. |

Text identical to the note's current text writes and logs nothing and returns `changed: false`.
The result extends the usual write result with `changed`, `new_hash` (the note's hash after the
call: pass it as `expected_hash` on a follow-up revision rather than reading again), `log_line`
(the line written, without its timestamp, or `null`) and `section_target` (the anchored link the
log line points at, for a section revision); `verification` is `null` when nothing changed.

Refusals (a [rejection](overview.md) with these codes; nothing is written):

| Code | When |
|------|------|
| `note_not_revisable` | The note is of a built-in type (its sections belong to its own tools) or of a custom type declared `append_only = true`. |
| `stale_revision` | A whole-body revision whose `expected_hash` no longer matches the bytes on disk: the note changed since it was read. `details.actual` is the current hash; read the note again and redo the edit on the fresh text. |
| `revision_invalid` | The `section` heading is empty, spans lines, contains `[`, `]`, `\|` or `#`, or starts with `^`; or `content` holds a heading at the section's own level or above, which would restructure the note. |
| `ambiguous_section` | The `section` heading matches more than one heading in the note. |
| `section_revision_unavailable_for_typst` | A `section` aimed at a Typst note. `details.remedy` names the whole-body form: read the note, edit the body, and pass all of it as `body` with `expected_hash`. |
| `body_carries_metadata_block` / `body_does_not_parse` / `link_needs_escape` | A Typst note's new `body` holds a `<cdno>` label, does not parse, or has a `[[…]]` that is not one inline run. The metadata block stays the engine's: `body` is everything after it. |
| `format_unsupported_for_type` | `format: typst` was given for a note type that does not admit Typst. `list_note_types` reports `admits_typst` for each type; `[typst] note_types` sets the list. |
| `stem_taken_by_other_format` | A `.typ` and an `.md` cannot share a stem in one folder. `details` names the note holding it. |
| `vars_unsupported_for_typst` | `vars` was given with `format: typst`. A Typst note has no template, so the whole body goes in the content itself. |
| `not_found` / `ambiguous_slug` | The `note` reference matches no note, or several (with their paths in `details.candidates`), as for `read_note`. |

A blank `reason`, `body` together with `section`, `section` without `content`, `content` without
`section`, or `body` without `expected_hash` is refused as an invalid argument (`-32602`) naming
the field.

## Renaming a note

| Tool | Inputs | Effect |
|------|--------|--------|
| `rename_note` | `note`, `to` or `relink: true`, `reason?`, `dry_run?`, `expected_plan?`, `skip_sync_check?` | Rename a note's slug and follow every reference to it in one transaction, logging a `renamed` line to today's daily note; or, with `relink`, keep the slug and rewrite the links still under a former name. ([`cdno note rename`](../cli/note.md#cdno-note-rename-note)) |

The preview is the contract. The sequence an agent follows:

1. **Preview.** Call with `dry_run: true`. The result is the plan and nothing is written (`written:
   false`); a refusal here is exactly the refusal a write would get.
2. **Show.** Put the plan in front of the person: what moves, how many references in how many notes,
   the `log_line`, any `warnings`. The person decides, not the agent.
3. **Write.** Call again with the same `note`, `to` and `reason` (the reason is part of the plan) and
   `expected_plan` set to the preview's `plan_hash`. What is written is the plan that was shown;
   if the vault changed in between the call is refused with `stale_plan` (below) and writes nothing.

`note` takes the references [`read_note`](reads.md) takes, including a former name of the note
(`found_by_former_name` in the result says which); `reason` is a short clause for the log line. A
rename to the note's current slug is an empty plan: `written: false`, `log_line: null`.

**Relinking.** `relink: true` in place of `to` keeps the note's slug and rewrites every link that
reaches it only through a former name, links and reference fields alike (the links are the
stragglers [`lint`](reads.md) reports, which a sync merge can bring back), to the current slug. The
same preview-then-write sequence applies, with `relink: true` on both calls. Nothing moves and the
aliases stay listed: the result has `moves: []` and `new_slug` equal to `from`, and the `log_line`
is `relinked [[<slug>]] — <title>` with an indented `references: <n>` line. With no straggler the
plan is empty and nothing is logged. An archived action note can be relinked, though not renamed.
A blank `to` beside `relink: true` reads as omitted. `relink` with `to`, either with `reason`, or neither `to` nor
`relink`, is refused as an invalid argument naming the field
([`cdno note rename --relink`](../cli/note.md#relinking-the-stragglers---relink)).

The result is the plan, whether or not it was written: `dry_run`, `target`, `from` (the note's
current slug), `new_slug`, `found_by_former_name` (the former name `note` matched, or `null`),
`moves` (`{from, to}`), `edits` (a `path` with its `changes`, each of kind `links` with a `count`,
`field` with a `key`, `alias`, `asset_paths` with a `count` (a Typst note's own path literals into
its assets folder) or `log_line`), `references`, `notes`, `warnings` (each with a `kind`
and a `message`: `frozen_prefix_already_diverged`, `old_slug_already_an_alias`, or `sync_check`),
`plan_hash`, `log_line` (`null` when the plan is empty), then `written`, `index_stale` (`true`: the
files are written and the rename succeeded; the index catches up on the next open) and
`verification`. A write nudges the sync agent only when the HTTP server runs with `--sync-nudge` (off by default; the stdio server never does); a preview never does.

Before planning, the tool checks the vault against its upstream **as last fetched**: it never
fetches. A preview is refused exactly as a write would be.

Refusals (a [rejection](overview.md); nothing is written):

| Code | When |
|------|------|
| `stale_plan` | The vault changed since the preview (any log append does it, and so does a different `reason`). `details` carries `expected` and `actual`. Preview again and show the person the new plan; never retry the same `expected_plan`. A blank `expected_plan` is refused as an invalid argument. |
| `behind_upstream` | The vault is `details.behind` commits behind its upstream. Retry in a few seconds; if still behind, the sync is stalled: tell the person rather than set `skip_sync_check`. |
| `git_operation_in_progress` | A merge, rebase or similar was left paused. Never overridable. Both git codes carry the check's `warnings` in `details` when it had any. |
| `stem_shared` | Another note shares the slug, so links to it cannot be told apart. |
| `note_not_renamable` | The note is of a kind the rename does not take (`details.reason`: `archived`, `folder`, a type). A portfolio whose slug a question shares is renamed through the question. |
| `reference_drift` / `reference_unaddressable` | A referrer's frontmatter would change beyond the reference, or names the note in a form the rewrite cannot address; the person fixes the note by hand first. |
| `rename_incomplete` | A write failed and the rollback could not restore `details.paths`: the person checks them by hand. |
| `slug_taken` / `invalid_slug` | `to` is taken or malformed: refused, never suffixed. |
| `not_found` / `ambiguous_slug` | The `note` reference matches no note, or several, as for `read_note`; a question and its same-slug portfolio resolve to the question. |

## Frontmatter

| Tool | Inputs | Effect |
|------|--------|--------|
| `set_frontmatter` | `note`, `key`, `value` | Set a declared, `settable = true` typed frontmatter field through the index (no desync). `note` is `today`, a `YYYY-MM-DD` date, or a vault-relative path. Engine-owned keys (`type`, `status`, a period key, and a project's `closed`, owned by `complete_project` / `drop_project` / `activate_project`) are rejected; the value is type-checked; `log_on_change` fields stamp a daily-log line. In a Typst note the value is rewritten inside the `<cdno>` block, byte for byte, and a key that is not already in the block is an error. ([`cdno frontmatter set`](../cli/frontmatter.md)) |

## Daily, weekly, and monthly sections

| Tool | Inputs | Effect |
|------|--------|--------|
| `note_to_daily` | `heading`, `body`, `date?` | Write worked-out substance to a daily note as one `### <heading>` entry under `## Notes`, with its pointer line in `## Logs`, in one write. See [below](#note_to_daily). ([`cdno log note`](../cli/log.md#cdno-log-note)) |
| `upsert_daily_section` | `section` (`Standup`\|`Intention`\|`Agenda`\|`Meeting`\|`Notes`), `content?`, `date?`, `append?` | Write or append a daily-note section. `Notes` is append-only: it takes `append: true` only, and `append: false` is refused; for one entry with its `## Logs` pointer, use `note_to_daily`. |
| `upsert_weekly_section` | `section` (`Wins`\|`Challenges`\|`One Improvement`\|`This Week's Goal`), `content?`, `date?`, `append?` | Write or append a weekly-note section. |
| `upsert_monthly_section` | `section` (`Wins`\|`Themes`\|`Next Month's Focus`), `content?`, `date?`, `append?` | Write or append a monthly-note section. |

### `note_to_daily`

| Input | Meaning |
|-------|---------|
| `heading` | The entry's heading, written as `### <heading>` under `## Notes` and used verbatim as the pointer's anchor. |
| `body` | The entry's substance. Leading blank lines and trailing whitespace are dropped. Its wikilinks are copied onto the pointer line. |
| `date` | ISO `YYYY-MM-DD` of the daily note to write to, stamped at the current time. Omitted = today. |

The entry is appended under the day's `## Notes`, which is created immediately above `## Logs` if
the day has none (the daily note itself is created if absent). The pointer line
`noted [[journal/<year>/daily/<date>#<heading>]]`, followed by the body's wikilinks in parentheses
when it has any, is appended to `## Logs` in the same write, so the entry is not logged again with
`append_to_log`. End an entry worth reusing with the tag `#concept` on its last line, so a later
review can find it as a candidate for promotion to a concept note.

The result adds `target` — the entry's anchored link, `journal/<year>/daily/<date>#<heading>`, to
cite as `[[<target>]]` (for example from a concept's `origin`) — and `log_line`, the pointer line
without its timestamp.

Refused with code `history_entry_heading_invalid` (nothing is written) when the heading reuses a
daily section name (`Standup`, `Intention`, `Agenda`, `Meeting`, `Notes`, `Logs`), matches a heading
already in that day's note, contains `[`, `]`, `|`, `#` or inline markup (bold, italics, code),
or starts with `^`. Headings inside the body must be level 3 or deeper and are held to the same
uniqueness rule, so a pasted derivation with its own `## Proof` must be demoted first. A blank
`heading` or `body` is refused as an invalid argument (`-32602`).

## Notes

- `append?` defaults to replacing the section; set it `true` to append instead.
- Dates are `YYYY-MM-DD`; week-scoped tools accept any day in the target week, and month-scoped
  tools accept any day in the target month.
- `vars?` is an optional `name -> value` map supplying values for a custom template's
  [`[variables.prompt]`](../../tutorials/templates-and-frontmatter.md) placeholders — the MCP analogue
  of the CLI's repeatable `--var name=value`. Omitting a required prompted variable fails with an
  "unresolved prompts" error. See [Creation and lifecycle tools](creation-and-lifecycle.md) for the
  full list of templated tools that accept it.
- See also: [Creation and lifecycle tools](creation-and-lifecycle.md), [JSON output](../json-output.md).
