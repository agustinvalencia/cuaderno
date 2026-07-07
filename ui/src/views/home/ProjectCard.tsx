import { useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { ActionListEntry } from "../../api/bindings/ActionListEntry";
import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import type { OrientationProject } from "../../api/bindings/OrientationProject";
import type { OrientationView } from "../../api/bindings/OrientationView";
import { completeAction, startAction, updateProjectState } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";
import { useMetrics } from "../../lib/metrics";
import { useToast } from "../../shell/Toasts";

/// The energy filter's no-match rule (design law): a card never
/// blanks. With a filter active, surface the first matching bullet;
/// otherwise keep the best-available action with a muted note —
/// low-energy moments must not be greeted by empty cards.
export function surfacedAction(
  project: OrientationProject,
  energy: EnergyLevel | null,
): { action: ActionListEntry | null; matchedFilter: boolean } {
  if (energy) {
    const match = project.actions.find((a) => a.energy === energy);
    if (match) return { action: match, matchedFilter: true };
  }
  const best =
    project.actions[0] ??
    (project.top_action
      ? { text: project.top_action.text, energy: project.top_action.energy, attached: null }
      : null);
  return { action: best, matchedFilter: energy === null };
}

export default function ProjectCard({
  project,
  energy,
}: {
  project: OrientationProject;
  energy: EnergyLevel | null;
}) {
  const client = useQueryClient();
  const { toast } = useToast();
  const showMetrics = useMetrics();
  const [started, setStarted] = useState(false);
  const [editing, setEditing] = useState(false);
  const stateDraft = useRef<HTMLTextAreaElement>(null);

  const { action, matchedFilter } = surfacedAction(project, energy);

  const start = useMutation({
    mutationFn: () => startAction(project.slug, action?.text ?? ""),
    onMutate: () => setStarted(true),
    onError: (error) => {
      setStarted(false);
      toast(String(error), "attention");
    },
  });

  const complete = useMutation({
    mutationFn: (text: string) => completeAction(project.slug, text),
    onMutate: async (text) => {
      await client.cancelQueries({ queryKey: ["get_orientation"] });
      const previous = client.getQueryData<OrientationView>(["get_orientation"]);
      client.setQueryData<OrientationView>(["get_orientation"], (view) =>
        view
          ? {
              ...view,
              projects: view.projects.map((p) =>
                p.slug === project.slug
                  ? {
                      ...p,
                      actions: p.actions.filter((a) => a.text !== text),
                      top_action: p.top_action?.text === text ? null : p.top_action,
                    }
                  : p,
              ),
            }
          : view,
      );
      return { previous };
    },
    onError: (error, _text, context) => {
      if (context?.previous) {
        client.setQueryData(["get_orientation"], context.previous);
      }
      toast(String(error), "attention");
    },
    onSuccess: () => toast(`Done: one step further on ${project.slug}.`),
    onSettled: () => client.invalidateQueries({ queryKey: ["get_orientation"] }),
  });

  const saveState = useMutation({
    mutationFn: (newState: string) => updateProjectState(project.slug, newState),
    onError: (error) => toast(String(error), "attention"),
    onSuccess: () => setEditing(false),
    onSettled: () => client.invalidateQueries({ queryKey: ["get_orientation"] }),
  });

  return (
    <article className="rounded-lg border border-line bg-bg-surface p-4">
      <div className="flex items-center gap-2">
        <span
          aria-hidden
          className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(project.context)}`}
        />
        <h3 className="min-w-0 flex-1 truncate font-medium text-ink">{project.slug}</h3>
        {showMetrics && project.actions.length > 0 && (
          <span className="rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-faint">
            {project.actions.length} open
          </span>
        )}
      </div>

      {editing ? (
        <form
          className="mt-2"
          onSubmit={(event) => {
            event.preventDefault();
            const draft = stateDraft.current?.value.trim();
            if (draft) saveState.mutate(draft);
          }}
        >
          <textarea
            ref={stateDraft}
            defaultValue={project.state_snippet}
            rows={3}
            aria-label={`Current state of ${project.slug}`}
            className="w-full rounded border border-line bg-bg-base p-2 text-sm text-ink"
          />
          <div className="mt-1 flex gap-2">
            <button
              type="submit"
              disabled={saveState.isPending}
              className="rounded border border-line px-2 py-0.5 text-xs text-ink hover:bg-bg-sunken"
            >
              Save
            </button>
            <button
              type="button"
              onClick={() => setEditing(false)}
              className="rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
            >
              Cancel
            </button>
          </div>
        </form>
      ) : (
        <button
          type="button"
          onClick={() => setEditing(true)}
          aria-label={`Edit the current state of ${project.slug}`}
          className="mt-2 block w-full rounded text-left text-sm text-ink-muted hover:bg-bg-sunken"
        >
          <span className="line-clamp-2">{project.state_snippet || "No state written yet."}</span>
        </button>
      )}

      {action && (
        <div className="mt-3">
          {!matchedFilter && (
            <p className="text-xs text-ink-faint">
              no {energy} action here — smallest step:
            </p>
          )}
          <p className="text-sm text-ink">
            <span aria-hidden className="text-ink-faint">
              →{" "}
            </span>
            {action.text}
            {action.energy && (
              <span className="ml-1 text-xs text-ink-faint">({action.energy})</span>
            )}
          </p>
          <div className="mt-3 flex items-center gap-2">
            {started ? (
              <span className="text-xs text-ink-muted">in today's log ✓</span>
            ) : (
              <button
                type="button"
                onClick={() => start.mutate()}
                disabled={start.isPending}
                className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
              >
                Start
              </button>
            )}
            <button
              type="button"
              onClick={() => complete.mutate(action.text)}
              disabled={complete.isPending}
              aria-label={`Mark done: ${action.text}`}
              className="rounded px-2 py-1 text-xs text-ink-muted hover:text-ink"
            >
              done
            </button>
          </div>
        </div>
      )}
    </article>
  );
}
