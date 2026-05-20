import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import type { SessionListItem } from '@/lib/types';

export type UseSessionsResult = {
  data: SessionListItem[];
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => void;
};

export function useSessions(): UseSessionsResult {
  const query = useQuery({
    queryKey: ['sessions'],
    queryFn: async () => {
      const res = await api.sessions();
      return res.sessions;
    },
    refetchInterval: 30_000,
  });

  return {
    data: query.data ?? [],
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error instanceof Error ? query.error : null,
    refetch: () => {
      void query.refetch();
    },
  };
}

function basename(p: string | null | undefined): string | null {
  if (!p) return null;
  const segs = p.replace(/\/+$/, '').split('/');
  const last = segs[segs.length - 1];
  return last || null;
}

/**
 * Project basenames (dedup'd, alphabetized) across every known session.
 * Drives the project filter dropdown in routes that need to show ALL
 * known projects — including sessions with zero inbox-eligible items
 * (which wouldn't otherwise surface through inbox-derived projects).
 */
export function useKnownProjects(): string[] {
  const { data } = useSessions();
  const names = new Set<string>();
  for (const s of data) {
    const b = basename(s.project_cwd);
    if (b) names.add(b);
  }
  return Array.from(names).sort();
}
