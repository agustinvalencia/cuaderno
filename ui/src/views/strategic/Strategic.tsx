// Strategic / Monthly view (M9, plan §1.5; #57) — the calm monthly
// review behind `/strategic`. One composed read paints every panel:
// the questions grid (by domain), the portfolio-health table, the
// button-based 5-slot project allocator (drag was dropped per review),
// the stewardship overview with habit sparklines, and the six-week
// commitments timeline.
//
// Colour laws hold throughout: context hues are identity, staleness is
// neutral ink emphasis (never a hue), and there is no red anywhere — an
// over-cap activate is met with a gentle "room for five" modal, not an
// alarm. Empty project slots read as soft dashed "open slot" — breathing
// room, not vacancy.
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ProjectSlot } from "../../api/bindings/ProjectSlot";
import type { QuestionSummary } from "../../api/bindings/QuestionSummary";
import type { PortfolioSummary } from "../../api/bindings/PortfolioSummary";
import type { StewardshipStrategicRow } from "../../api/bindings/StewardshipStrategicRow";
import type { StrategicBundle } from "../../api/bindings/StrategicBundle";
import {
  activateProject,
  CuadernoError,
  errorMessage,
  getStrategicBundle,
  parkProject,
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

/** `8 Jul` / `Jul 8` per locale, at local midnight (no timezone slip). */
function shortDate(date: string): string {
  return new Date(`${date}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}

/** Spelt cardinal for the cap copy ("Room for five…"); falls back to the
 * digit past nine. The default cap is 5, so this reads as the design's
 * literal line while staying correct for a vault that changed the cap. */
function numberWord(n: number): string {
  const words = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
  return words[n] ?? String(n);
}

export default function Strategic() {
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

  return <StrategicBody data={data} />;
}

function StrategicBody({ data }: { data: StrategicBundle }) {
  return (
    <div className="mx-auto max-w-4xl space-y-12 p-8">
      <header>
        <h1 className="text-xl font-semibold text-ink">Strategic</h1>
        <p className="mt-1 text-sm text-ink-muted">
          The long view — questions, projects, portfolios, and what's promised.
        </p>
      </header>

      <QuestionsGrid questions={data.questions} />
      <ProjectSlots active={data.active} parked={data.parked} maxActive={data.max_active} />
      <PortfolioHealth portfolios={data.portfolios} />
      <StewardshipsOverview rows={data.stewardships} />
      <NextSixWeeks commitments={data.commitments} today={data.today} />
    </div>
  );
}

// --- Questions grid ------------------------------------------------------

function QuestionsGrid({ questions }: { questions: QuestionSummary[] }) {
  const { openReader } = useReader();

  return (
    <section aria-label="Questions">
      <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">Questions</h2>
      {questions.length === 0 ? (
        <p className="mt-3 text-sm text-ink-muted">No active questions.</p>
      ) : (
        <div className="mt-3 grid gap-6 sm:grid-cols-2">
          {QUESTION_DOMAINS.map((domain) => {
            const inDomain = questions.filter((q) => q.domain === domain);
            if (inDomain.length === 0) return null;
            return (
              <div key={domain}>
                <h3 className="text-xs capitalize text-ink-muted">{domain}</h3>
                <ul className="mt-2 space-y-2">
                  {inDomain.map((q) => (
                    <li key={q.slug}>
                      <button
                        type="button"
                        onClick={() => openReader(`questions/${q.domain}/${q.slug}.md`)}
                        className="flex w-full flex-col items-start gap-1 rounded-md border border-line bg-bg-surface px-3 py-2 text-left hover:bg-bg-sunken"
                      >
                        <span className="text-sm text-ink">{q.question_text || q.slug}</span>
                        <span className="text-xs text-ink-faint">
                          updated {shortDate(q.updated)}
                        </span>
                      </button>
                    </li>
                  ))}
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
      <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">Projects</h2>

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
      <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
        Portfolio health
      </h2>
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
                      <span className="text-ink">{p.question || p.slug}</span>
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
      <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">Stewardships</h2>
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
              <span className="min-w-0 flex-1 truncate text-sm text-ink">
                {summary.name || summary.slug}
              </span>
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
      <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
        Next six weeks
      </h2>
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
