// Token-backed hue per life context (the `Context` union is generated
// from the Rust enum). Colour signals context, never urgency — design
// law.
import type { Context } from "../api/bindings/Context";

export type { Context };

/** The seven life contexts, in a stable display order (the three hue
 * families of plan §1.3: work-leaning, then personal, then civic).
 * Enumerated here rather than derived because the `Context` union is a
 * type, not a runtime value — this list is the runtime mirror the
 * filter chips iterate. */
export const CONTEXTS: Context[] = [
  "work",
  "university",
  "side-project",
  "personal",
  "family",
  "household",
  "legal",
];

/** Human label for a context chip — the kebab slug read as words. */
export function contextLabel(context: Context): string {
  return context.replace(/-/g, " ");
}

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
