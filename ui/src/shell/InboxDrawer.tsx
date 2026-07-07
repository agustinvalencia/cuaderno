// Right-side inbox drawer (plan §1.0): the visible landing place that
// makes global capture trustworthy. Lists uncategorised captures with
// per-item open-in-editor and discard; full triage stays a CLI/Claude
// concern. Deliberately hand-rolled focus management — no Radix
// (shadcn primitives are deferred to M5): the panel takes focus on
// open, Escape closes, and focus returns to the toggle button.
import { useEffect, useRef } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { InboxItem } from "../api/bindings/InboxItem";
import { discardInboxItem, errorMessage, listInbox, openInEditor } from "../api/commands";
import { useToast } from "./Toasts";

/** The `inbox/<slug>.md` note path for open-in-editor. */
function notePath(slug: string): string {
  return `inbox/${slug}.md`;
}

/** The leading `YYYY-MM-DD` of an inbox slug, formatted friendly, or
 * empty when the stem doesn't start with a date. Parsed at local
 * midnight so the day never slips a timezone. */
function capturedDate(slug: string): string {
  const match = /^(\d{4}-\d{2}-\d{2})/.exec(slug);
  if (!match) return "";
  return new Date(`${match[1]}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}

export default function InboxDrawer({
  onClose,
  returnFocusRef,
}: {
  onClose: () => void;
  returnFocusRef: React.RefObject<HTMLButtonElement | null>;
}) {
  const client = useQueryClient();
  const { toast } = useToast();
  const panelRef = useRef<HTMLDivElement>(null);

  const {
    data: items = [],
    isPending,
    isError,
  } = useQuery({ queryKey: ["list_inbox"], queryFn: listInbox });

  // Focus the panel on open; restore focus to the toggle on close so
  // keyboard users aren't dumped at the top of the document.
  useEffect(() => {
    panelRef.current?.focus();
    const toReturn = returnFocusRef.current;
    return () => toReturn?.focus();
  }, [returnFocusRef]);

  // Optimistic discard, mirroring ProjectCard's complete flow: drop the
  // row immediately, roll back and toast on error, reconcile on settle.
  const discard = useMutation({
    mutationFn: (slug: string) => discardInboxItem(slug),
    onMutate: async (slug) => {
      await client.cancelQueries({ queryKey: ["list_inbox"] });
      const previous = client.getQueryData<InboxItem[]>(["list_inbox"]);
      client.setQueryData<InboxItem[]>(["list_inbox"], (list) =>
        (list ?? []).filter((item) => item.slug !== slug),
      );
      return { previous };
    },
    onError: (error, _slug, context) => {
      if (context?.previous) client.setQueryData(["list_inbox"], context.previous);
      toast(errorMessage(error), "attention");
    },
    onSettled: () => client.invalidateQueries({ queryKey: ["list_inbox"] }),
  });

  const open = useMutation({
    mutationFn: (slug: string) => openInEditor(notePath(slug)),
    onError: (error) => toast(errorMessage(error), "attention"),
  });

  return (
    <div
      ref={panelRef}
      // Non-modal disclosure, not a dialog: there's no focus trap and
      // no aria-modal, so `role="dialog"` would over-promise. It's a
      // labelled region the user can Tab out of.
      role="region"
      aria-label="Inbox"
      tabIndex={-1}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          onClose();
        }
      }}
      className="fixed inset-y-0 right-0 z-40 flex w-80 flex-col border-l border-line bg-bg-surface shadow-lg outline-none"
    >
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <h2 className="text-sm font-semibold text-ink">Inbox</h2>
        <button
          type="button"
          onClick={onClose}
          aria-label="Close inbox"
          className="rounded px-2 py-1 text-xs text-ink-muted hover:text-ink"
        >
          close
        </button>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {isPending ? (
          <p className="text-sm text-ink-muted">Reading the inbox…</p>
        ) : isError ? (
          <p className="text-sm text-ink-muted">The inbox could not be read.</p>
        ) : items.length === 0 ? (
          <p className="text-sm text-ink-muted">Inbox zero. Nothing waiting on you.</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {items.map((item) => (
              <li key={item.slug} className="rounded border border-line bg-bg-base p-3">
                <p className="text-sm text-ink">{item.text || "(empty capture)"}</p>
                <div className="mt-2 flex items-center justify-between">
                  <span className="text-xs text-ink-faint">{capturedDate(item.slug)}</span>
                  <div className="flex gap-1">
                    <button
                      type="button"
                      onClick={() => open.mutate(item.slug)}
                      className="rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
                    >
                      open in editor
                    </button>
                    <button
                      type="button"
                      onClick={() => discard.mutate(item.slug)}
                      aria-label={item.text ? `Discard: ${item.text}` : "Discard empty capture"}
                      className="rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
                    >
                      discard
                    </button>
                  </div>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      <p className="border-t border-line px-4 py-3 text-xs text-ink-faint">
        Triage happens in the CLI or with Claude.
      </p>
    </div>
  );
}
