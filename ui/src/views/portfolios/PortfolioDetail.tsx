// Portfolio Detail (M8, plan §1.6; #58) — the dossier behind
// `/portfolios/:slug`. The plan's three-pane (selector → timeline →
// links) collapses to two here: a chronological evidence timeline
// (newest first, each row opening the note in the reader; its origin
// chip opens the producing note) and a links sidebar (the associated
// project and related questions). The quick-add composer is the app's
// ONLY note-creation form (sanctioned by #58): a slide-down form filing
// one evidence note. Its origin must name an existing note — the backend
// refuses a dangling link and the message shows inline.
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "react-router";
import type { PortfolioDetail as PortfolioDetailData } from "../../api/bindings/PortfolioDetail";
import {
  addEvidence,
  errorMessage,
  getPortfolio,
  openInEditor,
  resolveWikilink,
} from "../../api/commands";
import { useReader } from "../../shell/reader";
import { useToast } from "../../shell/Toasts";

/** `8 Jul` / `Jul 8` per locale, at local midnight (no timezone slip). */
function shortDate(date: string): string {
  return new Date(`${date}T00:00:00`).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}

/** The last path segment of a wikilink target, the slug a typed route
 * expects (`projects/surrogate-model` → `surrogate-model`). */
function lastSegment(target: string): string {
  return target.split("/").pop()?.replace(/\.md$/i, "") ?? target;
}

export default function PortfolioDetail() {
  const { slug = "" } = useParams();
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_portfolio", slug],
    queryFn: () => getPortfolio(slug),
  });

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">This portfolio could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  return <PortfolioDetailBody slug={slug} data={data} />;
}

function PortfolioDetailBody({ slug, data }: { slug: string; data: PortfolioDetailData }) {
  const navigate = useNavigate();
  const { openReader } = useReader();

  // A clicked link target (project frontmatter, related question, or an
  // evidence origin): a project opens its detail route, anything else
  // opens in the shell reader. Unresolvable targets are quietly ignored.
  async function openTarget(target: string) {
    let resolved;
    try {
      resolved = await resolveWikilink(target);
    } catch {
      return;
    }
    if (!resolved) return;
    if (resolved.note_type === "project") {
      navigate(`/projects/${lastSegment(resolved.path)}`);
    } else {
      openReader(resolved.path);
    }
  }

  return (
    <div className="mx-auto max-w-4xl p-8">
      <header className="flex items-baseline gap-3">
        <h1 className="min-w-0 flex-1 text-xl font-semibold text-ink">
          {data.question || slug}
        </h1>
        <span className="shrink-0 text-xs text-ink-faint">
          started {shortDate(data.created)}
        </span>
        <button
          type="button"
          onClick={() => void openInEditor(`portfolios/${slug}/_index.md`)}
          className="shrink-0 rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
        >
          Open in editor
        </button>
      </header>

      <div className="mt-8 grid grid-cols-1 gap-8 md:grid-cols-[1fr_16rem]">
        {/* Left: the evidence timeline + quick-add composer. */}
        <main className="min-w-0">
          <section aria-label="Evidence">
            <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
              Evidence
            </h2>
            {data.evidence.length === 0 ? (
              <p className="mt-3 rounded border border-line bg-bg-surface p-6 text-sm text-ink-muted">
                No evidence filed yet — this portfolio is waiting for its first artefact.
              </p>
            ) : (
              <ul className="mt-3 space-y-1">
                {data.evidence.map((row) => (
                  <li
                    key={row.path}
                    className="flex items-baseline gap-2 rounded border border-line bg-bg-surface px-3 py-2"
                  >
                    <button
                      type="button"
                      onClick={() => openReader(row.path)}
                      className="min-w-0 flex-1 truncate text-left text-sm text-ink hover:text-accent-interactive"
                    >
                      {row.source}
                    </button>
                    <span className="shrink-0 text-xs text-ink-faint">
                      {shortDate(row.created)}
                    </span>
                    <button
                      type="button"
                      onClick={() => void openTarget(row.origin)}
                      title={`origin: ${row.origin}`}
                      className="min-w-0 max-w-[40%] truncate rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-muted hover:text-ink"
                    >
                      {lastSegment(row.origin)}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <QuickAdd slug={slug} />
        </main>

        {/* Right: the links sidebar — the project and questions this
            portfolio hangs off. */}
        <aside aria-label="Links" className="space-y-6">
          <div>
            <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
              Project
            </h2>
            {data.project ? (
              <button
                type="button"
                onClick={() => navigate(`/projects/${lastSegment(data.project as string)}`)}
                className="mt-2 block w-full truncate rounded border border-line bg-bg-surface px-3 py-2 text-left text-sm text-ink hover:bg-bg-sunken"
              >
                {lastSegment(data.project)}
              </button>
            ) : (
              <p className="mt-2 text-sm text-ink-faint">Standalone — no project.</p>
            )}
          </div>

          <div>
            <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
              Questions
            </h2>
            {data.questions.length === 0 ? (
              <p className="mt-2 text-sm text-ink-faint">No linked questions.</p>
            ) : (
              <ul className="mt-2 space-y-1">
                {data.questions.map((q) => (
                  <li key={q}>
                    <button
                      type="button"
                      onClick={() => void openTarget(q)}
                      className="block w-full truncate rounded border border-line bg-bg-surface px-3 py-2 text-left text-sm text-ink hover:bg-bg-sunken"
                    >
                      {lastSegment(q)}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </aside>
      </div>

      <p className="mt-8 text-xs text-ink-faint">
        <Link to="/portfolios" className="hover:text-ink-muted">
          ← all portfolios
        </Link>
      </p>
    </div>
  );
}

/** The inline (not modal) quick-add composer: source + origin (both
 * required) + content. Origin must name an existing note — the backend
 * resolves it and refuses a dangling link, whose message we surface
 * inline beneath the field rather than only as a toast. */
function QuickAdd({ slug }: { slug: string }) {
  const client = useQueryClient();
  const { toast } = useToast();
  const [open, setOpen] = useState(false);
  const [source, setSource] = useState("");
  const [origin, setOrigin] = useState("");
  const [content, setContent] = useState("");

  function reset() {
    setSource("");
    setOrigin("");
    setContent("");
  }

  const submit = useMutation({
    mutationFn: () => addEvidence(slug, source.trim(), origin.trim(), content),
    onSuccess: () => {
      toast("Filed.");
      reset();
      setOpen(false);
    },
    onSettled: () => void client.invalidateQueries({ queryKey: ["get_portfolio", slug] }),
  });

  if (!open) {
    return (
      <div className="mt-10 border-t border-line pt-6">
        <button
          type="button"
          onClick={() => {
            // Clear any stale mutation error from a prior aborted attempt
            // so a reopened form never greets the user with an old error.
            submit.reset();
            setOpen(true);
          }}
          className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
        >
          File evidence
        </button>
      </div>
    );
  }

  const canSubmit = source.trim().length > 0 && origin.trim().length > 0 && !submit.isPending;

  return (
    <section aria-label="File evidence" className="mt-10 border-t border-line pt-6">
      <h2 className="text-xs font-medium uppercase tracking-wider text-ink-faint">
        File evidence
      </h2>
      <form
        className="mt-3 space-y-3"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSubmit) submit.mutate();
        }}
      >
        <div>
          <label htmlFor="evidence-source" className="block text-xs text-ink-muted">
            Source
          </label>
          <input
            id="evidence-source"
            value={source}
            onChange={(event) => setSource(event.target.value)}
            placeholder="citation, experiment id, conversation…"
            className="mt-1 w-full rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
          />
        </div>

        <div>
          <label htmlFor="evidence-origin" className="block text-xs text-ink-muted">
            Origin
          </label>
          <input
            id="evidence-origin"
            value={origin}
            onChange={(event) => setOrigin(event.target.value)}
            placeholder="projects/surrogate-model"
            className="mt-1 w-full rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
          />
          <p className="mt-1 text-xs text-ink-faint">
            Must name an existing note (the work this evidence came from).
          </p>
          {submit.isError && (
            <p className="mt-1 text-xs text-attention">{errorMessage(submit.error)}</p>
          )}
        </div>

        <div>
          <label htmlFor="evidence-content" className="block text-xs text-ink-muted">
            Notes
          </label>
          <textarea
            id="evidence-content"
            value={content}
            onChange={(event) => setContent(event.target.value)}
            rows={4}
            className="mt-1 w-full rounded border border-line bg-bg-base p-2 text-sm text-ink"
          />
        </div>

        <div className="flex gap-2">
          <button
            type="submit"
            disabled={!canSubmit}
            className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken disabled:opacity-50"
          >
            File it
          </button>
          <button
            type="button"
            onClick={() => {
              reset();
              // Also drop the mutation error, so reopening the (now
              // cleared) form doesn't resurface a stale refusal message.
              submit.reset();
              setOpen(false);
            }}
            className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
          >
            Cancel
          </button>
        </div>
      </form>
    </section>
  );
}
