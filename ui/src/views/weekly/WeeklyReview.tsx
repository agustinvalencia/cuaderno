// The Weekly Review (plan §1.4, #55): a guided 5-step flow built to
// resist becoming a chore. The steps are NON-LINEAR — the dots jump
// anywhere — and each step's save is complete in itself, so step 1
// alone is a valid, celebratory two-minute review. After the first
// actual save the flow reassures "you can stop here — it's already
// saved"; a mere "looked at" earns the softer "you can stop anytime".
// No "N of 5" counter unless the metrics toggle (§3.11) is on.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { getWeeklyBundle } from "../../api/commands";
import { useMetrics } from "../../lib/metrics";
import FocusStep from "./FocusStep";
import LookaheadStep from "./LookaheadStep";
import ProjectScanStep from "./ProjectScanStep";
import StewardshipScanStep from "./StewardshipScanStep";
import WinsStep from "./WinsStep";

// Step labels double as the dots' accessible names.
const STEPS = ["Wins", "Projects", "Stewardships", "Lookahead", "Focus"] as const;

// Steps whose completion means a real vault WRITE (wins save, a project
// state save, the focus save) — these earn the "it's already saved"
// reassurance. Steps 2 and 3 are read-only "looked at" marks that write
// nothing, so claiming "saved" for them would be dishonest.
const WRITE_STEPS: ReadonlySet<number> = new Set([0, 1, 4]);

/** The reviewed week's Monday, read as a calm "6 Jul 2026". */
function weekLabel(date: string): string {
  return new Date(`${date}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

export default function WeeklyReview() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_weekly_bundle"],
    queryFn: () => getWeeklyBundle(),
  });
  const showMetrics = useMetrics();
  const [step, setStep] = useState(0);
  // Which steps have been saved or marked looked-at. Drives the
  // optional metrics counter and the stop-anywhere reassurance.
  const [completed, setCompleted] = useState<Set<number>>(new Set());

  function markComplete(index: number) {
    setCompleted((prev) => {
      const next = new Set(prev);
      next.add(index);
      return next;
    });
  }

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">The vault could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  // Two honesty tiers for the stop-anywhere reassurance: only an actual
  // vault write earns "it's already saved"; read-only looked-at marks
  // get the softer "nothing here demands finishing".
  const hasWritten = [...completed].some((i) => WRITE_STEPS.has(i));
  const hasLookedAt = completed.size > 0;

  return (
    <div className="mx-auto max-w-3xl p-8">
      <h1 className="text-xl font-semibold text-ink">Weekly review</h1>
      <p className="mt-1 text-sm text-ink-muted">Week of {weekLabel(data.week_of)}</p>

      <nav aria-label="Review steps" className="mt-6 flex items-center gap-1">
        {STEPS.map((label, i) => {
          const isCurrent = i === step;
          const isDone = completed.has(i);
          return (
            // The button carries p-2 padding around the 10px dot so the
            // hit target is ~26px (>= the 24px WCAG minimum) while the
            // dot itself stays visually small.
            <button
              key={label}
              type="button"
              aria-label={label}
              aria-current={isCurrent ? "step" : undefined}
              onClick={() => setStep(i)}
              className="group rounded-full p-2"
            >
              <span
                aria-hidden
                className={`block h-2.5 w-2.5 rounded-full transition-colors ${
                  isCurrent
                    ? // The current step reads as more than a colour
                      // change: a focus-ring-token outline ringing the
                      // filled dot (plus aria-current for AT).
                      "bg-ink outline outline-2 outline-offset-2 outline-focus-ring"
                    : isDone
                      ? "bg-ink-muted"
                      : "border border-line bg-transparent group-hover:bg-bg-sunken"
                }`}
              />
            </button>
          );
        })}
        {/* The counter is a metrics surface: hidden by default (calm is
            the default posture), a muted aside only when the toggle is on. */}
        {showMetrics && (
          <span className="ml-2 text-xs text-ink-faint">
            {completed.size} of {STEPS.length} complete
          </span>
        )}
      </nav>

      {/* All five steps stay MOUNTED; only visibility toggles (the
          `hidden` attribute — display:none via the reset, so a hidden
          step is neither visible nor tab-reachable). Conditional
          rendering would unmount the inactive steps and discard their
          uncontrolled textarea drafts on every non-linear jump. */}
      <section className="mt-6">
        <div hidden={step !== 0}>
          <WinsStep bundle={data} onSaved={() => markComplete(0)} />
        </div>
        <div hidden={step !== 1}>
          <ProjectScanStep
            projects={data.projects}
            stuck={data.stuck}
            onSaved={() => markComplete(1)}
          />
        </div>
        <div hidden={step !== 2}>
          <StewardshipScanStep
            stewardships={data.stewardships}
            onLookedAt={() => markComplete(2)}
          />
        </div>
        <div hidden={step !== 3}>
          <LookaheadStep
            entries={data.commitments}
            today={data.today}
            onLookedAt={() => markComplete(3)}
          />
        </div>
        <div hidden={step !== 4}>
          <FocusStep bundle={data} onSaved={() => markComplete(4)} />
        </div>
      </section>

      {hasWritten ? (
        <p className="mt-8 text-sm text-ink-muted">you can stop here — it's already saved</p>
      ) : hasLookedAt ? (
        <p className="mt-8 text-sm text-ink-muted">
          you can stop anytime — nothing here demands finishing
        </p>
      ) : null}
    </div>
  );
}
