---
title: UI/UX Brief — ContextNest dashboard (all features)
slug: dashboard-all-features
route: / (root) + /sessions, /sessions/$id, /search, /phases, /tools, /substrate
brief_type: net-new
audience: ux-designer
created: 2026-05-20
author: Claude session 4c998114
status: draft
related_files:
  - docs/ui/design-proposal.md
  - web/src/main.tsx
  - web/src/styles.css
  - web/src/routes/__root.tsx
  - web/src/components/Sidebar.tsx
  - web/src/components/SubstrateBadge.tsx
  - src/api/tools.rs
  - src/api/cc_hooks.rs
  - src/inbox/mod.rs
  - src/ingest/claude_code/extractor.rs
  - docs/usage.md
  - docs/z-insight-schema.md
  - docs/ingest/claude-code-hooks.md
---

# UI/UX Brief — ContextNest dashboard

> **Brief type:** net-new (no prior UI exists; visual scaffold landed in PR #21 but every route is an `EmptyState`).
> **Read time:** ~12 min.
> **Designer can start:** yes for all features except `/phases` (visual treatment of clustered intent timeline needs PM call — see §14 open questions).

## 1. TL;DR

ContextNest is a Rust-based "continual-learning memory substrate" that ingests every turn of every local Claude Code session and stores structured memories — accomplishments, learnings, blockers, todos, decisions, action items — as queryable fragments with rich metadata. The dashboard is the operator-facing surface for those fragments: **what is Claude waiting on me for, across every session?**, **what did Claude learn this week?**, **show me where the agent is blocked.** It's a developer tool for a single power user (the substrate's owner), not a multi-tenant SaaS. Sleek and dense, dark-first, fast keyboard navigation, terminal-adjacent in feel. The designer needs to ship: (a) Inbox as the killer view, (b) Sessions/Search/Phases as supporting lenses on the same memory corpus, (c) Tools playground for power-user introspection, (d) Substrate ops console for health + hooks. Visual scaffold + brand tokens + sidebar already shipped (PR #21); every route page renders an `EmptyState` placeholder.

## 2. Why now

Not applicable — this is a net-new build for a v0.2 milestone that landed its substrate-side prerequisites this week (PRs #17 ingester → #18 metadata_filter → #19 inbox CLI → #20 real-time hook receiver). The dashboard turns the CLI-and-curl-only feature set into something a human can actually use day-to-day.

## 3. Job to be Done

**Primary JTBD:**
> When I'm context-switching between several Claude Code sessions, I want to see in one place what each session is waiting on me for and what it learned, so I can clear blockers fast and resume the right session without remembering which terminal it was in.

**Secondary jobs:**
- **When I'm starting a new Claude session,** I want to recall what Claude already learned about a topic across past sessions, so I can avoid re-explaining context that's already captured.
- **When the substrate misbehaves,** I want to see at a glance whether hooks are wired, what embedding model is active, and what fragment count looks like, so I can decide whether to restart or reconfigure.
- **When I want to verify a substrate behavior in isolation,** I want to call any of the seven tools (store / retrieve / update / summarize / discard / reconstruct / resonate) with a free-form JSON request and see the typed response, so I can debug without writing curl.

## 4. Users

| User type | Proficiency | Frequency | Context | Permission |
|---|---|---|---|---|
| **Substrate owner / power user** | High — runs the substrate themselves, comfortable with the seven-tool API + metadata-filter semantics | Daily, several short sessions, often while a Claude session is also active in a terminal pane | Desktop, focused; sometimes split with a terminal | None — single-user, localhost-only by default |
| **Read-only spectator** (future, not v1) | Medium | Occasional | Same machine, separate browser window | None |

Single-user assumption matters: no team workspaces, no permission boundaries, no audit log, no per-user theming. The dashboard talks to one substrate instance (own machine or LAN). Anything multi-user is explicitly deferred to a future product layer.

## 5. Current state inventory

### 5a. Files involved

| Role | File | What it contributes |
|---|---|---|
| **Brand seed** | `web/src/styles.css` | `@theme` tokens — accent `#00d4aa`, surface stack, urgency colours, type, radius |
| **App shell** | `web/src/main.tsx` | React 19 + QueryClient (30s default polling) + RouterProvider |
| **Root layout** | `web/src/routes/__root.tsx` | Sidebar + top bar with substrate health badge + outlet |
| **Sidebar** | `web/src/components/Sidebar.tsx` | 6-item nav, active-route highlight, keyboard-shortcut hints, attractor logo, footer with version + substrate origin |
| **Health badge** | `web/src/components/SubstrateBadge.tsx` | Live-polled health dot in top bar (15s) |
| **Inbox shell** | `web/src/routes/index.tsx` | `EmptyState` only — Step 2 lands the actual view |
| **Sessions shell** | `web/src/routes/sessions.tsx`, `web/src/routes/sessions.$id.tsx` | `EmptyState` — Step 3 |
| **Search shell** | `web/src/routes/search.tsx` | `EmptyState` — Step 4 |
| **Substrate shell** | `web/src/routes/substrate.tsx` | Partial — live health + version cards; rest in Step 5 |
| **Phases shell** | `web/src/routes/phases.tsx` | `EmptyState` — Step 6 |
| **Tools shell** | `web/src/routes/tools.tsx` | `EmptyState` — Step 6 |
| **API client** | `web/src/lib/api.ts` | Typed fetch over `/api/health`, `/api/status`, `/api/v1/tools/retrieve`, `/api/v1/tools/store` |
| **Types** | `web/src/lib/types.ts` | `MemoryKind`, `Urgency`, `FragmentMetadata`, `RetrieveHit` |

**Backend reference (what the UI consumes):**

| Capability | Backend file | Notes |
|---|---|---|
| Seven-tool API | `src/api/tools.rs:1040` | `POST /api/v1/tools/{store,retrieve,update,summarize,discard,reconstruct,resonate}` |
| `retrieve` + `metadata_filter` | `src/api/tools.rs:356` | Exact-equality filter, AND across keys, filters AFTER similarity but BEFORE top_k |
| Hook receiver | `src/api/cc_hooks.rs` | `POST /api/v1/cc/hook/<event>` returns 204 fast, async tail+ingest |
| `contextnest inbox` CLI logic | `src/inbox/mod.rs` | Renders `kind=user_action` + `kind=decision,awaiting_decision=true` by urgency; the UI's Inbox view replicates this |
| z-insight schema | `docs/z-insight-schema.md` | The metadata vocabulary the substrate stores; the UI's filters operate on this |

### 5b. Existing design assets

| Asset | Status |
|---|---|
| `docs/ui/design-proposal.md` | This brief's predecessor — IA + ASCII wireframes for every route. Engineering doc, not designer-grade hi-fi. |
| `web/dist/index.html` favicon SVG | Brand mark + accent colour (cyan-teal `#00d4aa`) — already extracted into `styles.css` `@theme` |
| `web/src/styles.css` `@theme` block | Design tokens (colours, type, radius) locked — the designer should treat these as immutable for v1 |
| Screenshots | none in repo |
| Figma file | none |
| `design/v1/` or similar dir | does not exist in this repo (the skill's reference dir is from a different project) |

**No prior visual design exists.** Treat as net-new. The brand tokens in `styles.css` and the ASCII wireframes in `docs/ui/design-proposal.md` are the only constraints.

### 5c. Related docs

- `docs/architecture.md` — substrate primitives (attractor manager, basin formation, decay)
- `docs/usage.md` — every tool's request/response with curl examples; the Tools playground will mirror these
- `docs/z-insight-schema.md` — the metadata fields the dashboard reads (`kind`, `urgency`, `awaiting_decision`, `src_session`, `project_cwd`, `step`, `reason`, etc.)
- `docs/ingest/claude-code-hooks.md` — install / event-by-event semantics; the Substrate page's hook panel mirrors this
- `docs/ui/design-proposal.md` — full IA + ASCII wireframes (engineering-doc, not designer-grade)
- `docs/roadmap/v0.2-claude-code-ingest.md` — strategic context for why this dashboard exists

## 6. Heuristic audit

Not applicable — net-new. No current UI to critique. (Inverse audit: design tensions and known traps for the designer.)

### Design tensions the designer must resolve

| # | Tension | Notes |
|---|---|---|
| 1 | **Density vs glanceability** | Power user wants info-dense (Linear / 1Password style) but Inbox cards need a primary read in ~1 second per card. Designer must show both layouts (default cards + a "compact" table toggle). |
| 2 | **Card-style vs row-style for Inbox** | Cards win for "Claude needs X from me" (each item has a primary read). Rows win at scale (10+ pending items). Designer proposes both; default = cards with toggle. |
| 3 | **Real-time freshness signaling** | The hook receiver lands new memories ~ seconds after a Claude turn. UI polls every 30s. Designer must signal *what's new* without making the UI feel restless — likely a subtle "new" badge that fades, NOT an animated toast. |
| 4 | **Metadata complexity** | Memories carry a free-form metadata bag (12+ possible keys). UI must surface 3-4 most-useful keys (`kind`, `urgency`, `awaiting_decision`, `project_cwd`) prominently and hide the rest behind a disclosure. |
| 5 | **Empty states as the most common state** | A fresh substrate has 0 fragments. After backfill, some kinds (decisions, blockers) are often 0. Designer must make empty states *informative*, not punitive — explain what populates the kind, link to the relevant CLI command. |
| 6 | **Substrate "down" is a normal state** | The substrate is a local process the user can stop. UI must handle "substrate unreachable" gracefully on every page, not just the top-bar badge. |

## 7. Information architecture

```
ContextNest dashboard
│
├── Sidebar (persistent, 224 px wide, collapsible to icon-only in future)
│   ├── Inbox        →  /
│   ├── Sessions     →  /sessions
│   ├── Search       →  /search
│   ├── Phases       →  /phases
│   ├── Tools        →  /tools
│   └── Substrate    →  /substrate
│
├── Top bar
│   ├── (left)  Page-context actions (filters, refresh) — per route
│   └── (right) Substrate health badge (dot + text)
│
└── Main pane (overflow-y auto, 24 px padding)
    └── <Route component>
```

Per-route hierarchies are detailed in §8 (State machine) — IA is per-feature there.

**Persistent IA decisions (apply to every route):**

- Sidebar is always visible. No hamburger; no mobile mode.
- Page-context controls live in the top bar (right side, before the badge) — keeps consistent with persistent global controls.
- Empty states use the attractor mark from `Logo.tsx` at 40 px, dimmed; do NOT use icons from `lucide-react` for empty states (they read as "generic").
- Hover states never use scale/translate; only background + border shifts. This is a data UI, not a marketing site.

## 8. State machine

Each feature gets its own subsection. **Designers consistently undermodel states for memory-driven UIs**; this section is the longest in the brief for a reason.

### 8.1 Inbox (`/`)

**Backing data:** `POST /api/v1/tools/retrieve` for every discovered session, with `metadata_filter: {kind: "user_action"}` and `metadata_filter: {kind: "decision", awaiting_decision: true}`, merged and sorted by urgency.

**JTBD focus:** Primary JTBD (§3). The killer view.

**IA:**
- H1: "Inbox" + secondary: "What Claude needs from you across N sessions"
- Section: Filter bar (urgency tabs, project filter, refresh control)
- Section: Item cards, grouped by session, sorted by urgency (now → soon → later)
- Empty state: "Nothing waiting — Claude isn't blocked on you"

**Card content (every item):**
- Urgency dot + label (now / soon / later — colours from `--color-urgency-*`)
- Session id (mono, bare UUID — typically truncated to first 8 chars for display) + project basename
- Action text (the imperative sentence from z-insight `requires_user_action[]`)
- Reason (single line, `--color-ink-muted`)
- Decision (only if `kind=decision` — render with a `?` glyph)
- `[ack]` button (writes a `kind=ack`-tagged memory so the item disappears)
- `[snooze 1h | 1d]` (optional, deferred)

**State machine:**

| State | What the user sees | Why |
|---|---|---|
| **Cold** (first paint, no data yet) | Skeleton: 3 dimmed card outlines, no badge counts | Tells user "I'm fetching" without empty-trap |
| **Loading next poll** | Existing cards stay; subtle pulse on the refresh control | Polling at 30s — never blanks the UI |
| **Empty (substrate has 0 inbox items)** | Attractor mark + "Nothing waiting — Claude isn't blocked on you" + tip linking to `docs/z-insight-schema.md` | Power user might forget the schema; nudge to the doc |
| **Empty (substrate unreachable)** | Red dot in top bar + body content "Substrate at localhost:28080 not responding. [restart command + check health] [docs link]" | Common; substrate is a local process |
| **Partial (some sessions returned, others errored)** | Render what came back + a small "(2 sessions unreachable)" badge | Don't all-or-nothing the view |
| **Error (full failure)** | Same body shell, error card centred with the actual error + retry button | Surface the actual error string from `ApiError`, not "something went wrong" |
| **Item ack'd** | Card fades to 30 % opacity over 250 ms, then removed from DOM next poll | Optimistic update; if the ack store call fails, restore opacity and show inline error |
| **New item arrived (since last view)** | Card has a subtle `--color-accent` left border for first 60 s after appearance | Real-time signal without toast/animation noise |

**Edge cases:**
- One session with 50+ user_actions — collapse after 5 with "show all 50" disclosure
- An item whose `reason` is missing or empty — render the action only, no reason line
- An item whose action text is multi-line — clamp to 3 lines with ellipsis + on-hover expand
- An item whose `step` field collides with another (e.g. two step:1 in the same urgency group) — surface step order verbatim, don't try to reorder

### 8.2 Sessions list (`/sessions`)

**Backing data:** Walk `~/.claude/projects/` for discovered sessions + `POST /retrieve` per session for kind=`session_title` and kind=`goal_phase` (top 1).

**IA:**
- H1: "Sessions" + "Every Claude Code session this substrate has seen"
- Top bar: Project filter (substring), date filter (last 1d / 7d / 30d / all), search box
- Row list, default sort = last activity desc

**Row content:**
- Substrate session id — the bare Claude Code UUID, mono, truncated to first 8 chars for display
- Urgency dot (last decision/action urgency, defaults to grey)
- Project basename (clickable → filter to that project)
- Last activity timestamp (relative: "12 min ago", "yesterday", "3d ago")
- Goal phase (latest one, single line)
- Sparkline of memory density (256-char tiny SVG, optional)
- Right-side mini-counts: `memories`, `goal_phases`, `decisions`
- Click → `/sessions/$id`

**States:**

| State | Behavior |
|---|---|
| Cold | 5 skeleton rows |
| Empty (no sessions discovered) | "No Claude Code sessions found under ~/.claude/projects. Make sure hooks are installed: `contextnest ingest claude-code --install-hooks`" |
| Empty (no fragments in substrate) | List discovered sessions, each with "(not yet ingested)" + `[backfill]` button that runs the batch ingest in-process |
| Error | Inline per-row error if a specific retrieve failed; overall error if discovery failed |
| Filtered to zero | "No sessions match. Clear filters." |

### 8.3 Session detail (`/sessions/$id`)

**Backing data:** Multiple `retrieve` calls scoped to the session, grouped by `kind`.

**IA:**
- H1: Session id + project + last activity
- Tabbed or accordion sections per kind:
  - **Goal phases** (default open) — chronologically ordered phase cards with time span + turn count
  - **Accomplishments** (collapsed) — flat list, newest first
  - **Learnings** (collapsed) — flat list
  - **Todos** (collapsed) — with status badges (pending / in_progress / completed)
  - **Decisions** (collapsed)
  - **Blockers** (collapsed)
  - **User actions** (collapsed)
  - **Raw timeline** (collapsed, advanced) — every memory in chrono order

**Goal phase card content:**
- Phase number + title (the clustered goal text)
- Time span (start → end, "44 min, 6 turns")
- Top 3 accomplishments (snippets)
- Counts: decisions, blockers, learnings within this phase
- Click → expand inline to full content

**States:**
| State | Behavior |
|---|---|
| Cold | Header + skeleton goal-phase cards |
| Empty (session has 0 fragments) | `[backfill this session]` button calling batch ingest |
| Partial (some kinds returned, others errored) | Render what loaded; small error badge per failing section |
| Error (session 404) | "Session `<uuid>` not found in substrate. [backfill] or [view all sessions]" |

**Edge cases:**
- A session with > 1000 memories — paginate within each accordion section
- A goal phase that spans only 1 turn — don't render time-span if start == end timestamp

### 8.4 Search (`/search`)

**Backing data:** `POST /retrieve` with `query` + optional `metadata_filter` chips, scoped to a session (single-select) or no session (cross-session — would require multiple calls merged client-side).

**IA:**
- H1: "Search"
- Search input (cmd+/ keyboard shortcut to focus)
- Filter chips below input: `[+ kind]` `[+ urgency]` `[+ project]` `[+ session]` — clicking opens a dropdown of valid values, selecting adds a chip; chips render as `kind:learning ×`
- Results list below, streaming as the user types (300ms debounce)
- Right-side: keyboard hints — `↑↓ to nav`, `↵ to open`, `esc to clear`

**Result card:**
- Kind badge (mono caps, small)
- Session id + project
- Similarity score (0.00-1.00) shown as a thin progress bar to the right
- Content snippet (first 200 chars, line wraps at 2 lines max)
- Stored timestamp + project
- Click → jumps to `/sessions/$id` with scroll-into-view to that fragment

**States:**
| State | Behavior |
|---|---|
| Cold (input focused, no query yet) | Suggestion chips: "recent learnings", "open decisions", "blockers across all projects" — click runs the matching filter |
| Loading | Skeleton results; existing results stay if any |
| Empty (query has 0 hits) | "No memories match. Try removing a filter." |
| Empty (query has hits but all filtered out) | Same as above but with chip-removal hint |
| Error | Inline error with the actual error string |

**Tension:** Substrate's default embedding is hash-based, which means semantic similarity scores are uniformly high (~0.99 for any pair). Designer should NOT make similarity score visually prominent in v1 — surface it small + monospace. When real embeddings ship (OpenAI/Ollama config), this can be elevated.

### 8.5 Phases (`/phases`)

**Backing data:** `POST /retrieve` with `metadata_filter: {kind: "goal_phase"}` across all sessions.

**IA:**
- H1: "Phases" + "Goal phases — multi-turn clustered intents across every session"
- Toggle: timeline view (default) vs cluster view (alt)
- Time-axis on left, phase cards stacked vertically by recency

**Phase card:**
- Phase title (the goal text)
- Session id + project (link to `/sessions/$id`)
- Time span + turn count
- Cluster size (e.g. "6 z-insights clustered")
- Top-2 facts/learnings extracted within this phase

**State machine:** same envelope as §8.2.

**Open design question (§14):** How to visualize "this phase clustered 6 turns into one"? Sparkline-like? Stacked dots? A horizontal accordion? Defer hi-fi for this until PM confirms — this is the one feature where information density vs cognitive load isn't a settled call.

### 8.6 Tools (`/tools`)

**Backing data:** Direct user-issued POST to any of the seven tool endpoints, returning typed responses.

**IA:**
- H1: "Tools"
- Tab strip across the top: `[store]` `[retrieve]` `[update]` `[summarize]` `[discard]` `[reconstruct]` `[resonate]`
- Below tab strip, two columns:
  - **Left (Request):** JSON editor with syntax highlight + line numbers, prefilled with the selected tool's template
  - **Right (Response):** Empty until first `[send]`, then displays response JSON with collapsible sections and inline copy buttons
- Below editor: `[send]` button + template chips (`▸ basic` / `▸ inbox-item` / `▸ goal-phase`) that swap the request body
- Below response: HTTP status + timing pill, e.g. `200 · 142ms`

**State machine:**
| State | Behavior |
|---|---|
| Cold | Default template prefilled per tool |
| Sending | `[send]` shows spinner; editor read-only |
| Success | Response panel populates; timing pill green |
| Error | Response panel shows the error body + timing pill red |
| Invalid request JSON | `[send]` disabled; inline marker at the bad line in editor |

**Designer note:** This page should *feel* like a dev tool — closer to Postman or Insomnia than to a SaaS dashboard. Monospace dominates; chrome is minimal.

### 8.7 Substrate (`/substrate`)

**Backing data:** `GET /api/health`, `GET /api/status`, fragment counts (via repeated retrieve with metadata filters), hook config (read `~/.claude/settings.json` — out of scope for the browser; for v1 the page shows what URL the install-hooks command WOULD write).

**IA:**
- H1: "Substrate"
- Card 1: Health card (already shipped) — big status dot + version + base URL
- Card 2: Fragment counts by kind (matrix or bar chart, 12 kinds)
- Card 3: Hooks panel — list of 4 hook events with wired URL each + `[reinstall]` `[uninstall]` buttons
- Card 4: Embedding provider — current ("local TF-IDF 256-d") + `[switch to OpenAI]` `[switch to Ollama]` actions
- Card 5: Recent activity — last 50 substrate operations (store / retrieve / discard) with timing

**State machine:** As in §8.2 — partial render per card if one specific call fails.

**Designer constraint:** v1 hook-install actions are read-only (show what's wired, link to docs). Writing settings.json from the browser would need a backend route that doesn't exist yet.

## 9. Accessibility baseline

| Check | Current | Required |
|---|---|---|
| Keyboard nav | Sidebar links work via Tab; routes don't yet have arrow-key nav | `g i` / `g s` / `g /` / `g p` / `g t` / `g o` global shortcuts (Step 7); arrow keys in lists |
| Focus ring | Default browser ring | Explicit `outline-2 outline-[--color-accent] outline-offset-2` on every interactive |
| Screen reader | Untested | `aria-live="polite"` on the Inbox + Substrate panels for polling updates; `aria-label` on every icon-only button |
| Colour contrast | Token system uses near-black + light-grey ink — passes WCAG AA on body copy; `--color-ink-dim` on `--color-surface-2` is borderline (3.8:1, needs verification) | All body copy AA; large text AAA where feasible; verify `--color-ink-dim` + audit any usage on tertiary surfaces |
| Motion | None yet | Respect `prefers-reduced-motion: reduce` — disable the new-item border-pulse animation when set |
| ARIA roles | Implicit only | Proper `role="navigation"` on sidebar, `role="main"` on content, `role="status"` on health badge |

## 10. Constraints

**Stack (immutable for this brief):**
- Vite 6 + React 19 (StrictMode)
- Tailwind v4 with `@theme` and `@utility` directives — NO `@apply` with custom classes, NO PostCSS plugins
- TanStack Router (file-based, code-split per route) + TanStack Query
- Component library: shadcn-style primitives owned in-tree under `web/src/components/ui` — no third-party UI kit
- Icons: `lucide-react` only (already a dep); no other icon set
- State: local React state + TanStack Query cache; `zustand` available but not required for v1

**Design tokens (immutable for this brief, see `web/src/styles.css`):**
- Surface stack `#0a0a0a` → `#2a2a2a` (5 steps)
- Accent `#00d4aa` (single accent across the app)
- Urgency `#ff6b6b` / `#ffd166` / `#52525b` for now / soon / later
- Inter (UI) + JetBrains Mono (IDs, JSON, timestamps)
- Radius `0.75rem` (card), `0.5rem` (control)
- Border `#27272a` (default), `#3f3f46` (strong)

**Performance budget:**
- TTI ≤ 1.5s on a modern laptop with localhost substrate
- Initial JS bundle ≤ 400 KB pre-gzip / 130 KB gzipped (currently 354 KB / 111 KB)
- Per-route lazy chunks ≤ 50 KB
- No layout shift on poll refresh — use skeletons that match final card heights

**Substrate API constraints:**
- All endpoints return JSON; no streaming yet
- `retrieve` is scoped to one `session_id` per call → cross-session views fan out client-side
- `metadata_filter` is exact-equality only (no range, no regex)
- `top_k` capped at 50 per call (substrate-side; designer doesn't decide this)
- Default embedding produces uniform similarity scores; don't lean on `similarity` for rank UX in v1

## 11. Success metrics

| Flavor | Metric |
|---|---|
| User outcome | Owner can answer "what is Claude waiting on me for?" in < 5 seconds from page load (Inbox) |
| User outcome | Owner can recall a learning from any past session in < 30 seconds (Search) |
| Behavioral | Owner opens Inbox at least once per active-session-day |
| Behavioral | Owner uses Tools page at least once when investigating a substrate bug (replaces curl) |
| Technical | TTI ≤ 1.5s p90 on localhost substrate |
| Technical | Inbox poll refresh causes 0 layout shift |
| Technical | Substrate-unreachable state degrades gracefully (no white-screen, no spinner stuck) |

All metrics are **proposed — confirm with the user**, since no formal PM exists for this single-user tool.

## 12. References

- **Linear inbox** — for the "items grouped by source, sorted by urgency, ack-to-dismiss" pattern. Note: Linear's inbox is heavier (full activity feed); ContextNest's is narrower (only items needing user action). Borrow the visual rhythm, not the scope.
- **GitHub Actions run summary card** — for density + status-at-a-glance treatment, especially for `/sessions` and `/substrate`. Good reference for "alive but degraded" visual states.
- **Postman / Insomnia** — for the `/tools` playground feel (request/response split, JSON editor, monospace dominance).
- **Obsidian command palette + graph view** — for the cross-session `/search` semantics. Obsidian's panel-based density is closer to the target than typical SaaS search.
- **Linear command palette (`Cmd+K`)** — pattern for the global keyboard shortcuts in Step 7.

## 13. Out of scope

- **Mobile layout** — desktop-only, single-user tool. Browser width < 1024 px shows a "ContextNest dashboard is desktop-only" message. No responsive design.
- **Light theme** — `@theme` tokens are dark-only by design. Light mode is a v2 consideration if anyone asks for it; for v1 the `<meta name="color-scheme" content="dark" />` is locked.
- **Auth / multi-user** — single substrate, single owner, localhost-first. No login flow, no permission model, no per-user data isolation.
- **Audit log / history** — the substrate stores fragments; the dashboard reads them. There's no "who did what when" trail because there's one user.
- **Real-time SSE updates** — the substrate doesn't expose an SSE endpoint. UI polls at 30 s. SSE may land as a substrate follow-up; not a design dependency for v1.
- **Hook configuration writes from the browser** — `/substrate` shows hook state read-only in v1; writing settings.json requires a backend route that doesn't exist.
- **Theming / customization** — single visual identity; no user-controlled accent picker, no font picker.
- **i18n** — English-only. The substrate is a developer tool with English-only content.
- **Export / import of memories** — there's no JSON export from the dashboard in v1. Power users can hit the API directly.

## 14. Open questions for PM (= the substrate owner)

1. **`/phases` visual language:** Sparkline? Stacked dots? Horizontal accordion? Vertical timeline (proposed)? Pick one for v1 and defer the alternatives.
2. **Inbox card vs row default:** Cards (proposed) or table rows (denser)? A compact toggle in v1 or deferred to v2?
3. **`[ack]` semantics:** Should ack'ing an inbox item *delete* the underlying memory or *tag it as acked* (proposed — non-destructive, reversible)? The latter is consistent with substrate's soft-delete approach.
4. **Polling interval:** 30 s default (proposed) or per-route configurable? Designer may want a "refresh now" button in addition.
5. **Empty-state copy ownership:** Designer proposes copy; owner reviews. Confirm.
6. **Substrate-unreachable copy:** Should the dashboard suggest a literal restart command (`./target/release/contextnest serve --bind 127.0.0.1:28080`)? Helpful for the single-user case but couples the UI to deploy-time specifics.

## 15. Definition of done (for the design phase)

- [ ] Hi-fi mocks of every route's success state (Inbox, Sessions list, Session detail, Search, Phases, Tools, Substrate)
- [ ] Hi-fi mocks of every state in §8 — at minimum cold, empty (zero data), empty (substrate down), partial, error
- [ ] Component spec for the Inbox card (the key reusable visual) with all variants (now / soon / later / decision / ack'd / new)
- [ ] Component spec for the Sessions row (with sparkline if used)
- [ ] Component spec for the Tools playground request/response editor
- [ ] Token usage table per component (which `--color-*` / `--font-*` / `--radius-*` for what)
- [ ] Empty-state copy for every route + every kind ("no decisions in this session", "no goal phases yet", etc.)
- [ ] `data-testid` naming scheme — convention: `cn-<feature>-<element>`, e.g. `cn-inbox-card`, `cn-sessions-row-actions`, `cn-tools-send-button`
- [ ] Accessibility annotations in mocks: focus order, aria roles, motion preferences
- [ ] Keyboard-shortcut map (`g i`, `g s`, `g /`, etc.) — confirm with engineer before locking the bindings
- [ ] Decision documented for §14 question #1 (`/phases` visual language) — that's the one open design call
- [ ] Mobile note: explicit "not in scope, shows desktop-only message"
