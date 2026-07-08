// Step 3 — Stewardship scan (read-only, plan §1.4). A calm list of the
// habits and areas being kept: name, context dot, tracking count and
// staleness in muted text. A lapsed stewardship reads only as grey
// prose — never an alarm. Being read-only, its "save" is a local "mark
// as looked at" that writes nothing.
import type { StewardshipSummary } from "../../api/bindings/StewardshipSummary";
import { contextDotClass } from "../../lib/contexts";

/** The muted status line: tracking volume and how fresh it is, or a
 * gentle "no tracking yet" for a stewardship with none. A stewardship
 * with a `tracking/` folder but nothing recent reads as lapsed prose. */
function statusLine(s: StewardshipSummary): string {
  if (s.last_tracking_date === null) {
    return s.variant === "expanded" ? "no tracking yet" : "dashboard only";
  }
  const days = s.staleness_days;
  const freshness =
    days === null ? "" : days <= 0n ? " · tracked today" : ` · last tracked ${days.toString()}d ago`;
  return `${s.tracking_count} tracked${freshness}`;
}

export default function StewardshipScanStep({
  stewardships,
  onLookedAt,
}: {
  stewardships: StewardshipSummary[];
  onLookedAt: () => void;
}) {
  return (
    <div>
      <h2 className="font-medium text-ink">Stewardships</h2>
      <p className="mt-1 text-sm text-ink-muted">A glance at what you're keeping up.</p>

      {stewardships.length === 0 ? (
        <p className="mt-3 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          Nothing to steward yet.
        </p>
      ) : (
        <ul className="mt-3 space-y-2">
          {stewardships.map((s) => (
            <li
              key={s.slug}
              className="flex items-center gap-3 rounded-md border border-line bg-bg-surface px-3 py-2"
            >
              <span
                aria-hidden
                className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(s.context)}`}
              />
              <span className="min-w-0 flex-1 truncate text-sm text-ink">
                {s.name || s.slug}
              </span>
              <span className="shrink-0 text-xs text-ink-faint">{statusLine(s)}</span>
            </li>
          ))}
        </ul>
      )}

      <div className="mt-4">
        <button
          type="button"
          onClick={onLookedAt}
          className="rounded border border-line px-3 py-1 text-sm text-ink-muted hover:bg-bg-sunken hover:text-ink"
        >
          Looked at these
        </button>
      </div>
    </div>
  );
}
