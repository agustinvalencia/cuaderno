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
| `--git-checkpoint-interval-secs <n>` | `CDNO_MCP_GIT_CHECKPOINT_INTERVAL_SECS` | `60` | How often the git sweep runs. `0` disables it; warns and no-ops when the vault isn't a git repo |
| `--git-checkpoint-mode <mode>` | `CDNO_MCP_GIT_CHECKPOINT_MODE` | `commit` | What the sweep does with a dirty tree: `commit` here, or `nudge-only` — see [The recovery trail](#the-recovery-trail) |
| `--sync-nudge` | `CDNO_MCP_SYNC_NUDGE` | off | Touch a sentinel file after every verified write so an external sync agent reacts at once instead of on its own timer — see [Pairing with a sync agent](#pairing-with-a-sync-agent) |
| `--sync-nudge-path <path>` | `CDNO_MCP_SYNC_NUDGE_PATH` | `<vault>/.git/cdno-sync.nudge` | Where that sentinel lives. Setting it does not by itself enable nudging |
| `--access-team-url <url>` | `CDNO_ACCESS_TEAM_URL` | — | Cloudflare Access team URL (JWT issuer + JWKS host). Requires `--access-aud`; activates origin JWT validation and lifts the loopback-only restriction |
| `--access-aud <tag>` | `CDNO_ACCESS_AUD` | — | The Access application's AUD tag (expected `aud` claim). Requires `--access-team-url` |

## The recovery trail

Exposing write tools remotely means anything a confused or prompt-injected session does lands in
your vault. The damage limit is that **every mutation ends up in a git commit** you can diff and
revert. The sweep is what provides it: on an interval it takes the vault write lock, and if the tree
is dirty it commits everything as `cdno-mcp checkpoint (N path(s))`. It is a sweep rather than a
per-write hook, so out-of-band edits — the CLI, your editor, a sync tool — join the trail too.

What must hold is that *something* commits. Which actor does is configurable:

| Mode | Who commits | How |
|------|-------------|-----|
| **interval** (default) | this server | `--git-checkpoint-interval-secs <n>`, default `60` |
| **nudge-only** | an external sync agent | `--git-checkpoint-mode nudge-only` |
| **disabled** | nobody | `--git-checkpoint-interval-secs 0` |

Reach for **nudge-only** when an agent already owns the repository's history. Per-minute checkpoint
commits would fight it: two git actors in one working tree, and the agent's coalesced,
unit-of-thought commits buried under machine noise. In this mode the sweep still runs and still
takes the lock, but it commits nothing — on a dirty tree it touches the
[sync-nudge sentinel](#pairing-with-a-sync-agent) and leaves the change exactly where it found it,
unstaged, for the agent to pick up.

```bash
cdno-mcp-server --vault /srv/vault --git-checkpoint-mode nudge-only
```

The sentinel path is the one `--sync-nudge-path` sets, so the sweep and the per-write nudge always
agree on it. You do not need `--sync-nudge` as well: that flag governs the *per-write* signal, and
the two are useful together (writes nudge immediately; the sweep catches anything that arrived out
of band) but independent.

**Both non-default modes hand the trail to somebody else, and the server says so at startup** —
nudge-only logs a warning naming the sentinel and stating that this process commits nothing, and
`0` warns that nothing in the process commits at all. If no agent is running, either is equivalent
to having no recovery trail.

Only `cdno-mcp-server` sweeps. The stdio binary has no checkpoint loop and none of these flags.

### When the sweep stops

The sweep's job is to be the thing you can fall back on, so it must never fail quietly. Two ways it
can stop, both loud:

- **A tick that runs and finds trouble** — a non-zero `git`, the vault write lock busy, someone
  else's paused merge or rebase — logs and retries on the next tick. Only a `git` that cannot be
  executed at all, five times running, disables the loop, with an error saying so.
- **A tick that never runs at all.** The sweep does its work on a pool of worker threads, and if the
  process cannot get one, the tick is queued and the loop waits. Nothing is committed and, until
  this was fixed, nothing was logged either — the endpoint stayed up and tool calls kept answering,
  so nothing outside could tell. A tick that overruns three sweep intervals (never less than 30
  seconds) now logs:

  ```
  ERROR git checkpoint sweep STALLED: a tick has not completed, no further tick can start, and so
  NOTHING in this process is committing — writes are no longer being recorded. ...
  ```

  It repeats while the tick stays stuck, and logs a recovery line if it completes. Alert on it: the
  two realistic causes are a process out of threads (see
  [Thread budget](#thread-budget-and-pids_limit)) and a wedged `git` invocation, and the process
  cannot tell them apart from inside — but either way the vault has stopped being recorded.

## Thread budget and `pids_limit`

A container sized with `pids_limit` counts every OS thread this process holds, not just processes.
The server bounds itself so that number is knowable, and logs it at startup:

```
INFO runtime thread budget workers=8 max_blocking_threads=16 max_os_threads=25
```

`max_os_threads` is the ceiling: one main thread, one async worker per CPU the process can see, and
a fixed pool of 16 for the blocking work (vault reads and writes, the reconciliation pass, the
checkpoint sweep). Size `pids_limit` above that number with room for whatever else shares the
container — a healthcheck that shells out needs to fork too, and a container that cannot fork
reports `unhealthy` and refuses `docker exec` while the server itself carries on serving.

Note that `workers` follows the CPUs the process can *see*, which on Linux is CPU affinity, not a
cgroup CPU quota: a small container on a large host still gets a worker per host core. Read the
number off the log line rather than assuming it.


## Pairing with a sync agent

A common shape is two clones of the vault repository — an always-on host running this server, and a
laptop — with an external agent on the host owning the commit-and-push loop. That agent normally
polls: it wakes on its own timer, sees a dirty tree, and commits. A write that landed a second after
the last poll waits out the whole interval.

`--sync-nudge` closes that gap. After every write the server has [verified](writes.md#every-write-is-verified),
it rewrites a sentinel file, changing both its modification time and its contents. The agent watches
that one path — launchd `WatchPaths`, `inotifywait`, `fswatch`, whatever it already uses — and acts
immediately.

```bash
cdno-mcp-server --vault /srv/vault --sync-nudge
# → touches /srv/vault/.git/cdno-sync.nudge after each verified write
```

The contract is deliberately narrow:

- **Off by default.** A deployment with no agent has nobody to signal.
- **One-way.** The server writes the sentinel and never reads it, so an agent that is absent,
  stopped, or slow costs nothing but latency.
- **Only after a verified write.** A write that failed, or that could not be read back, leaves the
  sentinel alone — an agent woken by writes that did not happen learns to ignore the signal.
- **Never fatal.** A sentinel that cannot be written is logged and skipped; the write still succeeds.
- **Never content.** The file holds a unix timestamp and nothing else. It names no note.

It lives under `.git/` on purpose: git will not track it, and no tool that mirrors the working tree
will carry it, so the signal cannot leak into the vault or across machines. `--sync-nudge-path`
moves it if your agent needs it elsewhere; parent directories are never created.

Only `cdno-mcp-server` has this. The stdio binary is a local session with no agent on the other side
of it, and offers no such flag.

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
