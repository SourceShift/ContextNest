import { Outlet, createRootRouteWithContext } from '@tanstack/react-router';
import type { QueryClient } from '@tanstack/react-query';

import { Sidebar } from '@/components/Sidebar';
import { TopBar } from '@/components/TopBar';

export const Route = createRootRouteWithContext<{
  queryClient: QueryClient;
}>()({
  component: RootLayout,
});

function RootLayout() {
  return (
    <div className="app">
      <Sidebar />
      <main className="main">
        <TopBar />
        <div className="content">
          <Outlet />
        </div>
      </main>
    </div>
  );
}
