// The centred, full-page note reader (UI request 2026-07-13, replacing the
// right-anchored slide-in drawer). A note opens as a real page in the
// content area — the sidebar stays, back/forward navigate history — with a
// comfortable centred reading measure so a note reads like a document, not
// a cramped side panel. A Read/Edit toggle flips the same centred column
// between the rendered view (maths, wikilinks, sectioned body) and a
// CodeMirror source editor. Reached at `/note/<vault-path>`; every surface
// that used to summon the drawer now navigates here via `useReader`.
import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useParams } from "react-router";
import {
  errorMessage,
  openInEditor,
  readNote,
  readNoteRaw,
  resolveWikilink,
  writeNoteRaw,
} from "../../api/commands";
import { useToast } from "../../shell/Toasts";
import { useReader } from "../../shell/reader";
import { MetaPanel } from "../../components/markdown/MetaPanel";
import MarkdownEditor from "../../components/markdown/MarkdownEditor";
import { NotePathProvider } from "../../components/markdown/Markdown";
import SectionedBody from "../../components/markdown/SectionedBody";
import { splitBodySections } from "../../lib/noteContent";

/** The file stem of a vault path — `projects/foo.md` → `foo` — for
 * deriving a route slug from a resolved note path. */
function pathStem(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.md$/i, "");
}

export default function NotePage() {
  // The splat param is the vault path (`/note/portfolios/x/_index.md` →
  // `portfolios/x/_index.md`); slashes pass through a `*` route verbatim.
  // Key the reader by it so note→note navigation remounts a *fresh*
  // instance: a stale edit draft must never carry into — and overwrite — a
  // different note, and per-note scroll/focus reset come for free (the
  // route otherwise reuses one instance across notes).
  const path = useParams()["*"] ?? "";
  return <NoteView key={path} path={path} />;
}

function NoteView({ path }: { path: string }) {
  const navigate = useNavigate();
  const { openReader } = useReader();
  const { toast } = useToast();
  const client = useQueryClient();
  const rootRef = useRef<HTMLDivElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);

  // Fresh note (this component remounts per path): scroll the reader to the
  // top and move focus to the title, so keyboard/screen-reader users land on
  // the new note — the page model lost the drawer's dialog focus management.
  useEffect(() => {
    let scroller = rootRef.current?.parentElement ?? null;
    while (scroller && scroller.scrollHeight <= scroller.clientHeight) {
      scroller = scroller.parentElement;
    }
    scroller?.scrollTo({ top: 0 });
    headingRef.current?.focus({ preventScroll: true });
  }, []);

  const { data, isPending, isError } = useQuery({
    queryKey: ["read_note", path],
    queryFn: () => readNote(path),
    enabled: path !== "",
  });

  const openEditor = useMutation({
    mutationFn: () => openInEditor(path),
    onError: (error) => toast(errorMessage(error), "attention"),
  });

  // In-app edit mode (posture B): swap the rendered view for a CodeMirror
  // editor over the note's raw markdown. The draft lives in a ref so
  // keystrokes don't re-render; `null` = "not seeded this edit session yet",
  // so a background raw-refetch can't clobber in-progress edits.
  const [editing, setEditing] = useState(false);
  const draft = useRef<string | null>(null);
  const raw = useQuery({
    queryKey: ["read_note_raw", path],
    queryFn: () => readNoteRaw(path),
    enabled: editing,
  });
  // Seed the draft the FIRST time this edit session has the raw content —
  // once per session (guarded by the null), never on later raw.data changes.
  if (editing && draft.current === null && raw.data !== undefined) {
    draft.current = raw.data;
  }

  function startEditing() {
    draft.current = null;
    setEditing(true);
  }
  function stopEditing() {
    draft.current = null;
    setEditing(false);
  }

  const save = useMutation({
    mutationFn: () => writeNoteRaw(path, draft.current ?? ""),
    onError: (error) => toast(errorMessage(error), "attention"),
    onSuccess: () => {
      toast("Saved.");
      // Prime the raw cache with exactly what we wrote, so a re-edit starts
      // from the saved text rather than a stale pre-save entry.
      client.setQueryData(["read_note_raw", path], draft.current);
      stopEditing();
      // Refresh this note's rendered read; sibling surfaces refresh off the
      // `write_note_raw` command's emitted `vault:changed` event (the app's
      // usual path), so we don't global-invalidate (which would clobber the
      // raw prime).
      void client.invalidateQueries({ queryKey: ["read_note", path] });
    },
  });

  // A wikilink click resolves to a note (or nothing). Project and
  // stewardship targets get their own views; every other note navigates to
  // its own centred page. An unresolved target is a no-op — the anchor
  // already rendered muted (plan §3.8).
  async function onWikilink(target: string) {
    let resolved;
    try {
      resolved = await resolveWikilink(target);
    } catch {
      return;
    }
    if (!resolved) return;
    if (resolved.note_type === "project") {
      navigate(`/projects/${pathStem(resolved.path)}`);
    } else if (resolved.note_type === "stewardship") {
      navigate("/stewardships");
    } else {
      openReader(resolved.path);
    }
  }

  return (
    // Read: a comfortable centred measure (~72ch) so prose and maths read
    // like a document. Edit: a wider centred column so source lines and
    // line numbers have room.
    <div
      ref={rootRef}
      className={`mx-auto w-full px-6 pb-16 ${editing ? "max-w-5xl" : "max-w-[72ch]"}`}
    >
      {/* Sticky header: title plus the note's actions, pinned to the top of
          the scroll so Edit / Save stay reachable on a long note (they used
          to sit only at the very bottom). */}
      <div className="sticky top-0 z-10 mb-6 flex items-start justify-between gap-4 border-b border-line bg-bg-base pt-8 pb-3">
        <h1
          ref={headingRef}
          tabIndex={-1}
          className="min-w-0 flex-1 text-xl font-semibold text-ink outline-none"
        >
          {data?.title ?? pathStem(path)}
        </h1>
        <div className="flex shrink-0 items-center gap-2">
          {editing ? (
            <>
              <button
                type="button"
                onClick={() => {
                  // Empty-content floor: never clobber a note to empty — a
                  // whole-note clear is almost always a slip, not intent.
                  if (!draft.current?.trim()) {
                    toast("Nothing to save — the note is empty.", "attention");
                    return;
                  }
                  save.mutate();
                }}
                disabled={save.isPending || raw.data === undefined}
                className="rounded border border-line bg-bg-sunken px-3 py-1 text-sm text-ink hover:bg-bg-base"
              >
                Save
              </button>
              <button
                type="button"
                onClick={stopEditing}
                className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
              >
                Cancel
              </button>
            </>
          ) : (
            <>
              <button
                type="button"
                onClick={startEditing}
                className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
              >
                Edit
              </button>
              <button
                type="button"
                onClick={() => openEditor.mutate()}
                disabled={openEditor.isPending}
                className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
              >
                Open in editor
              </button>
            </>
          )}
          <button
            type="button"
            onClick={() => navigate(-1)}
            className="rounded px-2 py-1 text-xs text-ink-muted hover:text-ink"
          >
            ← back
          </button>
        </div>
      </div>

      {editing ? (
        <div className="flex min-h-[70vh] flex-col">
          {raw.isPending ? (
            <p className="text-sm text-ink-muted">Loading the note to edit…</p>
          ) : raw.isError || raw.data === undefined ? (
            <p className="text-sm text-ink-muted">
              This note could not be opened for editing.
            </p>
          ) : (
            <MarkdownEditor
              key={path}
              initialDoc={raw.data}
              onChange={(value) => {
                draft.current = value;
              }}
            />
          )}
        </div>
      ) : path === "" ? (
        <p className="text-sm text-ink-muted">No note selected.</p>
      ) : isPending ? (
        <p className="text-sm text-ink-muted">Reading the note…</p>
      ) : isError || !data ? (
        <p className="text-sm text-ink-muted">This note could not be read.</p>
      ) : (
        <NotePathProvider path={path}>
          <MetaPanel frontmatter={data.frontmatter} className="mb-6" />
          <SectionedBody sections={splitBodySections(data.body)} onWikilink={onWikilink} />
        </NotePathProvider>
      )}
    </div>
  );
}
