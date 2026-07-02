# Tracking people

Cuaderno's built-in types don't include a "person" — deliberately, since a person decomposes into
the notes you already keep (daily logs, meetings, actions, commitments). But if you want to answer
questions like:

- *What was my last interaction with Jane?*
- *When did I ask Jane to do something — and what did she ask me?*

…a [custom note type](../reference/custom-note-types.md) plus a linking convention gets you there,
with no bespoke CRM. This recipe is the worked example; the files live in
[`examples/note-types/person/`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/note-types/person).

## 1. Declare the type

Add to `.cuaderno/config.toml`:

```toml
[note_types.person]
folder = "people"
```

That's the minimum — a person's identity is the note title. (Add `required`/`optional` fields if you
want enforced frontmatter; see the [reference](../reference/custom-note-types.md).)

Optionally copy the example [`person.md`](https://github.com/agustinvalencia/cuaderno/tree/main/examples/note-types/person/person.md)
to `.cuaderno/templates/person.md` for a note with a `## Log` section. Without a template, Cuaderno
synthesises a minimal note.

## 2. Create people

```bash
cdno note create person --title "Jane Smith"
```

This writes `people/jane-smith.md`. `cdno note list person` enumerates them; `cdno lint` keeps them
honest.

## 3. Link people from your notes

The key habit: whenever a person shows up in a daily log, a meeting section, or an action, reference
them with a `[[people/<slug>]]` wikilink:

```text
## Logs
- **14:30**: standup — [[people/jane-smith]] asked me to review the sparse-attention draft
```

Body wikilinks are indexed, so every mention becomes a backlink on the person's note, and the raw
text is full-text searchable.

## 4. Answer the questions

**"What was my last interaction with Jane?"** — the reliable answer is the top line of her person
note's `## Log`, which you keep most-recent-first. To scan across the vault, search her slug:

```bash
cdno search "people/jane-smith" --type daily --from 2026-06-01
```

Search ranks by relevance, **not date**, so bound the window with `--from` / `--to` and read the
dates on the hits (daily notes carry their date in the filename) rather than trusting the order.
Drop `--type` to include meetings and action notes.

**"What did Jane ask me — and what did I ask her?"** — this is *direction*, which lives in your
prose, not in structure. Write the log line so the direction is explicit ("Jane asked me…", "I asked
Jane…"), and either keep a running `## Log` in her person note or let search surface the lines. A
Claude skill can read the dated results and summarise who-owes-what; the structure gives it the
material, the prose gives it the direction.

## Why a "person" type and not a built-in

A custom `person` type is **schema-only** — it gives you a folder, linting, search, and backlinks,
but no bespoke behaviour. That's exactly right here: people don't need caps, state machines, or
aggregation; they need to be *findable and linkable*. If you later want richer per-person structure,
add fields to the config declaration — no code change. See
[Custom note types](../reference/custom-note-types.md) for the full schema.
