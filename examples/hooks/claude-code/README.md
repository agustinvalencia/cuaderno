# Claude Code hook: current focus injection

## What this does

This hook runs `cdno now --line` before each user prompt in Claude Code, injecting one line of context into every turn so agents see your current focus without being asked. Claude Code adds the hook's output to the conversation as context for that turn. The injected line looks like:

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

`cdno now --line` is index-free. It reads the config and the daily notes inside the `[focus]` windows (by default today and yesterday for the focus, and the last 14 days for the last pause), and nothing else: it never opens, creates or writes `.cuaderno/index.db`, never walks the vault, and never takes or waits for the vault's write lock. So its cost does not grow with the vault, and it is safe to run on every prompt beside a long-lived server (for example `cdno-mcp-server` in a container) that holds the same vault's index open. Only `now` is index-free: every other `cdno` verb, and shell completion (which opens the vault on every Tab), still opens and reconciles the index, so the rest of the CLI is not yet safe beside such a server (see [#1005](https://github.com/agustinvalencia/cuaderno/issues/1005)).

## Installation

1. Note `focus.sh`'s absolute path (Claude Code runs it by the path in `command`, so it does not need to be on your PATH). Use an absolute path or `$HOME/...` — a `command` without `args` runs through `sh -c`, so `$HOME` expands. Make sure the script is executable (`chmod +x focus.sh`).

2. Merge the contents of `settings.snippet.json` into your Claude Code settings:

   - **Global settings** (`~/.claude/settings.json`): Add or merge the `hooks` object. If you already have other hooks, add this entry to the existing `UserPromptSubmit` array, or create it if absent.
   - **Project-local settings**: the script and vault paths are machine-specific, so prefer `.claude/settings.local.json` (not committed) over `.claude/settings.json`.

   Example merge for global settings (replace `/absolute/path/to/...` with the actual path to your copied script):

   ```json
   {
     "hooks": {
       "UserPromptSubmit": [
         {
           "hooks": [
             {
               "type": "command",
               "command": "/absolute/path/to/examples/hooks/claude-code/focus.sh"
             }
           ]
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

   The hook inherits the environment of the process that launched Claude Code. An export in your
   shell profile reaches a `claude` started from that shell, but an IDE launched from the desktop
   may not see it — and the hook then silently prints nothing. Either launch from a shell that has
   the variable, or set it in the `env` key of your Claude Code settings.

Claude Code gives a `UserPromptSubmit` command hook 30 seconds by default (a `timeout` field
overrides it); `cdno now --line` waits on no lock, so no wrapper is needed.
A timeout does not block the prompt; only exit code 2 would, and this script never returns it.

## Behaviour on error

The hook always exits with code 0, even if:

- `CUADERNO_VAULT_PATH` is unset or empty — prints nothing
- `cdno` is not on PATH — prints nothing
- `cdno now --line` fails — prints nothing

Another process holding the vault's write lock does not delay it: the verb takes no lock.

This design ensures the hook can never block or break a turn.

## See also

- **RFC 0005 §5.6**: The detour protocol agents follow when they see the focus
- **RFC 0005 §3.3**: Performance measurement that informed this hook's design
- `examples/skills/` for agent skills that consume the focus injected by this hook
