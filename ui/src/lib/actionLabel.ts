// Human display form of a next-action bullet. The stored bullet text
// is preserved verbatim by the domain (it is the matching key for
// complete/promote), so it carries the trailing `(deep|medium|light)`
// energy suffix and any raw `[[target|label]]` wikilink. Views render
// the energy as their own tag, so showing the raw text doubled the
// energy and leaked wikilink syntax (spotted in v0.5.0 user testing).
// Display-only: never feed this back into a mutation.
const ENERGY_SUFFIX = /\s*\((?:deep|medium|light)\)\s*$/;
const WIKILINK = /\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g;

export function actionLabel(text: string): string {
  return text
    .replace(ENERGY_SUFFIX, "")
    .replace(WIKILINK, (_match, target: string, label?: string) => {
      if (label) return label;
      const segments = target.split("/");
      return segments[segments.length - 1] || target;
    })
    .trim();
}
