import { QueryClient } from "@tanstack/react-query";

/**
 * Shared QueryClient. Defaults tuned for the seven-tool memory API:
 *
 * - `staleTime: 30s` — the substrate's response shape is stable;
 *   re-fetching on every focus event is unnecessary noise.
 * - `gcTime: 5min` — keep cached responses around long enough that
 *   navigating between routes doesn't refetch.
 * - `retry: 2` — substrate is local in v0.2; two retries is enough to
 *   ride out a momentary blip without long stalls.
 */
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      gcTime: 5 * 60_000,
      retry: 2,
      refetchOnWindowFocus: false,
    },
    mutations: {
      retry: 0,
    },
  },
});
