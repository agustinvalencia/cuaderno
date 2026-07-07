// Rendered markdown for the note reader (plan §3.8). react-markdown +
// remark-gfm (tables, task lists, strikethrough) + the wikilink plugin,
// with component overrides styled from the semantic tokens so notes
// read as part of the app, not a raw dump.
import type { ComponentPropsWithoutRef } from "react";
import ReactMarkdown from "react-markdown";
import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import { remarkWikilinks } from "./remarkWikilinks";

/** An anchor carrying our wikilink marker calls back into the app;
 * every other anchor is an external URL we deliberately cannot open. */
type AnchorProps = ComponentPropsWithoutRef<"a"> & { "data-wikilink"?: string };

function anchorComponent(onWikilink: (target: string) => void) {
  return function Anchor({ "data-wikilink": wikilink, href, children, ...props }: AnchorProps) {
    if (wikilink) {
      return (
        <a
          href="#"
          data-wikilink={wikilink}
          onClick={(event) => {
            event.preventDefault();
            onWikilink(wikilink);
          }}
          className="text-ctx-work underline decoration-dotted underline-offset-2 hover:decoration-solid"
          {...props}
        >
          {children}
        </a>
      );
    }
    // External links render as muted text with the URL as a title.
    // The Tauri webview has no shell-open capability wired (no opener
    // scope for arbitrary URLs, by design — read-mostly, no browser
    // launching), so an anchor here would be a dead click; showing the
    // URL on hover keeps the information without the false affordance.
    return (
      <span title={href} className="text-ink-faint underline decoration-dotted underline-offset-2">
        {children}
      </span>
    );
  };
}

function markdownComponents(onWikilink: (target: string) => void): Components {
  return {
    a: anchorComponent(onWikilink),
    h1: (props) => <h1 className="mt-4 mb-2 text-lg font-semibold text-ink" {...props} />,
    h2: (props) => <h2 className="mt-4 mb-2 text-base font-semibold text-ink" {...props} />,
    h3: (props) => <h3 className="mt-3 mb-1 text-sm font-semibold text-ink" {...props} />,
    p: (props) => <p className="my-2 text-sm leading-relaxed text-ink" {...props} />,
    ul: (props) => <ul className="my-2 list-disc pl-5 text-sm text-ink" {...props} />,
    ol: (props) => <ol className="my-2 list-decimal pl-5 text-sm text-ink" {...props} />,
    li: (props) => <li className="my-0.5" {...props} />,
    blockquote: (props) => (
      <blockquote className="my-2 border-l-2 border-line pl-3 text-sm text-ink-muted" {...props} />
    ),
    em: (props) => <em className="italic" {...props} />,
    strong: (props) => <strong className="font-semibold" {...props} />,
    code: (props) => (
      <code className="rounded bg-bg-sunken px-1 py-0.5 font-mono text-xs text-ink" {...props} />
    ),
    pre: (props) => (
      <pre
        className="my-2 overflow-x-auto rounded border border-line bg-bg-sunken p-3 font-mono text-xs text-ink"
        {...props}
      />
    ),
    // gfm tables can be wide; give them their own horizontal scroll so
    // the reader body never scrolls sideways.
    table: (props) => (
      <div className="my-3 overflow-x-auto">
        <table className="w-full border-collapse text-sm text-ink" {...props} />
      </div>
    ),
    th: (props) => (
      <th className="border border-line bg-bg-sunken px-2 py-1 text-left font-medium" {...props} />
    ),
    td: (props) => <td className="border border-line px-2 py-1" {...props} />,
    // gfm task-list checkboxes: read-only in the reader — the reader is
    // a lens, not an editor.
    input: ({ type, ...props }) =>
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
    <ReactMarkdown
      // Wikilinks first so `[[…]]` is claimed before gfm autolinks the
      // surrounding text.
      remarkPlugins={[remarkWikilinks, remarkGfm]}
      components={markdownComponents(onWikilink)}
    >
      {body}
    </ReactMarkdown>
  );
}
