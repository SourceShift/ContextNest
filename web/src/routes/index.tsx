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
import { useKnownProjects } from '@/hooks/useSessions';

export const Route = createFileRoute('/')({
  component: InboxPage,
});

const URG_RANK: Record<Urgency, number> = { now: 0, soon: 1, later: 2 };

type SortMode = 'newest' | 'oldest' | 'urgency';
type KindFilter = 'all' | 'user_action' | 'decision';

// Parse the stored ISO ts to epoch ms. Missing/invalid → -Infinity so it
// sorts last under 'newest' (newest first) and first under 'oldest'.
function tsMs(stored: string): number {
  if (!stored) return -Infinity;
  const t = Date.parse(stored);
  return Number.isFinite(t) ? t : -Infinity;
}

function InboxPage() {
  const [urgFilter, setUrgFilter] = useState<'all' | Urgency>('all');
  const [projFilter, setProjFilter] = useState<string>('all');
  const [sessionFilter, setSessionFilter] = useState<string>('all');
  const [kindFilter, setKindFilter] = useState<KindFilter>('all');
  const [sortMode, setSortMode] = useState<SortMode>('newest');
  const [ackedIds, setAckedIds] = useState<Set<string>>(new Set());

  const { data: inboxData, isLoading, isError, error, refetch } = useInbox();

  // Show loading skeletons only on the very first load (no cached data yet)
  const showSkeleton = isLoading && inboxData.length === 0;

  const filteredItems = useMemo(
    () =>
      inboxData.filter((it) => {
        if (urgFilter !== 'all' && it.urgency !== urgFilter) return false;
        if (projFilter !== 'all' && it.project !== projFilter) return false;
        if (sessionFilter !== 'all' && it.sessionId !== sessionFilter) return false;
        if (kindFilter !== 'all' && it.kind !== kindFilter) return false;
        return true;
      }),
    [inboxData, urgFilter, projFilter, sessionFilter, kindFilter],
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

  const kindCounts = useMemo(
    () => ({
      all: inboxData.length,
      user_action: inboxData.filter((i) => i.kind === 'user_action').length,
      decision: inboxData.filter((i) => i.kind === 'decision').length,
    }),
    [inboxData],
  );

  // Flat sorted list — used by 'newest' / 'oldest' modes.
  const flatItems = useMemo(() => {
    const arr = filteredItems.slice();
    if (sortMode === 'newest') {
      arr.sort((a, b) => tsMs(b.stored) - tsMs(a.stored) || a.id.localeCompare(b.id));
    } else if (sortMode === 'oldest') {
      arr.sort((a, b) => tsMs(a.stored) - tsMs(b.stored) || a.id.localeCompare(b.id));
    }
    return arr;
  }, [filteredItems, sortMode]);

  // Grouped-by-session view — used by 'urgency' mode (legacy triage layout).
  // Within each session: urgency rank → step → ts desc.
  // Across sessions: min(urgency rank) → newest ts in group.
  const { grouped, sessionOrder } = useMemo(() => {
    const g: Record<string, InboxItemMock[]> = {};
    for (const it of filteredItems) {
      (g[it.sessionId] ||= []).push(it);
    }
    Object.values(g).forEach((arr) =>
      arr.sort(
        (a, b) =>
          URG_RANK[a.urgency] - URG_RANK[b.urgency] ||
          a.step - b.step ||
          tsMs(b.stored) - tsMs(a.stored),
      ),
    );
    const order = Object.keys(g).sort((a, b) => {
      const ra = Math.min(...g[a].map((x) => URG_RANK[x.urgency]));
      const rb = Math.min(...g[b].map((x) => URG_RANK[x.urgency]));
      if (ra !== rb) return ra - rb;
      const ta = Math.max(...g[a].map((x) => tsMs(x.stored)));
      const tb = Math.max(...g[b].map((x) => tsMs(x.stored)));
      return tb - ta;
    });
    return { grouped: g, sessionOrder: order };
  }, [filteredItems]);

  // Projects in the dropdown derive from EVERY known session (not just
  // sessions that have inbox-eligible items), so a project with no open
  // user_actions / awaiting_decisions still appears as a filter option.
  // Falls back to inbox-derived projects if the /api/v1/sessions call
  // fails for any reason.
  const knownProjects = useKnownProjects();
  const inboxProjects = useMemo(
    () => Array.from(new Set(inboxData.map((i) => i.project))),
    [inboxData],
  );
  const projects = useMemo(
    () => Array.from(new Set([...knownProjects, ...inboxProjects])).sort(),
    [knownProjects, inboxProjects],
  );

  // Session-id dropdown options: every session that contributes to the
  // CURRENT (project+urgency+kind)-filtered view, ordered newest-first by
  // the freshest item in that session.
  const sessionOptions = useMemo(() => {
    const preSession = inboxData.filter((it) => {
      if (urgFilter !== 'all' && it.urgency !== urgFilter) return false;
      if (projFilter !== 'all' && it.project !== projFilter) return false;
      if (kindFilter !== 'all' && it.kind !== kindFilter) return false;
      return true;
    });
    const freshness: Record<string, number> = {};
    for (const it of preSession) {
      const t = tsMs(it.stored);
      if (!(it.sessionId in freshness) || t > freshness[it.sessionId]) {
        freshness[it.sessionId] = t;
      }
    }
    return Object.entries(freshness)
      .sort((a, b) => b[1] - a[1])
      .map(([id]) => id);
  }, [inboxData, urgFilter, projFilter, kindFilter]);

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
        <div className="tabs" style={{ marginLeft: 8 }}>
          {(['all', 'user_action', 'decision'] as const).map((k) => (
            <button
              key={k}
              className={kindFilter === k ? 'active' : ''}
              onClick={() => setKindFilter(k)}
              type="button"
              title={k === 'all' ? 'All kinds' : `kind = ${k}`}
            >
              {k === 'all' ? 'All kinds' : k.replace('_', ' ')}{' '}
              <span className="pill">{kindCounts[k]}</span>
            </button>
          ))}
        </div>
        <div className="grow" />
        <SortSelect value={sortMode} onChange={setSortMode} />
        <SessionSelect
          value={sessionFilter}
          onChange={setSessionFilter}
          options={sessionOptions}
        />
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

      {!showSkeleton &&
        !isError &&
        ((sortMode === 'urgency' && sessionOrder.length === 0) ||
          (sortMode !== 'urgency' && flatItems.length === 0)) && (
          <div className="empty with-card">
            <div className="empty-title">Nothing matches this filter</div>
            <div className="empty-body">
              Clear urgency / kind / session / project filters to see more items.
            </div>
          </div>
        )}

      {!showSkeleton &&
        sortMode === 'urgency' &&
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

      {!showSkeleton &&
        sortMode !== 'urgency' &&
        flatItems.map((it) => (
          <InboxCard key={it.id} item={it} ack={ack} acked={ackedIds.has(it.id)} />
        ))}
    </div>
  );
}

function SortSelect({
  value,
  onChange,
}: {
  value: SortMode;
  onChange: (v: SortMode) => void;
}) {
  const [open, setOpen] = useState(false);
  const label: Record<SortMode, string> = {
    newest: 'newest first',
    oldest: 'oldest first',
    urgency: 'urgency (grouped)',
  };
  return (
    <div style={{ position: 'relative' }}>
      <button className="btn" onClick={() => setOpen((o) => !o)} type="button" title="Sort order">
        <Icon.Clock className="ic" />
        <span>
          sort: <span className="mono">{label[value]}</span>
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
            minWidth: 200,
            zIndex: 10,
            boxShadow: 'var(--shadow-pop)',
          }}
        >
          {(['newest', 'oldest', 'urgency'] as const).map((m) => (
            <div
              key={m}
              onClick={() => {
                onChange(m);
                setOpen(false);
              }}
              style={{
                padding: '6px 10px',
                borderRadius: 4,
                cursor: 'pointer',
                background: value === m ? 'var(--surface-3)' : 'transparent',
                fontFamily: 'var(--font-mono)',
                fontSize: 12,
                color: value === m ? 'var(--ink)' : 'var(--ink-muted)',
              }}
            >
              {label[m]}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function SessionSelect({
  value,
  onChange,
  options,
}: {
  value: string;
  onChange: (v: string) => void;
  options: string[];
}) {
  const [open, setOpen] = useState(false);
  const short = (id: string) => {
    if (id === 'all') return 'all';
    // cc-46d752ff-1933-4088-8af8-a44c106af45a -> cc-46d752ff…f45a
    const stripped = id.replace(/^cc-/, '');
    const head = stripped.slice(0, 8);
    const tail = stripped.slice(-4);
    return `cc-${head}…${tail}`;
  };
  return (
    <div style={{ position: 'relative' }}>
      <button
        className="btn"
        onClick={() => setOpen((o) => !o)}
        type="button"
        title="Filter by session"
      >
        <Icon.ArrowRight className="ic" />
        <span>
          session: <span className="mono">{short(value)}</span>
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
            minWidth: 240,
            maxHeight: 320,
            overflowY: 'auto',
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
              title={o}
            >
              {short(o)}
            </div>
          ))}
        </div>
      )}
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
