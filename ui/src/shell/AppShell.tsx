import { NavLink, Outlet } from "react-router";
import { useQuery } from "@tanstack/react-query";
import { getOrientation } from "../api/commands";
import { contextDotClass } from "../lib/contexts";
import { cycleTheme } from "../lib/theme";

const NAV = [
  { to: "/", label: "Today" },
  { to: "/actions", label: "Actions" },
  { to: "/commitments", label: "Commitments" },
  { to: "/weekly", label: "Weekly" },
  { to: "/strategic", label: "Strategic" },
];

const BROWSE = [
  { to: "/portfolios", label: "Portfolios" },
  { to: "/stewardships", label: "Stewardships" },
];

export default function AppShell() {
  // The sidebar's project list rides on the same query the Home view
  // uses — one cache entry, no extra invoke.
  const orientation = useQuery({ queryKey: ["get_orientation"], queryFn: getOrientation });

  return (
    <div className="flex h-screen">
      <aside className="flex w-56 shrink-0 flex-col border-r border-line bg-bg-sunken px-3 py-4">
        <div className="mb-6 px-2 text-sm font-semibold tracking-wide text-ink">cuaderno</div>

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

        <div className="mt-auto flex items-center justify-between px-2 pt-4">
          <button
            type="button"
            onClick={() => cycleTheme()}
            className="rounded px-2 py-1 text-xs text-ink-muted hover:text-ink"
          >
            theme
          </button>
        </div>
      </aside>

      <main className="min-w-0 flex-1 overflow-y-auto">
        <Outlet />
      </main>
    </div>
  );
}
