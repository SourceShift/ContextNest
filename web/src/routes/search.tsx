import { createFileRoute } from '@tanstack/react-router';
import { useEffect, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { Icon, KindBadge, ProjBadge, SessionPill } from '@/components/atoms';
import { api } from '@/lib/api';
import { useSessions, useKnownProjects } from '@/hooks/useSessions';
import type { RetrieveHit, SessionListItem } from '@/lib/types';

export const Route = createFileRoute('/search')({
  component: SearchPage,
});

type Chip = { k: string; v: string };
const CHIP_KEYS = ['kind', 'project', 'session', 'urgency'] as const;

// Static menu of kinds + urgencies (these are the substrate's enums,
// not data values). Project / session lists are populated live from
// useSessions / useKnownProjects so they reflect the actual substrate.
const STATIC_VALUES: Partial<Record<(typeof CHIP_KEYS)[number], string[]>> = {
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
  const [chips, setChips] = useState<Chip[]>([]);
  const [focused, setFocused] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const sessions = useSessions();
  const knownProjects = useKnownProjects();

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  const removeChip = (i: number) => setChips((c) => c.filter((_, j) => j !== i));
  const addChip = (k: string, v: string) => setChips((c) => [...c, { k, v }]);

  const sessionChip = chips.find((c) => c.k === 'session');
  const kindChip = chips.find((c) => c.k === 'kind');
  const projectChip = chips.find((c) => c.k === 'project');
  const urgencyChip = chips.find((c) => c.k === 'urgency');

  // Build the metadata_filter the substrate's retrieve handler accepts.
  // Kind and urgency map directly to metadata keys. Project requires a
  // project_cwd match — but the chip value is a basename, not the full
  // path, so we filter client-side after retrieval instead.
  const metadataFilter: Record<string, unknown> = useMemo(() => {
    const f: Record<string, unknown> = {};
    if (kindChip) f.kind = kindChip.v;
    if (urgencyChip) f.urgency = urgencyChip.v;
    return f;
  }, [kindChip, urgencyChip]);

  // Pick the target sessions: explicit chip > "first 10 newest sessions".
  // We fan out to multiple sessions because the substrate's retrieve is
  // session-scoped. For most substrates this is a handful of sessions,
  // but for a heavily-populated dashboard (your 96+ sessions) we cap at
  // 10 newest by last_ts to keep the query cheap.
  const targetSessions: SessionListItem[] = useMemo(() => {
    if (sessionChip) {
      return sessions.data.filter((s) => s.id === sessionChip.v);
    }
    const sorted = [...sessions.data].sort((a, b) => {
      const at = a.last_ts ? Date.parse(a.last_ts) : 0;
      const bt = b.last_ts ? Date.parse(b.last_ts) : 0;
      return bt - at;
    });
    return sorted.slice(0, 10);
  }, [sessions.data, sessionChip]);

  const searchQuery = useQuery({
    queryKey: ['search', q, metadataFilter, targetSessions.map((s) => s.id)],
    enabled: q.trim().length > 0 && targetSessions.length > 0,
    staleTime: 5_000,
    queryFn: async (): Promise<SearchResultRow[]> => {
      // Fan-out: one retrieve call per session in scope. Returns are
      // merged and sorted by similarity desc. For session-scoped queries
      // this is one call; for "all sessions" mode it's capped at 10.
      const perSession = await Promise.all(
        targetSessions.map(async (s) => {
          try {
            const res = await api.retrieve({
              session_id: s.id,
              query: q,
              top_k: 20,
              metadata_filter:
                Object.keys(metadataFilter).length > 0 ? metadataFilter : undefined,
            });
            const proj = basename(s.project_cwd);
            return res.hits.map(
              (hit): SearchResultRow => ({
                ...hit,
                session_id: s.id,
                project: proj,
              }),
            );
          } catch {
            return [];
          }
        }),
      );
      const flat = perSession.flat();
      // Apply client-side project filter (chip is a basename).
      const filtered = projectChip
        ? flat.filter((r) => r.project === projectChip.v)
        : flat;
      filtered.sort((a, b) => b.similarity - a.similarity);
      return filtered.slice(0, 40);
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

      <div className="filter-bar" style={{ paddingTop: 12, paddingBottom: 14 }}>
        {chips.map((c, i) => (
          <span className="chip active" key={i}>
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
        <div className="grow" />
        <span className="mono dim" style={{ fontSize: 11 }}>
          {q
            ? searchQuery.isLoading
              ? 'searching…'
              : `${results.length} hits · ${chips.length} filters · ${targetSessions.length} session(s) searched`
            : 'type to search'}
        </span>
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
