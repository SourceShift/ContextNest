# MCP server (`contextnest mcp serve`)

ContextNest ships a [Model Context Protocol](https://modelcontextprotocol.io)
server so any MCP-speaking agent — Claude Code, Cursor, Zed, Continue,
Aider — can call the memory substrate **natively**, instead of shelling
out to `curl` against the HTTP API.

> **Phase 1 scope.** Three core memory tools (`cn_store`, `cn_retrieve`,
> `cn_summarize`) over the stdio transport. The session / trajectory /
> inbox tools (`cn_sessions_list`, `cn_attention`, `cn_find_session`,
> `cn_learnings`, `cn_incomplete_todos`, `cn_resonate`) land in later
> phases — see `docs/roadmap/epics/cc-ingest/E-mcp-server.md`.

## How it works

```
┌──────────────┐  spawn (stdio)   ┌─────────────────────┐  HTTP   ┌────────────┐
│  Host agent  │ ───────────────▶ │ contextnest mcp serve│ ──────▶ │  substrate │
│ (Claude Code)│ ◀─ JSON-RPC 2.0 ─│  (this subprocess)   │ ◀───── │  :28080    │
└──────────────┘   over stdin/out └─────────────────────┘  json   └────────────┘
```

The host agent launches `contextnest mcp serve` as a subprocess and speaks
newline-delimited JSON-RPC 2.0 over stdin/stdout. Each tool call is one
HTTP round-trip to the substrate's `/api/v1/tools/*` endpoint. The MCP
process is stateless — it reconnects on the next call if the substrate
restarts.

> **stdout is the protocol channel.** In `mcp serve` mode all logs are
> redirected to **stderr** so they never corrupt the JSON-RPC stream. If
> you wrap the binary, preserve that separation.

## Install

Add to `~/.claude/settings.json` (or your agent's MCP config):

```json
{
  "mcpServers": {
    "contextnest": {
      "command": "contextnest",
      "args": ["mcp", "serve"],
      "env": { "CONTEXTNEST_URL": "http://localhost:28080" }
    }
  }
}
```

The substrate URL resolves in this order: `--url <addr>` flag →
`$CONTEXTNEST_URL` → `http://localhost:8080` (the bare-`serve` default;
note `make cn-serve` binds **28080**, so set `CONTEXTNEST_URL` accordingly).

## Tools

| Tool | Backed by | Required args | Optional args |
|---|---|---|---|
| `cn_store` | `POST /api/v1/tools/store` | `content` | `importance`, `session_id`, `metadata` |
| `cn_retrieve` | `POST /api/v1/tools/retrieve` | `query` | `top_k` (default 5), `session_id`, `metadata_filter` |
| `cn_summarize` | `POST /api/v1/tools/summarize` | `session_id` | `target_tokens` |

The substrate's JSON response is returned verbatim (pretty-printed) as the
tool's text content.

### Error semantics

- **Bad/missing arguments** (e.g. `cn_store` without `content`) → JSON-RPC
  protocol error `-32602 INVALID_PARAMS`. The call never reaches the
  substrate.
- **Substrate unreachable or non-2xx** → a *successful* `tools/call`
  result with `isError: true` and the failure text inside `content`, per
  the MCP convention that execution failures stay in the result so the
  model can read and react to them.

## Smoke test (no agent required)

Pipe raw JSON-RPC straight into the subprocess:

```bash
printf '%s\n' \
  '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{}}}' \
  '{"jsonrpc":"2.0","id":2,"method":"tools/list"}' \
  '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"cn_store","arguments":{"content":"hello from MCP","session_id":"smoke"}}}' \
  '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"cn_retrieve","arguments":{"query":"hello","session_id":"smoke"}}}' \
  | contextnest mcp serve --url http://127.0.0.1:28080
```

`initialize` echoes back the protocol version you requested (lenient
negotiation), `tools/list` advertises the three tools, and the two
`tools/call` lines do a real store + retrieve round-trip.

## Design note — why hand-rolled, not `rmcp`

Phase 1 needs exactly four JSON-RPC methods (`initialize`, `tools/list`,
`tools/call`, plus the `notifications/initialized` notification). That's a
few dozen lines on top of `serde_json` + `reqwest`, both already in the
dependency graph — so the server adds **no new dependency** to the single
binary. We would only adopt the official `rmcp` crate if a later phase
needs *server-initiated* requests (sampling / elicitation), which none of
the query tools require.

## Not in scope (Phase 1)

- Streaming / SSE for long-running tools — the query tools don't need it.
- Auth between the MCP process and the substrate — both run on localhost;
  put a reverse proxy in front if you expose the substrate remotely.
- A separate JS/Python MCP server — the Rust one is canonical.
