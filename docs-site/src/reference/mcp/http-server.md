# The HTTP server

`cdno-mcp-server` serves the same tool catalogue as the stdio `cdno-mcp`, over the MCP
**Streamable HTTP** transport, for clients that reach your vault remotely — most notably Claude's
custom-connector infrastructure, which connects from Anthropic's cloud for every surface
(web, desktop, **mobile**).

```bash
cdno-mcp-server --vault ~/vault              # listens on 127.0.0.1:8787, endpoint /mcp
```

## Security model — read this first

The binary issues **no OAuth of its own**, on purpose. Static bearer tokens are not spec-legal
for remote MCP connectors; real deployments terminate **OAuth 2.1 at an identity-aware proxy**
(for example Cloudflare Access with Managed OAuth in front of a Cloudflare Tunnel). The server's
own contribution is origin-side validation of the identity assertion the proxy injects
(`Cf-Access-Jwt-Assertion`): RS256 against the team's JWKS, strict issuer/audience/expiry, fail
closed — configure it with `CDNO_ACCESS_TEAM_URL` and `CDNO_ACCESS_AUD`. If the JWKS cannot be
fetched at startup, the server refuses to start rather than serve unauthenticated.

Without that configuration, `cdno-mcp-server` **refuses to bind anything but loopback**.
Be precise about what that guarantees: the process only accepts connections arriving on its own
loopback interface. It cannot detect a tunnel or SSH forward that bridges the port outward —
**never bridge this port without the authenticating proxy in front.** The server logs a warning
at startup to the same effect whenever it serves real vault data unauthenticated. Configuring
the JWT validation is exactly what lifts the non-loopback restriction (e.g. binding `0.0.0.0`
inside a container).

## Flags

| Flag | Env | Default | Purpose |
|------|-----|---------|---------|
| `--vault <path>` | `CUADERNO_VAULT_PATH` | cwd | Vault root |
| `--bind <addr>` | `CDNO_MCP_BIND` | `127.0.0.1:8787` | Listen address (non-loopback refused until #302) |
| `--allowed-host <host>` | `CDNO_MCP_ALLOWED_HOSTS` (comma-separated) | — | Extra `Host` header values to accept on top of the loopback defaults (DNS-rebinding protection). A public deployment adds its hostname |
| `--smoke` | — | off | Serve a single `echo` tool holding **no vault handle** — prove tunnel/auth infrastructure end-to-end with zero vault exposure |
| `--read-only` | — | off | Advertise only the context-gathering read tools; mutating tools are absent from the dispatch table entirely |
| `--reconcile-interval-secs <n>` | `CDNO_MCP_RECONCILE_INTERVAL_SECS` | `300` | Periodic index reconciliation; `0` disables |
| `--git-checkpoint-interval-secs <n>` | `CDNO_MCP_GIT_CHECKPOINT_INTERVAL_SECS` | `60` | Commit-if-dirty git sweep of the vault — makes every remote write diffable and revertible. `0` disables; warns and no-ops when the vault isn't a git repo |
| `--access-team-url <url>` | `CDNO_ACCESS_TEAM_URL` | — | Cloudflare Access team URL (JWT issuer + JWKS host). Requires `--access-aud`; activates origin JWT validation and lifts the loopback-only restriction |
| `--access-aud <tag>` | `CDNO_ACCESS_AUD` | — | The Access application's AUD tag (expected `aud` claim). Requires `--access-team-url` |

## Index freshness

Unlike a stdio session, this process is long-running while other writers — the CLI, editors, sync
tools — mutate the Markdown underneath it. Markdown is the source of truth and the index is a
cache, so the server re-runs the reconciliation pass on the configured interval as the correctness
backstop. Out-of-band edits become visible to `search_notes` and the context tools within one
interval at most.

## Timezone — set `TZ` on the host

The server timestamps everything it writes — log lines, daily entries, tracking entries — with the
process's **local** time (`chrono::Local::now()`). A container or host with no zoneinfo database and
no `TZ` set makes chrono fall back silently to **UTC**, so remote writes land hours behind the wall
clock with no error. Any deployment must therefore ship a zoneinfo DB (`tzdata` on Alpine, already
present on most distros) and set `TZ`, e.g. `TZ=Europe/Stockholm`.

At startup the server logs the offset it resolved, next to the "vault opened" line:

```
INFO local time zone resolved (server timestamps use process-local time) local_offset=+02:00 sample_local_now=2026-07-06T14:30:00+02:00
```

Check this line after deploying: a `local_offset=+00:00` you didn't intend is the tell-tale of a
missing `tzdata`/`TZ`. (A host legitimately in UTC is fine — the line is factual, not a warning.)

## Transport details

- Endpoint: `POST /mcp`. Clients must send `Accept: application/json, text/event-stream`
  (the Streamable HTTP spec requires both).
- Stateless JSON mode: every request is self-contained; responses are plain `application/json`
  (no SSE streams, no session ids). `GET`/`DELETE` on `/mcp` return `405`.
- Guardrails: request bodies are capped at 1 MiB and in-flight requests are bounded; the `Host`
  header is validated against the allowlist (403 otherwise).
