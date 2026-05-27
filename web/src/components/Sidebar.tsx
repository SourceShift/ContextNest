import { Link, useRouterState } from '@tanstack/react-router';

import { BrandMark, Icon } from './atoms';
import { useFeatures } from '@/hooks/useFeatures';
import { useInbox } from '@/hooks/useInbox';
import { useSessions } from '@/hooks/useSessions';
import { useStats } from '@/hooks/useStats';

type NavKey =
  | 'inbox'
  | 'sessions'
  | 'search'
  | 'phases'
  | 'trajectories'
  | 'features'
  | 'field'
  | 'tools'
  | 'substrate';

type NavItem = {
  k: NavKey;
  to:
    | '/'
    | '/sessions'
    | '/search'
    | '/phases'
    | '/trajectories'
    | '/features'
    | '/field'
    | '/tools'
    | '/substrate';
  label: string;
  icon: React.ReactNode;
  kbd: string;
  count?: number;
  urgent?: boolean;
};

export function Sidebar() {
  const { location } = useRouterState();
  const pathname = location.pathname;
  const inbox = useInbox();
  const sessions = useSessions();
  const stats = useStats();
  // Features count is the 24h window — same default the /features page
  // uses on first load, so the badge matches what the user will see
  // when they click through.
  const features = useFeatures({ since: '24h' });

  // Phases count is derived from substrate stats — every kind=goal_phase
  // fragment is a "phase" in the dashboard's vocabulary. When the stats
  // endpoint hasn't returned yet we just omit the badge rather than
  // flashing a 0; once it lands the badge appears.
  const phasesCount = stats.data?.by_kind.goal_phase;
  const trajectoryCount =
    (stats.data?.by_kind.decision_made ?? 0) +
    (stats.data?.by_kind.failure ?? 0) +
    (stats.data?.by_kind.verification ?? 0) +
    (stats.data?.by_kind.prompt_directive ?? 0) +
    (stats.data?.by_kind.assumption ?? 0) +
    (stats.data?.by_kind.memory_candidate ?? 0) +
    (stats.data?.by_kind.risk_flag ?? 0);
  const urgent = inbox.data.some((i) => i.urgency === 'now');

  const nav: NavItem[] = [
    {
      k: 'inbox',
      to: '/',
      label: 'Inbox',
      icon: <Icon.Inbox className="nav-icon" />,
      kbd: 'g i',
      count: inbox.data.length,
      urgent,
    },
    {
      k: 'sessions',
      to: '/sessions',
      label: 'Sessions',
      icon: <Icon.List className="nav-icon" />,
      kbd: 'g s',
      count: sessions.data.length,
    },
    {
      k: 'search',
      to: '/search',
      label: 'Search',
      icon: <Icon.Search className="nav-icon" />,
      kbd: 'g /',
    },
    {
      k: 'phases',
      to: '/phases',
      label: 'Phases',
      icon: <Icon.Layers className="nav-icon" />,
      kbd: 'g p',
      count: phasesCount,
    },
    {
      k: 'trajectories',
      to: '/trajectories',
      label: 'Trajectories',
      icon: <Icon.Hash className="nav-icon" />,
      kbd: 'g j',
      count: trajectoryCount || undefined,
    },
    {
      k: 'features',
      to: '/features',
      label: 'Features',
      icon: <Icon.Check className="nav-icon" />,
      kbd: 'g e',
      count: features.data?.count,
    },
    {
      k: 'field',
      to: '/field',
      label: 'Field',
      icon: <Icon.Atom className="nav-icon" />,
      kbd: 'g f',
    },
    {
      k: 'tools',
      to: '/tools',
      label: 'Tools',
      icon: <Icon.Terminal className="nav-icon" />,
      kbd: 'g t',
    },
    {
      k: 'substrate',
      to: '/substrate',
      label: 'Substrate',
      icon: <Icon.Cpu className="nav-icon" />,
      kbd: 'g o',
    },
  ];

  return (
    <aside className="sidebar" role="navigation">
      <div className="sidebar-brand">
        <BrandMark size={22} />
        <div>
          <div className="sidebar-brand-text">ContextNest</div>
          <div className="sidebar-brand-sub">v0.2.0-rc.3</div>
        </div>
      </div>
      <div className="sidebar-section-label">Views</div>
      {nav.map((it) => {
        const active = it.to === '/' ? pathname === '/' : pathname.startsWith(it.to);
        return (
          <Link
            key={it.k}
            to={it.to}
            className={`nav-item${active ? ' active' : ''}${
              it.k === 'inbox' && it.urgent ? ' has-urgent' : ''
            }`}
          >
            {it.icon}
            <span>{it.label}</span>
            {it.count !== undefined ? (
              <span className="nav-count">{it.count.toLocaleString()}</span>
            ) : (
              <span className="nav-kbd">{it.kbd}</span>
            )}
          </Link>
        );
      })}

      <div className="sidebar-footer">
        <div className="row">
          <span>origin</span>
          <span className="v">localhost:28080</span>
        </div>
        <div className="row">
          <span>fragments</span>
          <span className="v">{stats.data?.total_fragments?.toLocaleString() ?? '—'}</span>
        </div>
        <div className="row">
          <span>embed</span>
          <span className="v">tf-idf · 256d</span>
        </div>
        <div className="row" style={{ marginTop: 4, color: 'var(--ink-faint)' }}>
          <span>↑↓ select</span>
          <span>↵ open</span>
        </div>
      </div>
    </aside>
  );
}
