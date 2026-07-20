// A CodeMirror 6 markdown editor — the spike for in-app editing (posture
// B: free editing, lint/reconcile as the guardrail). CM6 is the engine
// Obsidian itself uses; it edits the source bytes directly (no lossy
// WYSIWYG round-trip), which is what a markdown-source-of-truth vault
// edited from several tools needs. Themed entirely through cuaderno's CSS
// tokens, so it follows light/dark for free.
import { useEffect, useRef } from "react";
import { EditorState } from "@codemirror/state";
import {
  EditorView,
  keymap,
  lineNumbers,
  highlightActiveLine,
  placeholder as placeholderExtension,
} from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
} from "@codemirror/commands";
import { markdown } from "@codemirror/lang-markdown";
import {
  defaultHighlightStyle,
  syntaxHighlighting,
} from "@codemirror/language";

const theme = EditorView.theme({
  "&": {
    color: "var(--color-ink)",
    backgroundColor: "var(--color-bg-surface)",
    // Tokenised so the Text size setting scales the editor with the reader.
    fontSize: "var(--editor-font-size)",
    height: "100%",
  },
  ".cm-scroller": {
    // The shared mono token, so the editor tracks the app's mono face.
    fontFamily: "var(--font-mono)",
    lineHeight: "1.55",
  },
  ".cm-content": {
    caretColor: "var(--color-accent-interactive)",
    padding: "0.5rem 0",
  },
  "&.cm-focused": { outline: "none" },
  ".cm-cursor, .cm-dropCursor": {
    borderLeftColor: "var(--color-accent-interactive)",
  },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection":
    {
      backgroundColor: "var(--color-bg-sunken)",
    },
  ".cm-gutters": {
    backgroundColor: "var(--color-bg-base)",
    color: "var(--color-ink-faint)",
    border: "none",
  },
  ".cm-activeLine": {
    backgroundColor:
      "color-mix(in oklch, var(--color-bg-sunken) 55%, transparent)",
  },
  ".cm-activeLineGutter": { backgroundColor: "var(--color-bg-sunken)" },
});

export default function MarkdownEditor({
  initialDoc,
  onChange,
  ariaLabel,
  placeholder,
  autoFocus = true,
}: {
  /** Seed content — read once at mount; the editor is uncontrolled after. */
  initialDoc: string;
  onChange: (value: string) => void;
  /** Accessible name for the editing surface (goes on the CM content). */
  ariaLabel?: string;
  /** Calm prompt shown while the editor is empty. */
  placeholder?: string;
  /** Focus the editor on mount. Right for a dedicated edit surface (the note
   * reader); off where the editor is one field among others (a review step). */
  autoFocus?: boolean;
}) {
  const host = useRef<HTMLDivElement>(null);
  // Keep onChange fresh without recreating the editor (which would lose
  // cursor/history) — the update listener reads the latest via the ref.
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    const parent = host.current;
    if (!parent) return;
    const extensions = [
      lineNumbers(),
      highlightActiveLine(),
      history(),
      keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
      markdown(),
      syntaxHighlighting(defaultHighlightStyle),
      EditorView.lineWrapping,
      theme,
      EditorView.updateListener.of((update) => {
        if (update.docChanged) onChangeRef.current(update.state.doc.toString());
      }),
    ];
    if (ariaLabel) {
      extensions.push(EditorView.contentAttributes.of({ "aria-label": ariaLabel }));
    }
    if (placeholder) {
      extensions.push(placeholderExtension(placeholder));
    }
    const view = new EditorView({
      parent,
      state: EditorState.create({ doc: initialDoc, extensions }),
    });
    if (autoFocus) view.focus();
    return () => view.destroy();
    // Seed once — later prop changes shouldn't blow away in-progress edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      ref={host}
      className="h-full min-h-0 overflow-hidden rounded border border-line"
    />
  );
}
