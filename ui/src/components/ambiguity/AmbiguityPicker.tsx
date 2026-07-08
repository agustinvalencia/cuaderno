// Disambiguation picker (#338) — the calm sibling of the project-cap
// modal. When a free-text selector matched more than one candidate, this
// lists the candidates and lets the user pick the one they meant;
// choosing re-invokes the original command with that exact string. This
// is UX, not an error: no red, no "be more specific" scolding — just a
// gentle "which of these did you mean?" over the same centred Radix
// Dialog the cap modal uses (focus trap, Esc, return-focus, and
// reduced-motion honoured by the global CSS).
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";
import type { AmbiguityResolver } from "./useAmbiguityResolver";

/** Driven entirely by a `useAmbiguityResolver` instance — spread its
 * fields in. `null` state keeps the dialog shut. */
export type AmbiguityPickerProps = Pick<
  AmbiguityResolver,
  "state" | "resolving" | "choose" | "close"
>;

export default function AmbiguityPicker({
  state,
  resolving,
  choose,
  close,
}: AmbiguityPickerProps) {
  return (
    <Dialog
      open={state !== null}
      onOpenChange={(next) => {
        // Radix drives open state; the only transition we own here is the
        // close (Esc / scrim / a programmatic dismiss).
        if (!next) close();
      }}
    >
      <DialogContent>
        <DialogTitle className="text-base font-medium text-ink">
          More than one {state?.noun ?? "match"} matched.
        </DialogTitle>
        <DialogDescription className="mt-1 text-sm text-ink-muted">
          {state ? (
            <>&ldquo;{state.query}&rdquo; could mean a few things. Which did you mean?</>
          ) : null}
        </DialogDescription>
        <ul className="mt-4 space-y-2">
          {state?.candidates.map((candidate) => (
            <li key={candidate}>
              <button
                type="button"
                onClick={() => choose(candidate)}
                disabled={resolving}
                className="w-full rounded-md border border-line bg-bg-surface px-3 py-2 text-left text-sm text-ink hover:bg-bg-sunken disabled:opacity-50"
              >
                {candidate}
              </button>
            </li>
          ))}
        </ul>
      </DialogContent>
    </Dialog>
  );
}
