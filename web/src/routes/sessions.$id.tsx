import { createFileRoute, Link } from '@tanstack/react-router';
import { useState } from 'react';

import { Icon, UrgencyLabel } from '@/components/atoms';
import { MOCK } from '@/lib/mock-data';

export const Route = createFileRoute('/sessions/$id')({
  component: SessionDetailPage,
});

type SectionKey =
  | 'goal_phases'
  | 'accomplishments'
  | 'learnings'
  | 'todos'
  | 'decisions'
  | 'blockers'
  | 'user_actions'
  | 'raw';

function SessionDetailPage() {
  const { id } = Route.useParams();
  const d = MOCK.sessionDetail;
  const sess = MOCK.sessions.find((s) => s.id === id) ?? d.session;

  const [open, setOpen] = useState<Record<SectionKey, boolean>>({
    goal_phases: true,
    accomplishments: false,
    learnings: false,
    todos: false,
    decisions: false,
    blockers: false,
    user_actions: false,
    raw: false,
  });
  const toggle = (k: SectionKey) => setOpen((o) => ({ ...o, [k]: !o[k] }));

  return (
    <div>
      <div className="session-head">
        <span className="sid">{sess.id}</span>
        <span className="proj">~/code/{sess.project}</span>
        <span style={{ color: 'var(--ink-muted)', fontSize: 12.5 }}>{sess.phase}</span>
        <span className="when">last activity · {sess.lastActivity}</span>
      </div>

      <div className="grid-3" style={{ marginBottom: 18 }}>
        <Stat label="memories" v={sess.counts.memories} />
        <Stat label="goal phases" v={sess.counts.goal_phases} />
        <Stat
          label="decisions"
          v={sess.counts.decisions}
          flag={sess.counts.decisions > 5 ? 'warn' : undefined}
        />
      </div>

      <Link to="/sessions" className="btn btn-ghost" style={{ marginBottom: 12 }}>
        ← all sessions
      </Link>

      <Section
        open={open.goal_phases}
        toggle={() => toggle('goal_phases')}
        name="Goal phases"
        count={d.goalPhases.length}
      >
        <div className="timeline" style={{ marginTop: 10 }}>
          {d.goalPhases.map((p, i) => (
            <div className="timeline-item" key={i}>
              <div className="timeline-time">{p.span}</div>
              <div className="phase-card">
                <div className="h">
                  <div>
                    <div className="title">{p.title}</div>
                    <div className="meta">
                      <span>
                        <b>{p.counts.decisions}</b> decisions
                      </span>
                      <span>
                        <b>{p.counts.learnings}</b> learnings
                      </span>
                      <span>
                        <b>{p.counts.blockers}</b> blockers
                      </span>
                    </div>
                  </div>
                </div>
                <ul className="accs">
                  {p.accs.map((a, j) => (
                    <li key={j}>{a}</li>
                  ))}
                </ul>
              </div>
            </div>
          ))}
        </div>
      </Section>

      <Section
        open={open.accomplishments}
        toggle={() => toggle('accomplishments')}
        name="Accomplishments"
        count={d.accomplishments.length}
      >
        {d.accomplishments.map((f, i) => (
          <div className="frag-row" key={i}>
            <div className="text">{f.text}</div>
            <div className="stamp">{f.t}</div>
          </div>
        ))}
      </Section>

      <Section
        open={open.learnings}
        toggle={() => toggle('learnings')}
        name="Learnings"
        count={d.learnings.length}
      >
        {d.learnings.map((f, i) => (
          <div className="frag-row" key={i}>
            <div className="text">{f.text}</div>
            <div className="stamp">{f.t}</div>
          </div>
        ))}
      </Section>

      <Section open={open.todos} toggle={() => toggle('todos')} name="Todos" count={d.todos.length}>
        {d.todos.map((f, i) => (
          <div className="frag-row" key={i}>
            <div className="text">
              <span className={`todo-status ${f.status}`}>{f.status.replace('_', ' ')}</span>
              {f.text}
            </div>
            <div className="stamp">{f.t}</div>
          </div>
        ))}
      </Section>

      <Section
        open={open.decisions}
        toggle={() => toggle('decisions')}
        name="Decisions"
        count={d.decisions.length}
      >
        {d.decisions.map((f, i) => (
          <div className="frag-row" key={i}>
            <div className="text">
              {f.text}
              {f.awaiting && (
                <div className="sub">
                  <span style={{ color: 'var(--urg-now)' }}>● awaiting_decision=true</span>
                </div>
              )}
            </div>
            <div className="stamp">{f.t}</div>
          </div>
        ))}
      </Section>

      <Section
        open={open.blockers}
        toggle={() => toggle('blockers')}
        name="Blockers"
        count={d.blockers.length}
      >
        {d.blockers.length === 0 ? (
          <div className="empty">
            <div className="empty-title dim">No blockers in this session.</div>
          </div>
        ) : (
          d.blockers.map((f, i) => (
            <div className="frag-row" key={i}>
              <div className="text">{f.text}</div>
              <div className="stamp">{f.t}</div>
            </div>
          ))
        )}
      </Section>

      <Section
        open={open.user_actions}
        toggle={() => toggle('user_actions')}
        name="User actions"
        count={d.userActions.length}
      >
        {d.userActions.map((f, i) => (
          <div className="frag-row" key={i}>
            <div className="text">
              <UrgencyLabel urg={f.urgency} /> <span style={{ marginLeft: 8 }}>{f.text}</span>
            </div>
            <div className="stamp">{f.t}</div>
          </div>
        ))}
      </Section>

      <Section
        open={open.raw}
        toggle={() => toggle('raw')}
        name="Raw timeline"
        count={142}
        hint="advanced"
      >
        <div className="empty">
          <div className="empty-title dim">Paginated raw fragment timeline (142 items)</div>
          <div className="empty-body">
            Every memory in chrono order. Useful when debugging the extractor or for
            `summarize`-style operations.
          </div>
        </div>
      </Section>
    </div>
  );
}

function Section({
  open,
  toggle,
  name,
  count,
  hint,
  children,
}: {
  open: boolean;
  toggle: () => void;
  name: string;
  count: number;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className={`acc${open ? ' open' : ''}`}>
      <div className="acc-head" onClick={toggle}>
        <Icon.Chevron className="chev" />
        <span className="name">{name}</span>
        {hint && (
          <span
            className="mono dim"
            style={{
              fontSize: 10,
              letterSpacing: '0.06em',
              textTransform: 'uppercase',
              marginLeft: 6,
            }}
          >
            {hint}
          </span>
        )}
        <span className="ct">{count}</span>
      </div>
      {open && <div className="acc-body">{children}</div>}
    </div>
  );
}

function Stat({ label, v, flag }: { label: string; v: number; flag?: 'warn' }) {
  return (
    <div className="card" style={{ padding: '14px 16px' }}>
      <div
        className="mono dim"
        style={{ fontSize: 10.5, letterSpacing: '0.08em', textTransform: 'uppercase' }}
      >
        {label}
      </div>
      <div
        style={{
          fontSize: 26,
          fontWeight: 500,
          marginTop: 4,
          fontFamily: 'var(--font-mono)',
          color: flag === 'warn' ? 'var(--urg-soon)' : 'var(--ink)',
        }}
      >
        {v}
      </div>
    </div>
  );
}
