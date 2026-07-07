// The Weekly Review (plan §1.4, #55): a guided 5-step flow built to
// resist becoming a chore. The steps are NON-LINEAR — the dots jump
// anywhere — and each step's save is complete in itself, so step 1
// alone is a valid, celebratory two-minute review. After the first save
// or "looked at" the flow reassures "you can stop here — it's already
// saved". No "N of 5" counter unless the metrics toggle (§3.11) is on.
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

  // Once anything is committed, every step so far persists — the
  // reassurance the anti-chore rule asks for.
  const hasSaved = completed.size > 0;

  return (
    <div className="mx-auto max-w-3xl p-8">
      <h1 className="text-xl font-semibold text-ink">Weekly review</h1>
      <p className="mt-1 text-sm text-ink-muted">Week of {weekLabel(data.week_of)}</p>

      <nav aria-label="Review steps" className="mt-6 flex items-center gap-3">
        {STEPS.map((label, i) => {
          const isCurrent = i === step;
          const isDone = completed.has(i);
          return (
            <button
              key={label}
              type="button"
              aria-label={label}
              aria-current={isCurrent ? "step" : undefined}
              onClick={() => setStep(i)}
              className={`h-2.5 w-2.5 rounded-full transition-colors ${
                isCurrent
                  ? "bg-ink"
                  : isDone
                    ? "bg-ink-muted"
                    : "border border-line bg-transparent hover:bg-bg-sunken"
              }`}
            />
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

      <section className="mt-6">
        {step === 0 && <WinsStep bundle={data} onSaved={() => markComplete(0)} />}
        {step === 1 && (
          <ProjectScanStep
            projects={data.projects}
            stuck={data.stuck}
            onSaved={() => markComplete(1)}
          />
        )}
        {step === 2 && (
          <StewardshipScanStep
            stewardships={data.stewardships}
            onLookedAt={() => markComplete(2)}
          />
        )}
        {step === 3 && (
          <LookaheadStep
            entries={data.commitments}
            today={data.today}
            onLookedAt={() => markComplete(3)}
          />
        )}
        {step === 4 && <FocusStep bundle={data} onSaved={() => markComplete(4)} />}
      </section>

      {hasSaved && (
        <p className="mt-8 text-sm text-ink-muted">you can stop here — it's already saved</p>
      )}
    </div>
  );
}
