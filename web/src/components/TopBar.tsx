import { useRouterState } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';

import { Icon } from './atoms';
import { api, substrateUrl } from '@/lib/api';

const CRUMBS: Record<string, string[]> = {
  '/': ['dashboard', 'inbox'],
  '/sessions': ['dashboard', 'sessions'],
  '/search': ['dashboard', 'search'],
  '/phases': ['dashboard', 'phases'],
  '/field': ['dashboard', 'field'],
  '/tools': ['dashboard', 'tools'],
  '/substrate': ['dashboard', 'substrate'],
};

export function TopBar() {
  const { location } = useRouterState();
  const pathname = location.pathname;

  const health = useQuery({
    queryKey: ['health'],
    queryFn: () => api.health(),
    refetchInterval: 15_000,
  });

  const sessionDetailMatch = pathname.match(/^\/sessions\/(.+)$/);
  const crumbs = sessionDetailMatch
    ? ['dashboard', 'sessions', sessionDetailMatch[1]]
    : (CRUMBS[pathname] ?? ['dashboard']);

  const state = health.isError
    ? 'down'
    : health.isLoading
      ? 'partial'
      : health.data?.healthy
        ? 'ok'
        : 'partial';

  const healthText =
    state === 'down'
      ? 'substrate · unreachable'
      : state === 'partial'
        ? 'substrate · checking'
        : `substrate · ok · ${origin(substrateUrl)}`;

  return (
    <header className="topbar" role="banner">
      <div className="topbar-left">
        <div className="crumb">
          {crumbs.map((c, i) => (
            <span key={`${i}-${c}`}>
              {i > 0 && <span className="sep">/</span>}
              <span className={i === crumbs.length - 1 ? 'cur' : ''}>{c}</span>
            </span>
          ))}
        </div>
      </div>
      <div className="topbar-right">
        <button
          className="btn btn-ghost"
          onClick={() => health.refetch()}
          title="Refresh now"
          type="button"
        >
          <Icon.Refresh className="ic" /> <span className="kbd">r</span>
        </button>
        <div
          className={`health${state === 'down' ? ' down' : state === 'partial' ? ' partial' : ''}`}
          role="status"
          aria-live="polite"
        >
          <span className="dot" />
          <span>{healthText}</span>
        </div>
      </div>
    </header>
  );
}

function origin(url: string): string {
  try {
    return new URL(url).host.replace(/:\d+$/, (m) => m);
  } catch {
    return url;
  }
}
