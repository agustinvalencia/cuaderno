// Actions view (plan §1.2, trimmed) — a single filterable list of open
// actions across every active project, grouped by project, filtered by
// energy. The cross-project complement to Home's per-project cards.
// Attached actions open in the shell note reader; unattached ones can
// be promoted to a manifest note in place.
import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "react-router";
import type { EnergyLevel } from "../../api/bindings/EnergyLevel";
import type { ProjectActions } from "../../api/bindings/ProjectActions";
import { completeAction, errorMessage, listAllActions, promoteAction } from "../../api/commands";
import { contextDotClass } from "../../lib/contexts";
import { useReader } from "../../shell/reader";
import { useToast } from "../../shell/Toasts";

const ENERGIES: EnergyLevel[] = ["deep", "medium", "light"];
const KEY = ["list_all_actions"];

function ActionRow({ slug, action }: { slug: string; action: ProjectActions["actions"][number] }) {
  const client = useQueryClient();
  const { toast } = useToast();
  const { openReader } = useReader();

  // Bullet text is the action's identity here — the React key (in the
  // parent map), the optimistic filter below, and the `completeAction`
  // argument all key on `action.text`. Two bullets with identical text
  // on one project would collide, matching the backend's own substring
  // matching (which would flag such a pair ambiguous). Accepted: distinct
  // actions read as distinct text.
  // Optimistic done: drop this bullet from the cached list immediately.
  const complete = useMutation({
    mutationFn: () => completeAction(slug, action.text),
    onMutate: async () => {
      await client.cancelQueries({ queryKey: KEY });
      const previous = client.getQueryData<ProjectActions[]>(KEY);
      client.setQueryData<ProjectActions[]>(KEY, (groups) =>
        (groups ?? []).map((group) =>
          group.slug === slug
            ? { ...group, actions: group.actions.filter((a) => a.text !== action.text) }
            : group,
        ),
      );
      return { previous };
    },
    onError: (err, _vars, context) => {
      if (context?.previous) client.setQueryData(KEY, context.previous);
      toast(errorMessage(err), "attention");
    },
    onSuccess: () => toast(`Done: one step further on ${slug}.`),
    onSettled: () => client.invalidateQueries({ queryKey: KEY }),
  });

  const promote = useMutation({
    mutationFn: () => promoteAction(slug, action.text),
    onError: (err) => toast(errorMessage(err), "attention"),
    onSuccess: () => toast("Promoted to an action note."),
    onSettled: () => client.invalidateQueries({ queryKey: KEY }),
  });

  return (
    <li className="flex items-center gap-2 rounded border border-line bg-bg-surface px-3 py-2">
      <span aria-hidden className="text-ink-faint">
        →
      </span>
      <span className="min-w-0 flex-1 text-sm text-ink">{action.text}</span>
      {action.energy && <span className="shrink-0 text-xs text-ink-faint">({action.energy})</span>}
      {action.attached ? (
        <button
          type="button"
          onClick={() => openReader(`actions/${action.attached!.slug}.md`)}
          className="shrink-0 rounded bg-bg-sunken px-1.5 py-0.5 text-xs text-ink-muted hover:text-ink"
        >
          note
        </button>
      ) : (
        <button
          type="button"
          onClick={() => promote.mutate()}
          disabled={promote.isPending}
          aria-label={`Promote to a note: ${action.text}`}
          className="shrink-0 rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
        >
          promote
        </button>
      )}
      <button
        type="button"
        onClick={() => complete.mutate()}
        aria-label={`Mark done: ${action.text}`}
        className="shrink-0 rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
      >
        done
      </button>
    </li>
  );
}

export default function Actions() {
  const { data, isPending, isError, error } = useQuery({
    queryKey: KEY,
    queryFn: listAllActions,
  });
  // Single-select energy filter (all = null), mirroring Home's chips.
  const [energy, setEnergy] = useState<EnergyLevel | null>(null);

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

  // Filter each group's bullets, then drop groups that empty out — a
  // filter narrows to what matches, it doesn't parade empty projects.
  const groups = data
    .map((group) => ({
      ...group,
      actions: energy ? group.actions.filter((a) => a.energy === energy) : group.actions,
    }))
    .filter((group) => group.actions.length > 0);

  const anyActions = data.some((group) => group.actions.length > 0);

  return (
    <div className="mx-auto max-w-3xl p-8">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-semibold text-ink">Actions</h1>
        <div role="group" aria-label="Energy filter" className="flex gap-1">
          {ENERGIES.map((level) => (
            <button
              key={level}
              type="button"
              aria-pressed={energy === level}
              onClick={() => setEnergy(energy === level ? null : level)}
              className={`rounded px-2 py-1 text-xs ${
                energy === level ? "bg-bg-sunken font-medium text-ink" : "text-ink-muted hover:text-ink"
              }`}
            >
              {level}
            </button>
          ))}
        </div>
      </div>

      {!anyActions ? (
        <p className="mt-8 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          No open actions anywhere. Enjoy it.
        </p>
      ) : groups.length === 0 ? (
        <p className="mt-8 text-sm text-ink-muted">No {energy} actions right now.</p>
      ) : (
        <div className="mt-6 space-y-6">
          {groups.map((group) => (
            <section key={group.slug} aria-label={group.slug}>
              <h2 className="flex items-center gap-2 text-sm font-medium">
                <span
                  aria-hidden
                  className={`h-2.5 w-2.5 shrink-0 rounded-full ${contextDotClass(group.context)}`}
                />
                <Link to={`/projects/${group.slug}`} className="text-ink hover:underline">
                  {group.slug}
                </Link>
              </h2>
              <ul className="mt-2 space-y-1">
                {group.actions.map((action) => (
                  <ActionRow key={action.text} slug={group.slug} action={action} />
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </div>
  );
}
