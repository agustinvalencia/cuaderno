// Step 4 — Commitments lookahead (read-only, plan §1.4). The next two
// weeks of dated promises, rendered by the same shared timeline the
// Commitments view and (later) the Strategic view use — filter empty,
// so every context shows. Read-only: its "save" is a local "mark as
// looked at" that writes nothing.
import CommitmentsTimeline from "../../components/commitments/CommitmentsTimeline";
import type { CommitmentEntry } from "../../api/bindings/CommitmentEntry";

// An empty active set means "all contexts" — the lookahead never
// pre-filters.
const NO_FILTER = new Set<never>();

export default function LookaheadStep({
  entries,
  today,
  onLookedAt,
}: {
  entries: CommitmentEntry[];
  today: string;
  onLookedAt: () => void;
}) {
  return (
    <div>
      <h2 className="font-medium text-ink">The next two weeks</h2>
      <p className="mt-1 text-sm text-ink-muted">What's already promised — nothing to add here.</p>

      {entries.length === 0 ? (
        <p className="mt-3 rounded border border-line bg-bg-surface p-6 text-ink-muted">
          Nothing promised in the next two weeks.
        </p>
      ) : (
        <CommitmentsTimeline entries={entries} today={today} filter={NO_FILTER} />
      )}

      <div className="mt-4">
        <button
          type="button"
          onClick={onLookedAt}
          className="rounded border border-line px-3 py-1 text-sm text-ink-muted hover:bg-bg-sunken hover:text-ink"
        >
          Looked at these
        </button>
      </div>
    </div>
  );
}
