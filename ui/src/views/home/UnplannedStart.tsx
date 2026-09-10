// Starting something that isn't on any map (#568).
//
// The shortlist answers "pick one thing" from what you planned. This is
// the other half of the morning: the thing that just landed, the fix you
// noticed, the errand you are about to do anyway. Before this, the app's
// only honest answer was "go and add it as an action first, then come
// back and start it" — two gestures and a context switch to record work
// you had already begun.
//
// It is a separate affordance from the shortlist's Start on purpose, and
// not a fallback when a start matches nothing: a fallback would turn
// every typo into a new action silently. Pressing Start here means "make
// this and begin it"; pressing Start there means "begin that". Different
// intents, different buttons.
//
// Collapsed by default — the planned shortlist is the main path, and an
// always-open form would invite capture-instead-of-doing, which is the
// friction the method exists to remove.
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import type { OrientationProject } from "../../api/bindings/OrientationProject";
import { errorMessage, startUnplannedAction } from "../../api/commands";
import { useToast } from "../../shell/Toasts";

const ENERGIES: EnergyLevel[] = ["deep", "medium", "light"];

export default function UnplannedStart({
  projects,
  energy,
}: {
  projects: OrientationProject[];
  energy: EnergyLevel | null;
}) {
  const client = useQueryClient();
  const { toast } = useToast();
  const [open, setOpen] = useState(false);
  const [project, setProject] = useState("");
  const [text, setText] = useState("");
  // The filter is a statement about the energy you have right now, so it
  // is the right default for work you are starting right now. With no
  // filter on, "medium" commits to least.
  //
  // Derived, not seeded into state: `useState(energy)` would capture the
  // filter as it stood when this mounted (collapsed, on first paint) and
  // then ignore every later change, so switching the filter and opening
  // the form would offer a stale bucket. The override wins once the user
  // states one — their explicit pick outranks the inferred default.
  const [override, setOverride] = useState<EnergyLevel | null>(null);
  const level = override ?? energy ?? "medium";

  const slug = project || projects[0]?.slug || "";

  const start = useMutation({
    mutationFn: () => startUnplannedAction(slug, text.trim(), level),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: () => {
      toast(`Started on ${slug}. It's on the map now, so you can tick it off.`);
      setText("");
      setOverride(null);
      setOpen(false);
      // The band reads the log this wrote; the shortlist and the map
      // both gained a bullet.
      void client.invalidateQueries({ queryKey: ["get_now"] });
      void client.invalidateQueries({ queryKey: ["read_daily"] });
      void client.invalidateQueries({ queryKey: ["get_orientation"] });
    },
  });

  // Nothing active means nothing to hang an action on — the shortlist
  // already says so, and a project picker with no projects would be a
  // dead form.
  if (projects.length === 0) return null;

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="mt-2 text-xs text-ink-faint hover:text-ink"
      >
        Starting something that isn't listed?
      </button>
    );
  }

  const canStart = text.trim().length > 0 && !start.isPending;

  return (
    <form
      aria-label="Start something that isn't listed"
      className="mt-2 rounded border border-line bg-bg-surface p-3"
      onSubmit={(event) => {
        event.preventDefault();
        if (canStart) start.mutate();
      }}
    >
      <div className="flex flex-wrap items-end gap-2">
        <div className="min-w-0 flex-1">
          <label htmlFor="unplanned-text" className="block text-xs text-ink-muted">
            What are you starting?
          </label>
          <input
            id="unplanned-text"
            value={text}
            onChange={(event) => setText(event.target.value)}
            // Autofocus is right here: the button was pressed to type.
            autoFocus
            className="mt-1 w-full rounded border border-line bg-bg px-2 py-1 text-sm text-ink"
          />
        </div>
        <div>
          <label htmlFor="unplanned-project" className="block text-xs text-ink-muted">
            On
          </label>
          <select
            id="unplanned-project"
            value={slug}
            onChange={(event) => setProject(event.target.value)}
            className="mt-1 rounded border border-line bg-bg px-2 py-1 text-sm text-ink"
          >
            {projects.map((candidate) => (
              <option key={candidate.slug} value={candidate.slug}>
                {candidate.slug}
              </option>
            ))}
          </select>
        </div>
        <div>
          <label htmlFor="unplanned-energy" className="block text-xs text-ink-muted">
            Energy
          </label>
          <select
            id="unplanned-energy"
            value={level}
            onChange={(event) => setOverride(event.target.value as EnergyLevel)}
            className="mt-1 rounded border border-line bg-bg px-2 py-1 text-sm text-ink"
          >
            {ENERGIES.map((candidate) => (
              <option key={candidate} value={candidate}>
                {candidate}
              </option>
            ))}
          </select>
        </div>
      </div>
      <div className="mt-3 flex items-center gap-2">
        <button
          type="submit"
          disabled={!canStart}
          className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken disabled:opacity-50"
        >
          Start
        </button>
        <button
          type="button"
          onClick={() => setOpen(false)}
          className="text-xs text-ink-faint hover:text-ink"
        >
          Cancel
        </button>
        <span className="ml-auto text-xs text-ink-faint">
          Adds it to {slug} and starts it
        </span>
      </div>
    </form>
  );
}
