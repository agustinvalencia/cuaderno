# The HTTP server

`cdno-mcp-server` serves the same tool catalogue as the stdio `cdno-mcp`, over the MCP
**Streamable HTTP** transport, for clients that reach your vault remotely — most notably Claude's
custom-connector infrastructure, which connects from Anthropic's cloud for every surface
(web, desktop, **mobile**).

```bash
cdno-mcp-server --vault ~/vault              # listens on 127.0.0.1:8787, endpoint /mcp
```

## Security model — read this first

The binary implements **no authentication itself**, on purpose. Static bearer tokens are not
spec-legal for remote MCP connectors; real deployments terminate **OAuth 2.1 at an identity-aware
proxy** (for example Cloudflare Access with Managed OAuth in front of a Cloudflare Tunnel), and the
origin validates the identity assertion the proxy injects (tracked in
[#302](https://github.com/agustinvalencia/cuaderno/issues/302)).

Until that origin check lands, `cdno-mcp-server` **refuses to bind anything but loopback**.
Be precise about what that guarantees: the process only accepts connections arriving on its own
loopback interface. It cannot detect a tunnel or SSH forward that bridges the port outward —
**never bridge this port without the authenticating proxy in front.** The server logs a warning at
startup to the same effect whenever it serves real vault data.

## Flags

| Flag | Env | Default | Purpose |
|------|-----|---------|---------|
| `--vault <path>` | `CUADERNO_VAULT_PATH` | cwd | Vault root |
| `--bind <addr>` | `CDNO_MCP_BIND` | `127.0.0.1:8787` | Listen address (non-loopback refused until #302) |
| `--allowed-host <host>` | `CDNO_MCP_ALLOWED_HOSTS` (comma-separated) | — | Extra `Host` header values to accept on top of the loopback defaults (DNS-rebinding protection). A public deployment adds its hostname |
| `--smoke` | — | off | Serve a single `echo` tool holding **no vault handle** — prove tunnel/auth infrastructure end-to-end with zero vault exposure |
| `--read-only` | — | off | Advertise only the context-gathering read tools; mutating tools are absent from the dispatch table entirely |
| `--reconcile-interval-secs <n>` | `CDNO_MCP_RECONCILE_INTERVAL_SECS` | `300` | Periodic index reconciliation; `0` disables |

## Index freshness

Unlike a stdio session, this process is long-running while other writers — the CLI, editors, sync
tools — mutate the Markdown underneath it. Markdown is the source of truth and the index is a
cache, so the server re-runs the reconciliation pass on the configured interval as the correctness
backstop. Out-of-band edits become visible to `search_notes` and the context tools within one
interval at most.

## Transport details

- Endpoint: `POST /mcp`. Clients must send `Accept: application/json, text/event-stream`
  (the Streamable HTTP spec requires both).
- Stateless JSON mode: every request is self-contained; responses are plain `application/json`
  (no SSE streams, no session ids). `GET`/`DELETE` on `/mcp` return `405`.
- Guardrails: request bodies are capped at 1 MiB and in-flight requests are bounded; the `Host`
  header is validated against the allowlist (403 otherwise).
