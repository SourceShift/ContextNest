# ContextNest development workflow

The rationale behind the workflow rules in `CLAUDE.md`. Distilled from the
patterns that worked in this repo, with references to broader best-practice
research where the choice is non-obvious.

Aimed at both human contributors and AI assistants (Claude Code) so both
follow the same playbook.

## 1. Parallel work via Git worktrees

ContextNest sits in a single repo but the work splits naturally across
parallel concerns: substrate refactors, ingest tweaks, dashboard pages,
docs rewrites. Switching branches in one working tree means rebuilding
`target/`, invalidating IDE state, and risking cross-contamination. Git
worktrees solve this — each branch lives in its own directory, sharing one
`.git` history.

### Directory layout

```
/Volumes/docker-ssd/Migration/Development/
├── ContextNest/                      # primary tree (default branch / main work)
├── ContextNest-<slug>/               # ad-hoc one-off worktree
└── worktrees/
    ├── cn-inbox-sort-filter/         # long-running feature worktrees
    ├── cn-drop-cc-prefix/            # land-and-prune branches
    └── cn-fix-embedder-context-limit/
```

The convention is **sibling directories**. Don't nest worktrees inside the
primary tree — `cargo` and `git` both get confused by `.git` files inside
a working tree.

### Creating, listing, removing

```bash
# Create a fresh worktree from origin/main on a new branch
git worktree add -b feat/<slug> ../ContextNest-<slug> origin/main

# Or for a long-lived parallel feature, use the worktrees/ directory
git worktree add -b feat/<slug> ../../worktrees/cn-<slug> origin/main

# See every worktree + its branch + its HEAD
git worktree list

# Tear down after the branch is merged
git worktree remove ../ContextNest-<slug>
git branch -d feat/<slug>          # optional; landed feature branches can stay or be pruned
```

**Always branch from `origin/main`, not from `HEAD`.** Branching from
whatever happens to be checked out in the primary tree can sneak unrelated
in-progress work into the new branch. The fetch is cheap; pay it.

### What's shared vs isolated

| Asset | Shared across worktrees? | Notes |
|---|---|---|
| `.git/` | yes | Single history; commits in one tree are visible to all |
| `target/` (cargo) | **no** | Each tree has its own; first build is cold |
| `node_modules/` (web/) | **no** | Run `pnpm install` per worktree |
| `~/.contextnest/wal.jsonl` | **yes** | Lives outside the repo; one substrate state for all worktrees |
| `~/.cargo/registry/` | yes | Cargo's global download cache; not per-worktree |
| `config.toml` | per-worktree (gitignored) | `make cn-config` per tree |

### Parallel agents

Worktrees make it safe to run multiple Claude Code sessions, or any other
AI agent, on different branches at once. The critical heuristic is **strict
file isolation** — worktrees don't solve merge conflicts; they defer them
to the merge phase. Scope each agent's task to a disjoint set of files
(e.g. `src/api/` vs `web/src/routes/` vs `docs/`) so the eventual merges
are trivial.

### Bash gotcha

The Claude Code Bash tool resets `cwd` between invocations. When working
in a non-primary worktree, prefix every command:

```bash
cd /Volumes/docker-ssd/Migration/Development/ContextNest-<slug> && cargo test
```

Without the explicit `cd`, the command silently runs in the primary tree.
This can mask which worktree state is actually being modified.

## 2. Branch + commit conventions

ContextNest follows **Conventional Branches** + **Conventional Commits**.
Both are parser-friendly, machine-readable, and enable automated changelog
generation downstream.

### Branch prefixes

| Prefix | When to use |
|---|---|
| `feat/` | New functionality (substrate tool, dashboard route, API endpoint) |
| `fix/` | Bug fix in existing functionality |
| `refactor/` | Internal restructuring without behavior change |
| `chore/` | Non-code maintenance (CI config, lint config, dep bumps not done by Dependabot) |
| `docs/` | Documentation only |
| `release/` | Release prep (rarely used for v0.1.x) |
| `hotfix/` | Urgent out-of-band production patches |

Rules:

- **Lowercase alphanumeric + hyphens only.** No uppercase, no underscores,
  no consecutive hyphens.
- **Descriptive slug, not a ticket number alone.** `feat/cn-drop-cc-prefix`
  reads better than `feat/issue-42`.
- **Always branch from `origin/main`.** Never edit on `main` directly and
  retroactively branch — create the branch *before* the first edit.

### Conventional Commits

Subject line shape: `<type>(<scope>): <imperative description>`

- `<type>` — same vocabulary as branch prefixes (`feat`, `fix`, `refactor`,
  `chore`, `docs`, `test`, `perf`, `build`, `ci`).
- `<scope>` — the subsystem touched. For ContextNest: `embedder`,
  `consolidation`, `session-id`, `cc-hooks`, `inbox`, `field`, `wal`,
  `web`, `ingest`, `api`, etc. Optional but valued.
- Subject is **imperative mood**: "If applied, this commit will …".
  - Yes: `fix(embedder): clamp input by max_input_length`
  - No: `fixed embedder clamping`
  - No: `Embedder Clamp Fix.`
- Subject ≤72 chars, no trailing period, capitalised first word after the
  colon.

Body (optional, after a blank line):

- Wrap at 72 chars manually (Git doesn't wrap for you).
- Explain **what** and **why**, not **how**. The diff shows how.
- For breaking changes, add a `BREAKING CHANGE:` footer paragraph, or
  append `!` before the colon in the subject (`feat(api)!: ...`).

Worked example from this repo:

```
fix(embedder): clamp input by max_input_length before provider call

Production substrate hit OpenAI-compatible 400 errors when a single
fragment exceeded the model's context window. The max_input_length knob
already existed on EmbeddingModelSettings but EmbeddingService never
enforced it — any over-budget fragment failed the embed call and lost
its embedding.

This change reads default_model.settings.max_input_length and truncates
over-budget input (char-aware, multibyte-safe) before dispatch. A
tracing::warn fires on truncation so operators can spot fragments that
exceed their configured budget.
```

## 3. Merge strategies

Three options Git provides, each with different historical consequences:

| Strategy | History | When to use |
|---|---|---|
| **Squash** | Linear, one commit per PR | Default for feature PRs in this repo |
| **Merge commit** (`--no-ff`) | Branching, preserves every commit | Between long-lived integration branches |
| **Rebase + merge** | Linear, preserves every commit | Multi-commit PRs where granularity matters and the branch is private |

**Default: squash.** Reasons:

1. The branch's intermediate commits ("wip", "fix typo", "address review")
   add noise to the trunk.
2. The PR description survives as the squash commit body, so context is
   preserved without history pollution.
3. `git log main --oneline` reads as a feature timeline, not a development
   timeline.

**Squash trap:** once a feature branch is squashed and you keep working on
the same branch, a second squash to main produces catastrophic merge
conflicts because Git can't map the rewritten hashes. After a squash:
delete the branch. If you need follow-up work, create a fresh branch off
the now-squashed `main`.

**Don't rebase public branches.** Rebasing rewrites hashes; anyone who
branched off the original will get out-of-sync. Rebase is for cleaning
up your *private* feature branch before opening the PR.

## 4. Pull request workflow

### Opening a PR

```bash
gh pr create --base main --head feat/<slug> \
  --title "<conventional-commit-style subject>" \
  --body-file <path-to-body.md>
```

Why `--body-file` instead of `--body`: PR bodies often contain backticks
and code fences that hereddoc parsing mangles. A file is simpler and
re-runnable.

### PR body template (minimum)

```markdown
## Summary
One paragraph: what changed and why.

## Diff
File count + line counts. Tables of touched areas if it helps.

## Test plan
- [x] cargo fmt --check
- [x] cargo build
- [x] cargo test — <N> tests pass
- [x] cargo clippy --lib --no-deps -- -D warnings — no new lints in touched files
- [ ] Manual: <replayable recipe>

## Migration safety (if applicable)
Backup paths, rollback steps, .bak breadcrumbs.

## Followups
Items deliberately not in this PR.
```

### Merging

**ContextNest's permission policy: human clicks merge in the GitHub UI.**

- `gh pr merge` and direct `git push origin main` are both blocked for
  agentic sessions. This is intentional — keeps a human in the loop on
  the default branch.
- Don't retry the denied commands. Surface the PR URL and stop.
- Squash is the default merge strategy (GitHub UI button).

## 5. CI gates

ContextNest's four canonical gates, in fail-fast order:

```bash
cargo fmt --check                                  # fastest, ~1s
cargo clippy --lib --no-deps -- -D warnings        # ~5s
cargo build                                        # ~40s on first run, ~10s incremental
cargo test                                         # ~30s, includes integration suites
```

Running them as a chain (fmt → clippy → build → test) means a missed
`cargo fmt` fails in under a second instead of after a full test run.

The Makefile target `make cn-check` packages this exact sequence (see the
`Makefile` for the recipe).

**Clippy debt baseline:** `origin/main` carries ~600 pre-existing clippy
errors from a newer rustc lint set. The gate for any PR is "no NEW errors
introduced by this diff", not "zero total errors" — filter clippy output
to files you actually touched before judging. The cleanup is a separate
hygiene task, tracked outside per-PR review.

## 6. Stateful-data safety

ContextNest persists state in two places, both outside the repo:

1. **WAL** at `~/.contextnest/wal.jsonl` — append-only JSONL, every
   successful `store` call appends a record. On startup the records replay
   into a fresh `ContextNestServices`.
2. **Sidecars** in memory — `fragment_texts`, `fragment_metadata`,
   `SessionIndex`, `consolidation_queue`. Fully ephemeral. Process restart
   wipes everything; replay from WAL reconstructs.

### Before any WAL-touching change

```bash
cp ~/.contextnest/wal.jsonl ~/.contextnest/wal.jsonl.bak-pre-<refactor>
```

"WAL-touching" means: changes to `WalRecord` variants, session-id format,
migration logic in `src/services/wal.rs`, or any code path that mints
session IDs (`src/ingest/claude_code/extractor.rs`, `src/api/cc_hooks.rs`,
`src/bin/contextnest.rs`).

The migrator itself writes `wal.new` then atomically renames
`wal → wal.bak`, `wal.new → wal`. That `.bak` is the **automatic recovery
breadcrumb** — don't delete it until the new binary has run successfully
for at least one session. Your manual `.bak-pre-<refactor>` is the
**second-line breadcrumb** if both the new binary and the migration are
buggy.

### Embedder model swaps

Swapping the embedding model invalidates all existing basins (the vectors
don't match the new model's space). There is **no re-consolidation worker**
for this case yet. Manual recovery: wipe the WAL, restart, re-ingest.

## 7. Pre-commit gating (status: not configured)

The article that inspired this doc recommends a two-tier validation
system: fast formatters in a pre-commit hook, slow tests in a pre-push
hook. ContextNest has **no Husky-style pre-commit hooks today** because:

1. The codebase is small enough that running `cargo fmt && cargo clippy`
   manually before a commit is friction-acceptable.
2. The CI pipeline catches missed formatting on the PR (the gate is
   `cargo fmt --check`, which fails loudly).
3. Adding a hook system means adding a dev-dep + a setup step + a
   maintenance burden; not worth it at the current scale.

If/when the contributor count grows past a handful, the natural addition
would be `pre-commit` (the Python tool, language-agnostic) running
`cargo fmt --check` + `cargo clippy --lib --no-deps -- -D warnings` on
the staged files. The `pre-push` hook would run `cargo test`. Logged as a
followup, not a current requirement.

## 8. Supply-chain security (status: partial)

Dependabot is configured (`.github/dependabot.yml`) and routinely opens
PRs for cargo and npm deps. Strategy:

- Patch-version bumps → merge eagerly after CI passes.
- Minor-version bumps → review the changelog for behavioral changes.
- Major-version bumps → treat as a manual upgrade task (separate branch,
  potential migration).

There's no SAST (Static Application Security Testing) tool wired in for
Rust today. The codebase is small enough that `cargo audit` (run manually
or in CI) covers the supply-chain advisory surface. The known unfixable
RSA Marvin advisory is acknowledged in `.cargo/audit.toml`.

For secret scanning, GitHub's native push protection (org-level) catches
the common cases. No additional tooling.

## 9. Summary — the rules in one place

1. Branch from `origin/main`, never edit on `main`.
2. Branch + commit conventions: `<type>/<slug>` and `<type>(<scope>): <imperative>`.
3. Use a worktree per concurrent branch; sibling directories, isolated `target/`.
4. WAL is shared across worktrees — back it up before WAL-schema changes.
5. Squash-merge feature PRs; never rebase a public branch.
6. Open PRs via `gh pr create --base main`; human clicks merge in the GitHub UI.
7. Four CI gates: `fmt --check → clippy → build → test`. Filter clippy output to your touched files.
8. Don't bypass hooks with `--no-verify` — fix the gate or surface why the bypass is justified.

## References

- [Conventional Commits](https://www.conventionalcommits.org/)
- [Conventional Branch](https://conventional-branch.github.io/)
- [Git worktree docs](https://git-scm.com/docs/git-worktree)
- ContextNest internal: `CLAUDE.md` (this file's daily-driver summary),
  `CONTRIBUTING.md` (the canonical pipeline + how to add a tool),
  `docs/architecture-honest.md` (env knobs, grep-verify recipe).
