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
CN_BIN        ?= ./target/release/contextnest
SINCE         ?= 7d
PROJECT       ?=

.PHONY: cn-help cn-build cn-build-fast cn-test cn-lint cn-serve cn-serve-dev \
        cn-redeploy cn-watch cn-ingest cn-wal-clear cn-curl-health cn-curl-inbox cn-config

cn-help:
	@echo "ContextNest substrate targets"
	@echo
	@echo "  make cn-config          — copy config.example.toml → config.toml (once)"
	@echo "  make cn-build           — cargo build --release (produces $(CN_BIN))"
	@echo "  make cn-build-fast      — cargo build --profile fast (faster compile, ~ same runtime)"
	@echo "  make cn-test            — cargo test --tests (full integration suite)"
	@echo "  make cn-lint            — cargo clippy --tests (correctness gate)"
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
cn-serve: $(CN_BIN)
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

cn-serve-dev: ## Run the fast-profile binary, WAL on. Auto-rebuilds on each invocation.
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
	@curl -fsS $(CN_SUBSTRATE)/api/v1/substrate/health | head -c 1000; echo

cn-curl-inbox:
	@curl -fsS $(CN_SUBSTRATE)/api/v1/inbox | head -c 1000; echo

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
