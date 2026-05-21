import { createFileRoute } from '@tanstack/react-router';
import { useMemo, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { ProjBadge, SessionPill } from '@/components/atoms';
import { api } from '@/lib/api';
import { useSessions } from '@/hooks/useSessions';
import type { RetrieveHit, SessionListItem } from '@/lib/types';

export const Route = createFileRoute('/phases')({
  component: PhasesPage,
});

type PhaseRow = {
  id: string;
  sessionId: string;
  project: string;
  title: string;
  ts: string | null;
  endTs: string | null;
  durationMs: number | null;
  turnSpan: number | null;
};

function basename(p: string | null | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

function metaStr(hit: RetrieveHit, key: string): string | undefined {
  const v = hit.metadata[key];
  return typeof v === 'string' ? v : undefined;
}

function metaNum(hit: RetrieveHit, key: string): number | undefined {
  const v = hit.metadata[key];
  return typeof v === 'number' ? v : undefined;
}

function formatDuration(ms: number | null): string {
  if (ms == null || ms <= 0) return '—';
  const minutes = Math.floor(ms / 60000);
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h === 0) return `${m}m`;
  return `${h}h ${m.toString().padStart(2, '0')}m`;
}

function formatTimeStamp(ts: string | null): string {
  if (!ts) return '—';
  // ISO → "MM-DD HH:MM"
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts.slice(0, 16);
  const mo = (d.getMonth() + 1).toString().padStart(2, '0');
  const dd = d.getDate().toString().padStart(2, '0');
  const hh = d.getHours().toString().padStart(2, '0');
  const mm = d.getMinutes().toString().padStart(2, '0');
  return `${mo}-${dd} · ${hh}:${mm}`;
}

// Cap how many sessions we fan-out per render. Goal phases are stored
// at most a handful per session; 30 newest sessions × top_k 20 phases
// = up to 600 fetched records, sortable client-side.
const MAX_SESSIONS_FANOUT = 30;

async function fetchSessionPhases(session: SessionListItem): Promise<PhaseRow[]> {
  try {
    const res = await api.retrieve({
      session_id: session.id,
      query: 'goal_phase',
      top_k: 20,
      metadata_filter: { kind: 'goal_phase' },
    });
    const proj = basename(session.project_cwd);
    return res.hits.map((hit) => {
      const ts = metaStr(hit, 'ts') ?? null;
      const endTs = metaStr(hit, 'end_ts') ?? null;
      let durationMs: number | null = null;
      if (ts && endTs) {
        const a = Date.parse(ts);
        const b = Date.parse(endTs);
        if (!Number.isNaN(a) && !Number.isNaN(b) && b >= a) durationMs = b - a;
      }
      return {
        id: hit.id,
        sessionId: session.id,
        project: proj,
        title: hit.content,
        ts,
        endTs,
        durationMs,
        turnSpan: metaNum(hit, 'turn_span') ?? null,
      };
    });
  } catch {
    return [];
  }
}

function PhasesPage() {
  const [viz, setViz] = useState<'timeline' | 'clusters'>('timeline');
  const sessions = useSessions();

  // Pick the newest N sessions to fan-out across. We always sort by
  // last_ts desc so the most-recent activity dominates the timeline.
  const targetSessions = useMemo(() => {
    const sorted = [...sessions.data].sort((a, b) => {
      const at = a.last_ts ? Date.parse(a.last_ts) : 0;
      const bt = b.last_ts ? Date.parse(b.last_ts) : 0;
      return bt - at;
    });
    return sorted.slice(0, MAX_SESSIONS_FANOUT);
  }, [sessions.data]);

  const phasesQuery = useQuery({
    queryKey: ['phases', targetSessions.map((s) => s.id)],
    enabled: targetSessions.length > 0,
    staleTime: 15_000,
    refetchInterval: 60_000,
    queryFn: async (): Promise<PhaseRow[]> => {
      const lists = await Promise.all(targetSessions.map(fetchSessionPhases));
      const flat = lists.flat();
      // Newest phase first overall — the timeline reads top-to-bottom.
      flat.sort((a, b) => {
        const at = a.ts ? Date.parse(a.ts) : 0;
        const bt = b.ts ? Date.parse(b.ts) : 0;
        return bt - at;
      });
      return flat;
    },
  });

  const ps: PhaseRow[] = phasesQuery.data ?? [];
  const isLoading = sessions.isLoading || phasesQuery.isLoading;
  const totalTurns = ps.reduce((a, p) => a + (p.turnSpan ?? 0), 0);

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Phases</h1>
          <div className="page-sub">
            Goal phases — multi-turn clustered intents across every session ·{' '}
            <span className="mono">{ps.length}</span> phases ·{' '}
            <span className="mono">{totalTurns}</span> turns spanned ·{' '}
            <span className="mono">{targetSessions.length}</span> sessions scanned
          </div>
        </div>
        <div className="page-actions">
          <span className="mono dim" style={{ fontSize: 11, marginRight: 4 }}>
            viz:
          </span>
          <div className="tabs">
            <button
              className={viz === 'timeline' ? 'active' : ''}
              onClick={() => setViz('timeline')}
              type="button"
            >
              timeline
            </button>
            <button
              className={viz === 'clusters' ? 'active' : ''}
              onClick={() => setViz('clusters')}
              type="button"
            >
              clusters
            </button>
          </div>
        </div>
      </div>

      {sessions.data.length > MAX_SESSIONS_FANOUT && (
        <div className="note-banner" style={{ marginBottom: 18 }}>
          <span className="dot" />
          <span>
            Showing phases from the {MAX_SESSIONS_FANOUT} most recent sessions ·{' '}
            <span className="mono dim">{sessions.data.length - MAX_SESSIONS_FANOUT}</span> older
            sessions not scanned.
          </span>
        </div>
      )}

      {isLoading && ps.length === 0 ? (
        <div className="card" style={{ padding: 24 }}>
          <div className="empty">
            <div className="empty-title">Scanning {targetSessions.length} sessions…</div>
          </div>
        </div>
      ) : ps.length === 0 ? (
        <div className="card" style={{ padding: 24 }}>
          <div className="empty">
            <div className="empty-title">No goal phases recorded</div>
            <div className="empty-sub">
              The extractor clusters consecutive z-insight `goal` strings into goal_phase
              fragments. Empty inbox usually means none of the scanned sessions emitted clusterable
              goal sequences yet.
            </div>
          </div>
        </div>
      ) : viz === 'timeline' ? (
        <div className="timeline">
          {ps.map((p) => (
            <div className="timeline-item" key={p.id}>
              <div className="timeline-time">
                {formatTimeStamp(p.ts)}
                {p.durationMs != null && <> · {formatDuration(p.durationMs)}</>}
              </div>
              <div className="phase-card">
                <div className="h">
                  <div style={{ flex: 1 }}>
                    <div className="title">{p.title}</div>
                    <div className="meta">
                      <SessionPill id={p.sessionId} />
                      <ProjBadge p={p.project} />
                      {p.turnSpan != null && (
                        <span>
                          <b>{p.turnSpan}</b> turns
                        </span>
                      )}
                      {p.endTs && <span>ended {formatTimeStamp(p.endTs)}</span>}
                    </div>
                  </div>
                </div>
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="cluster-grid">
          {ps.map((p) => (
            <div className="cluster-card" key={p.id}>
              <div
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  gap: 10,
                  alignItems: 'flex-start',
                }}
              >
                <div
                  className="title"
                  style={{
                    fontSize: 13.5,
                    fontWeight: 500,
                    color: 'var(--ink)',
                    lineHeight: 1.4,
                  }}
                >
                  {p.title}
                </div>
                <span className="mono dim" style={{ fontSize: 10 }}>
                  {formatDuration(p.durationMs)}
                </span>
              </div>
              <div className="cluster-dots">
                {Array.from({ length: 12 }).map((_, k) => (
                  <span
                    key={k}
                    className={p.turnSpan != null && k < p.turnSpan ? '' : 'faint'}
                  />
                ))}
              </div>
              <div
                className="meta"
                style={{
                  display: 'flex',
                  gap: 10,
                  fontFamily: 'var(--font-mono)',
                  fontSize: 10.5,
                  color: 'var(--ink-dim)',
                }}
              >
                <SessionPill id={p.sessionId} />
                {p.turnSpan != null && <span>{p.turnSpan} turns</span>}
                {p.ts && <span>{formatTimeStamp(p.ts)}</span>}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
