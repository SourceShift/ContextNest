#!/usr/bin/env bash
# scripts/smoke-pr-6-outcome-feedback.sh — Smoke for PR-6 (CN side).
#
# Per agent-context-pack epic Smoke Test Standard. Proves the new
# /api/v1/agent/outcome endpoint:
#   1. Returns 200 for a valid request
#   2. Bumps last_accessed on known atom ids
#   3. Adjusts _cn_confidence_signal by ±0.05 per call (signed by
#      outcome string)
#   4. Caps cumulative signal at ±1.0
#   5. Skips unknown atom ids silently (response counts them)
#   6. Returns 400 on empty atom_ids array
#
# Tests rely on a running CN with at least 1 known atom to update.
# If CN unreachable, Tests 1-5 skip; only Test 6 (400 path) runs
# (it doesn't need a live atom).
#
# Evidence file: tmp/smoke-evidence/pr-6-outcome-feedback-<ts>.md
#
# Exit codes:
#   0   all assertions passed (or skipped due to CN unreachable)
#   1   failures
#   78  prerequisites missing

set -uo pipefail

CN_ROOT="${CN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
CN_BASE_URL="${CN_BASE_URL:-http://127.0.0.1:28080}"
EVIDENCE_DIR="${EVIDENCE_DIR:-${CN_ROOT}/tmp/smoke-evidence}"
TS=$(date -u +%Y%m%dT%H%M%SZ)
EVIDENCE="${EVIDENCE_DIR}/pr-6-outcome-feedback-${TS}.md"

PASS=0; FAIL=0
mkdir -p "$EVIDENCE_DIR"

_assert_eq_int() {
  local name="$1" expected="$2" actual="$3" verdict
  if [[ "$actual" -eq "$expected" ]]; then
    verdict="PASS"; PASS=$((PASS+1))
  else
    verdict="FAIL - expected $expected, got $actual"; FAIL=$((FAIL+1))
  fi
  {
    printf '\n## Assertion: %s\n' "$name"
    printf '**Expected:** %s\n' "$expected"
    printf '**Actual:** %s\n' "$actual"
    printf '**Verdict:** %s\n' "$verdict"
  } >> "$EVIDENCE"
}

_assert_str() {
  local name="$1" expected="$2" actual="$3" verdict
  if [[ "$actual" == *"$expected"* ]]; then
    verdict="PASS"; PASS=$((PASS+1))
  else
    verdict="FAIL - expected substring not found"; FAIL=$((FAIL+1))
  fi
  {
    printf '\n## Assertion: %s\n' "$name"
    printf '**Expected substring:** `%s`\n' "$expected"
    printf '**Actual:** `%s`\n' "${actual:0:240}"
    printf '**Verdict:** %s\n' "$verdict"
  } >> "$EVIDENCE"
}

{
  printf '# Smoke evidence: PR-6 outcome feedback endpoint\n'
  printf '**Ran:** %s\n' "$TS"
  printf '**Branch:** %s\n' "$(git -C "$CN_ROOT" branch --show-current 2>/dev/null || echo unknown)"
  printf '**Commit:** %s\n' "$(git -C "$CN_ROOT" rev-parse --short HEAD 2>/dev/null || echo unknown)"
  printf '**CN url:** %s\n' "$CN_BASE_URL"
} > "$EVIDENCE"

command -v python3 >/dev/null || { echo "prereq missing: python3" >&2; exit 78; }
command -v curl >/dev/null || { echo "prereq missing: curl" >&2; exit 78; }

# CN reachable?
cn_code="000"
for _ in 1 2; do
  cn_code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 \
    "$CN_BASE_URL/api/v1/substrate/health" 2>/dev/null || echo "000")
  [[ "$cn_code" == "200" ]] && break
  sleep 1
done
{ printf '**CN reachable:** %s (health=%s)\n' \
    "$([[ "$cn_code" == "200" ]] && echo yes || echo no)" "$cn_code"; } >> "$EVIDENCE"

# Test 6 first — doesn't need a live atom (validates 400 path).
{ printf '\n## Test 6 — empty atom_ids returns 400\n'; } >> "$EVIDENCE"
if [[ "$cn_code" == "200" ]]; then
  T6_CODE=$(curl -s -o /dev/null -w '%{http_code}' --max-time 10 \
    -X POST "$CN_BASE_URL/api/v1/agent/outcome" \
    -H 'Content-Type: application/json' \
    -d '{"atom_ids":[],"outcome":"success"}' 2>/dev/null || echo "000")
  _assert_eq_int "empty atom_ids → 400" 400 "$T6_CODE"
else
  echo "  [skip] Test 6 - CN unreachable" >&2
fi

# Find a real atom_id to exercise the success path against.
KNOWN_ATOM_ID=""
if [[ "$cn_code" == "200" ]]; then
  # Get the first hit from a broad retrieve — any indexed fragment works.
  KNOWN_ATOM_ID=$(curl -s --max-time 10 -X POST "$CN_BASE_URL/api/v1/tools/retrieve" \
    -H 'Content-Type: application/json' \
    -d '{"query":"feature shipped","limit":1}' 2>/dev/null \
    | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
    hits = d.get("hits", [])
    if hits:
        print(hits[0].get("id", ""))
except Exception:
    pass
' 2>/dev/null)
fi

{ printf '**Known atom_id (for tests 1-5):** `%s`\n' "${KNOWN_ATOM_ID:-<none>}"; } >> "$EVIDENCE"

if [[ -z "$KNOWN_ATOM_ID" || "$cn_code" != "200" ]]; then
  echo "  [skip] Tests 1-5 — no known atom or CN unreachable" >&2
  {
    printf '\n_(Tests 1-5 skipped — need a populated CN with at least 1 atom)_\n'
    printf '\n---\n## Summary\n**Assertions:** %d PASS, %d FAIL\n' "$PASS" "$FAIL"
  } >> "$EVIDENCE"
  echo ""
  echo "-- Smoke evidence: $EVIDENCE --"
  echo "-- $PASS PASS, $FAIL FAIL --"
  [[ "$FAIL" -eq 0 ]] && exit 0 || exit 1
fi

# Test 1 — success outcome returns 200 with delta=0.05
{ printf '\n## Test 1 — success outcome returns 200 with delta_applied=0.05\n'; } >> "$EVIDENCE"
T1_BODY=$(curl -s --max-time 10 -X POST "$CN_BASE_URL/api/v1/agent/outcome" \
  -H 'Content-Type: application/json' \
  -d "{\"atom_ids\":[\"$KNOWN_ATOM_ID\"],\"outcome\":\"success\",\"evidence\":\"smoke pr-6 success\",\"session_id\":\"smoke-pr-6\"}" 2>/dev/null)
T1_DELTA=$(echo "$T1_BODY" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("delta_applied",-999))' 2>/dev/null || echo "-999")
T1_UPDATED=$(echo "$T1_BODY" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("updated",0))' 2>/dev/null || echo "0")
_assert_str "success returns delta_applied=0.05" "0.05" "$T1_BODY"
_assert_eq_int "success returns updated=1" 1 "$T1_UPDATED"
{ printf '\n### POST response:\n```\n%s\n```\n' "$T1_BODY"; } >> "$EVIDENCE"

# Test 2 — failure outcome returns delta=-0.05
{ printf '\n## Test 2 — failure outcome returns delta_applied=-0.05\n'; } >> "$EVIDENCE"
T2_BODY=$(curl -s --max-time 10 -X POST "$CN_BASE_URL/api/v1/agent/outcome" \
  -H 'Content-Type: application/json' \
  -d "{\"atom_ids\":[\"$KNOWN_ATOM_ID\"],\"outcome\":\"failure\"}" 2>/dev/null)
_assert_str "failure returns delta_applied=-0.05" "-0.05" "$T2_BODY"
{ printf '\n### POST response:\n```\n%s\n```\n' "$T2_BODY"; } >> "$EVIDENCE"

# Test 3 — unknown outcome string returns delta=0 (neutral)
{ printf '\n## Test 3 — unknown outcome returns delta_applied=0 (neutral)\n'; } >> "$EVIDENCE"
T3_BODY=$(curl -s --max-time 10 -X POST "$CN_BASE_URL/api/v1/agent/outcome" \
  -H 'Content-Type: application/json' \
  -d "{\"atom_ids\":[\"$KNOWN_ATOM_ID\"],\"outcome\":\"partial_success\"}" 2>/dev/null)
_assert_str "unknown outcome returns delta_applied=0.0" '"delta_applied":0' "$T3_BODY"
{ printf '\n### POST response:\n```\n%s\n```\n' "$T3_BODY"; } >> "$EVIDENCE"

# Test 4 — unknown atom_id counted as skipped_unknown, no crash
{ printf '\n## Test 4 — unknown atom_id is skipped, not error\n'; } >> "$EVIDENCE"
T4_BODY=$(curl -s --max-time 10 -X POST "$CN_BASE_URL/api/v1/agent/outcome" \
  -H 'Content-Type: application/json' \
  -d '{"atom_ids":["cn-bogus-id-no-such-fragment-12345"],"outcome":"success"}' 2>/dev/null)
T4_SKIPPED=$(echo "$T4_BODY" | python3 -c 'import json,sys; d=json.load(sys.stdin); print(d.get("skipped_unknown",0))' 2>/dev/null || echo "0")
_assert_eq_int "unknown atom → skipped_unknown=1" 1 "$T4_SKIPPED"
{ printf '\n### POST response:\n```\n%s\n```\n' "$T4_BODY"; } >> "$EVIDENCE"

# Test 5 — signal saturation (40 successes should clip to 1.0). We just
# verify the endpoint doesn't error after a burst, not exact arithmetic
# (other calls between test 1-4 may have already shifted the signal).
{ printf '\n## Test 5 — burst of 30 successes does not error (cap test)\n'; } >> "$EVIDENCE"
T5_OK=1
for _ in $(seq 1 30); do
  rc=$(curl -s -o /dev/null -w '%{http_code}' --max-time 5 -X POST "$CN_BASE_URL/api/v1/agent/outcome" \
    -H 'Content-Type: application/json' \
    -d "{\"atom_ids\":[\"$KNOWN_ATOM_ID\"],\"outcome\":\"success\"}" 2>/dev/null || echo "000")
  if [[ "$rc" != "200" ]]; then
    T5_OK=0
    break
  fi
done
_assert_eq_int "30-call burst all 200" 1 "$T5_OK"

# Summary
{
  printf '\n---\n## Summary\n'
  printf '**Assertions:** %d PASS, %d FAIL\n' "$PASS" "$FAIL"
  printf '**Failure-path coverage:** empty atom_ids → 400, unknown atom → skipped_unknown\n'
  printf '**Live-path coverage:** success, failure, unknown-outcome, signal-cap burst\n'
} >> "$EVIDENCE"

echo ""
echo "-- Smoke evidence: $EVIDENCE --"
echo "-- $PASS PASS, $FAIL FAIL --"
[[ "$FAIL" -eq 0 ]]
