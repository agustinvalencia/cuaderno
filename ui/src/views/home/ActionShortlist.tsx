// One line per project, not a card each (#442).
//
// The card grid restated what the sidebar already lists and what the
// project map says better, and it was most of the page. What the daily
// orientation actually needs is the method's "assess energy, pick ONE
// thing" — so this is the smallest surface that does that: the energy
// filter, one surfaced action per project, and a way to start it.
//
// The no-blank rule from the card grid survives: with a filter on and no
// match, the project still offers its best-available action rather than
// vanishing. A low-energy moment must not be met by an empty page.
import { Link } from "react-router";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import type { OrientationProject } from "../../api/bindings/OrientationProject";
import { errorMessage, startAction } from "../../api/commands";
import { actionLabel } from "../../lib/actionLabel";
import { contextDotClass } from "../../lib/contexts";
import { useToast } from "../../shell/Toasts";
import { surfacedAction } from "./surfacedAction";

export default function ActionShortlist({
  projects,
  energy,
}: {
  projects: OrientationProject[];
  energy: EnergyLevel | null;
}) {
  if (projects.length === 0) {
    return (
      <p className="rounded border border-line bg-bg-surface p-4 text-sm text-ink-muted">
        Nothing active. Your CLI or Claude can start a project when you're ready.
      </p>
    );
  }
  return (
    <ul className="space-y-1">
      {projects.map((project) => (
        <ShortlistRow key={project.slug} project={project} energy={energy} />
      ))}
    </ul>
  );
}

function ShortlistRow({
  project,
  energy,
}: {
  project: OrientationProject;
  energy: EnergyLevel | null;
}) {
  const client = useQueryClient();
  const { toast } = useToast();
  const { action, matchedFilter } = surfacedAction(project, energy);

  const start = useMutation({
    mutationFn: () => startAction(project.slug, action?.text ?? ""),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: () => {
      // The Now band reads the log this just wrote.
      void client.invalidateQueries({ queryKey: ["get_now"] });
      void client.invalidateQueries({ queryKey: ["read_daily"] });
    },
  });

  return (
    <li className="flex items-center gap-2 rounded border border-line bg-bg-surface px-3 py-2">
      <span
        aria-hidden
        className={`h-2 w-2 shrink-0 rounded-full ${contextDotClass(project.context)}`}
      />
      <Link
        to={`/projects/${project.slug}`}
        className="w-28 shrink-0 truncate text-sm text-ink-muted hover:text-ink"
      >
        {project.slug}
      </Link>
      {action ? (
        <>
          <span className="min-w-0 flex-1 truncate text-sm text-ink" title={actionLabel(action.text)}>
            {actionLabel(action.text)}
            {!matchedFilter && energy && (
              <span className="ml-1 text-xs text-ink-faint">(no {energy} action here)</span>
            )}
          </span>
          <button
            type="button"
            data-start
            onClick={() => start.mutate()}
            disabled={start.isPending}
            className="shrink-0 rounded border border-line px-2 py-0.5 text-xs text-ink hover:bg-bg-sunken"
          >
            Start
          </button>
        </>
      ) : (
        <span className="min-w-0 flex-1 truncate text-sm text-ink-faint">No open actions.</span>
      )}
    </li>
  );
}
