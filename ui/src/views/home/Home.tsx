import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import { getOrientation } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";
import ProjectCard from "./ProjectCard";

const ENERGIES: EnergyLevel[] = ["deep", "medium", "light"];

/** `8 Jul` / `Jul 8` per locale — the friendly short date for the
 * commitments strip. Parsed at local midnight so the day never slips a
 * timezone. */
function shortDate(date: string): string {
  return new Date(`${date}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}

export default function Home() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_orientation"],
    queryFn: getOrientation,
  });
  const [energy, setEnergy] = useState<EnergyLevel | null>(null);
  // Roving tabindex: exactly one card wrapper is in the tab order at a
  // time (the one at `focusedIndex`); arrow / j / k move the marker and
  // shift DOM focus in step.
  const [focusedIndex, setFocusedIndex] = useState(0);
  const cardRefs = useRef<(HTMLDivElement | null)[]>([]);

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

  const heading = new Date(`${data.today}T00:00:00`).toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
  });

  // Captured out here so the roving handler (a closure, where TS won't
  // carry the post-guard non-null narrowing of `data`) sees a plain
  // number rather than a possibly-undefined query result.
  const projectCount = data.projects.length;

  // Roving focus across the card grid: arrow keys / j / k move
  // between cards (operating the content, not Tab-through-everything).
  function onGridKeyDown(event: React.KeyboardEvent) {
    // Never hijack keystrokes destined for an editable control — the
    // inline Current State editor lives inside a card, and there `j`,
    // `k`, and the arrows are text input, not navigation.
    if (
      event.target instanceof HTMLElement &&
      event.target.closest("input, textarea, [contenteditable]")
    ) {
      return;
    }
    const keys: Record<string, number> = {
      ArrowRight: 1,
      ArrowDown: 1,
      j: 1,
      ArrowLeft: -1,
      ArrowUp: -1,
      k: -1,
    };
    const delta = keys[event.key];
    if (!delta) return;
    if (projectCount === 0) return;
    const next = (focusedIndex + delta + projectCount) % projectCount;
    setFocusedIndex(next);
    cardRefs.current[next]?.focus();
    event.preventDefault();
  }

  return (
    <div className="mx-auto max-w-4xl p-8">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-ink">{heading}</h1>
        <div role="group" aria-label="Energy filter" className="flex gap-1">
          {ENERGIES.map((level) => (
            <button
              key={level}
              type="button"
              aria-pressed={energy === level}
              onClick={() => setEnergy(energy === level ? null : level)}
              className={`rounded px-2 py-1 text-xs ${
                energy === level
                  ? "bg-bg-sunken font-medium text-ink"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              {level}
            </button>
          ))}
        </div>
      </div>

      {data.commitments.length > 0 && (
        <section aria-label="Due soon" className="mt-6">
          <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">Due soon</h2>
          <ul className="mt-2 flex flex-wrap gap-2">
            {data.commitments.map((commitment) => (
              <li
                key={`${commitment.source.kind}-${commitment.date}-${commitment.title}`}
                className="flex items-center rounded border border-line bg-bg-surface px-3 py-1.5 text-sm text-ink"
              >
                <span
                  aria-hidden
                  className={`mr-2 h-2 w-2 shrink-0 rounded-full ${contextDotClass(commitment.context)}`}
                />
                <span>{commitment.title}</span>
                <span className="ml-2 text-ink-muted">
                  {commitment.is_overdue
                    ? `planned for ${shortDate(commitment.date)}`
                    : shortDate(commitment.date)}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section aria-label="Your projects" className="mt-8">
        <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
          Your projects
        </h2>
        {data.projects.length === 0 ? (
          <p className="mt-3 rounded border border-line bg-bg-surface p-6 text-ink-muted">
            Nothing active. Your CLI or Claude can start one when you're ready.
          </p>
        ) : (
          <div
            role="list"
            aria-label="Project cards"
            onKeyDown={onGridKeyDown}
            className="mt-3 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3"
          >
            {data.projects.map((project, index) => (
              <div
                key={project.slug}
                ref={(el) => {
                  cardRefs.current[index] = el;
                }}
                role="listitem"
                data-card
                tabIndex={index === focusedIndex ? 0 : -1}
                onKeyDown={(event) => {
                  // Enter / Space on the wrapper itself (focus not on a
                  // child button) fires the card's primary action — the
                  // Start button, located by its `data-start` marker.
                  if (
                    (event.key === "Enter" || event.key === " ") &&
                    event.target === event.currentTarget
                  ) {
                    event.currentTarget
                      .querySelector<HTMLElement>("[data-start]")
                      ?.click();
                    event.preventDefault();
                  }
                }}
                className="rounded-lg"
              >
                <ProjectCard project={project} energy={energy} />
              </div>
            ))}
          </div>
        )}
      </section>

      {data.lapsed_habits.length > 0 && (
        <p className="mt-8 text-sm text-ink-faint">
          quietly lapsed: {data.lapsed_habits.map((habit) => habit.detail).join(" · ")} — no
          judgment, just a note
        </p>
      )}
    </div>
  );
}
