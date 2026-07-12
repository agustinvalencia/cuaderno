// The Oldest/Newest-first switch for a log-card surface (the daily `## Logs`
// stack and a project's "recently in your logs"). Self-contained: it reads
// and flips the shared, app-wide order preference (lib/logOrder), so a flip
// in one place flips every log surface.
import { toggleLogOrder, useLogOrder } from "../../lib/logOrder";

export function LogOrderToggle() {
  const order = useLogOrder();
  return (
    <button
      type="button"
      onClick={toggleLogOrder}
      // The accessible name contains the visible label (WCAG 2.5.3) and adds
      // the action; the visible text shows the current order.
      aria-label={`${order === "newest" ? "Newest" : "Oldest"} first — click to reverse log order`}
      className="shrink-0 rounded px-1.5 py-0.5 text-xs text-ink-muted hover:text-ink"
    >
      {order === "newest" ? "Newest first" : "Oldest first"} ⇅
    </button>
  );
}
