// Shared sectioned rendering of a note body (UI request 2026-07-12,
// unification follow-up). Given a note's `##` sections, it lays them out
// with hierarchy: the preamble (the `# Title`) plainly, each section under
// a clear title, and the append-only `## Logs` history as a scrollable
// stack of timestamped cards whose `[[wikilinks]]` stay clickable. Both
// the calendar panel (which parses a raw blob via `parseNote`) and the
// shell note reader (which gets a frontmatter-free body from `read_note`,
// split via `splitBodySections`) compose this alongside `MetaPanel`, so a
// note reads the same wherever it opens.
import Markdown from "./Markdown";
import { LogCard } from "../ui/log-card";
import {
  isLogsSection,
  parseLogEntries,
  type NoteSection,
} from "../../lib/noteContent";
import { orderLogs, useLogOrder, type LogOrder } from "../../lib/logOrder";
import { LogOrderToggle } from "../ui/log-order-toggle";

export default function SectionedBody({
  sections,
  onWikilink,
  capLogsHeight = false,
}: {
  sections: NoteSection[];
  onWikilink: (target: string) => void;
  /** Cap the `## Logs` stack at a fixed height with its own scroll. Right
   * for the wide, page-scrolled calendar (keeps a long day from shoving
   * other sections down); wrong for the narrow slide-in reader, whose own
   * panel scroll is the note's single scroll — there the Logs flow to
   * natural height (no nested/trapped inner scroller). Default off. */
  capLogsHeight?: boolean;
}) {
  const logOrder = useLogOrder();
  return (
    <div className="space-y-6">
      {sections.map((section, index) => (
        <SectionBlock
          key={`${section.heading ?? "lead"}-${index}`}
          section={section}
          onWikilink={onWikilink}
          capLogsHeight={capLogsHeight}
          logOrder={logOrder}
        />
      ))}
    </div>
  );
}

function SectionBlock({
  section,
  onWikilink,
  capLogsHeight,
  logOrder,
}: {
  section: NoteSection;
  onWikilink: (target: string) => void;
  capLogsHeight: boolean;
  logOrder: LogOrder;
}) {
  // The preamble (the `# Title` and anything before the first `##`)
  // renders plainly — it isn't a section of its own.
  if (section.heading === null) {
    return (
      <div className="max-w-none">
        <Markdown body={section.body} onWikilink={onWikilink} />
      </div>
    );
  }

  // The Logs history: a stack of timestamped cards so a long day reads as
  // a scannable ledger. Falls back to plain markdown if the section
  // doesn't parse into entries (an unexpected shape shouldn't hide it).
  if (isLogsSection(section.heading)) {
    const entries = parseLogEntries(section.body);
    if (entries.length > 0) {
      const cards = orderLogs(entries, logOrder).map((entry, index) => (
        <LogCard key={`${entry.time}-${index}`} time={entry.time}>
          {/* Through Markdown so a log line's `[[wikilinks]]` (e.g. a
              project state-change `state on [[slug]]`) stay clickable, as
              they were when the whole blob rendered as markdown. Margins
              zeroed so a one-line entry stays compact. */}
          <div className="[&>p]:my-0">
            <Markdown body={entry.text} onWikilink={onWikilink} />
          </div>
        </LogCard>
      ));
      return (
        <section aria-label={section.heading}>
          <div className="flex items-baseline justify-between gap-2">
            <SectionTitle>{section.heading}</SectionTitle>
            <LogOrderToggle />
          </div>
          {capLogsHeight ? (
            // Capped: a fixed-height inner scroll, focusable so a keyboard
            // user can arrow-scroll it (axe scrollable-region-focusable).
            <div
              tabIndex={0}
              aria-label={`${section.heading} entries`}
              className="mt-2 max-h-96 space-y-1.5 overflow-y-auto pr-1"
            >
              {cards}
            </div>
          ) : (
            // Uncapped: flow to natural height under the parent's own
            // scroll — no nested scroller, so no extra tab stop needed.
            <div className="mt-2 space-y-1.5">{cards}</div>
          )}
        </section>
      );
    }
  }

  return (
    <section aria-label={section.heading}>
      <SectionTitle>{section.heading}</SectionTitle>
      <div className="mt-2 max-w-none">
        <Markdown body={section.body} onWikilink={onWikilink} />
      </div>
    </section>
  );
}

/** A note section's title — clear and readable (this is content
 * structure), distinct from the tiny faint uppercase labels used for
 * metadata affordances. An `h2`: it follows the body's `# Title` (h1) and
 * precedes any `###` a section body renders, so heading order never skips
 * a level. `text-base` matches Markdown's own `h2`, seating a section
 * title visibly above body prose and any `###` subheading (both `text-sm`)
 * — the "meaningful visual hierarchy" the sections are for. */
function SectionTitle({ children }: { children: string }) {
  return <h2 className="text-base font-semibold text-ink">{children}</h2>;
}
