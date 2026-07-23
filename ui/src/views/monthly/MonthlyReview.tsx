// Monthly review (M9, plan §1.5; #57; made a ritual in #450) — behind
// `/monthly`.
//
// It was a dashboard wearing a review's name: five sections you looked
// at and closed, with no write and nothing asked. The monthly cadence is
// where RLM asks its highest-altitude question — "am I still pointed at
// the right questions?" — and a surface with no artefact has no reason
// to be visited on a schedule, so the cadence had no home in the app.
//
// Now it is stepped like the weekly review, and it leaves an artefact:
// Wins, Themes and Next Month's Focus land in the monthly note. The
// panels are unchanged reads, walked one at a time and stoppable
// partway.
//
// Colour laws hold throughout: context hues are identity, staleness is
// neutral ink emphasis (never a hue), and there is no red anywhere — an
// over-cap activate is met with a gentle "room for five" modal, not an
// alarm. Empty project slots read as soft dashed "open slot".
import { useState } from "react";
import { Link } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ProjectSlot } from "../../api/bindings/ProjectSlot";
import type { QuestionSummary } from "../../api/bindings/QuestionSummary";
import type { QuestionStrategicRow } from "../../api/bindings/QuestionStrategicRow";
import type { PortfolioSummary } from "../../api/bindings/PortfolioSummary";
import type { StewardshipStrategicRow } from "../../api/bindings/StewardshipStrategicRow";
import type { StrategicBundle } from "../../api/bindings/StrategicBundle";
import {
  activateProject,
  CuadernoError,
  errorMessage,
  getStrategicBundle,
  parkProject,
  saveMonthlySection,
} from "../../api/commands";
import CommitmentsTimeline from "../../components/commitments/CommitmentsTimeline";
import { Sparkline, usePrefersReducedMotion } from "../../components/charts/TrendChart";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "../../components/ui/dialog";
import type { Context } from "../../lib/contexts";
import { contextDotClass } from "../../lib/contexts";
import { stalenessAgo, stalenessTone } from "../../lib/staleness";
import { useReader } from "../../shell/reader";
import { shortDate } from "../../lib/dates";
import { SectionHeading } from "../../components/ui/section-heading";
import { Stepper, StepperNav, type Step } from "../../components/ui/stepper";
import { useToast } from "../../shell/Toasts";

// The commitments timeline takes a context filter; the Strategic six-week
// view shows every context, so it passes an empty set ("all"). Module
// constant so a fresh Set isn't minted on every render.
const NO_FILTER = new Set<Context>();

// The two question domains, in display order — the grid's columns.
const QUESTION_DOMAINS: QuestionSummary["domain"][] = ["research", "life"];

// Sparkline hue per context; drawn from CSS variables so it tracks the
// theme. Mirrors the chart module's palette by intent — the spark takes
// the stewardship's OWN context hue rather than cycling.
function contextStroke(context: Context): string {
  return `var(--color-ctx-${context})`;
}

/** Spelt cardinal for the cap copy ("Room for five…"); falls back to the
 * digit past nine. The default cap is 5, so this reads as the design's
 * literal line while staying correct for a vault that changed the cap. */
function numberWord(n: number): string {
  const words = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
  return words[n] ?? String(n);
}

export default function MonthlyReview() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_strategic_bundle"],
    queryFn: getStrategicBundle,
  });

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">The strategic view could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  return <MonthlyReviewBody data={data} />;
}

/** The month, e.g. "July 2026", for the header and the note-write's
 * `YYYY-MM`. Derived from the bundle's stamped `today`, never computed
 * from the platform clock. */
function monthOf(today: string): { label: string; ym: string } {
  const [y, m] = today.split("-");
  const label = new Date(Number(y), Number(m) - 1, 1).toLocaleDateString(undefined, {
    month: "long",
    year: "numeric",
  });
  return { label, ym: `${y}-${m}` };
}

const STEPS: Step[] = [
  { label: "Questions" },
  { label: "Portfolios" },
  { label: "Projects" },
  { label: "Stewardships" },
  { label: "Lookahead" },
  { label: "Focus" },
];

/** Steps that write to the note earn the firmer "already saved" line;
 * the read panels get the softer "nothing here demands finishing". */
const WRITE_STEPS = new Set([5]);

function MonthlyReviewBody({ data }: { data: StrategicBundle }) {
  const { label, ym } = monthOf(data.today);
  const [step, setStep] = useState(0);
  // Steps the reader has reached (any visit) or written (the focus step).
  // The stepper ticks visited steps, and the softer stop line can appear
  // once the reader has looked at anything — which, with reads never
  // marking themselves before, it never could.
  const [completed, setCompleted] = useState<Set<number>>(new Set([0]));

  function markComplete(index: number) {
    setCompleted((prev) => new Set(prev).add(index));
  }

  // Visiting a step counts as having looked at it. The write step still
  // earns its tick through `onSaved`, since reaching it is not the same
  // as having written the note.
  function goTo(index: number) {
    setStep(index);
    if (!WRITE_STEPS.has(index)) markComplete(index);
  }

  const hasWritten = [...completed].some((i) => WRITE_STEPS.has(i));
  const hasLookedAt = completed.size > 0;

  return (
    <div className="mx-auto max-w-4xl p-8">
      <h1 className="text-xl font-semibold text-ink">Monthly review</h1>
      <p className="mt-1 text-sm text-ink-muted">
        {label} — am I still pointed at the right questions?
      </p>

      <div className="mt-6">
        <Stepper
          steps={STEPS}
          current={step}
          completed={completed}
          onSelect={goTo}
          label="Review steps"
        />
      </div>

      {hasWritten ? (
        <p className="mt-3 text-sm text-ink-muted">you can stop here — it's already saved</p>
      ) : hasLookedAt ? (
        <p className="mt-3 text-sm text-ink-muted">
          you can stop anytime — nothing here demands finishing
        </p>
      ) : null}

      {/* Every step stays mounted; only visibility toggles, so a jump
          away and back never discards the focus step's draft. */}
      <section className="mt-6">
        <div hidden={step !== 0}>
          <QuestionsGrid questions={data.questions} portfolios={data.portfolios} />
        </div>
        <div hidden={step !== 1}>
          <PortfolioHealth portfolios={data.portfolios} />
        </div>
        <div hidden={step !== 2}>
          <ProjectSlots active={data.active} parked={data.parked} maxActive={data.max_active} />
        </div>
        <div hidden={step !== 3}>
          <StewardshipsOverview rows={data.stewardships} />
        </div>
        <div hidden={step !== 4}>
          <NextSixWeeks commitments={data.commitments} today={data.today} />
        </div>
        <div hidden={step !== 5}>
          <FocusStep month={ym} monthLabel={label} onSaved={() => markComplete(5)} />
        </div>
      </section>

      <StepperNav current={step} count={STEPS.length} onSelect={goTo} />
    </div>
  );
}

/** A save that failed mid-batch, carrying what did land so the reader is
 * not told "nothing saved" when part of it is on disk. */
class PartialSaveError extends Error {
  constructor(
    readonly section: string,
    readonly saved: string[],
    readonly detail: string,
  ) {
    super(detail);
  }
}

/** The one write on the page: the review's artefact. Wins, Themes and
 * Next Month's Focus into the monthly note — a dashboard with no write
 * has no reason to be visited on a schedule, which is exactly why the
 * cadence had no home before. */
function FocusStep({
  month,
  monthLabel,
  onSaved,
}: {
  month: string;
  monthLabel: string;
  onSaved: () => void;
}) {
  const { toast } = useToast();
  const [wins, setWins] = useState("");
  const [themes, setThemes] = useState("");
  const [focus, setFocus] = useState("");

  const save = useMutation({
    mutationFn: async () => {
      // Each section is its own compose/overwrite write, and they run in
      // sequence rather than all at once: a `Promise.all` that rejects on
      // the first failure would leave the note half-written with no word
      // on which sections landed. Serial, so a failure names the section
      // it stopped at and the ones before it are known-saved. Only the
      // sections you filled in are touched — a partial review leaves the
      // rest of the note alone.
      const pending: [string, string][] = [];
      if (wins.trim()) pending.push(["wins", wins.trim()]);
      if (themes.trim()) pending.push(["themes", themes.trim()]);
      if (focus.trim()) pending.push(["next-months-focus", focus.trim()]);
      const done: string[] = [];
      for (const [section, content] of pending) {
        try {
          await saveMonthlySection(section, content, month);
          done.push(section);
        } catch (error) {
          throw new PartialSaveError(section, done, errorMessage(error));
        }
      }
      return done.length;
    },
    onError: (error) => {
      if (error instanceof PartialSaveError && error.saved.length > 0) {
        // Say what did land, so the reader is not left thinking nothing
        // saved when in fact part of it is on disk.
        toast(
          `Saved ${error.saved.join(" and ")}, but ${error.section} failed: ${error.detail}`,
          "attention",
        );
        return;
      }
      toast(errorMessage(error), "attention");
    },
    onSuccess: () => {
      toast(`Saved to ${monthLabel}'s note.`);
      onSaved();
    },
  });

  const anything = wins.trim() !== "" || themes.trim() !== "" || focus.trim() !== "";

  return (
    <div>
      <h2 className="font-medium text-ink">Next month</h2>
      <p className="mt-1 text-sm text-ink-muted">
        What went well, what themes you noticed, and where to point next — written to{" "}
        {monthLabel}'s note.
      </p>

      <div className="mt-4 space-y-4">
        <FocusField
          label="Wins"
          hint="What the month is worth remembering for."
          value={wins}
          onChange={setWins}
        />
        <FocusField
          label="Themes"
          hint="Patterns worth naming — what kept recurring."
          value={themes}
          onChange={setThemes}
        />
        <FocusField
          label="Next month's focus"
          hint="The one thing to keep pointed at."
          value={focus}
          onChange={setFocus}
        />
      </div>

      <button
        type="button"
        onClick={() => save.mutate()}
        disabled={save.isPending || !anything}
        className="mt-4 rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken disabled:opacity-50"
      >
        Save to the note
      </button>
    </div>
  );
}

function FocusField({
  label,
  hint,
  value,
  onChange,
}: {
  label: string;
  hint: string;
  value: string;
  onChange: (next: string) => void;
}) {
  const id = `focus-${label.toLowerCase().replace(/[^a-z]+/g, "-")}`;
  return (
    <div>
      <label htmlFor={id} className="text-sm font-medium text-ink">
        {label}
      </label>
      <p className="text-xs text-ink-faint">{hint}</p>
      <textarea
        id={id}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        rows={3}
        className="mt-1 w-full resize-y rounded border border-line bg-bg-base p-2 text-sm text-ink"
      />
    </div>
  );
}

// --- Questions grid ------------------------------------------------------

/** The file stem of a vault path — `projects/alpha.md` → `alpha`. Used to
 * route a backlink path to its detail page. */
function fileStem(path: string): string {
  const base = path.slice(path.lastIndexOf("/") + 1);
  return base.endsWith(".md") ? base.slice(0, -3) : base;
}

/** The slug of a portfolio backlink path — `portfolios/surrogate/_index.md`
 * → `surrogate` (the parent dir, since the file is always `_index.md`, not
 * the slug). Routes a portfolio backlink to its detail page (#354). */
function portfolioSlug(path: string): string {
  const parts = path.split("/");
  return parts[0] === "portfolios" && parts.length >= 2 ? parts[1] : fileStem(path);
}

const chipClass = "text-xs text-ink-faint hover:text-ink-muted";

/** Portfolio link chip on a question card. A portfolio shares its
 * question's slug (portfolios.rs), so the Strategic bundle's flat
 * portfolio list is correlated to a question by exact slug match —
 * no backend link field is needed for v1. Mirrors the commitments
 * `OriginChip`: a small faint chip that navigates, here to the
 * portfolio detail route. */
function PortfolioChip({ slug }: { slug: string }) {
  return (
    <Link to={`/portfolios/${slug}`} className={chipClass}>
      {slug}
    </Link>
  );
}

/** A project that references the question — via its `core_question:`
 * frontmatter link or a body wikilink (#354, #395) — routes to the project
 * detail page. */
function ProjectChip({ slug }: { slug: string }) {
  return (
    <Link to={`/projects/${slug}`} className={chipClass}>
      {slug}
    </Link>
  );
}

/** An evidence backlink — opens the note in the reader, since evidence has
 * no dedicated detail route (#354). */
function ReaderBacklinkChip({ path }: { path: string }) {
  const { openReader } = useReader();
  return (
    <button type="button" onClick={() => openReader(path)} className={chipClass}>
      {fileStem(path)}
    </button>
  );
}

function QuestionsGrid({
  questions,
  portfolios,
}: {
  questions: QuestionStrategicRow[];
  portfolios: PortfolioSummary[];
}) {
  const { openReader } = useReader();

  return (
    <section aria-label="Questions">
      <SectionHeading>Questions</SectionHeading>
      {questions.length === 0 ? (
        <p className="mt-3 text-sm text-ink-muted">No active questions.</p>
      ) : (
        <div className="mt-3 grid gap-6 sm:grid-cols-2">
          {QUESTION_DOMAINS.map((domain) => {
            const inDomain = questions.filter((q) => q.summary.domain === domain);
            if (inDomain.length === 0) return null;
            return (
              <div key={domain}>
                <h3 className="text-xs capitalize text-ink-muted">{domain}</h3>
                <ul className="mt-2 space-y-2">
                  {inDomain.map(({ summary: q, backlinks }) => {
                    // Portfolio chips: the portfolios created for this
                    // question (sharing its slug), UNION the portfolios that
                    // body-link it with a *different* slug (via
                    // link_portfolio_to_question), deduped by slug so a
                    // portfolio is never chipped twice.
                    const portfolioSlugs = new Set(
                      portfolios.filter((p) => p.slug === q.slug).map((p) => p.slug),
                    );
                    for (const path of backlinks.portfolios) {
                      portfolioSlugs.add(portfolioSlug(path));
                    }
                    const portfolioChips = [...portfolioSlugs];
                    // Evidence body-links open in the reader. The `other`
                    // bucket (daily notes, actions, commitments …) is
                    // deliberately NOT chipped here — it's noise on the calm
                    // strategic grid; the full backlink set lives on the note.
                    const evidence = backlinks.evidence;
                    const hasChips =
                      portfolioChips.length > 0 ||
                      backlinks.projects.length > 0 ||
                      evidence.length > 0;
                    return (
                      // The card is a plain container, not a button: the
                      // title is the reader-opening control and the chips
                      // are accessible sibling links — interactive elements
                      // must not nest inside a button (a11y / DOM validity).
                      // The hover highlight and padding live on the title
                      // button (full-bleed via the container's overflow
                      // clip), so the highlighted area matches the click
                      // target — the old card-wide hover left the padding
                      // ring a dead zone that implied a click it didn't do.
                      <li
                        key={q.slug}
                        className="overflow-hidden rounded-md border border-line bg-bg-surface"
                      >
                        <button
                          type="button"
                          onClick={() => openReader(`questions/${q.domain}/${q.slug}.md`)}
                          className="flex w-full flex-col items-start gap-1 px-3 py-2 text-left hover:bg-bg-sunken"
                        >
                          <span className="text-sm text-ink">{q.question_text || q.slug}</span>
                          <span className="text-xs text-ink-faint">
                            updated {shortDate(q.updated)}
                          </span>
                        </button>
                        {hasChips && (
                          <div className="flex flex-wrap gap-x-3 gap-y-1 px-3 pb-2">
                            {portfolioChips.map((slug) => (
                              <PortfolioChip key={`pf:${slug}`} slug={slug} />
                            ))}
                            {backlinks.projects.map((path) => (
                              <ProjectChip key={`pj:${path}`} slug={fileStem(path)} />
                            ))}
                            {evidence.map((path) => (
                              <ReaderBacklinkChip key={`bl:${path}`} path={path} />
                            ))}
                          </div>
                        )}
                      </li>
                    );
                  })}
                </ul>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}

// --- Project-slot allocator ---------------------------------------------

function ProjectSlots({
  active,
  parked,
  maxActive,
}: {
  active: ProjectSlot[];
  parked: ProjectSlot[];
  maxActive: number;
}) {
  const client = useQueryClient();
  const { toast } = useToast();
  // The over-cap modal: opened when an activate is refused at the cap.
  const [capOpen, setCapOpen] = useState(false);

  function refresh() {
    void client.invalidateQueries({ queryKey: ["get_strategic_bundle"] });
  }

  const park = useMutation({
    mutationFn: (slug: string) => parkProject(slug),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => {
      // Parking frees a slot — close the modal if it was the path here.
      setCapOpen(false);
    },
    onSettled: refresh,
  });

  const activate = useMutation({
    mutationFn: (slug: string) => activateProject(slug),
    onError: (err) => {
      // The one non-toast error: hitting the cap opens the gentle modal
      // (plan §1.5) rather than flashing a message. Everything else is a
      // plain toast.
      if (err instanceof CuadernoError && err.payload.kind === "project_cap_reached") {
        setCapOpen(true);
      } else {
        toast(errorMessage(err), "attention");
      }
    },
    onSettled: refresh,
  });

  // The allocator draws `maxActive` slots: the filled ones first, then
  // soft dashed "open slot" placeholders for the remainder.
  const emptyCount = Math.max(0, maxActive - active.length);
  const emptySlots = Array.from({ length: emptyCount }, (_, i) => i);

  return (
    <section aria-label="Project slots">
      <SectionHeading>Projects</SectionHeading>

      <div className="mt-3 grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
        {active.map((slot) => (
          <div
            key={slot.slug}
            className="flex items-center gap-2 rounded-md border border-line bg-bg-surface px-3 py-2"
          >
            <span
              aria-hidden
              className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(slot.context)}`}
            />
            <span className="min-w-0 flex-1 truncate text-sm text-ink">{slot.slug}</span>
            <button
              type="button"
              onClick={() => park.mutate(slot.slug)}
              disabled={park.isPending}
              className="shrink-0 rounded px-2 py-0.5 text-xs text-ink-muted hover:bg-bg-sunken hover:text-ink disabled:opacity-50"
            >
              park
            </button>
          </div>
        ))}
        {emptySlots.map((i) => (
          <div
            key={`open-${i}`}
            className="flex items-center justify-center rounded-md border border-dashed border-line px-3 py-2 text-xs text-ink-faint"
          >
            open slot
          </div>
        ))}
      </div>

      {/* Parked shelf — activatable back into an open slot. */}
      {parked.length > 0 && (
        <div className="mt-4">
          <h3 className="text-xs text-ink-muted">Parked</h3>
          <ul className="mt-2 flex flex-wrap gap-2">
            {parked.map((slot) => (
              <li
                key={slot.slug}
                className="flex items-center gap-2 rounded-md border border-line bg-bg-sunken px-3 py-1.5"
              >
                <span
                  aria-hidden
                  className={`h-2 w-2 shrink-0 rounded-full ${contextDotClass(slot.context)}`}
                />
                <span className="text-sm text-ink-muted">{slot.slug}</span>
                <button
                  type="button"
                  onClick={() => activate.mutate(slot.slug)}
                  disabled={activate.isPending}
                  className="rounded px-2 py-0.5 text-xs text-ink-muted hover:bg-bg-surface hover:text-ink disabled:opacity-50"
                >
                  activate
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      <CapModal
        open={capOpen}
        onOpenChange={setCapOpen}
        active={active}
        maxActive={maxActive}
        onPark={(slug) => park.mutate(slug)}
        parking={park.isPending}
      />
    </section>
  );
}

/** The gentle over-cap modal (plan §1.5): "Room for five. Park one to
 * make space." — the active projects listed with inline park buttons, no
 * red, no scolding. Parking one frees a slot so the refused activate can
 * be tried again. */
function CapModal({
  open,
  onOpenChange,
  active,
  maxActive,
  onPark,
  parking,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  active: ProjectSlot[];
  maxActive: number;
  onPark: (slug: string) => void;
  parking: boolean;
}) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogTitle className="text-base font-medium text-ink">
          Room for {numberWord(maxActive)}. Park one to make space.
        </DialogTitle>
        <DialogDescription className="mt-1 text-sm text-ink-muted">
          You're focused on {numberWord(maxActive)} projects. Park one and the slot opens up.
        </DialogDescription>
        <ul className="mt-4 space-y-2">
          {active.map((slot) => (
            <li
              key={slot.slug}
              className="flex items-center gap-2 rounded-md border border-line bg-bg-surface px-3 py-2"
            >
              <span
                aria-hidden
                className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(slot.context)}`}
              />
              <span className="min-w-0 flex-1 truncate text-sm text-ink">{slot.slug}</span>
              <button
                type="button"
                onClick={() => onPark(slot.slug)}
                disabled={parking}
                className="shrink-0 rounded border border-line px-2 py-0.5 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
              >
                park
              </button>
            </li>
          ))}
        </ul>
      </DialogContent>
    </Dialog>
  );
}

// --- Portfolio health ----------------------------------------------------

function PortfolioHealth({ portfolios }: { portfolios: PortfolioSummary[] }) {
  return (
    <section aria-label="Portfolio health">
      <SectionHeading>
        Portfolio health
      </SectionHeading>
      {portfolios.length === 0 ? (
        <p className="mt-3 text-sm text-ink-muted">No portfolios yet.</p>
      ) : (
        <div className="mt-3 overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-xs text-ink-faint">
                <th className="py-1 pr-4 font-normal">Question</th>
                <th className="py-1 pr-4 font-normal">Evidence</th>
                <th className="py-1 font-normal">Last updated</th>
              </tr>
            </thead>
            <tbody>
              {portfolios.map((p) => {
                // Neutral tier from the shared ladder — never a hue.
                const tone = stalenessTone(p.staleness_days);
                const updated =
                  p.staleness_days === null ? "no evidence yet" : stalenessAgo(p.staleness_days);
                return (
                  <tr key={p.slug} className="border-t border-line">
                    <td className="py-2 pr-4">
                      {/* The same portfolio routes as a chip in the questions
                          grid above; a row in its own health table was the one
                          place it rendered as dead text (#440). */}
                      <Link
                        to={`/portfolios/${p.slug}`}
                        className="text-ink hover:text-accent-interactive hover:underline"
                      >
                        {p.question || p.slug}
                      </Link>
                    </td>
                    <td className="py-2 pr-4 text-ink-muted">{p.evidence_count}</td>
                    <td className={`py-2 ${tone}`}>{updated}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

// --- Stewardships overview ----------------------------------------------

function StewardshipsOverview({ rows }: { rows: StewardshipStrategicRow[] }) {
  const reducedMotion = usePrefersReducedMotion();

  return (
    <section aria-label="Stewardships">
      <SectionHeading>Stewardships</SectionHeading>
      {rows.length === 0 ? (
        <p className="mt-3 text-sm text-ink-muted">No stewardships yet.</p>
      ) : (
        <ul className="mt-3 space-y-1">
          {rows.map(({ summary, sparkline }) => (
            <li
              key={summary.slug}
              className="flex items-center gap-3 rounded-md border border-line bg-bg-surface px-3 py-2"
            >
              <span
                aria-hidden
                className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(summary.context)}`}
              />
              {/* A link, like the portfolio rows above — /stewardships/:slug
                  exists, and a review you cannot click through from is a
                  dead end. */}
              <Link
                to={`/stewardships/${summary.slug}`}
                className="min-w-0 flex-1 truncate text-sm text-ink hover:text-accent-interactive hover:underline"
              >
                {summary.name || summary.slug}
              </Link>
              {/* Sparkline draws only when there's data (expanded, tracked);
                  a flat stewardship's empty series renders nothing. */}
              <Sparkline
                values={sparkline}
                color={contextStroke(summary.context)}
                animate={!reducedMotion}
                label={`${summary.name || summary.slug}: 12-week habit trend`}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

// --- Next six weeks ------------------------------------------------------

function NextSixWeeks({
  commitments,
  today,
}: {
  commitments: StrategicBundle["commitments"];
  today: string;
}) {
  return (
    <section aria-label="Next six weeks">
      <SectionHeading>
        Next six weeks
      </SectionHeading>
      {commitments.length === 0 ? (
        <p className="mt-3 text-sm text-ink-muted">Nothing promised in this window.</p>
      ) : (
        <div className="mt-3">
          {/* Read-only reuse of the shared timeline: the Strategic view is
              a survey, not a place to complete promises. */}
          <CommitmentsTimeline entries={commitments} today={today} filter={NO_FILTER} readOnly />
        </div>
      )}
    </section>
  );
}
