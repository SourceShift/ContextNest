import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import type { FeaturesResponse } from '@/lib/types';

export type UseFeaturesOpts = {
  /** Duration suffix accepted by GET /api/v1/features?since=. Defaults
   * to whatever the backend defaults to (24h) when omitted. */
  since?: string;
  /** Optional layer filter (`frontend`, `backend`, `infra`, `docs`,
   * `tests`, `other`). Case-insensitive on the server. */
  layer?: string;
};

export type UseFeaturesResult = {
  data: FeaturesResponse | null;
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => void;
};

/**
 * Daily feature inventory. Lists every `MemoryKind::Feature` record
 * (one per agent-declared `delivered_features[]` entry) within a
 * time window, newest first.
 *
 * Refetch interval matches useInbox/useStats (30s) so the dashboard
 * picks up new features within a polling cycle without hammering
 * the backend.
 */
export function useFeatures(opts: UseFeaturesOpts = {}): UseFeaturesResult {
  const query = useQuery({
    queryKey: ['features', opts.since ?? '24h', opts.layer ?? '*'],
    queryFn: () =>
      api.features({
        since: opts.since,
        layer: opts.layer,
      }),
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
