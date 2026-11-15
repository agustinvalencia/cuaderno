#!/usr/bin/env bash
set -u

# Exit immediately with success if vault path is not set or empty.
# cdno with --vault "" exits 2 (clap error), and a UserPromptSubmit hook
# exiting 2 blocks the user's prompt. Never call cdno in this case.
if [ -z "${CUADERNO_VAULT_PATH:-}" ]; then
  exit 0
fi

# cdno now --line is index-free: it reads the daily notes and the config and
# never opens, creates or writes .cuaderno/index.db, so it is safe to run on
# every prompt beside a server that holds the same vault's index open.
# stderr is suppressed so nothing but the focus line reaches the prompt.
# Capture output and always succeed. If cdno is not on PATH or exits
# non-zero, anything it printed is discarded.
output=$(cdno now --line --vault "$CUADERNO_VAULT_PATH" 2>/dev/null) || output=""

# Print output only if non-empty.
if [ -n "$output" ]; then
  printf '%s\n' "$output"
fi

exit 0
