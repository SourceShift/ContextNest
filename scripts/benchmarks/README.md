# ContextNest live-feature benchmarks

Smoke + perf scripts that exercise shipped features against a **live
populated substrate** — the binary running on `make cn-serve` with
real ingested sessions, not the mock-mode test harness.

The goal: catch regressions and surface performance gaps that the
unit/integration test suite can't see, because those run against
~10-session mock-mode setups while production has thousands.

## Scripts

### `cn-feature-smoke.sh`

One-shot health + correctness + latency check across every shipped
HTTP surface:

- `/api/health`, `/api/v1/substrate/{config,health}` — baseline shape.
- `/api/v1/sessions/by-{file,feature,intent}` — substring + semantic
  session search (known-answer pairs captured from real sessions).
- `/api/v1/tools/retrieve` — fragment retrieval, plus
  `group_by:"session"` rollup (Option B) and `exclude_kinds` filter.
- `/api/v1/fragments?session_id=…` — verifies `_cn_content_density`
  (Option A) lands on newly-consolidated fragments.
- `/llm/v1/cache/stats` — proxy cache shape.
- `/api/v1/sessions/:id/summary` — session-summary projection.
- `/api/v1/sessions` — total ingested count.
- `/api/v1/inbox` — inbox kind-extension (PR #120).

Output is markdown with one row per check: pass/fail, wall-clock
latency, and a one-line evidence string (the actual top session id /
similarity score / count returned). Pipe to a file for archiving:

```bash
./scripts/benchmarks/cn-feature-smoke.sh > /tmp/cn-smoke-$(date +%Y%m%d).md
```

Exit code is 0 when every check passes, 1 when at least one fails.
The full report is still printed on failure so the operator sees
which property broke and what the actual response was.

Override the substrate URL via env: `CN_URL=http://host:port`.

## When to run

- **Before a release/tag** — sanity check that the shipped binary
  doesn't regress any of the load-bearing operator queries.
- **After a config change** — flipping the embedder, the redactor,
  encryption, or any feature flag in `config.toml`.
- **When users report "search feels worse"** — has substring+kind+
  density+rollup all simultaneously regressed, or only one signal?
  The smoke isolates the affected feature.

## What this is NOT

- **Not a load test.** Each endpoint is hit once; concurrent
  throughput isn't measured. Use `vegeta` / `wrk` against
  `/api/v1/tools/retrieve` if you want QPS numbers.
- **Not a unit test substitute.** The unit + integration suites
  (`cargo test`) run against deterministic mocks and catch
  correctness regressions early. This script is the production
  equivalent for the cases mocks can't reach.
- **Not a benchmark for ranking quality.** "Did the right session
  rank #1" is a property of the embedder (Qwen3) + the index, not
  of the endpoint contract. Use the dashboard's `/search` page +
  human judgment for ranking-quality checks.

## Dependencies

Bash, `curl`, `jq`, `perl` (Time::HiRes, ships in macOS+Linux
default Perl). No npm/cargo/python required.
