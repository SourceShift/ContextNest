# Epic — Embedding-based goal_phase clustering

**Depends on:** MVP foundation PR.

**Estimate:** ~1 day.

## What

Replace the phase-1 60% token-overlap clustering in the ingester's
extractor with embedding cosine similarity > 0.85 via the substrate's
`EmbeddingService`.

## Why

Token overlap merges goals that share lots of stop words and splits
goals that are semantically the same but worded differently. Example
from a real session:

| Goal text | Token overlap with previous | Should merge? |
|---|---|---|
| "Audit ContextNest's Rust files for unfinished implementations" | (baseline) | — |
| "Surface all incomplete/dummy/patchy code in the substrate" | 12% (very few shared tokens) | YES (same intent, just paraphrased) |
| "Audit the substrate for half-finished bits" | 50% (Audit + substrate) | YES |
| "Run the test suite" | 0% (pure pivot) | NO |

Token overlap gets the third case right but the second case wrong.
Embedding similarity gets both right because semantic similarity is
~0.91 between the first two even though their tokens barely overlap.

## Files touched

| File | Change |
|---|---|
| `src/ingest/claude_code/extractor.rs` | Replace `tokens_overlap_pct` with `embedding_cosine_sim`; add fallback to token-overlap when EmbeddingService is unavailable (degraded mode) |
| `tests/extractor_clustering_test.rs` | Add cases where token-overlap fails but embedding-clustering succeeds |
| `docs/roadmap/v0.2-claude-code-ingest.md` | Update the "Phase 2" mention to "shipped" |

## Implementation sketch

```rust
async fn cluster_goals(
    goals: Vec<(String, String)>,  // (timestamp, text)
    embedder: Option<&EmbeddingService>,
) -> Vec<GoalPhase> {
    let Some(embedder) = embedder else {
        return cluster_goals_token_overlap(goals);  // existing path
    };

    let mut phases: Vec<GoalPhase> = Vec::new();
    for (ts, text) in goals {
        let embedding = embedder.generate_embedding(&text).await?;
        match phases.last_mut() {
            Some(last) if cosine(&last.embedding, &embedding) > 0.85 => {
                last.extend(ts, text, embedding);
            }
            _ => phases.push(GoalPhase::new(ts, text, embedding)),
        }
    }
    phases
}
```

Cost per session: ~30 embeddings × ~$0.00002 each = $0.0006 per
session at OpenAI prices. Negligible.

## Success criteria

- The fixture session yields 6–8 `goal_phase` memories (matches
  human judgment of how many distinct phases the session had).
- Phase-1 token-overlap implementation stays as fallback when
  `EmbeddingService::is_enabled() == false`.
- Integration test compares both clustering modes against the same
  fixture and asserts embedding-mode produces strictly-fewer-or-equal
  phases (i.e. it merges more aggressively, which is the goal).

## What's NOT in scope

- LLM-synthesized representative text per cluster ("phase 3" from the
  original design doc). That's a separate follow-up.
- Multi-language embedding support. v0.2 is English-only.
- Cross-session clustering (phases from session A merging with phases
  from session B). Belongs in a different epic.
