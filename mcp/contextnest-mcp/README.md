# ContextNest MCP server

A stdio [Model Context Protocol](https://modelcontextprotocol.io) server that
exposes the ContextNest agent-memory substrate as typed tools, so an agent can
recall what past sessions decided, tried, failed at, verified, or shipped —
without hand-writing `curl` against the HTTP API.

```
agent ──stdio JSON-RPC──▶ contextnest-mcp ──HTTP──▶ ContextNest (127.0.0.1:28080)
```

## Why this exists (instead of curl recipes or a skill file)

The HTTP API is complete and documented. The trouble is the *calling* of it,
in three specific ways, each of which has already produced a false
"ContextNest is down" report:

| Failure mode | What actually happens | What this server does |
|---|---|---|
| `curl` with no/low `--max-time` on an unscoped `retrieve` | The scan covers every fragment: **9.3 s warm, 89 s cold** at 534k fragments / 12.4M edges. curl prints `000` and the caller concludes the service is dead. | Per-call-shape timeout: **120 s** unscoped, **30 s** scoped. A timeout is reported as a timeout. |
| Probing the wrong port | The substrate answers on **28080** only. `7048` is the z-dashboard daemon, `7130`/`17130` are InsForge, `5057` is the dashboard UI. | Base URL is fixed at 28080 by default; **every** transport error names the URL it tried and lists those ports. |
| "Falling back to local memory" | The caller never queried memory at all, and later reports "nothing in memory" as if memory were empty. | No silent fallback. A failure is an error the agent sees, with the URL, wall time, and status. |

A skill file remains useful as human-readable documentation and as the
fallback when the MCP server isn't running. This server is the reliable path
when it is.

## Install

```bash
cd mcp/contextnest-mcp
npm install          # one dependency tree: @modelcontextprotocol/sdk + zod
npm run smoke        # drives real JSON-RPC frames against the live substrate
```

Requirements: Node ≥ 20 (uses global `fetch` and `AbortSignal.timeout`) and a
running ContextNest (`make cn-serve`).

## Register it

Add to an MCP client config (`~/.claude.json` global, or a project `.mcp.json`):

```json
{
  "mcpServers": {
    "contextnest": {
      "type": "stdio",
      "command": "bash",
      "args": ["-lc", "exec npx tsx /absolute/path/to/ContextNest/mcp/contextnest-mcp/src/index.ts"],
      "env": { "CONTEXTNEST_BASE_URL": "http://127.0.0.1:28080" }
    }
  }
}
```

Use an absolute path — MCP servers start in an unspecified working directory.

**Do not route the command through a login shell** (`bash -l`, `zsh -l`). A
login shell writes terminal control sequences to **stdout** — verified:
`ESC[?2004l` (bracketed-paste mode) preceded the first JSON-RPC frame and made
the handshake unparseable for any client. If you need shell logic, use
`bash -c` (no `-l`) and resolve `tsx` inside the script. A non-JSON byte on
stdout is a hard protocol failure, not a cosmetic wart.

## Tools

| Tool | Answers |
|---|---|
| `cn_health` | Is the substrate up, and how populated? Call this first when anything else errors. |
| `cn_retrieve` | Semantic recall of atoms (decisions, failures, verifications, features). Scoped by session when known. |
| `cn_prompt_context_capsule` | Priority-ordered topic briefing (Risks → Decisions → Failures → …). Literal substring filter, not semantic. |
| `cn_session_find` | Free-text search over past sessions ("which session worked on X"). |
| `cn_session_by_file` | Which sessions touched a file path. |
| `cn_session_by_feature` | Which sessions shipped a named feature/route. |
| `cn_session_by_intent` | Sessions grouped by the user-intent they served. |
| `cn_session_summary` | Hydrate a session id into its summary. |
| `cn_session_trajectory` | Ordered atom timeline for a session (phases + records). |
| `cn_inbox` | Attention queue: pending todos and explicit user-actions. |
| `cn_attention` | Sessions flagged as blocked on the user. |
| `cn_features` | What shipped in a window, each with a replayable `how_to_test`. |
| `cn_outcome` | Write-back: reinforce or dock the atoms you actually used. |

## Configuration

| Env | Default | Effect |
|---|---|---|
| `CONTEXTNEST_BASE_URL` | `http://127.0.0.1:28080` | Substrate base URL. |
| `CONTEXTNEST_MCP_SCAN_TIMEOUT_MS` | `120000` | Timeout for the unscoped `retrieve` scan. |
| `CONTEXTNEST_MCP_TIMEOUT_MS` | `30000` | Timeout for every other call. |
| `CONTEXTNEST_MCP_CONTENT_CHARS` | `320` | Per-fragment text clip in results. |
| `CONTEXTNEST_MCP_MAX_ROWS` | `25` | Rows rendered per list (raw totals are still reported). |

## Notes for reviewers

- **Payload discipline.** The raw endpoints are big: `/inbox` answered 231 KB,
  `/features?since=24h` 50 KB, `/prompt-context/atoms` 104 KB. Every list tool
  clips rows and reports the untruncated total, so a recall never floods the
  agent's context window.
- **Unknown shapes degrade, they don't break.** Endpoints whose response shape
  isn't pinned by this client (`attention`, `by-feature`, `by-intent`) render
  through `renderGeneric`, which finds the first array property and prints
  clipped `key: value` rows. An upstream shape change yields "ugly but
  readable" rather than an empty result.
- **`outcome` reports `updated`, not `delta_applied`.** `delta_applied` is the
  per-atom magnitude and is returned even when every id was stale, so it is not
  a success indicator.

## Testing

```bash
npm run smoke        # end-to-end: initialize → tools/list → tools/call (live)
npm run typecheck    # tsc --noEmit
```

`smoke.sh` fails if any expected tool is missing, if any frame is unanswered or
non-JSON, or if `cn_health` doesn't report the substrate up.
