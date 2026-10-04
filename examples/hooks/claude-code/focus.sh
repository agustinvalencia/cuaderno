#!/usr/bin/env bash
set -u

# Exit immediately with success if vault path is not set or empty.
# cdno with --vault "" exits 2 (clap error), and a UserPromptSubmit hook
# exiting 2 blocks the user's prompt. Never call cdno in this case.
if [ -z "${CUADERNO_VAULT_PATH:-}" ]; then
  exit 0
fi

# Run cdno now --line with stderr suppressed (reconciliation warnings from
# unparseable notes in the vault). Capture output and always succeed.
# If cdno is not on PATH or exits non-zero, output remains empty.
output=$(cdno now --line --vault "$CUADERNO_VAULT_PATH" 2>/dev/null) || true

# Print output only if non-empty.
if [ -n "$output" ]; then
  printf '%s\n' "$output"
fi

exit 0
