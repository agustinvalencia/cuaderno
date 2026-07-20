// A one-line composer that appends a log entry to TODAY's daily `## Logs`
// (append-only, via the `log_quick` command — the same verb the global
// capture window's Cmd/Ctrl+Enter fires). Shared by the Today/Home landing
// and the calendar's daily panel. On success it refetches today's daily note
// so a surface that renders the logs (the calendar panel) shows the new entry;
// `date` is today's ISO string, used for that invalidation (a no-op where the
// daily isn't cached, e.g. Home).
import { useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { errorMessage, logQuick } from "../../api/commands";
import { useToast } from "../../shell/Toasts";

export function QuickLog({ date }: { date: string }) {
  const client = useQueryClient();
  const { toast } = useToast();
  const [text, setText] = useState("");
  // A synchronous in-flight guard: a second submit (a fast or held Enter)
  // arriving before the write resolves must not re-send the same not-yet-
  // cleared text and append a duplicate log line — the button's `disabled`
  // only takes effect a render later. Mirrors the capture bar's guard.
  const sending = useRef(false);

  const log = useMutation({
    mutationFn: (entry: string) => logQuick(entry),
    onSuccess: (_data, entry) => {
      // Clear only if the field still holds exactly what we sent, so anything
      // typed during the brief in-flight window isn't discarded.
      setText((current) => (current.trim() === entry ? "" : current));
      toast("Logged.");
      client.invalidateQueries({ queryKey: ["read_daily", date] });
    },
    onError: (error) => toast(errorMessage(error), "attention"),
    onSettled: () => {
      sending.current = false;
    },
  });

  function submit() {
    const entry = text.trim();
    if (!entry || sending.current) return;
    sending.current = true;
    log.mutate(entry);
  }

  return (
    <form
      aria-label="Add a log entry to today"
      className="flex gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        submit();
      }}
    >
      <input
        value={text}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          // A held Enter autorepeats keydown; ignore the repeats so one
          // keypress is one log entry.
          if (event.key === "Enter" && event.repeat) event.preventDefault();
        }}
        aria-label="Log entry"
        placeholder="Add a log entry…"
        className="min-w-0 flex-1 rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
      />
      <button
        type="submit"
        disabled={log.isPending || text.trim() === ""}
        className="shrink-0 rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken disabled:opacity-50"
      >
        Add log
      </button>
    </form>
  );
}
