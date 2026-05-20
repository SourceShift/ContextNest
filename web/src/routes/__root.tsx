import { Outlet, createRootRouteWithContext } from '@tanstack/react-router';
import type { QueryClient } from '@tanstack/react-query';

import { Sidebar } from '@/components/Sidebar';
import { SubstrateBadge } from '@/components/SubstrateBadge';

export const Route = createRootRouteWithContext<{
  queryClient: QueryClient;
}>()({
  component: RootLayout,
});

function RootLayout() {
  return (
    <div className="flex h-screen min-h-0 w-full bg-[--color-surface] text-[--color-ink]">
      <Sidebar />
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-12 shrink-0 items-center justify-between border-b border-[--color-border] px-6">
          <div />
          <SubstrateBadge />
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-6">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
