import { createFileRoute, Link } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import { Icon, UrgencyLabel } from '@/components/atoms';
import { useInbox } from '@/hooks/useInbox';
import { useSessions } from '@/hooks/useSessions';
import { useSessionDetail, shortStamp } from '@/hooks/useSessionDetail';
import { agoFrom } from '@/lib/relative-time';
import type { RetrieveHit, SessionListItem, TrajectoryRecord } from '@/lib/types';

export const Route = createFileRoute('/sessions/$id')({
  component: SessionDetailPage,
});

type SectionKey =
  | 'inbox'
  | 'goal_phases'
  | 'accomplishments'
  | 'learnings'
  | 'todos'
  | 'decisions'
  | 'blockers'
  | 'user_actions'
  | 'trajectory'
  | 'prompt_preview'
  | 'promotion_queue';

function basename(p: string | null | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

function metaString(hit: RetrieveHit, key: string): string | undefined {
  const v = hit.metadata[key];
  return typeof v === 'string' ? v : undefined;
}

function recMetaString(rec: TrajectoryRecord, key: string): string | undefined {
  const v = rec.metadata[key];
  return typeof v === 'string' ? v : undefined;
}

function SessionDetailPage() {
  const { id } = Route.useParams();
  const sessionsHook = useSessions();
  const detail = useSessionDetail(id);
  const inbox = useInbox();

  const session: SessionListItem | undefined = useMemo(
    () => sessionsHook.data.find((s) => s.id === id),
    [sessionsHook.data, id],
  );

  // Inbox items that belong to THIS session — the same data the global
  // dashboard inbox would show, narrowed to one session. Sort by ts desc
  // for parity with the global inbox view.
  const sessionInbox = useMemo(
    () =>
      inbox.data
        .filter((i) => i.sessionId === id)
        .sort((a, b) => (b.stored ?? '').localeCompare(a.stored ?? '')),
    [inbox.data, id],
  );

  const [open, setOpen] = useState<Record<SectionKey, boolean>>({
    inbox: true,
    goal_phases: true,
    accomplishments: false,
    learnings: false,
    todos: false,
    decisions: false,
    blockers: false,
    user_actions: false,
    trajectory: true,
    prompt_preview: true,
    promotion_queue: false,
  });
  const toggle = (k: SectionKey) => setOpen((o) => ({ ...o, [k]: !o[k] }));

  const d = detail.data;
  const isLoading = detail.isLoading || sessionsHook.isLoading;

  // Best-effort "current phase" derivation: take the newest goal_phase
  // record's content. Falls back to a placeholder when the session has
  // no phase fragments yet.
  const currentPhase = useMemo(() => {
    if (!d || d.goalPhases.length === 0) return null;
    // goalPhases come back ordered by similarity, not ts. Sort by ts desc.
    const sorted = [...d.goalPhases].sort((a, b) => {
      const at = metaString(a, 'ts') ?? '';
      const bt = metaString(b, 'ts') ?? '';
      return bt.localeCompare(at);
    });
    return sorted[0].content;
  }, [d]);

  const project = basename(session?.project_cwd);
  const lastActivity = session?.last_ts ? agoFrom(session.last_ts) : '—';
  const trajectory = d?.trajectory;
  const promptPreview = d?.promptPreview;

  return (
    <div>
      <div className="session-head">
        {/* Detail page is the one place that has horizontal room for
            the full canonical session id — show it without truncation
            and make it click-to-copy so users can paste it into
            jsonl path lookups under ~/.claude/projects/. */}
        <span
          className="sid"
          title={`${session?.id ?? id}\n(click to copy)`}
          style={{ cursor: 'pointer', userSelect: 'all' }}
          onClick={() => {
            const full = session?.id ?? id;
            if (typeof navigator !== 'undefined' && navigator.clipboard) {
              void navigator.clipboard.writeText(full).catch(() => {});
            }
          }}
        >
          {session?.id ?? id}
        </span>
        <span className="proj">{project}</span>
        <span style={{ color: 'var(--ink-muted)', fontSize: 12.5 }}>
          {currentPhase ?? (isLoading ? 'loading…' : 'no phase recorded')}
        </span>
        <span className="when">last activity · {lastActivity}</span>
      </div>

      <div className="grid-3" style={{ marginBottom: 18 }}>
        <Stat label="fragments" v={session?.fragment_count ?? 0} loading={isLoading} />
        <Stat
          label="goal phases"
          v={d?.goalPhases.length ?? 0}
          loading={isLoading}
        />
        <Stat
          label="trajectory"
          v={trajectory?.trajectory_count ?? 0}
          loading={isLoading}
          flag={(trajectory?.cost_profile.records_per_turn ?? 0) > 2 ? 'warn' : undefined}
        />
      </div>

      <Link to="/sessions" className="btn btn-ghost" style={{ marginBottom: 12 }}>
        ← all sessions
      </Link>

      {detail.isError && (
        <div className="note-banner" style={{ marginBottom: 14 }}>
          <span className="dot" style={{ background: 'var(--urg-now)' }} />
          <span>
            Failed to load session detail.{' '}
            <button
              className="btn btn-ghost sm"
              onClick={() => detail.refetch()}
              type="button"
            >
              retry
            </button>
          </span>
        </div>
      )}

      <Section
        open={open.inbox}
        toggle={() => toggle('inbox')}
        name="Inbox"
        hint="actionable for this session"
        count={sessionInbox.length}
        loading={inbox.isLoading}
      >
        {sessionInbox.length === 0 ? (
          <Empty msg="Nothing actionable for this session — no user_action, awaiting decision, or open todo." />
        ) : (
          sessionInbox.map((item) => (
            <div className="frag-row" key={item.id}>
              <div className="text">
                <span style={{ display: 'inline-flex', gap: 8, alignItems: 'center' }}>
                  <UrgencyLabel urg={item.urgency} />
                  <span
                    className="mono dim"
                    style={{
                      fontSize: 10,
                      letterSpacing: '0.08em',
                      textTransform: 'uppercase',
                    }}
                  >
                    {item.kind}
                  </span>
                </span>{' '}
                <span style={{ marginLeft: 8 }}>{item.action}</span>
                {item.reason && (
                  <div className="sub" style={{ marginTop: 4, color: 'var(--ink-muted)' }}>
                    {item.reason}
                  </div>
                )}
                {item.awaiting && (
                  <div className="sub">
                    <span style={{ color: 'var(--urg-now)' }}>● awaiting_decision=true</span>
                  </div>
                )}
              </div>
              <div className="stamp">{item.ago}</div>
            </div>
          ))
        )}
      </Section>

      <Section
        open={open.goal_phases}
        toggle={() => toggle('goal_phases')}
        name="Goal phases"
        count={d?.goalPhases.length ?? 0}
        loading={isLoading}
      >
        {d && d.goalPhases.length === 0 ? (
          <Empty msg="No goal phases recorded for this session." />
        ) : (
          <div className="timeline" style={{ marginTop: 10 }}>
            {d?.goalPhases.map((p) => (
              <div className="timeline-item" key={p.id}>
                <div className="timeline-time">{shortStamp(metaString(p, 'ts'))}</div>
                <div className="phase-card">
                  <div className="h">
                    <div>
                      <div className="title">{p.content}</div>
                      {metaString(p, 'end_ts') && (
                        <div className="meta">
                          <span>ended {shortStamp(metaString(p, 'end_ts'))}</span>
                        </div>
                      )}
                    </div>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}
      </Section>

      <Section
        open={open.trajectory}
        toggle={() => toggle('trajectory')}
        name="Trajectory"
        hint="phases, decisions, failures, verification"
        count={trajectory?.trajectory_count ?? 0}
        loading={isLoading}
      >
        {!trajectory || trajectory.trajectory_count === 0 ? (
          <Empty msg="No sparse trajectory records captured yet." />
        ) : (
          <div>
            <div className="trajectory-cost">
              <span>
                <b>{trajectory.cost_profile.records_per_turn.toFixed(2)}</b> records / turn
              </span>
              <span>
                <b>{trajectory.cost_profile.turns_estimate}</b> turns estimated
              </span>
              <span>
                <b>{trajectory.cost_profile.prompt_directives}</b> directives
              </span>
              <span>
                <b>{trajectory.cost_profile.risk_flags}</b> risk flags
              </span>
            </div>
            <div className="timeline" style={{ marginTop: 12 }}>
              {trajectory.phases.length === 0 ? (
                trajectory.records.slice(0, 30).map((r) => (
                  <TrajectoryRow key={r.id} record={r} />
                ))
              ) : (
                trajectory.phases.map((phase) => {
                  const records = trajectory.records.filter((r) => r.phase_idx === phase.idx);
                  return (
                    <div className="timeline-item" key={`${phase.idx}-${phase.goal}`}>
                      <div className="timeline-time">
                        {shortStamp(phase.start_ts ?? undefined)}
                        {phase.end_ts && <> · {shortStamp(phase.end_ts)}</>}
                      </div>
                      <div className="phase-card">
                        <div className="h">
                          <div style={{ flex: 1 }}>
                            <div className="title">{phase.goal}</div>
                            <div className="meta">
                              {Object.entries(phase.counts).map(([kind, count]) => (
                                <span key={kind}>
                                  <b>{count}</b> {kind.replace('_', ' ')}
                                </span>
                              ))}
                            </div>
                          </div>
                        </div>
                        {records.length > 0 && (
                          <div className="trajectory-records">
                            {records.slice(0, 10).map((r) => (
                              <TrajectoryRow key={r.id} record={r} compact />
                            ))}
                          </div>
                        )}
                      </div>
                    </div>
                  );
                })
              )}
            </div>
          </div>
        )}
      </Section>

      <Section
        open={open.prompt_preview}
        toggle={() => toggle('prompt_preview')}
        name="Prompt preview"
        hint="deterministic capsule"
        count={promptPreview?.item_count ?? 0}
        loading={isLoading}
      >
        {!promptPreview || promptPreview.item_count === 0 ? (
          <Empty msg="No prompt capsule material yet. Sparse capture is working when this stays empty for low-signal turns." />
        ) : (
          <div className="prompt-preview">
            {promptPreview.sections.map((section) => (
              <div className="prompt-section" key={section.key}>
                <div className="prompt-section-head">
                  <span>{section.title}</span>
                  <span className="ct">{section.items.length}</span>
                </div>
                {section.items.length === 0 ? (
                  <div className="empty-inline">empty</div>
                ) : (
                  section.items.map((r) => <TrajectoryRow key={r.id} record={r} />)
                )}
              </div>
            ))}
          </div>
        )}
      </Section>

      <Section
        open={open.promotion_queue}
        toggle={() => toggle('promotion_queue')}
        name="Promotion queue"
        hint="review before long-term memory"
        count={trajectory?.promotion_queue.length ?? 0}
        loading={isLoading}
      >
        {!trajectory || trajectory.promotion_queue.length === 0 ? (
          <Empty msg="No repeated or high-consequence candidates ready for promotion." />
        ) : (
          trajectory.promotion_queue.map((r) => <TrajectoryRow key={r.id} record={r} />)
        )}
      </Section>

      <Section
        open={open.accomplishments}
        toggle={() => toggle('accomplishments')}
        name="Accomplishments"
        count={d?.accomplishments.length ?? 0}
        loading={isLoading}
      >
        {d && d.accomplishments.length === 0 ? (
          <Empty msg="No accomplishments yet." />
        ) : (
          d?.accomplishments.map((f) => (
            <div className="frag-row" key={f.id}>
              <div className="text">{f.content}</div>
              <div className="stamp">{shortStamp(metaString(f, 'ts'))}</div>
            </div>
          ))
        )}
      </Section>

      <Section
        open={open.learnings}
        toggle={() => toggle('learnings')}
        name="Learnings"
        count={d?.learnings.length ?? 0}
        loading={isLoading}
      >
        {d && d.learnings.length === 0 ? (
          <Empty msg="No learnings captured." />
        ) : (
          d?.learnings.map((f) => (
            <div className="frag-row" key={f.id}>
              <div className="text">{f.content}</div>
              <div className="stamp">{shortStamp(metaString(f, 'ts'))}</div>
            </div>
          ))
        )}
      </Section>

      <Section
        open={open.todos}
        toggle={() => toggle('todos')}
        name="Todos"
        count={d?.todos.length ?? 0}
        loading={isLoading}
      >
        {d && d.todos.length === 0 ? (
          <Empty msg="No todos." />
        ) : (
          d?.todos.map((f) => {
            const status = metaString(f, 'task_status') ?? 'pending';
            return (
              <div className="frag-row" key={f.id}>
                <div className="text">
                  <span className={`todo-status ${status}`}>{status.replace('_', ' ')}</span>
                  {f.content}
                </div>
                <div className="stamp">{shortStamp(metaString(f, 'ts'))}</div>
              </div>
            );
          })
        )}
      </Section>

      <Section
        open={open.decisions}
        toggle={() => toggle('decisions')}
        name="Decisions"
        count={d?.decisions.length ?? 0}
        loading={isLoading}
      >
        {d && d.decisions.length === 0 ? (
          <Empty msg="No decisions recorded." />
        ) : (
          d?.decisions.map((f) => {
            const awaiting = f.metadata.awaiting_decision === true;
            return (
              <div className="frag-row" key={f.id}>
                <div className="text">
                  {f.content}
                  {awaiting && (
                    <div className="sub">
                      <span style={{ color: 'var(--urg-now)' }}>
                        ● awaiting_decision=true
                      </span>
                    </div>
                  )}
                </div>
                <div className="stamp">{shortStamp(metaString(f, 'ts'))}</div>
              </div>
            );
          })
        )}
      </Section>

      <Section
        open={open.blockers}
        toggle={() => toggle('blockers')}
        name="Blockers"
        count={d?.blockers.length ?? 0}
        loading={isLoading}
      >
        {d && d.blockers.length === 0 ? (
          <Empty msg="No blockers in this session." />
        ) : (
          d?.blockers.map((f) => (
            <div className="frag-row" key={f.id}>
              <div className="text">{f.content}</div>
              <div className="stamp">{shortStamp(metaString(f, 'ts'))}</div>
            </div>
          ))
        )}
      </Section>

      <Section
        open={open.user_actions}
        toggle={() => toggle('user_actions')}
        name="User actions"
        count={d?.userActions.length ?? 0}
        loading={isLoading}
      >
        {d && d.userActions.length === 0 ? (
          <Empty msg="No pending user actions." />
        ) : (
          d?.userActions.map((f) => {
            const urgency = (metaString(f, 'urgency') as 'now' | 'soon' | 'later') ?? 'later';
            return (
              <div className="frag-row" key={f.id}>
                <div className="text">
                  <UrgencyLabel urg={urgency} />{' '}
                  <span style={{ marginLeft: 8 }}>{f.content}</span>
                  {metaString(f, 'reason') && (
                    <div className="sub" style={{ marginTop: 4, color: 'var(--ink-muted)' }}>
                      {metaString(f, 'reason')}
                    </div>
                  )}
                </div>
                <div className="stamp">{shortStamp(metaString(f, 'ts'))}</div>
              </div>
            );
          })
        )}
      </Section>
    </div>
  );
}

function TrajectoryRow({
  record,
  compact = false,
}: {
  record: TrajectoryRecord;
  compact?: boolean;
}) {
  const status = recMetaString(record, 'status');
  const severity = recMetaString(record, 'severity');
  const scope = recMetaString(record, 'scope');
  return (
    <div className={`frag-row trajectory-row${compact ? ' compact' : ''}`}>
      <div className="text">
        <span className={`kind-badge ${record.kind}`}>{record.kind.replace('_', ' ')}</span>
        {status && <span className={`todo-status ${status}`}>{status.replace('_', ' ')}</span>}
        {severity && <span className="risk-chip">{severity}</span>}
        {scope && <span className="scope-chip">{scope}</span>}
        <span style={{ marginLeft: 8 }}>{record.content}</span>
        {recMetaString(record, 'evidence') && (
          <div className="sub">{recMetaString(record, 'evidence')}</div>
        )}
      </div>
      <div className="stamp">{shortStamp(record.ts ?? undefined)}</div>
    </div>
  );
}

function Section({
  open,
  toggle,
  name,
  count,
  hint,
  loading,
  children,
}: {
  open: boolean;
  toggle: () => void;
  name: string;
  count: number;
  hint?: string;
  loading?: boolean;
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
        <span className="ct">{loading ? '…' : count}</span>
      </div>
      {open && <div className="acc-body">{children}</div>}
    </div>
  );
}

function Empty({ msg }: { msg: string }) {
  return (
    <div className="empty">
      <div className="empty-title dim">{msg}</div>
    </div>
  );
}

function Stat({
  label,
  v,
  flag,
  loading,
}: {
  label: string;
  v: number;
  flag?: 'warn';
  loading?: boolean;
}) {
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
        {loading ? '…' : v.toLocaleString()}
      </div>
    </div>
  );
}
