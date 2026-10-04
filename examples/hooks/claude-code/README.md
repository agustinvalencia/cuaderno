# Claude Code hook: current focus injection

## What this does

This hook runs `cdno now --line` before each user prompt in Claude Code, injecting one line of context into every turn so agents see your current focus without being asked. The injected line looks like:

```
Focus: surrogate-model — Implement surrogate-model analysis (since 10:15)
```

or, when no focus is active:

```
Focus: none (last paused: surrogate-model — Implement surrogate-model analysis, next: …)
```

This single line is what turns RFC 0005 §5.6 from advice — "agents should notice when you move off the focus" — into behaviour: the agent sees it on every turn and can respond accordingly.

## Why it's needed

The CLI's discovery process walks **upward** from the current working directory to find the vault. When Claude Code is running, the session's working directory is the project being coded in, not the vault itself. So `CUADERNO_VAULT_PATH` or `--vault` must name the vault explicitly; upward discovery alone would find the wrong vault or none at all.

## Performance cost

One warm run of `cdno now --line` on a typical vault takes 20–100 milliseconds. On a vault with several hundred actions or a slow disk, reconciliation of the index can add up to ~5 seconds if another process holds the vault lock and the index is stale. This is rare; the hook prints nothing and returns successfully even when it stalls.

## Installation

1. Copy `focus.sh` to a location on your PATH, or note its absolute path.

2. Merge the contents of `settings.snippet.json` into your Claude Code settings:

   - **Global settings** (`~/.claude/settings.json`): Add or merge the `hooks` object. If you already have other hooks, add this entry to the existing `UserPromptSubmit` array, or create it if absent.
   - **Project-local settings** (`.claude/settings.json` in your project root): Do the same, scoped to your project.

   Example merge for global settings (replace `path/to/focus.sh` with the actual path):

   ```json
   {
     "hooks": {
       "UserPromptSubmit": [
         {
           "type": "command",
           "command": "path/to/focus.sh"
         }
       ]
     }
   }
   ```

3. Set the `CUADERNO_VAULT_PATH` environment variable to your vault's absolute path:

   ```bash
   export CUADERNO_VAULT_PATH=/path/to/your/vault
   ```

   If the variable is unset or empty, the hook prints nothing and exits cleanly.

## Behaviour on error

The hook always exits with code 0, even if:

- `CUADERNO_VAULT_PATH` is unset or empty — prints nothing
- `cdno` is not on PATH — prints nothing
- `cdno now --line` fails — prints nothing
- The vault is being written to by another process — may stall up to ~5 seconds, then print nothing or the current focus

This design ensures the hook can never block or break a turn.

## See also

- **RFC 0005 §5.6**: The detour protocol agents follow when they see the focus
- **RFC 0005 §3.3**: Performance measurement that informed this hook's design
- `examples/skills/` for agent skills that consume the focus injected by this hook
