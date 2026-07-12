// Sectioned presentation of a vault note in the calendar panel (UI
// request 2026-07-12). Instead of dumping the raw markdown blob — which
// rendered the YAML frontmatter as a stray rule-and-text run and left a
// long `## Logs` history as an undifferentiated wall — the note is parsed
// (see lib/noteContent) and composed from the shared pieces the note
// reader also uses: a separated `MetaPanel` metadata strip over the
// `SectionedBody` renderer.
import { MetaPanel } from "../../components/markdown/MetaPanel";
import SectionedBody from "../../components/markdown/SectionedBody";
import { parseNote } from "../../lib/noteContent";

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
      <SectionedBody sections={sections} onWikilink={onWikilink} />
    </div>
  );
}
