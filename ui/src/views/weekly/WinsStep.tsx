// Step 1 — Wins (celebration first, plan §1.4). A seeded, editable
// textarea: the seed is composed from the week's completed actions and
// a few log lines, unless the note already holds Wins content (existing
// beats seed). An empty week seeds nothing and shows the calm prompt as
// a placeholder rather than a blank reproach.
import { useRef } from "react";
import { useMutation } from "@tanstack/react-query";
import type { WeeklyBundle } from "../../api/bindings/WeeklyBundle";
import { errorMessage, saveWeeklySection } from "../../api/commands";
import { useToast } from "../../shell/Toasts";

// A few log lines are enough to jog memory; the whole week's log would
// bury the completions.
const SEED_LOG_LINES = 3;

/** Compose the Wins seed. Existing note content wins; otherwise build
 * it from completed actions ("- Completed: {title} ({project})") plus a
 * few log lines. `empty` is true only when nothing at all was parsed —
 * the calm-placeholder case. */
export function composeWinsSeed(bundle: WeeklyBundle): { seed: string; empty: boolean } {
  const existing = bundle.weekly.wins;
  if (existing) {
    return { seed: existing, empty: false };
  }
  const lines = [
    ...bundle.completed_actions.map((c) => `- Completed: ${c.title} (${c.project})`),
    ...bundle.logs.slice(0, SEED_LOG_LINES).map((l) => `- ${l.text}`),
  ];
  return { seed: lines.join("\n"), empty: lines.length === 0 };
}

export default function WinsStep({
  bundle,
  onSaved,
}: {
  bundle: WeeklyBundle;
  onSaved: () => void;
}) {
  const { toast } = useToast();
  const draft = useRef<HTMLTextAreaElement>(null);
  const { seed, empty } = composeWinsSeed(bundle);

  const save = useMutation({
    mutationFn: (content: string) => saveWeeklySection("wins", content, bundle.week_of),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: () => {
      toast("Wins saved.");
      onSaved();
    },
  });

  return (
    <div>
      <h2 className="font-medium text-ink">Wins</h2>
      <p className="mt-1 text-sm text-ink-muted">
        Celebration first — what went well this week?
      </p>
      <form
        className="mt-3"
        onSubmit={(event) => {
          event.preventDefault();
          const value = draft.current?.value.trim();
          if (value) save.mutate(value);
        }}
      >
        <textarea
          ref={draft}
          defaultValue={seed}
          rows={8}
          aria-label="This week's wins"
          placeholder={empty ? "No completions parsed — what felt like progress anyway?" : undefined}
          className="w-full rounded border border-line bg-bg-base p-2 text-sm text-ink"
        />
        <div className="mt-2">
          <button
            type="submit"
            disabled={save.isPending}
            className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
          >
            Save wins
          </button>
        </div>
      </form>
    </div>
  );
}
