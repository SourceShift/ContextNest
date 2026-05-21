import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import type { StatsResponse } from '@/lib/types';

export type UseStatsResult = {
  data: StatsResponse | null;
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => void;
};

/**
 * Substrate-wide aggregate counts. Used by the Substrate page's
 * kind-distribution panel.
 *
 * The endpoint is cheap (one read-lock scan), so a 30s refetch
 * interval is conservative — the dashboard refresh cadence matches
 * useInbox/useSessions for consistency.
 */
export function useStats(): UseStatsResult {
  const query = useQuery({
    queryKey: ['stats'],
    queryFn: () => api.stats(),
    refetchInterval: 30_000,
  });

  return {
    data: query.data ?? null,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error instanceof Error ? query.error : null,
    refetch: () => {
      void query.refetch();
    },
  };
}
