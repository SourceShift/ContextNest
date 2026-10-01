# Session goal drift — why a session stops being findable by its earlier work

**Date:** 2026-10-01
**Trigger:** a real retrieval failure. A session that spent its first hour
comparing an external repo against mini-ork could not be found by that
work once the session had moved on to a different goal.
**Corpus studied:** 316 unique arXiv papers surfaced by 28 queries, all 316
abstracts read, 43 read in full. Coverage caveats in the last section.

---

## TL;DR

The data is fine. The substrate stores **15 goal phases** for the session in
question, and phase 0 is the exact goal that could not be retrieved. Two of
the three ways to ask about a session throw that list away and answer with
the last phase only.

| Endpoint | What it does with the session's 15 phases | Works? |
|---|---|---|
| `GET /sessions/:id/trajectory` | returns all 15, with `phase_idx` on every record | yes |
| `POST /sessions/find` | searches every phase; returns `match_text` of the best one | yes |
| `GET /sessions/:id/summary` | keeps **1** (`latest by ts`) | lossy |
| `GET /sessions/by-intent` | embeds **1 vector** from that 1 phase | lossy, and times out |

So this is not a memory problem. It is a **projection** problem with a
**truncation** problem and a **latency** problem stacked on it.

---

## 1. The reproduction, live

Against the production substrate (337,928 fragments), session
`693dc40a-2fee-4f65-a511-665f6a0fe6bc`.

**`/summary` — the collapsed view.** Everything a caller needs to decide
"is this the session I mean" is gone:

```
$ curl -s localhost:28080/api/v1/sessions/693dc40a-.../summary
fragment_count: 287
  domain:        backend
  goal:          Ship the mini-ork certify one-liner demo that attracts
                 first users, per the 90-day plan
  current_state: certify C1-C3 merged and CI green; demo pair is ...
  topics:        ['90-day plan', 'lane cwd safety', 'live acceptance',
                  'mini-ork certify']
```

No "Raven". The `topics` union does not contain it either. The `domain` has
flipped from `research` to `backend`.

**`/trajectory` — the view that kept everything.** Same session, same
substrate:

```
phases: list[15]
  phase 0: "Find out how much EverMind-AI/Raven's code reuses mini-ork techniques"
           start_ts 2026-09-29T05:40:37.802Z
```

**`/find` — already answers correctly.** This is the important correction to
the original report:

```
$ curl -s -XPOST localhost:28080/api/v1/sessions/find \
    -d '{"query":"which session compared Raven vs mini-ork techniques"}'
  score=0.792  session_id=693dc40a-...
  match_kind: "goal_phase"
  match_text: "Find out how much EverMind-AI/Raven's code reuses mini-ork techniques"
```

`find` indexes `goal_phase` + `session_title` (`FIND_INDEXED_KINDS`) and
returns *which* phase matched. It is the one session-level entry point that
behaves correctly. The failure the user hit was real, but it is confined to
`/summary` and `/by-intent`, and to whatever consumes them.

**`/by-intent` — does not answer at all.**

```
$ time curl -s 'localhost:28080/api/v1/sessions/by-intent?q=which+session+compared+Raven...'
[http_code=000  time=120.00s]
```

Two minutes, no response. It builds one intent string per session and
embeds each on cache miss, so a cold call is O(sessions) embedder calls
(`src/api/sessions.rs:2899-2915`). It is the endpoint whose entire purpose
is "which session was about X".

---

## 2. Where the goal goes

Ingest already does the hard part. `cluster_goal_phases`
(`src/ingest/claude_code/extractor.rs:1485`) groups consecutive z-insight
`goal` values by token overlap, then `refine_goal_phases_by_embedding`
(line 1516) re-clusters them at cosine ≥ 0.85. Each resulting `GoalPhase`
carries `start_ts`, `end_ts`, `turn_count`. So one session with N distinct
goals produces N `GoalPhase` fragments, and they are all stored.

```
  INGEST                                                    STORAGE
  ──────                                                    ───────
  turn 1  goal:"compare Raven vs mini-ork"  ──┐
  turn 2  goal:"compare Raven vs mini-ork"  ──┤ cluster (token overlap,
  turn 3  goal:"turn comparison into tasks" ──┘  then cosine 0.85)
                    │
                    ▼
            GoalPhase #0  {text:"Find out how much EverMind-AI/Raven's
                           code reuses mini-ork techniques",
                           start_ts:05:40:37, end_ts:..., turn_count:5}
            GoalPhase #1  {text:"...prioritized task list..."}
            ...
            GoalPhase #14 {text:"Ship the mini-ork certify one-liner demo..."}
                    │
        ┌───────────┼────────────────────────────────┐
        ▼           ▼                                ▼
   /trajectory   /find                        /summary  /by-intent
   returns all   searches all                 keeps max(ts) ONLY
   + phase_idx   returns match_text           → embeds ONE vector
```

Everything left of the fan-out is correct. Both defects are on the right.

### Defect D1 — projection collapses to `max(ts)`

`session_summary` collects every `goal_phase` into a `Vec<(ts, text)>`
(`src/api/sessions.rs:1369`), sorts it, then:

```rust
let goal = goals.into_iter().next().map(|(_, t)| t);   // line 1438
```

`.next()` on a descending sort is "the latest". The other 14 are dropped.
Same for `state`. The reason is back-compat: `SessionSummaryPayload.goal`
is a single `Option<String>` because the z-dashboard categorizer wants one
string to put in a prompt. That consumer is satisfied by the latest goal
and is *not wrong* — but it means the schema has no place to express "this
session has been about five things".

### Defect D2 — one vector per session, built from the collapsed view

`compute_intent_text` (`src/api/sessions.rs:2970`) concatenates
`domain · topics · goal · current_state` — the collapsed fields — and
`by_intent` embeds that one string per session. The cache is keyed on a
hash of the string, so the comment at line 2913-2915 is explicit that the
entry invalidates "when the intent-text hash drifts (new goal_phase
fragment, topics changed)". Invalidation is exactly the moment the old
intent becomes unreachable.

This is the textbook case the information-retrieval literature calls an
index-granularity failure, and the literature is unanimous that the fix is
more vectors, not a better single vector. See §4.

### Defect D3 — the declared anchor is discarded at ingest

`grep -rn work_unit src/` → **zero hits**.

The z-insight protocol instructs the agent to emit, every turn:

```json
"work_unit": {"id": "wu-4f2a9c11", "title": "retry loop in the dispatcher",
              "phase": "implement"}
```

with the explicit rule that the id is carried across turns until the work
is delivered. That is a *declared* segmentation boundary. The substrate
currently *infers* boundaries by clustering goal text, which is strictly
harder and strictly less accurate — and then throws away the declaration.
`tasks[]` is ingested, but keyed only by task id/subject, with no link to
the work unit or the turn window it belonged to.

### Defect D4 — `find` truncates to the newest 5000 intent fragments

`FIND_CANDIDATE_CAP = 5000` (`src/api/sessions.rs:2666`), and the live
response reports `"truncated": true`. With 337,928 fragments across
sessions holding 5-15 phases each, 5000 covers roughly the newest few
hundred sessions. Older phases drop out of the candidate set entirely.
Nothing in the response says *which* sessions were excluded — `truncated`
is a boolean, so a caller cannot tell a genuine miss from a cap miss.

---

## 3. Why this is the interesting version of the problem

It is tempting to file this as "summaries are lossy". It is not. Three
distinctions matter and they change the fix:

1. **The old goal is not wrong.** It is not an obsolete fact that should be
   suppressed. For a question about 05:40-06:30 on 2026-09-29, phase 0 *is*
   the correct answer, and the certify goal is the wrong one. This is a
   **validity-window** problem (what was true *then*), not a knowledge-update
   problem (what is true *now*). Multi-source memory work makes the same
   split — recording observation time separately from validity time is what
   lets a system answer both.
2. **Deleting nothing is not enough.** The fragments are all present and
   atom-level search finds them. A store can be complete and still be
   unaddressable through the surface callers actually use.
3. **The fix must be reversible.** Any scheme that folds phases into a
   single new record re-creates the bug one level up.

---

## 4. What the literature says

316 unique papers, 28 queries. Four mechanisms recur, in order of how
directly they apply and how cheap they are here.

```
  cheap & no LLM ───────────────────────────────────────► needs an LLM
  ┌──────────────┬───────────────┬──────────────┬──────────────────┐
  │ M1 phases as │ M2 multi-     │ M3 declared  │ M4 hierarchical  │
  │ a first-class│ vector index  │ boundary     │ consolidation    │
  │ projection   │ (k per session)│ (work_unit) │ + provenance     │
  └──────────────┴───────────────┴──────────────┴──────────────────┘
     fixes D1       fixes D2        fixes D3        optional, later
```

### M1 — a segment is a sibling view of the raw turns, never a replacement

The single strongest consensus. **SCALE-QA/TSIM** (arXiv:2608.25655)
stream-segments a long thread by semantic shift and indexes each episode
through *three* views — raw, summary, cluster — all anchored to the same
episode, aggregating them into one episode score. It returns episodes, not
turns: 70.7 % evidence-hit vs 7.7 % for standard RAG. **SECOM**
(arXiv:2502.05589) is the clearest statement of the motivating defect: one
session covers multiple topics, so session-level units inject irrelevant
content and the retrieval unit should be a coherent *segment*. Measured on
LOCOMO GPT4-Score: turn-level 57.99, **session-level 51.18 (worst)**,
SECOM 69.33.

**Human-Inspired Memory** (arXiv:2605.08538) supplies the number that
should govern any consolidation design: aggressive merge of episodes drops
78.4 % → 48.4 %, and the paper's conclusion is blunt — *deduplicate, do not
summarise*. **MemoryOS** (arXiv:2506.06326) and **HORMA**
(arXiv:2606.11680) both build retrieval around segments rather than whole
sessions and report gains; HORMA does it with a filesystem hierarchy that
links every note back to raw trajectories, at ≤ 22 % of baseline tokens.

### M2 — several embeddings per entity, from different facets

This is the direct counter to one-vector-per-session, and three independent
systems do it:

| System | Vectors per unit | Measured |
|---|---|---|
| **LinkedIn HLTM** (arXiv:2604.26197) | facets + answerable-QA + summary, each embedded separately | multi-view updates touch only the leaf→root path |
| **CAST** (arXiv:2602.06051) | scene + character profile + graph node per episode | ablating short-window views costs −13.38 |
| **TSIM** (arXiv:2608.25655) | raw + summary + cluster per episode | 7.7 % → 70.7 % evidence-hit |
| **RECIPER** (arXiv:2604.11229) | paragraph + LLM procedure summary | +3.73 Recall@1, +2.85 nDCG@10 |
| **Multi-Prefix Embedding** (arXiv:2606.23642) | K ≈ 128 at 64-token chunks | agent answer accuracy 42.29 % → 51.45 % |

Two counterweights worth recording. **Chain of Retrieval** (arXiv:2507.10057)
and **DTCRS** (arXiv:2604.07012) decompose the *query* instead, which needs
no reindexing. And **Semantic Compression Trees** (arXiv:2608.21610) is a
warning against hierarchical *routing*: descending a compressed tree picked
the right document 20.2 % of the time vs 39.3 % flat. Build parents for
context expansion; retrieve flat.

### M3 — the boundary can be declared instead of inferred

**Pull** (arXiv:2609.14773) inserts an explicit tier between turn and
session — a per-turn ~100-char directory plus an entity-lifecycle graph —
built by a deterministic regex + ONNX purifier at 3.22 ms/turn with **zero
LLM calls**, and freezes turns beyond 300 into block summaries that stay
re-expandable (reversible, not lossy). **GAM** (arXiv:2604.12285)
consolidates on a detected semantic boundary rather than a token count.
**MERIT** (arXiv:2606.00547) keeps episode-level and turn-level stores and
retrieves per horizon.

For ContextNest this is the cheapest available win, because the boundary is
already being emitted and thrown away (D3). **Push Your Agent**
(arXiv:2605.23574) treats a work unit as the externally checkable unit of
progress and names the failure modes it enables measuring — false
completion, premature stopping, duplicate work, progress inflation. Those
are exactly the metrics that would have caught this bug.

### M4 — provenance links, and revision instead of overwrite

**Episodic-to-Semantic Consolidation** (arXiv:2607.01988) is the cleanest
design in the corpus: an append-only episodic log plus a derived semantic
store built by *deterministic* aggregation, where each derived fact carries
`last_supporting_event_id` and the run records the processed row range — so
any summary is traceable to the raw events behind it, with no LLM.
**ReTree** (arXiv:2608.10676) does dependency-directed revision: on a
contradiction it revises the introducing node, appends `(old, new, ρ)` to
that node's history, and prunes only descendants. **WikiSkill**
(arXiv:2608.27454) stamps every pattern page with the trace ids it cites.
Counter-example worth avoiding: **Trace2Skill** (arXiv:2603.25158)
deliberately drops per-trajectory links at merge time.

**Reversible Forgetting** (arXiv:2608.18177) supplies the state model —
`active | dormant | retired` with asymmetric thresholds and a transition
ledger — for the case where suppressing a memory is genuinely correct.
That is *not* the case here (see §3), but the ledger shape is the right
container for a phase's validity window.

### Evidence on the latency half of D2

The corpus has no direct measurement of O(sessions) cold-cache embedding
cost, because no surveyed system embeds per-entity on the request path.
The relevant finding is the inverse: **"Don't Ask the LLM to Track
Freshness"** (arXiv:2606.01435) shows the bottleneck is post-retrieval
*assembly*, not storage, and **"Selection vs Extraction"** (arXiv:2609.34227)
measures write-time extraction as **3,061× more expensive** than storing
raw dated turns. Both point the same way: precompute the per-phase vectors
at consolidation time, keep the request path to one query embedding.

---

## 5. Proposed fix

Staged so each stage is independently shippable and independently useful.
Stages 1-2 need no LLM call and no schema migration.

### Stage 1 — stop the projection from lying (fixes D1, D4)

Add `phases: Vec<SessionPhase>` to `SessionSummaryPayload`, where each
entry is `{text, start_ts, end_ts, turn_count}`. Keep `goal` and
`current_state` unchanged as the latest-value back-compat fields, so the
z-dashboard categorizer is untouched. This is additive and the data is
already in hand — `session_summary` builds the `goals` vector and then
throws all but one away at line 1438.

Make the truncation honest: change `truncated: bool` on `/find` to carry
the candidate count and the since-window, or raise the cap and surface
`candidates_scored`. A boolean that hides "your session was outside the
window" is worse than no field.

### Stage 2 — one vector per phase, not per session (fixes D2)

Replace `compute_intent_text`'s single string with one embedding per
`GoalPhase`, computed by the **existing background consolidation worker**
rather than on the request path. Cache keyed `(session_id, phase_id)`.

```
  before                          after
  ──────                          ─────
  1 embedding / session      →    1 embedding / phase
  built from max(ts) only         built from that phase's own text
  cold call = O(sessions)         cold call = 1 (query only)

  session X   ──► [ v_latest ]    session X ──► [ v_ph0, v_ph1, ... v_ph14 ]
  query ──────► cosine ──► rank   query ─────► max over phases ──► rank
                                              + return the matching phase
```

The literature's warning (arXiv:2608.21610) is to build the extra vectors
for *matching* but not to route through a tree. Ranking by best-matching
phase and returning which phase matched is flat retrieval with a
multi-vector index — exactly TSIM/HLTM/CAST. At 337 k fragments and ~10
phases per session this is a few thousand extra vectors, not a reindex.

### Stage 3 — accept the declared boundary (fixes D3)

Add `MemoryKind::WorkUnit` and ingest `z-insight.work_unit.{id,title,phase}`.
Stamp `work_unit_id` onto every fragment arriving in that turn. When
present, the work unit *replaces* the inferred `GoalPhase` boundary for
grouping; when absent, clustering stays as the fallback. This makes
segmentation deterministic where the agent already did the work, and it
gives `tasks[]` the back-link it currently lacks.

Because it is a declared id carried across turns, this also makes phases
stable under re-ingest — an inferred cluster can renumber when one turn's
text changes; a declared id cannot.

### Stage 4 — ask as-of questions (the actual user need)

`/by-intent` and `/find` gain an optional time window, and `/summary` gains
`?as_of=<ts>`, answering with the phase valid at that instant. The
motivation, stated almost word for word in **Chronological Knowledge
Retrieval** (arXiv:2604.14169), is that "prioritizing recency can
overwhelm semantic similarity", so that work retrieves independently per
discrete time span rather than reranking by date. Validity windows come
from adjacent phase timestamps — `valid_from = phase.start_ts`,
`valid_to = next_phase.start_ts` — with no annotation, and the A→B→A case
(two goals revisited) falls out correctly because both phases exist and
neither is deleted.

### What to measure

A regression test with a shape, not a vibe: ingest a synthetic session with
three disjoint goals, then assert that querying each goal's keywords
returns the session and names the *correct* phase index. `find` already
returns `match_kind`/`match_text`; the assertion is cheap to write and
would have caught all four defects.

---

## 6. Coverage and what was not read

Honest accounting, because the numbers are easy to inflate:

- **316 unique papers** were surfaced by 28 batched semantic+BM25 queries
  and deduplicated by arXiv id. All 316 abstracts were read.
- **Caveat on the abstracts:** the search API returns them truncated to
  exactly 400 characters, so abstract-level reading covers the first ~400
  chars of each, not the full abstract. Section claims resting only on an
  abstract are marked as such in the notes they came from.
- **43 papers were read in full**, via `get_paper_chunks`: the ones cited
  in §4 plus the segmentation and drift clusters. Numbers quoted in §4
  come from full texts.
- Not read in full: the remaining ~273, including all of the CV/event-camera
  and audio-segmentation hits that the broad "segmentation" queries pulled
  in and that have no bearing on text.
- **arXiv:2608.06663** ("The Horizon Gap", a 1,547-paper survey of
  long-horizon agent failures) is cited nowhere above: its full text did
  not return within the tool's size limit and only its abstract was read.
  It is the most likely source of a taxonomy worth returning to.
