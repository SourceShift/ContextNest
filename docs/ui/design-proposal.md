# ContextNest dashboard — design proposal

## Brand seed (already in repo, keeping it)

| Token | Value | Use |
|---|---|---|
| `--accent` | `#00d4aa` (cyan-teal) | Single accent — links, focus rings, active urgency=now badges, brand mark |
| `--surface` | `#0a0a0a` (near-black) | App background — true-dark, OLED-friendly |
| `--surface-1` | `#141414` | Cards, side panels |
| `--surface-2` | `#1f1f1f` | Hover/active surface |
| `--ink` | `#f4f4f5` | Primary text |
| `--ink-muted` | `#a1a1aa` | Secondary text, timestamps |
| `--ink-dim` | `#52525b` | Tertiary, dividers |
| `--urgency-now` | `#ff6b6b` | "Claude needs you now" badge |
| `--urgency-soon` | `#ffd166` | Backlog badge |
| `--urgency-later` | `#52525b` | Deferred badge |

Type: Inter (UI) + JetBrains Mono (IDs, timestamps, JSON).
Motif: the attractor (concentric circles, outer dashed) — already in favicon. Reuse as section dividers, loading state, empty-state hero.

## Stack

| Layer | Choice | Why |
|---|---|---|
| Build | Vite | Already implied by `web/dist/index-*.js` artifact |
| Framework | React 19 | Latest, server-component-ready if we ever go SSR |
| Router | TanStack Router | Type-safe, file-based, matches your other projects |
| Data | TanStack Query | Caching + invalidation for the polling-heavy use case |
| Styling | Tailwind v4 | Single-file design tokens via `@theme`, no PostCSS dance |
| UI primitives | shadcn-style components, owned in-tree under `web/src/components/ui` | No vendor lock-in, you own the diff |
| Code-edit views | Monaco (lazy-loaded only on /tools page) | For the JSON editor in the tools playground |

## Information architecture

```
┌─ Sidebar ─────────────────────────┐
│  ◉ Inbox       ← home (urgency)   │
│  ⏱ Sessions    ← list + detail    │
│  🔍 Search      ← cross-session    │
│  ⚡ Phases     ← goal clustering  │
│  🛠 Tools       ← 7-tool playground│
│  ⚙ Substrate   ← health + config  │
└───────────────────────────────────┘
```

Six routes total. Each one maps to a specific ContextNest capability:

| Route | Backed by | Purpose |
|---|---|---|
| `/` (Inbox) | `/api/v1/tools/retrieve` with `metadata_filter: {kind: user_action \| decision}` | The "what does Claude need from me" surface — same as `contextnest inbox` CLI |
| `/sessions` | discover `~/.claude/projects/*` + `/api/v1/tools/retrieve` per session | Sessions list with status, goal_phase, accomplishment count, last activity |
| `/sessions/$id` | retrieve per session, grouped by `kind` | Drill-down: goal phases, accomplishments, learnings, todos, blockers, decisions |
| `/search` | `/api/v1/tools/retrieve` with semantic query + metadata filter chips | Cross-session search with kind/urgency/project filters |
| `/phases` | metadata_filter `kind=goal_phase` | Timeline-style view of clustered intents across all sessions |
| `/tools` | direct calls to all 7 tools | Playground: store / retrieve / update / summarize / discard / reconstruct / resonate |
| `/substrate` | `/api/health`, `/api/status`, fragment count, hook installer | Ops console: health card, fragment count, hook install/uninstall, substrate restart |

## Top three screens — ASCII wireframes

### 1. Inbox (home)

```
┌──────────────────────────────────────────────────────────────────────┐
│ ◉  ContextNest          /inbox                          ⚙ admin    │
├─────┬────────────────────────────────────────────────────────────────┤
│ ◉ I │  Inbox         Claude needs you on 3 sessions                  │
│ ⏱ S │                                                                │
│ 🔍 / │  ┌────────────────────────────────────────────────────────────┐│
│ ⚡ P │  │ ● NOW  · 4c998114 · ContextNest                        ││
│ 🛠 T │  │   Approve squash-merge of PR #20 (CI green)                ││
│ ⚙ … │  │   Why: ships v0.2 Claude Code visibility feature set       ││
│     │  │   ❓ Confirm: Squash-merge PR #20 same flow as #17/#18/#19?││
│     │  │                                              [ack] [snooze]││
│     │  └────────────────────────────────────────────────────────────┘│
│     │                                                                │
│     │  ┌────────────────────────────────────────────────────────────┐│
│     │  │ ◐ SOON · 879fccc6 · researcher                          ││
│     │  │   2 items waiting — click to expand                        ││
│     │  └────────────────────────────────────────────────────────────┘│
│     │                                                                │
│     │  ◯ LATER  · 3 sessions, 8 items                          v   │
│     └────────────────────────────────────────────────────────────────┘
└──────────────────────────────────────────────────────────────────────┘
filters:  [● now]  [◐ soon]  [◯ later]  [all projects]  [refresh: 30s]
```

- Default tab shows NOW. Soon collapses to count chip. Later collapses fully.
- Each card has the urgency dot, session id, project name, action text, reason, decision (if present), and a `[ack]` button (writes a `kind=ack` memory back to the substrate so the item disappears).
- Top-right `[refresh: 30s]` polls retrieve; clicking it toggles to live SSE when the receiver supports it (deferred).

### 2. Sessions

```
┌──────────────────────────────────────────────────────────────────────┐
│ ⏱ Sessions                              ▾ all projects   ⌕ filter   │
├──────────────────────────────────────────────────────────────────────┤
│  4c998114  ◉  ContextNest                          12 min ago     │
│      Goal: Ship the real-time Claude Code hook receiver              │
│      ▒▒▒▒▒▒▒▒▒▒░░  784 memories · 36 goal-phases · 2 decisions       │
│                                                                      │
│  879fccc6  ◐  researcher                            1 hour ago    │
│      Goal: <not yet ingested — click to backfill>                    │
│      ░░░░░░░░░░░░  0 memories                                        │
│                                                                      │
│  2ad88d5e  ◯  ContextNest                            yesterday    │
│      Goal: v0.1.0 release follow-up                                  │
│      ▒▒▒░░░░░░░░░  142 memories · 8 goal-phases                      │
│                                                                      │
│  + 47 older sessions ▾                                               │
└──────────────────────────────────────────────────────────────────────┘
```

- Each row: session id (mono), urgency dot (last decision/action urgency), project name, last-activity timestamp.
- Sparkline (▒/░) shows memory density. Hover shows the breakdown by kind.
- Sessions with 0 memories show a `[backfill]` button → calls `/api/v1/cc/hook/user_prompt_submit` with the discovered transcript path.
- Group by project via top filter; default ordering = last-activity desc.

### 3. Sessions/$id (drill-down)

```
┌──────────────────────────────────────────────────────────────────────┐
│ ⏱ 4c998114                       12 min ago  ·  ContextNest      │
├──────────────────────────────────────────────────────────────────────┤
│  ▾ Goal phases (36)            ▾ Accomplishments (230)              │
│                                                                      │
│  ┌─ Phase 1 ──────────────────────── 14:08 → 14:52 (44m, 6 turns) ─┐│
│  │ Ship real-time Claude Code hook receiver                         ││
│  │ Top accomplishments:                                             ││
│  │   • merged PR #20 to main                                        ││
│  │   • installed 4 hooks into ~/.claude/settings.json                ││
│  │   • integration tests 5/5 pass                                   ││
│  │ Decisions: 1 ack'd                                               ││
│  │ Blockers: none                                                   ││
│  └──────────────────────────────────────────────────────────────────┘│
│                                                                      │
│  ┌─ Phase 2 ──────────────────────── 14:53 → 15:08 (15m, 4 turns) ─┐│
│  │ Demonstrate ContextNest end-to-end against this session          ││
│  │ ▶ accomplishments (8)                                            ││
│  │ ▶ learnings (3)                                                  ││
│  └──────────────────────────────────────────────────────────────────┘│
│                                                                      │
│  ▾ Learnings (150)                                                   │
│  ▾ Todos (157)                                                       │
│  ▾ Decisions (2)                                                     │
│  ▾ Blockers (0)                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

- Each goal-phase shows time range + turn count + the top N memories of each kind.
- Expandable sections per kind; lazy-fetched.
- Right rail (not shown above): timeline mini-map jumping to specific turns.

### 4. Search

```
┌──────────────────────────────────────────────────────────────────────┐
│ 🔍 Search                                                            │
├──────────────────────────────────────────────────────────────────────┤
│  ⌕ rustfmt MSRV gotchas                                              │
│  Filters: [kind:learning ×]  [+ kind] [+ project] [+ urgency]        │
│                                                                      │
│  ───────────────────────────────────────────────────────────────     │
│  ▸ learning  ·  4c998114  ·  sim=0.93                             │
│    is_none_or is stable since Rust 1.82 — repo MSRV is 1.80;         │
│    use matches!(opt, None | Some(...)) instead                       │
│    Stored 14:42 · ContextNest                                        │
│                                                                      │
│  ▸ learning  ·  4c998114  ·  sim=0.87                             │
│    CI runs cargo clippy --all-targets WITHOUT -D warnings, so 600+   │
│    pre-existing warnings don't block merge                           │
│    Stored 14:21 · ContextNest                                        │
│                                                                      │
│  ▸ accomplishment · 4c998114 · sim=0.71                           │
│    Applied rustfmt + reran 8 inbox unit tests (all pass)             │
│    Stored 13:55 · ContextNest                                        │
└──────────────────────────────────────────────────────────────────────┘
```

- Text input at top, chips below for metadata filters that get added with `[+ ...]` buttons.
- Results stream in as you type (300ms debounce), each card shows kind, session, similarity, content snippet, timestamp.
- Clicking a result jumps to `/sessions/$id` scrolled to that fragment.

### 5. Tools playground (deferred until inbox + sessions + search ship)

```
┌──────────────────────────────────────────────────────────────────────┐
│ 🛠 Tools                                                              │
├──────────────────────────────────────────────────────────────────────┤
│  [store] [retrieve] [update] [summarize] [discard] [reconstruct] [resonate]│
│                                                                      │
│  Endpoint: POST /api/v1/tools/store                                  │
│  ┌──────────────────────────────────┐  ┌─────────────────────────┐   │
│  │ Request                          │  │ Response                │   │
│  │ {                                │  │ {                       │   │
│  │   "content": "test memory",      │  │   "attractor_id": "...",│   │
│  │   "session_id": "demo",          │  │   "stored": true        │   │
│  │   "metadata": {"kind":"diag"}   │  │ }                       │   │
│  │ }                                │  │                         │   │
│  └──────────────────────────────────┘  └─────────────────────────┘   │
│  [send]   templates: ▸ basic ▸ inbox-item ▸ goal-phase               │
└──────────────────────────────────────────────────────────────────────┘
```

### 6. Substrate (ops)

```
┌──────────────────────────────────────────────────────────────────────┐
│ ⚙ Substrate                                                          │
├──────────────────────────────────────────────────────────────────────┤
│  ◉ HEALTHY                                       :28080 · v0.1.0     │
│  Fragments: 1,406    Sessions: 12    Memory: 142 MB                  │
│                                                                      │
│  Hooks                                                               │
│  ┌──────────────────────────────────────────────────────────────────┐│
│  │ ● SessionStart       wired → /api/v1/cc/hook/session_start       ││
│  │ ● UserPromptSubmit   wired → /api/v1/cc/hook/user_prompt_submit  ││
│  │ ● Stop               wired → /api/v1/cc/hook/stop                ││
│  │ ● TaskCompleted      wired → /api/v1/cc/hook/task_completed      ││
│  │                                                                  ││
│  │                  [reinstall hooks] [uninstall]                  ││
│  └──────────────────────────────────────────────────────────────────┘│
│                                                                      │
│  Embedding:  local TF-IDF (256-d)               [switch to openai]   │
│  LLM:        openai gpt-4o-mini                  [edit]              │
│                                                                      │
│  Recent activity                                                     │
│  ▷ 14:52  store     learning   4c998114   162 fragments           │
│  ▷ 14:51  retrieve  filter     4c998114   3 hits                  │
└──────────────────────────────────────────────────────────────────────┘
```

## Build plan (incremental, ship-as-you-go)

| Step | Page | LOC est. | Deliverable |
|---|---|---|---|
| 1 | Scaffold: Vite + React + Tailwind v4 + TanStack Router + shadcn primitives + brand tokens | ~400 | Empty app boots on :5173 with sidebar + theme |
| 2 | `/` Inbox (the killer view) | ~300 | Reads `/api/v1/tools/retrieve` with metadata_filter, groups by session, sorts by urgency |
| 3 | `/sessions` + `/sessions/$id` | ~400 | List + drill-down with goal-phase grouping |
| 4 | `/search` | ~250 | Debounced retrieve with chip-based filters |
| 5 | `/substrate` | ~200 | Health card + hook wiring panel + activity log |
| 6 | `/phases` + `/tools` | ~300 | Goal-phase timeline + tools playground |
| 7 | Polish pass | ~150 | Empty states, error boundaries, keyboard shortcuts (g→i, g→s, g→/, etc.) |

Total: ~2000 LOC of TypeScript + React. Each step is an independently mergeable PR.

## Questions before I build

1. **Color direction OK?** `#00d4aa` cyan-teal on near-black, with red/amber/grey for urgency. Pure dark or also a light mode?
2. **Density?** Inbox could either be card-style (above) or table-style (denser, more like Linear). Card style wins for the "things Claude needs from me" use case because each item has a primary read; table style wins if you have 50+ items per session. I'd default to **cards with a "compact" toggle**.
3. **Real-time?** Polling at 30s is simplest. SSE from substrate would be nicer but requires endpoint work — defer to a follow-up?
4. **Auth?** Local-only for now (no auth at all), or add a token check from `.env`?
5. **Ship in this repo as `web/` or a separate package?** Default = same repo.

Greenlight and I'll start with Step 1 (scaffold + tokens + sidebar) — about 30 min of work for a previewable shell.
