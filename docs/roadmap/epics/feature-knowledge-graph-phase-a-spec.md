# Phase A — Technical Spec · BAML-driven Markdown Classification Pipeline

**Parent epic:** [Feature Knowledge Graph from Markdown Docs](feature-knowledge-graph.md)
**Status:** Proposal. ~1.5–2 weeks engineering. Single deliverable.
**Last updated:** 2026-05-22.

Sister doc to the parent epic. The parent lays out the 5-phase
roadmap; this doc is the deep technical spec for **Phase A** — the
LLM-driven markdown ingest pipeline using BAML + DeepSeek + Qdrant.

## What the user asked for, exactly

> *Give the contents of a .md file to an LLM (DeepSeek), ask for
> structured fields via BAML, embed the LLM output into Qdrant, then
> use a proper method to infer the full feature set, goals per feature,
> acceptance criteria per feature, FE/BE requirements — all of it.*

This doc nails down:

1. **Exact BAML function signatures** (fields extracted per chunk +
   per doc + cross-doc).
2. **DeepSeek-specific calling patterns** (chosen because of cost +
   reasoning capability on technical docs).
3. **Qdrant collection schema** (separate from ContextNest's
   substrate; this is a **lens** over it, not a replacement).
4. **Rolling-key alias-collapse algorithm** (so "auth migration"
   and "switching to OAuth" don't become two different features).
5. **Cross-document feature aggregation** — the algorithm that turns
   N independent `.md` files into one canonical feature catalogue
   with goals, AC, FE/BE requirements, and implementation status.

## Related work (the 8 papers actually on the nose)

Every architectural choice below is grounded in a 2024–2026 paper.

| Paper | Direct contribution |
|---|---|
| **Mangla 2026 — MDKeyChunker** (arXiv:2603.23533) | **The closest analog.** A 3-stage MD pipeline that splits structurally, enriches each chunk in **one LLM call extracting 7 fields** (title, summary, keywords, entities, questions, key, related_keys), then merges semantically-keyed chunks via bin-packing. Reports Recall@5 = 1.000 (BM25 over structural chunks). Their "rolling key dictionary" cap at K=40 + LRU eviction is the alias-collapse mechanism we adopt verbatim. |
| **Shrimal et al. 2025 — PARSE** (arXiv:2510.08623) | Schemas are NOT static contracts; they are *natural-language-understanding contracts that LLMs interpret*. Their **ARCHITECT** module iteratively refines a JSON schema (descriptions + pattern constraints + required-field detection); **SCOPE** is a reflection-based guardrail that yields 92% error reduction in the first retry. Both map onto BAML's "schema as the prompt" philosophy. |
| **Khalid et al. 2026 — ReqFusion** (arXiv:2603.23482) | **PEGS-guided prompting** (Project/Environment/Goals/System) reaches F1=0.88 vs generic prompting's F1=0.71 — a +0.17 absolute lift from giving the LLM **structured semantic anchors** instead of free-form "extract requirements". Their **multi-provider consensus** mechanism (OpenAI + Claude + Groq) cuts false-positive rate from 34% to 8%. We adopt PEGS as the per-doc field shape and lift consensus as an optional Phase A.2 guardrail. |
| **Ferguson et al. 2026 — ExtractBench** (arXiv:2602.12247) | Benchmark methodology for end-to-end PDF-to-JSON. The metric set (schema-breadth, hallucination rate, missing-span rate) ports straight to MD-to-JSON. We adopt their evaluation harness shape. |
| **Mehmood et al. 2025 — README Classification** (arXiv:2507.21899) | Validates that LLMs can classify **GitHub READMEs** by intent / project category at production accuracy. Our problem is the same shape but for the full epic/todo/plan/blame corpus. |
| **Tiwari et al. 2025 — OntoRAG** (arXiv:2506.00664) | Automated ontology derivation from unstructured KBs. Tells us the canonical-name set should be **derived bottom-up** from the corpus, not imposed top-down — which our basin-clustering already does. |
| **Mohammed et al. 2025 — RAGsemble** (arXiv:2601.05266) | A 9-LLM ensemble for industrial part specs. Provides a cost/quality breakpoint study: ensembling helps until ~3 providers, then plateaus. Sets the upper bound for Phase A.2 consensus. |
| **Yue 2025 — Triple Extraction from SES** (arXiv:2509.00140) | Zero-shot triple extraction from software-engineering standards. Validates that the (subject, predicate, object) triple shape works for spec docs — informs how we extract `feature → depends_on → feature` edges. |

## Architecture overview

```
                  ╔════════════════════════════════════════════════════╗
                  ║              .md corpus (epics/, todos/, etc.)      ║
                  ╚══════════════════════╤══════════════════════════════╝
                                         │
                                         ▼
   ┌───────────────────────────────────────────────────────────────────┐
   │  STAGE 1 · Structural chunker (Rust; pulldown-cmark)              │
   │  ‣ heading tree                                                    │
   │  ‣ code/table/list as ATOMIC units (never split)                   │
   │  ‣ adaptive token-bounded slicing (default ≤ 1500 chars/chunk)    │
   │  ‣ outputs: ChunkRecord { id, doc, heading_path, text, mtime }   │
   └──────────────────────────┬────────────────────────────────────────┘
                              │
                              ▼
   ┌───────────────────────────────────────────────────────────────────┐
   │  STAGE 2 · BAML → DeepSeek single-call enrichment                 │
   │  ‣ ONE call per chunk extracts ~12 fields (defined below)          │
   │  ‣ rolling key dictionary K (cap K=40, LRU evict)                  │
   │  ‣ output validated against BAML-generated typed struct            │
   └──────────────────────────┬────────────────────────────────────────┘
                              │
                              ▼
   ┌───────────────────────────────────────────────────────────────────┐
   │  STAGE 3 · Doc-level aggregator                                   │
   │  ‣ collapses all chunks of one .md into ONE DocSpec record         │
   │  ‣ extracts doc-level fields: purpose, top features, doc_kind     │
   │  ‣ second BAML call (per doc, not per chunk)                       │
   └──────────────────────────┬────────────────────────────────────────┘
                              │
                              ▼
   ┌───────────────────────────────────────────────────────────────────┐
   │  STAGE 4 · Embedding + Qdrant write                                │
   │  ‣ EmbeddingService (existing) → 256d vectors                      │
   │  ‣ embed CHUNK records → contextnest_spec_chunks                   │
   │  ‣ embed DOC records   → contextnest_spec_docs                     │
   │  ‣ canonical name from basin manager (existing) → contextnest_     │
   │    spec_features (one row per canonical feature)                   │
   └──────────────────────────┬────────────────────────────────────────┘
                              │
                              ▼
   ┌───────────────────────────────────────────────────────────────────┐
   │  STAGE 5 · Cross-doc feature inference (the synthesis layer)      │
   │  ‣ groups chunks by canonical feature                              │
   │  ‣ runs third BAML call PER FEATURE: synthesize {goal, AC, FE     │
   │    requirements, BE requirements, status, sources}                 │
   │  ‣ result lives in contextnest_spec_features collection            │
   │  ‣ this is what /api/v1/feature-specs serves                       │
   └───────────────────────────────────────────────────────────────────┘
```

Three LLM calls per pipeline run, **not** O(n·m) like the naive
per-field approach. MDKeyChunker proved this works.

## Why DeepSeek + BAML

**DeepSeek-V3 / DeepSeek-R1** specifically because:

1. **Strong reasoning** on technical / spec content (matches the
   benchmark profile of our corpus).
2. **~10× cheaper** than GPT-4 / Claude 3.5 at similar quality on
   structured extraction tasks (per ReqFusion's cost analysis —
   they used DeepSeek-R1 alongside Claude and GPT-4 and reported
   similar accuracy; the price gap makes DeepSeek default).
3. **OpenAI-compatible API** so the existing
   `EmbeddingService.embedding_providers::CustomHttpEmbedding` shape
   already handles it.

**BAML** specifically because:

1. **Typed function-call interface** — the schema is declared once,
   compiled into both the LLM prompt and the Rust struct that parses
   the response. We get type safety end-to-end.
2. **Reflection / retry built-in** — when DeepSeek returns malformed
   JSON, BAML's `@check` directives validate-and-retry without us
   writing the validation harness from scratch. This is what PARSE
   (arXiv:2510.08623) calls "schema as guardrail."
3. **Provider portability** — switching from DeepSeek to Claude is
   one config line, not a refactor. Important for the V1.5 multi-
   provider consensus extension.

## The three BAML functions

### Function 1 — `ClassifyChunk`

Per chunk. Single call extracts every field the substrate needs to
classify and link this chunk.

```baml
class ChunkAnalysis {
    // ── core classification ────────────────────────────────
    title           string         @description("3-8 word descriptive title")
    summary         string         @description("1-2 sentences. Focus on what is UNIQUE about THIS chunk.")
    section_kind    SectionKind    @description("epic | todo | blame | fix | plan | tracking | reference")
    purpose         string         @description("One sentence: what does this section EXIST to convey?")

    // ── feature identity ──────────────────────────────────
    feature_name    string         @description("Specific subtopic 2-5 words, lowercase. Use a key from rolling_keys when it fits.")
    feature_aliases string[]       @description("0-3 alternate names this section uses for the same feature")
    is_feature_spec bool           @description("true when this section describes a feature THE SYSTEM SHOULD HAVE.")

    // ── functional content (the user's explicit ask) ──────
    functional_features  string[]  @description("Concrete user-visible features this section talks about. Distinct from implementation details.")
    fe_requirements      string[]  @description("Frontend-facing requirements: UI, UX, API surface visible to the client.")
    be_requirements      string[]  @description("Backend / infra requirements: data flow, persistence, perf, ops.")
    acceptance_criteria  string[]  @description("Concrete conditions that mark this feature 'done'. Phrased as testable assertions.")

    // ── status signal (drives the state machine) ──────────
    status         Status         @description("proposed|in_design|building|testing|shipped|abandoned")
    status_evidence string        @description("Exact quote / line that justifies the status assignment. Empty when inferred.")

    // ── graph (typed edges, from arXiv:2604.14220) ────────
    refers_to     string[]       @description("Other feature_names this section depends on or cites")
    supersedes    string[]       @description("Older feature_names this one replaces")
    conflicts_with string[]      @description("Feature_names this one contradicts")

    // ── quality signals ───────────────────────────────────
    confidence    float          @description("0-1, your own confidence in this classification")
    keywords      string[]       @description("5-8 domain-specific terms; used by BM25 fallback")
    entities      Entity[]       @description("Named entities — see Entity class")
    questions     string[]       @description("2-3 natural-language questions this chunk answers")
}

class Entity {
    name string
    kind EntityKind  // PERSON | ORG | LOC | TECH | CONCEPT | EVENT | METRIC | FILE | API
}

enum SectionKind {
    epic todo blame fix plan tracking reference
}

enum Status {
    proposed in_design building testing shipped abandoned
}

function ClassifyChunk(
    chunk_text:      string,
    heading_path:    string,
    position:        int,
    prev_summary:    string,
    rolling_keys:    map<string, RollingKeyMeta>,
) -> ChunkAnalysis {
    client DeepSeekV3
    prompt #"
        Analyze a chunk of a software engineering markdown document.
        Heading path: {{ heading_path }}
        Chunk position: {{ position }} of N
        Previous chunk summary: {{ prev_summary }}
        Rolling keys (use one of these if a prior chunk discussed the same subtopic, do NOT coin a synonym):
        {{ rolling_keys | json }}

        Chunk text:
        {{ chunk_text }}

        Extract every field per the schema. Reuse rolling-key names where applicable.
        Be conservative on `status` — only assign `shipped` when there's an explicit textual marker.
        {{ ctx.output_format }}
    "#
}
```

Why these fields specifically (the user said "and similar ones you need to suggest"):

- **`purpose`** — separates "what" the section IS (epic, plan, etc.) from "what" it COMMUNICATES. Lets the synthesis layer pick the right writeup.
- **`functional_features` vs `fe_requirements` vs `be_requirements`** — the user's exact ask. Three separate arrays so the aggregator can render layer-specific views.
- **`acceptance_criteria`** — testable assertions. Drives the `how_to_test` field on the existing `delivered_features` index. Closes the dev loop.
- **`status_evidence`** — the exact quote justifying the status. Prevents hallucinated status assignments (PARSE 2025's failure mode #3).
- **`refers_to` / `supersedes` / `conflicts_with`** — typed edges from Chakraborty & Guha 2026, drive the SUPERSEDES resolution algorithm in the state machine.

### Function 2 — `ClassifyDocument`

Per doc, after all chunks are classified. One additional call gives us
a doc-level rollup (the "what is this whole .md file about" view).

```baml
class DocAnalysis {
    one_sentence_purpose  string         @description("Single sentence: why does this whole document exist?")
    two_sentence_summary  string         @description("Two sentences: what is the most important content of this document?")
    doc_kind              DocKind        @description("epic | spec | plan | retro | postmortem | tracking | reference | mixed")
    primary_features      string[]       @description("Up to 5 canonical feature names this document is primarily about")
    secondary_features    string[]       @description("Additional feature names mentioned but not central")
    primary_status        Status         @description("The document's overall implementation status, computed as the worst-of-its-features semantics")
    audience              Audience       @description("engineer | manager | designer | mixed")
    confidence            float
}

enum DocKind { epic spec plan retro postmortem tracking reference mixed }
enum Audience { engineer manager designer mixed }

function ClassifyDocument(
    chunks: ChunkAnalysis[],
    doc_path: string,
    doc_mtime: string,
) -> DocAnalysis {
    client DeepSeekV3
    prompt #"
        You have already classified every chunk of this document.
        Here are the per-chunk results: {{ chunks | json }}
        Document path: {{ doc_path }}
        Document mtime: {{ doc_mtime }}

        Produce a document-level rollup per the schema. The
        `one_sentence_purpose` is the most important field — it
        will become the document's stable description across the
        substrate.
        {{ ctx.output_format }}
    "#
}
```

This is the explicit answer to the user's "summarize in two
sentences" requirement, plus what every other doc-classification
paper validates as the high-signal doc-level fields.

### Function 3 — `SynthesizeFeature` (the synthesis layer)

The novel piece. After every chunk is classified and every doc has a
rollup, we group chunks by canonical `feature_name` and run ONE
synthesis call per canonical feature.

```baml
class FeatureSpec {
    canonical_name   string
    one_line_purpose string         @description("One sentence: what does this feature do, in user-visible terms?")

    goal             string         @description("Why this feature exists. The problem it solves.")
    acceptance_criteria  string[]   @description("Concrete testable assertions, merged + deduped across all sources")
    fe_requirements      string[]   @description("Frontend requirements, merged across sources")
    be_requirements      string[]   @description("Backend requirements, merged across sources")
    functional_behavior  string     @description("400-word max prose description of how the feature works end-to-end")

    current_status   Status
    status_history   StatusChange[] @description("Chronological list of status changes with evidence")

    depends_on       string[]       @description("Other canonical feature names this feature requires")
    superseded_by    string?        @description("Canonical name of the feature that replaces this one, if any")
    supersedes       string[]       @description("Canonical names this feature replaces")
    conflicts_with   string[]

    sources          ChunkSource[]  @description("Every chunk that contributed to this synthesis, sorted by mtime desc")
    open_questions   string[]       @description("Contradictions or gaps surfaced during synthesis")
    confidence       float
}

class StatusChange {
    from   Status?
    to     Status
    when   string
    evidence string
    source ChunkSource
}

class ChunkSource {
    doc_path     string
    heading_path string
    line_range   [int, int]
    mtime        string
}

function SynthesizeFeature(
    canonical_name: string,
    chunks: ChunkAnalysis[],
    chunk_texts: string[],
) -> FeatureSpec {
    client DeepSeekV3
    prompt #"
        Synthesize a single canonical specification for the feature
        named "{{ canonical_name }}" from these sources:

        {% for c in chunks %}
        --- source: {{ c.source.doc_path }} ({{ c.source.heading_path }}) mtime={{ c.source.mtime }} ---
        {{ chunk_texts[loop.index0] }}
        {% endfor %}

        Rules:
        - Merge acceptance criteria, FE requirements, BE requirements
          across all sources. Deduplicate semantically (not just by string).
        - When sources disagree, prefer the most recent by mtime. Add the
          conflict to `open_questions`.
        - `current_status` follows the get_valid_status algorithm:
          walk SUPERSEDES edges before falling back to freshest-wins.
        - Quote source line ranges when claims are not obviously true.

        {{ ctx.output_format }}
    "#
}
```

This is the **deterministic answer** to "infer the whole feature
set, the goal of each feature, AC, FE/BE requirements." Each
canonical feature gets ONE `FeatureSpec` record produced by ONE call
against all its sources. The output is the cached canonical spec
that future agents read.

## Qdrant collections

Three Qdrant collections, written by the four stages above. All
share the same 256-d embedding produced by ContextNest's existing
`EmbeddingService`.

### Collection 1 — `contextnest_spec_chunks`

One point per chunk. The fine-grained semantic-search lane.

```
point_id    : uuid5(NAMESPACE, doc_path + heading_path + line_range)
vector      : embed(chunk_text)   // 256d
payload     : ChunkAnalysis (full JSON above)
              + chunk_text
              + source = { doc_path, heading_path, line_range, mtime }
```

### Collection 2 — `contextnest_spec_docs`

One point per .md file. The doc-level "what is this whole document
about" lane.

```
point_id    : uuid5(NAMESPACE, doc_path + doc_mtime)
vector      : embed(one_sentence_purpose + " " + two_sentence_summary)
payload     : DocAnalysis (full JSON above)
              + doc_path, doc_mtime
```

### Collection 3 — `contextnest_spec_features`

**One point per canonical feature.** The authoritative answer to
"what features should this system have?" Updated whenever any source
chunk changes (Phase D doc-watcher triggers re-synthesis).

```
point_id    : uuid5(NAMESPACE, canonical_name)
vector      : embed(one_line_purpose + " " + goal + " " + functional_behavior)
payload     : FeatureSpec (full JSON above)
```

A query like *"What features handle authentication?"* hits Collection 3
directly. A query like *"Show me the AC for the OAuth flow"* gets a
single payload back with `acceptance_criteria` ready to render.

## Rolling-key alias collapse

Adopted verbatim from MDKeyChunker (arXiv:2603.23533).

```python
class RollingKeyManager:
    K_MAX = 40
    keys: dict[str, RollingKeyMeta] = {}   # name → metadata

    def update(self, key: str, chunk_index: int):
        if key in self.keys:
            self.keys[key].count += 1
            self.keys[key].last_chunk = chunk_index
        else:
            self.keys[key] = RollingKeyMeta(first_chunk=chunk_index,
                                            last_chunk=chunk_index, count=1)
        if len(self.keys) > self.K_MAX:
            # LRU evict
            stale = min(self.keys, key=lambda k: self.keys[k].last_chunk)
            del self.keys[stale]

    def to_prompt_dict(self) -> dict[str, dict]:
        # injected into ClassifyChunk's prompt
        return {k: {"count": v.count} for k, v in self.keys.items()}
```

Bound: O(K_MAX) prompt tokens per call, regardless of corpus size.
The LLM sees prior keys and reuses them instead of coining synonyms.

MDKeyChunker's data: **89.8% cross-reference rate** — most chunks
correctly reuse prior keys. This is why we don't need a separate
alias dedup pass.

## Cross-doc feature aggregation algorithm

The synthesis step (Function 3) needs a list of chunks per canonical
feature. The algorithm:

```python
def aggregate_features(all_chunks: list[ChunkAnalysis]) -> dict[str, list[ChunkAnalysis]]:
    # Stage 1 — direct grouping by feature_name
    direct_groups: dict[str, list[ChunkAnalysis]] = defaultdict(list)
    for c in all_chunks:
        if c.feature_name:
            direct_groups[c.feature_name].append(c)

    # Stage 2 — embedding-space dedup of canonical names
    # (catch the cases where MDKeyChunker's rolling key didn't fire)
    names = list(direct_groups.keys())
    name_vectors = embedding_service.batch_embed(names)
    clusters = basin_manager.cluster(name_vectors, threshold=0.85)
    # ↑ uses the existing AttractorBasinManager from Phase 1 of the
    #   neural-field epic. Same primitive, new use.

    # Stage 3 — merge groups whose names landed in the same basin
    merged: dict[str, list[ChunkAnalysis]] = {}
    for basin_id, member_names in clusters.items():
        # canonical name = the member with most chunks; ties → shortest name
        canonical = max(member_names,
                        key=lambda n: (len(direct_groups[n]), -len(n)))
        merged[canonical] = []
        for n in member_names:
            for c in direct_groups[n]:
                # tag the chunk's aliases for audit
                c.feature_aliases = list(set(c.feature_aliases + [n]))
                merged[canonical].append(c)

    # Stage 4 — also fold in alias mentions across chunks
    # If chunk A's feature_aliases contains chunk B's feature_name,
    # they're talking about the same thing.
    for canonical, chunks in merged.items():
        all_aliases = {a for c in chunks for a in c.feature_aliases}
        for other_canonical in list(merged.keys()):
            if other_canonical in all_aliases and other_canonical != canonical:
                merged[canonical].extend(merged.pop(other_canonical))

    return merged
```

Three layers of alias collapse:

1. **Rolling-key reuse** during ClassifyChunk (MDKeyChunker's
   contribution; 89.8% effective).
2. **Embedding clustering** of `feature_name` values (catches misses
   from layer 1).
3. **Explicit `feature_aliases` cross-reference** (catches misses
   from both prior layers when an author explicitly names the alias).

Coverage of these three layers should approach 100% on well-curated
corpora. The remaining false-separations surface as `open_questions`
on the resulting FeatureSpec records.

## End-to-end example (worked from `docs/roadmap/epics/neural-field-real.md`)

Let's trace what happens for the `### Phase 4 — Basin-aware retrieval boost`
section of this repo's actual neural-field epic.

### Step 1 — chunk

```
chunk_id     = uuid5(NS, "docs/roadmap/epics/neural-field-real.md" +
                        "Phases > Phase 4 — Basin-aware retrieval boost" +
                        "[142, 178]")
chunk_text   = (37 lines of the Phase 4 spec)
heading_path = ["Phases", "Phase 4 — Basin-aware retrieval boost"]
mtime        = 2026-05-21T11:00:00Z
```

### Step 2 — ClassifyChunk returns

```json
{
  "title": "Basin-aware retrieval boost",
  "summary": "When a query matches fragment X, also surface X's basin-siblings via cluster reinforcement at 0.7 of top similarity, merging into top-K.",
  "section_kind": "epic",
  "purpose": "Specify how the retrieve handler expands hits using attractor-basin membership of the top match.",
  "feature_name": "basin-aware retrieval",
  "feature_aliases": ["basin expansion", "cluster reinforcement"],
  "is_feature_spec": true,
  "functional_features": ["expand top-K with basin siblings", "configurable boost factor"],
  "fe_requirements": [],
  "be_requirements": [
    "/api/v1/tools/retrieve handler queries basin_manager after cosine sort",
    "basin lookup happens only when top-hit similarity > 0",
    "session_id filter applies to expanded siblings"
  ],
  "acceptance_criteria": [
    "Query matching single fragment in 20-member basin returns multiple members in top-K",
    "Query across multiple basins still surfaces diverse hits",
    "Variance ratio in /field PCA improves"
  ],
  "status": "shipped",
  "status_evidence": "✓ Phase 4: Basin-aware retrieval boost (#34)",
  "refers_to": ["consolidation worker", "real basins surface"],
  "supersedes": [],
  "conflicts_with": [],
  "confidence": 0.94,
  "keywords": ["basin", "retrieve", "cosine", "expansion", "top-K"],
  "entities": [
    {"name": "/api/v1/tools/retrieve", "kind": "API"},
    {"name": "AttractorBasinManager", "kind": "TECH"}
  ],
  "questions": [
    "How does basin expansion modify retrieve top-K?",
    "What's the boost factor for basin siblings?",
    "How is session affinity preserved during expansion?"
  ]
}
```

### Step 3 — `ClassifyDocument` rollup

The whole `neural-field-real.md` file becomes ONE record:

```json
{
  "one_sentence_purpose": "Wire up the dormant attractor/decay/connection/reconstruction modules across 7 phases so ContextNest's tagline matches its runtime.",
  "two_sentence_summary": "ContextNest's tagline 'neural-field attractor consolidation' described aspiration not runtime. This epic closes the gap across 7 independently-shippable phases anchored on a background consolidation worker that keeps ingest fast.",
  "doc_kind": "epic",
  "primary_features": ["consolidation worker", "decay at retrieve", "real basins surface", "basin-aware retrieval", "connection-network expansion", "auto reconstruction", "substrate health"],
  "secondary_features": ["last_accessed bump", "drift_score", "supersedes-edge resolution"],
  "primary_status": "shipped",
  "audience": "engineer",
  "confidence": 0.96
}
```

### Step 4 — `SynthesizeFeature` for `basin-aware retrieval`

If three different docs mention this feature, the synthesis layer
combines them. For just the one source here:

```json
{
  "canonical_name": "basin-aware retrieval",
  "one_line_purpose": "Surface attractor-basin siblings of the top retrieve hit at a configurable boosted similarity.",
  "goal": "Pure cosine retrieve misses past work in the same conceptual cluster as the query's top hit. Expanding by basin membership recovers those siblings without changing query intent.",
  "acceptance_criteria": [
    "Single-fragment match in a 20-member basin returns multiple members in top-K",
    "Cross-basin queries still surface diverse hits",
    "Variance ratio in /field's PCA improves post-expansion"
  ],
  "fe_requirements": [],
  "be_requirements": [
    "/api/v1/tools/retrieve handler queries basin_manager after cosine sort",
    "Basin lookup happens only when top-hit similarity > 0",
    "Session_id filter applies to expanded siblings"
  ],
  "functional_behavior": "After the existing cosine top-K is computed, the handler looks up the top hit's basin via attractor_manager.list_basin_snapshots. For each sibling fragment in that basin that's not already in the top-K and that passes the current candidate filter, a new RetrieveHit is appended with similarity = top_sim × CONTEXTNEST_RETRIEVE_BASIN_BOOST (default 0.7). The expanded list is re-sorted and truncated to the requested top_k. ...",
  "current_status": "shipped",
  "status_history": [
    {"from": null, "to": "proposed", "when": "...", "evidence": "Epic phases table", "source": {...}},
    {"from": "proposed", "to": "shipped", "when": "2026-05-22T...", "evidence": "PR #34", "source": {...}}
  ],
  "depends_on": ["consolidation worker", "real basins surface"],
  "superseded_by": null,
  "supersedes": [],
  "conflicts_with": [],
  "sources": [
    {"doc_path": "docs/roadmap/epics/neural-field-real.md", "heading_path": "Phases > Phase 4 ...", "line_range": [142, 178], "mtime": "2026-05-21T..."}
  ],
  "open_questions": [],
  "confidence": 0.95
}
```

This is the agent-consumable feature spec. Future Claude sessions
query `GET /api/v1/feature-specs/basin-aware-retrieval` and get
this back deterministically.

## Multi-provider consensus (V1.5 follow-up — optional)

ReqFusion (arXiv:2603.23482) shows multi-provider consensus drops
false-positive rate from 34% to 8%. Pattern:

```
              ┌─────────────────────┐
              │   DeepSeek-V3 call  │
              └─────────┬───────────┘
                        │
              ┌─────────┴───────────┐
              ▼                     ▼
   ┌──────────────────┐   ┌──────────────────┐
   │ Claude 3.5 call  │   │   GPT-4o-mini    │
   └────────┬─────────┘   └────────┬─────────┘
            │                      │
            └──────────┬───────────┘
                       │
                       ▼
            ┌──────────────────────┐
            │  consensus voter     │
            │  weighted by         │
            │  historical accuracy │
            └──────────┬───────────┘
                       │
                       ▼
            ┌──────────────────────┐
            │ confidence < 0.5 →   │
            │   flag for review    │
            │ confidence ≥ 0.5 →   │
            │   accept             │
            └──────────────────────┘
```

Implementation: BAML lets us define the same `ClassifyChunk`
function with three different `client` directives. The orchestrator
runs all three in parallel, the consensus voter (~50 LOC) does the
weighted merge. **Cost:** 3× LLM spend. **Benefit:** ~4× error
reduction. Worth it for the spec corpus, not for runtime telemetry.

**Recommendation:** ship V1 with DeepSeek-only. Add consensus in
V1.5 if the per-feature `open_questions` array is non-empty for >5%
of features in production. Don't pre-optimize.

## Schema-evolution loop (V2 follow-up — optional)

PARSE (arXiv:2510.08623) reaches 64.7% accuracy gain by *evolving*
the schema itself based on extraction failures. Pattern:

1. Run extraction on a held-out validation corpus.
2. Analyze the failure modes (missing fields, hallucinated values,
   pattern mismatches).
3. Refine the BAML schema's `@description` fields + add
   `@check` constraints.
4. Loop until gains plateau (~5–6 iterations per their data).

Skip for V1. The BAML schema above is hand-tuned against the
existing corpus shape; we'll know after 2 weeks of production data
whether PARSE-style auto-refinement is worth the engineering cost.

## Acceptance criteria for Phase A

Phase A is done when all of these are true for `docs/` of this very
repo:

- [ ] `contextnest ingest markdown docs/` completes in ≤ 10 minutes
      for the current corpus (~50 .md files, ~150k tokens).
- [ ] `contextnest_spec_chunks` collection contains ≥ 1 point per
      heading-level-2+ section.
- [ ] `contextnest_spec_features` has a canonical entry for every
      named epic phase across all the merged epic docs (Phases 1–7
      of neural-field-real, plus the 5 phases of feature-knowledge-
      graph).
- [ ] At least 80% of `delivered_features[]` entries from past z-insight
      blocks find a matching `FeatureSpec` (cross-validation against
      the existing runtime feature index).
- [ ] No two distinct features collapse to the same canonical name
      AND no single feature splits into >2 canonical names (manual
      audit on 20 sampled features).
- [ ] Re-ingest of unchanged files produces zero net Qdrant point
      changes (idempotency).
- [ ] `GET /api/v1/feature-specs/basin-aware-retrieval` returns a
      synthesized spec whose `acceptance_criteria` matches what the
      Phase 4 epic section actually said, to within a manual review.

## Risks

| Risk | Mitigation |
|---|---|
| DeepSeek hallucinates `acceptance_criteria` that weren't in source | `status_evidence` field forces a verbatim quote; synthesis layer flags AC without a source quote into `open_questions` |
| BAML JSON validation fails repeatedly | Retry with exponential backoff (built into BAML); after 3 failures, drop the chunk into a `errors_to_review` bucket — don't block the whole pipeline |
| Rolling key dictionary thrashes when LRU evicts a still-active key | LRU window of 40 is empirically tuned in MDKeyChunker (89.8% reuse). If our corpus drops below 80%, increase to 80 |
| Cross-doc synthesis call is too long (multi-page features) | Cap chunk count per synthesis call at 12; if more, do hierarchical synthesis (synthesize sub-clusters first, then synthesize the synthesis) |
| Status conflict between docs (epic says shipped, todo says building) | Resolved by the `get_valid_status` algorithm from the parent epic (walks SUPERSEDES edges before falling back to freshest-wins) |
| LLM cost on first ingest of large corpus | 3 calls per file + 1 per canonical feature. For 50 files + 100 features: 250 calls. At DeepSeek pricing ≈ $0.50 total. Negligible. |

## What we are NOT building in Phase A

- Watch mode (deferred to Phase D of the parent epic)
- Status state machine endpoints (Phase C)
- Critic-guided reflexion on status transitions (Phase D)
- Dashboard surface (Phase E.1)
- Multi-provider consensus (V1.5)
- Schema-evolution loop (V2)

Phase A is **just the cold ingest + synthesis pipeline.** It produces
Qdrant collections; reading them is what Phase E does.

## Implementation outline (~2 weeks, single deliverable)

```
Day 1-2  src/ingest/markdown/walker.rs          (~150 LOC + tests)
Day 1-2  src/ingest/markdown/chunker.rs         (~250 LOC + tests)
Day 3-5  src/ingest/markdown/baml/*.baml        BAML function defs
Day 3-5  src/ingest/markdown/classifier.rs      (~300 LOC + tests)
Day 6-7  src/ingest/markdown/rolling_keys.rs    (~80 LOC + tests)
Day 8-9  src/ingest/markdown/aggregator.rs      (~200 LOC + tests)
Day 8-9  src/ingest/markdown/synthesizer.rs     (~150 LOC + tests)
Day 10   src/ingest/markdown/qdrant.rs          (~200 LOC + tests)
Day 11   CLI: contextnest ingest markdown        (~50 LOC)
Day 12   End-to-end test against docs/ of this very repo
```

~1400 LOC total, mostly mechanical once the BAML schema is settled.

## Sources

Every paper cited in this doc, in arXiv ID order:

- [arXiv:2509.00140 — LLM-based Zero-shot Triple Extraction for Automated Ontology Generation from Software Engineering Standards](https://arxiv.org/abs/2509.00140)
- [arXiv:2507.21899 — LLM-based Content Classification for GitHub Repositories by README Files](https://arxiv.org/abs/2507.21899)
- [arXiv:2506.00664 — OntoRAG: Automated Ontology Derivation from Unstructured KBs](https://arxiv.org/abs/2506.00664)
- [arXiv:2510.08623 — PARSE: LLM Driven Schema Optimization for Reliable Entity Extraction](https://arxiv.org/abs/2510.08623)
- [arXiv:2601.05266 — RAGsemble: Multi-LLM Ensemble for Industrial Part Specification Extraction](https://arxiv.org/abs/2601.05266)
- [arXiv:2602.12247 — ExtractBench: Benchmark and Evaluation Methodology for Complex Structured Extraction](https://arxiv.org/abs/2602.12247)
- [arXiv:2603.23482 — ReqFusion: A Multi-Provider Framework for Automated PEGS Analysis](https://arxiv.org/abs/2603.23482)
- [arXiv:2603.23533 — MDKeyChunker: Single-Call LLM Enrichment with Rolling Keys for High-Accuracy RAG](https://arxiv.org/abs/2603.23533)
- [arXiv:2604.14220 — Knowledge Graph RAG: Agentic Crawling in Enterprise Documents](https://arxiv.org/abs/2604.14220)
- [arXiv:2601.11688 — SpecMap: Hierarchical LLM Agent for Datasheet-to-Code Traceability](https://arxiv.org/abs/2601.11688)
