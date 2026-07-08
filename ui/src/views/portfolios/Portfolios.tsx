// Portfolio selector (M8, plan §1.6; #58) — the calm index behind
// `/portfolios`. Each row is one per-question dossier: its question, an
// evidence count, and a muted staleness line. Freshness is rendered as
// NEUTRAL emphasis, never a hue — colour is identity, never urgency
// (design law, and no semantic green/red token exists). A fresh
// portfolio's dot + line sit at full ink; an ageing one fades to
// ink-muted; a long-dormant one recedes to ink-faint, with a
// "last updated N d ago" title on hover. Rows link to `/portfolios/:slug`.
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import type { PortfolioSummary } from "../../api/bindings/PortfolioSummary";
import { listPortfolios } from "../../api/commands";

// Day thresholds for the freshness tiers. Portfolios accumulate slowly
// (a paper a week is active), so the bands are generous — this is a
// gentle "how warm is this dossier", not a deadline.
const AGEING_AFTER_DAYS = 30n;
const STALE_AFTER_DAYS = 90n;

interface Freshness {
  /** Neutral ink tier for the dot + line (no hue). */
  tone: string;
  /** Hover title spelling out the age in days. */
  title: string;
  /** The muted status line: count plus how fresh it is. */
  line: string;
}

function freshness(s: PortfolioSummary): Freshness {
  const count = `${s.evidence_count} ${s.evidence_count === 1 ? "note" : "notes"}`;

  if (s.last_updated === null || s.staleness_days === null) {
    return {
      tone: "text-ink-faint",
      title: "no evidence filed yet",
      line: "no evidence yet",
    };
  }

  const days = s.staleness_days;
  const ago = days <= 0n ? "today" : `${days.toString()}d ago`;
  const title = days <= 0n ? "last updated today" : `last updated ${days.toString()} d ago`;
  const tone =
    days <= AGEING_AFTER_DAYS
      ? "text-ink"
      : days <= STALE_AFTER_DAYS
        ? "text-ink-muted"
        : "text-ink-faint";
  return { tone, title, line: `${count} · last filed ${ago}` };
}

export default function Portfolios() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["list_portfolios"],
    queryFn: listPortfolios,
  });

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">Portfolios could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-3xl p-8">
      <header>
        <h1 className="text-xl font-semibold text-ink">Portfolios</h1>
        <p className="mt-1 text-sm text-ink-muted">
          The questions you're collecting evidence against.
        </p>
      </header>

      {data.length === 0 ? (
        <p className="mt-6 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          No portfolios yet.
        </p>
      ) : (
        <ul className="mt-6 space-y-2">
          {data.map((p) => {
            const f = freshness(p);
            return (
              <li key={p.slug}>
                <Link
                  to={`/portfolios/${p.slug}`}
                  className="flex items-center gap-3 rounded-md border border-line bg-bg-surface px-3 py-2 hover:bg-bg-sunken"
                >
                  <span
                    aria-hidden
                    title={f.title}
                    className={`h-2.5 w-2.5 shrink-0 rounded-full bg-current ${f.tone}`}
                  />
                  <span className="min-w-0 flex-1 truncate text-sm text-ink">
                    {p.question || p.slug}
                  </span>
                  <span className={`shrink-0 text-xs ${f.tone}`} title={f.title}>
                    {f.line}
                  </span>
                </Link>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
