// What you are in the middle of (#442).
//
// A context switch costs the thread, and reconstructing it means reading
// back through a note — which is the friction the method's decision-free
// capture is meant to remove. This says it outright.
//
// The state is the vault's, not the app's: starting an action already
// writes `started [[slug]] - text` to the daily log and completing it
// writes its counterpart, so this reads that back. Nothing to keep in
// sync, and it sees a start made from the CLI or by an agent over MCP,
// not only one clicked here.
//
// With nothing open it turns into the pick-one prompt rather than
// disappearing: on a blank morning "nothing started yet" is the honest
// answer, and the shortlist below is where to begin.
import { useEffect, useState } from "react";
import { Link } from "react-router";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { completeAction, errorMessage, getNow } from "../../api/commands";
import { actionLabel } from "../../lib/actionLabel";
import { contextDotClass } from "../../lib/contexts";
import { useToast } from "../../shell/Toasts";

/** How long ago `started` (an `HH:MM` stamp) was, in words. */
export function elapsedSince(started: string, now: Date): string | null {
  const [hours, minutes] = started.split(":").map(Number);
  if (Number.isNaN(hours) || Number.isNaN(minutes)) return null;
  const from = new Date(now);
  from.setHours(hours, minutes, 0, 0);
  const mins = Math.floor((now.getTime() - from.getTime()) / 60000);
  // A start stamped later than "now" means the clock moved (a nap past
  // midnight, a timezone change). Say nothing rather than "-3h ago".
  if (mins < 0) return null;
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m`;
  const h = Math.floor(mins / 60);
  const m = mins % 60;
  return m === 0 ? `${h}h` : `${h}h ${m}m`;
}

export default function NowBand() {
  const client = useQueryClient();
  const { toast } = useToast();
  const { data } = useQuery({ queryKey: ["get_now"], queryFn: getNow });

  // Tick so the elapsed label stays true without refetching the vault.
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const timer = setInterval(() => setNow(new Date()), 30_000);
    return () => clearInterval(timer);
  }, []);

  const complete = useMutation({
    mutationFn: () => completeAction(data?.project ?? "", data?.action ?? ""),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: () => toast(`Done: one step further on ${data?.project}.`),
    onSettled: () => {
      void client.invalidateQueries({ queryKey: ["get_now"] });
      void client.invalidateQueries({ queryKey: ["get_orientation"] });
    },
  });

  if (!data) {
    return (
      <div className="rounded-lg border border-line bg-bg-surface px-4 py-3">
        <p className="text-sm text-ink-muted">
          Nothing started yet. Pick one thing below and press start.
        </p>
      </div>
    );
  }

  const since = elapsedSince(data.started, now);
  return (
    <div
      aria-label="What you are working on"
      className="rounded-lg border border-line bg-bg-surface px-4 py-3"
    >
      <div className="flex items-center gap-2">
        <span
          aria-hidden
          className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(data.context ?? "")}`}
        />
        <Link
          to={`/projects/${data.project}`}
          className="truncate text-sm font-medium text-ink hover:text-accent-interactive"
        >
          {data.project}
        </Link>
        <span className="ml-auto shrink-0 text-xs text-ink-faint">
          since {data.started}
          {since === null ? "" : ` · ${since}`}
        </span>
      </div>
      <p className="mt-1.5 text-sm text-ink">{actionLabel(data.action)}</p>
      <div className="mt-2.5">
        <button
          type="button"
          onClick={() => complete.mutate()}
          disabled={complete.isPending}
          className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
        >
          Done
        </button>
      </div>
    </div>
  );
}
