// The seven life contexts (mirrors cdno-domain's `Context` enum) and
// their token-backed hues. Colour signals context, never urgency —
// design law.
export const CONTEXTS = [
  "work",
  "university",
  "side-project",
  "personal",
  "family",
  "household",
  "legal",
] as const;

export type Context = (typeof CONTEXTS)[number];

/** Tailwind class fragments per context, resolved from theme tokens. */
export function contextDotClass(context: string): string {
  switch (context) {
    case "work":
      return "bg-ctx-work";
    case "university":
      return "bg-ctx-university";
    case "side-project":
      return "bg-ctx-side-project";
    case "personal":
      return "bg-ctx-personal";
    case "family":
      return "bg-ctx-family";
    case "household":
      return "bg-ctx-household";
    case "legal":
      return "bg-ctx-legal";
    default:
      return "bg-ink-faint";
  }
}
