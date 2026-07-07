import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import { getOrientation } from "../../api/commands";
import ProjectCard from "./ProjectCard";

const ENERGIES: EnergyLevel[] = ["deep", "medium", "light"];

export default function Home() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_orientation"],
    queryFn: getOrientation,
  });
  const [energy, setEnergy] = useState<EnergyLevel | null>(null);
  const gridRef = useRef<HTMLDivElement>(null);

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

  // Roving focus across the card grid: arrow keys / j / k move
  // between cards (operating the content, not Tab-through-everything).
  function onGridKeyDown(event: React.KeyboardEvent) {
    const keys: Record<string, number> = {
      ArrowRight: 1,
      ArrowDown: 1,
      j: 1,
      ArrowLeft: -1,
      ArrowUp: -1,
      k: -1,
    };
    const delta = keys[event.key];
    if (!delta || !gridRef.current) return;
    const cards = Array.from(gridRef.current.querySelectorAll<HTMLElement>("[data-card]"));
    const current = cards.findIndex((card) => card.contains(document.activeElement));
    const next = cards[(current + delta + cards.length) % cards.length];
    next?.focus();
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
                className="rounded border border-line bg-bg-surface px-3 py-1.5 text-sm text-ink"
              >
                <span>{commitment.title}</span>
                <span className="ml-2 text-ink-muted">
                  {commitment.is_overdue ? `planned for ${commitment.date}` : commitment.date}
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
            ref={gridRef}
            onKeyDown={onGridKeyDown}
            className="mt-3 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3"
          >
            {data.projects.map((project) => (
              <div key={project.slug} data-card tabIndex={-1} className="rounded-lg">
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
