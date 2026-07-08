// Shared trend visualisations (Recharts). Extracted from
// StewardshipDetail (M7) when the Strategic view (M9) needed the same
// line chart plus a tiny sparkline sibling — one home so the two
// surfaces can't drift.
//
// These are STATUS visualisations, never goal trackers: no target
// lines, no red zones. Colours are drawn from the calm context hues via
// CSS variables (design law: colour is identity, never urgency — and no
// red token exists to misuse). Animation is suppressed under
// prefers-reduced-motion (plan §3.10) — the caller passes `animate`,
// seeded from `usePrefersReducedMotion` below.
import { useEffect, useState } from "react";
import { Line, LineChart, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { TrackingSeries } from "../../api/bindings/TrackingSeries";

// Series colours cycle through the context hues, drawn from CSS
// variables so they track the active theme.
export const SERIES_COLORS = [
  "var(--color-ctx-work)",
  "var(--color-ctx-university)",
  "var(--color-ctx-side-project)",
  "var(--color-ctx-personal)",
  "var(--color-ctx-family)",
  "var(--color-ctx-household)",
  "var(--color-ctx-legal)",
];

/** `8 Jul` / `Jul 8` per locale, at local midnight (no timezone slip). */
function shortDate(date: string): string {
  return new Date(`${date}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}

/** Reactive read of the reduced-motion preference — chart animation is
 * disabled when set (plan §3.10). Defaults to "no preference" where
 * matchMedia is unavailable (e.g. the test DOM). */
export function usePrefersReducedMotion(): boolean {
  // Seed from matchMedia in the initialiser (not a post-mount effect) so
  // a reduced-motion user never sees the one-frame animation flash from
  // an initial `false`. The effect below keeps it reactive to changes.
  const [reduced, setReduced] = useState(
    () => globalThis.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false,
  );
  useEffect(() => {
    const mq = globalThis.matchMedia?.("(prefers-reduced-motion: reduce)");
    if (!mq) return;
    setReduced(mq.matches);
    const onChange = () => setReduced(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return reduced;
}

/** One compact trend chart (~160px). Muted caption, axis text in
 * ink-faint, no grid — calm by construction. No reference/target lines:
 * these show status, not goals. */
export function TrendChart({
  series,
  color,
  animate,
}: {
  series: TrackingSeries;
  color: string;
  animate: boolean;
}) {
  const points = series.points.map((p) => ({ date: p.date, value: p.value }));
  // A single-point series draws no line segment, so a normal r:2 dot is
  // nearly invisible — a first tracking entry would read as an empty
  // chart. Render a clearly visible dot instead.
  const dotRadius = points.length === 1 ? 4 : 2;
  return (
    <figure>
      <figcaption className="text-xs text-ink-muted">{series.name}</figcaption>
      <div className="mt-1 h-40">
        <ResponsiveContainer width="100%" height="100%">
          <LineChart data={points} margin={{ top: 8, right: 8, bottom: 4, left: 0 }}>
            <XAxis
              dataKey="date"
              tickFormatter={shortDate}
              tick={{ fill: "var(--color-ink-faint)", fontSize: 11 }}
              tickLine={false}
              axisLine={{ stroke: "var(--color-line)" }}
              minTickGap={24}
            />
            <YAxis
              width={36}
              tick={{ fill: "var(--color-ink-faint)", fontSize: 11 }}
              tickLine={false}
              axisLine={false}
            />
            <Tooltip
              labelFormatter={(label) => shortDate(String(label))}
              contentStyle={{
                background: "var(--color-bg-surface)",
                border: "1px solid var(--color-line)",
                borderRadius: 6,
                fontSize: 12,
                color: "var(--color-ink)",
              }}
            />
            <Line
              type="monotone"
              dataKey="value"
              stroke={color}
              strokeWidth={2}
              dot={{ r: dotRadius, fill: color }}
              isAnimationActive={animate}
            />
          </LineChart>
        </ResponsiveContainer>
      </div>
    </figure>
  );
}

/** A tiny inline sparkline — 24px tall, line only, no axes, no grid, no
 * tooltip. Draws a bare rhythm (e.g. entries-per-week) in a context hue.
 * `values` is a plain numeric series the caller has already bucketed
 * (the frontend does no date maths — plan §3.7). Renders nothing for an
 * empty series so a flat/untracked stewardship shows no spark at all. */
export function Sparkline({
  values,
  color,
  animate,
  label,
}: {
  values: number[];
  color: string;
  animate: boolean;
  /** Accessible description of the trend (e.g. "gym: 12-week trend"). */
  label?: string;
}) {
  if (values.length === 0) return null;
  const data = values.map((value, index) => ({ index, value }));
  return (
    <div className="h-6 w-24" role="img" aria-label={label}>
      <ResponsiveContainer width="100%" height="100%">
        <LineChart data={data} margin={{ top: 2, right: 2, bottom: 2, left: 2 }}>
          <Line
            type="monotone"
            dataKey="value"
            stroke={color}
            strokeWidth={1.5}
            dot={false}
            isAnimationActive={animate}
          />
        </LineChart>
      </ResponsiveContainer>
    </div>
  );
}
