// Shared disambiguation resolver (#338). Several completion commands
// (complete_milestone, complete_action, resolve_waiting) take a
// free-text selector and match it as a case-insensitive substring. When
// a selector matches more than one candidate the backend returns a
// structured `CmdError::Ambiguous { query, candidates }` — designed for
// a picker, not a dead-end toast. This hook centralises the
// branch-on-kind + re-invoke logic so every write site handles ambiguity
// the same way: catch the error, open a picker of the candidates, and on
// a pick re-invoke the SAME command with the chosen (now-exact) string.
//
// The picker is UX, not failure (the CmdError doc-comment says as much),
// so the copy stays calm and the surface reuses the same Radix Dialog as
// the gentle project-cap modal — focus trap, Esc, and return-focus come
// for free, and reduced motion is honoured by the global CSS that
// neutralises the dialog animation.
import { useCallback, useRef, useState } from "react";
import { CuadernoError } from "../../api/commands";

/** The open picker's data — the query that was ambiguous, the candidate
 * strings to choose between, and the noun for the copy ("milestone",
 * "action", "blocker"). `null` when the picker is shut. */
export interface AmbiguityState {
  query: string;
  candidates: string[];
  noun: string;
}

/** A re-invoke of the command that raised the ambiguity, with the chosen
 * candidate substituted for the original free-text selector. Typically
 * `(choice) => someMutation.mutateAsync(choice)`, so the re-invoke reuses
 * the mutation's own success/rollback/toast handling. Resolves when the
 * command settles; the picker closes either way. */
export type AmbiguityRetry = (choice: string) => Promise<unknown>;

export interface AmbiguityResolver {
  /** The open picker's data, or `null` when nothing is being resolved. */
  state: AmbiguityState | null;
  /** True while a picked candidate's re-invoke is in flight — disables
   * the picker buttons so a second pick can't race the first. */
  resolving: boolean;
  /**
   * Call from a mutation's `onError`. If the error is an ambiguous match,
   * captures the re-invoke, opens the picker, and returns `true` — the
   * caller must NOT also toast. For any other error returns `false`, and
   * the caller handles it as usual (rollback + toast).
   *
   * @param noun what was matched, for the picker copy — defaults to the
   *   neutral "match".
   */
  handle: (err: unknown, retry: AmbiguityRetry, noun?: string) => boolean;
  /** Run the captured re-invoke with the picked candidate. */
  choose: (choice: string) => void;
  /** Dismiss the picker without choosing (Esc / scrim / cancel). */
  close: () => void;
}

export function useAmbiguityResolver(): AmbiguityResolver {
  const [state, setState] = useState<AmbiguityState | null>(null);
  const [resolving, setResolving] = useState(false);
  // The re-invoke is captured imperatively (a ref, not state) so the
  // closure over the failing command's other args survives a rerender
  // without forcing the picker to re-render when only the retry changes.
  const retryRef = useRef<AmbiguityRetry | null>(null);

  const handle = useCallback(
    (err: unknown, retry: AmbiguityRetry, noun = "match"): boolean => {
      if (err instanceof CuadernoError && err.payload.kind === "ambiguous") {
        const { query, candidates } = err.payload.data;
        // No candidates (an ambiguous *slug* — e.g. a stewardship that
        // exists both flat and expanded) is nothing to pick between; let
        // the caller toast it like any other fixable error.
        if (candidates.length === 0) return false;
        retryRef.current = retry;
        setState({ query, candidates, noun });
        return true;
      }
      return false;
    },
    [],
  );

  const close = useCallback(() => {
    setState(null);
    retryRef.current = null;
  }, []);

  const choose = useCallback(
    (choice: string) => {
      const retry = retryRef.current;
      if (!retry) return;
      setResolving(true);
      // Fire the re-invoke and close once it settles, whichever way. The
      // re-invoke (a mutation's `mutateAsync`) owns the user-facing
      // feedback — a success toast, or an attention toast on failure —
      // so the picker itself stays silent; its only job was to turn the
      // ambiguous selector into an exact one.
      Promise.resolve(retry(choice))
        .catch(() => {
          // Swallowed on purpose: the underlying mutation already
          // surfaced the failure. The picker just needs to close.
        })
        .finally(() => {
          setResolving(false);
          close();
        });
    },
    [close],
  );

  return { state, resolving, handle, choose, close };
}
