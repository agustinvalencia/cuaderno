// Calendar view (#340) — a month grid that loads daily notes into an
// EMBEDDED panel (not the shared NoteReader overlay, which has no
// navigation chrome). The panel renders the note's markdown read-only
// and carries quick jumps: prev day, next day, the day's week, and its
// month. Every jump target is a date the backend stamped on `read_daily`
// (prev_date / next_date / week_of / month), so the frontend never
// computes a domain date for a read (plan §3.7). A day, week, or month
// with no note shows a calm empty state — never an error.
import { useState } from "react";
import { useNavigate } from "react-router";
import { useQuery } from "@tanstack/react-query";
import Markdown from "../../components/markdown/Markdown";
import type { DailyView } from "../../api/bindings/DailyView";
import type { MonthlyView } from "../../api/bindings/MonthlyView";
import type { WeeklyView } from "../../api/bindings/WeeklyView";
import {
  getToday,
  listDailyDates,
  openInEditor,
  readDaily,
  readMonthly,
  readWeekly,
  resolveWikilink,
} from "../../api/commands";
import { useReader } from "../../shell/reader";
import MonthGrid from "./MonthGrid";

/** Which note the embedded panel is showing for the selected day. */
type PanelMode = "daily" | "weekly" | "monthly";

/** `Wednesday, 15 July 2026` — the panel's daily title. Parsed at local
 * midnight so the day never slips a timezone (matching Home). */
function fullDate(iso: string): string {
  return new Date(`${iso}T00:00:00`).toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
    year: "numeric",
  });
}

/** `15 July 2026` — short-of-weekday, for the "week of" label. */
function longDate(iso: string): string {
  return new Date(`${iso}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "long",
    year: "numeric",
  });
}

/** `July 2026` from a `YYYY-MM` month string (parsed at local midnight of
 * the 1st). */
function monthLabel(month: string): string {
  return new Date(`${month}-01T00:00:00`).toLocaleDateString(undefined, {
    month: "long",
    year: "numeric",
  });
}

/** The last path segment of a wikilink target (`projects/foo` → `foo`). */
function lastSegment(target: string): string {
  return target.split("/").pop()?.replace(/\.md$/i, "") ?? target;
}

export default function Calendar() {
  // The current date seeds the grid's initial month. Reading it from the
  // backend (rather than a client clock) keeps the "what month is it"
  // answer on the same authority as every other date in the app.
  const today = useQuery({ queryKey: ["get_today"], queryFn: getToday });

  if (today.isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (today.isError) {
    return (
      <div className="p-8">
        <p className="text-ink">The calendar could not be opened.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(today.error)}</p>
      </div>
    );
  }
  return <CalendarBody today={today.data} />;
}

function CalendarBody({ today }: { today: string }) {
  const navigate = useNavigate();
  const { openReader } = useReader();

  // The grid's viewport month. Paging it is display state (which month
  // the grid renders), not a note read — so this month arithmetic is
  // allowed where read-neighbour arithmetic is not. Seeded from today.
  const [initYear, initMonth] = today.split("-").map(Number);
  const [viewYear, setViewYear] = useState(initYear);
  const [viewMonth, setViewMonth] = useState(initMonth);

  // The selected day (an ISO string) and which note the panel shows.
  // Default to today so the panel opens populated rather than blank.
  const [selectedDate, setSelectedDate] = useState<string>(today);
  const [mode, setMode] = useState<PanelMode>("daily");

  // The days in the shown month that have a note, for the grid marks.
  const monthDays = useQuery({
    queryKey: ["list_daily_dates", viewYear, viewMonth],
    queryFn: () => listDailyDates(viewYear, viewMonth),
  });

  // The selected day's note plus its neighbour identities. Always
  // fetched when a day is selected — the week and month jumps read their
  // targets off this (week_of / month), so it must be present first.
  const daily = useQuery({
    queryKey: ["read_daily", selectedDate],
    queryFn: () => readDaily(selectedDate),
  });

  const weekOf = daily.data?.week_of;
  const monthOf = daily.data?.month;

  const weekly = useQuery({
    queryKey: ["read_weekly", weekOf],
    queryFn: () => readWeekly(weekOf as string),
    enabled: mode === "weekly" && weekOf !== undefined,
  });

  const monthly = useQuery({
    queryKey: ["read_monthly", monthOf],
    queryFn: () => readMonthly(monthOf as string),
    enabled: mode === "monthly" && monthOf !== undefined,
  });

  // A clicked wikilink inside a note: a project routes to its detail,
  // anything else opens in the shell reader; unresolvable targets are
  // quietly ignored (a muted span, per §3.8).
  async function openTarget(target: string) {
    let resolved;
    try {
      resolved = await resolveWikilink(target);
    } catch {
      return;
    }
    if (!resolved) return;
    if (resolved.note_type === "project") {
      navigate(`/projects/${lastSegment(resolved.path)}`);
    } else {
      openReader(resolved.path);
    }
  }

  function pageMonth(delta: number) {
    // Page the grid viewport by one month, wrapping the year. Display
    // state only — the selection and its note are untouched until a day
    // is clicked.
    const next = viewMonth + delta;
    if (next < 1) {
      setViewMonth(12);
      setViewYear(viewYear - 1);
    } else if (next > 12) {
      setViewMonth(1);
      setViewYear(viewYear + 1);
    } else {
      setViewMonth(next);
    }
  }

  function selectDay(iso: string) {
    setSelectedDate(iso);
    setMode("daily");
  }

  // Jump to a neighbour day the backend stamped (never computed here).
  function goToDay(iso: string) {
    setSelectedDate(iso);
    setMode("daily");
    // Follow the grid to the neighbour's month if it crossed a boundary.
    const [y, m] = iso.split("-").map(Number);
    setViewYear(y);
    setViewMonth(m);
  }

  // The selected day's day-of-month, but only when it falls in the shown
  // month — else the grid shows no selection.
  const [selYear, selMonth, selDay] = selectedDate.split("-").map(Number);
  const selectedDayInView =
    selYear === viewYear && selMonth === viewMonth ? selDay : null;

  const noteDays = new Set(
    (monthDays.data ?? [])
      .map((iso) => Number(iso.split("-")[2]))
      .filter((d) => !Number.isNaN(d)),
  );

  return (
    <div className="mx-auto max-w-5xl p-8">
      <h1 className="text-xl font-semibold text-ink">Calendar</h1>

      <div className="mt-6 grid grid-cols-1 gap-8 md:grid-cols-[20rem_1fr]">
        {/* Left: the month grid with its prev/next-month paging. */}
        <section aria-label="Month" className="min-w-0">
          <div className="mb-3 flex items-center justify-between">
            <button
              type="button"
              onClick={() => pageMonth(-1)}
              aria-label="Previous month"
              className="rounded border border-line px-2 py-1 text-sm text-ink-muted hover:text-ink"
            >
              ‹
            </button>
            <span className="text-sm font-medium text-ink">
              {monthLabel(`${viewYear}-${viewMonth < 10 ? `0${viewMonth}` : viewMonth}`)}
            </span>
            <button
              type="button"
              onClick={() => pageMonth(1)}
              aria-label="Next month"
              className="rounded border border-line px-2 py-1 text-sm text-ink-muted hover:text-ink"
            >
              ›
            </button>
          </div>
          <MonthGrid
            year={viewYear}
            month={viewMonth}
            noteDays={noteDays}
            selectedDay={selectedDayInView}
            onSelectDay={selectDay}
          />
        </section>

        {/* Right: the embedded note panel. */}
        <section aria-label="Note" className="min-w-0">
          <Panel
            mode={mode}
            setMode={setMode}
            daily={daily.data}
            dailyPending={daily.isPending}
            weekly={weekly.data}
            weeklyPending={weekly.isPending && weekly.fetchStatus !== "idle"}
            monthly={monthly.data}
            monthlyPending={monthly.isPending && monthly.fetchStatus !== "idle"}
            selectedDate={selectedDate}
            onGoToDay={goToDay}
            onWikilink={openTarget}
          />
        </section>
      </div>
    </div>
  );
}

/** The embedded panel: quick-nav controls, the note title, and the
 * note's markdown (read-only) or a calm empty state. */
function Panel({
  mode,
  setMode,
  daily,
  dailyPending,
  weekly,
  weeklyPending,
  monthly,
  monthlyPending,
  selectedDate,
  onGoToDay,
  onWikilink,
}: {
  mode: PanelMode;
  setMode: (mode: PanelMode) => void;
  daily: DailyView | undefined;
  dailyPending: boolean;
  weekly: WeeklyView | undefined;
  weeklyPending: boolean;
  monthly: MonthlyView | undefined;
  monthlyPending: boolean;
  selectedDate: string;
  onGoToDay: (iso: string) => void;
  onWikilink: (target: string) => void;
}) {
  // The active note (content + path), and whether it exists, per mode.
  // The daily always loads first (the week/month jumps read their target
  // dates off it), so a null daily means the panel is still warming up.
  const active =
    mode === "weekly"
      ? { view: weekly, pending: weeklyPending }
      : mode === "monthly"
        ? { view: monthly, pending: monthlyPending }
        : { view: daily, pending: dailyPending };

  const title =
    mode === "weekly"
      ? daily
        ? `Week of ${longDate(daily.week_of)}`
        : "Week"
      : mode === "monthly"
        ? daily
          ? monthLabel(daily.month)
          : "Month"
        : fullDate(selectedDate);

  const path = active.view?.path;
  const canJump = daily !== undefined;

  return (
    <div className="rounded-lg border border-line bg-bg-surface">
      <header className="flex flex-wrap items-center gap-2 border-b border-line px-4 py-3">
        <h2 className="min-w-0 flex-1 truncate text-base font-semibold text-ink">{title}</h2>
        {path && (
          <button
            type="button"
            onClick={() => void openInEditor(path)}
            className="shrink-0 rounded border border-line px-2 py-1 text-xs text-ink hover:bg-bg-sunken"
          >
            Open in editor
          </button>
        )}
      </header>

      {/* Quick-nav: prev/next day step through the backend-stamped
          neighbours; the day/week/month toggles switch which note the
          panel shows for the selected day. */}
      <nav aria-label="Note navigation" className="flex flex-wrap items-center gap-1 px-4 py-2">
        <button
          type="button"
          disabled={!canJump}
          onClick={() => daily && onGoToDay(daily.prev_date)}
          className="rounded border border-line px-2 py-1 text-xs text-ink-muted hover:text-ink disabled:opacity-50"
        >
          ‹ Prev day
        </button>
        <button
          type="button"
          disabled={!canJump}
          onClick={() => daily && onGoToDay(daily.next_date)}
          className="rounded border border-line px-2 py-1 text-xs text-ink-muted hover:text-ink disabled:opacity-50"
        >
          Next day ›
        </button>
        <span aria-hidden className="mx-1 text-ink-faint">
          |
        </span>
        {(["daily", "weekly", "monthly"] as const).map((m) => (
          <button
            key={m}
            type="button"
            aria-pressed={mode === m}
            onClick={() => setMode(m)}
            className={`rounded px-2 py-1 text-xs ${
              mode === m
                ? "bg-bg-sunken font-medium text-ink"
                : "text-ink-muted hover:text-ink"
            }`}
          >
            {m === "daily" ? "Day" : m === "weekly" ? "Week" : "Month"}
          </button>
        ))}
      </nav>

      <div className="px-4 py-3">
        {active.pending || (!active.view && mode === "daily" && dailyPending) ? (
          <p className="text-sm text-ink-muted">Reading…</p>
        ) : active.view && active.view.exists ? (
          <div className="max-w-none">
            <Markdown body={active.view.markdown} onWikilink={onWikilink} />
          </div>
        ) : (
          <EmptyState kind={mode} path={path} />
        )}
      </div>
    </div>
  );
}

/** The calm empty state for a day/week/month with no note yet — an
 * invitation, not an error, carrying the path to open in an editor. */
function EmptyState({ kind, path }: { kind: PanelMode; path: string | undefined }) {
  const noun = kind === "weekly" ? "week" : kind === "monthly" ? "month" : "day";
  return (
    <div className="rounded border border-line bg-bg-base p-6">
      <p className="text-sm text-ink-muted">No note for this {noun} yet.</p>
      {path && (
        <p className="mt-2 text-xs text-ink-faint">
          Start one in your editor:{" "}
          <button
            type="button"
            onClick={() => void openInEditor(path)}
            className="underline decoration-dotted underline-offset-2 hover:text-ink"
          >
            {path}
          </button>
        </p>
      )}
    </div>
  );
}
