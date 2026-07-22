// The index-exclusions notice (#440). Reconciliation leaves files out of
// the index for two reasons: the config `ignore` globs, and attachment
// artefacts owned by an evidence stub. A file absent from the index is
// absent from search, lint and backlinks too — so when an `ignore` glob
// looks over-broad, the app says so instead of presenting a legitimately
// empty view and leaving the user to guess.
//
// This is the failure #440 was filed for: a glob meant to hide attachment
// files matched every portfolio note as well, and the Portfolios section
// read as broken rather than as a misconfigured vault.
//
// Only the over-broad case surfaces. Artefacts are excluded by design and
// a small deliberate ignore list (a `CLAUDE.md`) is housekeeping, not a
// mistake — the backend decides which is which. Calm tier, never red,
// matching the config banner beside it, and dismissible: it describes a
// condition to look into, not an error to acknowledge.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { getIndexExclusions } from "../api/commands";

export default function IndexExclusionsBanner() {
  const { data } = useQuery({
    queryKey: ["get_index_exclusions"],
    queryFn: getIndexExclusions,
    // Startup state: fixed for the process lifetime, so never refetch.
    staleTime: Infinity,
  });
  const [dismissed, setDismissed] = useState(false);

  if (!data?.ignore_looks_over_broad || dismissed) return null;

  const total = data.ignored + data.indexed + data.artefacts;
  return (
    <div
      role="status"
      className="flex items-start gap-3 border-b border-line bg-bg-sunken px-4 py-2 text-sm text-attention"
    >
      <div className="min-w-0 flex-1">
        {`Your \`ignore\` globs are excluding ${data.ignored} of ${total} notes from the index.`}
        <span className="mt-1 block text-xs text-ink-faint">
          Excluded notes are missing from search, lint and backlinks — so a view can look
          empty when the files are fine. Check <code>ignore</code> in{" "}
          <code>.cuaderno/config.toml</code>: a <code>**</code> matches at every depth below,
          so <code>folder/*/**</code> also catches the level you meant to keep. The files are
          untouched on disk; narrowing the pattern and reindexing restores every note.
        </span>
      </div>
      <button
        type="button"
        onClick={() => setDismissed(true)}
        className="shrink-0 rounded px-2 py-0.5 text-xs text-ink-muted hover:text-ink"
      >
        Dismiss
      </button>
    </div>
  );
}
