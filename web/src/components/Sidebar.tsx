import { Link, useRouterState } from '@tanstack/react-router';
import {
  Inbox,
  History,
  Search,
  Workflow,
  Wrench,
  Settings2,
} from 'lucide-react';

import { Logo } from './Logo';
import { cn } from '@/lib/cn';

type NavItem = {
  to: '/' | '/sessions' | '/search' | '/phases' | '/tools' | '/substrate';
  icon: typeof Inbox;
  label: string;
  shortcut: string;
};

const NAV: NavItem[] = [
  { to: '/', icon: Inbox, label: 'Inbox', shortcut: 'g i' },
  { to: '/sessions', icon: History, label: 'Sessions', shortcut: 'g s' },
  { to: '/search', icon: Search, label: 'Search', shortcut: 'g /' },
  { to: '/phases', icon: Workflow, label: 'Phases', shortcut: 'g p' },
  { to: '/tools', icon: Wrench, label: 'Tools', shortcut: 'g t' },
  { to: '/substrate', icon: Settings2, label: 'Substrate', shortcut: 'g o' },
];

export function Sidebar() {
  const { location } = useRouterState();
  const pathname = location.pathname;

  return (
    <aside className="flex w-56 shrink-0 flex-col border-r border-[--color-border] bg-[--color-surface]">
      <div className="flex items-center gap-2 px-4 py-5">
        <Logo className="text-[--color-accent]" />
        <span className="text-base font-semibold tracking-tight">
          ContextNest
        </span>
      </div>

      <nav className="flex flex-col gap-1 px-2">
        {NAV.map((item) => {
          const active =
            item.to === '/'
              ? pathname === '/'
              : pathname.startsWith(item.to);
          const Icon = item.icon;
          return (
            <Link
              key={item.to}
              to={item.to}
              className={cn(
                'group flex items-center gap-3 rounded-lg px-3 py-2 text-sm transition-colors',
                active
                  ? 'bg-[--color-surface-2] text-[--color-ink]'
                  : 'text-[--color-ink-muted] hover:bg-[--color-surface-1] hover:text-[--color-ink]',
              )}
            >
              <Icon className="size-4 shrink-0" />
              <span className="flex-1">{item.label}</span>
              <span
                className={cn(
                  'mono text-[10px] tracking-wider transition-opacity',
                  active
                    ? 'text-[--color-ink-dim]'
                    : 'text-[--color-ink-faint] opacity-0 group-hover:opacity-100',
                )}
              >
                {item.shortcut}
              </span>
            </Link>
          );
        })}
      </nav>

      <div className="mt-auto px-4 py-4 text-[10px] text-[--color-ink-dim]">
        <div className="mono">v0.1.0</div>
        <div className="mono opacity-60">{substrateOrigin()}</div>
      </div>
    </aside>
  );
}

function substrateOrigin(): string {
  const url = import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:28080';
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
