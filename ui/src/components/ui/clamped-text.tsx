// Caps its content at a collapsed height with a soft bottom fade, and
// reveals a "more"/"less" toggle only when the content actually overflows
// that cap (UI request 2026-07-13). Keeps project cards uniform and
// scannable (design law: calm surfaces) when one entry — a project's
// surfaced next-action — carries a wall of text, without hiding it: the
// reader expands it in place rather than being sent elsewhere.
//
// Overflow is measured, never assumed: short content shows no fade and no
// toggle, so the affordance appears exactly when it earns its place. The
// fade is a CSS mask over the text itself — background-independent, so it
// dissolves cleanly in either theme with no colour to keep in sync.
import { useLayoutEffect, useRef, useState, type ReactNode } from "react";

// Fade the last stretch of the collapsed box to transparent. Applied only
// while collapsed-and-overflowing, so fully-shown content stays crisp.
const FADE = "linear-gradient(to bottom, black 55%, transparent)";

export function ClampedText({
  children,
  collapsedClass = "max-h-24",
  className = "",
}: {
  children: ReactNode;
  /** Tailwind max-height utility for the collapsed cap (default ~6rem). */
  collapsedClass?: string;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [expanded, setExpanded] = useState(false);
  const [overflowing, setOverflowing] = useState(false);

  useLayoutEffect(() => {
    const el = ref.current;
    // Measure only against the collapsed cap: while expanded, scrollHeight
    // equals clientHeight and would read as "not overflowing", wrongly
    // hiding the collapse control. Re-run on content or size changes.
    if (!el || expanded) return;
    const measure = () => setOverflowing(el.scrollHeight > el.clientHeight + 1);
    measure();
    // Live re-measurement is a nicety, not a requirement — the one-shot
    // measure above already sets the initial state. Guard the observer so a
    // host without `ResizeObserver` (e.g. jsdom under test) still works.
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [children, expanded]);

  const faded = overflowing && !expanded;

  return (
    <div>
      <div
        ref={ref}
        className={`${expanded ? "" : `${collapsedClass} overflow-hidden`} ${className}`}
        style={faded ? { maskImage: FADE, WebkitMaskImage: FADE } : undefined}
      >
        {children}
      </div>
      {overflowing && (
        <button
          type="button"
          onClick={() => setExpanded((value) => !value)}
          aria-expanded={expanded}
          className="mt-1 rounded text-xs text-ink-faint hover:text-ink"
        >
          {expanded ? "less" : "more"}
        </button>
      )}
    </div>
  );
}
