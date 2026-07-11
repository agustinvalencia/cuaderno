// The note's frontmatter, presented as a distinct metadata block (UI
// request 2026-07-12): a sunken, bordered strip with a faint "Properties"
// label and key/value pairs, clearly separated from the prose below so it
// reads as "note metadata" rather than the note's first heading (the old
// bare chip row abutting the body read like a header section). Renders
// nothing when there's no scalar frontmatter to show.

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
  if (pairs.length === 0) return null;
  return (
    <div className={`rounded-md border border-line bg-bg-sunken px-3 py-2.5 ${className}`}>
      <p className="mb-1.5 text-xs font-medium uppercase tracking-wider text-ink-faint">
        Properties
      </p>
      <dl className="flex flex-wrap gap-x-4 gap-y-1">
        {pairs.map(([key, value]) => (
          <div key={key} className="flex items-baseline gap-1.5">
            <dt className="text-xs text-ink-muted">{key}</dt>
            <dd className="text-xs text-ink">{value}</dd>
          </div>
        ))}
      </dl>
    </div>
  );
}
