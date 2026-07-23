// A readable name for a note referenced only by path (#441).
//
// The index carries a `title` only for note types that declare a
// frontmatter `title` field, which most RLM types do not — they put their
// name in the body H1, which feeds the search row rather than the note row.
// Reading each source's H1 to label a list would mean a file read per item,
// so the filename is what we have, and it is usually enough: the dated,
// slugified names cdno writes carry the source phrase that made them.

/** Strip the folder, the `.md`, and a leading `YYYY-MM-DD-` stamp. */
function stem(path: string): string {
  const base = path.split("/").pop() ?? path;
  return base.replace(/\.md$/i, "").replace(/^\d{4}-\d{2}-\d{2}-/, "");
}

/**
 * What to call the note at `path`, preferring its frontmatter `title`.
 *
 * Falls back to the filename read as words: `2026-07-13-index-shape.md`
 * becomes "index shape". An `_index.md` names its folder instead, since
 * "index" would describe every portfolio equally.
 */
export function noteLabel(path: string, title?: string | null): string {
  if (title && title.trim() !== "") return title.trim();

  const name = stem(path);
  if (name === "_index") {
    const folder = path.split("/").slice(-2, -1)[0];
    if (folder) return folder.replace(/-/g, " ");
  }
  // A note named only for its date — a daily, a weekly — keeps its dashes.
  // "2026 07 13" is not a friendlier way to write a date.
  if (/^\d{4}-\d{2}(-\d{2})?$/.test(name)) return name;
  const words = name.replace(/-/g, " ").trim();
  return words === "" ? path : words;
}
