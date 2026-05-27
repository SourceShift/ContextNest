import { createFileRoute, Link } from '@tanstack/react-router';
import { type ReactNode, useEffect, useMemo, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { Icon, ProjBadge, SessionPill } from '@/components/atoms';
import { ApiError, api } from '@/lib/api';
import { useKnownProjects, useSessions } from '@/hooks/useSessions';
import { agoFrom } from '@/lib/relative-time';
import type { SessionListItem, TrajectoryResponse } from '@/lib/types';

export const Route = createFileRoute('/trajectories')({
  component: TrajectoriesPage,
});

type TrajectorySession = {
  session: SessionListItem;
  project: string;
  trajectory: TrajectoryResponse;
  lastActivity: number;
};

type TrajectoryFilter = 'all' | 'promotion' | 'risks' | 'directives';
type SortMode = 'priority' | 'newest' | 'oldest' | 'records' | 'promotions' | 'risks';
type DateRange = 'all' | '1h' | '24h' | '7d' | '30d';
type Chip = { k: string; v: string };
const CHIP_KEYS = ['project', 'session', 'kind'] as const;
type ChipKey = (typeof CHIP_KEYS)[number];

type TrajectoryScanResult = {
  rows: TrajectorySession[];
  unavailable: number;
  failed: number;
};

const MAX_SESSIONS_FANOUT = 80;
const TRAJECTORY_KIND_VALUES = [
  'read_context',
  'verification',
  'evidence_ref',
  'decision_made',
  'failure',
  'prompt_directive',
  'assumption',
  'artifact',
  'memory_candidate',
  'risk_flag',
] as const;
const SORT_LABELS: Record<SortMode, string> = {
  priority: 'priority signal',
  newest: 'newest activity',
  oldest: 'oldest activity',
  records: 'record count',
  promotions: 'promotion queue',
  risks: 'risk flags',
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

function basename(p: string | null | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

function timeValue(ts: string | null | undefined): number {
  if (!ts) return 0;
  const n = Date.parse(ts);
  return Number.isNaN(n) ? 0 : n;
}

async function fetchTrajectorySession(
  session: SessionListItem,
): Promise<TrajectorySession | 'unavailable' | 'failed' | null> {
  try {
    const trajectory = await api.sessionTrajectory(session.id);
    if (trajectory.trajectory_count === 0) return null;
    return {
      session,
      project: basename(session.project_cwd),
      trajectory,
      lastActivity: timeValue(session.last_ts),
    };
  } catch (err) {
    if (err instanceof ApiError && err.status === 404) return 'unavailable';
    return 'failed';
  }
}

function TrajectoriesPage() {
  const sessions = useSessions();
  const knownProjects = useKnownProjects();
  const [filter, setFilter] = useState<TrajectoryFilter>('all');
  const [chips, setChips] = useState<Chip[]>([]);
  const [sortMode, setSortMode] = useState<SortMode>('priority');
  const [dateRange, setDateRange] = useState<DateRange>('all');
  const [scanLimit, setScanLimit] = useState<number>(MAX_SESSIONS_FANOUT);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const removeChip = (i: number) => setChips((c) => c.filter((_, j) => j !== i));
  const addChip = (k: string, v: string) =>
    setChips((c) => (c.some((x) => x.k === k && x.v === v) ? c : [...c, { k, v }]));

  const chipsByKey = useMemo(() => {
    const m: Record<ChipKey, string[]> = {
      project: [],
      session: [],
      kind: [],
    };
    for (const c of chips) {
      if ((CHIP_KEYS as readonly string[]).includes(c.k)) {
        m[c.k as ChipKey].push(c.v);
      }
    }
    return m;
  }, [chips]);

  const targetSessions = useMemo(() => {
    const cutoff = dateRange === 'all' ? null : nowMs - DATE_MS[dateRange];
    const scoped =
      chipsByKey.session.length > 0
        ? sessions.data.filter((s) => chipsByKey.session.includes(s.id))
        : sessions.data.filter((s) => {
            if (
              chipsByKey.project.length > 0 &&
              !chipsByKey.project.includes(basename(s.project_cwd))
            ) {
              return false;
            }
            if (cutoff !== null && timeValue(s.last_ts) < cutoff) {
              return false;
            }
            return true;
          });

    const sorted = [...scoped].sort((a, b) => timeValue(b.last_ts) - timeValue(a.last_ts));
    return sorted.slice(0, scanLimit);
  }, [chipsByKey.project, chipsByKey.session, dateRange, nowMs, scanLimit, sessions.data]);

  const trajectoriesQuery = useQuery({
    queryKey: ['trajectories', targetSessions.map((s) => s.id)],
    enabled: targetSessions.length > 0,
    staleTime: 15_000,
    refetchInterval: 60_000,
    queryFn: async (): Promise<TrajectoryScanResult> => {
      const scanned = await Promise.all(targetSessions.map(fetchTrajectorySession));
      const rows = scanned.filter(
        (row): row is TrajectorySession =>
          row !== null && row !== 'unavailable' && row !== 'failed',
      );
      return {
        rows,
        unavailable: scanned.filter((row) => row === 'unavailable').length,
        failed: scanned.filter((row) => row === 'failed').length,
      };
    },
  });

  const scan = trajectoriesQuery.data ?? { rows: [], unavailable: 0, failed: 0 };
  const rows = scan.rows;
  const visibleRows = useMemo(() => {
    const filtered = rows.filter((row) => {
      if (filter === 'promotion' && row.trajectory.promotion_queue.length === 0) return false;
      if (filter === 'risks' && row.trajectory.cost_profile.risk_flags === 0) return false;
      if (filter === 'directives' && row.trajectory.cost_profile.prompt_directives === 0) {
        return false;
      }
      if (
        chipsByKey.kind.length > 0 &&
        !row.trajectory.records.some((record) => chipsByKey.kind.includes(record.kind))
      ) {
        return false;
      }
      return true;
    });
    const sorted = filtered.slice();
    sorted.sort((a, b) => compareTrajectoryRows(a, b, sortMode));
    return sorted;
  }, [chipsByKey.kind, filter, rows, sortMode]);

  const totals = useMemo(
    () =>
      visibleRows.reduce(
        (acc, row) => {
          acc.records += row.trajectory.trajectory_count;
          acc.promotions += row.trajectory.promotion_queue.length;
          acc.risks += row.trajectory.cost_profile.risk_flags;
          acc.directives += row.trajectory.cost_profile.prompt_directives;
          return acc;
        },
        { records: 0, promotions: 0, risks: 0, directives: 0 },
      ),
    [visibleRows],
  );

  const isLoading = sessions.isLoading || trajectoriesQuery.isLoading;
  const scopedCount = targetSessions.length;
  const hiddenByLimit = useMemo(() => {
    if (chipsByKey.session.length > 0) return 0;
    const cutoff = dateRange === 'all' ? null : nowMs - DATE_MS[dateRange];
    const eligible = sessions.data.filter((s) => {
      if (chipsByKey.project.length > 0 && !chipsByKey.project.includes(basename(s.project_cwd))) {
        return false;
      }
      if (cutoff !== null && timeValue(s.last_ts) < cutoff) return false;
      return true;
    });
    return Math.max(0, eligible.length - targetSessions.length);
  }, [
    chipsByKey.project,
    chipsByKey.session.length,
    dateRange,
    nowMs,
    sessions.data,
    targetSessions.length,
  ]);

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Trajectories</h1>
          <div className="page-sub">
            Sessions with sparse trajectory signals · ranked by promotion queue, risks, and signal
            density · <span className="mono">{scopedCount}</span> sessions scanned
          </div>
        </div>
        <div className="page-actions">
          <button
            className="btn"
            onClick={() => {
              setNowMs(Date.now());
              void trajectoriesQuery.refetch();
            }}
            type="button"
          >
            <Icon.Refresh /> Refresh
          </button>
        </div>
      </div>

      <div className="filter-bar" style={{ paddingTop: 0, paddingBottom: 8, flexWrap: 'wrap' }}>
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
          knownSessions={sessions.data.map((s) => s.id).slice(0, 50)}
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
          {isLoading
            ? 'loading...'
            : `${visibleRows.length} shown · ${chips.length} filters · ${scopedCount} session${
                scopedCount === 1 ? '' : 's'
              } scanned`}
        </span>
      </div>

      <div className="filter-bar" style={{ paddingTop: 0, paddingBottom: 14, flexWrap: 'wrap' }}>
        <SortMenu value={sortMode} onChange={setSortMode} />
        <DateRangeMenu
          value={dateRange}
          onChange={(value) => {
            setNowMs(Date.now());
            setDateRange(value);
          }}
        />
        <LimitMenu value={scanLimit} onChange={setScanLimit} />
        <div className="grow" />
        {chipsByKey.kind.length > 0 && (
          <span className="mono dim" style={{ fontSize: 10.5, color: 'var(--ink-faint)' }}>
            kind filter applies after trajectory payloads load
          </span>
        )}
      </div>

      {hiddenByLimit > 0 && (
        <div className="note-banner" style={{ marginBottom: 18 }}>
          <span className="dot" />
          <span>
            Showing trajectories from the {scanLimit} most recent matching sessions ·{' '}
            <span className="mono dim">{hiddenByLimit}</span> older matching session
            {hiddenByLimit === 1 ? '' : 's'} not scanned.
          </span>
        </div>
      )}

      <div className="trajectory-overview">
        <Metric label="sessions" value={visibleRows.length} />
        <Metric label="records" value={totals.records} />
        <Metric label="promotion queue" value={totals.promotions} warn={totals.promotions > 0} />
        <Metric label="risk flags" value={totals.risks} warn={totals.risks > 0} />
        <Metric label="directives" value={totals.directives} />
      </div>

      {scan.unavailable > 0 && (
        <div className="note-banner" style={{ marginBottom: 18 }}>
          <span className="dot" style={{ background: 'var(--urg-now)' }} />
          <span>
            Trajectory endpoint returned 404 for{' '}
            <span className="mono dim">{scan.unavailable}</span> scanned session
            {scan.unavailable === 1 ? '' : 's'}. The frontend has the route, but the running backend
            likely needs to be rebuilt/restarted from this checkout.
          </span>
        </div>
      )}

      {scan.failed > 0 && (
        <div className="note-banner" style={{ marginBottom: 18 }}>
          <span className="dot" style={{ background: 'var(--urg-soon)' }} />
          <span>
            Failed to fetch trajectories for <span className="mono dim">{scan.failed}</span> scanned
            session{scan.failed === 1 ? '' : 's'}.
          </span>
        </div>
      )}

      <div className="features-filter-bar">
        <span className="filter-label mono">view</span>
        {[
          ['all', 'all'],
          ['promotion', 'promotion'],
          ['risks', 'risks'],
          ['directives', 'directives'],
        ].map(([key, label]) => (
          <button
            key={key}
            className={`btn-chip${filter === key ? ' active' : ''}`}
            onClick={() => setFilter(key as TrajectoryFilter)}
            type="button"
          >
            {label}
          </button>
        ))}
        <div className="grow" />
        <span className="mono dim">
          {isLoading
            ? 'loading...'
            : `${visibleRows.length} session${visibleRows.length === 1 ? '' : 's'}`}
        </span>
      </div>

      {trajectoriesQuery.isError && (
        <div className="empty with-card">
          <div className="empty-title">Could not load trajectories</div>
          <div className="empty-body">{trajectoriesQuery.error?.message ?? 'Unknown error'}</div>
        </div>
      )}

      {!trajectoriesQuery.isError && visibleRows.length === 0 && !isLoading && (
        <div className="empty with-card">
          <div className="empty-title">No trajectory sessions in this view</div>
          <div className="empty-body">
            New sessions appear here after z-insight emits sparse trajectory arrays such as{' '}
            <span className="mono">verification[]</span>, <span className="mono">decisions[]</span>,{' '}
            <span className="mono">risk_flags[]</span>, or{' '}
            <span className="mono">memory_candidates[]</span>.
          </div>
        </div>
      )}

      <div className="trajectory-session-list">
        {visibleRows.map((row) => (
          <TrajectorySessionCard key={row.session.id} row={row} />
        ))}
      </div>
    </div>
  );
}

function compareTrajectoryRows(a: TrajectorySession, b: TrajectorySession, sortMode: SortMode) {
  const ap = a.trajectory.promotion_queue.length;
  const bp = b.trajectory.promotion_queue.length;
  const ar = a.trajectory.cost_profile.risk_flags;
  const br = b.trajectory.cost_profile.risk_flags;

  switch (sortMode) {
    case 'newest':
      return b.lastActivity - a.lastActivity || defaultPriorityCompare(a, b);
    case 'oldest':
      return a.lastActivity - b.lastActivity || defaultPriorityCompare(a, b);
    case 'records':
      return (
        b.trajectory.trajectory_count - a.trajectory.trajectory_count ||
        defaultPriorityCompare(a, b)
      );
    case 'promotions':
      return bp - ap || defaultPriorityCompare(a, b);
    case 'risks':
      return br - ar || defaultPriorityCompare(a, b);
    case 'priority':
    default:
      return defaultPriorityCompare(a, b);
  }
}

function defaultPriorityCompare(a: TrajectorySession, b: TrajectorySession) {
  const ap = a.trajectory.promotion_queue.length;
  const bp = b.trajectory.promotion_queue.length;
  const ar = a.trajectory.cost_profile.risk_flags;
  const br = b.trajectory.cost_profile.risk_flags;
  return (
    bp - ap ||
    br - ar ||
    b.trajectory.trajectory_count - a.trajectory.trajectory_count ||
    b.lastActivity - a.lastActivity
  );
}

function Metric({ label, value, warn }: { label: string; value: number; warn?: boolean }) {
  return (
    <div className={`trajectory-metric${warn ? ' warn' : ''}`}>
      <span className="mono dim">{label}</span>
      <b>{value.toLocaleString()}</b>
    </div>
  );
}

function TrajectorySessionCard({ row }: { row: TrajectorySession }) {
  const counts = row.trajectory.cost_profile;
  const topKinds = Object.entries(row.session.by_kind)
    .filter(([kind]) =>
      [
        'decision_made',
        'failure',
        'verification',
        'prompt_directive',
        'assumption',
        'memory_candidate',
        'risk_flag',
      ].includes(kind),
    )
    .sort((a, b) => b[1] - a[1])
    .slice(0, 5);

  return (
    <div className="trajectory-session-card">
      <div className="trajectory-session-main">
        <div>
          <div className="trajectory-session-title">
            <SessionPill id={row.session.id} />
            <ProjBadge p={row.project} />
            <span className="mono dim">
              {row.session.last_ts ? agoFrom(row.session.last_ts) : '—'}
            </span>
          </div>
          <div className="trajectory-session-sub">
            {row.trajectory.phases[0]?.goal ?? 'No phase label recorded'}
          </div>
        </div>
        <Link to="/sessions/$id" params={{ id: row.session.id }} className="btn btn-ghost sm">
          Open <Icon.ArrowRight />
        </Link>
      </div>

      <div className="trajectory-session-stats">
        <span>
          <b>{row.trajectory.trajectory_count}</b> records
        </span>
        <span>
          <b>{row.trajectory.phases.length}</b> phases
        </span>
        <span>
          <b>{row.trajectory.promotion_queue.length}</b> promotions
        </span>
        <span>
          <b>{counts.risk_flags}</b> risks
        </span>
        <span>
          <b>{counts.records_per_turn.toFixed(2)}</b> / turn
        </span>
      </div>

      {topKinds.length > 0 && (
        <div className="trajectory-kind-strip">
          {topKinds.map(([kind, count]) => (
            <span key={kind} className={`kind-badge ${kind}`}>
              {kind.replace('_', ' ')} · {count}
            </span>
          ))}
        </div>
      )}
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
  const [key, setKey] = useState<ChipKey | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  const valuesFor: Record<ChipKey, readonly string[]> = {
    project: knownProjects,
    session: knownSessions,
    kind: TRAJECTORY_KIND_VALUES,
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
            minWidth: 220,
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
      options={['priority', 'newest', 'oldest', 'records', 'promotions', 'risks'] as const}
      optionLabel={(v) => SORT_LABELS[v]}
      onChange={onChange}
      title="Sort order"
      minWidth={210}
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
      label="scan"
      value={value}
      options={[10, 25, 50, 80, 100, 200] as const}
      optionLabel={(v) => `${v} sessions`}
      onChange={onChange}
      title="Maximum sessions to scan"
      minWidth={150}
    />
  );
}
