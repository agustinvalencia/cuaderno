// The in-app editor for `.cuaderno/custom.css` (the "Edit in app" action).
// A CodeMirror CSS editor in a wide dialog: it loads the file (seeding the
// documented template the first time, via init_custom_css), and Save writes
// it and re-injects the stylesheet so the change applies immediately — no
// leaving the app, no window refocus. "Edit in editor" (the sibling action)
// hands the same file to the user's external editor instead.
import { useEffect, useRef, useState } from "react";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, lineNumbers, highlightActiveLine } from "@codemirror/view";
import {
  defaultKeymap,
  history,
  historyKeymap,
  indentWithTab,
} from "@codemirror/commands";
import { css } from "@codemirror/lang-css";
import { defaultHighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { Dialog, DialogContent, DialogTitle } from "../components/ui/dialog";
import { errorMessage, initCustomCss, writeCustomCss } from "../api/commands";
import { loadCustomCss } from "../lib/customCss";
import { useToast } from "./Toasts";

// Themed through the app's tokens, like MarkdownEditor, so it follows
// light/dark and the active palette for free.
const editorTheme = EditorView.theme({
  "&": {
    color: "var(--color-ink)",
    backgroundColor: "var(--color-bg-surface)",
    fontSize: "var(--editor-font-size)",
    height: "100%",
  },
  ".cm-scroller": {
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
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, ::selection": {
    backgroundColor: "var(--color-bg-sunken)",
  },
  ".cm-gutters": {
    backgroundColor: "var(--color-bg-base)",
    color: "var(--color-ink-faint)",
    border: "none",
  },
  ".cm-activeLine": {
    backgroundColor: "color-mix(in oklch, var(--color-bg-sunken) 55%, transparent)",
  },
  ".cm-activeLineGutter": { backgroundColor: "var(--color-bg-sunken)" },
});

function CssEditor({
  initialDoc,
  onChange,
}: {
  initialDoc: string;
  onChange: (value: string) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;

  useEffect(() => {
    const parent = host.current;
    if (!parent) return;
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: initialDoc,
        extensions: [
          lineNumbers(),
          highlightActiveLine(),
          history(),
          keymap.of([...defaultKeymap, ...historyKeymap, indentWithTab]),
          css(),
          syntaxHighlighting(defaultHighlightStyle),
          EditorView.lineWrapping,
          editorTheme,
          EditorView.updateListener.of((update) => {
            if (update.docChanged) onChangeRef.current(update.state.doc.toString());
          }),
        ],
      }),
    });
    view.focus();
    return () => view.destroy();
    // Seed once — later prop changes shouldn't blow away in-progress edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return <div ref={host} className="h-full min-h-0 overflow-hidden rounded border border-line" />;
}

export default function CustomCssEditor({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { toast } = useToast();
  // `null` = not loaded yet; the editor mounts only once we have content, so
  // it seeds cleanly (the editor is uncontrolled after mount).
  const [initial, setInitial] = useState<string | null>(null);
  const draft = useRef("");
  const [saving, setSaving] = useState(false);

  // This component is mounted only while open (SettingsDialog gates it on
  // `cssEditorOpen`), so it loads once on mount; close is an unmount. The
  // `alive` guard drops a late resolve (and StrictMode's double-mount).
  useEffect(() => {
    let alive = true;
    initCustomCss()
      .then((content) => {
        if (!alive) return;
        draft.current = content;
        setInitial(content);
      })
      .catch((error) => {
        if (!alive) return;
        toast(errorMessage(error), "attention");
        onOpenChange(false);
      });
    return () => {
      alive = false;
    };
    // Load once on mount; toast/onOpenChange are stable enough for this.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function save() {
    setSaving(true);
    try {
      await writeCustomCss(draft.current);
      await loadCustomCss(); // apply immediately
      toast("Custom CSS applied.");
      onOpenChange(false);
    } catch (error) {
      toast(errorMessage(error), "attention");
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        aria-describedby={undefined}
        className="h-[80vh]"
        style={{ width: "min(56rem, calc(100vw - 2rem))" }}
      >
        <DialogTitle className="text-base font-semibold text-ink">Custom CSS</DialogTitle>
        <p className="mt-1 text-xs text-ink-faint">
          Redefines any theme token in .cuaderno/custom.css. Applied on Save.
        </p>

        <div className="mt-3 min-h-0 flex-1">
          {initial === null ? (
            <p className="text-sm text-ink-muted">Loading…</p>
          ) : (
            <CssEditor initialDoc={initial} onChange={(value) => (draft.current = value)} />
          )}
        </div>

        <div className="mt-4 flex justify-end gap-2">
          <button
            type="button"
            onClick={() => onOpenChange(false)}
            className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => void save()}
            disabled={saving || initial === null}
            className="rounded border border-line bg-bg-sunken px-3 py-1 text-sm text-ink hover:bg-bg-base disabled:opacity-50"
          >
            {saving ? "Saving…" : "Save"}
          </button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
