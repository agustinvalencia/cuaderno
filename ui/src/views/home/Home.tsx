import { useQuery } from "@tanstack/react-query";
import { getOrientation } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";

// M1 scope: render the live orientation calmly — commitments strip,
// project cards with state + top action, lapsed line. Interactions
// (energy selector, Start, ticks, inline edit) land in M2.
export default function Home() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_orientation"],
    queryFn: getOrientation,
  });

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

  return (
    <div className="mx-auto max-w-4xl p-8">
      <h1 className="text-xl font-semibold text-ink">{heading}</h1>

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
          <div className="mt-3 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {data.projects.map((project) => (
              <article
                key={project.slug}
                className="rounded-lg border border-line bg-bg-surface p-4"
              >
                <div className="flex items-center gap-2">
                  <span
                    aria-hidden
                    className={`h-2.5 w-2.5 rounded-full ${contextDotClass(project.context)}`}
                  />
                  <h3 className="truncate font-medium text-ink">{project.slug}</h3>
                </div>
                <p className="mt-2 line-clamp-2 text-sm text-ink-muted">{project.state_snippet}</p>
                {project.top_action && (
                  <p className="mt-3 text-sm text-ink">
                    <span aria-hidden className="text-ink-faint">
                      →{" "}
                    </span>
                    {project.top_action.text}
                    {project.top_action.energy && (
                      <span className="ml-1 text-xs text-ink-faint">
                        ({project.top_action.energy})
                      </span>
                    )}
                  </p>
                )}
              </article>
            ))}
          </div>
        )}
      </section>

      {data.lapsed_habits.length > 0 && (
        <p className="mt-8 text-sm text-ink-faint">
          quietly lapsed:{" "}
          {data.lapsed_habits.map((habit) => habit.detail).join(" · ")} — no judgment, just a note
        </p>
      )}
    </div>
  );
}
