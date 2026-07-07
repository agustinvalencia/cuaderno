// The slide-in note reader (plan §1.0, §3.8): a 380px Radix-backed
// panel showing any vault note rendered — title, flat frontmatter
// chips, the markdown body, and an "Open in editor" footer for the deep
// edits the app deliberately doesn't do. Wikilink clicks resolve to
// typed navigation (project / stewardship views) or replace the reader
// with the linked note.
import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { errorMessage, openInEditor, readNote, resolveWikilink } from "../../api/commands";
import { useToast } from "../../shell/Toasts";
import { Sheet, SheetContent, SheetTitle } from "../ui/sheet";
import Markdown from "./Markdown";

/** The file stem of a vault path — `projects/foo.md` → `foo` — for
 * deriving a route slug from a resolved note path. */
function pathStem(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.md$/i, "");
}

/** Flat scalar frontmatter as `key: value` chips. Objects and arrays
 * are skipped — a chip row is for at-a-glance metadata (type, context,
 * created), not nested structure. Returns `[]` for anything that isn't
 * a plain object (the wire type is `unknown`). */
function frontmatterChips(frontmatter: unknown): [string, string][] {
  if (!frontmatter || typeof frontmatter !== "object" || Array.isArray(frontmatter)) {
    return [];
  }
  const chips: [string, string][] = [];
  for (const [key, value] of Object.entries(frontmatter)) {
    if (value === null) continue;
    const scalar =
      typeof value === "string" || typeof value === "number" || typeof value === "boolean";
    if (scalar) chips.push([key, String(value)]);
  }
  return chips;
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
  const { data, isPending, isError } = useQuery({
    queryKey: ["read_note", path],
    queryFn: () => readNote(path),
  });

  const openEditor = useMutation({
    mutationFn: () => openInEditor(path),
    onError: (error) => toast(errorMessage(error), "attention"),
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

  const chips = frontmatterChips(data?.frontmatter);

  return (
    <Sheet open onOpenChange={(open) => !open && onClose()}>
      <SheetContent className="w-[380px] max-w-[90vw]" aria-describedby={undefined}>
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

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          {isPending ? (
            <p className="text-sm text-ink-muted">Reading the note…</p>
          ) : isError || !data ? (
            <p className="text-sm text-ink-muted">This note could not be read.</p>
          ) : (
            <>
              {chips.length > 0 && (
                <div className="mb-4 flex flex-wrap gap-1.5">
                  {chips.map(([key, value]) => (
                    <span
                      key={key}
                      className="rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-muted"
                    >
                      {key}: {value}
                    </span>
                  ))}
                </div>
              )}
              <Markdown body={data.body} onWikilink={onWikilink} />
            </>
          )}
        </div>

        <div className="border-t border-line px-5 py-3">
          <button
            type="button"
            onClick={() => openEditor.mutate()}
            disabled={openEditor.isPending}
            className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
          >
            Open in editor
          </button>
        </div>
      </SheetContent>
    </Sheet>
  );
}
