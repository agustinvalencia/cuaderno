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
| `appended_tail` | For append-shaped writes (`append_to_log`), the tail of the *section* the text went into — see [below](#the-appended-tail). `null` elsewhere |

**If the target cannot be read back, the tool returns an error rather than a success.** The wording
says the write is *unverified*, not failed: it may still have landed, so the right response is to
re-read the note, not to blindly repeat the write.

### The appended tail

`appended_tail` is scoped to the section the write targeted, not to the last bytes of the file.
`append_to_log` writes into `## Logs`, so that is the section you get back.

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
| `complete_action` | `project`, `query` | Complete an action; archives its note if any. |
| `add_milestone` | `project`, `title`, `target_date?`, `hard?` | Add a milestone; `hard` counts it in commitments and requires `target_date`. Omit `target_date` for a condition-gated milestone (`target: TBD`), which stays out of commitments. |
| `complete_milestone` | `project`, `query` | Complete a milestone (substring match). |
| `add_waiting_on` | `project`, `description` | Add a waiting-on blocker. |
| `resolve_waiting_on` | `project`, `query` | Resolve a waiting-on item (substring match). |

## Commitments and tracking

| Tool | Inputs | Effect |
|------|--------|--------|
| `create_commitment` | `title`, `due`, `context`, `project?`, `stewardship?`, `vars?` | Create a standalone commitment note. |
| `complete_commitment` | `commitment` (slug) | Mark a commitment done and archive it. |
| `create_tracking_entry` | `stewardship`, `activity`, `routine?`, `content?`, `vars?`, `metrics?`, `date?` | File a tracking note under an expanded stewardship. `metrics` is a JSON object merged into the entry's frontmatter — a scalar per reading (`{"balance": 1240.5}`), or an array of flat records when one entry holds several comparable items (`{"detail": [{"subject": "harmony", "minutes": 25}]}`); a scalar whose key is declared under `[schemas.tracking.fields]` is type-checked, and a key naming the note's identity (`type`, `stewardship`, `activity`, `date`) is refused. `date` files the entry for a past day (bounded to 50 years back, 1 year ahead). A second call for the same `(activity, date)` **merges** into the first: content appended, metrics folded in. Records carrying a stable `id` replace the record with that `id`; records without one append, so re-sending a payload without ids double-counts summed metrics. Either way the write is journalled to today's daily log. |

## Frontmatter

| Tool | Inputs | Effect |
|------|--------|--------|
| `set_frontmatter` | `note`, `key`, `value` | Set a declared, `settable = true` typed frontmatter field through the index (no desync). `note` is `today`, a `YYYY-MM-DD` date, or a vault-relative path. Engine-owned keys (`type`, `status`, a period key) are rejected; the value is type-checked; `log_on_change` fields stamp a daily-log line. ([`cdno frontmatter set`](../cli/frontmatter.md)) |

## Daily, weekly, and monthly sections

| Tool | Inputs | Effect |
|------|--------|--------|
| `upsert_daily_section` | `section` (`Standup`\|`Intention`\|`Agenda`\|`Meeting`), `content?`, `date?`, `append?` | Write or append a daily-note section. |
| `upsert_weekly_section` | `section` (`Wins`\|`Challenges`\|`One Improvement`\|`This Week's Goal`), `content?`, `date?`, `append?` | Write or append a weekly-note section. |
| `upsert_monthly_section` | `section` (`Wins`\|`Themes`\|`Next Month's Focus`), `content?`, `date?`, `append?` | Write or append a monthly-note section. |

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
