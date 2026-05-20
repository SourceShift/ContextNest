import { Link, useRouterState } from '@tanstack/react-router';

import { BrandMark, Icon } from './atoms';

type NavKey = 'inbox' | 'sessions' | 'search' | 'phases' | 'field' | 'tools' | 'substrate';

type NavItem = {
  k: NavKey;
  to: '/' | '/sessions' | '/search' | '/phases' | '/field' | '/tools' | '/substrate';
  label: string;
  icon: React.ReactNode;
  kbd: string;
  count?: number;
  urgent?: boolean;
};

const NAV: NavItem[] = [
  {
    k: 'inbox',
    to: '/',
    label: 'Inbox',
    icon: <Icon.Inbox className="nav-icon" />,
    kbd: 'g i',
    count: 4,
    urgent: true,
  },
  {
    k: 'sessions',
    to: '/sessions',
    label: 'Sessions',
    icon: <Icon.List className="nav-icon" />,
    kbd: 'g s',
    count: 6,
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
    count: 22,
  },
  {
    k: 'field',
    to: '/field',
    label: 'Field',
    icon: <Icon.Atom className="nav-icon" />,
    kbd: 'g f',
    count: 38,
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

export function Sidebar() {
  const { location } = useRouterState();
  const pathname = location.pathname;

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
      {NAV.map((it) => {
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
              <span className="nav-count">{it.count}</span>
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
          <span className="v">619</span>
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
