// Step 2 — Project scan (plan §1.4). One card per active project with
// an inline Current State editor (the ProjectCard pattern, decoupled
// from the orientation query and its OrientationProject shape). A stuck
// project earns a muted "state untouched for N days" line — informative,
// never accusatory, never red.
import { useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { ProjectSummary } from "../../api/bindings/ProjectSummary";
import type { StuckProject } from "../../api/bindings/StuckProject";
import { errorMessage, updateProjectState } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";
import { useToast } from "../../shell/Toasts";

function ProjectStateEditor({
  project,
  staleDays,
  onSaved,
}: {
  project: ProjectSummary;
  staleDays: bigint | null;
  onSaved: () => void;
}) {
  const client = useQueryClient();
  const { toast } = useToast();
  const [editing, setEditing] = useState(false);
  const draft = useRef<HTMLTextAreaElement>(null);

  const save = useMutation({
    mutationFn: (newState: string) => updateProjectState(project.slug, newState),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: (warnings) => {
      setEditing(false);
      toast(`Updated ${project.slug}.`);
      warnings.forEach((w) => toast(w, "attention"));
      onSaved();
    },
    // The review reads from this bundle, so a state write refreshes it.
    onSettled: () => client.invalidateQueries({ queryKey: ["get_weekly_bundle"] }),
  });

  return (
    <article className="rounded-lg border border-line bg-bg-surface p-4">
      <div className="flex items-center gap-2">
        <span
          aria-hidden
          className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(project.context)}`}
        />
        <h3 className="min-w-0 flex-1 truncate font-medium text-ink">{project.slug}</h3>
      </div>

      {staleDays !== null && (
        // Grey, factual — the staleness nudge, never a scolding.
        <p className="mt-1 text-xs text-ink-faint">
          state untouched for {staleDays.toString()} days
        </p>
      )}

      {editing ? (
        <form
          className="mt-2"
          onSubmit={(event) => {
            event.preventDefault();
            const value = draft.current?.value.trim();
            if (value) save.mutate(value);
          }}
        >
          <textarea
            ref={draft}
            defaultValue={project.state_snippet}
            rows={3}
            aria-label={`Current state of ${project.slug}`}
            className="w-full rounded border border-line bg-bg-base p-2 text-sm text-ink"
          />
          <div className="mt-1 flex gap-2">
            <button
              type="submit"
              disabled={save.isPending}
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
          <span className="line-clamp-2">
            {project.state_snippet || "No state written yet."}
          </span>
        </button>
      )}
    </article>
  );
}

export default function ProjectScanStep({
  projects,
  stuck,
  onSaved,
}: {
  projects: ProjectSummary[];
  stuck: StuckProject[];
  onSaved: () => void;
}) {
  // slug -> days untouched, for the muted staleness line.
  const staleBySlug = new Map(stuck.map((s) => [s.slug, s.days_unchanged]));

  if (projects.length === 0) {
    return (
      <div>
        <h2 className="font-medium text-ink">Projects</h2>
        <p className="mt-3 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          No active projects to scan.
        </p>
      </div>
    );
  }

  return (
    <div>
      <h2 className="font-medium text-ink">Projects</h2>
      <p className="mt-1 text-sm text-ink-muted">
        A quick look at where each one stands — edit any state that has drifted.
      </p>
      <div className="mt-3 space-y-3">
        {projects.map((project) => (
          <ProjectStateEditor
            key={project.slug}
            project={project}
            staleDays={staleBySlug.get(project.slug) ?? null}
            onSaved={onSaved}
          />
        ))}
      </div>
    </div>
  );
}
