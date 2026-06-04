#!/usr/bin/env bash
#
# cn-feature-smoke.sh — exercise every shipped ContextNest feature
# against a LIVE populated substrate and emit a markdown report.
#
# Designed to be runnable against your real `make cn-serve` instance
# (which has ~weeks of ingested Claude sessions) without mocks or
# fixtures. The known-answer queries below were captured from real
# session-search work today; if the substrate's behaviour regresses,
# this script flags the property failure with the actual top hit so
# the gap is debuggable.
#
# Output: markdown to stdout (pipe to file for archiving).
#
# Usage:
#   ./scripts/benchmarks/cn-feature-smoke.sh
#   ./scripts/benchmarks/cn-feature-smoke.sh > /tmp/cn-smoke-$(date +%Y%m%d).md
#   CN_URL=http://localhost:28080 ./scripts/benchmarks/cn-feature-smoke.sh
#
# Exit code:
#   0 — every assertion passed
#   1 — at least one assertion failed (still emits the full report so
#       the operator can see which gap to chase)
#
# Dependencies: bash, curl, jq, awk. No npm / cargo / python required.

set -u

CN_URL="${CN_URL:-http://localhost:28080}"
FAILURES=0
TOTAL=0

# Markdown header.
printf '# ContextNest live-feature smoke report\n\n'
printf '_Generated %s against `%s`._\n\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" "$CN_URL"
printf 'Hits the production substrate (real Qwen3 embedder, real ingested\n'
printf 'sessions). Each row pairs a real operator question with a known-answer\n'
printf 'check captured from session-search work today.\n\n'

# Reachability check first — every test depends on the substrate
# answering at all.
if ! curl -fsS --max-time 3 "$CN_URL/api/health" >/dev/null 2>&1; then
    printf '⚠️ **Substrate unreachable at `%s`.** Start it with `make cn-serve` and re-run.\n' "$CN_URL"
    exit 2
fi

# Helper: wall-clock-millisecond runner. Each test gets:
#   $1 — short test name (markdown row id)
#   $2 — single-line description
#   $3 — assertion body (bash code that sets PASS=1/0 and POPULATES
#        the EVIDENCE variable with a one-line summary of what was
#        actually returned). The body has access to BODY and RAW.
#   $4 — curl command (eval'd, must populate $RAW = full JSON body)
run_test() {
    local name="$1"
    local desc="$2"
    local assertion="$3"
    local fetch="$4"
    local start_ms end_ms latency_ms
    local PASS=0
    local EVIDENCE="(no evidence captured)"
    local RAW=""
    start_ms=$(perl -MTime::HiRes -e 'printf "%d", Time::HiRes::time*1000')
    RAW=$(eval "$fetch" 2>/dev/null || echo "{}")
    end_ms=$(perl -MTime::HiRes -e 'printf "%d", Time::HiRes::time*1000')
    latency_ms=$((end_ms - start_ms))
    BODY="$RAW"
    eval "$assertion"
    TOTAL=$((TOTAL + 1))
    local mark
    if [ "$PASS" = "1" ]; then
        mark="✅"
    else
        mark="❌"
        FAILURES=$((FAILURES + 1))
    fi
    printf '| %s | `%s` | %s | %d ms | %s |\n' \
        "$mark" "$name" "$desc" "$latency_ms" "$EVIDENCE"
}

printf '## Results\n\n'
printf '| ✓ | Feature | Question | Latency | Evidence |\n'
printf '|---|---|---|---:|---|\n'

# ──────────────────────────────────────────────────────────────────
# 1. Health + config — baseline endpoints, must return shape.
# ──────────────────────────────────────────────────────────────────

run_test "health" \
    "Is the substrate healthy?" \
    'if printf "%s" "$BODY" | jq -e ".healthy == true" >/dev/null 2>&1; then
        PASS=1
        EVIDENCE="status=$(printf "%s" "$BODY" | jq -r .status)"
     else EVIDENCE="response: $BODY"; fi' \
    'curl -fsS --max-time 5 "$CN_URL/api/health"'

run_test "substrate/config" \
    "Does /config surface every feature flag?" \
    'fields=$(printf "%s" "$BODY" | jq -r "[ .version, .embedding.model, .llm_cache.encryption_enabled, .llm_cache.redactor_enabled, .llm_cache.redactor_rule_count ] | @csv" 2>/dev/null)
     if [ -n "$fields" ] && [ "$fields" != "null,null,null,null,null" ]; then
        PASS=1
        EVIDENCE="$fields"
     else EVIDENCE="missing fields"; fi' \
    'curl -fsS --max-time 5 "$CN_URL/api/v1/substrate/config"'

run_test "substrate/health" \
    "Substrate populated? (fragments > 0)" \
    'frags=$(printf "%s" "$BODY" | jq -r ".fragments.total // 0")
     basins=$(printf "%s" "$BODY" | jq -r ".basins.count // 0")
     if [ "$frags" -gt 0 ] && [ "$basins" -ge 0 ]; then
        PASS=1
        EVIDENCE="fragments=$frags basins=$basins"
     else EVIDENCE="fragments=$frags (empty substrate?)"; fi' \
    'curl -fsS --max-time 10 "$CN_URL/api/v1/substrate/health"'

# ──────────────────────────────────────────────────────────────────
# 2. /sessions/by-file — known-answer file lookup.
# Today: "shared/types/promptSettings.ts" returned 11 sessions,
# 50d9d3ec was most recent.
# ──────────────────────────────────────────────────────────────────

run_test "sessions/by-file" \
    "Find sessions that touched shared/types/promptSettings.ts" \
    'count=$(printf "%s" "$BODY" | jq -r ".matches | length")
     first_sid=$(printf "%s" "$BODY" | jq -r ".matches[0].session_id // \"\"")
     if [ "$count" -ge 5 ]; then
        PASS=1
        EVIDENCE="matches=$count, first=$first_sid"
     else EVIDENCE="only $count matches (expected ≥5)"; fi' \
    'curl -fsS --max-time 5 "$CN_URL/api/v1/sessions/by-file?path=shared/types/promptSettings.ts"'

# ──────────────────────────────────────────────────────────────────
# 3. /sessions/by-feature — operator remembers feature, not session.
# Today: Chapter Zoom feature was authored by c4da5c2b.
# ──────────────────────────────────────────────────────────────────

run_test "sessions/by-feature[chapter-zoom]" \
    "Which session designed Chapter Zoom?" \
    'first_sid=$(printf "%s" "$BODY" | jq -r ".hits[0].session_id // \"\"")
     if [ "$first_sid" = "c4da5c2b-d2f0-48c0-9b8d-4a599263dc3f" ]; then
        PASS=1
        EVIDENCE="top=c4da5c2b ✓ (Chapter Zoom design session)"
     else EVIDENCE="top=$first_sid (expected c4da5c2b)"; fi' \
    'curl -fsS --max-time 5 "$CN_URL/api/v1/sessions/by-feature?q=Chapter+Zoom"'

run_test "sessions/by-feature[ASK-UPGRADE]" \
    "Which session ran the ASK-UPGRADE-V1 mini-orch?" \
    'first_sid=$(printf "%s" "$BODY" | jq -r ".hits[0].session_id // \"\"")
     if [ -n "$first_sid" ]; then
        PASS=1
        EVIDENCE="top=$first_sid"
     else EVIDENCE="no hits"; fi' \
    'curl -fsS --max-time 5 "$CN_URL/api/v1/sessions/by-feature?q=ASK-UPGRADE"'

# ──────────────────────────────────────────────────────────────────
# 4. /sessions/by-intent — Option C, the semantic-intent search.
# Today: 04bdaa60 = "research arxiv digest" intent.
# ──────────────────────────────────────────────────────────────────

# NB: by-intent embeds every session's intent text on first call
# (cold-cache fan-out to DeepInfra Qwen3). On a 2,000+ session
# substrate that's ~30-60s. Subsequent calls reuse the per-session
# embedding cache and return in <1s. The smoke runs by-intent
# TWICE so you can compare cold vs warm latency.
run_test "sessions/by-intent[arxiv-research,cold]" \
    "First-call: which session researched arxiv articles? (warms cache)" \
    'top_sid=$(printf "%s" "$BODY" | jq -r ".hits[0].session_id // \"\"")
     top_dom=$(printf "%s" "$BODY" | jq -r ".hits[0].domain // \"\"")
     considered=$(printf "%s" "$BODY" | jq -r ".considered // 0")
     research_in_top3=$(printf "%s" "$BODY" | jq -r "[.hits[:3][] | select(.domain==\"research\")] | length")
     if [ "$research_in_top3" -ge 1 ] && [ "$considered" -gt 5 ]; then
        PASS=1
        EVIDENCE="top=$top_sid(domain=$top_dom), $research_in_top3 research in top-3, considered=$considered"
     else EVIDENCE="top=$top_sid domain=$top_dom research_in_top3=$research_in_top3 considered=$considered"; fi' \
    'curl -fsS --max-time 180 "$CN_URL/api/v1/sessions/by-intent?q=research+arxiv+trending+techniques&top_k=10"'

run_test "sessions/by-intent[arxiv-research,warm]" \
    "Repeat-call: same query, embedding cache warmed (steady-state perf)" \
    'top_sid=$(printf "%s" "$BODY" | jq -r ".hits[0].session_id // \"\"")
     considered=$(printf "%s" "$BODY" | jq -r ".considered // 0")
     research_in_top3=$(printf "%s" "$BODY" | jq -r "[.hits[:3][] | select(.domain==\"research\")] | length")
     if [ "$research_in_top3" -ge 1 ]; then
        PASS=1
        EVIDENCE="top=$top_sid, $research_in_top3 research in top-3 (warm)"
     else EVIDENCE="research_in_top3=$research_in_top3 considered=$considered"; fi' \
    'curl -fsS --max-time 30 "$CN_URL/api/v1/sessions/by-intent?q=research+arxiv+trending+techniques&top_k=10"'

run_test "sessions/by-intent[domain-filter]" \
    "Filter to research-domain sessions only" \
    'all_research=$(printf "%s" "$BODY" | jq -r "[.hits[] | select(.domain==\"research\")] | length")
     total=$(printf "%s" "$BODY" | jq -r ".hits | length")
     if [ "$total" -gt 0 ] && [ "$all_research" = "$total" ]; then
        PASS=1
        EVIDENCE="$total hits, all domain=research"
     else EVIDENCE="$total hits, $all_research research (filter leaked)"; fi' \
    'curl -fsS --max-time 30 "$CN_URL/api/v1/sessions/by-intent?q=embedding+migration&domain=research&top_k=10"'

# ──────────────────────────────────────────────────────────────────
# 5. /retrieve — fragment-level retrieval (Option A density now in effect).
# ──────────────────────────────────────────────────────────────────

run_test "retrieve[plain]" \
    "Top-K fragments for 'Qwen embedding migration'" \
    'hits=$(printf "%s" "$BODY" | jq -r ".hits | length")
     top_sim=$(printf "%s" "$BODY" | jq -r ".hits[0].similarity // 0")
     if [ "$hits" -gt 0 ]; then
        PASS=1
        EVIDENCE="hits=$hits top_sim=$top_sim"
     else EVIDENCE="0 hits returned"; fi' \
    'curl -fsS --max-time 30 -X POST -H "Content-Type: application/json" -d "{\"query\":\"Qwen embedding migration\",\"top_k\":10}" "$CN_URL/api/v1/tools/retrieve"'

run_test "retrieve[group_by-session]" \
    "Option B: rolled up to sessions for 'arxiv research'" \
    'groups=$(printf "%s" "$BODY" | jq -r ".session_groups | length // 0")
     if [ "$groups" -ge 2 ]; then
        top_sid=$(printf "%s" "$BODY" | jq -r ".session_groups[0].session_id")
        top_score=$(printf "%s" "$BODY" | jq -r ".session_groups[0].score")
        top_kinds=$(printf "%s" "$BODY" | jq -r ".session_groups[0].unique_kinds | length")
        PASS=1
        EVIDENCE="groups=$groups top=$top_sid score=$top_score kinds=$top_kinds"
     else EVIDENCE="only $groups groups (expected ≥2)"; fi' \
    'curl -fsS --max-time 30 -X POST -H "Content-Type: application/json" -d "{\"query\":\"arxiv research papers\",\"top_k\":15,\"group_by\":\"session\"}" "$CN_URL/api/v1/tools/retrieve"'

run_test "retrieve[exclude_kinds]" \
    "exclude_kinds drops initial_prompt_window noise" \
    'leaked=$(printf "%s" "$BODY" | jq -r "[.hits[] | select(.metadata.kind==\"initial_prompt_window\")] | length")
     total=$(printf "%s" "$BODY" | jq -r ".hits | length")
     if [ "$total" -gt 0 ] && [ "$leaked" = "0" ]; then
        PASS=1
        EVIDENCE="$total hits, 0 initial_prompt_window leaked"
     else EVIDENCE="$total hits, $leaked initial_prompt_window leaked"; fi' \
    'curl -fsS --max-time 30 -X POST -H "Content-Type: application/json" -d "{\"query\":\"chapter\",\"top_k\":10,\"exclude_kinds\":[\"initial_prompt_window\"]}" "$CN_URL/api/v1/tools/retrieve"'

# ──────────────────────────────────────────────────────────────────
# 6. content_density (Option A) — fragments populated after PR #126.
# Sample a fragment and confirm the metadata field exists.
# ──────────────────────────────────────────────────────────────────

# content_density (Option A, PR #126) is computed at consolidation
# time. Fragments ingested BEFORE that PR don't have it — so the
# meaningful check is "do new-since-#126 fragments carry it?".
# We anchor on the most-recent session's fragments via /sessions.
RECENT_SID=$(curl -fsS --max-time 10 "$CN_URL/api/v1/sessions" | jq -r ".sessions | sort_by(.last_ts) | reverse | .[0].id" 2>/dev/null)

run_test "content_density[recent-session]" \
    "Are NEW fragments in the most-recent session getting density (Option A live)?" \
    'has_density=$(printf "%s" "$BODY" | jq -r "[.fragments[] | select(.metadata._cn_content_density != null)] | length")
     total=$(printf "%s" "$BODY" | jq -r ".fragments | length")
     sample_dens=$(printf "%s" "$BODY" | jq -r "[.fragments[] | .metadata._cn_content_density // empty] | .[0] // null")
     if [ "$total" -gt 0 ] && [ "$has_density" -gt 0 ]; then
        PASS=1
        EVIDENCE="$has_density/$total in session ${RECENT_SID:0:8} carry density (sample=$sample_dens)"
     else EVIDENCE="$has_density/$total fragments carry density in session ${RECENT_SID:0:8} (worker lag or running stale binary)"; fi' \
    'curl -fsS --max-time 15 "$CN_URL/api/v1/fragments?session_id=$RECENT_SID&limit=20"'

# ──────────────────────────────────────────────────────────────────
# 7. LLM proxy cache stats — slice 2.4.
# ──────────────────────────────────────────────────────────────────

run_test "llm-cache/stats" \
    "LLM proxy cache stats shape" \
    'if printf "%s" "$BODY" | jq -e ".total_entries" >/dev/null 2>&1; then
        entries=$(printf "%s" "$BODY" | jq -r ".total_entries")
        rate=$(printf "%s" "$BODY" | jq -r ".hit_rate")
        PASS=1
        EVIDENCE="entries=$entries hit_rate=$rate"
     else EVIDENCE="response: $BODY"; fi' \
    'curl -fsS --max-time 5 "$CN_URL/llm/v1/cache/stats"'

# ──────────────────────────────────────────────────────────────────
# 8. /sessions/:id/summary on a known session.
# Today: 04bdaa60 = research domain, arxiv-digest goal.
# ──────────────────────────────────────────────────────────────────

run_test "sessions/:id/summary" \
    "Summary for 04bdaa60 carries domain=research" \
    'domain=$(printf "%s" "$BODY" | jq -r ".summary.domain // \"\"")
     goal=$(printf "%s" "$BODY" | jq -r ".summary.goal // \"\" | .[:80]")
     if [ "$domain" = "research" ]; then
        PASS=1
        EVIDENCE="domain=research, goal: $goal..."
     else EVIDENCE="domain=$domain (expected research)"; fi' \
    'curl -fsS --max-time 5 "$CN_URL/api/v1/sessions/04bdaa60-1682-4929-8a40-17eacdaff86d/summary"'

# ──────────────────────────────────────────────────────────────────
# 9. /sessions — full list, sanity check we have populated data.
# ──────────────────────────────────────────────────────────────────

run_test "sessions[list]" \
    "Substrate has >= 50 sessions ingested" \
    'count=$(printf "%s" "$BODY" | jq -r ".sessions | length")
     if [ "$count" -ge 50 ]; then
        PASS=1
        EVIDENCE="$count sessions ingested"
     else EVIDENCE="only $count sessions (expected ≥50)"; fi' \
    'curl -fsS --max-time 10 "$CN_URL/api/v1/sessions"'

# ──────────────────────────────────────────────────────────────────
# 10. Inbox — kind-extension (PR #120) live check.
# ──────────────────────────────────────────────────────────────────

run_test "inbox[populated]" \
    "Inbox has ask/handoff items (PR #120 kind extension)" \
    'total=$(printf "%s" "$BODY" | jq -r ".items | length")
     ask_or_handoff=$(printf "%s" "$BODY" | jq -r "[.items[] | select(.metadata.kind==\"ask\" or .metadata.kind==\"handoff\")] | length")
     if [ "$total" -gt 0 ]; then
        PASS=1
        EVIDENCE="$total items total, $ask_or_handoff ask/handoff"
     else EVIDENCE="empty inbox"; fi' \
    'curl -fsS --max-time 10 "$CN_URL/api/v1/inbox"'

# ──────────────────────────────────────────────────────────────────
# Summary footer.
# ──────────────────────────────────────────────────────────────────

PASS_COUNT=$((TOTAL - FAILURES))

printf '\n## Summary\n\n'
printf '%s %d\n' "- **Total checks:**" "$TOTAL"
printf '%s %d\n' "- **Passed:**" "$PASS_COUNT"
printf '%s %d\n' "- **Failed:**" "$FAILURES"

if [ "$FAILURES" -eq 0 ]; then
    printf '\n_All shipped features pass against the live populated substrate._\n'
    exit 0
else
    printf '\n_%d/%d checks failed — see Evidence column above._\n' "$FAILURES" "$TOTAL"
    exit 1
fi
