// A labelled stepper for the review rituals (#449).
//
// The weekly review's stepper was five unlabelled 10px dots. The step
// names existed only as `aria-label` — present for a screen reader,
// invisible to everyone else — so a sighted user saw five dots and had
// to click each one to learn what it was. The shape of the review, what
// it is about to ask of you, was hidden until you had walked it.
//
// State was encoded purely as colour and shape, too. Here the current
// step is named and emphasised, a completed one is ticked, and a rail
// connects them so the sequence reads as a sequence.
//
// Extracted rather than inlined because the monthly review is the same
// ritual at a different cadence and should not grow a second one.
import { Fragment } from "react";

export interface Step {
  /** Shown on the rail and used as the button's accessible name. */
  label: string;
}

export function Stepper({
  steps,
  current,
  completed,
  onSelect,
  label,
}: {
  steps: Step[];
  current: number;
  /** Indices already saved or marked looked-at. */
  completed: Set<number>;
  onSelect: (index: number) => void;
  /** Names the navigation landmark ("Review steps"). */
  label: string;
}) {
  return (
    <nav aria-label={label}>
      <ol className="flex flex-wrap items-center gap-1">
        {steps.map((step, i) => {
          const isCurrent = i === current;
          const isDone = completed.has(i);
          return (
            <Fragment key={step.label}>
              {i > 0 && (
                // The rail. Decorative — the order is already carried by
                // the list and by each button's position in it.
                <li aria-hidden className="h-px w-3 shrink-0 bg-line sm:w-5" />
              )}
              <li>
                <button
                  type="button"
                  aria-current={isCurrent ? "step" : undefined}
                  onClick={() => onSelect(i)}
                  className={`flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs ${
                    isCurrent
                      ? "bg-bg-sunken font-medium text-ink outline outline-2 outline-offset-1 outline-focus-ring"
                      : isDone
                        ? "text-ink-muted hover:text-ink"
                        : "text-ink-faint hover:text-ink"
                  }`}
                >
                  {/* A tick for a finished step, its number otherwise —
                      so "where am I" and "what is left" are both
                      readable without relying on a colour difference.
                      Hidden from AT: the label names the step and
                      `aria-current` marks the position. */}
                  <span
                    aria-hidden
                    className={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full text-[10px] ${
                      isDone ? "bg-ink-muted text-bg-surface" : "border border-line"
                    }`}
                  >
                    {isDone ? "✓" : i + 1}
                  </span>
                  {step.label}
                </button>
              </li>
            </Fragment>
          );
        })}
      </ol>
    </nav>
  );
}

/** Back / Next beneath a step's content. Clicking a rail entry was the
 * only way to move, which makes a linear ritual feel like a menu. */
export function StepperNav({
  current,
  count,
  onSelect,
}: {
  current: number;
  count: number;
  onSelect: (index: number) => void;
}) {
  return (
    <div className="mt-6 flex items-center justify-between border-t border-line pt-4">
      <button
        type="button"
        onClick={() => onSelect(current - 1)}
        disabled={current === 0}
        className="rounded border border-line px-3 py-1 text-sm text-ink-muted hover:text-ink disabled:opacity-40"
      >
        Back
      </button>
      <button
        type="button"
        onClick={() => onSelect(current + 1)}
        disabled={current === count - 1}
        className="rounded border border-line px-3 py-1 text-sm text-ink-muted hover:text-ink disabled:opacity-40"
      >
        Next
      </button>
    </div>
  );
}
