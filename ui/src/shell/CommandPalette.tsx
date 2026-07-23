// Command palette (plan §1.0, §5) — Cmd+K opens a jump-to-note search
// (the load-bearing half of a pull-optimised tool) plus static view
// navigation, and two verbs: "Capture…" and "Log to daily…", each a
// one-line submit into the vault. Built on cmdk for list navigation
// inside a Radix dialog (focus trap, Esc, return-focus). Styled from
// the semantic tokens over cmdk's unstyled primitives.
import { useEffect, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useNavigate } from "react-router";
import { Command } from "cmdk";
import * as Dialog from "@radix-ui/react-dialog";
import type { SearchResultEntry } from "../api/bindings/SearchResultEntry";
import { captureQuick, errorMessage, logQuick, searchVault } from "../api/commands";
import { useReader } from "./reader";
import type { SettingsSection } from "./SettingsDialog";
import { useToast } from "./Toasts";

/** Static navigation targets — the visible views, jumpable by name, in
 * the sidebar's own order so the palette teaches the same shape (#444). */
const NAV_ITEMS: { label: string; to: string }[] = [
  { label: "Today", to: "/" },
  { label: "Calendar", to: "/calendar" },
  { label: "Weekly", to: "/weekly" },
  { label: "Monthly", to: "/monthly" },
  { label: "Actions", to: "/actions" },
  { label: "Commitments", to: "/commitments" },
  { label: "Stewardships", to: "/stewardships" },
  { label: "Questions", to: "/questions" },
  { label: "Portfolios", to: "/portfolios" },
];

/** The settings sections the palette can open directly.
 *
 * Templates and Vault config left the sidebar for `Cmd+,` (#444), so
 * without these two entries the only way to reach them by name would be
 * typing a URL. They open the dialog at that section rather than
 * navigating, since that is where they now live. */
const SETTINGS_ITEMS: { label: string; section: SettingsSection }[] = [
  { label: "Settings…", section: "appearance" },
  { label: "Vault config…", section: "config" },
  { label: "Templates…", section: "templates" },
];

/** Debounce window before a keystroke turns into a search invoke — long
 * enough to skip mid-word noise, short enough to feel live (plan §5). */
const SEARCH_DEBOUNCE_MS = 150;

/** `projects/foo.md` → `foo`, for routing a project search hit. */
function pathStem(path: string): string {
  return (path.split("/").pop() ?? path).replace(/\.md$/i, "");
}

type Mode = "root" | "capture" | "log";

export default function CommandPalette({
  open,
  onOpenChange,
  onOpenSettings,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onOpenSettings: (section: SettingsSection) => void;
}) {
  const navigate = useNavigate();
  const { openReader } = useReader();
  const { toast } = useToast();
  const [mode, setMode] = useState<Mode>("root");
  const [search, setSearch] = useState("");
  const [debounced, setDebounced] = useState("");
  const verbInput = useRef<HTMLInputElement>(null);

  // Reset to a clean root state whenever the palette closes, so it never
  // reopens mid-verb or showing a stale query.
  useEffect(() => {
    if (!open) {
      setMode("root");
      setSearch("");
      setDebounced("");
    }
  }, [open]);

  useEffect(() => {
    const timer = setTimeout(() => setDebounced(search), SEARCH_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [search]);

  const { data: results = [] } = useQuery({
    queryKey: ["search_vault", debounced],
    queryFn: () => searchVault(debounced),
    enabled: open && mode === "root" && debounced.trim().length > 0,
  });

  const capture = useMutation({
    mutationFn: (text: string) => captureQuick(text),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => {
      toast("Captured to your inbox.");
      onOpenChange(false);
    },
  });

  const log = useMutation({
    mutationFn: (text: string) => logQuick(text),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => {
      toast("Logged to today.");
      onOpenChange(false);
    },
  });

  function go(to: string) {
    navigate(to);
    onOpenChange(false);
  }

  function openResult(result: SearchResultEntry) {
    if (result.note_type === "project") {
      go(`/projects/${pathStem(result.path)}`);
    } else if (result.note_type === "stewardship") {
      go("/stewardships");
    } else {
      openReader(result.path);
      onOpenChange(false);
    }
  }

  // Static nav items filter live against the typed query; a blank query
  // shows them all.
  const query = search.trim().toLowerCase();
  const navMatches = query
    ? NAV_ITEMS.filter((item) => item.label.toLowerCase().includes(query))
    : NAV_ITEMS;
  const settingsMatches = query
    ? SETTINGS_ITEMS.filter((item) => item.label.toLowerCase().includes(query))
    : SETTINGS_ITEMS;

  const verbMode = mode !== "root";

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Overlay className="fixed inset-0 z-40 bg-black/20" />
        <Dialog.Content
          aria-label="Command palette"
          aria-describedby={undefined}
          className="fixed left-1/2 top-24 z-50 w-[min(36rem,90vw)] -translate-x-1/2 overflow-hidden rounded-lg border border-line bg-bg-surface shadow-lg outline-none"
        >
          <Dialog.Title className="sr-only">Command palette</Dialog.Title>

          {verbMode ? (
            <form
              onSubmit={(event) => {
                event.preventDefault();
                const text = verbInput.current?.value.trim();
                if (!text) return;
                (mode === "capture" ? capture : log).mutate(text);
              }}
            >
              <input
                ref={verbInput}
                autoFocus
                type="text"
                aria-label={mode === "capture" ? "Capture to inbox" : "Log to daily"}
                placeholder={mode === "capture" ? "Capture a thought…" : "Log a line to today…"}
                className="w-full border-b border-line bg-transparent px-4 py-3 text-sm text-ink outline-none placeholder:text-ink-faint"
              />
              {/* Esc is owned by the Radix dialog and closes the whole
                  palette (not just the verb) — the copy says so honestly
                  rather than promising a back-to-root that isn't wired. */}
              <p className="px-4 py-2 text-xs text-ink-faint">
                {mode === "capture" ? "Enter to capture" : "Enter to log"} · Esc closes
              </p>
            </form>
          ) : (
            <Command shouldFilter={false} loop>
              <Command.Input
                autoFocus
                value={search}
                onValueChange={setSearch}
                placeholder="Search notes or jump to a view…"
                className="w-full border-b border-line bg-transparent px-4 py-3 text-sm text-ink outline-none placeholder:text-ink-faint"
              />
              <Command.List className="max-h-80 overflow-y-auto p-2">
                <Command.Empty className="px-2 py-3 text-sm text-ink-muted">
                  Nothing matches. Try fewer words.
                </Command.Empty>

                <Command.Group
                  heading="Navigate"
                  className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-ink-faint"
                >
                  {navMatches.map((item) => (
                    <Command.Item
                      key={item.to}
                      value={`nav:${item.label}`}
                      onSelect={() => go(item.to)}
                      className="cursor-pointer rounded px-2 py-1.5 text-sm text-ink data-[selected=true]:bg-bg-sunken"
                    >
                      {item.label}
                    </Command.Item>
                  ))}
                  {results.map((result) => (
                    <Command.Item
                      key={result.path}
                      value={result.path}
                      onSelect={() => openResult(result)}
                      className="flex cursor-pointer items-center gap-2 rounded px-2 py-1.5 text-sm text-ink data-[selected=true]:bg-bg-sunken"
                    >
                      <span className="min-w-0 flex-1 truncate">
                        {result.title ?? result.path}
                      </span>
                      <span className="shrink-0 rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-muted">
                        {result.note_type}
                      </span>
                    </Command.Item>
                  ))}
                </Command.Group>

                <Command.Group
                  heading="Do"
                  className="[&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:py-1 [&_[cmdk-group-heading]]:text-xs [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:uppercase [&_[cmdk-group-heading]]:tracking-wider [&_[cmdk-group-heading]]:text-ink-faint"
                >
                  <Command.Item
                    value="verb:capture"
                    onSelect={() => setMode("capture")}
                    className="cursor-pointer rounded px-2 py-1.5 text-sm text-ink data-[selected=true]:bg-bg-sunken"
                  >
                    Capture…
                  </Command.Item>
                  <Command.Item
                    value="verb:log"
                    onSelect={() => setMode("log")}
                    className="cursor-pointer rounded px-2 py-1.5 text-sm text-ink data-[selected=true]:bg-bg-sunken"
                  >
                    Log to daily…
                  </Command.Item>
                  {settingsMatches.map((item) => (
                    <Command.Item
                      key={item.section}
                      value={`settings:${item.label}`}
                      onSelect={() => {
                        onOpenSettings(item.section);
                        onOpenChange(false);
                      }}
                      className="cursor-pointer rounded px-2 py-1.5 text-sm text-ink data-[selected=true]:bg-bg-sunken"
                    >
                      {item.label}
                    </Command.Item>
                  ))}
                </Command.Group>
              </Command.List>
            </Command>
          )}
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
