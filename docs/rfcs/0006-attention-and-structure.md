# RFC 0006 — Attention and structure: what the project cap counts, and what it should not

| | |
|---|---|
| **Status** | Draft, diagnosis only — 2026-10-18. §§1–4 are for review on their own; §5 (proposal) is deliberately not written until they have been. |
| **Tracked by** | — (no issue yet; this RFC is the issue) |
| **Affects** | `docs/design.md` (§ on the project cap, lifecycle and parking), `cdno-domain` (project lifecycle, orientation, reviews), `cdno-cli`, `cdno-mcp`, the review skills in `examples/`; the extent depends on §5 |
| **Related** | RFC 0004 (closures; the lifecycle enum and its folders), RFC 0005 (focus; `during:`), #667 (a project could only be parked), #611 (a parked project's milestones and the register), `max_active_projects` in `.cuaderno/config.toml` |

> **Authorship.** Drafted by Claude (Anthropic) from a series of design conversations with the
> maintainer in October 2026. The observations in §2 are the maintainer's, anonymised; the
> reading of them in §3, the survey in §4 and the principles in §6 are Claude's. Nothing here is
> decided. Where the text says "the maintainer confirms", the decision is open and §7 names it.

> **Why there is no proposal section.** The first attempt at this problem went straight to a
> remedy (a third lifecycle state, see §5.5) before the problem had been stated. That remedy may
> yet be right, but a document that opens with it would be reviewed as "is this the right name
> and bound" rather than "is this the right instrument". The proposal is therefore held back
> until §§2–4 and §6 have been read on their own terms. §5 lists what has already been tried or
> considered, in neutral past tense, as prior art.

---

## 1. Summary

The Research Logbook Method caps active projects (default five, `max_active_projects`) to bound
attention, and the cap is right. But in practice the cap is felt as too tight, people raise it
(the maintainer's own vault went from 5 to 6 as a stopgap), and, worse, they stop writing some
work down at all because there is no slot for it. A method whose premise is externalisation has
produced an incentive not to externalise. That cannot be fixed by tuning a number.

The diagnosis this RFC puts forward: the active project map is one primitive doing four jobs
that need to vary independently, and the method offers exactly one lever, parking, that moves
all four together. Every pressure observed so far is one of the four needing to come apart from
the others. The cap is the symptom; the flat taxonomy under it is the cause.

## 2. Observations

These are the specimens. They come from one vault over four months, anonymised; the claim is
that they are typical of anyone running the method over a mixed life, not peculiar to that
vault. Each is stated as what the user wanted and what the method made them do instead.

**O1. A quiet project that still needs its map.** A side project (as it happens, this
repository) gets little of the user's own time on weekdays. Agents move it forward; the user
reviews, decides and steers at weekends. It needs a project map: its Current State is updated,
next actions live on it, `during:` lines and focus point at it. But holding a full attention
slot for it is wrong, and parking it is worse: parking reads as "I stopped", drops the map out
of every view, and bringing it back needs a free slot. The user kept it active and complained
about the cap.

**O2. One work programme, several maps.** A research programme at work split, naturally and
correctly, into three project maps: a library, a method study, and a demonstrator. They have
different Current States, different tempos and different collaborators; one map with milestones
would have been a worse model. The three maps consume three of five slots, with no relation
between them that any view can see, and the orientation screen lists them as three unrelated
projects.

**O3. Work created parked, then refused.** A project that genuinely had not started yet (a
supervision that begins in some months) was created parked because the slots were full. The
user then wanted to pre-stage its first next actions. `add_action` refuses on a parked project
(`ProjectNotActive`), so the actions were written into a daily note instead, where no view will
find them. Parking was the right lifecycle answer and the wrong structural one.

**O4. Work never written down.** The user holds, outside the vault, a list of six undertakings
they have not created because there is no slot: finishing a degree (which would split into two
examinations and an administrative tail), a second ceremony for an event already held, with its
own legal follow-up abroad that would spawn children; a possible further degree, which if it
starts would spawn several projects of its own; a room renovation; learning the local language;
a residency or citizenship application. These are the clearest evidence: the method is pushing
real work out of the system.

**O5. The cap drifted and nobody noticed.** The vault's config was raised from 5 to 6 as an
experiment and left uncommitted; the remote MCP server stayed at 5; the vault had six active
projects. Two surfaces disagreed about the rule for two weeks. A number that is easy to nudge
and hard to defend gets nudged.

**O6. Cadence mismatch in the reviews.** The daily orientation and the weekly review ask the
same question of every active project ("did it move, why not"). A weekend project, a
weekday-only one and a bureaucratic one that moves once a quarter all get the same daily
pressure. The user's response was to want to hide some of them, which is O1 again from the
review's side.

## 3. Diagnosis

### 3.1 One primitive, four jobs

An active project map currently does all of the following, and the method has no way to grant
one without the others:

| Job | What it means today |
|---|---|
| **J1. Where structure lives** | Current State, Next Actions, Waiting On, Milestones, Links; the target of `during:` and of focus; the thing `get_project_context` reads |
| **J2. The unit the cap counts** | `status: active` is what `max_active_projects` counts |
| **J3. What orientation and the reviews nag about** | the active set is what `get_orientation` suggests from and what the weekly review walks |
| **J4. The node in the hierarchy** | on the operations track, the only node: actions below it, nothing above |

Mapping the observations onto the jobs:

| Observation | Wanted | Got |
|---|---|---|
| O1 quiet project | J1 without J2 and J3 | all four or none |
| O2 programme in three maps | J4 one level deeper | three unrelated J2 units |
| O3 parked, actions refused | J1 without J2 | parking removes J1 too |
| O4 work not written down | J1 and J4 without J2 | nothing, because J2 is full |
| O6 cadence | J3 to vary per project | J3 uniform across the active set |

Parking is the one lever, and it switches J1, J2 and J3 off together (and J4 is moot because
there is only one level). Every "the cap is too low" complaint above is really "I need one of
these jobs without the others".

### 3.2 Four defects, stated generally

**D1. A WIP limit was imported without its backlog.** The cap is a Kanban work-in-progress
limit, chosen for the ADHD reason (forced prioritisation, one screen), and that choice is right.
But in Kanban the limit presupposes an unbounded, fully structured backlog column; only "doing"
is capped. RLM's backlog is `parked`, and parked is not a column, it is a freezer: no next
actions (O3), out of every view, and emotionally "I stopped" (O1). The system has a cap on
work-in-progress and no legitimate place for work-not-in-progress, so the backlog lives in the
user's head (O4).

**D2. The operations track is one level shallower than the inquiry track, with no stated
reason.** Inquiry has three levels: evidence, portfolio, question; `docs/design.md` says a
question sits above projects, may spawn several over time, and outlives them. Operations has
action, project, nothing. But operational outcomes also outlive and spawn several bounded
deliverables (a degree, a residency, a work programme, a renovation in phases). The method's
binary, "projects end, stewardships do not", misses the third case: an outcome that ends, but
over years and through several maps. The current answer, milestones inside one project, holds
only while the children share one Current State, one tempo and one set of collaborators. When
they do not, the user splits the map (O2) and the cap punishes them for modelling correctly.

**D3. Status is an overloaded enum.** `active` means, at once: counts, is shown, accepts
writes, and is mine to move. `parked` negates all of it. Each new real situation then wants a
new variant (a state for "agents move it", a state for "waiting on an institution", a state for
"weekends only", a state for "not yet started"). That does not scale, and it muddles two kinds
of fact. *Who holds the ball* is a genuine state: it changes at a moment, the change deserves
its log line, and the review should ask a different question afterwards. *Cadence* is an
attribute. *Counts toward the cap* should be derived from the rest, never stored.

**D4. The cap has no theory of its unit.** "Five projects" assumes projects are commensurable
units of attention. A two-year degree and a weekend of cleaning both satisfy "bounded, with a
deliverable". Kanban limits work because cards are sized; RLM never says what size a project
is, so the cap measures the wrong thing and feels arbitrary in both directions: too tight for
small things (O1, O3), meaningless for huge ones (O4).

### 3.3 What is not broken

The easy fix would throw out the discipline. It should not:

- **The cap is right and should not rise.** Raising it pretends every map needs a full slot of
  attention (O5 is what happens when it is nudged instead of defended).
- **The closed-enum restraint is right.** The method has declined `meeting`, `milestone` and
  `monthly` as types and declined a `person` built-in; the pressure to add a type should stay
  high. D2 argues for one more *distinction*, not for a looser policy.
- **"Projects end, stewardships do not" is right.** D2 adds a third case beside it; it does not
  replace it.
- **Parking as a lifecycle answer is right.** "Interesting but not now" is a real state. What
  is wrong is what parking *removes* (D1), not that it exists.

## 4. Background — what the method and the code say today

For reference, so the diagnosis can be checked against the text rather than against memory.

- **The cap.** `docs/design.md` § project conventions: "maximum 5 active projects across all
  contexts. Parked projects move to `projects/_parked/`. Enforced by the tool — creating a 6th
  active project prompts parking one first." `max_active_projects` in `.cuaderno/config.toml`
  overrides the default. The count is over `status: active` maps, one per map, regardless of
  size, context, or who moves it.
- **Lifecycle.** `status` is one of `active`, `parked`, `completed`, `dropped` (RFC 0004),
  each with one folder. Every transition logs one fixed line. A map whose status disagrees
  with its folder is a lint error.
- **What parking removes.** `crates/cdno-domain/src/vault/projects/actions.rs`: `add_action`,
  `update_project_state`, milestones and waiting-on all refuse on a parked project with
  `ProjectNotActive`; the docstring on the listing verb says "listing queued work on a parked
  project is rarely useful — activate it". A parked project's milestones also leave the
  commitments register (#611), and it is absent from `get_orientation`. `activate` is
  cap-checked.
- **Hierarchy.** Operations: action (bullet or promoted note) → project. Above the project
  there is `core_question`, which links a project to a *question* (inquiry track) and nothing on
  the operations track. `## Links` on a map carries free-form `- Portfolio: [[…]]` and
  `- Project: [[…]] — carved out …` lines, which is how O2's three maps are currently related
  to each other: by prose, invisible to every view.
- **Reviews.** `get_orientation` suggests one project from the active set; the weekly review
  walks the active set for movement; the monthly review has a slot-allocation step that asks
  whether a parked project should return. All three ask one question of every active project.

## 5. Prior art and approaches already considered

Each entry notes which of the four jobs (§3.1) it tried to separate. The pattern is that every
earlier idea separated one job and left the other three stuck.

### 5.1 Raise `max_active_projects`

Cheapest experiment, one line of config. Tried (O5). It changes J2 only, by making it weaker,
and it does nothing for J1, J3 or J4. It also pretends the quiet project needs a full attention
slot, which was the thing being objected to.

### 5.2 Caps per context (`context:` is already on every map)

The first proposal. A per-context budget (so many work, so many side-project, so many
household) within or instead of the total. Rejected because the cap bounds attention and a
per-context budget raises the total in disguise (three plus two plus one is six on the
orientation screen), while most contexts would sit at zero or one. It touches J2 only. The
useful idea inside it is a *ceiling within the total* ("at most three work projects out of
five") that stops one context crowding out the rest; that is a J2 refinement worth keeping in
mind but it frees no slot for O1 and models nothing for O2 or O4.

### 5.3 A rolling cap

Count only projects touched in the last N days. Rejected: unpredictable, which is the wrong
property for an ADHD-facing rule ("will creating this be refused today?" should have a stable
answer). J2 only.

### 5.4 A `rhythm:` field so orientation skips a project on weekdays

Fixes the noise (J3) and nothing else; the slot is still held. It is, however, the first idea
that correctly identified cadence as a *display* concern rather than a lifecycle one, and that
reading is kept in §6.

### 5.5 A third open lifecycle state between `active` and `parked`

Working name `delegated`; the maintainer's design note of 2026-10-14. The map stays fully
usable (J1 kept: state updates log, next actions live on it, focus and `during:` can point at
it) but it does not count toward the cap (J2 off) and the reviews do not ask why it did not
move (J3 changed: they ask "what came back, what needs my decision"). Three transitions, each
logging one line; the same shape of change as RFC 0004's closures. Open at the time: the bound
(what stops "delegate one first" replacing "park one first"), whether a focus started on such a
project should promote it (leaning no, with the weekly review noticing the pattern), and the
name itself (`background`, `simmering`, `maintenance`, `autopilot`, `supervised` considered and
set aside; the chosen axis was *who moves it*, not tempo).

This is the most developed idea and the only one that separates J1 from J2 and J3. It is held
out of the proposal section for two reasons. First, it rests on one specimen (O1), while the
diagnosis rests on six. Second, it reaches for a new enum variant, which is exactly the move D3
warns against; whether "who holds the ball" should be a *state* or should fall out of the cap's
counting rule (D4) plus a review question (O6) is one of the things §5 of this RFC, when
written, has to decide rather than assume.

### 5.6 Milestones as the programme layer

The method's current answer to O2: one map, several milestones. Adequate while the children
share a Current State; breaks when they do not. J4 only, and only one level.

### 5.7 GTD, for comparison

GTD does not cap projects at all; it relies on the weekly review, and it separates Projects,
Next Actions, Waiting For, Someday/Maybe, Areas of Responsibility (≈ stewardships) and higher
horizons (goals, vision). RLM deliberately added the WIP limit on top of this for the ADHD
reason. What RLM did not import alongside it: Waiting For as a first-class list distinct from
"mine" (cf. 5.5), Someday/Maybe distinct from "paused" (cf. O3: both are `parked` today), and a
goals horizon above projects (cf. D2). Not an argument for adopting GTD's lists; an argument
that the WIP limit was imported from one tradition and the backlog structure from neither.

## 6. Principles a repair must satisfy

Stated as principles rather than features, so they can be checked against any vault.

**P1. Existence is free; attention is scarce.** Any work may be fully modelled at any time:
a map, next actions, state, links, children. The cap counts attention and only attention.
Corollary: a map that does not count loses nothing except being counted and nagged. Parking as
it stands violates this, which is why O3 happened.

**P2. Name the unit the cap counts.** A candidate definition: *a map on which I personally hold
the next move*. Once the unit is defined, maps that someone else is moving (an agent, a
colleague, an institution) and maps that have not started fall out of the count by definition,
not by special-casing, and the number can stay at five. The definition is the maintainer's to
choose (§7); whichever it is, it must be stateable in one sentence and give a stable answer
on any given day (cf. 5.3).

**P3. Give operations the depth inquiry already has.** Question : portfolio : evidence should
have a counterpart outcome : project : action. The method needs the *word* before the tooling,
because without it users invent the layer badly (one monster map, or unlinked siblings). Whether
the layer is a built-in, a config-defined custom type (RFC 0002), or a convention on `## Links`
that views learn to read, is a tooling decision downstream of accepting the distinction.

**P4. Status records relationship, not tempo.** Transitions in who holds the ball (me, someone
else, nobody yet) are the things that may deserve a state and a log line. Cadence (weekday,
weekend, quarterly) lives in orientation and review configuration, never in the lifecycle enum.

**P5. Reviews ask the question the relationship implies.** Mine: did it move? Handed off: what
came back, what needs my decision? Not yet started: is it time? One uniform "why did this not
move" is the review-side half of the same flaw (O6).

**P6. Every transition still logs one fixed line.** Whatever §5 adds, it keeps the invariant
that the daily log can reconstruct a project's history. No silent status changes; in
particular, no automatic promotion when a focus starts on a non-counting map (the review
notices the pattern and asks).

**P7. Keep the closed-enum pressure.** One new distinction, justified by six specimens, is the
budget. The repair should not open the door to a variant per situation (D3).

## 7. Decisions for the maintainer, before §5 is written

- **D1. Is the four-jobs reading accepted?** If a reviewer can show one of J1–J4 is not
  actually independent of the others in practice, the rest of the document changes shape.
- **D2. What is the unit the cap counts?** (P2.) "A map on which I hold the next move" is the
  candidate. Alternatives: "a map I touched this week" (rejected in 5.3 for unpredictability),
  "an outcome, with its open fronts" (per-category caps in another hat; see 5.2).
- **D3. Does the programme layer (P3) enter as a built-in, a custom type, or a convention
  first?** The RFC 0002 machinery makes "convention first, built-in if it earns it" cheap; a
  built-in is needed as soon as a view has to group by it.
- **D4. Is "who holds the ball" a state or a derived property?** (D3 in §3.2, 5.5.) A state is
  easier to log and to transition deliberately; a derived property cannot drift and needs no
  new enum variant. This is the decision 5.5 pre-empted.
- **D5. What should parking stop removing?** (P1.) At minimum, structural writes on a parked
  map (O3). Whether a parked map's milestones should return to the register is #611 and is
  separate.
- **D6. Should the diagnostic in the 2026-10-14 note be run before §5 is written?** "Of the
  current active projects, which would stop counting under the P2 definition today?" If one,
  the definition earns its place and the cap stays at five. If three, the cap is simply too
  low and no definition will hide it.

## 8. Verification, in advance

How to tell, a few months after whatever §5 ships, whether the hole is closed:

- The user keeps no list of undertakings outside the vault for want of a slot (O4 → zero).
- `max_active_projects` is back at its default, committed, and the same on every surface (O5).
- No next action lives in a daily note because its map refused it (O3).
- A multi-map programme appears as one line with its fronts on the orientation screen (O2).
- The weekly review asks at least two different questions of at least two different maps (O6).
