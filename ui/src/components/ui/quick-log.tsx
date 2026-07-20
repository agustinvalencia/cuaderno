// A one-line composer that appends a log entry to TODAY's daily `## Logs`
// (append-only, via the `log_quick` command — the same verb the global
// capture window's Cmd/Ctrl+Enter fires). Shared by the Today/Home landing
// and the calendar's daily panel. On success it refetches today's daily note
// so a surface that renders the logs (the calendar panel) shows the new entry;
// `date` is today's ISO string, used for that invalidation (a no-op where the
// daily isn't cached, e.g. Home).
import { useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { errorMessage, logQuick } from "../../api/commands";
import { useToast } from "../../shell/Toasts";

export function QuickLog({ date }: { date: string }) {
  const client = useQueryClient();
  const { toast } = useToast();
  const [text, setText] = useState("");

  const log = useMutation({
    mutationFn: (entry: string) => logQuick(entry),
    onSuccess: () => {
      setText("");
      toast("Logged.");
      client.invalidateQueries({ queryKey: ["read_daily", date] });
    },
    onError: (error) => toast(errorMessage(error), "attention"),
  });

  return (
    <form
      aria-label="Add a log entry to today"
      className="flex gap-2"
      onSubmit={(event) => {
        event.preventDefault();
        const entry = text.trim();
        if (entry) log.mutate(entry);
      }}
    >
      <input
        value={text}
        onChange={(event) => setText(event.target.value)}
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
