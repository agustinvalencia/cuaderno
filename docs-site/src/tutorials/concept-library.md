# Building a concept library

This walkthrough follows one piece of understanding from a scribble in the day to a refined concept
note: you work something out, note it, meet it again three weeks later, promote it, and then
improve it. The method behind each step is in [The concept library](../concepts/concept-library.md).

It assumes a vault created by `cdno init`, which declares the `concept` type. For an older vault,
first run `cdno config note-type install --name concept`.
The project `surrogate-model` stands in for whatever you are working on.

The input files below are scratch; write them outside the vault (for example under `/tmp`), or a
stray `.md` in the vault root is reported as unindexable. The commands here use `/tmp`.

## 1. Note the substance in the day

On 2 September you work out why a low-rank refit is cheap. It is not a one-line event, so it does
not go in `## Logs`; write it to a file (or let the editor open, below) and log it as a note:

```bash
cat > /tmp/woodbury.md <<'EOF'
(A + UCV)^-1 = A^-1 - A^-1 U (C^-1 + V A^-1 U)^-1 V A^-1.

Cheap when A^-1 is already known and the update is low-rank: today's use was the
k=3 refit on [[surrogate-model]]. #concept
EOF

cdno log note --heading "Woodbury identity" --body-file /tmp/woodbury.md
```

```text
Noted journal/2026/daily/2026-09-02#Woodbury identity
```

The body ends with `#concept` because this looks reusable. Run interactively without the flags,
`cdno log note` asks for the heading and opens the body in your editor instead.

The daily note now holds the entry under `## Notes`, and a pointer to it in `## Logs`, with the
entry's wikilinks copied onto the pointer line:

```markdown
# Wednesday, 2 September 2026

## Notes
### Woodbury identity
(A + UCV)^-1 = A^-1 - A^-1 U (C^-1 + V A^-1 U)^-1 V A^-1.

Cheap when A^-1 is already known and the update is low-rank: today's use was the
k=3 refit on [[surrogate-model]]. #concept

## Logs
- **14:32**: noted [[journal/2026/daily/2026-09-02#Woodbury identity]] ([[surrogate-model]])
```

`## Notes` did not exist yet, so it was created just above `## Logs`. You did not have to decide
whether this deserves a note of its own — that decision can wait.

## 2. Meet it again

On 24 September the same identity comes up in a k=5 refit. Note it again, under a heading of its
own for that day. `--date` writes to a chosen day's note (stamped at the current time); `--json`
shows everything the command returns:

```bash
printf 'Refit again with k=5 on [[surrogate-model]]: Woodbury keeps the update at O(k^3) rather than a full re-inverse.\n#concept\n' > /tmp/refit.md

cdno log note --heading "Low-rank refit" --body-file /tmp/refit.md --date 2026-09-24 --json
```

```json
{
  "log_line": "noted [[journal/2026/daily/2026-09-24#Low-rank refit]] ([[surrogate-model]])",
  "message": "Noted journal/2026/daily/2026-09-24#Low-rank refit",
  "path": "journal/2026/daily/2026-09-24.md",
  "target": "journal/2026/daily/2026-09-24#Low-rank refit"
}
```

`target` is the entry's address: cite it anywhere as `[[<target>]]`.

A heading already used that day, or one that reuses a section name, is refused and nothing is
written:

```text
$ cdno log note --heading "Logs" --body-file /tmp/refit.md --date 2026-09-24
Error: heading `Logs` is not allowed in `## Notes`: it is the name of a daily section
```

## 3. Find the candidates

The subject has now come up on two dates — the moment to promote. Search the daily notes for the
tag's word:

```bash
cdno search concept --type daily
```

```text
Search: concept

▎ 1. Thursday, 24 September 2026    daily
▎ journal/2026/daily/2026-09-24.md
▎ …a full re-inverse. #[concept] ## Logs - **14:40**: noted [[journal…

▎ 2. Wednesday, 2 September 2026    daily
▎ journal/2026/daily/2026-09-02.md
▎ …refit on [[surrogate-model]]. #[concept] ## Logs - **14:32**: noted [[journal…
```

Search matches the **word** `concept`, not the tag, so in a lived-in vault it also returns days
that only created, revised or linked a concept note. Open each hit (`cdno open 2026-09-24`) and read
the `## Notes` entries that end in `#concept` to see which share a subject.

## 4. Check the library first

Before you create anything, make sure the library does not already hold this concept:

```bash
cdno search woodbury --type concept
```

```text
Search: woodbury
  (no matches)
```

Had there been a hit, you would refine that note (step 6) rather than create a second one.

## 5. Promote: create with `origin`

Write the concept's opening in a file — without the `# Title` line, which the engine writes — and
create the note with `origin` pointing at both entries:

```bash
cat > /tmp/concept-body.md <<'EOF'
A low-rank correction to a matrix whose inverse you already hold can be inverted
without starting again.
EOF

cdno note create concept --title "Woodbury identity" --body-file /tmp/concept-body.md \
  --origin "[[journal/2026/daily/2026-09-02#Woodbury identity]] [[journal/2026/daily/2026-09-24#Low-rank refit]]"
```

```text
Created concepts/woodbury-identity.md
```

```markdown
---
type: concept
created: 2026-09-25
tags: []
origin: '[[journal/2026/daily/2026-09-02#Woodbury identity]] [[journal/2026/daily/2026-09-24#Low-rank refit]]'
---

# Woodbury identity

A low-rank correction to a matrix whose inverse you already hold can be inverted
without starting again.

## Statement

## Why it matters

## See also
```

The body fills the template's `{{body}}` slot, and the template adds its three sections after it.
`origin` is stored as a single quoted string. Its links are indexed with the write, so the daily
notes it names show the concept among their backlinks straight away.

Fill in `tags` in your editor (`tags: [linear-algebra]`). A `--field tags=…` value is written as a
plain string, not a list, so the tag list is easiest to edit by hand.

The creation is logged to the day you run it, and you do not log it again:

```text
- **09:10**: concept created [[concepts/woodbury-identity]] — Woodbury identity
```

## 6. Refine it

Fill the empty sections one at a time. Each call names the section, the file holding its new text
(without the heading), and a reason:

```bash
printf '(A + UCV)^-1 = A^-1 - A^-1 U (C^-1 + V A^-1 U)^-1 V A^-1\n' > /tmp/statement.md
cdno note revise woodbury-identity --section Statement --content-file /tmp/statement.md \
  --reason "stated the identity"
```

```text
Revised concepts/woodbury-identity.md
```

With `--json`, the result says what was logged:

```bash
printf 'Refitting a rank-k update costs O(k^3) plus the products with A^-1, instead of a fresh O(n^3) inverse; the k=5 refit on [[surrogate-model]] is the worked case.\n' > /tmp/why.md
cdno note revise woodbury-identity --section "Why it matters" --content-file /tmp/why.md \
  --reason "added the cost comparison" --json
```

```json
{
  "changed": true,
  "log_line": "revised [[concepts/woodbury-identity#Why it matters]] — added the cost comparison",
  "message": "Revised concepts/woodbury-identity.md",
  "new_hash": "1d1d1cbac3cb10a7",
  "path": "concepts/woodbury-identity.md",
  "section_target": "concepts/woodbury-identity#Why it matters"
}
```

Running the same revision again changes nothing, so nothing is written or logged:

```text
$ cdno note revise woodbury-identity --section Statement --content-file /tmp/statement.md --reason "again"
No change to concepts/woodbury-identity.md
```

The reason is not optional. In a script, leaving it out is an error; in a terminal, you are asked
for it:

```text
$ cdno note revise woodbury-identity --no-interactive --section "See also" --content-file /tmp/see-also.md
Error: missing required flag: --reason (provide it explicitly or run interactively in a TTY)
```

To rewrite the whole body instead, pass `--body-file`, or run `cdno note revise woodbury-identity`
in a terminal with no file and the current body opens in your editor. If the note changes on disk
while that editor is open, the revision is refused rather than overwriting the change.

Only custom types that are not append-only can be revised; a project, for one, is refused:

```text
$ cdno note revise surrogate-model --body-file /tmp/why.md --reason "tidy"
Error: note 'projects/surrogate-model.md' cannot be revised: 'project' is a built-in note type; its sections are owned by its own commands
```

## 7. Read the trail

The day's `## Logs` now tells the concept's story in three lines, and the two earlier days keep
their `noted` pointers:

```markdown
## Logs
- **09:10**: concept created [[concepts/woodbury-identity]] — Woodbury identity
- **09:14**: revised [[concepts/woodbury-identity#Statement]] — stated the identity
- **09:21**: revised [[concepts/woodbury-identity#Why it matters]] — added the cost comparison
```

There is no `was:` / `now:` copy of the old text: the log records *when* and *why*, and if you keep
the vault in git, git records *what*. Tracing how your understanding of the identity evolved is a
search over the journal: `cdno search woodbury --type daily`.

## With an assistant

Over [MCP](../reference/mcp/overview.md) the same walk uses four tools, and the assistant's tool
descriptions carry the habits above:

1. `note_to_daily` with `heading` and `body` — step 1 and 2.
2. `search_notes` with `query: "concept"` and `note_type: "daily"`, then `read_note` on each hit —
   step 3.
3. `search_notes` with `note_type: "concept"`, then `create_custom_note` with `type_name: "concept"`,
   `title`, `body` and `origin` — steps 4 and 5.
4. `revise_note` with `section`, `content` and a `reason` it drafts from what it changed — step 6.
   A whole-body `revise_note` needs the `content_hash` from `read_note` as `expected_hash`.

See [Write tools](../reference/mcp/writes.md) and
[Context-gathering tools](../reference/mcp/reads.md) for the parameters and refusal codes.

Next: [Actions](actions.md).
