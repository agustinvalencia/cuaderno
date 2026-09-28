# RFC 0003 — Installing a bundled note type into an existing vault

| | |
|---|---|
| **Status** | Accepted — 2026-09-28 (three-seat review, two rounds, on the RFC PR; §7 confirmed by the maintainer) |
| **Tracked by** | epic issue (opened on acceptance); implementation in one PR per §8 |
| **Affects** | `cdno-cli` (`init`, `config note-type`), `cdno-mcp` (one description), `examples/note-types/`, `docs-site` |
| **Related** | RFC 0002 (the `concept` type this exists to install); `cdno config` (#598) and `cdno templates` (#599), whose gates and verbs this reuses; `examples/note-types/README.md` (the two-file recipe this replaces) |

> **Authorship.** Drafted by Claude (Anthropic) from a question the maintainer raised after
> RFC 0002 shipped: how does an existing vault get the concept type, and should `cdno init`
> grow an option for it. A three-seat review round (CLI surface; method and user; safety, tests
> and maintenance) settled the four open questions in §7; the record is on the PR. The
> maintainer's rulings are in §7.

---

## 1. Summary

Add one verb that installs a note type the binary ships with into an already-initialised vault:

```bash
cdno config note-type install --name concept
```

It appends the type's `[note_types.<name>]` declaration as text through the existing
validate-first, compare-and-swap config gate, installs the type's template into
`.cuaderno/templates/` when no file of that name exists, and creates the type's folder. An
existing declaration is never modified. `cdno init` calls the same function for the types it
seeds, so a new vault and an upgraded one are identical by construction.

`cdno init` itself does not change: it still refuses to run on an initialised vault.

### 1.1 How you will use it

You upgrade `cdno`, open your vault of two years, and run `cdno config note-type install
--list`. It shows `concept: not installed`, with its folder, fields and template sections. You
run `cdno config note-type install --name concept`; it reports the declaration written, the
template written and `concepts/` created, and `cdno config validate` passes. From then on the
tutorial in `docs-site/src/tutorials/concept-library.md` applies to your vault unchanged. If
you had copied the block by hand last week, the same command reports the declaration as kept and
matching, writes the template you had not copied, and exits 0.

---

## 2. Motivation

RFC 0002 made `concept` a custom type that `cdno init` declares in a new vault. An existing vault
gets it by hand: copy a TOML block from `examples/note-types/concept/config.toml` into
`.cuaderno/config.toml`, then copy `concept.md` into `.cuaderno/templates/`. That is two files
fetched from the repository, one of them edited into a config the tool otherwise guards with a
validate-first gate, for a type the binary already carries verbatim (`CONCEPT_TYPE_BLOCK` and
`CONCEPT_TEMPLATE` in `crates/cdno-cli/src/commands/init.rs`).

Every vault the maintainer cares about is an existing one. If adoption of the concept library
depends on a copy-and-paste recipe, the six-week trial (RFC 0002 T18) starts on a vault that was
set up differently from the one the tests cover.

The obvious shortcut, `cdno init --concepts`, is rejected in §5.1.

---

## 3. Background

- `cdno init` refuses if `.cuaderno/` exists, deliberately: re-init is destructive and the user
  opts in by removing the directory. It seeds `daily.md`, and since #644 and #649 the concept
  declaration and `concept.md`.
- `cdno config note-type set / remove` edit `[note_types.<name>]` through the config gate
  (`finish_edit` in `crates/cdno-cli/src/commands/config.rs`): validate the candidate text
  first, then compare-and-swap against the hash the read handed out. A config that would not
  reopen is never written. Like every config write, this path bypasses `VaultTransaction` and
  the cross-process `.cuaderno/.lock` (CLAUDE.md lists it as a documented exception): the
  compare-and-swap catches a concurrent hand edit or a second `install`, while a `cdno` note
  write at the same moment is simply unserialised with it, as today. Config and note files never
  share bytes, so the risk is the same one `note-type set` already carries.
- `note-type set` builds its candidate with `config_edit::set_note_type`, which rebuilds the
  table key by key through `toml_edit`. That drops comments. `install` cannot use it (§4.2.1).
- `cdno templates save / new / eject` manage `.cuaderno/templates/`. A custom type with no
  `template =` key resolves to `<name>.md` (`crates/cdno-domain/src/vault/templating.rs`).
- `examples/note-types/` holds two reference types, `concept` and `person`, each a `config.toml`
  block plus a template, with a README recipe. A test (`example_matches_init` in
  `crates/cdno-cli/tests/init.rs`) pins the `concept` pair byte-identical to what `init` writes.

---

## 4. Proposal

### 4.1 The verb

```
cdno config note-type install --name <NAME> [--json]
cdno config note-type install --list [--json]
cdno config note-type install --name <NAME> --dry-run
```

- `--name` is an `Option<String>` folded through `gather_or_error` like `set` and `remove`
  (`docs/cli-ergonomics.md`: `install` mutates the vault, so it takes no positional). Omitted
  and interactive, it offers a picker over the bundled types; the picker counts as a prompt, so
  the command confirms before writing. Omitted and non-interactive, `missing_flag("name")`.
- `--list` conflicts with `--name`. It prints, per bundled type: name, one-line purpose, folder,
  required and optional fields, template filename and the template's section headings, and the
  vault's state: `not installed`, `installed (matches)`, `installed (declaration differs)`,
  `installed (template customised)`. It does not print template bodies.
- `--dry-run` prints the exact block and template that would be written and changes nothing.

Bundled types are the note types `init` knows, nothing more; the RFC uses "bundled" throughout
and introduces no other noun. Once installed, a bundled type is an ordinary custom declaration
the owner may edit or delete.

### 4.2 Behaviour

The steps run in this order, so a failure never leaves a declaration pointing at a missing
template: template, folder, declaration.

#### 4.2.1 Template

Create `.cuaderno/templates/` if absent. Write the bundled template only when the file named by
the declaration that will be in force is absent: for a fresh install that is the bundled
filename; for an existing declaration it is its `template` key or `<name>.md`, and the bundled
file is written only when that name equals the bundled filename. Otherwise report
`not installed (declaration names x.md)`. A present file is never overwritten; the report says
whether it is identical to the bundled one.

#### 4.2.2 Folder

Create the type's folder if absent (`concepts/`). Empty folders are not indexed, so this is
cosmetic, but it makes the install visible and keeps `init` and `install` identical.

#### 4.2.3 Declaration

Read the config with `read_config_from` and parse it with the same `read_model` the structured
verbs use; refuse with their message if it does not parse. Decide "declared" from
`model.note_types.contains_key(name)`, never from a text search (a commented
`# [note_types.concept]` must not count, and a dotted `note_types.concept.folder = …` key must).

- **Absent:** the candidate is the original text, then a newline if it does not already end in
  one, then a blank line if it does not already end in one, then the bundled block verbatim.
  Submit it through `finish_edit` and report its errors unchanged. `DEFAULT_CONFIG_TOML` ends in
  `\n\n`, so this adds nothing on a fresh vault and T0's byte-identical probe holds. A config
  that declares `note_types` as an inline table cannot take an appended header; the gate refuses
  with a duplicate-key error, and the verb translates it: "`note_types` is declared inline; add
  the block with `cdno config edit`".
- **Present:** never modified and never merged. The command compares it field by field, as a
  parsed `CustomNoteType` rather than as text, with the bundled declaration and reports
  `kept (matches bundled)` or `kept (differs: <key>: bundled […], yours […])`, with the
  `note-type set` invocation that would adopt the bundled value. It then carries on with the
  other steps.

The command exits 0 unless a write fails. When nothing was written, the last line reads
`<name>: already installed, nothing to do`. This rule was chosen over refusing (§7.4): refusing
strands every vault that followed a recipe with an optional template, and the only escape,
`note-type remove`, discards the owner's edits. Carrying on writes only absent files and never a
declared value, so it cannot surprise anyone.

#### 4.2.4 Report

```
declaration  written | kept (matches bundled) | kept (differs: …)
template     written .cuaderno/templates/concept.md | kept (matches bundled) | kept (customised) | not installed (declaration names x.md)
folder       created concepts/ | present concepts/
```

`--json` follows the `config` verbs' convention (the shared `emit` helper, with `changed`):

```json
{"changed": true, "note_type": "concept",
 "declaration": "written",
 "template": {"path": "templates/concept.md", "action": "written"},
 "folder": {"path": "concepts", "action": "created"}}
```

with the same enum values as the text report, and `--list --json` as
`{"bundled": [{"name": "concept", "state": "not_installed", …}]}`.

### 4.3 Bundled types

A bundled type is `(name, purpose, declaration block, template filename, template content)`, a
`const` slice in `cdno-cli`. The block and template are `include_str!`ed from
`examples/note-types/<name>/`, so there is exactly one copy on disk and the examples cannot
drift from the binary. The existing byte-identical test then asserts something more useful: that
`cdno config validate` accepts the installed result and that `init` and `install` produce the
same files.

One type ships: `concept`. `person` stays in `examples/` (§7.2). Its example comment ("merge
this table into your config") is reworded in the concept block's style regardless, so that it
could be installed verbatim later.

A registry invariant test guards future entries: no bundled name is in `NoteType::ALL`, no
template filename is `<builtin>.md` or `<builtin>-*.md` (which would silently become a built-in's
override), and no folder's top segment is in the reserved set.

### 4.4 `init` reuses it

`init` keeps its contract (refuse on an existing vault, create the layout, write the default
config and `daily.md`) and then calls `install_bundled(root, "concept")`. The config is therefore
written twice on `init`, the default through `fs::write` and the append through the gate; that
is harmless and the doc comment says so. Whether `init` seeds `concept` at all was decided in
RFC 0002 §9 and is not reopened here.

### 4.5 The MCP surface

No install tool (§5.4). One description changes: when `create_custom_note` is asked for a type
that is not declared, its refusal names the command, "run `cdno config note-type install --name
<name>`" when the name is bundled, so an agent can tell the owner what to do.

---

## 5. Alternatives

### 5.1 `cdno init --concepts` on an existing vault

Rejected. `init` has one contract, and the refusal on an existing vault is a safety property.
Making it additive under a flag gives one verb two meanings, leaves the semantics of "already
declared" and "template customised" undefined, and puts every future bundled type on `init` as
another flag. The user's question that prompted this RFC ("would it override the existing
vault's?") is exactly the doubt a dual-purpose `init` creates.

### 5.2 Do nothing; keep the two-file recipe

Rejected for the reason in §2: the binary already carries the content, and a hand-edited config
is the one thing the config verbs exist to avoid.

### 5.3 `cdno templates install`

Rejected: the template is the smaller half, and the declaration is what makes the type exist.
The verb belongs where declarations are edited.

### 5.4 An MCP install tool

Not proposed. Installing a type is a one-off vault-administration act, like `init` and
`config`, neither of which has an MCP counterpart. `list_note_types` already tells an agent
whether `concept` exists, and §4.5 makes the refusal name the command.

### 5.5 `cdno doctor`

Deferred. A report of what a fresh vault would have that this one lacks (bundled types not
installed, templates that differ from bundled, index staleness) is the natural home for the
upgrade story; `cdno-core/src/index.rs` already anticipates one. `--list` is its first slice
and should read the same registry. Not in this RFC.

---

## 6. Compatibility

- No change for new vaults: `init` produces the same files as before, through the shared
  function.
- A vault that copied the block by hand: `install` reports the declaration as kept, and writes
  the template if it was never copied.
- A vault whose owner deleted the declaration but kept notes under `concepts/` and an edited
  template: `install` writes the declaration, keeps the template (`customised`), and reports the
  folder as present.
- A vault where another custom type already owns `folder = "concepts"`: the gate refuses ("both
  declare folder `concepts`"); the verb reports that in plain terms, and the template written in
  the earlier step is removed again so nothing is left behind.
- No index, schema or domain change. `cdno-core` and `cdno-domain` are untouched.
- The release notes carry one line for existing vaults: "Run `cdno config note-type install
  --name concept` to add the concept library to a vault created before 0.40."

---

## 7. Decisions

Settled by the review round; the maintainer confirms or overrides on the PR.

1. **Name and shape.** `config note-type install --name <NAME>`. `install` keeps shipped types
   apart from `set` in `--help`; `--name` matches its siblings and the convention.
2. **`person`.** Not bundled in this RFC. The registry can carry it, but listing it beside
   `concept` gives it standing the method has not argued: it is absent from `docs/design.md` §3
   and has no place in RFC 0002's filing test. It follows as a one-entry addition once the
   design document says where people sit.
3. **Folder creation.** Yes, for visibility and so `init` and `install` match.
4. **Existing declaration.** Kept untouched, compared and reported; the template and folder
   steps still run; exit 0. Neither refuse (strands the optional-template vaults) nor merge
   (would silently change an owner's `optional` list).

---

## 8. Implementation plan

Small enough for one PR after acceptance; three commits if reviewed separately.

- **T0.** `include_str!` the concept pair from `examples/note-types/concept/`; extract
  `install_bundled(root, name) -> InstallReport` in `cdno-cli`; make `init` call it; reword the
  `person` example comment; add the registry invariant test. No behaviour change. Probe:
  `cdno init` output before and after is byte-identical for `config.toml` and `templates/`, and
  the directory listing matches when compared on the same date (the listing includes
  `init_dirs(today)` year folders).
- **T1.** The verb: `--name` with picker, `--list`, `--dry-run`, `--json`, the report, the
  refusal translations. `assert_cmd` tests: fresh install writes all three; second run reports
  kept, kept, present and exits 0; declared-without-template gets the template; declaration
  naming another template file gets `not installed`; customised template is kept and reported;
  deleted-declaration-with-notes case; folder collision refuses and leaves nothing behind;
  missing `.cuaderno/templates/` is created; inline `note_types` table gets the translated
  message; `--list` states are right in each of those vaults. The gate-holds test calls
  `install_bundled` directly with a hand-built invalid block (a `const` registry cannot be
  mutated from `assert_cmd`), asserting nothing is written.
- **T2.** Docs: `docs-site/src/reference/cli/config.md`; `reference/custom-note-types.md`;
  `concepts/concept-library.md` and the tutorial's "for an older vault" line; an "Upgrading an
  older vault" section in `getting-started/initialise-a-vault.md` (whose tree also omits
  `concepts/` today); `concepts/configuration.md`; `docs/design.md` §5.12 and RFC 0002 §6.1,
  which still describe the hand copy; `examples/note-types/README.md`; the `create_custom_note`
  description; `CHANGELOG.md` worded for the release notes; `STATUS.md`. `mdbook build
  docs-site` clean.

---

## 9. Verification

- `cdno init` in a temp dir, then `install --name concept` in the same vault: three `kept` or
  `present` lines, `already installed, nothing to do`, exit 0, and the vault is byte-identical.
- A vault initialised by a pre-#644 binary (no concept block): `install --name concept` adds the
  block, the template and the folder; `cdno config validate` passes; `cdno note create concept
  --title x --body-file f` works and logs `concept created [[concepts/x]] — x`.
- A vault with the block copied by hand and no template: the declaration is kept and matches,
  the template is written.
- `install --name concept` after the user edited `.cuaderno/templates/concept.md`: the template
  is untouched and reported as customised.
- The gate-holds test: an invalid block is never written.
- The registry invariant test fails on a bundled name, template filename or folder that
  collides with a built-in.
