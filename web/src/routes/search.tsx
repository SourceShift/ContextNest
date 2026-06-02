import { createFileRoute } from '@tanstack/react-router';
import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { Icon, KindBadge, ProjBadge, SessionPill } from '@/components/atoms';
import { api } from '@/lib/api';
import { useSessions, useKnownProjects } from '@/hooks/useSessions';
import type { FeatureHit, FileMatch, RetrieveHit } from '@/lib/types';

/**
 * Search modes.
 *
 * - `memories` — original behaviour. Fragment-level snippet hits with
 *   per-row similarity. Best when you want to skim the actual content.
 * - `sessions` — group fragments by their owning session. Shows one
 *   card per matching session, ranked by best hit + summed weight,
 *   with the top snippets nested inside. Best for the daily-driver
 *   question "WHICH session was I working on X?"
 * - `features` — substring search across every session's declared
 *   `delivered_features[].feature` names. Best when you remember
 *   what you SHIPPED but not which session shipped it.
 */
type SearchMode = 'memories' | 'sessions' | 'features' | 'files';
const MODE_LABELS: Record<SearchMode, string> = {
  memories: 'Memories',
  sessions: 'Sessions',
  features: 'Features',
  files: 'Files',
};
const MODE_HINTS: Record<SearchMode, string> = {
  memories: 'Per-fragment snippet view — best for skimming raw content.',
  sessions:
    'Grouped by session — best for "WHICH session was I working on X?". Each card links to that session.',
  features:
    'Substring search over agent-declared feature names — best when you remember what you SHIPPED but not which session shipped it.',
  files:
    'Substring search over files_touched across every session — best when you remember the FILE you edited but not which session edited it. Sessions with the file as their only edit (total=1) are likely the surgical authoring sessions.',
};

/**
 * URL search params for the search page. Both fields optional so
 * a bare `/search` visit still works.
 *
 * Deep-links from elsewhere in the dashboard (e.g. the /sessions
 * page's topic-search callout) populate these so the user lands
 * on a pre-filled, mode-correct view.
 */
type SearchParams = {
  q?: string;
  mode?: SearchMode;
};

export const Route = createFileRoute('/search')({
  component: SearchPage,
  validateSearch: (raw: Record<string, unknown>): SearchParams => {
    const out: SearchParams = {};
    if (typeof raw.q === 'string' && raw.q.length > 0) out.q = raw.q;
    if (
      typeof raw.mode === 'string' &&
      (raw.mode === 'memories' ||
        raw.mode === 'sessions' ||
        raw.mode === 'features' ||
        raw.mode === 'files')
    ) {
      out.mode = raw.mode;
    }
    return out;
  },
});

type Chip = { k: string; v: string };
const CHIP_KEYS = ['kind', 'project', 'session', 'urgency'] as const;
type ChipKey = (typeof CHIP_KEYS)[number];

// Static menu of kinds + urgencies (these are the substrate's enums,
// not data values). Project / session lists are populated live from
// useSessions / useKnownProjects so they reflect the actual substrate.
const STATIC_VALUES: Partial<Record<ChipKey, string[]>> = {
  kind: [
    'learning',
    'accomplishment',
    'decision',
    'blocker',
    'todo',
    'user_action',
    'goal_phase',
    'state',
    'current_task',
    'summary',
  ],
  urgency: ['now', 'soon', 'later'],
};

type SortMode = 'similarity' | 'newest' | 'oldest' | 'importance';
type DateRange = 'all' | '1h' | '24h' | '7d' | '30d';
const SORT_LABELS: Record<SortMode, string> = {
  similarity: 'similarity (best match)',
  newest: 'newest first',
  oldest: 'oldest first',
  importance: 'importance (high → low)',
};
const DATE_LABELS: Record<DateRange, string> = {
  all: 'any time',
  '1h': 'last hour',
  '24h': 'last 24 hours',
  '7d': 'last 7 days',
  '30d': 'last 30 days',
};
const DATE_MS: Record<DateRange, number> = {
  all: 0,
  '1h': 60 * 60 * 1000,
  '24h': 24 * 60 * 60 * 1000,
  '7d': 7 * 24 * 60 * 60 * 1000,
  '30d': 30 * 24 * 60 * 60 * 1000,
};

function tsMs(ts: unknown): number {
  if (typeof ts !== 'string' || !ts) return -Infinity;
  const t = Date.parse(ts);
  return Number.isFinite(t) ? t : -Infinity;
}

function basename(p: string | null | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

function shortTs(ts: string | undefined): string {
  if (!ts) return '';
  // ISO → 2026-05-21 · 09:30
  const m = ts.match(/^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})/);
  return m ? `${m[1]} · ${m[2]}` : ts.slice(0, 16);
}

// Light highlight — wraps every occurrence of the query (case-insensitive)
// in <mark>. Not robust HTML escaping; the substrate's content is
// trusted user-written text from the same machine.
function highlight(text: string, query: string): string {
  if (!query) return text;
  const safeQuery = query.trim();
  if (!safeQuery) return text;
  try {
    const re = new RegExp(safeQuery.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'gi');
    return text.replace(re, (m) => `<mark>${m}</mark>`);
  } catch {
    return text;
  }
}

type SearchResultRow = RetrieveHit & {
  session_id: string;
  project: string;
};

/**
 * Fragment kinds that every session has and which contain boilerplate
 * substrings ("[user turn N] ..." etc.) that systematically dominate
 * topic queries via shared tokens under the TF-IDF embedder.
 *
 * Excluded by default in topic-search modes (`sessions`). Memories
 * mode keeps everything because that's the explicit "show me every
 * fragment" view. Users can re-include via the
 * `Include boilerplate` toggle when they specifically WANT to find a
 * session by its opening user prompt.
 */
const NOISE_KINDS_FOR_TOPIC_SEARCH = ['initial_prompt_window'];

function SearchPage() {
  // Pre-fill from URL search params on first render so deep-links
  // from elsewhere in the dashboard (e.g. /sessions topic-search
  // callout) land on the right mode + query without an extra click.
  const params = Route.useSearch();
  const [mode, setMode] = useState<SearchMode>(params.mode ?? 'sessions');
  const [q, setQ] = useState(params.q ?? '');
  // ON by default: noise kinds are dropped server-side in topic-search
  // modes. The "search is garbage" rage came from these dominating
  // Sessions-mode results. Power users can flip OFF to opt back in.
  const [hideBoilerplate, setHideBoilerplate] = useState(true);
  // `qDebounced` is what we hand to React Query as the query key. Keeping
  // it separate from `q` (which drives the input) means the input stays
  // snappy while the actual retrieve fires at most once per ~250ms.
  // Combined with the substrate's single-call cross-session retrieve,
  // this reduces a fast typist's 6+ keystrokes worth of fan-out from
  // 60+ HTTP requests down to 1.
  const [qDebounced, setQDebounced] = useState('');
  const [chips, setChips] = useState<Chip[]>([]);
  const [sortMode, setSortMode] = useState<SortMode>('similarity');
  const [limit, setLimit] = useState<number>(50);
  const [dateRange, setDateRange] = useState<DateRange>('all');
  const [focused, setFocused] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const sessions = useSessions();
  const knownProjects = useKnownProjects();

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  useEffect(() => {
    const t = setTimeout(() => setQDebounced(q), 250);
    return () => clearTimeout(t);
  }, [q]);

  const removeChip = (i: number) => setChips((c) => c.filter((_, j) => j !== i));
  const addChip = (k: string, v: string) =>
    // Dedup: don't add the same {k, v} twice.
    setChips((c) => (c.some((x) => x.k === k && x.v === v) ? c : [...c, { k, v }]));

  // Group chips by category so we can build OR-sets per category.
  const chipsByKey = useMemo(() => {
    const m: Record<ChipKey, string[]> = {
      kind: [],
      project: [],
      session: [],
      urgency: [],
    };
    for (const c of chips) {
      if ((CHIP_KEYS as readonly string[]).includes(c.k)) {
        m[c.k as ChipKey].push(c.v);
      }
    }
    return m;
  }, [chips]);

  // Build the metadata_filter the substrate's retrieve handler accepts.
  // BE filter is strict-equality AND-across-keys. We can ONLY send
  // category=X to the BE when there's EXACTLY ONE chip in that category
  // (single value). Multi-value categories ("kind=learning OR
  // kind=decision") fall through to client-side post-filtering.
  // Project is always client-side (chip stores basename, BE wants
  // project_cwd path).
  const metadataFilter: Record<string, unknown> = useMemo(() => {
    const f: Record<string, unknown> = {};
    if (chipsByKey.kind.length === 1) f.kind = chipsByKey.kind[0];
    if (chipsByKey.urgency.length === 1) f.urgency = chipsByKey.urgency[0];
    return f;
  }, [chipsByKey]);

  // Build a fragment_id → project lookup so we can stamp `project` onto
  // each hit cheaply. The substrate gives us the owning session_id back
  // on each hit (in cross-session mode); we then resolve that to a
  // project_cwd via the session list we already have cached.
  const projectBySession = useMemo(() => {
    const m = new Map<string, string>();
    for (const s of sessions.data) m.set(s.id, basename(s.project_cwd));
    return m;
  }, [sessions.data]);

  // Pick target sessions only when chips actually scope the query. With no
  // session/project chips, omit both session_id and session_ids so the backend
  // performs global search from its own session index.
  const targetSessionIds: string[] = useMemo(() => {
    if (chipsByKey.session.length > 0) return chipsByKey.session;
    if (chipsByKey.project.length > 0) {
      return sessions.data
        .filter((s) => chipsByKey.project.includes(basename(s.project_cwd)))
        .map((s) => s.id);
    }
    return [];
  }, [chipsByKey.project, chipsByKey.session, sessions.data]);

  // Client-side post-filter for everything BE can't enforce:
  //   1. multi-value OR within a category
  //   2. project (basename)
  //   3. date range (ts within last N ms)
  const postFilter = (rows: SearchResultRow[]): SearchResultRow[] => {
    const cutoff = dateRange === 'all' ? null : Date.now() - DATE_MS[dateRange];
    return rows.filter((r) => {
      if (chipsByKey.project.length > 0 && !chipsByKey.project.includes(r.project)) return false;
      if (chipsByKey.kind.length > 1) {
        const k = (r.metadata.kind as string | undefined) ?? '';
        if (!chipsByKey.kind.includes(k)) return false;
      }
      if (chipsByKey.urgency.length > 1) {
        const u = (r.metadata.urgency as string | undefined) ?? '';
        if (!chipsByKey.urgency.includes(u)) return false;
      }
      if (cutoff !== null) {
        const t = tsMs(r.metadata.ts);
        if (t < cutoff) return false;
      }
      return true;
    });
  };

  const applySort = (rows: SearchResultRow[]): SearchResultRow[] => {
    const arr = rows.slice();
    switch (sortMode) {
      case 'newest':
        arr.sort((a, b) => tsMs(b.metadata.ts) - tsMs(a.metadata.ts) || b.similarity - a.similarity);
        break;
      case 'oldest':
        arr.sort((a, b) => tsMs(a.metadata.ts) - tsMs(b.metadata.ts) || b.similarity - a.similarity);
        break;
      case 'importance':
        arr.sort((a, b) => b.importance - a.importance || b.similarity - a.similarity);
        break;
      case 'similarity':
      default:
        arr.sort((a, b) => b.similarity - a.similarity);
        break;
    }
    return arr;
  };

  const searchQuery = useQuery({
    queryKey: [
      'search',
      qDebounced,
      metadataFilter,
      targetSessionIds,
      // Client-side filters/sort are part of the cache key so the result
      // memo updates the moment the user flips a control, even when the
      // BE response is cached.
      chipsByKey.kind,
      chipsByKey.project,
      chipsByKey.urgency,
      dateRange,
      sortMode,
      limit,
      // Mode + hideBoilerplate change the BE request (exclude_kinds),
      // so they must be part of the cache key.
      mode,
      hideBoilerplate,
    ],
    enabled:
      qDebounced.trim().length > 0 &&
      (chipsByKey.project.length === 0 || targetSessionIds.length > 0),
    staleTime: 5_000,
    queryFn: async (): Promise<SearchResultRow[]> => {
      const singleSession = chipsByKey.session.length === 1 ? chipsByKey.session[0] : null;
      // Pull more than `limit` rows when client-side filters are active —
      // post-filter may drop a lot. Cap at 200 to keep the substrate's
      // snapshot lock + hydration cheap.
      const clientSideActive =
        chipsByKey.kind.length > 1 ||
        chipsByKey.urgency.length > 1 ||
        chipsByKey.project.length > 0 ||
        dateRange !== 'all';
      const top_k = Math.min(200, clientSideActive ? Math.max(limit * 3, 100) : limit);

      // Default-drop noise kinds in topic-search modes. The Memories
      // mode user explicitly asked for the raw-fragment view so we
      // never strip there — keep that behaviour for power-users.
      const excludeKinds =
        hideBoilerplate && mode !== 'memories'
          ? NOISE_KINDS_FOR_TOPIC_SEARCH
          : undefined;
      const res = await api.retrieve({
        query: qDebounced,
        top_k,
        ...(singleSession
          ? { session_id: singleSession }
          : chipsByKey.session.length > 1 || chipsByKey.project.length > 0
            ? { session_ids: targetSessionIds }
            : {}),
        metadata_filter:
          Object.keys(metadataFilter).length > 0 ? metadataFilter : undefined,
        exclude_kinds: excludeKinds,
      });
      const fallbackSession = singleSession ?? '';
      const rows: SearchResultRow[] = res.hits.map((hit) => {
        const sid = hit.session_id ?? fallbackSession;
        return {
          ...hit,
          session_id: sid,
          project: projectBySession.get(sid) ?? '?',
        };
      });
      const filtered = postFilter(rows);
      const sorted = applySort(filtered);
      return sorted.slice(0, limit);
    },
  });

  // `results` is computed from `searchQuery.data` via a memoised
  // identity so downstream `useMemo` dependency arrays only re-run
  // when the data array itself changes — not on every render where
  // the `??` would produce a fresh empty array.
  const results: SearchResultRow[] = useMemo(
    () => searchQuery.data ?? [],
    [searchQuery.data],
  );

  // Sessions mode rolls up the same `results` array by session_id.
  // Computed at render time — cheap, deterministic, no extra fetch.
  const sessionGroups = useMemo(() => groupBySession(results), [results]);

  // Features mode hits its own endpoint. Enabled only when in
  // features mode AND the user has typed something.
  const featuresQuery = useQuery({
    queryKey: ['sessionsByFeature', qDebounced],
    enabled: mode === 'features' && qDebounced.trim().length > 0,
    staleTime: 5_000,
    queryFn: async () => api.sessionsByFeature(qDebounced.trim()),
  });
  const featureHits: FeatureHit[] = featuresQuery.data?.hits ?? [];

  // Files mode — substring search across files_touched across every
  // session. Enabled only in files mode + non-empty query. Backend
  // already substring-matches case-insensitively.
  const filesQuery = useQuery({
    queryKey: ['sessionsByFile', qDebounced],
    enabled: mode === 'files' && qDebounced.trim().length > 0,
    staleTime: 5_000,
    queryFn: async () => api.sessionsByFile(qDebounced.trim()),
  });
  const fileMatches: FileMatch[] = filesQuery.data?.matches ?? [];

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Search</h1>
          <div className="page-sub">{MODE_HINTS[mode]}</div>
        </div>
        <div className="page-actions">
          <span className="mono dim" style={{ fontSize: 11 }}>
            cmd+/ to focus
          </span>
        </div>
      </div>

      {/* Mode toggle — the single most important UX upgrade.
          "Sessions" is the default because that's the daily-driver
          question ("which session was I working on X?"). The other
          two modes are one click away. */}
      <div className="filter-bar" style={{ paddingTop: 0, paddingBottom: 10 }}>
        {(Object.keys(MODE_LABELS) as SearchMode[]).map((m) => (
          <button
            key={m}
            className={`btn${mode === m ? ' btn-active' : ''}`}
            onClick={() => setMode(m)}
            type="button"
            title={MODE_HINTS[m]}
          >
            {MODE_LABELS[m]}
          </button>
        ))}
      </div>

      <div className="search-input">
        <Icon.Search className="icon" style={{ width: 16, height: 16 }} />
        <input
          ref={inputRef}
          value={q}
          onChange={(e) => setQ(e.target.value)}
          placeholder="search memories — try 'awaiting decisions', 'embedding choices', 'migration plan'…"
        />
        {q && (
          <button className="btn btn-ghost sm" onClick={() => setQ('')} type="button">
            <Icon.X />
          </button>
        )}
      </div>

      <div className="filter-bar" style={{ paddingTop: 12, paddingBottom: 8, flexWrap: 'wrap' }}>
        {chips.map((c, i) => (
          <span className="chip active" key={`${c.k}:${c.v}:${i}`}>
            <span style={{ color: 'var(--ink-faint)' }}>{c.k}:</span>
            {c.v}
            <span className="x" onClick={() => removeChip(i)}>
              <Icon.X />
            </span>
          </span>
        ))}
        <ChipAdder
          onAdd={addChip}
          knownProjects={knownProjects}
          knownSessions={sessions.data.map((s) => s.id).slice(0, 25)}
        />
        {chips.length > 0 && (
          <button
            className="btn btn-ghost sm"
            onClick={() => setChips([])}
            type="button"
            title="Clear all filter chips"
          >
            <Icon.X /> clear filters
          </button>
        )}
        <div className="grow" />
        <span className="mono dim" style={{ fontSize: 11 }}>
          {q
            ? searchQuery.isLoading
              ? 'searching…'
              : `${results.length} hits · ${chips.length} filters · ${
                  chipsByKey.session.length > 0 || chipsByKey.project.length > 0
                    ? `${targetSessionIds.length} session(s)`
                    : 'all sessions'
                } searched`
            : 'type to search'}
        </span>
      </div>

      <div className="filter-bar" style={{ paddingTop: 0, paddingBottom: 14, flexWrap: 'wrap' }}>
        <SortMenu value={sortMode} onChange={setSortMode} />
        <DateRangeMenu value={dateRange} onChange={setDateRange} />
        <LimitMenu value={limit} onChange={setLimit} />
        {/* Topic-search modes default-drop noise kinds; Memories mode
            keeps the toggle hidden because that mode is the explicit
            "show me every fragment" view and stripping there would
            contradict the mode's contract. */}
        {mode !== 'memories' && (
          <button
            className="btn"
            type="button"
            onClick={() => setHideBoilerplate((v) => !v)}
            title={
              hideBoilerplate
                ? `Boilerplate kinds (${NOISE_KINDS_FOR_TOPIC_SEARCH.join(
                    ', ',
                  )}) dropped from results. Click to include them.`
                : 'Boilerplate kinds are being included — these dominate topic queries via shared tokens. Click to hide them.'
            }
          >
            <Icon.Filter className="ic" />
            <span>
              boilerplate:{' '}
              <span className="mono">{hideBoilerplate ? 'hidden' : 'shown'}</span>
            </span>
          </button>
        )}
        <div className="grow" />
        {(chipsByKey.kind.length > 1 ||
          chipsByKey.urgency.length > 1 ||
          chipsByKey.project.length > 0 ||
          dateRange !== 'all') && (
          <span
            className="mono dim"
            style={{ fontSize: 10.5, color: 'var(--ink-faint)' }}
            title="Filters that are too expressive for the BE — applied after the substrate returns hits"
          >
            ⚙ client-side post-filter active
          </span>
        )}
      </div>

      {q === '' && (
        <div className="filter-bar" style={{ paddingTop: 0 }}>
          <span className="mono dim" style={{ fontSize: 11, marginRight: 8 }}>
            quick searches:
          </span>
          <span className="chip" onClick={() => setQ('recent learnings')}>
            recent learnings
          </span>
          <span className="chip" onClick={() => setQ('open decisions')}>
            open decisions
          </span>
          <span className="chip" onClick={() => setQ('blockers')}>
            blockers across all projects
          </span>
          <span className="chip" onClick={() => setQ('user actions today')}>
            user actions today
          </span>
        </div>
      )}

      <div className="card" style={{ padding: 0 }}>
        {q === '' ? (
          <div className="empty">
            <Icon.Search
              style={{ width: 36, height: 36, color: 'var(--ink-faint)', opacity: 0.6 }}
            />
            <div className="empty-title">Search the substrate</div>
            <div className="empty-body">
              {mode === 'features'
                ? 'Type the name (or any substring) of a feature you shipped — e.g. "reader summary lens", "cache encryption".'
                : mode === 'files'
                  ? 'Type a file path or basename — e.g. "AgentStreamRail.tsx", "src/services/embedding". Substring, case-insensitive.'
                  : mode === 'sessions'
                    ? 'Type a topic you discussed — e.g. "reader summary lenses", "embedding choices". Results grouped by session, newest first.'
                    : 'Type a query above, or click a quick search. Add chips to constrain by kind, project, or session.'}
            </div>
          </div>
        ) : mode === 'files' ? (
          filesQuery.isLoading ? (
            <div className="empty">
              <div className="empty-title">searching file paths…</div>
            </div>
          ) : fileMatches.length === 0 ? (
            <div className="empty">
              <div className="empty-title">No files match</div>
              <div className="empty-body">
                Try a shorter substring — match is case-insensitive on
                any path containing the query. Sessions with no
                files_touched metadata won't appear (older ingest path).
              </div>
            </div>
          ) : (
            fileMatches.map((m) => (
              <FileSearchRow key={m.session_id} match={m} query={q} />
            ))
          )
        ) : mode === 'features' ? (
          featuresQuery.isLoading ? (
            <div className="empty">
              <div className="empty-title">searching feature names…</div>
            </div>
          ) : featureHits.length === 0 ? (
            <div className="empty">
              <div className="empty-title">No feature names match</div>
              <div className="empty-body">
                Try a shorter substring — the search is plain substring,
                case-insensitive. Or switch to <strong>Sessions</strong> mode
                to search by topic instead of feature name.
              </div>
            </div>
          ) : (
            featureHits.map((h, i) => (
              <FeatureSearchRow key={`${h.session_id}-${i}`} hit={h} query={q} />
            ))
          )
        ) : mode === 'sessions' ? (
          searchQuery.isLoading ? (
            <div className="empty">
              <div className="empty-title">searching sessions…</div>
            </div>
          ) : sessionGroups.length === 0 ? (
            <div className="empty">
              <div className="empty-title">No sessions match</div>
              <div className="empty-body">
                Try removing a filter or broadening the query. Or switch to{' '}
                <strong>Features</strong> mode if you remember the feature name.
              </div>
            </div>
          ) : (
            sessionGroups.map((g) => (
              <SessionGroupRow key={g.session_id} group={g} query={q} />
            ))
          )
        ) : results.length === 0 ? (
          <div className="empty">
            <div className="empty-title">No memories match</div>
            <div className="empty-body">Try removing a filter or broadening the query.</div>
          </div>
        ) : (
          results.map((r, i) => {
            const kind = (r.metadata.kind as string | undefined) ?? 'unknown';
            const stored = shortTs(r.metadata.ts as string | undefined);
            return (
              <div
                key={r.id}
                className={`search-result${focused === i ? ' focused' : ''}`}
                onMouseEnter={() => setFocused(i)}
              >
                <div>
                  <div className="meta-row">
                    <KindBadge kind={kind} />
                    <SessionPill id={r.session_id} />
                    <ProjBadge p={r.project} />
                    {stored && <span>· {stored}</span>}
                  </div>
                  <div
                    className="snippet"
                    dangerouslySetInnerHTML={{ __html: highlight(r.content, q) }}
                  />
                </div>
                <div className="sim">
                  <div className="mono" style={{ fontSize: 10.5 }}>
                    sim · {r.similarity.toFixed(2)}
                  </div>
                  <div className="sim-bar">
                    <span style={{ width: `${Math.max(0, Math.min(1, r.similarity)) * 100}%` }} />
                  </div>
                  <div
                    className="mono"
                    style={{ fontSize: 9.5, color: 'var(--ink-faint)', marginTop: 2 }}
                  >
                    tf-idf · 256d
                  </div>
                </div>
              </div>
            );
          })
        )}
      </div>

      <div className="note-banner" style={{ marginTop: 14 }}>
        <span className="dot" style={{ background: 'var(--urg-soon)' }} />
        <span>
          Substrate is on the hash-based TF-IDF embedder by default — similarity scores are
          uniformly high and not yet a reliable rank signal. Swap to Ollama or OpenAI from{' '}
          <a style={{ color: 'var(--accent)' }} href="#" onClick={(e) => e.preventDefault()}>
            Substrate · embedding
          </a>
          .
        </span>
      </div>
    </div>
  );
}

function ChipAdder({
  onAdd,
  knownProjects,
  knownSessions,
}: {
  onAdd: (k: string, v: string) => void;
  knownProjects: string[];
  knownSessions: string[];
}) {
  const [open, setOpen] = useState(false);
  const [key, setKey] = useState<(typeof CHIP_KEYS)[number] | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  // Merge static enums (kind, urgency) with live values (project, session)
  // so the dropdown reflects whatever is actually in the substrate.
  const valuesFor: Record<(typeof CHIP_KEYS)[number], string[]> = {
    kind: STATIC_VALUES.kind ?? [],
    urgency: STATIC_VALUES.urgency ?? [],
    project: knownProjects,
    session: knownSessions,
  };

  useEffect(() => {
    const fn = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) {
        setOpen(false);
        setKey(null);
      }
    };
    document.addEventListener('click', fn);
    return () => document.removeEventListener('click', fn);
  }, []);

  return (
    <div ref={ref} style={{ position: 'relative' }}>
      <span className="chip add" onClick={() => setOpen((o) => !o)}>
        <Icon.Plus /> filter
      </span>
      {open && (
        <div
          style={{
            position: 'absolute',
            left: 0,
            top: 'calc(100% + 4px)',
            background: 'var(--surface-2)',
            border: '1px solid var(--border)',
            borderRadius: 8,
            padding: 4,
            minWidth: 180,
            zIndex: 10,
            boxShadow: 'var(--shadow-pop)',
          }}
        >
          {!key ? (
            CHIP_KEYS.map((k) => (
              <div
                key={k}
                onClick={() => setKey(k)}
                style={{
                  padding: '6px 10px',
                  borderRadius: 4,
                  cursor: 'pointer',
                  fontFamily: 'var(--font-mono)',
                  fontSize: 12,
                  color: 'var(--ink-muted)',
                }}
              >
                {k}
              </div>
            ))
          ) : (
            <>
              <div
                style={{
                  padding: '4px 10px',
                  fontFamily: 'var(--font-mono)',
                  fontSize: 10,
                  color: 'var(--ink-faint)',
                  textTransform: 'uppercase',
                }}
              >
                {key} ·
              </div>
              {valuesFor[key].map((v) => (
                <div
                  key={v}
                  onClick={() => {
                    onAdd(key, v);
                    setOpen(false);
                    setKey(null);
                  }}
                  style={{
                    padding: '5px 10px',
                    borderRadius: 4,
                    cursor: 'pointer',
                    fontFamily: 'var(--font-mono)',
                    fontSize: 12,
                    color: 'var(--ink)',
                  }}
                >
                  {v}
                </div>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Sort / Date / Limit dropdowns
//
// Three minor controls colocated under the filter bar. All three render the
// same way (label + chevron + popover with options) — extract one component
// rather than three near-identical ones.
// ─────────────────────────────────────────────────────────────────────────────

function PopoverMenu<T extends string | number>({
  icon,
  label,
  value,
  options,
  optionLabel,
  onChange,
  title,
  minWidth,
}: {
  icon: ReactNode;
  label: string;
  value: T;
  options: readonly T[];
  optionLabel: (v: T) => string;
  onChange: (v: T) => void;
  title?: string;
  minWidth?: number;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const fn = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener('click', fn);
    return () => document.removeEventListener('click', fn);
  }, []);

  return (
    <div ref={ref} style={{ position: 'relative' }}>
      <button className="btn" onClick={() => setOpen((o) => !o)} type="button" title={title}>
        {icon}
        <span>
          {label}: <span className="mono">{optionLabel(value)}</span>
        </span>
        <Icon.Chevron style={{ transform: 'rotate(90deg)', color: 'var(--ink-faint)' }} />
      </button>
      {open && (
        <div
          style={{
            position: 'absolute',
            left: 0,
            top: 'calc(100% + 4px)',
            background: 'var(--surface-2)',
            border: '1px solid var(--border)',
            borderRadius: 8,
            padding: 4,
            minWidth: minWidth ?? 200,
            zIndex: 10,
            boxShadow: 'var(--shadow-pop)',
          }}
        >
          {options.map((o) => (
            <div
              key={String(o)}
              onClick={() => {
                onChange(o);
                setOpen(false);
              }}
              style={{
                padding: '6px 10px',
                borderRadius: 4,
                cursor: 'pointer',
                background: value === o ? 'var(--surface-3)' : 'transparent',
                fontFamily: 'var(--font-mono)',
                fontSize: 12,
                color: value === o ? 'var(--ink)' : 'var(--ink-muted)',
              }}
            >
              {optionLabel(o)}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function SortMenu({ value, onChange }: { value: SortMode; onChange: (v: SortMode) => void }) {
  return (
    <PopoverMenu
      icon={<Icon.Clock className="ic" />}
      label="sort"
      value={value}
      options={['similarity', 'newest', 'oldest', 'importance'] as const}
      optionLabel={(v) => SORT_LABELS[v]}
      onChange={onChange}
      title="Sort order"
      minWidth={220}
    />
  );
}

function DateRangeMenu({
  value,
  onChange,
}: {
  value: DateRange;
  onChange: (v: DateRange) => void;
}) {
  return (
    <PopoverMenu
      icon={<Icon.Clock className="ic" />}
      label="when"
      value={value}
      options={['all', '1h', '24h', '7d', '30d'] as const}
      optionLabel={(v) => DATE_LABELS[v]}
      onChange={onChange}
      title="Date range filter"
      minWidth={180}
    />
  );
}

function LimitMenu({ value, onChange }: { value: number; onChange: (v: number) => void }) {
  return (
    <PopoverMenu
      icon={<Icon.Search className="ic" />}
      label="limit"
      value={value}
      options={[10, 25, 50, 100, 200] as const}
      optionLabel={(v) => `${v} results`}
      onChange={onChange}
      title="Max results"
      minWidth={140}
    />
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Sessions mode — group retrieve hits by session_id
//
// Why this is the daily-driver default: the user's actual question is
// usually "which session was I working on X?" — not "show me every
// fragment that mentions X". A session with 20 fragments mentioning the
// topic is a much stronger answer than 20 individual fragment rows
// scattered across the result list.
//
// Ranking heuristic: `best_similarity + 0.05 * fragment_count` — the
// best single hit dominates, but a session with many mentions wins a
// tie. The 0.05 weight is small enough that a 0.9-similarity single
// hit still beats a 0.7-similarity 4-hit session.
// ─────────────────────────────────────────────────────────────────────────────

type SessionGroup = {
  session_id: string;
  project: string;
  best_similarity: number;
  fragment_count: number;
  rank_score: number;
  newest_ts: number; // ms since epoch, -Infinity when no fragment carries a ts
  top_snippets: SearchResultRow[];
};

function groupBySession(rows: SearchResultRow[]): SessionGroup[] {
  const byId = new Map<string, SearchResultRow[]>();
  for (const r of rows) {
    const arr = byId.get(r.session_id);
    if (arr) {
      arr.push(r);
    } else {
      byId.set(r.session_id, [r]);
    }
  }
  const groups: SessionGroup[] = [];
  for (const [session_id, hits] of byId.entries()) {
    const sorted = hits.slice().sort((a, b) => b.similarity - a.similarity);
    const best = sorted[0]?.similarity ?? 0;
    const newest = hits.reduce(
      (acc, h) => Math.max(acc, tsMs(h.metadata.ts)),
      -Infinity,
    );
    groups.push({
      session_id,
      project: hits[0]?.project ?? '?',
      best_similarity: best,
      fragment_count: hits.length,
      rank_score: best + 0.05 * hits.length,
      newest_ts: newest,
      top_snippets: sorted.slice(0, 3),
    });
  }
  groups.sort((a, b) => b.rank_score - a.rank_score);
  return groups;
}

function SessionGroupRow({ group, query }: { group: SessionGroup; query: string }) {
  // Ticket #2 — inline session-summary preview. Lazy-fetch on demand
  // when the user clicks the "Summary" toggle. Cached server-side so
  // repeated opens are free.
  const [showSummary, setShowSummary] = useState(false);
  const summaryQuery = useQuery({
    queryKey: ['session-summary', group.session_id],
    enabled: showSummary,
    staleTime: 60_000,
    queryFn: () => api.sessionSummary(group.session_id),
  });

  return (
    <div className="search-result" style={{ alignItems: 'stretch' }}>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="meta-row" style={{ marginBottom: 6 }}>
          <SessionPill id={group.session_id} />
          <ProjBadge p={group.project} />
          <span className="mono dim" style={{ fontSize: 11 }}>
            · {group.fragment_count} matching fragment
            {group.fragment_count === 1 ? '' : 's'}
          </span>
          {Number.isFinite(group.newest_ts) && (
            <span className="mono dim" style={{ fontSize: 11 }}>
              · newest {shortTs(new Date(group.newest_ts).toISOString())}
            </span>
          )}
          <button
            className="btn btn-ghost sm"
            type="button"
            onClick={() => setShowSummary((v) => !v)}
            title={
              showSummary
                ? 'Hide the LLM-generated one-paragraph summary'
                : 'Show a one-paragraph summary of this session (lazy-fetched, server-cached)'
            }
            style={{ fontSize: 10.5, marginLeft: 'auto' }}
          >
            {showSummary ? 'hide summary' : 'show summary'}
          </button>
        </div>
        {showSummary && (
          <div
            className="snippet"
            style={{
              marginTop: 4,
              marginBottom: 6,
              paddingLeft: 6,
              borderLeft: '2px solid var(--accent)',
              fontSize: 11.5,
              color: 'var(--ink-muted)',
              fontStyle: 'italic',
            }}
          >
            {summaryQuery.isLoading
              ? 'loading summary…'
              : summaryQuery.isError
                ? '(summary unavailable — endpoint failed)'
                : summaryQuery.data?.summary || '(no summary yet — try again after the session has more fragments)'}
          </div>
        )}
        {group.top_snippets.map((s) => (
          <div key={s.id} style={{ marginTop: 4 }}>
            <div className="meta-row" style={{ marginBottom: 2 }}>
              <KindBadge kind={(s.metadata.kind as string | undefined) ?? 'unknown'} />
              <span className="mono dim" style={{ fontSize: 10.5 }}>
                sim · {s.similarity.toFixed(2)}
              </span>
            </div>
            <div
              className="snippet"
              style={{ paddingLeft: 4, borderLeft: '2px solid var(--border)' }}
              dangerouslySetInnerHTML={{ __html: highlight(s.content, query) }}
            />
          </div>
        ))}
      </div>
      <div className="sim">
        <div className="mono" style={{ fontSize: 10.5 }}>
          best · {group.best_similarity.toFixed(2)}
        </div>
        <div className="sim-bar">
          <span
            style={{
              width: `${Math.max(0, Math.min(1, group.best_similarity)) * 100}%`,
            }}
          />
        </div>
        <div
          className="mono"
          style={{ fontSize: 9.5, color: 'var(--ink-faint)', marginTop: 2 }}
        >
          rank · {group.rank_score.toFixed(2)}
        </div>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Features mode — substring search across declared feature names
// ─────────────────────────────────────────────────────────────────────────────

function FeatureSearchRow({ hit, query }: { hit: FeatureHit; query: string }) {
  return (
    <div className="search-result">
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="meta-row" style={{ marginBottom: 4 }}>
          <SessionPill id={hit.session_id} />
          {hit.layer && (
            <span className="mono dim" style={{ fontSize: 11 }}>
              · {hit.layer}
            </span>
          )}
          {hit.ts && (
            <span className="mono dim" style={{ fontSize: 11 }}>
              · {shortTs(hit.ts)}
            </span>
          )}
        </div>
        <div
          className="snippet"
          dangerouslySetInnerHTML={{ __html: highlight(hit.feature, query) }}
        />
        {hit.files.length > 0 && (
          <div className="mono dim" style={{ fontSize: 10.5, marginTop: 4 }}>
            files:{' '}
            {hit.files.slice(0, 3).join(' · ')}
            {hit.files.length > 3 && ` · +${hit.files.length - 3} more`}
          </div>
        )}
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Files mode — substring search across files_touched
//
// total_files==1 is the strong signal: session touched ONLY this file → it's
// the surgical authoring session. Anything bigger = the file was edited as
// part of a broader run. Render that distinction prominently because it's
// the most useful disambiguator for "who created this".
// ─────────────────────────────────────────────────────────────────────────────

function FileSearchRow({ match, query }: { match: FileMatch; query: string }) {
  const isAuthoringSession = match.total_files === 1;
  return (
    <div className="search-result">
      <div style={{ flex: 1, minWidth: 0 }}>
        <div className="meta-row" style={{ marginBottom: 4 }}>
          <SessionPill id={match.session_id} />
          <span
            className="mono dim"
            style={{ fontSize: 11 }}
            title={
              isAuthoringSession
                ? 'Session touched ONLY this file — likely the surgical authoring session.'
                : `Session touched ${match.total_files} files total; this one matched the query.`
            }
          >
            · {match.matched_files.length} matched / {match.total_files} total
          </span>
          {isAuthoringSession && (
            <span
              className="mono"
              style={{
                fontSize: 10.5,
                color: 'var(--accent)',
                background: 'var(--surface-2)',
                border: '1px solid var(--border)',
                borderRadius: 4,
                padding: '0 6px',
              }}
            >
              authoring
            </span>
          )}
        </div>
        <ul
          className="mono"
          style={{
            margin: 0,
            paddingLeft: 4,
            listStyle: 'none',
            borderLeft: '2px solid var(--border)',
            fontSize: 11.5,
          }}
        >
          {match.matched_files.slice(0, 5).map((p) => (
            <li
              key={p}
              style={{ padding: '2px 8px', wordBreak: 'break-all' }}
              dangerouslySetInnerHTML={{ __html: highlight(p, query) }}
            />
          ))}
          {match.matched_files.length > 5 && (
            <li className="dim" style={{ padding: '2px 8px' }}>
              · +{match.matched_files.length - 5} more
            </li>
          )}
        </ul>
      </div>
    </div>
  );
}
