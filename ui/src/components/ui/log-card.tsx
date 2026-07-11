// A single log entry as a card (plan §3.10 calm surfaces): a fixed-width
// time/date column beside the entry text, giving the "when + what"
// hierarchy a bare list line lacks. Shared by the daily-note Logs section
// and the project "recently in your logs" list — anywhere timestamped log
// lines are surfaced. Tabular, monospace times so a column of entries
// reads as a scannable ledger, not prose.
import type { ReactNode } from "react";

export function LogCard({
  time,
  date,
  children,
  className = "",
}: {
  /** The entry time, e.g. "14:32" — the prominent scan anchor. */
  time?: string;
  /** An optional day label; project mentions span days, a single day's
   * own logs share one date and omit it. Shown faint above the time. */
  date?: string;
  children: ReactNode;
  className?: string;
}) {
  const hasStamp = Boolean(time || date);
  return (
    <div
      className={`flex gap-3 rounded-md border border-line bg-bg-surface px-3 py-2 ${className}`}
    >
      {hasStamp && (
        <div className="flex w-12 shrink-0 flex-col items-start leading-tight">
          {date && <span className="text-xs text-ink-faint">{date}</span>}
          {time && (
            <span className="font-mono text-xs tabular-nums text-ink-muted">{time}</span>
          )}
        </div>
      )}
      <div className="min-w-0 flex-1 self-center text-sm text-ink">{children}</div>
    </div>
  );
}
