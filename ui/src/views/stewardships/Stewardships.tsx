// Stewardship list (M7, plan §1.7) — the calm index behind
// `/stewardships`. Each row is one perpetual responsibility: a context
// dot, its name, a variant chip (expanded / flat), and a muted
// staleness line. A lapsed stewardship reads only as grey prose, never
// an alarm. Rows link to `/stewardships/:slug`.
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import type { StewardshipSummary } from "../../api/bindings/StewardshipSummary";
import { listStewardships } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";

/** The muted status line: tracking volume and how fresh it is, or a
 * gentle "no tracking yet" for an expanded stewardship with none /
 * "dashboard only" for a flat one. */
function statusLine(s: StewardshipSummary): string {
  if (s.last_tracking_date === null) {
    return s.variant === "expanded" ? "no tracking yet" : "dashboard only";
  }
  const days = s.staleness_days;
  const freshness =
    days === null
      ? ""
      : days <= 0n
        ? " · last tracked today"
        : ` · last tracked ${days.toString()}d ago`;
  return `${s.tracking_count} tracked${freshness}`;
}

export default function Stewardships() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["list_stewardships"],
    queryFn: listStewardships,
  });

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">Stewardships could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl p-8">
      <header>
        <h1 className="text-xl font-semibold text-ink">Stewardships</h1>
        <p className="mt-1 text-sm text-ink-muted">
          The perpetual responsibilities you're keeping up.
        </p>
      </header>

      {data.length === 0 ? (
        <p className="mt-6 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          Nothing to steward yet.
        </p>
      ) : (
        <ul className="mt-6 space-y-2">
          {data.map((s) => (
            <li key={s.slug}>
              <Link
                to={`/stewardships/${s.slug}`}
                className="flex items-center gap-3 rounded-md border border-line bg-bg-surface px-3 py-2 hover:bg-bg-sunken"
              >
                <span
                  aria-hidden
                  className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(s.context)}`}
                />
                <span className="min-w-0 flex-1 truncate text-sm text-ink">
                  {s.name || s.slug}
                </span>
                <span className="shrink-0 rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-faint">
                  {s.variant}
                </span>
                <span className="shrink-0 text-xs text-ink-faint">{statusLine(s)}</span>
              </Link>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
