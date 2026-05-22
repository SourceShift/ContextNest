import { createFileRoute, useNavigate } from '@tanstack/react-router';
import { useState } from 'react';

import { Icon, UrgencyDot } from '@/components/atoms';
import { useSessions } from '@/hooks/useSessions';
import { useInbox } from '@/hooks/useInbox';
import { agoFrom } from '@/lib/relative-time';
import type { SessionListItem } from '@/lib/types';
import type { InboxItemMock } from '@/lib/mock-data';

export const Route = createFileRoute('/sessions/')({
  component: SessionsPage,
});

function basename(p: string | null | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

// Map ISO ts to ms-since-now for filtering. `null` becomes Infinity so
// sessions with no timestamps are excluded by every non-`all` range —
// that matches the "show me recent activity" intent of the picker.
function msAgo(ts: string | null): number {
  if (!ts) return Number.POSITIVE_INFINITY;
  const t = Date.parse(ts);
  if (Number.isNaN(t)) return Number.POSITIVE_INFINITY;
  return Date.now() - t;
}

// The session list endpoint gives us per-kind counts but not an "urgency"
// signal. We derive it from the inbox: if a session has any user_action
// or awaiting decision in the dashboard inbox, treat the session as
// `now`-urgent; otherwise default to `later`.
function deriveUrgency(
  sessionId: string,
  inbox: InboxItemMock[],
): 'now' | 'soon' | 'later' {
  let best: 'now' | 'soon' | 'later' = 'later';
  for (const item of inbox) {
    if (item.sessionId !== sessionId) continue;
    if (item.urgency === 'now') return 'now';
    if (item.urgency === 'soon' && best === 'later') best = 'soon';
  }
  return best;
}

function SessionsPage() {
  const [filter, setFilter] = useState('');
  const [range, setRange] = useState<'1d' | '7d' | '30d' | 'all'>('7d');
  const navigate = useNavigate();
  const sessions = useSessions();
  const inbox = useInbox();

  const all: SessionListItem[] = sessions.data;
  const inboxData = inbox.data;

  const projects = Array.from(
    new Set(all.map((s) => basename(s.project_cwd))),
  ).sort();

  const filtered = all.filter((s) => {
    if (filter) {
      const proj = basename(s.project_cwd).toLowerCase();
      const id = s.id.toLowerCase();
      if (!proj.includes(filter.toLowerCase()) && !id.includes(filter.toLowerCase())) {
        return false;
      }
    }
    const ms = msAgo(s.last_ts);
    if (range === '1d' && ms > 24 * 3600 * 1000) return false;
    if (range === '7d' && ms > 7 * 24 * 3600 * 1000) return false;
    if (range === '30d' && ms > 30 * 24 * 3600 * 1000) return false;
    return true;
  });

  // Sort newest-first by last_ts so the most-recent sessions sit at the
  // top regardless of the underlying iteration order from the substrate.
  filtered.sort((a, b) => {
    const at = a.last_ts ? Date.parse(a.last_ts) : 0;
    const bt = b.last_ts ? Date.parse(b.last_ts) : 0;
    return bt - at;
  });

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Sessions</h1>
          <div className="page-sub">
            Every Claude Code session this substrate has seen ·{' '}
            <span className="mono">{all.length}</span> total,{' '}
            <span className="mono">{filtered.length}</span> shown
          </div>
        </div>
        <div className="page-actions">
          <button className="btn" onClick={() => sessions.refetch()} type="button">
            <Icon.Refresh /> Refresh
          </button>
        </div>
      </div>

      <div className="filter-bar">
        <div className="search-input" style={{ width: 280, padding: '6px 12px' }}>
          <Icon.Search className="icon" />
          <input
            placeholder="filter by project or id…"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>
        <div className="tabs">
          {(['1d', '7d', '30d', 'all'] as const).map((r) => (
            <button
              key={r}
              className={range === r ? 'active' : ''}
              onClick={() => setRange(r)}
              type="button"
            >
              {r}
            </button>
          ))}
        </div>
        <div className="grow" />
        <span className="mono dim" style={{ fontSize: 11 }}>
          ~/.claude/projects/ · {projects.length} projects
        </span>
      </div>

      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <div
          className="session-row"
          style={{
            background: 'var(--surface-2)',
            borderBottom: '1px solid var(--border)',
            fontFamily: 'var(--font-mono)',
            fontSize: 10,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
            color: 'var(--ink-faint)',
            cursor: 'default',
          }}
        >
          <span />
          <span>session id</span>
          <span>project</span>
          <span>fragments</span>
          <span style={{ textAlign: 'right' }}>counts · d · l · todo</span>
          <span style={{ textAlign: 'right' }}>last activity</span>
        </div>
        {filtered.map((s) => {
          const urgency = deriveUrgency(s.id, inboxData);
          const proj = basename(s.project_cwd);
          const dec = s.by_kind.decision ?? 0;
          const learn = s.by_kind.learning ?? 0;
          const todo = s.by_kind.todo ?? 0;
          return (
            <div
              key={s.id}
              className="session-row"
              onClick={() => navigate({ to: '/sessions/$id', params: { id: s.id } })}
            >
              <UrgencyDot urg={urgency} />
              {/* Show a visually compact head…tail of the canonical
                  session id; the full id is in `title` and is what gets
                  routed to on click. */}
              <span
                className="sid"
                title={s.id}
              >
                {s.id.length > 16 ? `${s.id.slice(0, 11)}…${s.id.slice(-8)}` : s.id}
              </span>
              <span>
                <span className="proj mono">{proj}</span>
                <span
                  className="phase"
                  style={{ display: 'block', marginTop: 3, fontSize: 12 }}
                >
                  {s.fragment_count.toLocaleString()} fragments
                </span>
              </span>
              <span className="mono dim" style={{ fontSize: 11 }}>
                {s.fragment_count.toLocaleString()}
              </span>
              <span className="counts">
                <span title="decisions">{dec}</span>
                <span title="learnings">{learn}</span>
                <span title="todos">{todo}</span>
              </span>
              <span className="when">{s.last_ts ? agoFrom(s.last_ts) : '—'}</span>
            </div>
          );
        })}
        {filtered.length === 0 && !sessions.isLoading && (
          <div className="empty">
            <div className="empty-title">
              {all.length === 0 ? 'No sessions in the substrate yet' : 'No sessions match'}
            </div>
            {all.length === 0 ? (
              <div className="empty-sub">
                Run <span className="mono">make cn-ingest SINCE=7d</span> to backfill, or wait
                for live cc_hooks to populate.
              </div>
            ) : (
              <button
                className="btn btn-ghost"
                onClick={() => {
                  setFilter('');
                  setRange('all');
                }}
                type="button"
              >
                clear filters
              </button>
            )}
          </div>
        )}
        {sessions.isLoading && filtered.length === 0 && (
          <div className="empty">
            <div className="empty-title">Loading…</div>
          </div>
        )}
      </div>
    </div>
  );
}
