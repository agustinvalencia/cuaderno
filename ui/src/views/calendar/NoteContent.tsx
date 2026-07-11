// Sectioned presentation of a vault note in the calendar panel (UI
// request 2026-07-12). Instead of dumping the raw markdown blob — which
// rendered the YAML frontmatter as a stray rule-and-text run and left a
// long `## Logs` history as an undifferentiated wall — the note is parsed
// (see lib/noteContent) and laid out with hierarchy: a separated metadata
// strip, each `##` section under a clear title, and the append-only
// `## Logs` section as a scrollable stack of timestamped cards.
import Markdown from "../../components/markdown/Markdown";
import { MetaPanel } from "../../components/markdown/MetaPanel";
import { LogCard } from "../../components/ui/log-card";
import { isLogsSection, parseLogEntries, parseNote, type NoteSection } from "../../lib/noteContent";

export default function NoteContent({
  markdown,
  onWikilink,
}: {
  markdown: string;
  onWikilink: (target: string) => void;
}) {
  const { frontmatter, sections } = parseNote(markdown);
  return (
    <div className="space-y-6">
      <MetaPanel frontmatter={frontmatter} />
      {sections.map((section, index) => (
        <SectionBlock
          key={`${section.heading ?? "lead"}-${index}`}
          section={section}
          onWikilink={onWikilink}
        />
      ))}
    </div>
  );
}

function SectionBlock({
  section,
  onWikilink,
}: {
  section: NoteSection;
  onWikilink: (target: string) => void;
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

  // The Logs history: a scrollable stack of timestamped cards, so a long
  // day of entries reads as a scannable ledger and never pushes the rest
  // of the note out of reach. Falls back to plain markdown if the section
  // doesn't parse into entries (an unexpected shape shouldn't hide it).
  if (isLogsSection(section.heading)) {
    const entries = parseLogEntries(section.body);
    if (entries.length > 0) {
      return (
        <section aria-label={section.heading}>
          <SectionTitle>{section.heading}</SectionTitle>
          <div className="mt-2 max-h-96 space-y-1.5 overflow-y-auto pr-1">
            {entries.map((entry, index) => (
              <LogCard key={`${entry.time}-${index}`} time={entry.time}>
                {entry.text}
              </LogCard>
            ))}
          </div>
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
 * a level. */
function SectionTitle({ children }: { children: string }) {
  return <h2 className="text-sm font-semibold text-ink">{children}</h2>;
}
