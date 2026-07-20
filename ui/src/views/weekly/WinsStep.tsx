// Step 1 — Wins (celebration first, plan §1.4). A seeded, editable
// textarea: the seed is composed from the week's completed actions and
// a few log lines, unless the note already holds Wins content (existing
// beats seed). An empty week seeds nothing and shows the calm prompt as
// a placeholder rather than a blank reproach.
import { useRef } from "react";
import { useMutation } from "@tanstack/react-query";
import type { WeeklyBundle } from "../../api/bindings/WeeklyBundle";
import MarkdownEditor from "../../components/markdown/MarkdownEditor";
import { errorMessage, saveWeeklySection } from "../../api/commands";
import { shortDate } from "../../lib/dates";
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
  const { seed, empty } = composeWinsSeed(bundle);
  // The editor is uncontrolled (seeded once); track its latest value in a ref
  // so Save reads it without re-rendering the editor on every keystroke.
  const draft = useRef(seed);

  const save = useMutation({
    mutationFn: (content: string) => saveWeeklySection("wins", content, bundle.week_of),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: () => {
      toast("Wins saved.");
      onSaved();
    },
  });

  const completed = bundle.completed_actions;

  return (
    <div>
      <h2 className="font-medium text-ink">Wins</h2>
      <p className="mt-1 text-sm text-ink-muted">
        Celebration first — what went well this week?
      </p>

      {/* The week's completions as celebration cards — a scannable "look
          what you did" before you write it up. Calm, never a tally. */}
      {completed.length > 0 && (
        <section aria-labelledby="wins-completed" className="mt-4">
          <h3
            id="wins-completed"
            className="text-xs font-medium uppercase tracking-wider text-ink-faint"
          >
            Completed this week
          </h3>
          <ul className="mt-2 grid gap-1.5 sm:grid-cols-2">
            {completed.map((action, index) => (
              <li
                key={`${action.slug}-${action.title}-${index}`}
                className="flex items-start gap-2 rounded-md border border-line bg-bg-surface px-3 py-2"
              >
                <span aria-hidden className="mt-0.5 shrink-0 text-accent-interactive">
                  ✓
                </span>
                <div className="min-w-0">
                  <p className="text-sm text-ink">{action.title}</p>
                  <p className="text-xs text-ink-faint">
                    {action.project} · {shortDate(action.completed)}
                  </p>
                </div>
              </li>
            ))}
          </ul>
        </section>
      )}

      <form
        className="mt-3"
        onSubmit={(event) => {
          event.preventDefault();
          const value = draft.current.trim();
          if (value) save.mutate(value);
        }}
      >
        <div className="h-64">
          <MarkdownEditor
            initialDoc={seed}
            ariaLabel="This week's wins"
            autoFocus={false}
            placeholder={
              empty ? "No completions parsed — what felt like progress anyway?" : undefined
            }
            onChange={(value) => {
              draft.current = value;
            }}
          />
        </div>
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
