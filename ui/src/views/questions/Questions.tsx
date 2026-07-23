// Important Questions (#443).
//
// RLM puts three-to-five research questions and three-to-five life
// questions *above* the project level, reviewed monthly, precisely to stop
// drift across months. They existed everywhere in cuaderno except the one
// place you look daily: the app surfaced them only as chips inside the
// Strategic dashboard, the view visited least. The thing meant to sit above
// projects was reachable only through the view you open monthly.
//
// So: grouped by domain, phrased as questions (the H1, never the slug),
// each showing what is pointed at it — because a question nothing links to
// is one nobody is working on, and that is the useful signal. And it is
// writable: a question you have answered should stop asking itself, or the
// page becomes another read-only dashboard.
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router";

import type { QuestionDomain } from "../../api/bindings/QuestionDomain";
import type { QuestionStatus } from "../../api/bindings/QuestionStatus";
import type { QuestionStrategicRow } from "../../api/bindings/QuestionStrategicRow";
import { errorMessage, listQuestions, setQuestionStatus } from "../../api/commands";
import { CappedList } from "../../components/ui/capped-list";
import { SectionHeading } from "../../components/ui/section-heading";
import { noteLabel } from "../../lib/noteLabel";
import { useReader } from "../../shell/reader";
import { useToast } from "../../shell/Toasts";

/** Domains in the order RLM names them: the work, then the life. */
const DOMAINS: { key: QuestionDomain; label: string; blurb: string }[] = [
  { key: "research", label: "Research", blurb: "What the work is trying to find out." },
  { key: "life", label: "Life", blurb: "What the years are for." },
];

/** Statuses a question can be moved to, and what each one means. */
const STATUSES: { key: QuestionStatus; label: string }[] = [
  { key: "active", label: "Active" },
  { key: "parked", label: "Parked" },
  { key: "answered", label: "Answered" },
  { key: "retired", label: "Retired" },
];

export default function Questions() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["list_questions"],
    queryFn: listQuestions,
  });

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">The questions could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-4xl p-8">
      <header>
        <h1 className="text-xl font-semibold text-ink">Questions</h1>
        <p className="mt-1 text-sm text-ink-muted">
          What you are actually trying to find out — above any one project, and worth
          re-reading when a month has gone by.
        </p>
      </header>

      {data.length === 0 ? (
        <p className="mt-8 rounded border border-line bg-bg-surface p-6 text-sm text-ink-muted">
          No questions yet. Your CLI or Claude can write one when you know what you are
          asking.
        </p>
      ) : (
        DOMAINS.map(({ key, label, blurb }) => (
          <DomainSection
            key={key}
            label={label}
            blurb={blurb}
            rows={data.filter((row) => row.summary.domain === key)}
          />
        ))
      )}
    </div>
  );
}

function DomainSection({
  label,
  blurb,
  rows,
}: {
  label: string;
  blurb: string;
  rows: QuestionStrategicRow[];
}) {
  if (rows.length === 0) return null;
  // Active first — the ones still being asked lead; the settled ones stay
  // reachable below rather than disappearing.
  const active = rows.filter((r) => r.summary.status === "active");
  const rest = rows.filter((r) => r.summary.status !== "active");

  return (
    <section aria-label={label} className="mt-8">
      <div className="flex items-baseline gap-3">
        <SectionHeading>{label}</SectionHeading>
        <span className="text-xs text-ink-faint">{blurb}</span>
      </div>
      <div className="mt-3 space-y-3">
        {active.map((row) => (
          <QuestionCard key={row.summary.slug} row={row} />
        ))}
      </div>
      {rest.length > 0 && (
        <div className="mt-3">
          <CappedList
            label={`settled ${label.toLowerCase()} questions`}
            limit={0}
            items={rest.map((row) => (
              <div key={row.summary.slug} className="mb-3">
                <QuestionCard row={row} />
              </div>
            ))}
          />
        </div>
      )}
    </section>
  );
}

function QuestionCard({ row }: { row: QuestionStrategicRow }) {
  const client = useQueryClient();
  const { toast } = useToast();
  const { openReader } = useReader();
  const { summary, backlinks } = row;

  const setStatus = useMutation({
    mutationFn: (status: QuestionStatus) => setQuestionStatus(summary.slug, status),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: (_data, status) => {
      const name = summary.question_text || summary.slug;
      toast(status === "answered" ? `Answered: ${name}.` : `${name} is now ${status}.`);
      void client.invalidateQueries({ queryKey: ["list_questions"] });
    },
  });

  const links: [string, string[]][] = [
    ["projects", backlinks.projects],
    ["portfolios", backlinks.portfolios],
    ["evidence", backlinks.evidence],
    ["other", backlinks.other],
  ];
  const linked = links.filter(([, paths]) => paths.length > 0);

  return (
    <article className="rounded-lg border border-line bg-bg-surface p-4">
      <div className="flex items-start gap-3">
        <h3 className={`min-w-0 flex-1 text-sm font-medium ${statusTone(summary.status)}`}>
          {summary.question_text || summary.slug}
        </h3>
        <select
          aria-label={`Status of ${summary.slug}`}
          value={summary.status}
          disabled={setStatus.isPending}
          onChange={(event) => setStatus.mutate(event.target.value as QuestionStatus)}
          className="shrink-0 rounded border border-line bg-bg-base px-2 py-0.5 text-xs text-ink"
        >
          {STATUSES.map(({ key, label }) => (
            <option key={key} value={key}>
              {label}
            </option>
          ))}
        </select>
      </div>

      {linked.length === 0 ? (
        // Not an error state: a question with nothing pointing at it is a
        // question nobody is working on, which is exactly what a monthly
        // read-through is looking for.
        <p className="mt-2 text-xs text-ink-faint">
          Nothing links here yet — no project, no evidence.
        </p>
      ) : (
        <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1">
          {linked.map(([group, paths]) => (
            <div key={group} className="flex flex-wrap items-baseline gap-1.5">
              <span className="text-xs text-ink-faint">{group}</span>
              {paths.map((path) => (
                <LinkChip key={path} path={path} onOpen={() => openReader(path)} />
              ))}
            </div>
          ))}
        </div>
      )}

      <p className="mt-2 text-xs text-ink-faint">last touched {summary.updated}</p>
    </article>
  );
}

/** A linked note: routes to its own view where one exists, else opens the
 * reader. */
function LinkChip({ path, onOpen }: { path: string; onOpen: () => void }) {
  const label = noteLabel(path, null);
  const chip =
    "rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-muted hover:text-ink";

  if (path.startsWith("projects/")) {
    const slug = path.split("/").pop()?.replace(/\.md$/i, "") ?? path;
    return (
      <Link to={`/projects/${slug}`} className={chip}>
        {label}
      </Link>
    );
  }
  if (path.startsWith("portfolios/")) {
    const slug = path.split("/")[1];
    return (
      <Link to={`/portfolios/${slug}`} className={chip}>
        {label}
      </Link>
    );
  }
  return (
    <button type="button" onClick={onOpen} className={chip}>
      {label}
    </button>
  );
}

/** A settled question recedes rather than vanishing — ink emphasis, never
 * a hue (the vault's standing law: colour is identity, not status). */
function statusTone(status: QuestionStatus): string {
  return status === "active" ? "text-ink" : "text-ink-muted";
}
