# RFC 0006 — Attention and structure: handing off, outcomes, and what the project cap counts

| | |
|---|---|
| **Status** | Draft, amended after review — 2026-10-19 (four-seat review, three rounds, on PR #779: method theorist; the method's user; domain architecture and data model; surfaces, safety, tests and maintenance. All seats converged on the mechanism; §10 records what the maintainer settled and what remains). The proposal in §§7–8 was written after the review, from the amended diagnosis. |
| **Tracked by** | PR #779 |
| **Affects** | The published method (two new practices, one clarification, a versioning rule); `docs/design.md`; `cdno-core` (config: method version); `cdno-domain` (project lifecycle, the cap, the register, orientation, review contexts, a bundled note type, one migration framework); `cdno-cli` (`cdno project`, `cdno orient`, `cdno migrate`); `cdno-mcp` (three tools, DTO fields, tool descriptions); `examples/skills/` (and the review skills that live outside this repository); `docs-site` |
| **Related** | RFC 0004 §9 Q1 (the parked-map write rule this RFC reverses), #611 (parked milestones and the register), RFC 0002 and RFC 0003 (custom and bundled note types, the route the outcome type takes), RFC 0005 (focus; `during:`), `max_active_projects` in `.cuaderno/config.toml` |

> **Amended after review.** The first draft was diagnosis only, by design, and was reviewed on
> PR #779 by four seats over three rounds, each assessing both the method's mechanisms in the
> abstract and their cost in cdno. The review corrected the diagnosis in five places (creation at
> the cap is never refused; a fifth job, the commitments register; the parked freeze is a decision
> RFC 0004 recorded; the four jobs are two variables, one invariant and one missing type; several
> §4 facts) and converged on a mechanism. The maintainer then redirected the proposal: the
> solution is stated first at the level of the method, so that someone running the Research
> Logbook Method on paper or in any tool can follow it, and cdno implements that text. §§1–6 are
> the amended diagnosis; §7 is the method; §8 is cdno; §9 the staging; §10 the decisions; §11 the
> verification. The record is on the PR.

> **Authorship.** Drafted by Claude (Anthropic) from design conversations with the maintainer in
> October 2026 and from the four-seat review. The observations in §2 are invented specimens that
> keep only the shape of what was seen in one vault; the diagnosis in §3, the survey in §5, the
> principles in §6 and the costing in §8 are Claude's and the seats', amended by the review. The
> method text in §7, the choice of "outcome" as the name, the decision that handing off is a
> practice in its own right, and the versioning rule are the maintainer's.

---

## 1. Summary

The Research Logbook Method caps active projects (default five, `max_active_projects`) to bound
attention, and the cap is right. But in practice the cap is felt as too tight, people raise it
(one vault went from 5 to 6 as a stopgap), and, worse, they stop writing some work down at all.
A method whose premise is externalisation has produced an incentive not to externalise. That
cannot be fixed by tuning a number.

The diagnosis: the active project map is one primitive doing several jobs that need to vary
independently (where structure lives; the unit the cap counts; what the reviews ask about;
whether its dates are live promises), the operations track has no node above the project, and
the method offers exactly one lever, parking, that moves every job at once. Creation at the cap
is never refused; the damage is done afterwards, by a parked map that accepts no writes, leaves
every view, and drops its deadlines from the register.

The repair, at the level of the method (§7): a parked map is a complete map; a project may be
**handed off**, with the holder's name on its map, and the cap counts the maps that are yours;
and an **outcome** is a new page above projects, bounded and long, reached through a sequence of
projects. The seven practices as published are RLM 1.0; this is RLM 1.1, additive, with a
versioning rule so later changes that alter meaning ship with a migration. cdno then (§8) stops
refusing writes on parked maps, adds a `held_by` field with one logging verb and a derived
"counts" flag, ships `outcome` as a bundled note type with an `outcome:` link on the project
map, exposes holder and counts to the reviews, and records the method version in the vault.
No migration is needed for any of it.

## 2. Observations

These are the specimens. **They are invented.** Each keeps only the shape of a situation seen in
one vault over four months (bounded or long-horizon; mine to move or someone else's; started or
not) and none of its particulars, so nothing below describes any real undertaking. The claim is
that the shapes are typical of anyone running the method over a mixed life. Each is stated as
what the user wanted and what the method made them do instead.

**O1. A quiet project that still needs its map.** A hobby codebase gets little of the user's
own time on weekdays. Agents move it forward; the user reviews, decides and steers at weekends.
It needs a project map: its Current State is updated, next actions live on it, `during:` lines
and focus point at it. But holding a full attention slot for it is wrong, and parking it is
worse: parking reads as "I stopped", drops the map out of every view, and bringing it back
needs a free slot. The user kept it active and complained about the cap.

**O2. One programme, several maps.** A tabletop game being brought to print split, naturally
and correctly, into three project maps: the rulebook, the artwork commission, and the print
run. They have different Current States, different tempos and different collaborators; one map
with milestones would have been a worse model. The three maps consume three of five slots, with
no relation between them that any view can see, and the orientation screen lists them as three
unrelated projects.

**O3. Work created parked, then refused.** A project that genuinely had not started yet (a
choir season that begins next spring) was created parked because the slots were full. The user
then wanted to pre-stage its first next actions. `add_action` refuses on a parked project
(`ProjectNotActive`), so the actions were written into a daily note instead, where no view will
find them. Parking was the right lifecycle answer and the wrong structural one.

**O4. Work never written down.** The user holds, outside the vault, a list of six undertakings
they have not created: restoring a wooden dinghy (which would split into the hull, the rigging,
and registration with the harbour); a family recipe book, with a photograph archive alongside it
that would spawn children of its own; a possible move of a sports club's website to a new host,
which if it starts would spawn several projects; a garden pond; learning the cello; a planning
application for a shed, which is mostly waiting on the council. These are the clearest
evidence: the method is pushing real work out of the system.

*Corrected after review.* The first draft said this happened "because there is no slot". The
code says otherwise: `create_project` never refuses at the cap; it creates the map parked
(`crates/cdno-domain/src/vault/projects/lifecycle.rs:124–130`; the cap is checked only on
`activate`). What stops the user is O3 (the parked map accepts no structure), the absence of a
parent for the ones that split (O2), and the connotation of "parked" (O1). O4 is the other three
seen from outside the vault, and it remains the most important specimen because it shows the
incentive not to externalise.

**O5. The cap drifted and nobody noticed** (corroboration). The vault's config was raised from 5
to 6 as an experiment and left uncommitted; the remote MCP server stayed at 5; the vault had six
active projects. Two surfaces disagreed about the rule for two weeks. A number that is easy to
nudge and hard to defend gets nudged. The deployment half of this is that config is read once,
at start-up (`crates/cdno-cli/src/bootstrap.rs:80`).

**O6. Cadence mismatch in the reviews** (corroboration; O1 from the review's side). The daily
orientation and the weekly review ask the same question of every active project ("did it move,
why not"). A weekend project, a weekday-only one and a planning application that moves once a
quarter all get the same daily pressure. The user's response was to want to hide some of them.

**O7. The moment of creation at the cap** (added by the user's seat). The create-project skill
creates the map parked, offers to swap it with an active one, and then offers to add its first
action. `add_action`'s description never says the project must be active; the agent calls it;
the single domain guard `resolve_active_project` (`crates/cdno-domain/src/vault/projects/mod.rs:220`)
refuses; the rejection offers only "activate first", which loops back to the swap. The skill's
own text, "don't fight the cap, that's the method working", teaches the user to stop asking.
This is the common path, not an edge case: the cap sentence in the MCP server instructions
begins past the point where Claude Code truncates them (measured by the surfaces seat at
character 2,073 of a 2,048 cut), so an agent's whole briefing on projects is the tool
descriptions.

## 3. Diagnosis

### 3.1 One primitive, several jobs

An active project map currently does all of the following, and the method has no way to grant
one without the others:

| Job | What it means today |
|---|---|
| **J1. Where structure lives** | Current State, Next Actions, Waiting On, Milestones, Links; the target of `during:` and of focus; what `get_project_context` reads |
| **J2. The unit the cap counts** | `status: active` is what `max_active_projects` counts |
| **J3. What the reviews ask about, and which question** | the active set is what the skills walk; every map gets "did it move?" |
| **J5. Its dated milestones are live promises** | the commitments register reads sources 1 and 4 only from active maps (`crates/cdno-domain/src/vault/commitments.rs:708, 822`) |

*Amended after review.* The first draft listed a fourth job, "the node in the hierarchy".
The theorist's seat showed that is not a job the map does but a level the taxonomy lacks; it
is restated as D2's premise below. J5 was missed and is added: it is the reason parking a map
silently removes a deadline. J3 splits into *when* a question is asked (cadence) and *which*
question; the second co-varies with J2, since both depend on who is moving the map. The honest
structure is two independent variables (who holds it; cadence), one invariant (structure is
always available), and one missing level (the parent).

Mapping the observations onto the jobs:

| Observation | Wanted | Got |
|---|---|---|
| O1 quiet project | J1 without J2 and J3 | all or none |
| O2 programme in three maps | a level above the map | three unrelated J2 units |
| O3 parked, actions refused | J1 without J2 | parking removes J1 and J5 too |
| O4, O7 work not written down | J1 and a parent, without J2 | O3 at the moment of creation |
| O6 cadence | J3 to vary per map | J3 uniform across the active set |

Parking is the one lever, and it switches J1, J2, J3 and J5 off together. Every "the cap is too
low" complaint above is really "I need one of these jobs without the others". In the code the
coupling is one predicate used twice: `status == Active` is both the write gate
(`resolve_active_project`) and the count gate (`active_projects()`).

### 3.2 Four defects, stated generally

**D1. A WIP limit was imported without its backlog.** The cap is a Kanban work-in-progress
limit, chosen for the ADHD reason (forced prioritisation, one screen), and that choice is right.
But in Kanban the limit presupposes a fully structured backlog column; only "doing" is capped.
RLM's backlog has a *place*, `_parked/`, groomed at the monthly review's slot-allocation step,
which is Kanban's backlog grooming. What it lacks is *structure*: a parked map accepts no next
actions (O3), leaves every view, drops its deadlines (J5), and reads as "I stopped" (O1). The
place is a freezer, so the backlog lives in the user's head (O4).

**D2. The operations track has no bounded node above the project.** Inquiry has three levels:
evidence, portfolio, question; `docs/design.md` says a question sits above projects, may spawn
several over time, and outlives them. Operations has action, project, nothing. But operational
undertakings also outlive and spawn several bounded deliverables (a boat restoration, a book, a
website migration, a renovation in phases). The method's binary, "projects end, stewardships do
not", misses the third case: something that ends, but over years and through several maps. The
current answer, milestones inside one map, holds only while the children share one Current
State, one tempo and one set of collaborators; when they do not, the user splits the map (O2)
and the cap punishes them for modelling correctly. *Narrowed after review:* for an
inquiry-driven programme the parent exists already, the question, linked by `core_question`
and computed by `question_backlinks` (`crates/cdno-domain/src/vault/context.rs:617`); that no
view groups by it is a view defect. The missing level is for bounded outcomes with no question
above them, and a programme with a question above it needs no second parent.

**D3. The write gate and the count gate are the same predicate.** `active` means, at once:
counts, accepts writes, is shown, has live dates, and is mine to move. `parked` negates all of
it. Each new real situation then wants a new status (a state for "agents move it", a state for
"waiting on a planning office", a state for "weekends only", a state for "not yet started").
That does not scale, and it muddles kinds of fact that vary independently. *Who holds the map*
changes at a moment, deserves its log line, and changes the review's question. *Cadence* is an
attribute. *Counts toward the cap* should be derived from the rest, never stored. Questions have
the same four-value status and nobody complains, because nothing counts questions and nothing
refuses writes on a parked one; the overload is the shared predicate, not the enum's arity.

**D4. The cap half-names its unit.** "Five projects" assumes projects are commensurable units of
attention. The method already sizes the *front* ("three next actions per project is a ceiling,
not a backlog"), so a map is one unit of attention because its open front is bounded, however
large the deliverable behind it. What it does not say is whose attention: a map whose front
someone else is working is not a unit of *my* attention, and the cap counts it anyway (O1).
That is the only part of D4 that needs new mechanism.

### 3.3 What is not broken

The easy fix would throw out the discipline. It should not:

- **The cap is right and should not rise.** Raising it pretends every map needs a full slot of
  attention (O5 is what happens when it is nudged instead of defended).
- **The closed-enum restraint is right.** The method has declined `meeting`, `milestone` and
  `monthly` as types and declined a `person` built-in; the pressure to add a type should stay
  high. This RFC adds two distinctions, each stated as a practice of the method and each
  justified by its own specimens; it does not loosen the policy.
- **"Projects end, stewardships do not" is right.** D2 adds a third case beside it; it does not
  replace it.
- **Parking as a lifecycle answer is right.** "Interesting but not now" is a real state. What
  is wrong is what parking *removes* (D1), not that it exists.
- **Kanban's "blocked cards still count" is deliberately rejected.** Kanban counts a blocked
  card so that blockage hurts and gets unblocked. For an ADHD user, nagging about what you
  cannot move is a guilt engine (`docs/design.md`, design principles). The method keeps the
  nag off and asks a different question instead (§7.2).

## 4. Background — what the method and the code say today

For reference, so the diagnosis can be checked against the text rather than against memory.
Corrected after review where marked.

- **The cap.** `docs/design.md` § project conventions says: "maximum 5 active projects across
  all contexts. Parked projects move to `projects/_parked/`. Enforced by the tool — creating a
  6th active project prompts parking one first." *The last sentence is wrong against the code*:
  `create_project` creates the map parked at the cap and never returns `ProjectCapReached`;
  only `activate` is cap-checked (`lifecycle.rs:124–130, 266`). The docs-site business-rules
  page repeats the error. `max_active_projects` in `.cuaderno/config.toml` overrides the
  default. The count is over `status: active` maps, one per map, regardless of size, context,
  or who moves it.
- **Lifecycle.** `status` is one of `active`, `parked`, `completed`, `dropped` (RFC 0004), each
  with one folder. Every transition logs one fixed line. A map whose status disagrees with its
  folder is a lint error.
- **What parking removes, and that it was decided.** `resolve_active_project`
  (`projects/mod.rs:220`) is the single guard; `add_action`, `complete_action`, `drop_action`,
  `promote_action`, `list_actions`, the milestone and waiting-on verbs, `update_project_state`
  and `set_core_question` all go through it and refuse a parked map with `ProjectNotActive`.
  This is not an accident of the code: RFC 0004 §9 Q1 recorded the rule "verbs that change a
  project's plan need it active; verbs that end it accept active or parked". This RFC reverses
  Q1 and says so (§7.1). A parked map's hard milestones also leave the commitments register
  (`commitments.rs:708, 822`; #611, closed by RFC 0004), and the map is absent from orientation.
  `activate` is cap-checked.
- **Hierarchy.** Operations: action (bullet or promoted note) → project. Above the project
  there is `core_question`, linking to a question on the inquiry track, and nothing on the
  operations track. `## Links` on a map carries free-form `- Portfolio: [[…]]` and
  `- Project: [[…]] — carved out …` lines; these wikilinks *are* indexed, as untyped edges, but
  no view reads them as a parent relation, and no MCP tool can write that section.
- **Reviews.** `get_orientation` returns the active set and suggests nothing by itself; the
  daily-orientation skill and `cdno orient` pick one. The weekly and monthly contexts are
  composed in `cdno-mcp`, not in the domain. Of the review skills, only `daily-orientation` and
  `quick-capture` live in `examples/skills/`; `create-project`, `weekly-review` and
  `weekly-planning` live outside this repository, and there is no monthly-review skill, only
  `get_monthly_context`. All of them ask one question of every active project.
- **The agent's briefing.** The MCP server instructions are the agent's whole briefing on the
  method, and Claude Code truncates them at 2,048 characters; the cap sentence sits past that
  point. In practice an agent's briefing on projects is the tool descriptions.

## 5. Prior art and approaches already considered

Each entry notes which of the jobs (§3.1) it tried to separate. The pattern is that every
earlier idea separated one job and left the others stuck.

### 5.1 Raise `max_active_projects`

Cheapest experiment, one line of config. Tried (O5). It changes J2 only, by making it weaker,
and it does nothing for J1, J3 or J5. It also pretends the quiet project needs a full attention
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
property for an ADHD-facing rule ("will this count today?" should have a stable answer). J2
only.

### 5.4 A `rhythm:` field so orientation skips a project on weekdays

Fixes the noise (J3) and nothing else; the slot is still held. It is, however, the first idea
that correctly identified cadence as a *display* concern rather than a lifecycle one. Deferred
(§6, P4): §7.2 dissolves most cadence cases, and §11 will show whether a residue remains.

### 5.5 A third open lifecycle state between `active` and `parked`

Working name `delegated`; an earlier design note. The map stays fully usable (J1 kept) but it
does not count toward the cap (J2 off) and the reviews ask "what came back" rather than "why
did it not move" (J3 changed). Three transitions, each logging one line; the same shape of
change as RFC 0004's closures. Open at the time: the bound (what stops "delegate one first"
replacing "park one first"), whether a focus started on such a project should promote it, and
the name (`background`, `simmering`, `maintenance`, `autopilot`, `supervised` considered and
set aside; the chosen axis was *who moves it*, not tempo).

This was the most developed idea and the only one that separated J1 from J2 and J3. The review
confirmed its axis and rejected its form: a new `ProjectStatus` variant breaks seven exhaustive
matches loudly and seven `!= Active` sites silently (the register among them), needs a folder,
a lint row and a locator candidate by RFC 0004's precedent, and an older binary that meets one
map with an unknown status loses orientation, listing, creation and activation outright, since
`active_projects()` fails closed. The axis survives as §7.2; the form becomes a field (§8.2).

### 5.6 Milestones as the programme layer

The method's current answer to O2: one map, several milestones. Adequate while the children
share a Current State; breaks when they do not.

### 5.7 A stewardship that carries next actions (added by the theorist's seat)

The method already has a type for "a thing I maintain indefinitely and steer at weekends": a
stewardship, uncapped and never asked "did it move". O1's codebase is arguably one, with the
bounded undertakings on it as projects. The gap is that a stewardship has no Next Actions and
cannot be the target of focus. Letting it carry them would fix O1's structural half without
touching the project lifecycle. Rejected, by the user's seat: a stewardship with next actions
and a focus is a project in all but name, and on a bad day the user would have to decide which
of two next-action-bearing types a thing is before writing it down. One place for next actions
is a rule a person can hold; two is not.

### 5.8 Grouping by `core_question` (added by the theorist's seat)

For a programme with a question above it, the parent exists and is invisible. Grouping the
orientation screen by `core_question` fixes O2's display for that case with no new type, and
it is the same mechanism the outcome needs. Adopted as the rehearsal for §8.3: it lands first,
and the outcome link reuses its shape.

### 5.9 GTD, for comparison

GTD does not cap projects at all; it relies on the weekly review, and it separates Projects,
Next Actions, Waiting For, Someday/Maybe, Areas of Responsibility (≈ stewardships) and higher
horizons (goals, vision). RLM deliberately added the WIP limit on top of this for the ADHD
reason. What RLM did not import alongside it: Waiting For as something distinct from "mine"
at the level of a whole project (cf. §7.2), Someday/Maybe distinct from "paused" (cf. O3: both
are `parked` today, and §7.1 makes that fine), and a goals horizon above projects (cf. §7.3).
Not an argument for adopting GTD's lists; an argument that the WIP limit was imported from one
tradition and the backlog structure from neither.

## 6. Principles a repair must satisfy

Stated as principles rather than features, so they can be checked against any vault, and
amended where the review found them wanting.

**P1. Existence is free; attention is scarce.** Any work may be fully modelled at any time:
a map, next actions, state, links, a parent. The cap counts attention and only attention. A map
that does not count loses nothing except being counted and nagged. *Bound, added after
review:* what does not count is reviewed by title at the monthly review only, never daily or
weekly; otherwise a hundred structured parked maps become the next guilt engine.

**P2. Name the unit the cap counts, in one sentence that is stable from day to day.** The
sentence (settled, §10): *a map counts unless I have written down who else is moving it.* It
names the stored fact the tool checks, so an agent cannot misjudge it, and it absorbs both "I
hold the next move" and "I execute the next actions" without asking the person to adjudicate
between them on a bad day.

**P3. Give operations the bounded parent inquiry already has.** The method needs the *word*
before the tooling, because without it users invent the layer badly (one monster map, or
unlinked siblings). The parent is modelled on the question: an uncapped reference page, linked
*from* the child, children computed, reviewed monthly, never a folder that contains its
projects.

**P4. Status records relationship, not tempo.** Who holds the map is the thing that may deserve
a declaration and a log line. Cadence is not stored in this cut: §7.2 dissolves the common
cases (the quarterly bureaucratic project is someone else's between bursts; the agent-moved
codebase is someone else's on weekdays), and the pure residue, a map I execute myself but only
at weekends, is left to §11 to show whether it is real.

**P5. Reviews ask the question the relationship implies.** Mine: did it move? Handed off: what
came back, what needs my decision? Parked: is it time? (monthly only). Under an outcome: which
front moved, what is next?

**P6. Every transition still logs one fixed line.** The daily log can reconstruct a project's
history. No silent changes of holder or parent; in particular, starting a focus on a map never
changes who holds it (the weekly review notices the pattern and asks).

**P7. Keep the closed-enum pressure.** Two new distinctions, each a practice of the method with
its own specimens and its own page or line, is the budget. Neither is a lifecycle variant.

## 7. The method: Research Logbook Method 1.1

This section is written to be read without the tool. It is the text proposed for the published
method and for `docs/design.md`. The seven practices as published are RLM 1.0; the three
changes below are additive, so every 1.0 vault is a valid 1.1 vault.

### 7.1 Parking, clarified

A parked project map is a complete map. Parking changes two things only: the project does not
count against the five, and it is read at the monthly review rather than daily. Everything else
a map does, it keeps. You may write next actions on a parked map, update its state, note a
milestone, note who you are waiting on. A hard-dated milestone on a parked map is still a
promise and stays in the commitments register. The one thing a parked map does not take is
your focus: being in the middle of something is the strongest sign you are moving it yourself,
so a map you start work on is a map that counts.

This reverses RFC 0004 §9 Q1, which froze parked maps to stop stale deadlines nagging. The nag
problem is solved the other way round: a parked map's dates are live, and a stale one is
dropped, with its log line, rather than silenced by the lifecycle.

### 7.2 Practice 8: handing off

A project map already answers: where was I, what do I believe and what would change my mind,
what are my next actions, what is time-sensitive, where is everything. It gains one more
question: **whose move is it?**

A project you have handed to someone else carries their name at the top of its map:
`held by: the harbour office`, `held by: a colleague`, `held by: an agent`. A map with no name
on it is yours. Two rules follow.

- **The cap counts the maps that are yours.** *A project counts unless the map says who else is
  moving it.* A handed-off project is still open: it keeps its map, keeps logging, keeps its
  place in views and its dates in the register. It is not a sixth slot; it is a project that is
  not a unit of your attention this week.
- **The reviews ask a handed-off project a different question.** Yours: did it move? Handed
  off: what came back, and what is waiting on your decision? That is the whole difference
  between a thing you are driving and a thing you are supervising.

Handing off and taking back are entries in the chronological log like any other change to a
map: `handed [[boat-registration]] to the harbour office`; `took back [[boat-registration]]`.
Taking back is subject to the cap exactly as activating a parked project is.

Two guards. If you cannot name who has it, it is yours. And handing off is never the answer to
"the cap is full": a project you are about to work on yourself is yours whatever its map says,
and the weekly review will notice a map you keep starting work on that claims someone else
holds it.

The map already has a Waiting On section for single items. Handing off is Waiting On lifted to
the whole project, which is why the method already half-had it. Its ancestry is GTD's Waiting
For, kept per project rather than as a separate list.

### 7.3 Practice 9: outcomes

A new kind of page, beside questions, above projects. A **question** is open-ended and may turn
out to be the wrong question; a **stewardship** never ends; an **outcome** is bounded and long:
a result you will reach through a sequence of projects, each of which ends, and which is
reached when the last of them does. Restoring a boat is an outcome; the hull, the rigging and
the registration are its projects, one or two at a time. Knuth's multi-volume work is the
exemplar in the spirit of the other seven practices: the volumes are projects; the work is the
outcome.

An outcome page is one sheet: what done looks like; the projects it has had and has; the one
that comes next. A project writes the outcome it belongs to at the top of its map, the way it
already writes its core question; a project can exist without an outcome, as it can without a
question, and a project with a question above it needs no outcome as well.

Outcomes hold no slot and have no cap, like questions. They are read at the monthly review:
which front moved, what is the next project under it, and is this still wanted? The daily
orientation shows a project under its outcome, so three maps of one undertaking read as one
line with three fronts rather than as three unrelated things.

What the method does *not* do: it does not count outcomes, does not limit fronts per outcome,
and does not make an outcome a folder that contains its projects. The parent is a link from the
child, as with questions, so the projects stay where they are.

### 7.4 Versioning the method

The seven practices as published are **RLM 1.0**. A change that adds a page type or an optional
line on an existing page, and changes no meaning of an existing page, is a **minor** version;
this RFC is **1.1**, since every 1.0 vault is a valid 1.1 vault with no outcomes and no
hand-offs. A change that alters what an existing page means, moves things, or makes a line
required is a **major** version and ships with a migration. A vault records the method version
it follows, and the published method keeps a changelog of practices, so a vault and the method
can be read against each other.

## 8. What cdno does

Each mechanism is justified by a sentence of §7 and costed from the review. Sizes: S one PR; M
an implementation plan of 5–10 tasks; L RFC 0004 scale (~20 tasks).

### 8.0 Groundwork (no behaviour change)

- Correct `docs/design.md` § project conventions and the docs-site business-rules page: creation
  at the cap creates a parked map; the cap is checked on activation.
- Factor the cap count into one `slot_usage()` in `cdno-domain` and hoist the weekly and monthly
  context composition from `cdno-mcp` into the domain, so the fields §8.5 adds have one home.
- `slots` on `get_orientation`, matching `list_projects`; the create-project skill stops
  hard-coding "5".
- Move the cap rule out of the server instructions and into the descriptions of
  `create_project`, `park_project`, `activate_project` and `add_action`, since the instructions
  are cut before it.
- Bring `weekly-review` into `examples/skills/` so §8.5 and §11 can be tested here.

**S.** Lands first, before anything below.

### 8.1 Parked maps accept writes (§7.1)

- A new `resolve_open_project` in `crates/cdno-domain/src/vault/projects/mod.rs` accepting an
  active map at `projects/` or a parked map at `_parked/` (folder and status agreeing, as
  `resolve_closable_project` already does). Callers switched: `add_action`, `complete_action`,
  `drop_action`, `promote_action`, `list_actions`, the milestone and waiting-on verbs,
  `update_project_state`, `set_core_question`. **Not** switched: `start_action`,
  `switch_action`, `start_unplanned_action` and their `switch_` forms, which keep
  `resolve_active_project`; `pause` and `resume` resolve nothing against the map today.
- The register (`commitments.rs:708, 822`) reads sources 1 and 4 from active *and parked* maps;
  closed maps stay out. `add_milestone`'s write result says whether the date is live.
- `ProjectNotActive` keeps its name; its message and the MCP `project_not_active` rejection are
  reworded to name the three focus verbs as the only ones that need an active map. The CLI
  project pickers list parked maps for the switched verbs.
- RFC 0004 §9 Q1 is amended in place with one sentence saying why the stale-nag problem is now
  solved by live dates plus `drop_milestone` rather than by the freeze. #611 is cited.
- No migration, no folder, no enum change, no new log line. About ten domain tests that assert
  `ProjectNotActive` on a parked map flip to positive tests; one positive test per switched
  verb; `start_action` on a parked map still refused; a parked hard milestone appears in
  `commitments()`.

**S–M.**

### 8.2 Handing off (§7.2)

- **Field.** `held_by: <name>` on the project map, optional; absent means the maintainer.
  Parsed into `ProjectFrontmatter` as `Option<String>`; an empty string is refused. The key is
  reserved from `set_frontmatter` through `is_reserved_key`
  (`crates/cdno-domain/src/vault/set_frontmatter.rs:240`) only, as `closed` is, and **never**
  through `hard_reserved_fields`: `validate_reserved_schema_fields`
  (`crates/cdno-domain/src/type_registry.rs:253–264`) refuses to open a vault whose config
  declares a hard-reserved key, so a vault that ran the zero-code trial (§10, D6) would fail at
  `Vault::new`. A probe test pins this: a vault carrying `[schemas.project.fields.held_by]` opens
  on the shipped binary and `set_frontmatter` on the key returns `reserved_schema_field`.
- **Verb.** One operation, new file `crates/cdno-domain/src/vault/projects/holder.rs`:
  `hand_off(slug, holder)` and `take_back(slug)` as one verb with an optional holder (omitted
  means take back). Take-back is cap-checked like `activate`; hand-off is not. The verb refuses
  the maintainer's own sentinels (`me`, `I`, `myself`, any case) with a message saying to omit
  the holder to take it back, so an agent never writes `held_by: me` and silently frees a slot.
  Log line, in the existing mutable-field family: `holder on [[slug]]` with indented `was:` /
  `now:` (`was: —` for the maintainer). No lint prefix change: `FOCUS_MARKER_PREFIXES`
  (`lint.rs:619`) is the focus family only.
- **Count.** `counts_toward_cap(&self) -> bool` on the project summary: `status == Active &&
  held_by.is_none()`. `slot_usage()` counts it; `create_project`, `activate`, take-back, and the
  stuck-project scan in `context.rs` use it. `active_projects()` keeps its meaning (open and
  shown). Starting a focus never changes `held_by` (P6); the weekly context exposes "focus
  started on a handed-off map N times this week" so the skill can ask.
- **Surfaces.** MCP: one tool `hand_off_project` (holder optional), `held_by` (nullable) and
  `counts` on every project summary DTO, `slots` reporting `counted` and `cap`; rejection
  `project_cap_reached` on take-back; descriptions of `create_project`, `activate_project`,
  `park_project`, `list_projects` and `get_orientation` rewritten, and the catalogue pin +1.
  Agents branch on `counts` and never interpret the name. The `project_cap_reached` rejection
  and every description never name handing off as a remedy. CLI: `cdno project hand-off --slug
  … [--to …]` under flags-and-prompts; `list` and `show` print the holder. Lint: a Warning when
  counted maps exceed the cap.
- **Compatibility.** No migration: `active_projects()` already parses every map's frontmatter.
  A map with an unknown optional key still parses on an older binary and is counted as before
  (pinned by a version-skew test). Rollback is deleting the field.

**M** (6–8 tasks).

### 8.3 Outcomes (§7.3)

- **Rehearsal first (5.8).** `orientation_context` groups active projects by `core_question`
  where one is set; the daily-orientation skill renders the group as one line with its fronts.
  1–2 tasks, lands with 8.0.
- **Type.** `outcome` ships as a bundled custom type (RFC 0003 route; `BUNDLED_NOTE_TYPE_NAMES`
  gains one name), folder `outcomes/`, template with `## What done looks like`, `## Projects`
  (rendered from backlinks), `## Next`. Mutable, refined in place like a concept; `revise_note`
  applies. No lifecycle in this cut: an outcome is retired by hand, and if it earns a lifecycle
  it is promoted to a built-in in a later RFC with the RFC 0004 closure shape.
- **Link.** `outcome: <slug>` on the project map, mirroring `core_question`, set by a dedicated
  verb modelled on `set_core_question` (`projects/core_question.rs`) that logs
  `outcome on [[slug]]` with `was:`/`now:` and inserts the key when absent. Not the generic
  setter: whether an agent could write it would depend on each vault's `settable`, strict-exists
  fails on maps that predate the key, and the log line would fall outside the fixed family.
  Children by a backlink query shaped like `question_backlinks`. `create_project --outcome`.
- **Views.** `orientation_context` groups by `outcome` as it does by `core_question`; the
  monthly context lists outcomes with their open and closed fronts; `get_project_context`
  carries the outcome. One MCP tool (`set_project_outcome`), one CLI subcommand, catalogue pin
  +1.
- No migration, no `paths.rs` constant beyond the bundled name, no exhaustive match.

**S–M** (about 7 tasks). Ships as its own RFC, 0007, so that 0006 is not blocked on it (§9).

### 8.4 Method version and migrations (§7.4)

- `method_version = "1.1"` in `.cuaderno/config.toml`, written by `cdno init`; a vault without
  the key is read as 1.0. `cdno` warns, once per start, on a vault whose version is newer than
  it understands, and `cdno lint` reports it.
- `cdno migrate`: an ordered, append-only list of file migrations in `cdno-domain`, each
  idempotent, each logging one line (`vault migrated 1.0 → 1.1`) to the daily log, with
  `--dry-run`. The list is empty for 1.1; the mechanism exists before it is needed, as the
  index migrations do. A migration runs inside `VaultTransaction` and under the write lock.

**S** (2–3 tasks), lands with 8.0.

### 8.5 Reviews (§7.1–7.3, P5)

The weekly and monthly contexts expose, per project: `held_by`, `counts`, `outcome`, and the
focus-on-handed-off count. The skills branch on them: mine, "did it move?"; handed off, "what
came back, what needs your decision?" (only when the map's state or Waiting On changed since
last week); parked, by title, monthly, "is it time?"; outcome, monthly, "which front moved,
what is next, still wanted?". Domain: fields only. Skills: `daily-orientation` and
`weekly-review` in this repo; the maintainer's other skills follow.

**S–M**, dependent on 8.2 and 8.3.

## 9. Staging and split

Two RFCs, as all four seats recommended, so the cap repair is not blocked on the outcome type:

- **RFC 0006** (this one): 8.0 groundwork, 8.1 parked writes with the register, 8.2 handing off,
  8.4 method version, 8.5 review fields, and the `core_question` grouping rehearsal from 8.3.
  Order: 8.0 and 8.4 → 8.1 → 8.2 → 8.5. About 12 tasks in domain and core, 16–18 with surfaces.
- **RFC 0007**: 8.3 outcomes, with the method text of §7.3 as its §1. About 7 tasks.

Together roughly RFC 0004's size. Nothing needs an index migration or a file migration.

## 10. Decisions

Settled by the review and the maintainer:

- **The reading of the problem** (§3): accepted, in the restated form (two variables, one
  invariant, one missing level, plus the register).
- **The unit the cap counts**: *a map counts unless I have written down who else is moving it.*
- **Who holds it**: declared, not derived (derivation from Next Actions and Waiting On fails on
  the mixed case and on the agent case); stored as a field orthogonal to `status`, never a
  lifecycle variant; two values, with "not yet started" being parked-with-writes.
- **The parent**: a practice of the method, named **outcome**; in cdno a bundled custom type
  and a typed link on the child, not a `## Links` convention and not a built-in yet.
- **Handing off is a practice in its own right**, not a clause under project maps, because it
  is the one that changes what the cap means.
- **Parking keeps every plan-changing write**; focus starts stay refused; parked hard dates
  return to the register in the same change; RFC 0004 Q1 is amended in writing.
- **Field names**: `held_by` and `outcome`.
- **Versioning**: RLM 1.0 is the seven practices; this is 1.1; minor is additive, major ships a
  migration; the vault records its version.
- **The diagnostic**: run by hand on the current active set, under the settled sentence, before
  implementation starts, and again after 8.1 ships; never a `cdno` command. The RFC's own reading
  of the result stands: if one map stops counting, the practice earns its place; if three, the
  cap is too low and no practice would hide it.

Open, for the implementation plan rather than this RFC:

- Whether `list_actions` on a parked map is switched with the other verbs (the seats say yes).
- The exact wording of the review questions in the skills, and the threshold for "changed since
  last week" on a handed-off map.
- Whether the `core_question` grouping rehearsal ships in 0006 or waits for 0007.

## 11. Verification, in advance

Observable from the tool where the surfaces seat could make it so, by a human otherwise:

- The user keeps no list of undertakings outside the vault for want of a slot (O4 → zero; human).
- `max_active_projects` is at its default, committed, and the same on every surface; `cdno lint`
  warns when counted maps exceed it (O5).
- No next action lives in a daily note because its map refused it; one positive test per
  plan-changing verb on a parked map, and `start_action` on one still refused (O3, O7).
- A multi-map undertaking appears as one line with its fronts on the orientation screen; pinned
  on `orientation_context` for `core_question` in 0006 and `outcome` in 0007 (O2).
- The weekly review asks at least two different questions of at least two different maps;
  checkable once `weekly-review` is in `examples/skills/`, a human check until then (O6).
- A vault carrying the trial `held_by` schema block opens on the shipped binary; a map with an
  unknown optional key still parses and counts on an older binary (compatibility).
- Six months on, the cadence residue (P4): if the maintainer has wanted a `rhythm:` line on any
  map, 5.4 returns as a minor version; if not, it stays out.
