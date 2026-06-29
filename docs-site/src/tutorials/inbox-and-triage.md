# Inbox and triage

The inbox is a pressure-release valve: capture a thought *now*, decide what to do with it *later*.
This keeps the daily loop friction-free — you never have to stop and classify something mid-flow.

## Capture

```bash
cdno capture "does Chen 2025 use the same preconditioner?"
cdno capture "ask IT about the cluster quota"
```

Each capture becomes a small slug-named note in `inbox/`. That's the whole cost — no fields, no
decisions. Capture liberally.

## Triage

When you have a moment (a natural fit for the [weekly review](weekly-review.md)), process the inbox:

```bash
cdno triage
```

Interactively, `triage` walks each pending capture and offers, for each one, to:

- **keep it as a project action** — turn it into a next action on a project, or
- **discard it** — it served its purpose, or
- **skip it** — leave it for next time.

Run non-interactively (or with `--no-interactive`), `triage` just **lists** what's pending — a quick
way to see the backlog without acting on it.

## A healthy inbox is empty-ish

The inbox is a transit point, not a filing cabinet. The goal isn't zero at all times — it's that
nothing important lives *only* there. Anything worth keeping becomes an action, a piece of evidence,
a question, or a log line; the rest gets discarded without ceremony.

## With Claude

The MCP tools `capture`, `triage_inbox` (lists pending items), and `discard_inbox_item` let an
assistant capture on your behalf and help you clear the backlog conversationally.

Next: [Searching your vault](search.md).
