# mini-orch Makefile
#
# All runtime artefacts live under .agentflow/ — this matches how mini-orch
# integrates into host projects (clone into <project>/.agentflow/ → scripts
# walk up to find .agentflow/state.db).

AF := .agentflow

.PHONY: help install test test-quick migrate migrate-force clean lint check

help:
	@echo "mini-orch — autonomous LLM code delivery orchestrator"
	@echo
	@echo "Targets:"
	@echo "  install        — first-time setup (deps + config templates + npm install + state.db init)"
	@echo "  test           — full umbrella test suite (~350 tests across 5 suites)"
	@echo "  test-quick     — kickoff-lints + self-healing + agent-comms (fast smoke)"
	@echo "  migrate        — apply any new SQL migrations to state.db"
	@echo "  migrate-force  — drop + recreate state.db from scratch (DESTROYS DATA)"
	@echo "  lint           — syntax-check all .sh files"
	@echo "  check          — verify deps + config templates copied"
	@echo "  clean          — remove node_modules, runs/, *.bak (state.db preserved)"

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
