// Config editor (#365, PR3) — an editable view of the vault's raw
// `.cuaderno/config.toml`. PR1 shipped a read-only inspector; this makes
// it an editor: a <textarea> on the draft/baseline dirty model (mirroring
// the Templates editor), a Save that runs the backend's validate ->
// compare-and-swap -> write -> live-reload gate, inline validation shown
// pre-save (debounced, and via an explicit Check), and a distinct
// "changed on disk" notice on a compare-and-swap conflict.
//
// The backend is the single source of truth for validity — the editor
// never re-derives config constraints, it only renders what
// validate_config / save_config report. Validation notices use the
// calm attention tier (no red): an invalid draft is a normal editing
// state, not an error. Raw editing only here; the structured form is PR5.
import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
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

export default function Config() {
  const read = useQuery({ queryKey: ["read_config"], queryFn: readConfig });

  if (read.isPending) {
    return <p className="p-8 text-ink-muted">Reading the config…</p>;
  }
  if (read.isError) {
    return (
      <div className="p-8">
        <p className="text-ink">The config could not be opened.</p>
        <p className="mt-2 text-sm text-ink-muted">{String(read.error)}</p>
      </div>
    );
  }
  // Key the editor by the loaded hash so a hard external reload (a new
  // document identity) resets the draft/baseline state cleanly.
  return <ConfigEditor key={read.data.hash} doc={read.data} />;
}

/** How long after the last keystroke the editor auto-validates the draft.
 * Long enough not to fire mid-word, short enough to feel live. */
const VALIDATE_DEBOUNCE_MS = 400;

function ConfigEditor({ doc }: { doc: ConfigDocument }) {
  const client = useQueryClient();
  const { toast } = useToast();

  // `baseline` is the last-loaded (or last-saved) content; `draft` is what
  // the textarea holds; `hash` is the content hash the next save echoes
  // back for its compare-and-swap. All three seed from the loaded doc and
  // move together on a successful save.
  const [baseline, setBaseline] = useState(doc.content);
  const [draft, setDraft] = useState(doc.content);
  const [hash, setHash] = useState(doc.hash);

  // The last validation outcome (null until the draft is first checked).
  const [validation, setValidation] = useState<ValidationResult | null>(null);
  // Set when a save is rejected because the file changed on disk under the
  // editor (a compare-and-swap conflict) — a distinct, reload-first state.
  const [conflict, setConflict] = useState(false);

  const dirty = draft !== baseline;

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
      setDraft(saved.content);
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
        setDraft(fresh.content);
        setHash(fresh.hash);
        setConflict(false);
        setValidation(null);
      });
  }

  return (
    <div className="mx-auto max-w-5xl p-8">
      <h1 className="text-xl font-semibold text-ink">Config</h1>
      <p className="mt-2 text-sm text-ink-muted">
        Edit <code className="text-ink-faint">.cuaderno/config.toml</code>{" "}
        directly. Saving validates the whole file first — a config that would
        not reopen is refused, so an edit here can never break the vault — then
        applies live, no restart. Use <span className="text-ink">Check</span>{" "}
        to dry-run the same validation before saving.
      </p>

      <div className="mt-6 rounded-lg border border-line bg-bg-surface">
        <header className="flex flex-wrap items-center gap-2 border-b border-line px-4 py-3">
          <h2 className="min-w-0 flex-1 truncate text-base font-semibold text-ink">
            config.toml
          </h2>
          <button
            type="button"
            onClick={() => check.mutate()}
            disabled={check.isPending}
            className="shrink-0 rounded border border-line px-3 py-1 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
          >
            Check
          </button>
          <button
            type="button"
            onClick={() => save.mutate(draft)}
            disabled={!dirty || save.isPending}
            className="shrink-0 rounded border border-line px-3 py-1 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
          >
            Save
          </button>
        </header>

        <div className="px-4 py-3">
          {conflict ? (
            <ConflictNotice onReload={reloadFromDisk} />
          ) : (
            validation !== null && <CheckResult result={validation} />
          )}

          <label htmlFor="config-editor" className="sr-only">
            config.toml content
          </label>
          <textarea
            id="config-editor"
            value={draft}
            spellCheck={false}
            onChange={(event) => {
              setDraft(event.target.value);
              // A fresh edit supersedes a prior conflict — the user is
              // acting; let the next save (or reload) re-establish state.
              if (conflict) setConflict(false);
            }}
            className="h-[32rem] w-full resize-y rounded border border-line bg-bg-base p-3 font-mono text-sm text-ink"
          />
        </div>
      </div>
    </div>
  );
}

/** The inline outcome of a validation (debounced, Check, or a save
 * rejection): a calm OK, or the backend's message with its source
 * line/column when TOML reported one. Attention tier, never red — an
 * invalid draft is a normal editing state. */
function CheckResult({ result }: { result: ValidationResult }) {
  if (result.ok) {
    return (
      <p role="status" className="mb-3 text-sm text-ink-muted">
        Config is valid — it would open cleanly.
      </p>
    );
  }
  const { message, line, col } = result.error;
  // Line/col only accompany a TOML syntax error; a semantic validation
  // error carries the message alone.
  const position =
    line !== null ? ` (line ${line}${col !== null ? `, column ${col}` : ""})` : "";
  return (
    <p role="status" className="mb-3 text-sm text-attention">
      Config is not valid{position}: {message}
    </p>
  );
}

/** The distinct compare-and-swap conflict notice: the on-disk file
 * changed since it was opened (a hand-edit landed underneath), so the
 * save was refused rather than clobber it. Offers a reload. */
function ConflictNotice({ onReload }: { onReload: () => void }) {
  return (
    <p role="status" className="mb-3 text-sm text-attention">
      The config changed on disk since it was opened — reload before saving so
      your edit does not overwrite the newer version.{" "}
      <button
        type="button"
        onClick={onReload}
        className="underline decoration-dotted underline-offset-2 hover:text-ink"
      >
        Reload
      </button>
    </p>
  );
}
