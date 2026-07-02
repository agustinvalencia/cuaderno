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
