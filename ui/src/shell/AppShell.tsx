import { lazy, Suspense, useEffect, useRef, useState, type ReactNode } from "react";
import { NavLink, Outlet } from "react-router";
import { useQuery } from "@tanstack/react-query";
import {
  Calendar,
  CalendarRange,
  Compass,
  Briefcase,
  Handshake,
  HelpCircle,
  Inbox as InboxIcon,
  ListChecks,
  Search,
  Settings as SettingsIcon,
  Sprout,
  Sun,
  type LucideIcon,
} from "lucide-react";
import { getOrientation, listInbox } from "../api/commands";
import { contextDotClass } from "../lib/contexts";
import InboxDrawer from "./InboxDrawer";
import SettingsDialog, { type SettingsSection } from "./SettingsDialog";
import WatcherPill from "./WatcherPill";
import ConfigStatusBanner from "./ConfigStatusBanner";
import IndexExclusionsBanner from "./IndexExclusionsBanner";
import { ReaderProvider } from "./reader";
import { useHistoryNavigation } from "./useHistoryNavigation";
import { useDeepLinkNavigation } from "./useDeepLinkNavigation";

// The command palette pulls cmdk, which isn't on the shell's first paint,
// so it loads lazily on first ⌘K — keeping that dep out of the main chunk.
// (The note reader is now its own `/note/*` route, code-split in routes.tsx.)
const CommandPalette = lazy(() => import("./CommandPalette"));

// The nav is the first and most persistent thing the app says about
// itself, and a flat list of ten destinations says "here are ten
// screens". The method is two tracks — inquiry and operations — bound by
// a cadence of daily, weekly and monthly review, so that is what the
// sidebar is shaped like (#444).
//
// The old "Browse" bucket mixed the knowledge layer (Portfolios), the
// responsibility layer (Stewardships) and two settings surfaces
// (Templates, Config) into one list, which invited the reading that a
// template is a note. Templates and Config now live behind Cmd+, where
// configuration belongs; their routes survive for deep links.
type NavItem = { to: string; label: string; icon: LucideIcon };

const RHYTHM: NavItem[] = [
  { to: "/", label: "Today", icon: Sun },
  { to: "/calendar", label: "Calendar", icon: Calendar },
  { to: "/weekly", label: "Weekly", icon: CalendarRange },
  { to: "/monthly", label: "Monthly", icon: Compass },
];

// Projects are not in this list: they get their own sub-header inside the
// group, because the active list nests under it and the header carries the
// slot count.
const OPERATIONS: NavItem[] = [
  { to: "/actions", label: "Actions", icon: ListChecks },
  { to: "/commitments", label: "Commitments", icon: Handshake },
  { to: "/stewardships", label: "Stewardships", icon: Sprout },
];

const INQUIRY: NavItem[] = [
  { to: "/questions", label: "Questions", icon: HelpCircle },
  { to: "/portfolios", label: "Portfolios", icon: Briefcase },
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
  // Which settings section to land on, and `null` for closed. Held as the
  // section rather than a bare boolean so the palette can jump straight to
  // Vault config or Templates — the destinations that left the sidebar.
  const [settingsSection, setSettingsSection] = useState<SettingsSection | null>(null);

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
        setSettingsSection("appearance");
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

        {/* Scrolls on its own so a vault at the project cap never pushes
            the Inbox and Settings buttons off a short window. */}
        <div className="min-h-0 flex-1 overflow-y-auto">
          <NavGroup label="Rhythm" blurb="the cadence" items={RHYTHM} first />

          <NavGroup label="Operations" blurb="delivery" items={OPERATIONS}>
            {/* Projects lead the group and carry the list, so the header
                doubles as the slot counter. */}
            <div className="mt-1 flex items-baseline justify-between gap-2 px-2">
              <span className="text-sm text-ink-muted">Projects</span>
              {/* Not behind `useMetrics()`: the cap is a rule of the
                  method, not a progress reading. It is the reason a sixth
                  project has to displace one, and the sidebar is where it
                  should be legible rather than discovered by hitting it. */}
              {orientation.data !== undefined && (
                <span className="shrink-0 text-xs text-ink-faint">
                  {orientation.data.projects.length} of {orientation.data.max_active} slots
                </span>
              )}
            </div>
            <nav aria-label="Active projects" className="mt-0.5 flex flex-col gap-0.5">
              {(orientation.data?.projects ?? []).map((project) => (
                <NavLink
                  key={project.slug}
                  to={`/projects/${project.slug}`}
                  className={({ isActive }) =>
                    `flex items-center gap-2.5 rounded py-1 pl-4 pr-2 text-sm ${
                      isActive
                        ? "bg-bg-surface font-medium text-ink"
                        : "text-ink-muted hover:text-ink"
                    }`
                  }
                >
                  {/* The context dot is this row's "icon" — its colour
                      carries the project's life context. */}
                  <span
                    aria-hidden
                    className={`h-2 w-2 shrink-0 rounded-full ${contextDotClass(project.context)}`}
                  />
                  <span className="truncate">{project.slug}</span>
                </NavLink>
              ))}
            </nav>
          </NavGroup>

          <NavGroup label="Inquiry" blurb="investigation" items={INQUIRY} />
        </div>

        <button
          ref={inboxButtonRef}
          type="button"
          aria-expanded={inboxOpen}
          aria-label={`Inbox, ${inboxCount} waiting`}
          onClick={() => setInboxOpen((open) => !open)}
          className={`mt-2 flex items-center justify-between rounded px-2 py-1 text-sm ${
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
            onClick={() => setSettingsSection("appearance")}
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
          <IndexExclusionsBanner />
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
          <CommandPalette
            open={paletteOpen}
            onOpenChange={setPaletteOpen}
            onOpenSettings={setSettingsSection}
          />
        </Suspense>
      )}
      <SettingsDialog
        section={settingsSection}
        onSectionChange={setSettingsSection}
      />
    </div>
    </ReaderProvider>
  );
}

/** One track of the sidebar: its name, a one-word gloss in the method's
 * language, and its destinations.
 *
 * `children` render *above* the flat items, not below — the only group
 * that uses them is Operations, whose Projects block leads it. */
function NavGroup({
  label,
  blurb,
  items,
  first,
  children,
}: {
  label: string;
  blurb: string;
  items: NavItem[];
  /** Skips the top margin, for the group that sits under the title strip. */
  first?: boolean;
  children?: ReactNode;
}) {
  return (
    <div className={first ? "" : "mt-5"}>
      <div className="flex items-baseline gap-1.5 px-2">
        <span className="text-xs font-medium uppercase tracking-wider text-ink-faint">
          {label}
        </span>
        <span className="truncate text-xs text-ink-faint opacity-70">{blurb}</span>
      </div>
      {children}
      <nav aria-label={label} className="mt-1 flex flex-col gap-0.5">
        {items.map(({ to, label: itemLabel, icon: Icon }) => (
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
            <span>{itemLabel}</span>
          </NavLink>
        ))}
      </nav>
    </div>
  );
}
