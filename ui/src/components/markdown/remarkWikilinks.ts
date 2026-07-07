// A tiny remark plugin turning `[[target]]` and `[[target|label]]`
// into link nodes the Markdown renderer can wire to in-app navigation
// (plan §3.8). It runs BEFORE remark-gfm's autolinking, and walks the
// mdast tree by hand rather than pulling in unist-util-visit — the
// traversal we need (split text nodes, skip code) is a few lines.
//
// Escape hatch: anything that doesn't parse as a wikilink (empty
// target, stray brackets) is left as literal text, never a broken
// link.

/** A parsed run of body text: either a literal stretch or a wikilink
 * with its resolution target and display label. */
export type WikilinkSegment =
  | { kind: "text"; value: string }
  | { kind: "link"; target: string; label: string };

// Inner content of a `[[…]]` — no nested brackets, so `[[a]][[b]]`
// splits into two links and a stray `[[` stays literal. The `+?` is
// lazy but the character class already forbids `]`, so it stops at the
// first `]]`.
const WIKILINK = /\[\[([^[\]]+)\]\]/g;

/**
 * Split one text string into literal and wikilink segments. Pure and
 * exported so the parsing rules (|label handling, adjacent links, the
 * unparseable-stays-literal escape hatch) can be unit-tested without a
 * full unified pipeline.
 */
export function wikilinkSegments(value: string): WikilinkSegment[] {
  const segments: WikilinkSegment[] = [];
  let lastIndex = 0;
  // Fresh lastIndex per call — the regex is module-level (one compile)
  // but stateful, so reset before iterating.
  WIKILINK.lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = WIKILINK.exec(value)) !== null) {
    if (match.index > lastIndex) {
      segments.push({ kind: "text", value: value.slice(lastIndex, match.index) });
    }
    const inner = match[1];
    const pipe = inner.indexOf("|");
    const target = (pipe >= 0 ? inner.slice(0, pipe) : inner).trim();
    const label = (pipe >= 0 ? inner.slice(pipe + 1) : inner).trim();
    if (target === "") {
      // Unparseable (e.g. `[[|x]]`, `[[ ]]`) — keep the raw text so the
      // reader shows exactly what the note author wrote.
      segments.push({ kind: "text", value: match[0] });
    } else {
      segments.push({ kind: "link", target, label: label || target });
    }
    lastIndex = WIKILINK.lastIndex;
  }
  if (lastIndex < value.length) {
    segments.push({ kind: "text", value: value.slice(lastIndex) });
  }
  return segments;
}

// Minimal mdast shapes — enough to walk and rewrite children without
// depending on `@types/mdast`. A node either carries `children` (we
// recurse) or, for text, a `value` (we may split it).
interface MdastNode {
  type: string;
  value?: string;
  url?: string;
  children?: MdastNode[];
  data?: Record<string, unknown>;
}

/** Build the mdast link node for a wikilink segment. The
 * `data.hProperties['data-wikilink']` rides through to the rendered
 * `<a>` so the Markdown anchor override can intercept the click; the
 * `href` is a harmless `#` (navigation is handled in JS, never the
 * browser). */
function linkNode(target: string, label: string): MdastNode {
  return {
    type: "link",
    // A real `url` is required: mdast-util-to-hast's link handler reads
    // `node.url` (and would crash on undefined) before hProperties get a
    // chance to override the rendered href. `#` is inert — navigation is
    // JS, driven off the `data-wikilink` marker below.
    url: "#",
    data: {
      hName: "a",
      hProperties: { "data-wikilink": target, href: "#" },
    },
    children: [{ type: "text", value: label }],
  };
}

function walk(node: MdastNode): void {
  if (!node.children) return;
  const rewritten: MdastNode[] = [];
  for (const child of node.children) {
    if (child.type === "text" && typeof child.value === "string") {
      const segments = wikilinkSegments(child.value);
      // A pure-text run (no wikilinks) yields a single text segment —
      // preserve the original node in that case rather than rebuild it.
      if (segments.length === 1 && segments[0].kind === "text") {
        rewritten.push(child);
        continue;
      }
      for (const segment of segments) {
        rewritten.push(
          segment.kind === "text"
            ? { type: "text", value: segment.value }
            : linkNode(segment.target, segment.label),
        );
      }
    } else {
      // `code`/`inlineCode` carry a `value` and no `children`, so they
      // are never descended into — wikilink syntax inside code stays
      // literal, as it should.
      walk(child);
      rewritten.push(child);
    }
  }
  node.children = rewritten;
}

/** The remark plugin. Register ahead of remark-gfm so wikilinks are
 * claimed before gfm's autolink pass sees the surrounding text. */
export function remarkWikilinks() {
  return (tree: MdastNode): void => {
    walk(tree);
  };
}
