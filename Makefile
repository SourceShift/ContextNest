# mini-orch Makefile
#
# All runtime artefacts live under .agentflow/ — this matches how mini-orch
# integrates into host projects (clone into <project>/.agentflow/ → scripts
# walk up to find .agentflow/state.db).

AF := .agentflow

.PHONY: help install setup test test-quick migrate migrate-force clean lint check dev-be dev-fe

help:
	@echo "mini-orch — autonomous LLM code delivery orchestrator"
	@echo
	@echo "Targets:"
	@echo "  install        — mini-orch first-time setup (deps + config templates + state.db)"
	@echo "  test           — full umbrella test suite (~350 tests across 5 suites)"
	@echo "  test-quick     — kickoff-lints + self-healing + agent-comms (fast smoke)"
	@echo "  migrate        — apply any new SQL migrations to state.db"
	@echo "  migrate-force  — drop + recreate state.db from scratch (DESTROYS DATA)"
	@echo "  lint           — syntax-check all .sh files"
	@echo "  check          — verify deps + config templates copied"
	@echo "  clean          — remove node_modules, runs/, *.bak (state.db preserved)"
	@echo
	@echo "ContextNest dev (substrate + dashboard):"
	@echo "  setup          — one-shot install of BE deps + FE deps for ContextNest"
	@echo "  dev-be         — run the substrate with hot-reload on .rs changes (cargo-watch)"
	@echo "  dev-fe         — run the web dashboard (vite) on http://localhost:5057"
	@echo "  cn-*           — see 'make cn-help' for the full ContextNest target list"

install:
	./install.sh

test:
	$(AF)/tests/run-all.sh

test-quick:
	$(AF)/tests/test_kickoff_lints.sh
	$(AF)/tests/test_self_healing.sh
	$(AF)/tests/test_agent_comms.sh

migrate:
	@for m in $(AF)/migrations/*.sql; do \
		echo "applying $$m..."; \
		sqlite3 $(AF)/state.db < "$$m" 2>&1 | head -3 || true; \
	done
	@echo "✓ migrations applied"

migrate-force:
	@printf 'This DESTROYS $(AF)/state.db. Confirm with Y: '; read y; [ "$$y" = Y ] || exit 1
	rm -f $(AF)/state.db $(AF)/state.db-shm $(AF)/state.db-wal
	$(MAKE) migrate

lint:
	@fail=0; for f in $$(find $(AF) -name '*.sh' -not -path '*/node_modules/*'); do \
		bash -n "$$f" 2>&1 || fail=1; \
	done; \
	if [ $$fail -eq 0 ]; then echo "✓ all .sh files syntactically valid"; else exit 1; fi

check:
	@printf '  $(AF)/state.db: '; [ -e $(AF)/state.db ] && echo "✓" || echo "✗ — run 'make migrate'"
	@printf '  $(AF)/config/scope-patterns.yaml: '; [ -e $(AF)/config/scope-patterns.yaml ] && echo "✓" || echo "✗ — run 'make install'"
	@printf '  $(AF)/config/agents.yaml: '; [ -e $(AF)/config/agents.yaml ] && echo "✓" || echo "✗ — run 'make install'"
	@printf '  $(AF)/mini-orch/scripts/cl_kimi.sh: '; [ -e $(AF)/mini-orch/scripts/cl_kimi.sh ] && echo "✓" || echo "✗ — run 'make install' + edit"
	@printf '  $(AF)/llm/node_modules: '; [ -d $(AF)/llm/node_modules ] && echo "✓" || echo "✗ — run 'make install'"

clean:
	rm -rf $(AF)/llm/node_modules $(AF)/llm/dist
	rm -rf $(AF)/runs/
	find $(AF) -name '*.bak' -delete
	find $(AF) -name '*.bak.*' -delete
	@echo "✓ cleaned (state.db preserved — use 'make migrate-force' to nuke)"

# ── File-loss-prevention watchdogs ──────────────────────────────────────
# See docs/file-loss-recovery.md for the failure vectors these address.
# Both safe to run unattended every 15 min.

stash-watchdog:    ## stash-watchdog dry-run — show stale mini-ork stashes
	@bash $(AF)/lib/stash-watchdog.sh

stash-watchdog-pop: ## stash-watchdog --autopop — actually pop stashes
	@bash $(AF)/lib/stash-watchdog.sh --autopop

dispatch-watchdog: ## dispatch-watchdog dry-run — show stale dispatches
	@bash $(AF)/lib/dispatch-watchdog.sh

dispatch-watchdog-close: ## dispatch-watchdog --close --emit-event
	@bash $(AF)/lib/dispatch-watchdog.sh --close --emit-event

watchdogs-install: ## Install both watchdogs as cron jobs (*/15 min)
	@AF_ABS=$$(cd $(AF) && pwd); REPO=$$(git rev-parse --show-toplevel); \
	 WT="*/15 * * * * cd $$REPO && bash $$AF_ABS/lib/stash-watchdog.sh --autopop >> $$AF_ABS/logs/stash-watchdog.log 2>&1"; \
	 DW="*/15 * * * * cd $$REPO && bash $$AF_ABS/lib/dispatch-watchdog.sh --close --emit-event >> $$AF_ABS/logs/dispatch-watchdog.log 2>&1"; \
	 mkdir -p $(AF)/logs; \
	 ( crontab -l 2>/dev/null | grep -vF "$$AF_ABS/lib/stash-watchdog.sh" | grep -vF "$$AF_ABS/lib/dispatch-watchdog.sh"; \
	   echo "$$WT"; echo "$$DW" ) | crontab - && \
	 echo "✓ installed watchdog crons (*/15 min)"

watchdogs-uninstall: ## Remove watchdog cron jobs
	@AF_ABS=$$(cd $(AF) && pwd); \
	 crontab -l 2>/dev/null | grep -vF "$$AF_ABS/lib/stash-watchdog.sh" | grep -vF "$$AF_ABS/lib/dispatch-watchdog.sh" | crontab - && \
	 echo "✓ removed watchdog crons"

watchdogs-status: ## Show installed watchdog cron jobs
	@crontab -l 2>/dev/null | grep -E "stash-watchdog|dispatch-watchdog" || echo "(no watchdog crons — run: make watchdogs-install)"

recover-list: ## mo-recover list — show recoverable files from last 24h
	@$(AF)/bin/mo-recover list

.PHONY: stash-watchdog stash-watchdog-pop dispatch-watchdog \
        dispatch-watchdog-close watchdogs-install watchdogs-uninstall \
        watchdogs-status recover-list

# ╔══════════════════════════════════════════════════════════════════════╗
# ║  ContextNest substrate targets (cn-* prefix)                         ║
# ║                                                                      ║
# ║  These targets manage the Rust substrate (src/, target/release/      ║
# ║  contextnest) — separate from the mini-orch targets above. The cn-   ║
# ║  prefix avoids colliding with mini-orch's `test`, `lint`, `clean`.   ║
# ║                                                                      ║
# ║  Override any variable from the command line:                        ║
# ║    make cn-ingest SINCE=14d PROJECT=researcher                       ║
# ║    make cn-serve  CN_BIND=0.0.0.0:9090                               ║
# ╚══════════════════════════════════════════════════════════════════════╝

CN_BIND       ?= 127.0.0.1:28080
CN_SUBSTRATE  ?= http://$(CN_BIND)
CN_WAL        ?= $(HOME)/.contextnest/wal.jsonl
CN_CHECKPOINT ?= $(CN_WAL:.jsonl=.canonical.sqlite)
CN_BIN        ?= ./target/release/contextnest

# ── Production substrate ────────────────────────────────────────────────
# The operator's live data lives outside the repo. `cn-prod` is `cn-redeploy`
# for that data: preflight, build the current checkout, kill every running
# substrate, start against CN_PROD_DATA, wait for health. The log and pid land
# next to the data so a later session can find what is running and why.
CN_PROD_DATA     ?= /Volumes/docker-ssd/Migration/Development/contextnest-data
CN_PROD_BIND     ?= 127.0.0.1:28080
CN_PROD_WAL      ?= $(CN_PROD_DATA)/wal.jsonl
CN_PROD_ARENA    ?= $(CN_PROD_DATA)/arena
CN_PROD_CHECKPOINT ?= $(CN_PROD_DATA)/wal.canonical.sqlite
CN_PROD_PORT     ?= $(lastword $(subst :, ,$(CN_PROD_BIND)))
CN_PROD_CONFIG   ?= ./config.toml
# Seconds to wait for /api/v1/substrate/health after starting prod. The listener
# binds only after the canonical checkpoint restore finishes, and that phase is
# I/O-bound on the data volume. Measured boots on ~330-380 k fragments:
#   87-109 s   machine idle
#   726 s      345 k, VM + a cargo build sharing the disk
#   824 s      346 k retained of 380 k, VM + `cargo build --release` on the disk
#               (serve-20261003-110233.log: 09:02:34 start → 09:16:18 listening)
#   1560 s     400 k (2026-10-04), 1520 s at 412 k (2026-10-06): I/O-bound in
#               CheckpointStore::restore (sqlite3_step over the 7.8 GB checkpoint)
#               and growing ~12 k fragments/day, so 1800 s no longer leaves headroom.
# A fixed 180 s budget therefore reports "won't start" for a healthy slow start,
# and the contended case is the norm on this host, not the exception. The
# asymmetry decides the default: waiting too long only delays bad news, whereas
# giving up early turns a healthy substrate into a false alarm. The recipe prints
# progress every 10 s, and the failure path names the boot phase it reached.
CN_PROD_HEALTH_TIMEOUT ?= 3600
# Per-probe curl timeout for the health poll. The endpoint is not cheap once the
# substrate is live: it walks the fragment/basin/edge tables, and while the boot
# backlog drains it measured 2.1-5.4 s across six consecutive probes (2 of 6 over
# 3 s). A 3 s budget therefore reported a healthy server as down — keep this well
# above the observed worst case.
CN_PROD_HEALTH_PROBE_TIMEOUT ?= 15
SINCE         ?= 7d
PROJECT       ?=

.PHONY: cn-help cn-build cn-build-fast cn-test cn-lint cn-serve cn-serve-dev cn-run-existing \
        cn-redeploy cn-watch cn-ingest cn-wal-clear cn-curl-health cn-curl-inbox cn-config \
        cn-preflight cn-prod cn-prod-stop cn-prod-status cn-prod-logs cn-prod-build cn-prod-preflight

cn-help:
	@echo "ContextNest substrate targets"
	@echo
	@echo "  make cn-config          — copy config.example.toml → config.toml (once)"
	@echo "  make cn-build           — cargo build --release (produces $(CN_BIN))"
	@echo "  make cn-build-fast      — cargo build --profile fast (faster compile, ~ same runtime)"
	@echo "  make cn-test            — cargo test --tests (full integration suite)"
	@echo "  make cn-lint            — cargo clippy --tests (correctness gate)"
	@echo "  make cn-prod            — KILL every running substrate, build, start on CN_PROD_DATA, wait for health"
	@echo "  make cn-prod-stop       — stop every running contextnest (any checkout, any binary)"
	@echo "  make cn-prod-status     — what is serving prod: pid, rss, checkpoint size, health"
	@echo "  make cn-prod-logs       — tail the newest prod serve log"
	@echo "  make cn-preflight       — print resolved WAL/checkpoint + refuse a pre-v0.2 checkpoint"
	@echo "  make cn-serve           — run the release binary, WAL on, config.toml loaded"
	@echo "  make cn-redeploy        — rebuild release + restart cn-serve (deploys a code change)"
	@echo "  make cn-serve-dev       — cargo run --profile fast (auto-rebuilds, target/fast/)"
	@echo "  make cn-watch           — auto-rebuild + restart on .rs changes (needs cargo-watch)"
	@echo "  make cn-ingest          — backfill Claude Code sessions; vars: SINCE PROJECT"
	@echo "                              e.g. make cn-ingest SINCE=7d PROJECT=researcher"
	@echo "  make cn-curl-health     — substrate health snapshot against the running server"
	@echo "  make cn-curl-inbox      — dump current /api/v1/inbox contents"
	@echo "  make cn-wal-clear       — DELETE $(CN_WAL) (next serve starts fresh)"
	@echo
	@echo "Overridable variables (current defaults shown):"
	@echo "  CN_BIND=$(CN_BIND)"
	@echo "  CN_SUBSTRATE=$(CN_SUBSTRATE)"
	@echo "  CN_WAL=$(CN_WAL)"
	@echo "  CN_CHECKPOINT=$(CN_CHECKPOINT)"
	@echo "  CN_PROD_DATA=$(CN_PROD_DATA)"
	@echo "  CN_PROD_BIND=$(CN_PROD_BIND)"
	@echo "  CN_PROD_CONFIG=$(CN_PROD_CONFIG)   (must be a real config; see cn-prod)"
	@echo "  CN_BIN=$(CN_BIN)"
	@echo "  SINCE=$(SINCE)   PROJECT=$(PROJECT)"
	@echo
	@echo "Secrets: set DEEPINFRA_API_KEY (or OPENAI_API_KEY) in your shell."
	@echo "Mini-orch targets remain available under 'make help'."

cn-config:
	@if [ -f config.toml ]; then \
	  echo "config.toml already exists — not overwriting"; \
	else \
	  cp config.example.toml config.toml && \
	  echo "✓ created config.toml from template (edit to taste; it's git-ignored)"; \
	fi

cn-build:
	cargo build --release

cn-test:
	cargo test --tests

cn-lint:
	cargo clippy --tests -- -A clippy::all -D clippy::correctness

# Refuses to start without an API key in the env, because a silent fall-through
# to the local TF-IDF default is more confusing than a fast failure when the
# operator clearly meant to use a real provider (their config.toml sets one).
# Explicit opt-out for operators running an immutable prebuilt artifact.
cn-run-existing:
	CONTEXTNEST_WAL_PATH=$(CN_WAL) $(CN_BIN) serve --bind $(CN_BIND)

# Print the resolved substrate paths and refuse a pre-v0.2 checkpoint BEFORE
# the build. The binary refuses one too (`CheckpointStore::open`), but by then
# you have waited for a release build to learn it. A pre-v0.2 checkpoint stores
# vectors as inline JSON (~11 KB per 1024-d vector) and boots the old 12 GB
# heap profile — the shape that exhausted the host's compressor on 2026-09-30.
cn-preflight:
	@echo "substrate:  $(CN_SUBSTRATE)"
	@echo "WAL:        $(CN_WAL)"
	@echo "checkpoint: $(CN_CHECKPOINT)"
	@for f in $(CN_WAL) $(CN_CHECKPOINT); do \
	  if [ -f "$$f" ]; then \
	    printf '  %8s  %s\n' "$$(du -h "$$f" | cut -f1)" "$$f"; \
	  else \
	    printf '  %8s  %s\n' "(absent)" "$$f"; \
	  fi; \
	done
	@if [ -f "$(CN_CHECKPOINT)" ] && command -v sqlite3 >/dev/null 2>&1; then \
	  legacy="$$(sqlite3 "$(CN_CHECKPOINT)" "SELECT CASE WHEN EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='objects') AND NOT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='vectors') THEN 1 ELSE 0 END" 2>/dev/null)"; \
	  if [ "$$legacy" = "1" ]; then \
	    echo; \
	    echo "REFUSING: $(CN_CHECKPOINT) is a pre-v0.2 checkpoint (inline JSON vectors)."; \
	    echo "  Compact it into a new file first — the source is opened read-only:"; \
	    echo "    $(CN_BIN) checkpoint compact --from $(CN_CHECKPOINT) --into $(CN_CHECKPOINT).compacted.sqlite"; \
	    echo "  Move the compacted file into place and keep the original as a .bak breadcrumb."; \
	    echo "  Or set CN_WAL to a v0.2 data dir, e.g. CN_WAL=/path/to/v0.2/wal.jsonl."; \
	    echo "  See docs/upgrading/v0.2.0.md. (Override: CONTEXTNEST_ALLOW_LEGACY_CHECKPOINT=1.)"; \
	    exit 1; \
	  fi; \
	fi

cn-serve: cn-preflight cn-build
	@if [ ! -f config.toml ]; then \
	  echo "no config.toml — copying from example"; $(MAKE) cn-config; \
	fi
	@if [ -z "$$DEEPINFRA_API_KEY" ] && [ -z "$$OPENAI_API_KEY" ]; then \
	  echo "warning: neither DEEPINFRA_API_KEY nor OPENAI_API_KEY is set in env."; \
	  echo "         If config.toml points at a remote provider, ingest calls will fail."; \
	fi
	mkdir -p $(dir $(CN_WAL))
	CONTEXTNEST_WAL_PATH=$(CN_WAL) $(CN_BIN) serve --bind $(CN_BIND)

# Deploy a code change to the running substrate: rebuild the release binary,
# stop the instance currently bound to CN_BIND, then start the fresh one.
# The stop step is why this exists as its own target — `cn-serve` alone would
# fail to bind while the old process still holds the port. Foreground, so it
# blocks the terminal like `cn-serve` does.
cn-redeploy: ## Rebuild release + restart cn-serve (stops the running instance first).
	cargo build --release
	@echo "stopping running contextnest on $(CN_BIND) (if any)…"
	-@pkill -f 'contextnest serve' 2>/dev/null; sleep 1
	$(MAKE) cn-serve

# Stop EVERY contextnest process, regardless of which checkout or binary
# started it. Blanket on purpose: the failure this exists to prevent is a
# *stale* instance from another checkout holding the port — on 2026-10-01 one
# started from the pre-disk-first checkout held 12.3 GB of heap (91 M
# allocations) against the legacy checkpoint and put the host's compressor into
# the band that panicked it on 09-30. SIGTERM, wait 10 s, then SIGKILL.
cn-prod-stop:
	@echo "stopping every contextnest process (serve or ingest)…"
	-@pkill -x contextnest 2>/dev/null; \
	  for _ in 1 2 3 4 5 6 7 8 9 10; do \
	    pgrep -x contextnest >/dev/null 2>&1 || break; sleep 1; \
	  done; \
	  if pgrep -x contextnest >/dev/null 2>&1; then \
	    echo "still alive after 10s — SIGKILL"; pkill -9 -x contextnest 2>/dev/null; sleep 1; \
	  fi
	@if pgrep -x contextnest >/dev/null 2>&1; then \
	  echo "ERROR: contextnest refuses to die:"; ps -o pid,etime,command= -p $$(pgrep -x contextnest | tr '\n' ',' | sed 's/,$$//'); \
	  exit 1; \
	fi
	@if lsof -nP -iTCP:$(CN_PROD_PORT) -sTCP:LISTEN >/dev/null 2>&1; then \
	  echo "ERROR: port $(CN_PROD_PORT) is still held by something else:"; \
	  lsof -nP -iTCP:$(CN_PROD_PORT) -sTCP:LISTEN; exit 1; \
	fi
	@echo "✓ nothing listening on $(CN_PROD_PORT)"

# Rewrite the prod checkpoint in (kind,id) order, in place.
#
# Why boot time depends on this: the canonical checkpoint is maintained by
# incremental `INSERT ... ON CONFLICT DO UPDATE` upserts, so over days the
# b-tree pages drift out of physical order. The restore's per-kind scans then
# become random 4 KB reads — measured 52 MB/s against a 464 MB/s sequential
# floor, which is what turns a 30 s boot into an hour. `compact` streams the
# source in key order into a fresh file, so every scan is sequential again.
#
# Cost is proportional to how fragmented the file already is: a deep clean of
# a badly drifted checkpoint is slow (the read is the expensive part), while
# re-running on an already-ordered file is quick. Run it after heavy ingest
# rather than on a schedule.
cn-compact: cn-prod-stop ## Stop the substrate and rewrite CN_PROD_CHECKPOINT in key order.
	@if [ ! -f "$(CN_PROD_CHECKPOINT)" ]; then \
	  echo "ERROR: no checkpoint at $(CN_PROD_CHECKPOINT)"; exit 1; \
	fi
	@if [ ! -x "$(CN_BIN)" ]; then echo "ERROR: $(CN_BIN) missing — run make cn-build"; exit 1; fi
	@STAMP=$$(date +%Y%m%d-%H%M%S); \
	  OUT="$(CN_PROD_CHECKPOINT).compacted.$$STAMP"; \
	  echo "compacting $(CN_PROD_CHECKPOINT) -> $$OUT (this reads the whole file; may take a while)"; \
	  $(CN_BIN) checkpoint compact --from "$(CN_PROD_CHECKPOINT)" --into "$$OUT" || { \
	    echo "ERROR: compact failed; $(CN_PROD_CHECKPOINT) untouched"; rm -f "$$OUT" "$$OUT-wal" "$$OUT-shm"; exit 1; }; \
	  BAK="$(CN_PROD_CHECKPOINT).bak-pre-compact-$$STAMP"; \
	  mv "$(CN_PROD_CHECKPOINT)" "$$BAK"; \
	  mv "$$OUT" "$(CN_PROD_CHECKPOINT)"; \
	  rm -f "$$OUT-wal" "$$OUT-shm"; \
	  echo "✓ compacted"; \
	  ls -lh "$$BAK" "$(CN_PROD_CHECKPOINT)"; \
	  echo "  source kept as $$BAK — delete it once the next boot looks healthy."

# Preflight the PROD paths, not the ~/.contextnest defaults.
cn-prod-preflight:
	@$(MAKE) --no-print-directory cn-preflight \
	  CN_WAL=$(CN_PROD_WAL) CN_CHECKPOINT=$(CN_PROD_CHECKPOINT)

cn-prod-build:
	@if [ -n "$(CN_PROD_SKIP_BUILD)" ]; then \
	  echo "CN_PROD_SKIP_BUILD set — running $(CN_BIN) as-is"; \
	else \
	  $(MAKE) cn-build; \
	fi

# Build first, kill second, start third — that ordering keeps the downtime to
# the boot (WAL replay + checkpoint restore, ~100 s on 330 k fragments) rather
# than the release build.
cn-prod: cn-prod-preflight cn-prod-build cn-prod-stop ## Kill any running substrate, build, start against CN_PROD_DATA, wait for health.
	@if [ ! -d "$(CN_PROD_DATA)" ]; then \
	  echo "ERROR: missing $(CN_PROD_DATA) — is docker-ssd mounted?"; exit 1; \
	fi
	@if [ ! -f "$(CN_PROD_WAL)" ]; then echo "ERROR: missing $(CN_PROD_WAL)"; exit 1; fi
	@if [ ! -x "$(CN_BIN)" ]; then echo "ERROR: missing $(CN_BIN) — run make cn-build"; exit 1; fi
	@if [ ! -f "$(CN_PROD_CONFIG)" ]; then \
	  echo "ERROR: no config at $(CN_PROD_CONFIG)."; \
	  echo "  The embedding provider in that file must match the one that wrote the"; \
	  echo "  checkpoint, or the substrate re-embeds all ~330 k fragments at boot."; \
	  echo "  Do NOT let make cn-config generate one for prod. Point at an existing"; \
	  echo "  config instead, e.g. CN_PROD_CONFIG=/path/to/your/config.toml."; \
	  exit 1; \
	fi
	@if [ -z "$$DEEPINFRA_API_KEY" ] && [ -z "$$OPENAI_API_KEY" ]; then \
	  echo "warning: neither DEEPINFRA_API_KEY nor OPENAI_API_KEY is set;"; \
	  echo "         embedding calls will fail (ingest, consolidation)."; \
	fi
	@mkdir -p $(CN_PROD_ARENA)
	@LOG=$(CN_PROD_DATA)/serve-$$(date +%Y%m%d-%H%M%S).log; \
	  echo "starting $(CN_BIN) on $(CN_PROD_BIND)"; \
	  echo "  data: $(CN_PROD_DATA)"; \
	  echo "  log:  $$LOG"; \
	  CONTEXTNEST_CONFIG=$(CN_PROD_CONFIG) \
	  CONTEXTNEST_WAL_PATH=$(CN_PROD_WAL) \
	  CONTEXTNEST_VECTOR_ARENA_DIR=$(CN_PROD_ARENA) \
	  CONTEXTNEST_CONSOLIDATION_CONCURRENCY=2 \
	  CONTEXTNEST_CPU_WORKERS=2 \
	  TOKIO_WORKER_THREADS=4 \
	  nohup nice -n 10 $(CN_BIN) serve --bind $(CN_PROD_BIND) > "$$LOG" 2>&1 & \
	  echo $$! > $(CN_PROD_DATA)/serve.pid; \
	  echo "  pid:  $$(cat $(CN_PROD_DATA)/serve.pid)"
	@echo "waiting for /api/v1/substrate/health (budget $(CN_PROD_HEALTH_TIMEOUT)s, override with CN_PROD_HEALTH_TIMEOUT)…"
	@LOG=$$(ls -t $(CN_PROD_DATA)/serve-*.log 2>/dev/null | head -1); \
	for i in $$(seq 1 $$(( $(CN_PROD_HEALTH_TIMEOUT) / 2 ))); do \
	  if curl -sf -m $(CN_PROD_HEALTH_PROBE_TIMEOUT) http://$(CN_PROD_BIND)/api/v1/substrate/health >/dev/null 2>&1; then \
	    echo "✓ healthy after ~$$((i * 2))s"; exit 0; \
	  fi; \
	  if [ $$((i % 5)) -eq 0 ]; then echo "  … still booting ($$((i * 2))s)"; fi; \
	  sleep 2; \
	done; \
	echo "ERROR: not healthy after $(CN_PROD_HEALTH_TIMEOUT)s"; \
	echo "--- boot phases so far ($$LOG) ---"; \
	grep -a -E 'Starting ContextNest|WAL replay|vector arena|checkpoint restore|restored canonical|server listening' "$$LOG" | tail -12; \
	echo "--- if the last phase is 'checkpoint restore' the boot is merely slow:"; \
	echo "    re-run with a larger CN_PROD_HEALTH_TIMEOUT, do not treat it as a crash."; \
	exit 1
	@curl -s -m $(CN_PROD_HEALTH_PROBE_TIMEOUT) http://$(CN_PROD_BIND)/api/v1/substrate/health \
	  | jq '{fragments: .fragments.total, basins: .basins.count, edges: .connections.edges}'
	@echo "the process is detached; 'make cn-prod-stop' stops it, 'make cn-prod-logs' follows it"

cn-prod-status: ## Show what is serving prod: pid, footprint, health.
	@if pgrep -x contextnest >/dev/null 2>&1; then \
	  for pid in $$(pgrep -x contextnest); do \
	    printf 'pid %s  up %s  rss %s MB\n' "$$pid" \
	      "$$(ps -o etime= -p $$pid | tr -d ' ')" \
	      "$$(( $$(ps -o rss= -p $$pid | tr -d ' ') / 1024 ))"; \
	    printf '  cwd:  %s   (this is where ./config.toml came from)\n' \
	      "$$(lsof -a -p $$pid -d cwd -Fn 2>/dev/null | grep '^n' | cut -c2-)"; \
	  done; \
	else \
	  echo "no contextnest running"; \
	fi
	@if [ -f "$(CN_PROD_DATA)/serve.pid" ]; then echo "serve.pid: $$(cat $(CN_PROD_DATA)/serve.pid)"; fi
	@echo "checkpoint: $$(du -h $(CN_PROD_CHECKPOINT) 2>/dev/null | cut -f1)"
	@curl -s -m $(CN_PROD_HEALTH_PROBE_TIMEOUT) http://$(CN_PROD_BIND)/api/v1/substrate/health \
	  | jq '{fragments: .fragments.total, basins: .basins.count, edges: .connections.edges}' \
	  || echo "(health endpoint not answering)"

cn-prod-logs: ## Follow the newest prod serve log.
	@tail -f "$$(ls -t $(CN_PROD_DATA)/serve-*.log | head -1)"

cn-ingest: $(CN_BIN)
	$(CN_BIN) ingest claude-code \
	  --substrate $(CN_SUBSTRATE) \
	  --since $(SINCE) \
	  $(if $(PROJECT),--project $(PROJECT),)

# ── Dev-loop helpers ────────────────────────────────────────────────────
# The `fast` profile (defined in Cargo.toml) builds in ~60s clean / ~10s
# incremental and produces an optimized-enough binary at target/fast/.
# Use these for the edit-restart loop; reserve `cn-serve` for benchmarks
# or production-shaped runs.

CN_BIN_FAST   ?= ./target/fast/contextnest

cn-build-fast: ## Build the fast-profile binary (cargo build --profile fast)
	cargo build --profile fast

cn-serve-dev: cn-preflight ## Run the fast-profile binary, WAL on. Auto-rebuilds on each invocation.
	@if [ ! -f config.toml ]; then $(MAKE) cn-config; fi
	@mkdir -p $(dir $(CN_WAL))
	cargo run --profile fast --bin contextnest -- serve --bind $(CN_BIND)

cn-watch: ## Auto-rebuild + restart on .rs file changes (requires cargo-watch). Ctrl-C exits both.
	@if ! command -v cargo-watch >/dev/null 2>&1; then \
	  echo "cargo-watch not installed — run: cargo install cargo-watch"; exit 1; \
	fi
	@if [ ! -f config.toml ]; then $(MAKE) cn-config; fi
	@mkdir -p $(dir $(CN_WAL))
	CONTEXTNEST_WAL_PATH=$(CN_WAL) cargo watch \
	  --watch src --watch Cargo.toml \
	  -x 'run --profile fast --bin contextnest -- serve --bind $(CN_BIND)'

cn-curl-health:
	@./scripts/operator-curl.sh -fsS $(CN_SUBSTRATE)/api/v1/substrate/health | head -c 1000; echo

cn-curl-inbox:
	@./scripts/operator-curl.sh -fsS $(CN_SUBSTRATE)/api/v1/inbox | head -c 1000; echo

cn-wal-clear:
	@printf 'DELETE $(CN_WAL)? This wipes substrate persistence. Confirm with y/Y: '; \
	read ans; case "$$ans" in [Yy]|[Yy][Ee][Ss]) ;; *) echo "aborted"; exit 1;; esac; \
	rm -f $(CN_WAL); echo "✓ removed $(CN_WAL)"

# Marker target — re-runs cn-build if the binary is missing.
$(CN_BIN):
	$(MAKE) cn-build

# ─────────────────────────────────────────────────────────────────────────────
# Top-level dev convenience targets
#
# Thin wrappers around the cn-* family so first-time contributors can do
# `make setup && make dev-be` (or `make dev-fe`) without learning the full
# cn-* matrix. Intentionally short names; the cn-* targets remain the
# authoritative recipes.
# ─────────────────────────────────────────────────────────────────────────────

setup: ## One-shot setup: ContextNest BE deps + FE deps. Idempotent.
	@echo "→ ContextNest backend setup"
	@if [ -f ./install.sh ]; then ./install.sh; else \
	  echo "no install.sh — ensure cargo + rustup are available, then run 'make cn-build'"; \
	fi
	@echo "→ ContextNest frontend deps (pnpm install in web/)"
	@if [ -d web ]; then \
	  (cd web && pnpm install); \
	else \
	  echo "no web/ dir — skipping FE setup"; \
	fi
	@echo "✓ setup complete — try 'make dev-be' in one terminal and 'make dev-fe' in another"

dev-be: ## Run the substrate backend with auto-rebuild on .rs changes. Wraps cn-watch.
	$(MAKE) cn-watch

dev-fe: ## Run the web dashboard (vite). Hot-reload via Vite HMR.
	@if [ ! -d web/node_modules ]; then \
	  echo "web/node_modules missing — running pnpm install first"; \
	  (cd web && pnpm install); \
	fi
	@cd web && pnpm dev

.PHONY: cn-tenant-config
cn-tenant-config: ## Write private server + application-tenant configuration files, without restarting the service.
	python3 scripts/configure_tenants.py
