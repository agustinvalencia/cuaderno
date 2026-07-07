// Token-backed hue per life context (the `Context` union is generated
// from the Rust enum). Colour signals context, never urgency — design
// law.
import type { Context } from "../api/bindings/Context";

export type { Context };

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
