// Config view (#365). PR1 shipped a read-only inspector; PR3 made it an
// editable raw `.cuaderno/config.toml` editor; PR5a added a Form/Raw
// toggle whose Form side rendered the parsed config read-only; PR5b makes
// that Form side EDITABLE (add/edit/remove note types and schema fields).
//
// The raw editor is a <textarea> on the draft/baseline dirty model
// (mirroring the Templates editor), with a Save that runs the backend's
// validate -> compare-and-swap -> write -> live-reload gate, inline
// validation shown pre-save (debounced, and via an explicit Check), and a
// distinct "changed on disk" notice on a compare-and-swap conflict. All
// that editing machinery now lives in the shared `useConfigDraft` hook,
// so the raw editor and PR5b's form bind to the same model.
//
// The backend is the single source of truth for validity — the editor
// never re-derives config constraints, it only renders what
// validate_config / save_config report. Validation notices use the calm
// attention tier (no red): an invalid draft is a normal editing state,
// not an error.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import type { ConfigDocument } from "../../api/bindings/ConfigDocument";
import { readConfig, type ValidationResult } from "../../api/commands";
import ConfigStructuredView from "./ConfigStructuredView";
import { useConfigDraft, type ConfigDraft } from "./useConfigDraft";

/** Which side of the Form/Raw toggle is showing. Defaults to `"raw"`:
 * even though the Form is editable as of PR5b, the raw editor remains the
 * least-surprise landing and the full-fidelity safety net (it can reach
 * every key, including the `[variables]` block the form omits). The Form
 * is one click away. */
type ViewMode = "raw" | "structured";

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
  // Key the view by the loaded hash so a hard external reload (a new
  // document identity) resets the draft/baseline state cleanly.
  return <ConfigView key={read.data.hash} doc={read.data} />;
}

function ConfigView({ doc }: { doc: ConfigDocument }) {
  const [mode, setMode] = useState<ViewMode>("raw");
  // The shared draft model. Held here (above both panels) so it survives
  // toggling and is the single seam PR5b's editable form binds to.
  const cfg = useConfigDraft(doc);

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
          <ModeToggle mode={mode} onChange={setMode} />
          {/* Check + Save act on the shared draft, so they serve BOTH the
              raw editor and the form (a form edit dirties the same draft). */}
          <button
            type="button"
            onClick={cfg.check}
            disabled={cfg.checking}
            className="shrink-0 rounded border border-line px-3 py-1 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
          >
            Check
          </button>
          <button
            type="button"
            onClick={cfg.save}
            disabled={!cfg.dirty || cfg.saving}
            className="shrink-0 rounded border border-line px-3 py-1 text-xs text-ink hover:bg-bg-sunken disabled:opacity-50"
          >
            Save
          </button>
        </header>

        <div className="px-4 py-3">
          {/* The notices (conflict or validation) are shared: a form edit
              runs the same validate/save gate, so a rejection surfaces
              identically whichever side is showing. */}
          {cfg.conflict ? (
            <ConflictNotice onReload={cfg.reloadFromDisk} />
          ) : (
            cfg.validation !== null && <CheckResult result={cfg.validation} />
          )}
          {mode === "raw" ? <ConfigEditor cfg={cfg} /> : <ConfigStructuredView cfg={cfg} />}
        </div>
      </div>
    </div>
  );
}

/** The Form/Raw segmented toggle. "Form" is the structured editor (PR5b);
 * "Raw" is the editable <textarea>. Mirrors the calendar view's
 * segmented-control styling. */
function ModeToggle({
  mode,
  onChange,
}: {
  mode: ViewMode;
  onChange: (next: ViewMode) => void;
}) {
  const modes: { value: ViewMode; label: string }[] = [
    { value: "raw", label: "Raw" },
    { value: "structured", label: "Form" },
  ];
  return (
    <div className="flex shrink-0 gap-1" role="group" aria-label="Config view mode">
      {modes.map(({ value, label }) => (
        <button
          key={value}
          type="button"
          aria-pressed={mode === value}
          onClick={() => onChange(value)}
          className={`rounded px-2 py-1 text-xs ${
            mode === value
              ? "bg-bg-sunken font-medium text-ink"
              : "text-ink-muted hover:text-ink"
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

/** The presentational raw editor: just the <textarea> over the shared
 * draft. The validation/conflict notices and the Check/Save buttons live
 * in the view (they are shared with the form, which drives the same
 * draft). */
function ConfigEditor({ cfg }: { cfg: ConfigDraft }) {
  return (
    <>
      <label htmlFor="config-editor" className="sr-only">
        config.toml content
      </label>
      <textarea
        id="config-editor"
        value={cfg.draft}
        spellCheck={false}
        onChange={(event) => cfg.setDraft(event.target.value)}
        className="h-[32rem] w-full resize-y rounded border border-line bg-bg-base p-3 font-mono text-sm text-ink"
      />
    </>
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
