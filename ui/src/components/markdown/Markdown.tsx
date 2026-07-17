// Rendered markdown for the note reader (plan §3.8). react-markdown +
// remark-gfm (tables, task lists, strikethrough) + remark-math/rehype-katex
// (LaTeX maths, the load-bearing case for research notes) + the wikilink
// plugin, with component overrides styled from the semantic tokens so notes
// read as part of the app, not a raw dump. KaTeX's stylesheet and fonts are
// vendored (imported below), never a CDN — the app CSP blocks external
// hosts.
import { createContext, useContext } from "react";
import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { useQuery } from "@tanstack/react-query";
import ReactMarkdown from "react-markdown";
import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import remarkBreaks from "remark-breaks";
import rehypeKatex from "rehype-katex";
import "katex/dist/katex.min.css";
import { openExternalUrl, readNoteAsset } from "../../api/commands";
import { remarkWikilinks } from "./remarkWikilinks";

/** An `http(s)`/`mailto` link the app can hand to the OS opener. Anything
 * else (a relative path, a bare fragment, an unknown scheme) has nothing safe
 * to open, so it stays inert. Mirrors the backend `is_openable_external_url`
 * allowlist — the backend re-validates before opening. */
function isOpenableExternalUrl(href: string): boolean {
  return /^(https?:\/\/|mailto:)/i.test(href.trimStart());
}

// The path of the note whose body is being rendered, so an embedded image
// (`![alt](assets/fig.png)`, whose src is relative to the note) can be
// resolved to vault bytes. Provided by the note surfaces that know their
// path (the centred note page, the calendar panel); absent elsewhere, in
// which case a relative image degrades to its caption rather than breaking.
const NotePathContext = createContext<string | null>(null);

export function NotePathProvider({
  path,
  children,
}: {
  path: string;
  children: ReactNode;
}) {
  return <NotePathContext.Provider value={path}>{children}</NotePathContext.Provider>;
}

type ImgProps = ComponentPropsWithoutRef<"img"> & { node?: unknown };

/** An embedded image. A `data:` src renders directly; an external URL
 * can't load (the webview has no outbound access), so it degrades to its
 * alt caption; a vault-relative src (with a note path in scope) is fetched
 * as bytes by the child `VaultImage`. Rendered with spans (not `<figure>`)
 * since react-markdown nests the image inside a `<p>`, where block elements
 * are invalid. */
function NoteImage({ src, alt, node: _node, ...props }: ImgProps) {
  const notePath = useContext(NotePathContext);
  if (typeof src === "string" && /^data:/i.test(src)) {
    return (
      <img
        src={src}
        alt={alt ?? ""}
        className="my-3 block max-w-full rounded border border-line"
        {...props}
      />
    );
  }
  const relative = typeof src === "string" && src !== "" && !/^https?:/i.test(src);
  // No fetch — and so no `useQuery`/QueryClient requirement — unless there
  // is genuinely a vault image to resolve.
  if (!relative || notePath === null) {
    return <span className="text-xs text-ink-faint italic">{alt || "image"}</span>;
  }
  return <VaultImage notePath={notePath} src={src} alt={alt} />;
}

/** Fetch a vault-relative image's bytes (as a `data:` URI) and render it
 * with its caption. Split out of `NoteImage` so the `useQuery` call — and
 * its QueryClient requirement — only exists when there is actually an
 * in-vault image to resolve. */
function VaultImage({
  notePath,
  src,
  alt,
}: {
  notePath: string;
  src: string;
  alt?: string;
}) {
  const asset = useQuery({
    queryKey: ["read_note_asset", notePath, src],
    queryFn: () => readNoteAsset(notePath, src),
    staleTime: Infinity,
  });
  if (asset.isPending) {
    return <span className="text-xs text-ink-faint">Loading image…</span>;
  }
  if (asset.isError || !asset.data) {
    return (
      <span className="text-xs text-ink-faint italic">
        [image unavailable{alt ? `: ${alt}` : ""}]
      </span>
    );
  }
  return (
    <span className="my-4 block">
      <img src={asset.data} alt={alt ?? ""} className="max-w-full rounded border border-line" />
      {alt && <span className="mt-1 block text-xs text-ink-muted">{alt}</span>}
    </span>
  );
}

// KaTeX options: never throw on a malformed expression — render the raw
// `$…$` source instead, in a calm desaturated tone (the vault's amber
// accent), so a typo degrades to legible source, not a red error box (the
// no-red design law). `strict: false` tolerates the LaTeX-isms real notes
// carry (e.g. `\ell`, stray `\,`) rather than warning on them.
const KATEX_OPTIONS = {
  throwOnError: false,
  errorColor: "var(--color-attention)",
  strict: false,
} as const;

/** An anchor carrying our wikilink marker calls back into the app;
 * every other anchor is an external URL we deliberately cannot open.
 * `node` is react-markdown's hast node — destructured out here (and in
 * every override below) so it never spreads onto a DOM element. */
type AnchorProps = ComponentPropsWithoutRef<"a"> & {
  "data-wikilink"?: string;
  node?: unknown;
};

function anchorComponent(onWikilink: (target: string) => void) {
  return function Anchor({
    "data-wikilink": wikilink,
    href,
    children,
    node: _node,
    ...props
  }: AnchorProps) {
    if (wikilink) {
      return (
        <a
          href="#"
          data-wikilink={wikilink}
          onClick={(event) => {
            event.preventDefault();
            onWikilink(wikilink);
          }}
          className="text-accent-interactive underline decoration-dotted underline-offset-2 hover:decoration-solid"
          {...props}
        >
          {children}
        </a>
      );
    }
    // An external http(s)/mailto link opens in the user's default browser
    // (or mail client) via the backend opener — the webview can't navigate
    // there itself, so we intercept the click and hand the URL off. The
    // backend re-validates the scheme before opening.
    if (typeof href === "string" && isOpenableExternalUrl(href)) {
      return (
        <a
          href={href}
          title={href}
          onClick={(event) => {
            event.preventDefault();
            void openExternalUrl(href);
          }}
          className="text-accent-interactive underline decoration-dotted underline-offset-2 hover:decoration-solid"
          {...props}
        >
          {children}
        </a>
      );
    }
    // A link with no openable scheme (a relative path, a bare fragment, an
    // unknown scheme) has nothing safe to open, so it stays inert muted text
    // with the target on hover — an honest non-affordance, not a dead link.
    return (
      <span title={href} className="text-ink-faint underline decoration-dotted underline-offset-2">
        {children}
      </span>
    );
  };
}

function markdownComponents(onWikilink: (target: string) => void): Components {
  // Every override destructures react-markdown's `node` (the hast
  // node) out of the props before spreading the rest onto the DOM
  // element — an unrecognised `node` attribute would otherwise trip
  // React's "unknown prop" warning and leak the parse tree into markup.
  return {
    a: anchorComponent(onWikilink),
    img: NoteImage,
    // Sizes are `em`-relative (not fixed `text-sm`/`text-lg`) so the whole
    // body scales with the reader's `--reader-font-size` wrapper below —
    // the Text size setting — while keeping the same visual proportions.
    h1: ({ node: _node, ...props }) => (
      <h1
        className="mt-4 mb-2 text-[length:var(--reader-heading-1)] font-semibold text-ink"
        {...props}
      />
    ),
    h2: ({ node: _node, ...props }) => (
      <h2
        className="mt-4 mb-2 text-[length:var(--reader-heading-2)] font-semibold text-ink"
        {...props}
      />
    ),
    h3: ({ node: _node, ...props }) => (
      <h3
        className="mt-3 mb-1 text-[length:var(--reader-heading-3)] font-semibold text-ink"
        {...props}
      />
    ),
    p: ({ node: _node, ...props }) => <p className="my-2 text-ink" {...props} />,
    ul: ({ node: _node, ...props }) => (
      <ul className="my-2 list-disc pl-5 text-ink" {...props} />
    ),
    ol: ({ node: _node, ...props }) => (
      <ol className="my-2 list-decimal pl-5 text-ink" {...props} />
    ),
    li: ({ node: _node, ...props }) => <li className="my-0.5" {...props} />,
    blockquote: ({ node: _node, ...props }) => (
      <blockquote className="my-2 border-l-2 border-line pl-3 text-ink-muted" {...props} />
    ),
    em: ({ node: _node, ...props }) => <em className="italic" {...props} />,
    strong: ({ node: _node, ...props }) => <strong className="font-semibold" {...props} />,
    code: ({ node: _node, ...props }) => (
      <code className="rounded bg-bg-sunken px-1 py-0.5 font-mono text-[0.85em] text-ink" {...props} />
    ),
    pre: ({ node: _node, ...props }) => (
      <pre
        className="my-2 overflow-x-auto rounded border border-line bg-bg-sunken p-3 font-mono text-[0.85em] text-ink"
        {...props}
      />
    ),
    // gfm tables can be wide; give them their own horizontal scroll so
    // the reader body never scrolls sideways.
    table: ({ node: _node, ...props }) => (
      <div className="my-3 overflow-x-auto">
        <table className="w-full border-collapse text-[0.95em] text-ink" {...props} />
      </div>
    ),
    th: ({ node: _node, ...props }) => (
      <th className="border border-line bg-bg-sunken px-2 py-1 text-left font-medium" {...props} />
    ),
    td: ({ node: _node, ...props }) => <td className="border border-line px-2 py-1" {...props} />,
    // gfm task-list checkboxes: read-only in the reader — the reader is
    // a lens, not an editor.
    input: ({ type, node: _node, ...props }) =>
      type === "checkbox" ? (
        <input type="checkbox" disabled className="mr-1 align-middle" {...props} />
      ) : (
        <input type={type} {...props} />
      ),
    hr: () => <hr className="my-4 border-line" />,
  };
}

export default function Markdown({
  body,
  onWikilink,
}: {
  body: string;
  onWikilink: (target: string) => void;
}) {
  return (
    // The reading-typography wrapper: base size / line-height / face come
    // from the `--reader-*` variables (globals.css), driven by the Text
    // size / Line spacing / Reading font settings. Every markdown surface
    // (reader, calendar, project & stewardship maps) inherits it, so notes
    // render consistently and respond to the settings everywhere. `code`
    // and `pre` re-assert `font-mono`, so they stay monospace regardless
    // of the chosen reading font.
    <div
      className="text-ink"
      style={{
        fontSize: "var(--reader-font-size)",
        lineHeight: "var(--reader-line-height)",
        fontFamily: "var(--reader-font-family)",
      }}
    >
      <ReactMarkdown
        // Wikilinks first so `[[…]]` is claimed before gfm autolinks the
        // surrounding text; remark-math parses `$…$` / `$$…$$` into math
        // nodes that rehype-katex then renders. remark-breaks renders a
        // single newline as a line break (a soft break becomes a hard `<br>`),
        // matching how Obsidian shows notes — so a standup's `Yesterday` /
        // `Today` / `Due soon` lines stay on their own lines rather than
        // collapsing into one paragraph.
        remarkPlugins={[remarkWikilinks, remarkGfm, remarkMath, remarkBreaks]}
        rehypePlugins={[[rehypeKatex, KATEX_OPTIONS]]}
        components={markdownComponents(onWikilink)}
      >
        {body}
      </ReactMarkdown>
    </div>
  );
}
