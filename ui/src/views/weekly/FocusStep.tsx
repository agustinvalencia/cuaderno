// Step 5 — Focus (plan §1.4). One line for next week's single
// direction, saved to NEXT week's note's This Week's Goal — the goal is
// "carried into the next week's note by the review" (domain weekly.rs),
// so the save targets bundle.next_week_of, never the reviewed week
// (whose own goal must survive the review untouched). Quick-pick
// buttons from the active project slugs fill the input in a tap. If
// next week's note already carries a goal (planning got there first),
// it's shown for editing rather than overwritten blindly.
import { useState } from "react";
import { useMutation } from "@tanstack/react-query";
import type { WeeklyBundle } from "../../api/bindings/WeeklyBundle";
import { errorMessage, saveWeeklySection } from "../../api/commands";
import { useToast } from "../../shell/Toasts";

export default function FocusStep({
  bundle,
  onSaved,
}: {
  bundle: WeeklyBundle;
  onSaved: () => void;
}) {
  const { toast } = useToast();
  // Controlled so the quick-pick buttons can fill it. Seeded from NEXT
  // week's existing goal when present (the section this step writes).
  const [value, setValue] = useState(bundle.next_week_goal ?? "");

  const save = useMutation({
    mutationFn: (content: string) =>
      saveWeeklySection("this-weeks-goal", content, bundle.next_week_of),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: () => {
      toast("Focus set.");
      onSaved();
    },
  });

  return (
    <div>
      <h2 className="font-medium text-ink">Next week's focus</h2>
      <p className="mt-1 text-sm text-ink-muted">One direction is enough.</p>

      <form
        className="mt-3"
        onSubmit={(event) => {
          event.preventDefault();
          const trimmed = value.trim();
          if (trimmed) save.mutate(trimmed);
        }}
      >
        <input
          type="text"
          value={value}
          onChange={(event) => setValue(event.target.value)}
          aria-label="Next week's focus"
          placeholder="The one thing that matters most next week"
          className="w-full rounded border border-line bg-bg-base p-2 text-sm text-ink"
        />

        {bundle.projects.length > 0 && (
          <div className="mt-2 flex flex-wrap gap-2">
            {bundle.projects.map((project) => (
              <button
                key={project.slug}
                type="button"
                onClick={() => setValue(project.slug)}
                className="rounded-full border border-line px-2.5 py-1 text-xs text-ink-muted hover:bg-bg-sunken hover:text-ink"
              >
                {project.slug}
              </button>
            ))}
          </div>
        )}

        <div className="mt-3">
          <button
            type="submit"
            disabled={save.isPending}
            className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
          >
            Set focus
          </button>
        </div>
      </form>
    </div>
  );
}
