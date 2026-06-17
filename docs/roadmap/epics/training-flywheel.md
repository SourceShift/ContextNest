# Epic — Training-data flywheel

**Status:** Proposal. ~4-5 days across 3 follow-up PRs. Each independently
shippable; no big-bang merge.

**Owner:** TBA.

**Last updated:** 2026-06-16.

## What shipped (slice #0)

`GET /api/v1/training/provenance-pairs` (PR #156) harvests the provenance
signal the substrate already grounds (#149–#155) into a labeled corpus:
every fragment carrying a `provenance` tier emitted as a
`(content, provenance_tier)` row, with a zero-filled five-tier histogram
(`tier_counts`) reporting dataset balance across the window.

That closed HarnessBridge §3 integration point #5's *lowest-risk* slice —
read-only over existing sidecars, no schema change, no new write path. It
proved the signal is harvestable. It did **not** answer how the corpus
gets exported, evaluated, or scrubbed before it leaves the box. Those are
the three follow-ups below.

## The honest problem

The live endpoint is a *moving target* — it caps at 500 rows, reflects
whatever the substrate holds *right now*, and serves raw content over the
localhost boundary with no redaction. That's fine for "show me the corpus
shape." It's wrong for "freeze a reproducible training set" and "let this
corpus cross a machine boundary." A fine-tune or eval needs a stable
artifact with a manifest; anything leaving the box needs scrubbing.

## Follow-up slices

| # | Slice | Why it matters | Effort |
|---|---|---|---|
| 1 | [Export-time privacy filter](#slice-1--export-time-privacy-filter) | Gates *any* non-localhost use of the corpus. Until raw content is scrubbed at the boundary, the flywheel can't leave the dev box. | ~1.5 days |
| 2 | [Grounding-rate eval](#slice-2--grounding-rate-eval) | The flywheel's *measurement*. Without it, "the agent got better at grounding claims" is an unfalsifiable assertion. | ~1.5 days |
| 3 | [`training export` CLI + frozen JSONL](#slice-3--training-export-cli) | The flywheel's *artifact*. Turns the moving-target endpoint into a reproducible, manifest-stamped training set. | ~1.5 days |

### Sequencing

**#1 (privacy filter) is the critical-path follow-up.** Both #2 and #3
move corpus content somewhere — to an eval harness, to a `.jsonl` on
disk that gets shared. Neither should ship the raw path before the
boundary is scrubbed. Privacy first is not gold-plating; it's the
precondition for the other two having a safe output target.

**#2 and #3 are siblings** — same corpus, two consumers (a metric, a
file). Order them by which you want to validate first: the eval answers
"is the flywheel working?", the export answers "can I feed it to a
fine-tune?". The export's frozen JSONL is also the natural *input* to a
batch eval run, so #3 → #2 if you want the eval to read files instead of
the live endpoint.

---

## Slice #1 — Export-time privacy filter

**What.** A dedicated `ExportFilter` applied at the export boundary —
**not** at ingest. Called just before `Json(...)` in
`list_provenance_pairs` (and the slice-#3 CLI). The substrate stays
*complete*; only data crossing the boundary is scrubbed.

**Why export-time, not ingest-time.** The existing
`E-privacy-filter.md` epic redacts at *ingest* — keeping secrets out of
the substrate entirely. That's a different trust boundary. The flywheel's
problem is the opposite end: the substrate is trusted local storage, but
its *content* — absolute paths revealing `/Users/admin/...`,
secret-shaped tokens, internal URLs in `how_to_test`, the `project_cwd`
fingerprint, session UUIDs — must not ride a training corpus to another
machine. Redacting at ingest would *lose* data the substrate legitimately
needs for retrieval; redacting at export preserves the substrate and
scrubs the copy.

**Stance: opt-out, redaction always-on at export.** Mirror
`discover_sessions`' `.cn-ignore` opt-out (today it gates ingest but is
*not* honored by `list_provenance_pairs`). Then run every exported row
through the filter unconditionally:

- **content** — through `Redactor::with_defaults()` (already exists at
  `src/ingest/claude_code/redactor.rs`: aws / github_pat / anthropic /
  openai / gcp / ssn / credit_card rules) — currently **not wired into
  the live `ServicesSink::store` hot path**, so reuse it here.
- **`project_cwd`** — hash (plain digest) so cross-project balance is
  still countable without leaking the path.
- **`session_id`** — salted hash so rows from one session still cluster
  without exposing the raw UUID.

**Done when:** an exported corpus contains no absolute paths, no
secret-shaped tokens, no raw `project_cwd`/`session_id`; `.cn-ignore`'d
projects are absent; substrate retrieval is unchanged.

## Slice #2 — Grounding-rate eval

**What.** A single metric over the harvested corpus:

```
grounding_rate = observed / (observed + claimed + absent + contradicted)
```

`partial` is **excluded** — it's instrument ambiguity (a run exists but
the outcome was unclassifiable), not an agent behavior, so folding it
into either numerator or denominator distorts the signal.

**Why a grounding-rate, not a classifier or RL.** A classifier is
*circular* — the substrate's `provenance_weight` is already a hand-coded
classifier; training a model to reproduce its labels measures nothing.
RL/DPO is premature — we have no reward signal yet and no volume. The
grounding-rate is the honest first metric: it asks "of the claims this
agent made that we could check, what fraction were receipt-backed?" and
needs zero new model machinery.

**Split: temporal, not random.** Reuse the endpoint's existing `since=`
window as the held-out boundary — older turns train/baseline, newer
turns evaluate. Random splits leak future behavior into the baseline.
"Improved" = **+0.10 absolute** grounding-rate within the *same
project/agent* (cross-agent comparison is noise until slice-#2's data gap
is closed).

**Data gap (blocks honest comparison):** the corpus has no
**`agent_version` / `model_id`** field, so a grounding-rate delta can't
distinguish "the agent improved" from "a different agent/model was used."
Capturing this is a prerequisite for the metric to mean anything across
time. **`turn_index`** is a nice-to-have (enables grounding-vs-context-
length curves) but not blocking.

## Slice #3 — `training export` CLI

**What.** A new `contextnest training export --since --tier --project
--out` subcommand + a `cn-training-export` Makefile target (mirrors
`cn-ingest`). Writes **frozen JSONL**, not the live endpoint.

**Why a CLI, not the endpoint.** The endpoint is a moving target with a
500-row cap — wrong for a reproducible set. The CLI snapshots the
substrate to an immutable file you can version, diff, and feed to a
fine-tune.

**Row schema (chat-format, fine-tune-ready):**

```json
{
  "messages": [
    { "role": "user", "content": "<reconstructed prompt context>" },
    { "role": "assistant", "content": "<the claim>" }
  ],
  "metadata": {
    "fragment_id": "...", "session_id": "...", "kind": "...",
    "provenance": "observed", "ts": "...", "source_fragment_ids": [...]
  },
  "input": "<raw content, pre-chat-format>"
}
```

**Manifest sidecar** `*.jsonl.manifest.json`: `schema_version`,
`exported_at`, `substrate_version` + commit, `since_window`, `tier` /
`project` filters, `row_count`, `tier_counts`, `sha256` of the JSONL.
The manifest is what makes the export *reproducible* — it records exactly
which substrate state and filters produced the file.

**Composes with slice #1:** the CLI runs the same `ExportFilter` before
writing each row, so the on-disk corpus is scrubbed by construction.

---

## Out of scope (not yet epics)

- Labeling/eval *harness* beyond the single grounding-rate metric — no
  human-in-the-loop relabeling, no active learning.
- Any *training* run (fine-tune, DPO) — this epic produces and measures
  the corpus; consuming it is downstream.
- `turn_index` capture — deferred with slice #2's data gap.

## Versioning

Ships as part of v0.2.x as slices land. Independent of the v0.3
LLM-proxy milestone.
