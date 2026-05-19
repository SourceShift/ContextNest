# ContextNest Architecture

How the substrate works under the seven-tool API — written for a reader who
has used the API once or twice and wants to understand the moving parts before
extending them.

## 1. Mental model

Treat memory the way the brain treats it, not the way a database does:

- **Fragments** are the atoms — short pieces of text agents care about.
- **Attractors** are crystallised fragments — when a fragment is stored, the
  substrate adapts the surrounding "field" so that related cues pull toward it.
- **Basins** are clusters of mutually-reinforcing attractors. When you store
  five fragments about the same bug, they end up in one basin and retrieval
  hits the basin, not five separate items.
- **Connection network** is the graph between basins. Activation in one basin
  raises activation in connected basins (the `resonate` mechanism).
- **Adaptive decay** keeps the field healthy: unused attractors weaken over
  time so the hot, recently-useful ones dominate retrieval.
- **Reconstruction** is gap-filling — a partial cue activates whatever it can,
  and the surrounding attractors fill in the rest into a coherent answer.

The key distinction from a vector database is that ContextNest doesn't just
return the closest item to a query; it lets the field do work — activation
spreads, neighbours contribute, and degraded retrieval produces useful output
instead of "no match".

## 2. System diagram

```mermaid
flowchart TB
    Client["LLM agent / HTTP client"]

    subgraph API["src/api"]
        direction TB
        Tools["tools.rs<br/>store · retrieve · update · summarize<br/>discard · reconstruct · resonate"]
        Middleware["middleware/<br/>CORS · request-context · validation<br/>compression · metrics · error-intercept"]
    end

    subgraph Memory["src/memory/attractors"]
        direction TB
        MAM["MemoryAttractorManager<br/>process_memories entry point"]
        Basin["AttractorBasin<br/>basin formation"]
        ConnNet["ConnectionNetwork<br/>activation graph"]
        Decay["AdaptiveDecay<br/>importance over time"]
        GapFill["GapFillingEngine<br/>reconstruction-store"]
        ReconProto["ReconstructionProtocol<br/>retrieve→assemble"]
    end

    subgraph Services["src/services"]
        direction TB
        Session["SessionIndex<br/>session_id → fragment_id"]
        Graph["Neo4j graph (optional)<br/>cross-fragment edges"]
        Embed["EmbeddingService<br/>provider abstraction"]
        LLM["LlmService<br/>Anthropic · OpenAI · Google"]
    end

    Storage[("Fragment store + Vec&lt;f32&gt; embeddings")]

    Client -->|"POST /api/v1/tools/&lt;name&gt;"| Middleware
    Middleware --> Tools
    Tools --> MAM
    Tools --> Session
    Tools -.->|"summarize"| LLM
    Tools -.->|"optional"| Graph
    MAM --> Basin
    MAM --> ConnNet
    MAM --> Decay
    MAM --> GapFill
    MAM --> ReconProto
    Basin --> Storage
    ConnNet --> Storage
    Embed -.->|"text → Vec&lt;f32&gt;"| MAM

    classDef api fill:#1f4e79,stroke:#2e75b6,color:#fff
    classDef mem fill:#2e7d32,stroke:#4caf50,color:#fff
    classDef svc fill:#7b1fa2,stroke:#ab47bc,color:#fff
    classDef store fill:#5d4037,stroke:#8d6e63,color:#fff
    class Tools,Middleware api
    class MAM,Basin,ConnNet,Decay,GapFill,ReconProto mem
    class Session,Graph,Embed,LLM svc
    class Storage store
```

## 3. The store path

When an agent calls `POST /api/v1/tools/store`, this is what happens end to
end. `process_memories` is the single entry point — every store goes through
basin formation, network indexing, and reconstruction-store population in one
pass before the call returns.

```mermaid
sequenceDiagram
    autonumber
    participant Client as Agent
    participant API as tools.rs
    participant Session as SessionIndex
    participant Embed as EmbeddingService
    participant MAM as MemoryAttractorManager
    participant Basin as AttractorBasin
    participant Net as ConnectionNetwork
    participant Recon as ReconstructionStore

    Client->>API: store {content, importance, session_id}
    API->>Embed: embed(content)
    Embed-->>API: 768-d embedding
    API->>Session: register(session_id, fragment_id)
    Session-->>API: ok
    API->>MAM: process_memories(fragment, embedding)
    MAM->>Basin: form_or_attach(fragment)
    Note over Basin: Creates a new basin when no neighbour is within threshold, otherwise attaches to and reinforces the existing one
    Basin-->>MAM: basin_id
    MAM->>Net: index_edges(basin_id, neighbours)
    Net-->>MAM: edge_count
    MAM->>Recon: populate(fragment, basin_id)
    Recon-->>MAM: ok
    MAM-->>API: {fragment_id, basin_id, edges_added}
    API-->>Client: 200 {fragment_id, attractor_basin, ...}
```

The interesting property: storage is **idempotent on near-duplicates**. Two
fragments about the same bug land in one basin and reinforce it instead of
fragmenting the field.

## 4. The retrieve + reconstruct path

`retrieve` is similarity search with basin awareness. `reconstruct` adds
gap-filling — useful when the agent's cue is incomplete.

```mermaid
sequenceDiagram
    autonumber
    participant Client as Agent
    participant API as tools.rs
    participant Session as SessionIndex
    participant Embed as EmbeddingService
    participant MAM as MemoryAttractorManager
    participant Net as ConnectionNetwork
    participant GapFill as GapFillingEngine

    Client->>API: retrieve {query, top_k, session_id}
    API->>Embed: embed(query)
    Embed-->>API: embedding
    API->>Session: scope(session_id)
    Session-->>API: fragment_ids
    API->>MAM: retrieve(embedding, top_k, scope)
    MAM->>Net: activate_basins(embedding)
    Net-->>MAM: ranked_basins
    MAM-->>API: top_k attractors
    API-->>Client: 200 {attractors[]}

    Note over Client: Agent realises the cue was partial. Asks for reconstruction.

    Client->>API: reconstruct {partial_cue, session_id}
    API->>MAM: reconstruct(cue, scope)
    MAM->>Net: activate_basins(cue_embedding)
    Net-->>MAM: weak_activation (degraded)
    MAM->>GapFill: fill(weak_activation, neighbours)
    GapFill->>Net: pull_in_neighbour_attractors
    Net-->>GapFill: neighbour_attractors
    GapFill-->>MAM: reconstructed_fragments
    MAM-->>API: assembled answer
    API-->>Client: 200 {reconstructed, confidence}
```

The trick on `reconstruct`: even when initial activation is weak, the
network's edges pull in adjacent attractors that the original cue didn't
match directly. The result is an answer assembled from fragments — not a
miss.

## 5. The resonate path

`resonate` is the differentiator vs flat vector stores. It looks for
emergent activation patterns: signal that doesn't come from a single
attractor, but from the geometry of how multiple attractors light up.

```mermaid
sequenceDiagram
    autonumber
    participant Client as Agent
    participant API as tools.rs
    participant MAM as MemoryAttractorManager
    participant Net as ConnectionNetwork
    participant Phase as PhaseSync

    Client->>API: resonate {query, session_id}
    API->>MAM: resonate(query)
    MAM->>Net: activate_basins(query_embedding)
    Net-->>MAM: activation_pattern[]
    MAM->>Phase: detect_coherence(activation_pattern)
    Phase-->>MAM: coherent_groups[]
    Note over Phase: Group basins that activated together more strongly than their individual scores predict
    MAM-->>API: emergent_patterns
    API-->>Client: 200 {patterns[], strength}
```

Concrete example: agent stores debugging fragments about three separate
issues over six weeks. Each individual fragment is unremarkable. But when
the agent later asks about a new error and `resonate` activates the field,
it surfaces "these three past issues share a common root cause you didn't
notice" — because they form a coherent activated group, not because any one
of them matched the query directly.

## 6. Session affinity

The substrate itself is global — basins and connection edges accumulate
across all sessions. The `SessionIndex` is a thin per-session overlay that
keeps `session_id → fragment_id` routing tables.

```mermaid
flowchart LR
    Sess1["Session A"]
    Sess2["Session B"]
    Sess3["Session C"]

    Idx["SessionIndex<br/>session_id → fragment_id maps"]

    subgraph Global["Global substrate (no sessions)"]
        Frags["Fragments + embeddings"]
        Basins["AttractorBasins"]
        Edges["ConnectionNetwork edges"]
    end

    Sess1 --> Idx
    Sess2 --> Idx
    Sess3 --> Idx
    Idx --> Frags
    Idx --> Basins
    Idx --> Edges

    classDef sess fill:#ef6c00,stroke:#f57c00,color:#fff
    classDef idx fill:#7b1fa2,stroke:#ab47bc,color:#fff
    classDef glob fill:#2e7d32,stroke:#4caf50,color:#fff
    class Sess1,Sess2,Sess3 sess
    class Idx idx
    class Frags,Basins,Edges glob
```

This means `retrieve` for session A doesn't return session B's fragments
(the index filters), but the basin geometry is shared. If two sessions
happen to store fragments that belong in the same basin, the basin
strengthens — even though neither session can see across the boundary.

The contract: a single mutating operation always writes to all three
`SessionIndex` maps (active / deleted / reverse) inside one `write()`
guard. Readers acquire only the map they need, so read concurrency stays
high. See `src/services/session_index.rs` for the consistency contract.

## 7. Concurrency model

| Layer | Lock | Reason |
|---|---|---|
| `SessionIndex` | `RwLock<HashMap>` × 3 | Reads dominate; writers acquire all three under one guard |
| `MemoryAttractorManager` | per-component locks (`AttractorBasin`, `ConnectionNetwork`) | Each component owns its state; manager orchestrates |
| `LlmService` | `tokio::sync::Mutex` on the underlying provider client | HTTP client is not Send-safe across awaits otherwise |
| Storage | `parking_lot::RwLock` on fragment store | Fastest non-async lock; fragment reads are the hot path |

Avoid holding the manager lock across `await` points. The `adaptive_decay`
module enforces this — its scheduler reads under a lock, drops it, then
executes the async decay operation. Pattern to follow for new code.

## 8. Why neural-field attractors vs flat vector store

The four behaviours you get for free that pgvector doesn't:

1. **Reinforcement on near-duplicates.** Same bug stored five different ways
   ends up as one strengthened basin, not five competing hits.
2. **Adaptive decay.** Stale debugging memories from six months ago don't
   dominate retrieval. Recently-useful basins stay sharp; cold ones fade.
   No cron jobs, no manual eviction policies.
3. **Reconstruction from partial cues.** Vector similarity returns nothing
   useful when the cue is degraded. The gap-filler assembles a coherent
   answer from neighbours.
4. **Emergent pattern surfacing.** `resonate` finds coherent groups —
   patterns that no single fragment carries but multiple together imply.
   This is the one that's hard to build on top of pgvector after the fact.

The cost: ContextNest is opinionated about how it stores. You can't drop
in arbitrary embeddings and expect the substrate to behave like a flat
store. Embed text fragments via the configured `EmbeddingService` and let
the manager decide basin assignment.

## 9. Where to read next

- [usage.md](usage.md) — practical how-to with curl examples per tool
- [`src/api/tools.rs`](../src/api/tools.rs) — request/response shapes
- [`src/memory/attractors/memory_attractor_manager.rs`](../src/memory/attractors/memory_attractor_manager.rs) — the orchestrator entry point
- [`src/services/session_index.rs`](../src/services/session_index.rs) — session-affinity contract
- [`src/services/llm.rs`](../src/services/llm.rs) — multi-provider LLM abstraction
- [CONTRIBUTING.md](../CONTRIBUTING.md) — canonical pipeline + adding a new tool
