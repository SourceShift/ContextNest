#!/usr/bin/env bash
# diagnose-hot-be.sh — capture a CPU profile of the ContextNest BE when it's
# spinning at high CPU.
#
# Origin: 2026-05-26 — after observing PID 93445 stuck at 99% CPU for 60+
# minutes, with global `/api/v1/tools/retrieve` silently returning empty.
# Restarting the BE fixed the bug, but we never captured what was looping.
# Next occurrence — run this script and the sample lands in /tmp/.
#
# Modes:
#   ./diagnose-hot-be.sh           # one-shot: sample NOW, regardless of CPU
#   ./diagnose-hot-be.sh --watch   # watch loop: sample only when CPU > THRESHOLD
#                                  # sustains for 3 consecutive 1-sec ticks
#
# Outputs:
#   /tmp/cn-be-hot-<ISO>.sample    # raw sample(1) output
#   stdout                          # top thread summary (hot frames first)
#
# Requires: macOS (sample is /usr/bin/sample). On Linux use perf or py-spy.

set -euo pipefail

THRESHOLD="${CN_HOT_CPU_THRESHOLD:-50}"   # %CPU above which we consider it "hot"
SAMPLE_DURATION="${CN_HOT_SAMPLE_SECS:-10}"
MODE="${1:-once}"

find_pid() {
  pgrep -f 'contextnest serve' | head -1
}

cpu_pct() {
  # ps emits a single number column; strip whitespace.
  ps -p "$1" -o %cpu= | tr -d '[:space:]'
}

capture() {
  local pid="$1"
  local ts
  ts="$(date +%Y%m%dT%H%M%S)"
  local out="/tmp/cn-be-hot-${ts}.sample"
  echo "→ sampling pid=$pid for ${SAMPLE_DURATION}s → $out"
  /usr/bin/sample "$pid" "$SAMPLE_DURATION" -mayDie -file "$out" >/dev/null 2>&1 || true

  echo
  echo "═══ top thread states (filter out idle parkers) ═══"
  # Grep all "+ NNNN something" frame headers, drop the always-parked
  # _pthread_cond_wait / __psynch_cvwait noise, show the most-sampled
  # frames per thread.
  grep -E "^\s*\+?\s*[0-9]+\s+(contextnest|ContextNest|MemoryAttractor|session_index|basin|consolidat|WAL|Wal|fragment|extractor|tokio)" "$out" \
    | grep -vE "_pthread_cond_wait|__psynch_cvwait|park_internal|Parker::park" \
    | sed 's/^[[:space:]]*//' \
    | sort \
    | uniq -c \
    | sort -rn \
    | head -25
  echo
  echo "═══ thread roster (first sample under each thread head) ═══"
  grep -E "^[[:space:]]*[0-9]+ Thread_" "$out" | head -20
  echo
  echo "Full sample saved to: $out"
  echo "Suggested follow-ups:"
  echo "  open $out                      # full text in editor"
  echo "  grep -c basin_aware_expand $out   # was basin recomputation looping?"
  echo "  grep -c consolidat $out           # was the consolidation worker?"
  echo "  grep -c 'add_node\\|edge'     $out  # ConnectionNetwork graph mutation?"
}

watch_loop() {
  local pid
  pid="$(find_pid)"
  if [ -z "$pid" ]; then
    echo "no contextnest serve process found" >&2
    exit 1
  fi
  echo "watching pid=$pid for sustained %cpu > ${THRESHOLD}…  (Ctrl-C to stop)"
  local hot_count=0
  while true; do
    local c
    c="$(cpu_pct "$pid")"
    # Strip decimal — integer compare is enough for "is it hot".
    local c_int="${c%.*}"
    printf '\r%s  pid=%s  cpu=%s%%  hot_streak=%d  ' \
      "$(date +%H:%M:%S)" "$pid" "$c" "$hot_count"
    if [ "$c_int" -gt "$THRESHOLD" ]; then
      hot_count=$((hot_count + 1))
      if [ "$hot_count" -ge 3 ]; then
        echo
        echo "*** CPU spike confirmed (${c}% for 3+s) — capturing sample ***"
        capture "$pid"
        echo "*** sample done, resetting hot streak ***"
        hot_count=0
      fi
    else
      hot_count=0
    fi
    sleep 1
  done
}

case "$MODE" in
  --watch)
    watch_loop
    ;;
  once|*)
    pid="$(find_pid)"
    if [ -z "$pid" ]; then
      echo "no contextnest serve process found" >&2
      exit 1
    fi
    cur="$(cpu_pct "$pid")"
    echo "pid=$pid  current cpu=$cur%  (one-shot sample, regardless of cpu)"
    capture "$pid"
    ;;
esac
