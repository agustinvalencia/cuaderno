// Actions view (plan §1.2; reworked in #445) — the operational track's
// working surface: the list you consult when you have twenty minutes and
// want to spend them well.
//
// It used to be one unbounded column of every open bullet on every active
// project — no way to jump to a project, no context filter, no search, and
// rows that wrapped to two or three lines beside single-line neighbours.
// Fifty ragged rows in one scroll is the opposite of "pick one thing".
//
// So: a project rail with counts on the left, a sticky filter bar on the
// right, and rows that stay one line until you ask for more.
//
// The energy filter matters more here than anywhere else in the app. RLM
// tags work by cognitive cost precisely so you can match it to the state
// you are actually in; the old filter tested `a.energy === energy`, so an
// untagged bullet silently vanished the moment you touched it — the one
// mechanism the method offers for this, quietly dropping work on the
// floor. "Untagged" is now a chip of its own, and every chip carries its
// count, so nothing disappears without saying so.
import { useMemo, useState, type ReactNode } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router";
import type { ActionListEntry } from "../../api/bindings/ActionListEntry";
import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import type { ProjectActions } from "../../api/bindings/ProjectActions";
import { completeAction, errorMessage, listAllActions, promoteAction } from "../../api/commands";
import AmbiguityPicker from "../../components/ambiguity/AmbiguityPicker";
import { useAmbiguityResolver } from "../../components/ambiguity/useAmbiguityResolver";
import { ClampedText } from "../../components/ui/clamped-text";
import { SectionHeading } from "../../components/ui/section-heading";
import { actionLabel } from "../../lib/actionLabel";
import { CONTEXTS, contextDotClass, contextLabel, type Context } from "../../lib/contexts";
import { useReader } from "../../shell/reader";
import { useToast } from "../../shell/Toasts";

const KEY = ["list_all_actions"];

/** The energy chips, in ascending cost, plus the bucket the old filter
 * had no name for. A bullet carries no energy suffix far more often than
 * not; "untagged" being unnameable is what made it unreachable. */
type EnergyFilter = EnergyLevel | "untagged";
const ENERGIES: EnergyFilter[] = ["deep", "medium", "light", "untagged"];

/** Which chip an action answers to. */
function energyOf(action: ActionListEntry): EnergyFilter {
  return action.energy ?? "untagged";
}

/** Everything the filter bar and the rail narrow by. */
export interface Filters {
  project: string | null;
  contexts: Set<Context>;
  energies: Set<EnergyFilter>;
  /** Already trimmed and lower-cased. */
  query: string;
}

/** Does this bullet survive the row-level filters? An empty chip set is
 * "no opinion", not "nothing". */
function actionMatches(action: ActionListEntry, f: Filters): boolean {
  if (f.energies.size > 0 && !f.energies.has(energyOf(action))) return false;
  if (f.query && !actionLabel(action.text).toLowerCase().includes(f.query)) return false;
  return true;
}

/** Does this project survive the group-level filters? */
function groupMatches(group: ProjectActions, f: Filters): boolean {
  if (f.project !== null && group.slug !== f.project) return false;
  if (f.contexts.size > 0 && !f.contexts.has(group.context)) return false;
  return true;
}

/** Apply a filter set and drop the projects it empties — a filter narrows
 * to what matches, it does not parade empty projects.
 *
 * Order is not this function's business: the view sorts once, up front, so
 * the rail and the list cannot disagree, and filtering preserves it.
 * Within a project, bullets keep the order they have in the project map.
 * That order is the author's — RLM's "1-3 next actions" is a ranked list,
 * not a bag — so re-sorting it here would discard the one piece of
 * prioritisation the method asks you to make. */
export function applyFilters(groups: ProjectActions[], f: Filters): ProjectActions[] {
  return groups
    .filter((group) => groupMatches(group, f))
    .map((group) => ({ ...group, actions: group.actions.filter((a) => actionMatches(a, f)) }))
    .filter((group) => group.actions.length > 0);
}

/** How many bullets a filter set would leave. */
export function countUnder(groups: ProjectActions[], f: Filters): number {
  return applyFilters(groups, f).reduce((total, group) => total + group.actions.length, 0);
}

/** Toggle one member of a set, returning a new set. */
function toggled<T>(set: Set<T>, value: T): Set<T> {
  const next = new Set(set);
  if (!next.delete(value)) next.add(value);
  return next;
}

/** The two writes a row can make, hoisted to the view.
 *
 * There used to be one `useAmbiguityResolver` and one mounted
 * `AmbiguityPicker` per row — a dialog's worth of state per action on the
 * page, for a dialog only one of which can ever be open. One resolver
 * serves the whole list; the row passes its own project and bullet. */
function useActionWrites() {
  const client = useQueryClient();
  const { toast } = useToast();
  const ambiguity = useAmbiguityResolver();

  // Bullet text is an action's identity here — the React key, the
  // optimistic filter, and the backend argument all key on it. A
  // disambiguation pick re-fires the same mutation with an exact
  // candidate string, so the variables carry the text rather than the
  // mutation closing over it.
  const complete = useMutation({
    mutationFn: ({ slug, text }: { slug: string; text: string }) => completeAction(slug, text),
    onMutate: async ({ slug, text }) => {
      await client.cancelQueries({ queryKey: KEY });
      const previous = client.getQueryData<ProjectActions[]>(KEY);
      client.setQueryData<ProjectActions[]>(KEY, (groups) =>
        (groups ?? []).map((group) =>
          group.slug === slug
            ? { ...group, actions: group.actions.filter((a) => a.text !== text) }
            : group,
        ),
      );
      return { previous };
    },
    onError: (err, { slug }, context) => {
      if (context?.previous) client.setQueryData(KEY, context.previous);
      if (ambiguity.handle(err, (choice) => complete.mutateAsync({ slug, text: choice }), "action"))
        return;
      toast(errorMessage(err), "attention");
    },
    onSuccess: (_result, { slug }) => toast(`Done: one step further on ${slug}.`),
    onSettled: () => client.invalidateQueries({ queryKey: KEY }),
  });

  const promote = useMutation({
    mutationFn: ({ slug, text }: { slug: string; text: string }) => promoteAction(slug, text),
    onError: (err, { slug }) => {
      if (ambiguity.handle(err, (choice) => promote.mutateAsync({ slug, text: choice }), "action"))
        return;
      toast(errorMessage(err), "attention");
    },
    onSuccess: () => toast("Promoted to an action note."),
    onSettled: () => client.invalidateQueries({ queryKey: KEY }),
  });

  return { complete, promote, ambiguity };
}

type ActionWrites = ReturnType<typeof useActionWrites>;

export default function Actions() {
  const { data, isPending, isError, error } = useQuery({ queryKey: KEY, queryFn: listAllActions });
  const [project, setProject] = useState<string | null>(null);
  const [contexts, setContexts] = useState<Set<Context>>(new Set());
  const [energies, setEnergies] = useState<Set<EnergyFilter>>(new Set());
  const [rawQuery, setRawQuery] = useState("");
  const writes = useActionWrites();

  // Sorted once, here: the rail and the list both read from this, so
  // ordering established in one place cannot drift between them. The
  // index yields active projects in no particular order.
  const groups = useMemo(
    () => [...(data ?? [])].sort((a, b) => a.slug.localeCompare(b.slug)),
    [data],
  );

  if (isPending) {
    return <p className="p-8 text-ink-muted">Reading the vault…</p>;
  }
  if (isError) {
    return (
      <div className="p-8">
        <p className="text-ink">Actions could not be read.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(error)}</p>
      </div>
    );
  }

  const query = rawQuery.trim().toLowerCase();
  const filters: Filters = { project, contexts, energies, query };
  const shown = applyFilters(groups, filters);
  const total = groups.reduce((n, group) => n + group.actions.length, 0);
  const showing = shown.reduce((n, group) => n + group.actions.length, 0);
  const filtered = project !== null || contexts.size > 0 || energies.size > 0 || query !== "";

  // Every count answers "what would I get if I clicked this" — so each is
  // taken with its own dimension replaced and the others held.
  const countWith = (override: Partial<Filters>) => countUnder(groups, { ...filters, ...override });

  // Only the contexts actually present get a chip: seven of which five
  // read zero is not a filter, it is a wall (lead with what is there).
  const presentContexts = CONTEXTS.filter((c) => groups.some((g) => g.context === c));

  if (total === 0) {
    return (
      <div className="mx-auto max-w-5xl p-8">
        <h1 className="text-xl font-semibold text-ink">Actions</h1>
        <p className="mt-8 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          No open actions anywhere. Enjoy it.
        </p>
      </div>
    );
  }

  return (
    <div className="mx-auto max-w-5xl p-8">
      <h1 className="text-xl font-semibold text-ink">Actions</h1>

      <div className="mt-6 grid grid-cols-1 gap-8 md:grid-cols-[13rem_1fr]">
        {/* The rail. Sticky so a long list never leaves you scrolling back
            up to change project. */}
        <nav aria-label="Projects" className="md:sticky md:top-0 md:self-start">
          <ul className="space-y-0.5">
            <RailItem
              label="All actions"
              count={countWith({ project: null })}
              selected={project === null}
              onSelect={() => setProject(null)}
            />
            {groups.map((group) => (
              <RailItem
                key={group.slug}
                label={group.slug}
                context={group.context}
                count={countWith({ project: group.slug })}
                selected={project === group.slug}
                onSelect={() => setProject(project === group.slug ? null : group.slug)}
              />
            ))}
          </ul>
        </nav>

        <div className="min-w-0">
          {/* Sticky within the shell's scroller, and opaque, so the bar
              you filter with stays reachable from the bottom of a long
              list. */}
          <div className="sticky top-0 z-10 -mx-2 bg-bg-base px-2 pb-3 pt-1">
            <div role="group" aria-label="Filter by context" className="flex flex-wrap items-center gap-1.5">
              {presentContexts.map((context) => (
                <Chip
                  key={context}
                  label={contextLabel(context)}
                  active={contexts.has(context)}
                  count={countWith({ contexts: new Set([context]) })}
                  onClick={() => setContexts(toggled(contexts, context))}
                >
                  <span
                    aria-hidden
                    className={`h-2 w-2 shrink-0 rounded-full ${contextDotClass(context)}`}
                  />
                </Chip>
              ))}
            </div>

            <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
              <div role="group" aria-label="Filter by energy" className="flex flex-wrap gap-1.5">
                {ENERGIES.map((level) => (
                  <Chip
                    key={level}
                    label={level}
                    active={energies.has(level)}
                    count={countWith({ energies: new Set([level]) })}
                    onClick={() => setEnergies(toggled(energies, level))}
                  />
                ))}
              </div>
              <label htmlFor="action-search" className="sr-only">
                Filter actions by text
              </label>
              <input
                id="action-search"
                type="search"
                value={rawQuery}
                onChange={(event) => setRawQuery(event.target.value)}
                placeholder="Filter…"
                className="ml-auto w-36 rounded border border-line bg-bg-surface px-2 py-1 text-xs text-ink outline-none placeholder:text-ink-faint"
              />
            </div>

            {filtered && (
              // What a filter is hiding, said plainly. The old one gave no
              // count at all, so a chip that silently dropped every
              // untagged bullet looked like an empty vault.
              <p className="mt-2 flex items-center gap-2 text-xs text-ink-faint">
                <span>
                  Showing {showing} of {total}.
                </span>
                <button
                  type="button"
                  onClick={() => {
                    setProject(null);
                    setContexts(new Set());
                    setEnergies(new Set());
                    setRawQuery("");
                  }}
                  className="rounded underline decoration-dotted underline-offset-2 hover:text-ink"
                >
                  Clear
                </button>
              </p>
            )}
          </div>

          {shown.length === 0 ? (
            <p className="mt-4 text-sm text-ink-muted">
              Nothing matches that. <span className="text-ink-faint">{total} open in all.</span>
            </p>
          ) : (
            <div className="mt-1 space-y-6">
              {shown.map((group) => (
                <section key={group.slug} aria-label={group.slug}>
                  <div className="flex items-center gap-2">
                    <span
                      aria-hidden
                      className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(group.context)}`}
                    />
                    <SectionHeading>
                      <Link to={`/projects/${group.slug}`} className="hover:text-ink">
                        {group.slug}
                      </Link>
                    </SectionHeading>
                    <span className="text-xs text-ink-faint">{group.actions.length}</span>
                  </div>
                  <ul className="mt-2 space-y-1">
                    {group.actions.map((action) => (
                      <ActionRow key={action.text} slug={group.slug} action={action} writes={writes} />
                    ))}
                  </ul>
                </section>
              ))}
            </div>
          )}
        </div>
      </div>

      {/* One picker for the whole list, not one per row. */}
      <AmbiguityPicker
        state={writes.ambiguity.state}
        resolving={writes.ambiguity.resolving}
        choose={writes.ambiguity.choose}
        close={writes.ambiguity.close}
      />
    </div>
  );
}

/** One project in the rail: its name, its life-context dot, and how many
 * actions it holds under the filters that are not the rail. */
function RailItem({
  label,
  context,
  count,
  selected,
  onSelect,
}: {
  label: string;
  context?: Context;
  count: number;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <li>
      <button
        type="button"
        aria-current={selected ? "true" : undefined}
        onClick={onSelect}
        className={`flex w-full items-center gap-2 rounded px-2 py-1 text-left text-sm ${
          selected ? "bg-bg-sunken font-medium text-ink" : "text-ink-muted hover:text-ink"
        }`}
      >
        {context !== undefined && (
          <span aria-hidden className={`h-2 w-2 shrink-0 rounded-full ${contextDotClass(context)}`} />
        )}
        <span className="min-w-0 flex-1 truncate">{label}</span>
        {/* Not behind `useMetrics()`: a count of things to do is the list's
            own length, not a progress reading about how you are doing. */}
        <span className="shrink-0 text-xs text-ink-faint">{count}</span>
      </button>
    </li>
  );
}

/** A filter chip carrying its own count, so choosing one is never a leap.
 *
 * The count rides in the accessible name ("deep, 4") rather than only in
 * the visual — a chip whose number is decoration to a screen reader is
 * the same silent drop in a different form. */
function Chip({
  label,
  active,
  count,
  onClick,
  children,
}: {
  label: string;
  active: boolean;
  count: number;
  onClick: () => void;
  children?: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      aria-label={`${label}, ${count}`}
      onClick={onClick}
      // A chip that would show nothing recedes rather than disappearing:
      // its absence would be the same silent drop this rework is undoing.
      className={`flex items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs ${
        active
          ? "border-line bg-bg-sunken font-medium text-ink"
          : count === 0
            ? "border-line text-ink-faint"
            : "border-line text-ink-muted hover:text-ink"
      }`}
    >
      {children}
      <span aria-hidden>{label}</span>
      <span aria-hidden className="text-ink-faint">
        {count}
      </span>
    </button>
  );
}

function ActionRow({
  slug,
  action,
  writes,
}: {
  slug: string;
  action: ActionListEntry;
  writes: ActionWrites;
}) {
  const { openReader } = useReader();
  const { complete, promote } = writes;
  // Row-level pending, from the in-flight mutation's own variables: one
  // shared mutation serves every row, so `isPending` alone would grey out
  // the whole list.
  const busy = (m: typeof complete | typeof promote) =>
    m.isPending && m.variables?.slug === slug && m.variables?.text === action.text;

  return (
    <li className="flex items-start gap-2 rounded border border-line bg-bg-surface px-3 py-2">
      <span aria-hidden className="text-sm leading-5 text-ink-faint">
        →
      </span>
      {/* One line until it earns more. A long action used to wrap to two
          or three lines beside single-line neighbours, which is what made
          the list read ragged; `ClampedText` measures the overflow and
          offers "more" only when there is more. */}
      <ClampedText
        collapsedClass="max-h-5"
        className="min-w-0 flex-1 text-sm leading-5 text-ink"
        resetKey={action.text}
      >
        {actionLabel(action.text)}
      </ClampedText>
      {action.energy && (
        <span className="shrink-0 text-xs leading-5 text-ink-faint">({action.energy})</span>
      )}
      {action.attached ? (
        <button
          type="button"
          onClick={() => openReader(`actions/${action.attached!.slug}.md`)}
          className="shrink-0 rounded bg-bg-sunken px-1.5 py-0.5 text-xs leading-5 text-ink-muted hover:text-ink"
        >
          note
        </button>
      ) : (
        <button
          type="button"
          onClick={() => promote.mutate({ slug, text: action.text })}
          disabled={busy(promote)}
          aria-label={`Promote to a note: ${actionLabel(action.text)}`}
          className="shrink-0 rounded px-2 py-0.5 text-xs leading-5 text-ink-muted hover:text-ink"
        >
          promote
        </button>
      )}
      <button
        type="button"
        onClick={() => complete.mutate({ slug, text: action.text })}
        disabled={busy(complete)}
        aria-label={`Mark done: ${actionLabel(action.text)}`}
        className="shrink-0 rounded px-2 py-0.5 text-xs leading-5 text-ink-muted hover:text-ink"
      >
        done
      </button>
    </li>
  );
}
