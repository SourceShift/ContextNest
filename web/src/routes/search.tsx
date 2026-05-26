import { createFileRoute } from '@tanstack/react-router';
import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { Icon, KindBadge, ProjBadge, SessionPill } from '@/components/atoms';
import { api } from '@/lib/api';
import { useSessions, useKnownProjects } from '@/hooks/useSessions';
import type { RetrieveHit } from '@/lib/types';

export const Route = createFileRoute('/search')({
  component: SearchPage,
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

function SearchPage() {
  const [q, setQ] = useState('');
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

  // Pick the target sessions: when sessions chips are present, scope to
  // those; otherwise hand the substrate the full session list. The
  // substrate's cross-session mode (POST /api/v1/tools/retrieve with
  // `session_ids`) does the merge under a single snapshot lock, so we
  // no longer need the per-session HTTP fan-out the old UI relied on.
  const targetSessionIds: string[] = useMemo(() => {
    if (chipsByKey.session.length > 0) return chipsByKey.session;
    return sessions.data.map((s) => s.id);
  }, [sessions.data, chipsByKey.session]);

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
    ],
    enabled: qDebounced.trim().length > 0 && targetSessionIds.length > 0,
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

      const res = await api.retrieve({
        query: qDebounced,
        top_k,
        ...(singleSession
          ? { session_id: singleSession }
          : { session_ids: targetSessionIds }),
        metadata_filter:
          Object.keys(metadataFilter).length > 0 ? metadataFilter : undefined,
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

  const results: SearchResultRow[] = searchQuery.data ?? [];

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Search</h1>
          <div className="page-sub">
            Semantic + metadata search across every memory · scoped to one session or cross-session
          </div>
        </div>
        <div className="page-actions">
          <span className="mono dim" style={{ fontSize: 11 }}>
            cmd+/ to focus
          </span>
        </div>
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
              : `${results.length} hits · ${chips.length} filter${chips.length === 1 ? '' : 's'} · ${targetSessionIds.length} session(s) searched`
            : 'type to search'}
        </span>
      </div>

      <div className="filter-bar" style={{ paddingTop: 0, paddingBottom: 14, flexWrap: 'wrap' }}>
        <SortMenu value={sortMode} onChange={setSortMode} />
        <DateRangeMenu value={dateRange} onChange={setDateRange} />
        <LimitMenu value={limit} onChange={setLimit} />
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
              Type a query above, or click a quick search. Add chips to constrain by kind, project,
              or session.
            </div>
          </div>
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
