# RFC 0003 — Installing a bundled note type into an existing vault

| | |
|---|---|
| **Status** | Draft — 2026-09-28 |
| **Tracked by** | this PR; an epic and task issues follow acceptance |
| **Affects** | `cdno-cli` (`init`, `config note-type`), `examples/note-types/`, `docs-site` |
| **Related** | RFC 0002 (the `concept` type this exists to install); `cdno config` (#598) and `cdno templates` (#599), whose gates and verbs this reuses; `examples/note-types/README.md` (the two-file recipe this replaces) |

> **Authorship.** Drafted by Claude (Anthropic) from a question the maintainer raised after
> RFC 0002 shipped: how does an existing vault get the concept type, and should `cdno init`
> grow an option for it. The decision to keep `init` single-purpose and put the capability
> beside the config verbs is the maintainer's to confirm in §7.

---

## 1. Summary

Add one verb that installs a note type the binary ships with into an already-initialised vault:

```bash
cdno config note-type install concept
```

It writes the type's `[note_types.<name>]` declaration through the existing validate-first,
compare-and-swap config gate, installs the type's template into `.cuaderno/templates/` when no
file of that name exists, and creates the type's folder. `cdno init` calls the same function for
the presets it seeds, so a new vault and an upgraded one are identical by construction.

`cdno init` itself does not change: it still refuses to run on an initialised vault.

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
- `cdno config note-type set / remove` edit `[note_types.<name>]` through `save_config_raw`'s
  gate: validate the candidate first, then compare-and-swap against the hash the read handed out.
  A config that would not reopen is never written.
- `cdno templates save / new / eject` manage `.cuaderno/templates/`. `templates new <type>`
  scaffolds a starter from a custom type's `required` and `optional` fields; it does not know
  the shipped concept template.
- `examples/note-types/` holds two reference types, `concept` and `person`, each a `config.toml`
  block plus a template, with a README recipe. A test pins the `concept` pair byte-identical to
  what `init` writes.

---

## 4. Proposal

### 4.1 The verb

```
cdno config note-type install <NAME> [--json]
cdno config note-type install --list
```

`<NAME>` is a preset name. `--list` prints the presets the binary ships and, for each, whether the
vault already has a `[note_types.<name>]` table. A read verb, so the positional is allowed
(`docs/cli-ergonomics.md`); there is nothing to prompt for beyond the name, which is gathered
through `gather_or_error` like the other `note-type` verbs when omitted.

### 4.2 Behaviour

1. **Declaration.** If `[note_types.<name>]` is absent, append the preset's block to
   `.cuaderno/config.toml` through the same gate `note-type set` uses. If it is present, refuse
   with "note type `<name>` is already declared; remove it with `cdno config note-type remove
   --name <name>` to reinstall", and change nothing. The verb never merges into or overwrites a
   declaration, because a user who edited it has a reason.
2. **Template.** If `.cuaderno/templates/<preset template>` is absent, write it. If present, keep
   it and say so. The same rule `init` applies today.
3. **Folder.** Create the type's folder if absent (`concepts/` for the concept preset). Empty
   folders are not indexed, so this is cosmetic, but it makes the install visible.
4. **Report.** One line per outcome: declaration written, template written or kept, folder
   created or present. `--json` returns the same three facts.

The verb is idempotent in the sense that a second run changes nothing and says why. It is not a
force: there is no `--force`, because the removal path already exists and is explicit.

### 4.3 Presets

A preset is `(name, declaration block, template filename, template content)`, a `const` slice in
`cdno-cli`. Two ship:

- `concept`: the RFC 0002 type, from the constants `init` already carries.
- `person`: the `examples/note-types/person/` pair, promoted from example to preset. Its README
  recipe is the same two-file copy this RFC removes.

`examples/note-types/` stays as the human-readable reference, and the existing byte-identical
test grows to cover every preset, so the examples and the binary cannot drift.

### 4.4 `init` reuses it

`init` keeps its contract (refuse on an existing vault, create the layout, write the default
config and `daily.md`) and then calls the install function for the presets it seeds. Today that is
`concept` only; `person` is not seeded, because a new vault should not open with a `people/`
folder its owner did not ask for. Whether `init` should seed `concept` at all was decided in
RFC 0002 §9 and is not reopened here.

---

## 5. Alternatives

### 5.1 `cdno init --concepts` on an existing vault

Rejected. `init` has one contract, and the refusal on an existing vault is a safety property.
Making it additive under a flag gives one verb two meanings, leaves the semantics of "already
declared" and "template customised" undefined, and puts every future preset on `init` as another
flag. The user's question that prompted this RFC ("would it override the existing vault's?") is
exactly the doubt a dual-purpose `init` creates.

### 5.2 Do nothing; keep the two-file recipe

Rejected for the reason in §2: the binary already carries the content, and a hand-edited config
is the one thing the config verbs exist to avoid.

### 5.3 `cdno templates install`

Rejected: the template is the smaller half, and the declaration is what makes the type exist.
The verb belongs where declarations are edited.

### 5.4 An MCP tool

Not proposed. Installing a type is a one-off vault-administration act, like `init` and
`config`, neither of which has an MCP counterpart. `list_note_types` already tells an agent
whether `concept` exists; the tool description can say "ask the owner to run `cdno config
note-type install concept`" if it does not.

---

## 6. Compatibility

- No change for new vaults: `init` produces the same files as before, through the shared function.
- No change for vaults that already copied the block by hand: `install concept` refuses because
  the declaration is present, and reports the template as kept.
- No index, schema or domain change. `cdno-core` and `cdno-domain` are untouched.
- `examples/note-types/README.md` shortens both recipes to one command; the files stay.

---

## 7. Decisions and open questions

1. **Name.** `config note-type install` (proposed) versus `config note-type add --preset`. The
   proposal keeps `set` for user-defined types and `install` for shipped ones, so the two cannot
   be confused in `--help`.
2. **`person` as a preset.** Proposed yes, since the mechanism is free once it exists. If no, the
   registry has one entry and the README keeps the `person` recipe.
3. **Folder creation.** Proposed yes, for visibility. It could be dropped; the first note creates
   the folder anyway.
4. **Refuse versus merge on an existing declaration.** Proposed refuse. A merge would silently
   change a user's `optional` list.

---

## 8. Implementation plan

Small enough for one PR after acceptance, or three if reviewed separately:

- **T0.** Extract `install_preset(root, name) -> InstallReport` into `cdno-cli`, backed by the
  preset registry; make `init` call it for `concept`; extend the byte-identical test to every
  preset. No behaviour change. Probe: `cdno init` output is byte-identical before and after
  (compare `config.toml`, `templates/`, and the directory listing).
- **T1.** The verb, `--list`, `--json`, the refusals, and `assert_cmd` tests: fresh vault installs
  both facts; second run refuses; customised template is kept; `--list` shows presence; a config
  that fails validation after the append is not written (mutate the block in a test to prove the
  gate holds).
- **T2.** Docs: `docs-site/src/reference/cli/config.md`, `reference/custom-note-types.md`, the
  "for an older vault" line in `tutorials/concept-library.md`, `examples/note-types/README.md`,
  `CHANGELOG.md`, `STATUS.md`. `mdbook build docs-site` clean.

---

## 9. Verification

- `cdno init` in a temp dir, then `cdno config note-type install concept` in the same vault:
  refuses, and the vault is byte-identical to before.
- A vault initialised by a pre-#644 binary (no concept block): `install concept` adds the block,
  the template and the folder; `cdno config validate` passes; `cdno note create concept --title x
  --body-file f` works and logs `concept created [[concepts/x]] — x`.
- `install concept` after the user edited `.cuaderno/templates/concept.md`: the template is
  untouched and reported as kept.
- The examples-to-binary test fails if either `examples/note-types/<preset>/` file drifts.
