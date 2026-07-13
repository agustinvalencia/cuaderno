import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { NavLink, Outlet } from "react-router";
import { useQuery } from "@tanstack/react-query";
import {
  Calendar,
  CalendarRange,
  Compass,
  Briefcase,
  Handshake,
  Inbox as InboxIcon,
  LayoutTemplate,
  ListChecks,
  Search,
  Settings as SettingsIcon,
  SlidersHorizontal,
  Sprout,
  Sun,
  type LucideIcon,
} from "lucide-react";
import { getOrientation, listInbox } from "../api/commands";
import { contextDotClass } from "../lib/contexts";
import InboxDrawer from "./InboxDrawer";
import SettingsDialog from "./SettingsDialog";
import WatcherPill from "./WatcherPill";
import ConfigStatusBanner from "./ConfigStatusBanner";
import { ReaderProvider } from "./reader";
import { useHistoryNavigation } from "./useHistoryNavigation";
import { useDeepLinkNavigation } from "./useDeepLinkNavigation";

// The command palette pulls cmdk, which isn't on the shell's first paint,
// so it loads lazily on first ⌘K — keeping that dep out of the main chunk.
// (The note reader is now its own `/note/*` route, code-split in routes.tsx.)
const CommandPalette = lazy(() => import("./CommandPalette"));

const NAV: { to: string; label: string; icon: LucideIcon }[] = [
  { to: "/", label: "Today", icon: Sun },
  { to: "/actions", label: "Actions", icon: ListChecks },
  { to: "/calendar", label: "Calendar", icon: Calendar },
  { to: "/commitments", label: "Commitments", icon: Handshake },
  { to: "/weekly", label: "Weekly", icon: CalendarRange },
  { to: "/strategic", label: "Strategic", icon: Compass },
];

const BROWSE: { to: string; label: string; icon: LucideIcon }[] = [
  { to: "/portfolios", label: "Portfolios", icon: Briefcase },
  { to: "/stewardships", label: "Stewardships", icon: Sprout },
  { to: "/templates", label: "Templates", icon: LayoutTemplate },
  { to: "/config", label: "Config", icon: SlidersHorizontal },
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
  // Open a note when a `cuaderno://note/<path>` deep link fires.
  useDeepLinkNavigation();

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
          macOS `sidebar` vibrancy material blurs through. */}
      <aside className="app-sidebar flex w-[var(--sidebar-width)] shrink-0 flex-col border-r border-line px-3 pb-4">
        {/* Full-width draggable title strip: restores native window
            dragging under the overlay title bar — the whole top edge, not
            just the brand text (the earlier regression) — and clears the
            inset traffic lights. Height is `--titlebar-height` (globals),
            the same var the content-pane gutter uses, so the whole top
            edge is one grab target. `-mx-3` spans the sidebar's padding;
            nav and buttons below are separate targets, so they still
            click. */}
        <div
          data-tauri-drag-region
          className="-mx-3 mb-2 flex h-[var(--titlebar-height)] items-end px-5 pb-1.5 text-sm font-semibold tracking-wide text-ink"
        >
          cuaderno
        </div>

        <nav aria-label="Views" className="flex flex-col gap-0.5">
          {NAV.map(({ to, label, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              end={to === "/"}
              className={({ isActive }) =>
                `flex items-center gap-2.5 rounded px-2 py-1 text-sm ${
                  isActive ? "bg-bg-surface font-medium text-ink" : "text-ink-muted hover:text-ink"
                }`
              }
            >
              <Icon className="h-4 w-4 shrink-0" aria-hidden strokeWidth={1.75} />
              <span>{label}</span>
            </NavLink>
          ))}
        </nav>

        <div className="mt-6 px-2 text-xs font-medium uppercase tracking-wider text-ink-faint">
          Projects
        </div>
        <nav aria-label="Active projects" className="mt-1 flex flex-col gap-0.5">
          {(orientation.data?.projects ?? []).map((project) => (
            <NavLink
              key={project.slug}
              to={`/projects/${project.slug}`}
              className="flex items-center gap-2.5 rounded px-2 py-1 text-sm text-ink-muted hover:text-ink"
            >
              {/* The context dot is this row's "icon" — its colour carries
                  the project's life context. */}
              <span
                aria-hidden
                className={`ml-0.5 h-2 w-2 shrink-0 rounded-full ${contextDotClass(project.context)}`}
              />
              <span className="truncate">{project.slug}</span>
            </NavLink>
          ))}
        </nav>

        <div className="mt-6 px-2 text-xs font-medium uppercase tracking-wider text-ink-faint">
          Browse
        </div>
        <nav aria-label="Browse" className="mt-1 flex flex-col gap-0.5">
          {BROWSE.map(({ to, label, icon: Icon }) => (
            <NavLink
              key={to}
              to={to}
              className="flex items-center gap-2.5 rounded px-2 py-1 text-sm text-ink-muted hover:text-ink"
            >
              <Icon className="h-4 w-4 shrink-0" aria-hidden strokeWidth={1.75} />
              <span>{label}</span>
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
          <span className="flex items-center gap-2.5">
            <InboxIcon className="h-4 w-4 shrink-0" aria-hidden strokeWidth={1.75} />
            Inbox
          </span>
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
          <span className="flex items-center gap-2.5">
            <Search className="h-4 w-4 shrink-0" aria-hidden strokeWidth={1.75} />
            Search &amp; jump
          </span>
          {/* Glyphs, not emoji — the ⌘K hint (plan §5). */}
          <span className="rounded bg-bg-sunken px-1.5 py-0.5 text-ink-faint">⌘K</span>
        </button>

        <WatcherPill />

        <div className="mt-2 pt-4">
          <button
            type="button"
            aria-label="Open settings"
            onClick={() => setSettingsOpen(true)}
            className="flex w-full items-center justify-between rounded px-2 py-1 text-xs text-ink-muted hover:text-ink"
          >
            <span className="flex items-center gap-2.5">
              <SettingsIcon className="h-4 w-4 shrink-0" aria-hidden strokeWidth={1.75} />
              Settings
            </span>
            <span className="rounded bg-bg-sunken px-1.5 py-0.5 text-ink-faint">⌘,</span>
          </button>
        </div>
      </aside>

      {/* The opaque content pane: carries the base background (moved off
          `body`, which is now transparent for the sidebar vibrancy) so the
          material shows only behind the frosted sidebar. A slim draggable
          gutter at the top lets the window move from the content side too,
          matching the sidebar's title strip. */}
      <main className="flex min-w-0 flex-1 flex-col bg-bg-base">
        <div data-tauri-drag-region className="h-[var(--titlebar-height)] shrink-0" />
        <div className="min-h-0 flex-1 overflow-y-auto">
          <ConfigStatusBanner />
          <Outlet />
        </div>
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
    </div>
    </ReaderProvider>
  );
}
