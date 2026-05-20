import { createFileRoute } from '@tanstack/react-router';
import { useState } from 'react';

import { Icon, ProjBadge, SessionPill } from '@/components/atoms';
import { MOCK } from '@/lib/mock-data';

export const Route = createFileRoute('/phases')({
  component: PhasesPage,
});

function PhasesPage() {
  const [viz, setViz] = useState<'timeline' | 'clusters'>('timeline');
  const ps = MOCK.phases;

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Phases</h1>
          <div className="page-sub">
            Goal phases — multi-turn clustered intents across every session ·{' '}
            <span className="mono">{ps.length}</span> phases ·{' '}
            <span className="mono">{ps.reduce((a, p) => a + p.cluster, 0)}</span> z-insights
            clustered
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

      <div className="note-banner" style={{ marginBottom: 18 }}>
        <span className="dot" />
        <span>
          <span className="muted">§14·Q1 resolved:</span> vertical timeline picked for v1;
          cluster-grid available via tweak. Sparkline-of-clusters deferred.
        </span>
      </div>

      {viz === 'timeline' ? (
        <div className="timeline">
          {ps.map((p, i) => (
            <div className="timeline-item" key={i}>
              <div className="timeline-time">
                {p.time} · {p.duration}
              </div>
              <div className="phase-card">
                <div className="h">
                  <div style={{ flex: 1 }}>
                    <div className="title">{p.title}</div>
                    <div className="meta">
                      <SessionPill id={p.sessionId} />
                      <ProjBadge p={p.project} />
                      <span>
                        <b>{p.turns}</b> turns
                      </span>
                      <span>
                        <b>{p.cluster}</b> z-insights clustered
                      </span>
                    </div>
                  </div>
                  <button className="btn sm btn-ghost" type="button">
                    <Icon.ArrowRight /> open
                  </button>
                </div>
                <ul className="accs">
                  {p.facts.map((f, j) => (
                    <li key={j}>{f}</li>
                  ))}
                </ul>
              </div>
            </div>
          ))}
        </div>
      ) : (
        <div className="cluster-grid">
          {ps.map((p, i) => (
            <div className="cluster-card" key={i}>
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
                  {p.duration}
                </span>
              </div>
              <div className="cluster-dots">
                {Array.from({ length: 12 }).map((_, k) => (
                  <span key={k} className={k < p.cluster ? '' : 'faint'} />
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
                <span>{p.turns} turns</span>
                <span>{p.cluster} clustered</span>
              </div>
              <div className="muted" style={{ fontSize: 12, marginTop: 10, lineHeight: 1.5 }}>
                {p.facts[0]}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
