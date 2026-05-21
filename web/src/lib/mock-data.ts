import type { Urgency } from '@/components/atoms';

export type SessionMock = {
  id: string;
  project: string;
  cwd: string;
  lastActivity: string;
  lastActivityMs: number;
  phase: string;
  counts: {
    memories: number;
    goal_phases: number;
    decisions: number;
    blockers: number;
    learnings: number;
    todos: number;
  };
  sparkline: number[];
  urgency: Urgency;
};

export type InboxItemMock = {
  id: string;
  sessionId: string;
  project: string;
  kind: 'user_action' | 'decision' | 'todo';
  awaiting?: boolean;
  urgency: Urgency;
  action: string;
  reason: string;
  decision?: string;
  step: number;
  stored: string;
  ago: string;
  isNew?: boolean;
};

export type PhaseMock = {
  title: string;
  sessionId: string;
  project: string;
  span: string;
  duration: string;
  turns: number;
  cluster: number;
  facts: string[];
  time: string;
};

export type SessionDetailMock = {
  session: SessionMock;
  goalPhases: Array<{
    title: string;
    span: string;
    accs: string[];
    counts: { decisions: number; blockers: number; learnings: number };
  }>;
  accomplishments: Array<{ text: string; t: string }>;
  learnings: Array<{ text: string; t: string }>;
  todos: Array<{
    text: string;
    status: 'pending' | 'in_progress' | 'completed';
    t: string;
  }>;
  decisions: Array<{ text: string; awaiting?: boolean; t: string }>;
  blockers: Array<{ text: string; t: string }>;
  userActions: Array<{ text: string; urgency: Urgency; t: string }>;
};

export type SearchResultMock = {
  kind: string;
  sessionId: string;
  project: string;
  similarity: number;
  snippet: string;
  stored: string;
};

export type ActivityMock = {
  t: string;
  verb: 'store' | 'retrieve' | 'discard' | 'update' | 'summarize' | 'reconstruct' | 'resonate';
  target: string;
  ms: number;
  ok: boolean;
};

export type KindCountMock = {
  k: string;
  n: number;
  color: string;
};

const sessions: SessionMock[] = [
  {
    id: 'cc-7f3a2e91',
    project: 'contextnest',
    cwd: '~/code/contextnest',
    lastActivity: '12 min ago',
    lastActivityMs: 12 * 60 * 1000,
    phase: 'Wire hook receiver to ingest pipeline',
    counts: { memories: 142, goal_phases: 4, decisions: 7, blockers: 1, learnings: 18, todos: 6 },
    sparkline: [2, 3, 1, 4, 6, 5, 8, 12, 9, 7, 11, 14, 10, 6, 4, 7, 9, 12, 15, 11, 8, 4, 2, 3],
    urgency: 'now',
  },
  {
    id: 'cc-a812bc40',
    project: 'z-insight',
    cwd: '~/code/z-insight',
    lastActivity: '1h 8m ago',
    lastActivityMs: 68 * 60 * 1000,
    phase: 'Refactor extractor to emit awaiting_decision flag',
    counts: { memories: 78, goal_phases: 3, decisions: 4, blockers: 0, learnings: 12, todos: 3 },
    sparkline: [3, 5, 7, 9, 11, 8, 4, 2, 1, 3, 6, 8, 10, 12, 9, 7, 5, 3, 1, 2, 4, 6, 5, 3],
    urgency: 'soon',
  },
  {
    id: 'cc-d4f0e8b2',
    project: 'ratchet',
    cwd: '~/code/ratchet',
    lastActivity: 'yesterday',
    lastActivityMs: 26 * 60 * 60 * 1000,
    phase: 'Postgres migration: 0007 idempotency keys',
    counts: { memories: 211, goal_phases: 6, decisions: 12, blockers: 2, learnings: 24, todos: 9 },
    sparkline: [8, 12, 15, 18, 14, 11, 9, 7, 4, 3, 2, 1, 4, 6, 8, 11, 13, 10, 7, 5, 3, 6, 9, 11],
    urgency: 'soon',
  },
  {
    id: 'cc-19883c5d',
    project: 'contextnest',
    cwd: '~/code/contextnest',
    lastActivity: '3d ago',
    lastActivityMs: 3 * 24 * 60 * 60 * 1000,
    phase: 'Embedding provider switch (TF-IDF → Ollama)',
    counts: { memories: 96, goal_phases: 4, decisions: 5, blockers: 1, learnings: 14, todos: 2 },
    sparkline: [1, 2, 4, 7, 9, 11, 14, 12, 8, 5, 3, 2, 4, 6, 8, 9, 7, 4, 2, 1, 3, 5, 7, 6],
    urgency: 'later',
  },
  {
    id: 'cc-602fe917',
    project: 'scratch-llm',
    cwd: '~/code/scratch-llm',
    lastActivity: '6d ago',
    lastActivityMs: 6 * 24 * 60 * 60 * 1000,
    phase: 'Spike: KV cache invalidation strategies',
    counts: { memories: 34, goal_phases: 2, decisions: 1, blockers: 0, learnings: 5, todos: 0 },
    sparkline: [4, 6, 8, 5, 3, 2, 1, 0, 0, 2, 4, 3, 5, 6, 4, 2, 1, 1, 0, 0, 0, 0, 0, 0],
    urgency: 'later',
  },
  {
    id: 'cc-bbe71a08',
    project: 'contextnest',
    cwd: '~/code/contextnest',
    lastActivity: '11d ago',
    lastActivityMs: 11 * 24 * 60 * 60 * 1000,
    phase: 'Initial scaffold + brand tokens',
    counts: { memories: 58, goal_phases: 3, decisions: 3, blockers: 0, learnings: 8, todos: 1 },
    sparkline: [0, 0, 2, 5, 9, 12, 8, 4, 2, 1, 3, 5, 4, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    urgency: 'later',
  },
];

const inbox: InboxItemMock[] = [
  {
    id: 'frag_8a91c0d2',
    sessionId: 'cc-7f3a2e91',
    project: 'contextnest',
    kind: 'user_action',
    urgency: 'now',
    action:
      'Decide whether the hook receiver should backpressure when ingest queue depth > 500, or drop with a counter.',
    reason:
      'Ingest queue ran past 800 in the last 24h; we paused tail to recover. A policy here unblocks the v0.2 release.',
    step: 1,
    stored: '2026-05-20T14:18:11Z',
    ago: '8 min ago',
    isNew: true,
  },
  {
    id: 'frag_19e8a704',
    sessionId: 'cc-7f3a2e91',
    project: 'contextnest',
    kind: 'decision',
    awaiting: true,
    urgency: 'now',
    action:
      'Pick metadata_filter operator for v0.2: AND-only (current) or AND/OR with explicit groups?',
    reason:
      'Two callers already worked around AND-only by issuing 2 retrieves. AND/OR adds ~80 LoC + a parser; AND-only is simpler.',
    decision: 'AND-only with documented client-merge pattern · vs · AND/OR groups',
    step: 2,
    stored: '2026-05-20T13:55:30Z',
    ago: '32 min ago',
    isNew: true,
  },
  {
    id: 'frag_4400ab91',
    sessionId: 'cc-a812bc40',
    project: 'z-insight',
    kind: 'user_action',
    urgency: 'soon',
    action:
      'Review extractor diff in `src/ingest/claude_code/extractor.rs:412` — emits awaiting_decision on every decision; should it be the default?',
    reason:
      'Backfill of past sessions would flip ~40 historical decisions to awaiting_decision=true, polluting Inbox.',
    step: 1,
    stored: '2026-05-20T13:12:00Z',
    ago: '1h 15m ago',
  },
  {
    id: 'frag_82c0d3ee',
    sessionId: 'cc-a812bc40',
    project: 'z-insight',
    kind: 'user_action',
    urgency: 'soon',
    action: 'Add a test fixture for the empty-tool-result edge case Claude hit on turn 47.',
    reason: 'Without it, the extractor panics on a JSON null tool_result during backfill.',
    step: 2,
    stored: '2026-05-20T13:08:11Z',
    ago: '1h 19m ago',
  },
  {
    id: 'frag_d11b2900',
    sessionId: 'cc-d4f0e8b2',
    project: 'ratchet',
    kind: 'decision',
    awaiting: true,
    urgency: 'soon',
    action: "Confirm 0007 migration is non-destructive on prod schema before tonight's deploy.",
    reason: 'ALTER TABLE idempotency_keys ADD COLUMN expires_at — backfill plan attached.',
    decision: 'Deploy as-is · vs · Stage on read-replica first',
    step: 1,
    stored: '2026-05-19T22:11:00Z',
    ago: 'yesterday',
  },
  {
    id: 'frag_3920bcaa',
    sessionId: 'cc-d4f0e8b2',
    project: 'ratchet',
    kind: 'user_action',
    urgency: 'later',
    action: 'Pick a logging crate — tracing (proposed) or log + env_logger?',
    reason: "I started in tracing; happy to switch if you'd rather standardize across your repos.",
    step: 1,
    stored: '2026-05-19T17:44:20Z',
    ago: 'yesterday',
  },
  {
    id: 'frag_771e0a14',
    sessionId: 'cc-19883c5d',
    project: 'contextnest',
    kind: 'user_action',
    urgency: 'later',
    action: 'Provide an OPENAI_API_KEY (or Ollama URL) so I can wire real embeddings.',
    reason:
      'TF-IDF default works but similarity scores are uniform ~0.99; useful UX needs a real embedder.',
    step: 1,
    stored: '2026-05-17T11:00:00Z',
    ago: '3d ago',
  },
];

const phases: PhaseMock[] = [
  {
    title: 'Wire hook receiver to ingest pipeline',
    sessionId: 'cc-7f3a2e91',
    project: 'contextnest',
    span: '12:14 → 14:18',
    duration: '2h 04m',
    turns: 14,
    cluster: 6,
    facts: [
      'POST /api/v1/cc/hook/<event> returns 204 within ~3ms (target was <10ms)',
      'Tail+ingest is async; queue depth is the back-pressure signal.',
    ],
    time: 'today · 12:14',
  },
  {
    title: 'Refactor extractor to emit awaiting_decision flag',
    sessionId: 'cc-a812bc40',
    project: 'z-insight',
    span: '10:30 → 12:08',
    duration: '1h 38m',
    turns: 9,
    cluster: 4,
    facts: [
      'awaiting_decision is set only when extractor sees a question form + no follow-up answer in next 3 turns',
      'Heuristic agrees with hand-labelled set 28/30 cases',
    ],
    time: 'today · 10:30',
  },
  {
    title: '0007 migration: idempotency_keys.expires_at',
    sessionId: 'cc-d4f0e8b2',
    project: 'ratchet',
    span: 'yesterday 19:00 → 22:11',
    duration: '3h 11m',
    turns: 21,
    cluster: 8,
    facts: [
      'Backfill plan: 1M rows at 5k/batch, ~3.5 min on staging',
      "Default to NOW() + interval '30 days' for legacy rows",
    ],
    time: 'yesterday · 19:00',
  },
  {
    title: 'Embedding provider switch (TF-IDF → Ollama)',
    sessionId: 'cc-19883c5d',
    project: 'contextnest',
    span: '3d ago 09:42 → 11:07',
    duration: '1h 25m',
    turns: 11,
    cluster: 5,
    facts: [
      'Ollama nomic-embed-text 768-d ran at 11 q/s on M2',
      'Cosine similarity over 5k fragments takes 22ms server-side',
    ],
    time: '3d ago · 09:42',
  },
  {
    title: 'Spike: KV cache invalidation strategies',
    sessionId: 'cc-602fe917',
    project: 'scratch-llm',
    span: '6d ago 14:00 → 15:18',
    duration: '1h 18m',
    turns: 7,
    cluster: 3,
    facts: [
      'LRU + max-tokens is the simplest correct choice for our workload',
      'Sliding-window adds 12% memory for <2% hit-rate gain',
    ],
    time: '6d ago · 14:00',
  },
  {
    title: 'Initial scaffold + brand tokens',
    sessionId: 'cc-bbe71a08',
    project: 'contextnest',
    span: '11d ago 16:20 → 18:01',
    duration: '1h 41m',
    turns: 13,
    cluster: 6,
    facts: [
      'Cyan-teal #00d4aa locked as single accent',
      'Inter + JetBrains Mono pairing committed',
    ],
    time: '11d ago · 16:20',
  },
];

const sessionDetail: SessionDetailMock = {
  session: sessions[0],
  goalPhases: [
    {
      title: 'Wire hook receiver to ingest pipeline',
      span: '12:14 → 14:18 · 2h 04m · 14 turns',
      accs: [
        'POST /api/v1/cc/hook/<event> returns 204 in ~3ms p50',
        'Async tail+ingest pulls from /tmp/cc-hook-<sid>.jsonl',
        'Queue depth counter exposed at /api/status',
      ],
      counts: { decisions: 2, blockers: 1, learnings: 5 },
    },
    {
      title: 'Add metadata_filter to retrieve',
      span: '10:42 → 12:14 · 1h 32m · 11 turns',
      accs: [
        'Exact-equality, AND across keys, applied AFTER similarity',
        'Bench: 8ms p50 over 10k fragments',
      ],
      counts: { decisions: 1, blockers: 0, learnings: 3 },
    },
    {
      title: 'Inbox CLI subcommand',
      span: 'yesterday 16:00 → 18:30 · 2h 30m · 18 turns',
      accs: [
        'contextnest inbox prints kind=user_action + awaiting decisions',
        'Sorted by urgency: now → soon → later',
      ],
      counts: { decisions: 2, blockers: 0, learnings: 4 },
    },
  ],
  accomplishments: [
    {
      text: 'Hook receiver lands new memories in ~3ms p50 from claude turn → substrate.',
      t: '14:18',
    },
    { text: 'metadata_filter AND-mode passes the 12-case acceptance suite.', t: '12:09' },
    { text: 'Inbox CLI ships behind a feature flag.', t: 'y · 18:11' },
  ],
  learnings: [
    {
      text: "Tokio's mpsc unbounded is the wrong default when ingest can outpace the consumer — switched to bounded(1024) + drop counter.",
      t: '13:55',
    },
    {
      text: 'axum extractor for application/jsonl needs a custom Body reader; the built-in JsonExtractor errors on multi-line.',
      t: '11:08',
    },
    {
      text: "TanStack Query's `placeholderData: keepPreviousData` avoids the layout shift on poll refresh.",
      t: '10:12',
    },
  ],
  todos: [
    { text: 'Decide hook receiver back-pressure policy', status: 'in_progress', t: '13:55' },
    { text: 'Add /api/v1/cc/hook/healthcheck endpoint', status: 'pending', t: '12:30' },
    { text: 'Bench substrate at 50k fragments', status: 'completed', t: 'y · 17:00' },
    { text: 'Document hook install for fish + zsh', status: 'pending', t: 'y · 16:42' },
  ],
  decisions: [
    {
      text: 'metadata_filter is AND-only for v0.2; document the client-side merge pattern.',
      awaiting: true,
      t: '13:55',
    },
    {
      text: "Soft-delete is the substrate's only delete; discard tags `discarded=true`.",
      t: 'y · 17:11',
    },
  ],
  blockers: [
    {
      text: 'Hook receiver drops events when queue > 1024; need back-pressure decision.',
      t: '13:55',
    },
  ],
  userActions: [
    {
      text: 'Decide hook receiver back-pressure: backpressure vs drop+counter',
      urgency: 'now',
      t: '14:18',
    },
    { text: 'Confirm AND-only metadata_filter for v0.2', urgency: 'now', t: '13:55' },
  ],
};

const searchResults: SearchResultMock[] = [
  {
    kind: 'learning',
    sessionId: 'cc-7f3a2e91',
    project: 'contextnest',
    similarity: 0.92,
    snippet:
      "Tokio's <mark>mpsc unbounded</mark> is the wrong default when ingest can outpace the consumer — switched to bounded(1024) + drop counter.",
    stored: '2026-05-20 · 13:55',
  },
  {
    kind: 'learning',
    sessionId: 'cc-d4f0e8b2',
    project: 'ratchet',
    similarity: 0.81,
    snippet:
      'Postgres <mark>idempotency</mark>: ALTER ADD COLUMN with default NOW() rewrites the whole table — staged backfill is cheaper than a single migration.',
    stored: '2026-05-19 · 21:08',
  },
  {
    kind: 'decision',
    sessionId: 'cc-7f3a2e91',
    project: 'contextnest',
    similarity: 0.76,
    snippet:
      "<mark>Soft-delete</mark> is the substrate's only delete; discard tags discarded=true rather than removing the fragment.",
    stored: '2026-05-19 · 17:11',
  },
  {
    kind: 'learning',
    sessionId: 'cc-19883c5d',
    project: 'contextnest',
    similarity: 0.71,
    snippet:
      'Ollama <mark>nomic-embed-text</mark> ran at 11 q/s on M2; cosine similarity over 5k fragments takes 22ms server-side.',
    stored: '2026-05-17 · 10:24',
  },
  {
    kind: 'fact',
    sessionId: 'cc-a812bc40',
    project: 'z-insight',
    similarity: 0.68,
    snippet:
      'Extractor sets <mark>awaiting_decision</mark> when a question form appears with no answer in next 3 turns; matches 28/30 hand-labelled.',
    stored: '2026-05-20 · 11:31',
  },
];

const toolTemplates: Record<string, unknown> = {
  store: {
    content:
      'Hook receiver back-pressure should bound mpsc to 1024 and increment a drop counter on overflow.',
    kind: 'learning',
    session_id: 'cc-7f3a2e91',
    metadata: {
      project_cwd: '~/code/contextnest',
      urgency: 'now',
      src_session: 'cc-7f3a2e91',
      step: 1,
    },
  },
  retrieve: {
    session_id: 'cc-7f3a2e91',
    query: 'back-pressure policy',
    top_k: 10,
    metadata_filter: { kind: 'learning' },
  },
  update: {
    fragment_id: 'frag_8a91c0d2',
    metadata: { awaiting_decision: false, resolved_at: '2026-05-20T14:30:00Z' },
  },
  summarize: { session_id: 'cc-7f3a2e91', window: { kind: 'goal_phase', limit: 5 } },
  discard: { fragment_id: 'frag_19e8a704', reason: 'duplicate' },
  reconstruct: {
    session_id: 'cc-7f3a2e91',
    from: '2026-05-20T12:00:00Z',
    to: '2026-05-20T14:18:00Z',
  },
  resonate: { session_id: 'cc-7f3a2e91', seed_fragment: 'frag_8a91c0d2', depth: 2, top_k: 8 },
};

const toolResponses: Record<string, unknown> = {
  store: {
    fragment_id: 'frag_91ab02de',
    stored_at: '2026-05-20T14:30:11.244Z',
    embedding_dim: 256,
    metadata_applied: {
      project_cwd: '~/code/contextnest',
      urgency: 'now',
      src_session: 'cc-7f3a2e91',
      step: 1,
      kind: 'learning',
    },
  },
  retrieve: {
    hits: [
      {
        fragment_id: 'frag_a01dee92',
        similarity: 0.991,
        content: 'Tokio mpsc unbounded is the wrong default when ingest can outpace the consumer.',
        metadata: { kind: 'learning', stored_at: '2026-05-20T13:55:30Z', step: 3 },
      },
      {
        fragment_id: 'frag_b78c0a14',
        similarity: 0.988,
        content: 'Bounded(1024) + drop counter is the agreed policy direction.',
        metadata: { kind: 'learning', stored_at: '2026-05-20T13:57:08Z', step: 4 },
      },
    ],
    count: 2,
    query_embedding_dim: 256,
    filtered_pre_top_k: 14,
  },
};

const recentActivity: ActivityMock[] = [
  { t: '14:30:11', verb: 'store', target: 'cc-7f3a2e91/frag_91ab02de', ms: 4, ok: true },
  { t: '14:30:08', verb: 'retrieve', target: 'cc-7f3a2e91 · k=10', ms: 12, ok: true },
  { t: '14:30:00', verb: 'retrieve', target: 'cc-a812bc40 · k=10', ms: 9, ok: true },
  { t: '14:29:58', verb: 'retrieve', target: 'cc-d4f0e8b2 · k=10', ms: 11, ok: true },
  { t: '14:28:31', verb: 'store', target: 'cc-7f3a2e91/frag_88a04c10', ms: 3, ok: true },
  { t: '14:28:11', verb: 'store', target: 'cc-7f3a2e91/frag_771a8c00', ms: 4, ok: true },
  { t: '14:27:40', verb: 'retrieve', target: 'cc-7f3a2e91 · k=20', ms: 18, ok: true },
  { t: '14:25:14', verb: 'discard', target: 'frag_440b3911', ms: 6, ok: true },
  { t: '14:24:01', verb: 'store', target: 'cc-a812bc40/frag_2099aa01', ms: 5, ok: true },
  { t: '14:23:50', verb: 'retrieve', target: 'cc-7f3a2e91 · k=10', ms: 13, ok: true },
];

const kindCounts: KindCountMock[] = [
  { k: 'learning', n: 142, color: '#a78bfa' },
  { k: 'accomplishment', n: 98, color: '#00d4aa' },
  { k: 'fact', n: 84, color: '#a1a1aa' },
  { k: 'decision', n: 41, color: '#ffd166' },
  { k: 'user_action', n: 28, color: '#ff6b6b' },
  { k: 'todo', n: 22, color: '#60a5fa' },
  { k: 'goal_phase', n: 18, color: '#f472b6' },
  { k: 'blocker', n: 9, color: '#ff6b6b' },
  { k: 'session_title', n: 6, color: '#a1a1aa' },
  { k: 'ack', n: 4, color: '#71717a' },
];

export const MOCK = {
  sessions,
  inbox,
  phases,
  sessionDetail,
  searchResults,
  toolTemplates,
  toolResponses,
  recentActivity,
  kindCounts,
};
