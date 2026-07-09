// Config inspector (#365, PR1) — a read-only view of the vault's
// `.cuaderno/config.toml` plus a dry-run "Check" that runs the backend's
// exact validation (the same set `Vault::new` runs) and reports OK or the
// error inline. No editing or saving here: the raw editor + save lands in
// PR3, the structured form in PR5. This is purely an inspector.
import { useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import type { ConfigDocument } from "../../api/bindings/ConfigDocument";
import { readConfig, validateConfig, type ValidationResult } from "../../api/commands";

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
  return <ConfigBody doc={read.data} />;
}

function ConfigBody({ doc }: { doc: ConfigDocument }) {
  // The last Check outcome (null until the button is pressed). The check
  // runs against the file content as read — PR1 has no editor, so what is
  // validated is exactly what is displayed.
  const [result, setResult] = useState<ValidationResult | null>(null);

  const check = useMutation({
    mutationFn: () => validateConfig(doc.content),
    onSuccess: setResult,
  });

  return (
    <div className="mx-auto max-w-5xl p-8">
      <h1 className="text-xl font-semibold text-ink">Config</h1>
      <p className="mt-2 text-sm text-ink-muted">
        A read-only view of{" "}
        <code className="text-ink-faint">.cuaderno/config.toml</code>. Use{" "}
        <span className="text-ink">Check</span> to dry-run the same validation the
        app runs when it opens the vault. Editing lands in a later release.
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
        </header>

        <div className="px-4 py-3">
          {result !== null && <CheckResult result={result} />}

          {doc.content.length === 0 ? (
            <p className="text-sm text-ink-muted">
              This vault has no <code className="text-ink-faint">config.toml</code>{" "}
              yet — the built-in defaults apply.
            </p>
          ) : (
            <>
              <label htmlFor="config-content" className="sr-only">
                config.toml content
              </label>
              <pre
                id="config-content"
                className="max-h-[32rem] overflow-auto rounded border border-line bg-bg-base p-3 font-mono text-sm text-ink"
              >
                {doc.content}
              </pre>
            </>
          )}
        </div>
      </div>
    </div>
  );
}

/** The inline outcome of a Check: a calm OK confirmation or the backend's
 * validation error, with its source line/column when TOML reported one. */
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
