# Epic — MCP server wrapping the queries

**Depends on:** [E-sessions-endpoint.md](E-sessions-endpoint.md).

**Estimate:** ~2 days.

## What

An MCP server (`contextnest mcp serve` subcommand) that exposes
ContextNest's query surface as MCP tools any MCP-speaking agent can
call natively — no Bash + curl shim required.

Tool surface:

| MCP tool | Backed by | What the agent gets |
|---|---|---|
| `cn_sessions_list` | `GET /api/v1/sessions` | List recent sessions for a project |
| `cn_session_get` | `GET /api/v1/sessions/:id` | Detail of one session |
| `cn_attention` | `GET /api/v1/sessions/attention` | What needs the user right now |
| `cn_find_session` | `POST /api/v1/sessions/find` | NL query for past sessions |
| `cn_learnings` | `GET /api/v1/tools/retrieve` w/ `metadata_filter: {kind: "learning"}` | Past hard-won facts |
| `cn_incomplete_todos` | `GET /api/v1/tools/retrieve` w/ `metadata_filter: {kind: "todo", task_status: "pending"}` | Open work across sessions |
| `cn_resonate` | `POST /api/v1/tools/resonate` | Emergent patterns |
| `cn_store` | `POST /api/v1/tools/store` | Manual memory writes from the agent |

## Why

Claude Code (and Cursor / Aider / Continue / Zed) natively call MCP
tools — adding `contextnest mcp serve` to `~/.claude/settings.json`'s
`mcpServers` block lets the agent ask "what's open in this project?"
without the user prompting it. Auto-injected context at SessionStart.

## Files touched

| File | Change |
|---|---|
| `src/mcp/mod.rs` | New module — MCP server using `rmcp` or `mcp-sdk-rs` |
| `src/mcp/tools.rs` | Tool definitions + handlers calling into the HTTP layer |
| `src/cli/mod.rs` + `src/bin/contextnest.rs` | `mcp` subcommand variant + `serve` |
| `Cargo.toml` | Add MCP SDK dep |
| `docs/ingest/mcp-server.md` | Installation + per-tool examples |

## Implementation sketch

The MCP server runs as a stdio subprocess (the standard MCP transport)
invoked by Claude Code. Each tool handler is ~10 lines: parse params,
call the corresponding HTTP endpoint via `reqwest`, transform the
response into the MCP `Content` shape.

Or — if the MCP server is co-located with the substrate — call the
substrate's services directly via the in-process `ContextNestServices`
container, skipping HTTP. This is faster but requires the MCP and
substrate to run in one process.

Configuration in `~/.claude/settings.json`:

```json
{
  "mcpServers": {
    "contextnest": {
      "command": "contextnest",
      "args": ["mcp", "serve"],
      "env": { "CONTEXTNEST_URL": "http://localhost:8080" }
    }
  }
}
```

## Success criteria

- `contextnest mcp serve` advertises 8 tools via the MCP `tools/list`
  method.
- Claude Code can call `cn_attention` natively (visible in the agent's
  reasoning logs) without the user typing `curl`.
- Latency per tool call <200ms p95 (one HTTP roundtrip to localhost).
- The MCP server survives substrate restart — it reconnects on the
  next tool call.

## What's NOT in scope

- Server-Sent Events / streaming for long-running tools. MCP doesn't
  need them for these queries.
- Auth between MCP server and substrate. Both on localhost; expose
  via reverse-proxy auth if you deploy remotely.
- A separate JS/Python MCP server. The Rust one is the canonical;
  port to other languages only if the ecosystem demands it.
