// The degraded-watcher pill + poll fallback (plan §3.1). When the
// backend reports the watcher degraded (a notify Rescan/overflow or a
// failed reconcile), live event-driven refresh can no longer be
// trusted, so: show a muted grey pill in the sidebar footer — calm
// wording, no alarm — and poll instead, invalidating all queries
// every 60s until the next healthy batch clears the state. Renders
// nothing while healthy (no empty frame).

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useWatcherState } from "../lib/watcherStatus";

// One minute: coarse enough to stay cheap (refetches are index-backed)
// and fine enough that a degraded session never drifts far from disk.
const POLL_MS = 60_000;

export default function WatcherPill() {
  const state = useWatcherState();
  const client = useQueryClient();
  const degraded = state === "degraded";

  // The poll fallback lives with the pill so the two degraded-mode
  // behaviours (tell the user, keep data fresh anyway) switch on and
  // off together. The effect re-runs on recovery and clears the timer.
  useEffect(() => {
    if (!degraded) return;
    const timer = setInterval(() => {
      void client.invalidateQueries();
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [degraded, client]);

  if (!degraded) return null;

  return (
    <span
      role="status"
      title="live updates paused — refresh with focus"
      className="mt-2 self-start rounded-full bg-bg-sunken px-2 py-0.5 text-xs text-ink-faint"
    >
      live updates paused
    </span>
  );
}
