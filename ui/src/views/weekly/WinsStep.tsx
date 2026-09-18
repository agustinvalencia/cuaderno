// Step 1 — Wins (celebration first, plan §1.4; reworked in #449).
//
// RLM insists the wins come first because that is what reframes a week
// for a brain prone to counting only what it missed. The step asked you
// to compose prose in a fixed-height editor at exactly the moment the
// method is trying to hand you a list of things that already went right.
//
// So the candidates — the week's completed actions and its log lines —
// are cards you add with a click, and the wins themselves are cards you
// can tick, edit, reorder and remove. Markdown stays the source of
// truth: what is saved is ordinary bullets, and a hand-written `## Wins`
// section parses straight back into cards.
import { useRef, useState } from "react";
import { useMutation } from "@tanstack/react-query";
import type { WeeklyBundle } from "../../api/bindings/WeeklyBundle";
import { errorMessage, saveWeeklySection } from "../../api/commands";
import { CappedList } from "../../components/ui/capped-list";
import { shortDate } from "../../lib/dates";
import { useToast } from "../../shell/Toasts";
import { parseWins, reorder, serialiseWins, type Win, type WinCard } from "./wins";

/** How many candidates show before "show all". The old seed took the
 * first three log lines and dropped the rest silently. */
const CANDIDATES_SHOWN = 5;

/** What the week offers as a starting point, in the order the method
 * would read them: what you finished, then what you wrote down. */
export function candidatesFor(bundle: WeeklyBundle): string[] {
  return [
    ...bundle.completed_actions.map((c) => `Completed: ${c.title} (${c.project})`),
    ...bundle.logs.map((l) => l.text),
  ];
}

/** The wins the step opens with: whatever the note already holds. An
 * existing section always beats a seed — the note is the record. */
export function initialWins(bundle: WeeklyBundle): Win[] {
  return bundle.weekly.wins ? parseWins(bundle.weekly.wins) : [];
}

export default function WinsStep({
  bundle,
  onSaved,
}: {
  bundle: WeeklyBundle;
  onSaved: () => void;
}) {
  const { toast } = useToast();
  // A monotonic id per row, so React identity follows the win rather than
  // its position. `useRef` rather than a counter in render, so an id is
  // never reused across renders.
  const nextId = useRef(0);
  const mint = (win: Win): WinCard => ({ ...win, id: nextId.current++ });
  const [wins, setWins] = useState<WinCard[]>(() => initialWins(bundle).map(mint));
  const candidates = candidatesFor(bundle);
  const taken = new Set(wins.map((w) => w.text));

  const save = useMutation({
    mutationFn: (content: string) => saveWeeklySection("wins", content, bundle.week_of),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: () => {
      toast("Wins saved.");
      onSaved();
    },
  });

  function add(text: string) {
    // A candidate from the week is a thing that happened, so it comes in
    // ticked; a blank "add your own" is not yet anything, so it does not.
    setWins((current) => [
      ...current,
      mint({ text, done: text !== "", checkbox: false }),
    ]);
  }

  // Blank rows are scaffolding, not wins — they do not get written.
  const savable = wins.filter((w) => w.text.trim() !== "");

  /** What to call a win in an accessible name. A just-added blank one
   * has no text to name it by, and "Win: " reads as nothing at all. */
  function nameOf(win: Win): string {
    return win.text.trim() || "new win";
  }

  function update(index: number, patch: Partial<Win>) {
    setWins((current) => current.map((w, i) => (i === index ? { ...w, ...patch } : w)));
  }

  return (
    <div>
      <h2 className="font-medium text-ink">Wins</h2>
      <p className="mt-1 text-sm text-ink-muted">
        Celebration first — what went well this week?
      </p>

      <section aria-label="Your wins" className="mt-4">
        {wins.length === 0 ? (
          <p className="rounded border border-line bg-bg-surface p-4 text-sm text-ink-muted">
            Nothing here yet. Add from the week below, or write your own — something felt
            like progress even on a week that did not look like it.
          </p>
        ) : (
          <ul className="space-y-1.5">
            {wins.map((win, index) => (
              <li
                key={win.id}
                className="flex items-center gap-2 rounded-md border border-line bg-bg-surface px-3 py-2"
              >
                <input
                  type="checkbox"
                  checked={win.done}
                  aria-label={`Done: ${nameOf(win)}`}
                  // Only `done` moves here. Whether the bullet is written
                  // as a checkbox is derived at serialise time from
                  // `done || checkbox`, so unticking a plain win returns
                  // it to a plain bullet.
                  onChange={(event) => update(index, { done: event.target.checked })}
                  className="shrink-0 accent-[var(--color-accent-interactive)]"
                />
                {/* Editable in place. The old blob could only be edited
                    as a whole, which meant fixing one word was a
                    text-selection exercise in a 16-line box. */}
                <input
                  type="text"
                  value={win.text}
                  aria-label={`Win: ${nameOf(win)}`}
                  onChange={(event) => update(index, { text: event.target.value })}
                  className="min-w-0 flex-1 rounded border border-transparent bg-transparent px-1 py-0.5 text-sm text-ink hover:border-line focus:border-line focus:outline-none"
                />
                <button
                  type="button"
                  onClick={() => setWins((c) => reorder(c, index, index - 1))}
                  disabled={index === 0}
                  aria-label={`Move up: ${nameOf(win)}`}
                  className="shrink-0 rounded px-1 text-xs text-ink-faint hover:text-ink disabled:opacity-30"
                >
                  ↑
                </button>
                <button
                  type="button"
                  onClick={() => setWins((c) => reorder(c, index, index + 1))}
                  disabled={index === wins.length - 1}
                  aria-label={`Move down: ${nameOf(win)}`}
                  className="shrink-0 rounded px-1 text-xs text-ink-faint hover:text-ink disabled:opacity-30"
                >
                  ↓
                </button>
                <button
                  type="button"
                  onClick={() => setWins((c) => c.filter((_, i) => i !== index))}
                  aria-label={`Remove: ${nameOf(win)}`}
                  className="shrink-0 rounded px-1 text-xs text-ink-faint hover:text-ink"
                >
                  ×
                </button>
              </li>
            ))}
          </ul>
        )}

        <button
          type="button"
          onClick={() => add("")}
          className="mt-2 rounded text-xs text-ink-faint underline decoration-dotted underline-offset-2 hover:text-ink"
        >
          Add your own
        </button>
      </section>

      {/* The week's own record, offered rather than pasted. These cards
          used to sit above the editor as decoration — the same
          completions the prose below already listed, with no way to act
          on them. */}
      {candidates.length > 0 && (
        <section aria-label="From your week" className="mt-6 border-t border-line pt-4">
          <h3 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
            From your week
          </h3>
          <div className="mt-2">
            <CappedList
              label="candidates"
              limit={CANDIDATES_SHOWN}
              items={candidates.map((text, index) => {
                const already = taken.has(text);
                const completion = bundle.completed_actions[index];
                return (
                  <div
                    key={`${text}-${index}`}
                    className="mb-1.5 flex items-center gap-2 rounded-md border border-line bg-bg-surface px-3 py-2"
                  >
                    <div className="min-w-0 flex-1">
                      <p className="truncate text-sm text-ink">{text}</p>
                      {completion && (
                        <p className="text-xs text-ink-faint">
                          {shortDate(completion.completed)}
                        </p>
                      )}
                    </div>
                    <button
                      type="button"
                      onClick={() => add(text)}
                      disabled={already}
                      aria-label={already ? `Already added: ${text}` : `Add: ${text}`}
                      className="shrink-0 rounded border border-line px-2 py-0.5 text-xs text-ink-muted hover:text-ink disabled:opacity-40"
                    >
                      {already ? "added" : "add"}
                    </button>
                  </div>
                );
              })}
            />
          </div>
        </section>
      )}

      <div className="mt-4">
        <button
          type="button"
          disabled={save.isPending || savable.length === 0}
          onClick={() => save.mutate(serialiseWins(savable))}
          className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken disabled:opacity-50"
        >
          Save wins
        </button>
      </div>
    </div>
  );
}
