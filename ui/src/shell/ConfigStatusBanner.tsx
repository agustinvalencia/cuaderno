// The config-status banner (GH #365 PR4, #384). After an external edit to
// `.cuaderno/config.toml`, the backend keeps the last good config live and
// emits `config:status`:
//   - `invalid` — the edit could not be parsed/validated; it was not applied.
//   - `deferred` — the vault was busy, so an otherwise-fine edit hasn't
//     applied yet; it will on the next change to config.toml.
// Both render calmly — attention (amber) tier, never red, matching the
// config editor's own tone. `valid` clears the notice (renders nothing, no
// empty frame).

import { useConfigStatus } from "../lib/configStatus";

export default function ConfigStatusBanner() {
  const { health, message } = useConfigStatus();
  if (health === "valid") return null;

  const lead =
    health === "deferred"
      ? "The vault was busy, so this config change hasn't applied yet — it'll take effect on the next change to config.toml."
      : "config.toml has an error and was not applied — the app is still using your last valid config.";

  return (
    <div
      role="status"
      className="border-b border-line bg-bg-sunken px-4 py-2 text-sm text-attention"
    >
      {lead}
      {message !== null && (
        <span className="mt-1 block font-mono text-xs text-ink-faint">{message}</span>
      )}
    </div>
  );
}
