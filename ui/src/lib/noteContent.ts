// Pure parsing helpers that turn a vault note's raw markdown blob (as
// `read_daily`/`read_weekly`/`read_monthly` return it — frontmatter and
// all) into the pieces the calendar panel presents distinctly (UI request
// 2026-07-12): the frontmatter as metadata, the body split into its `##`
// sections, and a `## Logs` section broken into individual timestamped
// entries. Frontend-side because it's a pure presentation concern — the
// domain deliberately hands the note back as one blob (markdown is the
// source of truth; the app is a lens over it).

/** One `## ` section of a note. `heading` is null for the preamble before
 * the first heading (typically the `# Title`). `body` is the markdown
 * under the heading, heading line excluded, trimmed of surrounding blank
 * lines. */
export interface NoteSection {
  heading: string | null;
  body: string;
}

export interface ParsedNote {
  /** Scalar frontmatter as a flat `key → value` record (`{}` if none).
   * Container values (arrays/objects, which appear as an empty scalar or
   * indented children) are dropped — the metadata strip shows scalars. */
  frontmatter: Record<string, string>;
  sections: NoteSection[];
}

/** A single timestamped daily-log entry (`- **HH:MM**: text`). */
export interface LogEntry {
  time: string;
  text: string;
}

// A leading YAML frontmatter fence: `---\n … \n---`, tolerant of CRLF.
const FRONTMATTER_RE = /^---\r?\n([\s\S]*?)\r?\n---\r?\n?/;

/** Parse a note's raw markdown into frontmatter + `##` sections. */
export function parseNote(markdown: string): ParsedNote {
  let rest = markdown;
  let frontmatter: Record<string, string> = {};
  const fm = FRONTMATTER_RE.exec(markdown);
  if (fm) {
    frontmatter = parseScalarFrontmatter(fm[1]);
    rest = markdown.slice(fm[0].length);
  }
  return { frontmatter, sections: splitBodySections(rest) };
}

/** Parse the frontmatter block's `key: value` lines into a flat record.
 * Only top-level (unindented) scalar lines with a non-empty value are
 * kept: a bare `tags:` heading a YAML list, or any indented child, is a
 * container and skipped (the metadata strip is for at-a-glance scalars).
 * Surrounding single/double quotes on the value are stripped. */
function parseScalarFrontmatter(block: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of block.split("\n")) {
    // Skip indented (nested) lines and list items.
    if (/^\s/.test(line) || line.trimStart().startsWith("- ")) continue;
    const m = /^([A-Za-z0-9_-]+):\s*(.*)$/.exec(line);
    if (!m) continue;
    const value = stripQuotes(m[2].trim());
    if (value.length === 0) continue; // a block container key (e.g. a list) — skip
    // Skip inline flow containers (`[a, b]`, `{x: 1}`) too — the strip
    // shows at-a-glance scalars, not structure.
    if (value.startsWith("[") || value.startsWith("{")) continue;
    out[m[1]] = value;
  }
  return out;
}

function stripQuotes(value: string): string {
  if (value.length >= 2) {
    const first = value[0];
    const last = value[value.length - 1];
    if ((first === '"' && last === '"') || (first === "'" && last === "'")) {
      return value.slice(1, -1);
    }
  }
  return value;
}

/** Split a note body (frontmatter already removed) on its level-2 (`## `)
 * headings. `###`+ headings stay inside their section's body. Sections
 * with an empty body are dropped — an unfilled scaffold heading is
 * "what's missing", which the calendar deliberately doesn't lead with.
 *
 * Exported so the note reader — which gets a frontmatter-free `body` from
 * `read_note` — can section it the same way the calendar sections a raw
 * blob via [`parseNote`], for one shared presentation. */
export function splitBodySections(body: string): NoteSection[] {
  const sections: NoteSection[] = [];
  let heading: string | null = null;
  let buf: string[] = [];
  let inFence = false;
  const flush = () => {
    const content = buf.join("\n").trim();
    if (content.length > 0) sections.push({ heading, body: content });
  };
  for (const line of body.split("\n")) {
    // Track fenced code blocks (``` or ~~~) so a `## ` line *inside* a
    // fence — a note quoting markdown/shell — isn't mistaken for a
    // section heading and doesn't split the fence apart.
    if (/^\s*(```|~~~)/.test(line)) {
      inFence = !inFence;
      buf.push(line);
      continue;
    }
    const m = inFence ? null : /^##\s+(.+?)\s*$/.exec(line);
    if (m) {
      flush();
      heading = m[1];
      buf = [];
    } else {
      buf.push(line);
    }
  }
  flush();
  return sections;
}

/** True when a section heading is the append-only daily `## Logs`
 * history section (case-insensitive). */
export function isLogsSection(heading: string | null): boolean {
  return heading !== null && heading.trim().toLowerCase() === "logs";
}

/** Parse a `## Logs` section body into `(time, text)` entries, mirroring
 * the domain's `parse_log_lines`: each entry is `- **HH:MM**: text`;
 * indented continuation lines fold into the previous entry's text,
 * joined by `; `. Lines that don't match are ignored. */
export function parseLogEntries(body: string): LogEntry[] {
  const out: LogEntry[] = [];
  for (const raw of body.split("\n")) {
    const line = raw.replace(/\s+$/, "");
    const m = /^-\s+\*\*(\d{1,2}:\d{2})\*\*:\s(.*)$/.exec(line);
    if (m) {
      out.push({ time: m[1], text: m[2] });
    } else if (line.trim().length > 0 && /^\s/.test(raw) && out.length > 0) {
      out[out.length - 1].text += `; ${line.trim()}`;
    }
  }
  return out;
}
