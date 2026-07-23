// Today (#442) — the day, as written.
//
// The daily note IS the day: intention, standup, agenda, and the
// append-only log that is the method's spine. It used to live one view
// away, behind the Calendar, while this page showed a grid of project
// cards restating the sidebar. So the note is the page now, and the
// project cards are gone.
//
// Above it, in the order a morning actually needs them: what you are in
// the middle of, a place to log a line, what is promised soon, and the
// energy-filtered shortlist that answers "pick one thing".
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import { getOrientation, readDaily, resolveWikilink } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";
import { shortDate } from "../../lib/dates";
import { parseNote } from "../../lib/noteContent";
import SectionedBody from "../../components/markdown/SectionedBody";
import { MetaPanel } from "../../components/markdown/MetaPanel";
import { QuickLog } from "../../components/ui/quick-log";
import { SectionHeading } from "../../components/ui/section-heading";
import { useReader } from "../../shell/reader";
import ActionShortlist from "./ActionShortlist";
import NowBand from "./NowBand";

const ENERGIES: EnergyLevel[] = ["deep", "medium", "light"];

export default function Home() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_orientation"],
    queryFn: getOrientation,
  });
  const [energy, setEnergy] = useState<EnergyLevel | null>(null);

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">The vault could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  const heading = new Date(`${data.today}T00:00:00`).toLocaleDateString(undefined, {
    weekday: "long",
    day: "numeric",
    month: "long",
  });

  return (
    <div className="mx-auto max-w-3xl p-8">
      <div className="flex items-center justify-between gap-3">
        <h1 className="text-xl font-semibold text-ink">{heading}</h1>
        <div role="group" aria-label="Energy filter" className="flex gap-1">
          {ENERGIES.map((level) => (
            <button
              key={level}
              type="button"
              aria-pressed={energy === level}
              onClick={() => setEnergy(energy === level ? null : level)}
              className={`rounded px-2 py-1 text-xs ${
                energy === level
                  ? "bg-bg-sunken font-medium text-ink"
                  : "text-ink-muted hover:text-ink"
              }`}
            >
              {level}
            </button>
          ))}
        </div>
      </div>

      <div className="mt-4">
        <NowBand />
      </div>

      <div className="mt-4">
        <QuickLog date={data.today} />
      </div>

      {data.commitments.length > 0 && (
        <section aria-label="Due soon" className="mt-6">
          <SectionHeading>Due soon</SectionHeading>
          <ul className="mt-2 flex flex-wrap gap-2">
            {data.commitments.map((commitment) => (
              <li
                key={`${commitment.source.kind}-${commitment.date}-${commitment.title}`}
                className="flex items-center rounded border border-line bg-bg-surface px-3 py-1.5 text-sm text-ink"
              >
                <span
                  aria-hidden
                  className={`mr-2 h-2 w-2 shrink-0 rounded-full ${contextDotClass(commitment.context)}`}
                />
                <span>{commitment.title}</span>
                <span className="ml-2 text-ink-muted">
                  {commitment.is_overdue
                    ? `planned for ${shortDate(commitment.date)}`
                    : shortDate(commitment.date)}
                </span>
              </li>
            ))}
          </ul>
        </section>
      )}

      <section aria-label="Pick one thing" className="mt-6">
        <SectionHeading>Pick one thing</SectionHeading>
        <div className="mt-2">
          <ActionShortlist projects={data.projects} energy={energy} />
        </div>
      </section>

      <DailyNote date={data.today} />

      {data.lapsed_habits.length > 0 && (
        <p className="mt-8 text-sm text-ink-faint">
          quietly lapsed: {data.lapsed_habits.map((habit) => habit.detail).join(" · ")} — no
          judgment, just a note
        </p>
      )}
    </div>
  );
}

/** Today's note, rendered the same way the calendar's panel renders it —
 * one reader for one note, wherever it opens. */
function DailyNote({ date }: { date: string }) {
  const { openReader } = useReader();
  const { data } = useQuery({
    queryKey: ["read_daily", date],
    queryFn: () => readDaily(date),
  });

  async function onWikilink(target: string) {
    try {
      const resolved = await resolveWikilink(target);
      if (resolved) openReader(resolved.path);
    } catch {
      // An unresolved target is a no-op; the anchor already rendered muted.
    }
  }

  if (!data) return null;

  // Parsed once: the frontmatter and the sections come from the same pass.
  const note = data.exists ? parseNote(data.markdown) : null;

  return (
    <section aria-label="Today's note" className="mt-10 border-t border-line pt-6">
      {note ? (
        <>
          <MetaPanel frontmatter={note.frontmatter} />
          <div className="mt-4">
            <SectionedBody sections={note.sections} onWikilink={onWikilink} capLogsHeight />
          </div>
        </>
      ) : (
        <p className="text-sm text-ink-muted">
          No note for today yet — logging a line above starts one.
        </p>
      )}
    </section>
  );
}
