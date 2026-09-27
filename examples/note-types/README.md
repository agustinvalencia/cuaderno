# Example custom note types

[Custom note types](https://agustinvalencia.github.io/cuaderno/reference/custom-note-types.html) let
you declare your own schema-only note type in `.cuaderno/config.toml` — for entities the eleven
built-in types don't cover. Each folder here is a ready-made example: a config snippet plus an
optional template.

## `person/` — track the people you work with

Answers questions like *"what was my last interaction with X?"* and *"what did X ask me to do?"*
without a bespoke CRM.

1. Merge [`person/config.toml`](person/config.toml) into your vault's `.cuaderno/config.toml`.
2. Optionally copy [`person/person.md`](person/person.md) to `.cuaderno/templates/person.md` for a
   richer note shape (without it, Cuaderno synthesises a minimal note).
3. Create people and log interactions:

   ```bash
   cdno note create person --title "Jane Smith"
   cdno note list person
   ```

Reference a person from your daily logs, meeting notes, and action notes with
`[[people/jane-smith]]`. Then:

- **Last interaction** — read the top line of the person note's `## Log` (kept most-recent-first);
  or `cdno search "people/jane-smith" --type daily --from <date>` to find mentions (relevance-ranked,
  so read the dates rather than the order).
- **Who asked whom** — note the direction in the prose (the template's `## Log` comment shows the
  convention); search surfaces the lines, you read the direction.

See the full recipe in
[Tracking people](https://agustinvalencia.github.io/cuaderno/tutorials/tracking-people.html).

## `concept/` — a library of reusable understanding

A concept note is a unit of understanding worth keeping and reusing, tied to no project, question,
stewardship or portfolio: a theorem and the intuition behind it, a definition, a technique and when
it applies, a procedure you will run again (RFC 0002). `cdno init` already writes the same
`[note_types.concept]` block into a new vault's config; this copy is for vaults created before
that, and a test keeps the two byte-identical.

1. Copy the block in [`concept/config.toml`](concept/config.toml) into your vault's
   `.cuaderno/config.toml`.
2. Copy [`concept/concept.md`](concept/concept.md) to `.cuaderno/templates/concept.md` (the block
   names it as the type's template).
3. Create concepts, supplying the body and, when promoting from the daily log, the origin:

   ```bash
   cdno note create concept --title "Woodbury identity" --body-file woodbury.md \
     --origin "[[journal/2026/daily/2026-09-02#Woodbury identity]]"
   ```

   The MCP `create_custom_note` tool takes the same `body` and `origin`. Promotion is simply
   create-with-`origin`: there is no separate operation, and `origin` records the daily-log
   entries the concept came from. It is written as a quoted YAML string, so several links in one
   value stay valid.

The body is inserted where `{{body}}` sits, under the title and above the fallback sections
*Statement*, *Why it matters* and *See also*. Those sections appear always, whether or not a body
is supplied: an agent that drafted a full body can pass its own headings and delete or ignore the
empty ones, and a note started empty still shows a human what to write. A procedure or a definition
simply uses different headings. The template's frontmatter keys are in canonical order (`type`, the
declared fields, then `title`), so a new concept note is clean under `cdno lint --strict`.
