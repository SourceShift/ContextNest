# Using ContextNest

A practical, copy-paste-friendly walk through every tool plus three
end-to-end integration recipes.

For the conceptual model — why basins, why decay, why reconstruction —
see [architecture.md](architecture.md) first.

## 1. Get the substrate running

```bash
git clone https://github.com/SourceShift/ContextNest.git
cd ContextNest
cp .env.example .env
cargo run -- serve
```

Default bind: `0.0.0.0:8080`. Override with `--bind 127.0.0.1:9000`.

A health probe should respond immediately:

```bash
curl -s http://localhost:8080/health
# → {"status":"healthy","ready":true,...}
```

If you want LLM-backed `summarize`, set one provider before `cargo run`:

```bash
export CONTEXTNEST_LLM_PROVIDER=anthropic
export ANTHROPIC_API_KEY=sk-ant-...
```

Unset means `summarize` falls back to a statistics-only path; every other
tool is unaffected.

## 2. The seven tools

Every tool is `POST /api/v1/tools/<name>` with a JSON body. All bodies
accept an optional `session_id` (defaults to `"default"`). Examples below
use `jq` for readability — strip it if you don't have it installed.

### 2.1 `store` — persist a fragment

```bash
curl -s -X POST http://localhost:8080/api/v1/tools/store \
  -H 'Content-Type: application/json' \
  -d '{
    "content": "The cargo audit RSA Marvin advisory has no upstream fix yet; we acknowledge it in .cargo/audit.toml.",
    "importance": 0.7,
    "session_id": "demo"
  }' | jq .
```

Returns:

```json
{
  "fragment_id": "f_4c8e...",
  "attractor_basin": "b_2a91...",
  "edges_added": 3,
  "stored_at": "2026-05-19T10:14:22Z"
}
```

`importance` is a float in `[0, 1]` and drives both the initial activation
and the decay curve. Use `0.9+` for "this is the answer to a question I
already had", `0.4-0.6` for "might be useful later", `<0.3` only when you
know it's marginal.

### 2.2 `retrieve` — fetch by semantic similarity

```bash
curl -s -X POST http://localhost:8080/api/v1/tools/retrieve \
  -H 'Content-Type: application/json' \
  -d '{
    "query": "what was the deal with the RSA audit warning?",
    "top_k": 3,
    "session_id": "demo"
  }' | jq .
```

Returns up to `top_k` attractors ranked by activation score. Each entry
includes the original content, the basin id, and the activation strength.

`top_k` defaults to 5 and caps at 50.

### 2.3 `update` — mutate a stored attractor

```bash
curl -s -X POST http://localhost:8080/api/v1/tools/update \
  -H 'Content-Type: application/json' \
  -d '{
    "fragment_id": "f_4c8e...",
    "importance": 0.95,
    "session_id": "demo"
  }' | jq .
```

Both `content` and `importance` are optional individually. Use this when
you learn that a memory you stored earlier turned out to be load-bearing.

### 2.4 `summarize` — compact a region into one attractor

```bash
curl -s -X POST http://localhost:8080/api/v1/tools/summarize \
  -H 'Content-Type: application/json' \
  -d '{
    "session_id": "demo",
    "region": "all",
    "target_size": 500
  }' | jq .
```

Region selectors: `"all"` (whole session), `"basin:<id>"` (one basin),
`"recent:<n>"` (last n fragments). `target_size` is a soft character
budget.

If `CONTEXTNEST_LLM_PROVIDER` is configured, summarization goes through
the LLM. Otherwise it returns a statistics-only compaction (token
counts, basin names, dates).

### 2.5 `discard` — remove an attractor

```bash
# Soft delete (default — recoverable, keeps basin shape)
curl -s -X POST http://localhost:8080/api/v1/tools/discard \
  -H 'Content-Type: application/json' \
  -d '{"fragment_id": "f_4c8e...", "session_id": "demo"}' | jq .

# Hard delete (irreversible, basin re-formed without it)
curl -s -X POST http://localhost:8080/api/v1/tools/discard \
  -H 'Content-Type: application/json' \
  -d '{"fragment_id": "f_4c8e...", "session_id": "demo", "hard": true}' | jq .
```

Soft-deleted fragments are excluded from `retrieve` and `resonate` but
their basin contribution stays — undoing a soft delete is a single
`store` of the same content (it re-attaches to the existing basin).

### 2.6 `reconstruct` — gap-filling from a partial cue

This is the one that's hard to build elsewhere. Use it when your agent
has a cue but not a complete query.

```bash
curl -s -X POST http://localhost:8080/api/v1/tools/reconstruct \
  -H 'Content-Type: application/json' \
  -d '{
    "partial_cue": "RSA something marvin",
    "session_id": "demo"
  }' | jq .
```

Returns a reconstructed fragment assembled from whichever attractors the
partial cue activated plus their neighbours. The response includes a
`confidence` score — `>0.7` is usually trustworthy, `<0.4` means the
network couldn't anchor the cue and the result is a guess.

### 2.7 `resonate` — surface emergent patterns

```bash
curl -s -X POST http://localhost:8080/api/v1/tools/resonate \
  -H 'Content-Type: application/json' \
  -d '{
    "query": "intermittent test failure on CI",
    "session_id": "demo"
  }' | jq .
```

Returns coherent activation groups: sets of basins that lit up together
more strongly than any one of them did individually. The output is
`{patterns: [{basins, coherence, summary}, ...]}`.

Read pattern entries as: "these N basins seem to share a common theme
relevant to your query, even though no single fragment matched directly".

## 3. End-to-end demo: a single-agent note-taking session

```bash
SESS="agent-2026-05-19"

# Capture a few related observations across an afternoon
for fact in \
  "Rust 1.80 made dyn-trait coercion stricter; old impl<T> for Box<dyn Trait> patterns break" \
  "The fix is to add `+ 'static` bound on trait objects in old crates" \
  "We hit this on serde_json 0.9 -> 1.0 upgrade; resolved by bumping all transitive deps" \
  "tower-http compression-br feature is required for our middleware stack to compile" \
  "Neo4j 5.x driver neo4rs 0.9 requires connection-string format bolt://user:pass@host"
do
  curl -s -X POST http://localhost:8080/api/v1/tools/store \
    -H 'Content-Type: application/json' \
    -d "{\"content\": \"$fact\", \"importance\": 0.6, \"session_id\": \"$SESS\"}" > /dev/null
done

# Hours later — partial recall
curl -s -X POST http://localhost:8080/api/v1/tools/reconstruct \
  -H 'Content-Type: application/json' \
  -d "{\"partial_cue\": \"trait object 1.80 broke\", \"session_id\": \"$SESS\"}" | jq .

# Look for emergent patterns
curl -s -X POST http://localhost:8080/api/v1/tools/resonate \
  -H 'Content-Type: application/json' \
  -d "{\"query\": \"upgrade compatibility issues\", \"session_id\": \"$SESS\"}" | jq .

# End of day — compact the whole session into one attractor
curl -s -X POST http://localhost:8080/api/v1/tools/summarize \
  -H 'Content-Type: application/json' \
  -d "{\"session_id\": \"$SESS\", \"region\": \"all\", \"target_size\": 600}" | jq .
```

## 4. Integration recipes

### 4.1 Use it from Python

```python
import requests

class CN:
    def __init__(self, host="http://localhost:8080", session="default"):
        self.host = host
        self.session = session

    def _post(self, tool, body):
        body["session_id"] = self.session
        return requests.post(f"{self.host}/api/v1/tools/{tool}",
                             json=body, timeout=5).json()

    def store(self, content, importance=0.5):
        return self._post("store", {"content": content,
                                    "importance": importance})

    def retrieve(self, query, top_k=5):
        return self._post("retrieve", {"query": query, "top_k": top_k})

    def reconstruct(self, partial_cue):
        return self._post("reconstruct", {"partial_cue": partial_cue})

    def resonate(self, query):
        return self._post("resonate", {"query": query})

mem = CN(session="my-agent")
mem.store("agent learned that retry budgets cap at 3 attempts here")
print(mem.retrieve("how many retries are allowed"))
```

### 4.2 Use it from a shell loop (5-line bash agent)

```bash
#!/usr/bin/env bash
# Asks for input, retrieves relevant memory, then stores the answer.
SESS="bash-agent"
while read -p "you> " line; do
  curl -s -X POST localhost:8080/api/v1/tools/retrieve \
    -H 'Content-Type: application/json' \
    -d "{\"query\": \"$line\", \"top_k\": 3, \"session_id\": \"$SESS\"}" \
  | jq -r '.attractors[]?.content' | sed 's/^/memory> /'
  curl -s -X POST localhost:8080/api/v1/tools/store \
    -H 'Content-Type: application/json' \
    -d "{\"content\": \"$line\", \"importance\": 0.5, \"session_id\": \"$SESS\"}" \
    > /dev/null
done
```

### 4.3 Give Claude Code persistent memory via hooks

ContextNest pairs naturally with Claude Code's hook system —
`SessionStart` retrieves memories from past work, `UserPromptSubmit`
injects relevant context, `PreCompact` persists about-to-be-truncated
context, `Stop` summarises the session.

Sketch in `~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [{"hooks": [{"type": "command",
      "command": "cn-hook session_start"}]}],
    "UserPromptSubmit": [{"hooks": [{"type": "command",
      "command": "cn-hook prompt"}]}],
    "PreCompact": [{"hooks": [{"type": "command",
      "command": "cn-hook pre_compact"}]}],
    "Stop": [{"hooks": [{"type": "command",
      "command": "cn-hook stop"}]}]
  }
}
```

The wrapper script reads the hook's JSON from stdin, calls the
ContextNest HTTP API with a curl, and (for `SessionStart` /
`UserPromptSubmit`) emits `{"hookSpecificOutput": {"additionalContext":
"..."}}` to inject memory into Claude's context.

Keep each hook under ~2s — Claude Code blocks on hook execution.
`retrieve` and `resonate` are cheap. Never put `summarize` on
`UserPromptSubmit`; reserve it for `Stop` where latency is invisible.

## 5. Operational notes

### LLM provider switching mid-session

```bash
# Anthropic
export CONTEXTNEST_LLM_PROVIDER=anthropic
export ANTHROPIC_API_KEY=...

# Switch to a local Ollama via base_url override (no code change)
export CONTEXTNEST_LLM_PROVIDER=openai
export OPENAI_API_KEY=ollama  # placeholder
export CONTEXTNEST_LLM_BASE_URL=http://localhost:11434/v1
export CONTEXTNEST_LLM_MODEL=llama3
```

The server reads env at boot — restart after changing it. Per-request
overrides are on the roadmap for v0.2.

### Memory pressure

Adaptive decay handles cold attractors automatically — no operator
action needed for normal workloads. If you do want to force a sweep:

```bash
# Soft-discard everything older than 30 days with importance < 0.3
# (planned for v0.2 as a `cleanup` tool; for now do it client-side)
```

### Backup and restore

In-process attractor state lives in memory; durable persistence is a
v0.2 work item. For v0.1.0, treat ContextNest as a long-running
companion process — restart loses state. The Neo4j graph service
(optional) persists fragment edges but not basin geometry.

For production: deploy behind a supervisor that restarts on crash,
checkpoint relevant attractors via periodic `summarize` calls into your
own durable store (e.g., dump the summarize output to S3 hourly).

### Authentication

ContextNest ships without built-in auth. Deploy behind a reverse proxy
that enforces TLS + auth. See the README's "Authentication & deployment"
section for the canonical nginx snippet.

## 6. Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| `retrieve` returns `[]` for queries that should hit | `session_id` mismatch — fragment stored under `default`, retrieved under `agent-1` | Pass the same `session_id` to both calls |
| `summarize` returns statistics, not prose | No `CONTEXTNEST_LLM_PROVIDER` set | Export provider + api key, restart server |
| `reconstruct` returns confidence < 0.3 consistently | Too few fragments stored — basin geometry is sparse | Either store more fragments or fall back to `retrieve` |
| Slow `resonate` calls (>500ms) | Large session — coherence detection scales with basin count | Run `summarize` periodically to compact |
| `cargo run -- serve` panics on Neo4j connection | Optional graph service is enabled but no Neo4j running | Either start Neo4j (`docker-compose up neo4j`) or disable the graph feature in your config |
| `Authorization: Bearer ...` header is set but `user_id` stays None | v0.1.0 does not parse JWTs — the auth shim is intentional. JWT/OIDC arrives in v0.5+ | Use a reverse proxy that enforces auth and trust ContextNest behind it |

## 7. Where to go next

- [architecture.md](architecture.md) — the substrate's mental model + sequence diagrams
- [`tests/canonical_memory_chain.rs`](../tests/canonical_memory_chain.rs) — end-to-end store→retrieve→reconstruct integration test, useful as a worked example
- [`tests/seven_tools_api.rs`](../tests/seven_tools_api.rs) — the full HTTP surface exercised
- [CONTRIBUTING.md](../CONTRIBUTING.md) — how to extend the tool set
