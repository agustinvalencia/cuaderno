// The section-label idiom — uppercase, tracked, faint — that marks a
// content section's title across the views (~20 open-coded copies). One
// component so the label style is defined once; a restyle is a single
// edit, not a grep-and-replace. Renders an `h2` by default (these mark
// document sections); pass `as="h3"` for the rare nested case. Extra
// layout classes (margins, padding) ride through `className` — the base
// carries none, so there's no conflict to resolve.
import type { ReactNode } from "react";

export function SectionHeading({
  children,
  className = "",
  as: As = "h2",
}: {
  children: ReactNode;
  className?: string;
  as?: "h2" | "h3";
}) {
  const base = "text-xs font-medium uppercase tracking-wider text-ink-faint";
  return <As className={className ? `${base} ${className}` : base}>{children}</As>;
}
