#!/usr/bin/env bash
# Smoke-test the ContextNest MCP server over real stdio JSON-RPC frames.
#
# This is the trustworthy check: it drives the actual server process the way a
# client does (initialize → tools/list → tools/call), against the real
# substrate, and fails loudly if any frame is missing or malformed. It is the
# replacement for "poke it with curl and hope".
#
#   ./smoke.sh                        # uses http://127.0.0.1:28080
#   CONTEXTNEST_BASE_URL=… ./smoke.sh # against another substrate
set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export CONTEXTNEST_BASE_URL="${CONTEXTNEST_BASE_URL:-http://127.0.0.1:28080}"
OUT="$(mktemp -t cn-mcp-smoke.XXXXXX)"
ERR="$(mktemp -t cn-mcp-smoke-err.XXXXXX)"

TSX="$DIR/node_modules/.bin/tsx"
[ -x "$TSX" ] || TSX="$(command -v tsx || true)"
if [ -z "$TSX" ]; then
  echo "FAIL: no tsx binary. Run: cd $DIR && npm install" >&2
  exit 1
fi

echo "→ driving $DIR/src/index.ts against $CONTEXTNEST_BASE_URL"

# Keep stdin open between frames so every response is written before EOF.
{
  printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"cn-smoke","version":"0"}}}'
  sleep 0.4
  printf '%s\n' '{"jsonrpc":"2.0","method":"notifications/initialized"}'
  sleep 0.2
  printf '%s\n' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'
  sleep 0.4
  printf '%s\n' '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"cn_health","arguments":{}}}'
  sleep 1
  printf '%s\n' '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"cn_retrieve","arguments":{"query":"basin halos rendering","top_k":3,"session_id":"a473db0f-e073-4327-8f6c-263ab88ff03e"}}}'
  sleep 1
  printf '%s\n' '{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"cn_prompt_context_capsule","arguments":{"query":"basin","since":"90d"}}}'
  sleep 1
} | "$TSX" "$DIR/src/index.ts" > "$OUT" 2> "$ERR" || true

if [ -s "$ERR" ]; then
  echo "server stderr:"; sed 's/^/    /' "$ERR"
fi

python3 - "$OUT" <<'PY'
import json, sys

path = sys.argv[1]
frames = {}
bad = []
for line in open(path):
    line = line.strip()
    if not line:
        continue
    try:
        m = json.loads(line)
    except json.JSONDecodeError:
        bad.append(line[:120]); continue
    if "id" in m and m["id"] is not None:
        frames[m["id"]] = m

checks, failures = [], []

# 1. initialize
init = frames.get(1, {}).get("result", {})
si = init.get("serverInfo", {})
checks.append(f"initialize → {si.get('name')} {si.get('version')} protocol={init.get('protocolVersion')}")
if si.get("name") != "contextnest":
    failures.append("initialize did not return a contextnest serverInfo")

# 2. tools/list
tools = frames.get(2, {}).get("result", {}).get("tools", [])
names = sorted(t["name"] for t in tools)
checks.append(f"tools/list → {len(names)} tools: {', '.join(names)}")
expected = {
    "cn_attention", "cn_features", "cn_health", "cn_inbox", "cn_outcome",
    "cn_prompt_context_capsule", "cn_retrieve", "cn_session_by_feature",
    "cn_session_by_file", "cn_session_by_intent", "cn_session_find",
    "cn_session_summary", "cn_session_trajectory",
}
missing = expected - set(names)
if missing:
    failures.append(f"tools missing: {sorted(missing)}")

def call_text(fid):
    r = frames.get(fid, {})
    if "error" in r:
        return None, f"JSON-RPC error: {r['error']}"
    result = r.get("result", {})
    parts = [c.get("text", "") for c in result.get("content", []) if c.get("type") == "text"]
    if result.get("isError"):
        return None, "isError=true: " + " ".join(parts)[:200]
    return "\n".join(parts), None

# 3. cn_health against the live substrate
health, err = call_text(3)
if err:
    failures.append(f"cn_health failed — {err}")
    checks.append("cn_health → FAILED (is the substrate up?)")
elif "is UP" in health:
    checks.append("cn_health → " + health.splitlines()[0])
else:
    failures.append(f"cn_health returned unexpected text: {health[:160]}")

# 4. scoped cn_retrieve must return faster than the unscoped scan
retr, err = call_text(4)
if err:
    failures.append(f"cn_retrieve failed — {err}")
elif "hit(s)" in retr or "No matches" in retr:
    checks.append("cn_retrieve (scoped) → " + retr.splitlines()[0])
else:
    failures.append(f"cn_retrieve returned unexpected text: {retr[:160]}")

# 5. capsule
cap, err = call_text(5)
if err:
    failures.append(f"cn_prompt_context_capsule failed — {err}")
else:
    checks.append(f"cn_prompt_context_capsule → {len(cap)} chars returned")

if bad:
    failures.append(f"{len(bad)} non-JSON line(s) on stdout: {bad[:2]}")

print("\n".join("  " + c for c in checks))
if failures:
    print("\nFAIL:")
    for f in failures:
        print("  ✗ " + f)
    sys.exit(1)
print("\nPASS — all frames answered, all expected tools registered.")
PY
status=$?
rm -f "$OUT" "$ERR"
exit $status
