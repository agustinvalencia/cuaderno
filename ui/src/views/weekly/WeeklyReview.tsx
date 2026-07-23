// The Weekly Review (plan §1.4, #55): a guided 5-step flow built to
// resist becoming a chore. The steps are NON-LINEAR — the dots jump
// anywhere — and each step's save is complete in itself, so step 1
// alone is a valid, celebratory two-minute review. After the first
// actual save the flow reassures "you can stop here — it's already
// saved"; a mere "looked at" earns the softer "you can stop anytime".
// No "N of 5" counter unless the metrics toggle (§3.11) is on.
import { useState } from "react";
import { Stepper, StepperNav } from "../../components/ui/stepper";
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

      <div className="mt-6 flex flex-wrap items-center gap-3">
        {/* Named steps on a rail. They were five unlabelled 10px dots
            whose names lived only in `aria-label` — so a sighted user saw
            five dots and had to click each one to find out what it was,
            and the shape of the review stayed hidden until you had walked
            it. */}
        <Stepper
          steps={STEPS.map((label) => ({ label }))}
          current={step}
          completed={completed}
          onSelect={setStep}
          label="Review steps"
        />
        {/* The counter is a metrics surface: hidden by default (calm is
            the default posture), a muted aside only when the toggle is on. */}
        {showMetrics && (
          <span className="text-xs text-ink-faint">
            {completed.size} of {STEPS.length} complete
          </span>
        )}
      </div>

      {/* The reassurance sits above the step, not below it. On a tall
          step it was under the fold — which is exactly the moment it is
          needed. */}
      {hasWritten ? (
        <p className="mt-3 text-sm text-ink-muted">you can stop here — it's already saved</p>
      ) : hasLookedAt ? (
        <p className="mt-3 text-sm text-ink-muted">
          you can stop anytime — nothing here demands finishing
        </p>
      ) : null}

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

      {/* Clicking a rail entry used to be the only way to move, which
          makes a linear ritual read as a menu. */}
      <StepperNav current={step} count={STEPS.length} onSelect={setStep} />
    </div>
  );
}
