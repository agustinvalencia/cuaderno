// The editing state + save/validate machinery behind the Config view
// (#365, hoisted in PR5a). Extracted verbatim from the old inline
// ConfigEditor so both the raw <textarea> editor and PR5b's structured
// FORM can bind to the SAME draft/baseline dirty model, the SAME
// validate -> compare-and-swap -> write -> live-reload save gate, and the
// SAME conflict-then-reload recovery. Pure refactor: no behaviour change.
//
// The backend stays the single source of truth for validity — this hook
// only renders what validate_config / save_config report. An invalid
// draft is a normal editing state (attention tier, never red).
import { useEffect, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import type { ConfigDocument } from "../../api/bindings/ConfigDocument";
import type { ConfigSaveError } from "../../api/bindings/ConfigSaveError";
import {
  errorMessage,
  readConfig,
  saveConfig,
  validateConfig,
  type ValidationResult,
} from "../../api/commands";
import { useToast } from "../../shell/Toasts";

/** How long after the last keystroke the editor auto-validates the draft.
 * Long enough not to fire mid-word, short enough to feel live. */
export const VALIDATE_DEBOUNCE_MS = 400;

/** Everything the Config view (raw editor now, structured form in PR5b)
 * needs to read and drive the shared draft. */
export interface ConfigDraft {
  /** The current buffer — what the editor renders and Save persists. */
  draft: string;
  /** Replace the buffer (a raw-editor keystroke, or a form field edit in
   * PR5b). Clears a pending conflict, mirroring a fresh user action. */
  setDraft: (next: string) => void;
  /** The last-loaded (or last-saved) content the dirty check compares
   * against. */
  baseline: string;
  /** The content hash the next save echoes back for its compare-and-swap. */
  hash: string;
  /** Whether the draft diverges from the baseline (enables Save). */
  dirty: boolean;
  /** The last validation outcome (null until first checked). */
  validation: ValidationResult | null;
  /** Set when a save was refused because the file changed on disk under
   * the editor (a compare-and-swap conflict) — a distinct reload-first
   * state. */
  conflict: boolean;
  /** Persist the current draft through the backend save gate. */
  save: () => void;
  /** Whether a save is in flight (disables Save). */
  saving: boolean;
  /** Dry-run the backend validation against the current draft, now. */
  check: () => void;
  /** Whether an explicit Check is in flight. */
  checking: boolean;
  /** Discard the draft and re-seed from the fresh on-disk config — the
   * recovery from a compare-and-swap conflict. */
  reloadFromDisk: () => void;
}

export function useConfigDraft(doc: ConfigDocument): ConfigDraft {
  const client = useQueryClient();
  const { toast } = useToast();

  // `baseline` is the last-loaded (or last-saved) content; `draft` is the
  // live buffer; `hash` is the content hash the next save echoes back for
  // its compare-and-swap. All three seed from the loaded doc and move
  // together on a successful save.
  const [baseline, setBaseline] = useState(doc.content);
  const [draft, setDraftState] = useState(doc.content);
  const [hash, setHash] = useState(doc.hash);

  // The last validation outcome (null until the draft is first checked).
  const [validation, setValidation] = useState<ValidationResult | null>(null);
  // Set when a save is rejected because the file changed on disk under the
  // editor (a compare-and-swap conflict) — a distinct, reload-first state.
  const [conflict, setConflict] = useState(false);

  const dirty = draft !== baseline;

  // A fresh edit supersedes a prior conflict — the user is acting; let the
  // next save (or reload) re-establish state.
  function setDraft(next: string) {
    setDraftState(next);
    if (conflict) setConflict(false);
  }

  // Debounced pre-save validation: after the draft settles, dry-run the
  // backend's exact check so the author sees an invalid config before
  // pressing Save. The stale-closure guard (`cancelled`) drops a result
  // whose draft has already been superseded.
  useEffect(() => {
    let cancelled = false;
    const timer = setTimeout(() => {
      void validateConfig(draft).then((result) => {
        if (!cancelled) setValidation(result);
      });
    }, VALIDATE_DEBOUNCE_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [draft]);

  // An explicit Check runs the same validation immediately (no debounce).
  const check = useMutation({
    mutationFn: () => validateConfig(draft),
    onSuccess: setValidation,
  });

  const save = useMutation({
    mutationFn: (content: string) => saveConfig(content, hash),
    onSuccess: (saved) => {
      // The saved content + fresh hash become the new baseline, so the
      // editor is clean again and the next save's compare-and-swap uses
      // the up-to-date hash without a re-fetch.
      setBaseline(saved.content);
      setDraftState(saved.content);
      setHash(saved.hash);
      setConflict(false);
      setValidation({ ok: true });
      toast("Saved and applied.");
    },
    onError: (err) => {
      // `saveConfig` throws the tagged `ConfigSaveError` verbatim — switch
      // on its kind to route each failure to the right surface. (react-query
      // types the callback arg as `Error`; the real thrown value is the
      // serialised tagged union, so cast through `unknown`.)
      const payload = err as unknown as ConfigSaveError;
      switch (payload.kind) {
        case "validation":
          // Surface it inline exactly like the pre-save check, so a
          // rejection and a dry-run read identically.
          setValidation({ ok: false, error: payload.data });
          break;
        case "conflict":
          // The file moved under the editor — prompt a reload rather than
          // clobber the newer file.
          setConflict(true);
          break;
        default:
          toast(errorMessage(new Error(payload.data)), "attention");
      }
    },
  });

  // Reload the on-disk config, discarding the current draft — the
  // recovery from a compare-and-swap conflict. Refetches the read query
  // and re-seeds the editor from the fresh document.
  function reloadFromDisk() {
    void client
      .invalidateQueries({ queryKey: ["read_config"] })
      .then(() => client.fetchQuery({ queryKey: ["read_config"], queryFn: readConfig }))
      .then((fresh) => {
        setBaseline(fresh.content);
        setDraftState(fresh.content);
        setHash(fresh.hash);
        setConflict(false);
        setValidation(null);
      });
  }

  return {
    draft,
    setDraft,
    baseline,
    hash,
    dirty,
    validation,
    conflict,
    save: () => save.mutate(draft),
    saving: save.isPending,
    check: () => check.mutate(),
    checking: check.isPending,
    reloadFromDisk,
  };
}
