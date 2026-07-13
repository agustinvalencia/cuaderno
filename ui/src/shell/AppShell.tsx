import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { NavLink, Outlet } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { getOrientation, listInbox } from "../api/commands";
import { contextDotClass } from "../lib/contexts";
import InboxDrawer from "./InboxDrawer";
import SettingsDialog from "./SettingsDialog";
import WatcherPill from "./WatcherPill";
import ConfigStatusBanner from "./ConfigStatusBanner";
import { ReaderProvider, useReader } from "./reader";
import { useHistoryNavigation } from "./useHistoryNavigation";

// The note reader pulls react-markdown + remark-gfm; the palette pulls
// cmdk. Neither is on the shell's first paint, so both load lazily —
// the reader only when a note is opened, the palette only on first ⌘K —
// keeping those deps out of the main chunk.
const NoteReader = lazy(() => import("../components/markdown/NoteReader"));
const CommandPalette = lazy(() => import("./CommandPalette"));

/** The single note-reader panel for the whole app (plan §6): distant
 * surfaces (timeline chips, palette results, backlinks) open it via
 * `useReader`; it renders here, once. A wikilink to a plain note
 * replaces the panel in place; project/stewardship links route away and
 * `NoteReader` closes it. */
function ReaderHost() {
  const { openPath, openReader, closeReader } = useReader();
  if (!openPath) return null;
  // `key={openPath}` remounts the reader on note-to-note navigation so
  // scroll position and internal state reset instead of bleeding from
  // the previous note into the next.
  return (
    <Suspense fallback={null}>
      <NoteReader
        key={openPath}
        path={openPath}
        onClose={closeReader}
        onNavigate={openReader}
      />
    </Suspense>
  );
}

const NAV = [
  { to: "/", label: "Today" },
  { to: "/actions", label: "Actions" },
  { to: "/calendar", label: "Calendar" },
  { to: "/commitments", label: "Commitments" },
  { to: "/weekly", label: "Weekly" },
  { to: "/strategic", label: "Strategic" },
];

const BROWSE = [
  { to: "/portfolios", label: "Portfolios" },
  { to: "/stewardships", label: "Stewardships" },
  { to: "/templates", label: "Templates" },
  { to: "/config", label: "Config" },
];

export default function AppShell() {
  // The sidebar's project list rides on the same query the Home view
  // uses — one cache entry, no extra invoke.
  const orientation = useQuery({ queryKey: ["get_orientation"], queryFn: getOrientation });
  // The inbox count rides on the same query the drawer uses — one cache
  // entry, invalidated by the `inbox` area (invalidation.ts).
  const inbox = useQuery({ queryKey: ["list_inbox"], queryFn: listInbox });
  const inboxCount = inbox.data?.length ?? 0;
  const [inboxOpen, setInboxOpen] = useState(false);
  const inboxButtonRef = useRef<HTMLButtonElement>(null);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Back/forward via the mouse side buttons, Cmd/Ctrl+[ / ], and the
  // native macOS bridge — history navigation, as in a browser.
  useHistoryNavigation();

  // Global Cmd/Ctrl+K toggles the command palette (plan §1.0); Cmd/Ctrl+,
  // opens Settings (the macOS Preferences convention). Bound on window so
  // they fire from any focused view; each dialog owns Esc-to-close (Radix).
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (!(event.metaKey || event.ctrlKey)) return;
      if (event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPaletteOpen((prev) => !prev);
      } else if (event.key === ",") {
        event.preventDefault();
        setSettingsOpen(true);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <ReaderProvider>
    <div className="flex h-screen">
      {/* `app-sidebar` is the translucent frosted surface (globals.css) the
          macOS `sidebar` vibrancy material blurs through. The top padding
          clears the inset traffic lights of the overlay title bar. */}
      <aside className="app-sidebar flex w-56 shrink-0 flex-col border-r border-line px-3 pb-4 pt-8">
        {/* The brand strip doubles as the window drag handle — the overlay
            title bar has no bar of its own to grab (Tauri's
            data-tauri-drag-region; nav/buttons below are separate targets,
            so they still click through). */}
        <div
          data-tauri-drag-region
          className="mb-6 px-2 text-sm font-semibold tracking-wide text-ink"
        >
          cuaderno
        </div>

        <nav aria-label="Views" className="flex flex-col gap-1">
          {NAV.map(({ to, label }) => (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              className={({ isActive }) =>
                `rounded px-2 py-1 text-sm ${
                  isActive ? "bg-bg-surface font-medium text-ink" : "text-ink-muted hover:text-ink"
                }`
              }
            >
              {label}
            </NavLink>
          ))}
        </nav>

        <div className="mt-6 px-2 text-xs font-medium uppercase tracking-wider text-ink-faint">
          Projects
        </div>
        <nav aria-label="Active projects" className="mt-1 flex flex-col gap-1">
          {(orientation.data?.projects ?? []).map((project) => (
            <NavLink
              key={project.slug}
              to={`/projects/${project.slug}`}
              className="flex items-center gap-2 rounded px-2 py-1 text-sm text-ink-muted hover:text-ink"
            >
              <span
                aria-hidden
                className={`h-2 w-2 shrink-0 rounded-full ${contextDotClass(project.context)}`}
              />
              <span className="truncate">{project.slug}</span>
            </NavLink>
          ))}
        </nav>

        <div className="mt-6 px-2 text-xs font-medium uppercase tracking-wider text-ink-faint">
          Browse
        </div>
        <nav aria-label="Browse" className="mt-1 flex flex-col gap-1">
          {BROWSE.map(({ to, label }) => (
            <NavLink
              key={to}
              to={to}
              className="rounded px-2 py-1 text-sm text-ink-muted hover:text-ink"
            >
              {label}
            </NavLink>
          ))}
        </nav>

        <button
          ref={inboxButtonRef}
          type="button"
          aria-expanded={inboxOpen}
          aria-label={`Inbox, ${inboxCount} waiting`}
          onClick={() => setInboxOpen((open) => !open)}
          className={`mt-auto flex items-center justify-between rounded px-2 py-1 text-sm ${
            inboxOpen ? "bg-bg-surface text-ink" : "text-ink-muted hover:text-ink"
          }`}
        >
          <span>Inbox</span>
          {inboxCount > 0 && (
            // Grey, never red — the badge is a count, not an alarm (§1.0).
            <span className="rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-faint">
              {inboxCount}
            </span>
          )}
        </button>

        <button
          type="button"
          onClick={() => setPaletteOpen(true)}
          aria-label="Open command palette"
          className="mt-4 flex items-center justify-between rounded px-2 py-1 text-xs text-ink-muted hover:text-ink"
        >
          <span>Search &amp; jump</span>
          {/* Glyphs, not emoji — the ⌘K hint (plan §5). */}
          <span className="rounded bg-bg-sunken px-1.5 py-0.5 text-ink-faint">⌘K</span>
        </button>

        <WatcherPill />

        <div className="mt-2 px-2 pt-4">
          <button
            type="button"
            aria-label="Open settings"
            onClick={() => setSettingsOpen(true)}
            className="flex w-full items-center justify-between rounded px-2 py-1 text-xs text-ink-muted hover:text-ink"
          >
            <span>Settings</span>
            <span className="rounded bg-bg-sunken px-1.5 py-0.5 text-ink-faint">⌘,</span>
          </button>
        </div>
      </aside>

      {/* The opaque content pane: carries the base background (moved off
          `body`, which is now transparent for the sidebar vibrancy) so the
          material shows only behind the frosted sidebar. */}
      <main className="min-w-0 flex-1 overflow-y-auto bg-bg-base">
        <ConfigStatusBanner />
        <Outlet />
      </main>

      {inboxOpen && (
        <InboxDrawer onClose={() => setInboxOpen(false)} returnFocusRef={inboxButtonRef} />
      )}

      {/* Mounted only once opened, so cmdk loads on first ⌘K, not on
          the shell's first paint. */}
      {paletteOpen && (
        <Suspense fallback={null}>
          <CommandPalette open={paletteOpen} onOpenChange={setPaletteOpen} />
        </Suspense>
      )}
      <SettingsDialog open={settingsOpen} onOpenChange={setSettingsOpen} />
      <ReaderHost />
    </div>
    </ReaderProvider>
  );
}
