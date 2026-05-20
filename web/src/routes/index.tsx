import { createFileRoute, Link } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import {
  Icon,
  KindBadge,
  ProjBadge,
  SessionPill,
  UrgencyDot,
  UrgencyLabel,
  type Urgency,
} from '@/components/atoms';
import type { InboxItemMock } from '@/lib/mock-data';
import { useInbox } from '@/hooks/useInbox';

export const Route = createFileRoute('/')({
  component: InboxPage,
});

const URG_RANK: Record<Urgency, number> = { now: 0, soon: 1, later: 2 };

function InboxPage() {
  const [urgFilter, setUrgFilter] = useState<'all' | Urgency>('all');
  const [projFilter, setProjFilter] = useState<string>('all');
  const [ackedIds, setAckedIds] = useState<Set<string>>(new Set());

  const { data: inboxData, isLoading, isError, error, refetch } = useInbox();

  // Show loading skeletons only on the very first load (no cached data yet)
  const showSkeleton = isLoading && inboxData.length === 0;

  const filteredItems = useMemo(
    () =>
      inboxData.filter((it) => {
        if (urgFilter !== 'all' && it.urgency !== urgFilter) return false;
        if (projFilter !== 'all' && it.project !== projFilter) return false;
        return true;
      }),
    [inboxData, urgFilter, projFilter],
  );

  const counts = useMemo(
    () => ({
      all: inboxData.length,
      now: inboxData.filter((i) => i.urgency === 'now').length,
      soon: inboxData.filter((i) => i.urgency === 'soon').length,
      later: inboxData.filter((i) => i.urgency === 'later').length,
    }),
    [inboxData],
  );

  const { grouped, sessionOrder } = useMemo(() => {
    const g: Record<string, InboxItemMock[]> = {};
    for (const it of filteredItems) {
      (g[it.sessionId] ||= []).push(it);
    }
    Object.values(g).forEach((arr) =>
      arr.sort((a, b) => URG_RANK[a.urgency] - URG_RANK[b.urgency] || a.step - b.step),
    );
    const order = Object.keys(g).sort((a, b) => {
      const ra = Math.min(...g[a].map((x) => URG_RANK[x.urgency]));
      const rb = Math.min(...g[b].map((x) => URG_RANK[x.urgency]));
      return ra - rb;
    });
    return { grouped: g, sessionOrder: order };
  }, [filteredItems]);

  const projects = useMemo(() => Array.from(new Set(inboxData.map((i) => i.project))), [inboxData]);

  const ack = (id: string) => setAckedIds((prev) => new Set(prev).add(id));

  const activeSessions = new Set(inboxData.map((i) => i.sessionId)).size;

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Inbox</h1>
          <div className="page-sub">
            What Claude needs from you, across <span className="mono">{activeSessions}</span> active
            sessions
          </div>
        </div>
        <div className="page-actions">
          <button className="btn btn-ghost" title="Mark all as acknowledged" type="button">
            <Icon.Check /> Ack all visible
          </button>
        </div>
      </div>

      <div className="filter-bar">
        <div className="tabs">
          {(['all', 'now', 'soon', 'later'] as const).map((u) => (
            <button
              key={u}
              className={urgFilter === u ? 'active' : ''}
              onClick={() => setUrgFilter(u)}
              type="button"
            >
              {u !== 'all' && <span className={`urg-dot urg-${u}`} style={{ marginTop: 0 }} />}
              {u === 'all' ? 'All' : u[0].toUpperCase() + u.slice(1)}{' '}
              <span className="pill">{counts[u]}</span>
            </button>
          ))}
        </div>
        <div className="grow" />
        <ProjectSelect value={projFilter} onChange={setProjFilter} options={projects} />
      </div>

      {showSkeleton && (
        <>
          <div className="skel-card" />
          <div className="skel-card" />
          <div className="skel-card" />
        </>
      )}

      {!showSkeleton && isError && (
        <div className="empty with-card error-card">
          <Icon.Alert style={{ width: 28, height: 28, color: 'var(--urg-now)', opacity: 0.8 }} />
          <div className="empty-title">Inbox fetch failed</div>
          <div className="empty-body mono" style={{ color: 'var(--ink-muted)' }}>
            {error?.message ?? 'Unknown error'}
          </div>
          <button
            className="btn btn-primary"
            onClick={refetch}
            style={{ marginTop: 4 }}
            type="button"
          >
            <Icon.Refresh /> Retry
          </button>
        </div>
      )}

      {!showSkeleton && !isError && sessionOrder.length === 0 && (
        <div className="empty with-card">
          <div className="empty-title">Nothing matches this filter</div>
          <div className="empty-body">Clear the urgency or project filter to see more items.</div>
        </div>
      )}

      {!showSkeleton &&
        sessionOrder.map((sid) => {
          const sessionItem = inboxData.find((i) => i.sessionId === sid);
          return (
            <div className="session-group" key={sid}>
              <div className="session-group-head">
                <span className="sid">{sid}</span>
                <span className="proj">~/code/{sessionItem?.project ?? '?'}</span>
                <span className="meta">{grouped[sid].length} waiting</span>
              </div>
              {grouped[sid].map((it) => (
                <InboxCard key={it.id} item={it} ack={ack} acked={ackedIds.has(it.id)} />
              ))}
            </div>
          );
        })}
    </div>
  );
}

function ProjectSelect({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: string[];
}) {
  const [open, setOpen] = useState(false);
  return (
    <div style={{ position: 'relative' }}>
      <button className="btn" onClick={() => setOpen((o) => !o)} type="button">
        <Icon.Folder className="ic" />
        <span>
          project: <span className="mono">{value}</span>
        </span>
        <Icon.Chevron style={{ transform: 'rotate(90deg)', color: 'var(--ink-faint)' }} />
      </button>
      {open && (
        <div
          style={{
            position: 'absolute',
            right: 0,
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
          {['all', ...options].map((o) => (
            <div
              key={o}
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
              {o}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function InboxCard({
  item,
  ack,
  acked,
}: {
  item: InboxItemMock;
  ack: (id: string) => void;
  acked: boolean;
}) {
  const isDecision = item.kind === 'decision';
  return (
    <article
      className={`inbox-card${item.isNew ? ' new' : ''}${acked ? ' ack' : ''}`}
      data-testid="cn-inbox-card"
    >
      <UrgencyDot urg={item.urgency} />
      <div className="body">
        <div className="head">
          <UrgencyLabel urg={item.urgency} />
          <KindBadge kind={item.kind} />
          {item.isNew && !acked && (
            <span
              className="mono"
              style={{ fontSize: 10, color: 'var(--accent)', letterSpacing: '0.06em' }}
            >
              ● NEW
            </span>
          )}
          <span className="mono dim" style={{ fontSize: 10.5, marginLeft: 'auto' }}>
            step {item.step}
          </span>
        </div>
        <div className="action">
          {isDecision && <span style={{ color: 'var(--urg-soon)', marginRight: 6 }}>?</span>}
          {item.action}
        </div>
        <div className="reason">
          <Icon.Info className="ic" />
          <span>{item.reason}</span>
        </div>
        {isDecision && item.decision && (
          <div className="reason" style={{ marginTop: 4, color: 'var(--ink)' }}>
            <Icon.Question className="ic" />
            <span className="mono" style={{ fontSize: 11.5 }}>
              {item.decision}
            </span>
          </div>
        )}
        <div className="footer-meta">
          <Link to="/sessions/$id" params={{ id: item.sessionId }}>
            <SessionPill id={item.sessionId} />
          </Link>
          <ProjBadge p={item.project} />
          <span>
            <Icon.Clock style={{ marginRight: 4, verticalAlign: '-1px' }} />
            {item.ago}
          </span>
          <span className="mono dim">{item.id}</span>
        </div>
      </div>
      <div className="actions">
        <button className="btn sm btn-ghost" title="Open source turn" type="button">
          <Icon.ArrowRight />
        </button>
        <button className="btn sm" title="Snooze 1h" type="button">
          snooze
        </button>
        <button
          className="btn sm btn-primary"
          onClick={() => ack(item.id)}
          disabled={acked}
          type="button"
        >
          <Icon.Check /> ack
        </button>
      </div>
    </article>
  );
}
