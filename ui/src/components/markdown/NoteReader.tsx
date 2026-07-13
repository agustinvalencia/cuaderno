// The slide-in note reader (plan §1.0, §3.8): a 380px Radix-backed
// panel showing any vault note rendered — title, a separated `MetaPanel`
// metadata strip, the sectioned body (shared `SectionedBody`, the same
// rendering the calendar panel uses — titled `##` sections and `## Logs`
// as timestamped cards), and an "Open in editor" footer for the deep
// edits the app deliberately doesn't do. Wikilink clicks resolve to
// typed navigation (project / stewardship views) or replace the reader
// with the linked note.
import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import {
  errorMessage,
  openInEditor,
  readNote,
  readNoteRaw,
  resolveWikilink,
  writeNoteRaw,
} from "../../api/commands";
import { useToast } from "../../shell/Toasts";
import { Sheet, SheetContent, SheetTitle } from "../ui/sheet";
import { MetaPanel } from "./MetaPanel";
import MarkdownEditor from "./MarkdownEditor";
import SectionedBody from "./SectionedBody";
import { splitBodySections } from "../../lib/noteContent";

/** The file stem of a vault path — `projects/foo.md` → `foo` — for
 * deriving a route slug from a resolved note path. */
function pathStem(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.md$/i, "");
}

export default function NoteReader({
  path,
  onClose,
  onNavigate,
}: {
  path: string;
  onClose: () => void;
  /** Replace the reader with a different note (a wikilink to a plain
   * note, resolved). Typed nav (project/stewardship) routes instead. */
  onNavigate: (path: string) => void;
}) {
  const navigate = useNavigate();
  const { toast } = useToast();
  const client = useQueryClient();
  const { data, isPending, isError } = useQuery({
    queryKey: ["read_note", path],
    queryFn: () => readNote(path),
  });

  const openEditor = useMutation({
    mutationFn: () => openInEditor(path),
    onError: (error) => toast(errorMessage(error), "attention"),
  });

  // In-app edit mode (spike, posture B): swap the read view for a
  // CodeMirror editor over the note's raw markdown. The draft lives in a
  // ref so keystrokes don't re-render; `null` = "not yet seeded this edit
  // session", so a background raw-refetch can't clobber in-progress edits.
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
      // Refresh this note's display read. Sibling surfaces the reconcile
      // touched (backlinks, search) refresh off the `write_note_raw`
      // command's emitted `vault:changed` event — the app's usual path — so
      // we don't global-invalidate here (which would clobber the raw prime).
      void client.invalidateQueries({ queryKey: ["read_note", path] });
    },
  });

  // A wikilink click resolves to a note (or nothing). Project and
  // stewardship targets get their own views and close the reader; every
  // other note replaces the reader in place. An unresolved target is a
  // no-op — the anchor already rendered muted, and a toast per dead
  // link would be noise (plan §3.8).
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
      onClose();
    } else if (resolved.note_type === "stewardship") {
      navigate("/stewardships");
      onClose();
    } else {
      onNavigate(resolved.path);
    }
  }

  return (
    <Sheet open onOpenChange={(open) => !open && onClose()}>
      <SheetContent
        className={
          editing ? "w-[760px] max-w-[95vw]" : "w-[380px] max-w-[90vw]"
        }
        aria-describedby={undefined}
        // While editing, Esc / click-outside must not silently discard the
        // draft — require an explicit Save or Cancel.
        onEscapeKeyDown={editing ? (event) => event.preventDefault() : undefined}
        onInteractOutside={editing ? (event) => event.preventDefault() : undefined}
      >
        <div className="flex items-start justify-between border-b border-line px-5 py-4">
          <SheetTitle className="min-w-0 flex-1 truncate pr-2 text-sm font-semibold text-ink">
            {data?.title ?? path}
          </SheetTitle>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close note reader"
            className="shrink-0 rounded px-2 py-1 text-xs text-ink-muted hover:text-ink"
          >
            close
          </button>
        </div>

        {editing ? (
          <div className="flex min-h-0 flex-1 flex-col px-4 py-3">
            {raw.isPending ? (
              <p className="text-sm text-ink-muted">
                Loading the note to edit…
              </p>
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
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
            {isPending ? (
              <p className="text-sm text-ink-muted">Reading the note…</p>
            ) : isError || !data ? (
              <p className="text-sm text-ink-muted">
                This note could not be read.
              </p>
            ) : (
              <>
                <MetaPanel frontmatter={data.frontmatter} className="mb-5" />
                <SectionedBody
                  sections={splitBodySections(data.body)}
                  onWikilink={onWikilink}
                />
              </>
            )}
          </div>
        )}

        <div className="flex items-center gap-2 border-t border-line px-5 py-3">
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
        </div>
      </SheetContent>
    </Sheet>
  );
}
