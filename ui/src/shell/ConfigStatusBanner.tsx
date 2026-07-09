// The invalid-config banner (GH #365 PR4). When an external edit to
// `.cuaderno/config.toml` fails to open, the backend keeps the last good
// config live and emits `config:status` {valid:false, message}; this
// banner tells the user their edit was not applied, calmly — attention
// (amber) tier, never red, matching the config editor's own tone. A
// later valid edit clears the notice. Renders nothing while valid (no
// empty frame).

import { useConfigStatus } from "../lib/configStatus";

export default function ConfigStatusBanner() {
  const { valid, message } = useConfigStatus();
  if (valid) return null;

  return (
    <div
      role="status"
      className="border-b border-line bg-bg-sunken px-4 py-2 text-sm text-attention"
    >
      config.toml has an error and was not applied — the app is still using your
      last valid config.
      {message !== null && (
        <span className="mt-1 block font-mono text-xs text-ink-faint">{message}</span>
      )}
    </div>
  );
}
