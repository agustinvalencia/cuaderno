// Project Detail (plan §1.8) — the full project map behind
// `/projects/:slug`. Structured, writable sections for the daily
// micro-edits (current state, next actions, waiting-on, milestones)
// sit above the map rendered verbatim ("The map as written"), so the
// interactive surfaces never hide anything the author wrote. Backlinks
// and log mentions are quiet, clickable context. A parked project
// renders read-only: history stays visible, the write affordances fall
// away.
import { actionLabel } from "../../lib/actionLabel";
import { useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "react-router";
import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import type { ProjectDetail as ProjectDetailData } from "../../api/bindings/ProjectDetail";
import {
  activateProject,
  addAction,
  addWaitingOn,
  completeAction,
  completeMilestone,
  errorMessage,
  getProject,
  openInEditor,
  parkProject,
  resolveWaiting,
  resolveWikilink,
  updateProjectState,
} from "../../api/commands";
import AmbiguityPicker from "../../components/ambiguity/AmbiguityPicker";
import { useAmbiguityResolver } from "../../components/ambiguity/useAmbiguityResolver";
import Markdown from "../../components/markdown/Markdown";
import { LogCard } from "../../components/ui/log-card";
import { contextDotClass } from "../../lib/contexts";
import { useMetrics } from "../../lib/metrics";
import { useReader } from "../../shell/reader";
import { shortDate } from "../../lib/dates";
import { SectionHeading } from "../../components/ui/section-heading";
import { useToast } from "../../shell/Toasts";

const ENERGIES: EnergyLevel[] = ["deep", "medium", "light"];

/** The project map's note path for open-in-editor — parked maps live
 * under `_parked/`. */
function projectPath(slug: string, parked: boolean): string {
  return parked ? `projects/_parked/${slug}.md` : `projects/${slug}.md`;
}

/** Pull the body of a `## <heading>` section out of the map markdown,
 * to seed the inline editor. Returns "" when the section is absent —
 * the editor then starts blank, and the save writes a fresh section. */
function extractSection(body: string, heading: string): string {
  const lines = body.split("\n");
  const start = lines.findIndex((l) => /^##\s/.test(l) && l.replace(/^##\s+/, "").trim() === heading);
  if (start < 0) return "";
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((l) => /^##\s/.test(l));
  return (end < 0 ? rest : rest.slice(0, end)).join("\n").trim();
}

export default function ProjectDetail() {
  const { slug = "" } = useParams();
  const { data, isPending, isError, error } = useQuery({
    queryKey: ["get_project", slug],
    queryFn: () => getProject(slug),
  });

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">This project could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  return <ProjectDetailBody slug={slug} data={data} />;
}

function ProjectDetailBody({ slug, data }: { slug: string; data: ProjectDetailData }) {
  const client = useQueryClient();
  const { toast } = useToast();
  const navigate = useNavigate();
  const { openReader } = useReader();
  const showMetrics = useMetrics();
  const parked = data.status === "parked";
  const key = ["get_project", slug];

  // A single picker serves this view's three substring-matched writes
  // (complete action, resolve blocker, tick milestone). Each mutation's
  // onError hands the resolver a re-invoke via `mutateAsync`, so a chosen
  // candidate reuses that mutation's own success/rollback/toast path.
  const ambiguity = useAmbiguityResolver();

  const [editingState, setEditingState] = useState(false);
  const stateDraft = useRef<HTMLTextAreaElement>(null);
  const [energy, setEnergy] = useState<EnergyLevel>("medium");
  const actionDraft = useRef<HTMLInputElement>(null);
  const waitingDraft = useRef<HTMLInputElement>(null);
  const resolveDraft = useRef<HTMLInputElement>(null);

  function invalidate() {
    void client.invalidateQueries({ queryKey: key });
  }

  // A wikilink in the map body: resolve to typed navigation (project /
  // stewardship views) or open the linked note in the shell reader.
  // Unresolved targets are a no-op — the anchor already rendered muted.
  async function onWikilink(target: string) {
    let resolved;
    try {
      resolved = await resolveWikilink(target);
    } catch {
      return;
    }
    if (!resolved) return;
    if (resolved.note_type === "project") {
      navigate(`/projects/${resolved.path.split("/").pop()?.replace(/\.md$/i, "")}`);
    } else if (resolved.note_type === "stewardship") {
      navigate("/stewardships");
    } else {
      openReader(resolved.path);
    }
  }

  const saveState = useMutation({
    mutationFn: (newState: string) => updateProjectState(slug, newState),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => setEditingState(false),
    onSettled: invalidate,
  });

  const lifecycle = useMutation({
    mutationFn: () => (parked ? activateProject(slug) : parkProject(slug)),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => {
      toast(parked ? `${slug} is active again.` : `${slug} parked — room to breathe.`);
      // Lifecycle changes the sidebar's active set too.
      void client.invalidateQueries({ queryKey: ["get_orientation"] });
    },
    onSettled: invalidate,
  });

  // Optimistic done: drop the bullet from the cached detail immediately
  // (cheap, high-frequency), roll back on error.
  //
  // Bullet text is the action's identity throughout this view — the
  // React key, the optimistic filter below, and the `completeAction`
  // argument all key on `action.text`. Two bullets with identical text
  // would collide (and the backend's case-insensitive substring match
  // would reject the pair as ambiguous). We accept that: distinct next
  // actions read as distinct text in practice.
  const complete = useMutation({
    mutationFn: (text: string) => completeAction(slug, text),
    onMutate: async (text) => {
      await client.cancelQueries({ queryKey: key });
      const previous = client.getQueryData<ProjectDetailData>(key);
      client.setQueryData<ProjectDetailData>(key, (view) =>
        view ? { ...view, actions: view.actions.filter((a) => a.text !== text) } : view,
      );
      return { previous };
    },
    onError: (err, _text, context) => {
      if (context?.previous) client.setQueryData(key, context.previous);
      // A bullet substring matching several actions opens the picker;
      // choosing re-runs this same mutation with the exact text.
      if (ambiguity.handle(err, (choice) => complete.mutateAsync(choice), "action")) return;
      toast(errorMessage(err), "attention");
    },
    onSuccess: () => toast(`Done: one step further on ${slug}.`),
    onSettled: invalidate,
  });

  const add = useMutation({
    mutationFn: (text: string) => addAction(slug, text, energy),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => {
      if (actionDraft.current) actionDraft.current.value = "";
    },
    onSettled: invalidate,
  });

  const addWaiting = useMutation({
    mutationFn: (item: string) => addWaitingOn(slug, item),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => {
      if (waitingDraft.current) waitingDraft.current.value = "";
      toast("Noted — waiting on that now.");
    },
    onSettled: invalidate,
  });

  const resolveWait = useMutation({
    mutationFn: (query: string) => resolveWaiting(slug, query),
    onError: (err) => {
      // The free-text resolve box is the likeliest ambiguity: a short
      // query can match several blockers. Pick the one meant.
      if (ambiguity.handle(err, (choice) => resolveWait.mutateAsync(choice), "blocker")) return;
      toast(errorMessage(err), "attention");
    },
    onSuccess: () => {
      if (resolveDraft.current) resolveDraft.current.value = "";
      toast("Unblocked.");
    },
    onSettled: invalidate,
  });

  const tickMilestone = useMutation({
    mutationFn: (name: string) => completeMilestone(slug, name),
    onError: (err) => {
      if (ambiguity.handle(err, (choice) => tickMilestone.mutateAsync(choice), "milestone")) return;
      toast(errorMessage(err), "attention");
    },
    onSuccess: (_data, name) => toast(`Milestone reached: ${name}.`),
    onSettled: () => {
      invalidate();
      // A completed milestone also drops out of the commitments window.
      void client.invalidateQueries({ queryKey: ["get_commitments"] });
    },
  });

  const backlinkGroups: [string, string[]][] = [
    ["portfolios", data.backlinks.portfolios],
    ["questions", data.backlinks.questions],
    ["evidence", data.backlinks.evidence],
    ["actions", data.backlinks.actions],
    ["other", data.backlinks.other],
  ];
  const hasBacklinks = backlinkGroups.some(([, paths]) => paths.length > 0);
  const editorPath = projectPath(slug, parked);

  return (
    <div className="mx-auto max-w-3xl p-8">
      <header className="flex items-center gap-3">
        <span
          aria-hidden
          className={`h-3 w-3 shrink-0 rounded-full ${contextDotClass(data.context)}`}
        />
        <h1 className="min-w-0 flex-1 truncate text-xl font-semibold text-ink">{slug}</h1>
        <span className="rounded bg-bg-sunken px-2 py-0.5 text-xs text-ink-muted">
          {data.status}
        </span>
        <button
          type="button"
          onClick={() => lifecycle.mutate()}
          disabled={lifecycle.isPending}
          className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
        >
          {parked ? "Activate" : "Park"}
        </button>
        <button
          type="button"
          onClick={() => void openInEditor(editorPath)}
          className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
        >
          Open in editor
        </button>
      </header>

      {/* Current State — inline editor (active projects only). */}
      <section aria-label="Current state" className="mt-8">
        <SectionHeading>
          Current state
        </SectionHeading>
        {parked ? (
          <p className="mt-2 text-sm text-ink-muted">
            This project is parked. Activate it to edit its state.
          </p>
        ) : editingState ? (
          <form
            className="mt-2"
            onSubmit={(event) => {
              event.preventDefault();
              const draft = stateDraft.current?.value.trim();
              if (draft) saveState.mutate(draft);
            }}
          >
            <textarea
              ref={stateDraft}
              defaultValue={extractSection(data.body_markdown, "Current State")}
              rows={4}
              aria-label={`Current state of ${slug}`}
              className="w-full rounded border border-line bg-bg-base p-2 text-sm text-ink"
            />
            <div className="mt-1 flex gap-2">
              <button
                type="submit"
                disabled={saveState.isPending}
                className="rounded border border-line px-2 py-0.5 text-xs text-ink hover:bg-bg-sunken"
              >
                Save
              </button>
              <button
                type="button"
                onClick={() => setEditingState(false)}
                className="rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
              >
                Cancel
              </button>
            </div>
          </form>
        ) : (
          <button
            type="button"
            onClick={() => setEditingState(true)}
            aria-label={`Edit the current state of ${slug}`}
            className="mt-2 block rounded text-left text-sm text-accent-interactive hover:underline"
          >
            Edit current state
          </button>
        )}
      </section>

      {/* Next Actions — tick + quick-add (active only). */}
      <section aria-label="Next actions" className="mt-8">
        <SectionHeading>
          Next actions
        </SectionHeading>
        {data.actions.length === 0 ? (
          <p className="mt-2 text-sm text-ink-muted">
            {parked ? "Parked — actions resume on activation." : "No open actions."}
          </p>
        ) : (
          <ul className="mt-2 space-y-1">
            {data.actions.map((action) => (
              <li
                key={action.text}
                className="flex items-center gap-2 rounded border border-line bg-bg-surface px-3 py-2"
              >
                <span aria-hidden className="text-ink-faint">
                  →
                </span>
                <span className="min-w-0 flex-1 text-sm text-ink">{actionLabel(action.text)}</span>
                {action.energy && (
                  <span className="shrink-0 text-xs text-ink-faint">({action.energy})</span>
                )}
                <button
                  type="button"
                  onClick={() => complete.mutate(action.text)}
                  aria-label={`Mark done: ${actionLabel(action.text)}`}
                  className="shrink-0 rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
                >
                  done
                </button>
              </li>
            ))}
          </ul>
        )}
        {!parked && (
          <form
            className="mt-2 flex items-center gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              const text = actionDraft.current?.value.trim();
              if (text) add.mutate(text);
            }}
          >
            <input
              ref={actionDraft}
              type="text"
              aria-label="New next action"
              placeholder="Add a next action…"
              className="min-w-0 flex-1 rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
            />
            <select
              aria-label="Energy for the new action"
              value={energy}
              onChange={(event) => setEnergy(event.target.value as EnergyLevel)}
              className="rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
            >
              {ENERGIES.map((level) => (
                <option key={level} value={level}>
                  {level}
                </option>
              ))}
            </select>
            <button
              type="submit"
              disabled={add.isPending}
              className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
            >
              Add
            </button>
          </form>
        )}
      </section>

      {/* Waiting On — add + resolve quick-rows (no structured list in
          the bundle; the map body below shows the current items). */}
      {!parked && (
        <section aria-label="Waiting on" className="mt-8">
          <SectionHeading>
            Waiting on
          </SectionHeading>
          <form
            className="mt-2 flex items-center gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              const item = waitingDraft.current?.value.trim();
              if (item) addWaiting.mutate(item);
            }}
          >
            <input
              ref={waitingDraft}
              type="text"
              aria-label="New waiting-on blocker"
              placeholder="I'm now blocked on…"
              className="min-w-0 flex-1 rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
            />
            <button
              type="submit"
              disabled={addWaiting.isPending}
              className="rounded border border-line px-3 py-1 text-sm text-ink hover:bg-bg-sunken"
            >
              Add
            </button>
          </form>
          <form
            className="mt-2 flex items-center gap-2"
            onSubmit={(event) => {
              event.preventDefault();
              const query = resolveDraft.current?.value.trim();
              if (query) resolveWait.mutate(query);
            }}
          >
            <input
              ref={resolveDraft}
              type="text"
              aria-label="Resolve a waiting-on blocker"
              placeholder="Resolve waiting on…"
              className="min-w-0 flex-1 rounded border border-line bg-bg-base px-2 py-1 text-sm text-ink"
            />
            <button
              type="submit"
              disabled={resolveWait.isPending}
              className="rounded px-3 py-1 text-sm text-ink-muted hover:text-ink"
            >
              Resolve
            </button>
          </form>
        </section>
      )}

      {/* Milestones — tick to complete. */}
      {data.open_milestones.length > 0 && (
        <section aria-label="Milestones" className="mt-8">
          <SectionHeading>
            Milestones
          </SectionHeading>
          <ul className="mt-2 space-y-1">
            {data.open_milestones.map((milestone) => (
              <li
                key={milestone.name}
                className="flex items-center gap-2 rounded border border-line bg-bg-surface px-3 py-2"
              >
                <span className="min-w-0 flex-1 text-sm text-ink">{milestone.name}</span>
                {milestone.is_hard && (
                  <span className="shrink-0 rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-muted">
                    hard:
                  </span>
                )}
                {milestone.date && (
                  <span className="shrink-0 text-xs text-ink-faint">
                    {shortDate(milestone.date)}
                  </span>
                )}
                {!parked && (
                  <button
                    type="button"
                    onClick={() => tickMilestone.mutate(milestone.name)}
                    aria-label={`Mark milestone reached: ${milestone.name}`}
                    className="shrink-0 rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
                  >
                    done
                  </button>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}

      {/* Backlinks — quiet, clickable rows opening the note reader. */}
      {hasBacklinks && (
        <section aria-label="Backlinks" className="mt-8">
          <SectionHeading>
            Linked from
          </SectionHeading>
          {backlinkGroups.map(([group, paths]) =>
            paths.length === 0 ? null : (
              <div key={group} className="mt-2">
                <p className="text-xs text-ink-faint">{group}</p>
                <ul className="mt-1 space-y-0.5">
                  {paths.map((path) => (
                    <li key={path}>
                      <button
                        type="button"
                        onClick={() => openReader(path)}
                        className="truncate text-left text-sm text-ink-muted hover:text-ink"
                      >
                        {path}
                      </button>
                    </li>
                  ))}
                </ul>
              </div>
            ),
          )}
        </section>
      )}

      {/* Recently in your logs. */}
      {data.log_mentions.length > 0 && (
        <section aria-label="Recent log mentions" className="mt-8">
          <SectionHeading>
            Recently in your logs
          </SectionHeading>
          <div className="mt-2 space-y-1.5">
            {data.log_mentions.map((mention, index) => (
              <LogCard
                key={`${mention.date}-${mention.time}-${index}`}
                date={shortDate(mention.date)}
                time={mention.time.slice(0, 5)}
              >
                {mention.text}
              </LogCard>
            ))}
          </div>
        </section>
      )}

      {/* Metrics (opt-in): the one honest "% done" a project has. */}
      {showMetrics && data.open_milestones.length > 0 && (
        <p className="mt-6 text-xs text-ink-faint">{data.open_milestones.length} milestones open</p>
      )}

      {/* The map as written — the full body, so nothing is hidden. */}
      <section aria-label="The map as written" className="mt-10 border-t border-line pt-6">
        <SectionHeading>
          The map as written
        </SectionHeading>
        <div className="mt-2">
          <Markdown body={data.body_markdown} onWikilink={onWikilink} />
        </div>
      </section>

      <p className="mt-8 text-xs text-ink-faint">
        <Link to="/" className="hover:text-ink-muted">
          ← back to today
        </Link>
      </p>

      <AmbiguityPicker
        state={ambiguity.state}
        resolving={ambiguity.resolving}
        choose={ambiguity.choose}
        close={ambiguity.close}
      />
    </div>
  );
}
