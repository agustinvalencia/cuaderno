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

## Evidence

| Tool | Inputs | Effect |
|------|--------|--------|
| `file_to_portfolio` | `portfolio`, `source`, `origin`, `content?`, `attach?`, `vars?` | File evidence into a portfolio; `attach` is a server-side path to a non-Markdown artefact (`vars` is ignored on the attach path). ([`cdno file`](../cli/file.md)) |

## Projects, actions, milestones, waiting-on

| Tool | Inputs | Effect |
|------|--------|--------|
| `update_project_state` | `project`, `new_state` | Rewrite the Current State (auto-logs the previous). |
| `set_core_question` | `project`, `core_question?`, `clear?` | Set the project's core question (bare `questions/<domain>/<slug>` target, not `[[…]]`); `clear: true` detaches. Auto-logs the previous value. |
| `add_action` | `project`, `title`, `energy`, `with_note?`, `vars?` | Append a next action; `with_note` also scaffolds a manifest note (`vars` applies only then). |
| `promote_action` | `project`, `query`, `vars?` | Promote a bullet to a manifest note (substring match). |
| `start_action` | `project`, `query` | Log that work on an existing bullet is starting. Logs the **resolved** bullet text, so the later close pairs with it. Errors when `query` matches nothing — it will not create the action. |
| `start_unplanned_action` | `project`, `title`, `energy` | Add the bullet **and** start it, in one commit, for work that was on no map. Separate from `start_action` on purpose: a fallback would turn a typo into a new action. |
| `complete_action` | `project`, `query` | Complete an action; archives its note if any. |
| `drop_action` | `project`, `query`, `reason?` | Close an action **without** recording it as done (superseded, abandoned, reprioritised); archives its note as `status: dropped`. |
| `add_milestone` | `project`, `title`, `target_date?`, `hard?` | Add a milestone; `hard` counts it in commitments and requires `target_date`. Omit `target_date` for a condition-gated milestone (`target: TBD`), which stays out of commitments. |
| `complete_milestone` | `project`, `query` | Complete a milestone (substring match). |
| `drop_milestone` | `project`, `query`, `reason?` | Remove a milestone, with the lines indented beneath it, **without** recording it as met (superseded, mis-typed, not happening); logs `milestone dropped on`. Completed bullets are never matched. |
| `add_waiting_on` | `project`, `description` | Add a waiting-on blocker. |
| `resolve_waiting_on` | `project`, `query` | Resolve a waiting-on item (substring match). |

## Commitments and tracking

| Tool | Inputs | Effect |
|------|--------|--------|
| `create_commitment` | `title`, `due`, `context`, `project?`, `stewardship?`, `vars?` | Create a standalone commitment note. |
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
| `body` | **Whole-body form.** The note's new body, everything after the frontmatter (which is kept as it is), written verbatim. |
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
| `not_found` / `ambiguous_slug` | The `note` reference matches no note, or several (with their paths in `details.candidates`), as for `read_note`. |

A blank `reason`, `body` together with `section`, `section` without `content`, `content` without
`section`, or `body` without `expected_hash` is refused as an invalid argument (`-32602`) naming
the field.

## Frontmatter

| Tool | Inputs | Effect |
|------|--------|--------|
| `set_frontmatter` | `note`, `key`, `value` | Set a declared, `settable = true` typed frontmatter field through the index (no desync). `note` is `today`, a `YYYY-MM-DD` date, or a vault-relative path. Engine-owned keys (`type`, `status`, a period key, and a project's `closed`, owned by `complete_project` / `drop_project` / `activate_project`) are rejected; the value is type-checked; `log_on_change` fields stamp a daily-log line. ([`cdno frontmatter set`](../cli/frontmatter.md)) |

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
