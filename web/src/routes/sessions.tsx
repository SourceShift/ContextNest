import { createFileRoute, useNavigate } from '@tanstack/react-router';
import { useState } from 'react';

import { Icon, Sparkline, UrgencyDot } from '@/components/atoms';
import { MOCK } from '@/lib/mock-data';

export const Route = createFileRoute('/sessions')({
  component: SessionsPage,
});

function SessionsPage() {
  const [filter, setFilter] = useState('');
  const [range, setRange] = useState<'1d' | '7d' | '30d' | 'all'>('7d');
  const navigate = useNavigate();

  const all = MOCK.sessions;
  const filtered = all.filter((s) => {
    if (filter && !s.project.includes(filter) && !s.id.includes(filter)) return false;
    if (range === '1d' && s.lastActivityMs > 24 * 3600 * 1000) return false;
    if (range === '7d' && s.lastActivityMs > 7 * 24 * 3600 * 1000) return false;
    if (range === '30d' && s.lastActivityMs > 30 * 24 * 3600 * 1000) return false;
    return true;
  });

  const projects = Array.from(new Set(all.map((s) => s.project)));

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
          <button className="btn" type="button">
            <Icon.Plus /> Backfill all
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
          <span>project · current phase</span>
          <span>density</span>
          <span style={{ textAlign: 'right' }}>counts · m · d · l</span>
          <span style={{ textAlign: 'right' }}>last activity</span>
        </div>
        {filtered.map((s) => (
          <div
            key={s.id}
            className="session-row"
            onClick={() => navigate({ to: '/sessions/$id', params: { id: s.id } })}
          >
            <UrgencyDot urg={s.urgency} />
            <span className="sid">{s.id}</span>
            <span>
              <span className="proj mono">{s.project}</span>
              <span className="phase" style={{ display: 'block', marginTop: 3, fontSize: 12 }}>
                {s.phase}
              </span>
            </span>
            <Sparkline data={s.sparkline} w={70} h={18} />
            <span className="counts">
              <span title="memories">{s.counts.memories}</span>
              <span title="decisions">{s.counts.decisions}</span>
              <span title="learnings">{s.counts.learnings}</span>
            </span>
            <span className="when">{s.lastActivity}</span>
          </div>
        ))}
        {filtered.length === 0 && (
          <div className="empty">
            <div className="empty-title">No sessions match</div>
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
          </div>
        )}
      </div>
    </div>
  );
}
