// The note's frontmatter, presented as a distinct metadata block (UI
// request 2026-07-12; collapsible + aligned 2026-07-13): a sunken, bordered
// strip clearly separated from the prose below so it reads as "note
// metadata" rather than the note's first heading. The "Properties" label is
// a disclosure toggle (default open), and the pairs lay out as an aligned
// key/value grid — a run-on wrapped row read as one dense line. Renders
// nothing when there's no scalar frontmatter to show.
import { useState } from "react";

/** Flat scalar frontmatter as `[key, value]` pairs. Objects and arrays
 * are skipped — a metadata strip is for at-a-glance scalars (type,
 * context, created), not nested structure. Returns `[]` for anything
 * that isn't a plain object (the wire type is `unknown`). */
export function scalarFrontmatter(frontmatter: unknown): [string, string][] {
  if (!frontmatter || typeof frontmatter !== "object" || Array.isArray(frontmatter)) {
    return [];
  }
  const pairs: [string, string][] = [];
  for (const [key, value] of Object.entries(frontmatter)) {
    if (value === null) continue;
    const scalar =
      typeof value === "string" || typeof value === "number" || typeof value === "boolean";
    if (scalar) pairs.push([key, String(value)]);
  }
  return pairs;
}

export function MetaPanel({
  frontmatter,
  className = "",
}: {
  frontmatter: unknown;
  className?: string;
}) {
  const pairs = scalarFrontmatter(frontmatter);
  const [open, setOpen] = useState(true);
  if (pairs.length === 0) return null;
  return (
    <div className={`rounded-md border border-line bg-bg-sunken ${className}`}>
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="flex w-full items-center gap-1.5 rounded-md px-3 py-2 text-xs font-medium uppercase tracking-wider text-ink-faint hover:text-ink-muted"
      >
        {/* Geometric disclosure glyph (not an emoji), rotating with state. */}
        <span aria-hidden className="text-[0.6rem] leading-none">
          {open ? "▾" : "▸"}
        </span>
        Properties
        {!open && (
          <span className="ml-0.5 tracking-normal normal-case">({pairs.length})</span>
        )}
      </button>
      {open && (
        <dl className="grid grid-cols-1 gap-x-8 gap-y-1.5 px-3 pt-0.5 pb-2.5 sm:grid-cols-2">
          {pairs.map(([key, value]) => (
            <div key={key} className="flex items-baseline gap-2">
              <dt className="w-20 shrink-0 truncate text-xs text-ink-muted">{key}</dt>
              <dd className="min-w-0 text-xs text-ink">{value}</dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}
