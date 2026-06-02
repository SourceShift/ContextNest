import { useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import { pca2d } from '@/lib/pca';
import { useSessions } from './useSessions';
import { useStats } from './useStats';
import type { BasinSummary, ConnectionRow, FragmentRow, SessionListItem } from '@/lib/types';

/**
 * Field-view data. With T1+T2+T3+T4 in place this hook returns:
 *
 * - **fragments** with their 256-d embeddings (re-embedded on demand
 *   for sidecar-only entries via the backend's `with_embedding=true`
 *   query).
 * - **basins** with real centroids when the canonical attractor store
 *   has them, or project-derived centroids as a documented fallback.
 * - **connections** — actual retrieve co-occurrence pairs the substrate
 *   has accumulated since the last server restart.
 * - **layout** — top-2 PCA components projecting embeddings into 2D,
 *   so visual proximity = semantic similarity.
 */
export type FieldFragment = FragmentRow & {
  /** Days since metadata.ts; null when the fragment has no ts. */
  ageDays: number | null;
  /** Basin (project basename) this fragment belongs to. */
  project: string;
};

export type FieldData = {
  fragments: FieldFragment[];
  basins: BasinSummary[];
  connections: ConnectionRow[];
  kinds: string[];
  /** PCA result: { x, y } per fragment (same order as `fragments`) and
   *  the % variance captured by the top-2 components. */
  layout: {
    xy: Array<{ x: number; y: number }>;
    varianceRatio: number;
  };
};

function basename(p: string | null | undefined): string {
  if (!p) return 'unknown';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || 'unknown';
}

function ageDaysFrom(ts: string | null | undefined): number | null {
  if (!ts) return null;
  const t = Date.parse(ts);
  if (Number.isNaN(t)) return null;
  return Math.max(0, Math.floor((Date.now() - t) / (24 * 3600 * 1000)));
}

// Cap how many fragments we pull for the field.
//
// PCA cost is ~O(N · D · iters) which at N=500, D=256, 60 iters is
// ~7.7M ops (~50ms on M-series) — that's not the bottleneck.
//
// The REAL bottleneck is fragment-side: each fragment whose embedding
// isn't cached gets a network round-trip to the embedding provider
// (DeepInfra/OpenAI). At 200ms/call and 16-way parallel that's still
// 200 × (250/16) = 3s on a cold load. We cap at 250 to keep that
// ceiling reasonable. After the first load, the server's per-fragment
// embedding cache makes subsequent refreshes near-instant.
const FIELD_FRAGMENT_LIMIT = 250;

export type UseFieldOptions = {
  /** Gate expensive field calls until the user has chosen a scope. */
  enabled?: boolean;
  /** Optional session filter — narrows to one session's fragments. */
  sessionId?: string;
  /** Optional project filter — narrows to one folder/project's fragments.
   *  Matched against the basename of `metadata.project_cwd` server-side. */
  project?: string;
};

export function useFieldData(opts: UseFieldOptions = {}) {
  const sessions = useSessions();
  const stats = useStats();

  const fragmentsQuery = useQuery({
    queryKey: ['field/fragments', opts.sessionId ?? null, opts.project ?? null],
    enabled: opts.enabled ?? true,
    refetchInterval: 30_000,
    staleTime: 10_000,
    queryFn: () =>
      api.fragments({
        session_id: opts.sessionId,
        project: opts.project,
        with_embedding: true,
        limit: FIELD_FRAGMENT_LIMIT,
      }),
  });

  const basinsQuery = useQuery({
    queryKey: ['field/basins', opts.project ?? null, opts.sessionId ?? null],
    enabled: opts.enabled ?? true,
    refetchInterval: 30_000,
    queryFn: () =>
      api.basins({
        project: opts.project,
        session_id: opts.sessionId,
      }),
  });

  const connectionsQuery = useQuery({
    queryKey: ['field/connections', opts.project ?? null, opts.sessionId ?? null],
    enabled: opts.enabled ?? true,
    refetchInterval: 15_000,
    queryFn: () =>
      api.connections({
        project: opts.project,
        session_id: opts.sessionId,
        limit: 200,
      }),
  });

  // Memoise enriched fragments. We add ageDays and project basename so
  // downstream code doesn't keep re-parsing the metadata blob.
  const fragments: FieldFragment[] = useMemo(() => {
    const rows = fragmentsQuery.data?.fragments ?? [];
    return rows.map((f) => {
      const ts = typeof f.metadata.ts === 'string' ? (f.metadata.ts as string) : null;
      const project = basename(
        (f.metadata.project_cwd as string | undefined) ??
          // Fallback: derive from session listing.
          sessions.data.find((s: SessionListItem) => s.id === f.session_id)?.project_cwd ??
          undefined,
      );
      return {
        ...f,
        ageDays: ageDaysFrom(ts),
        project,
      };
    });
  }, [fragmentsQuery.data, sessions.data]);

  // PCA — only run when we actually have embeddings on the fragments.
  // Some rows can be missing them if the backend's embedding service
  // fell over for that specific fragment; we use a zero vector as the
  // placeholder so the projection still works (those fragments collapse
  // to the origin, which is honest).
  const layout = useMemo(() => {
    if (fragments.length < 2) {
      return {
        xy: fragments.map(() => ({ x: 0, y: 0 })),
        varianceRatio: 0,
      };
    }
    const dim =
      fragments.find((f) => f.embedding && f.embedding.length > 0)?.embedding?.length ?? 256;
    const matrix = fragments.map((f) =>
      f.embedding && f.embedding.length === dim ? f.embedding : new Array<number>(dim).fill(0),
    );
    const result = pca2d(matrix);
    return { xy: result.coords, varianceRatio: result.varianceRatio };
  }, [fragments]);

  const data: FieldData = useMemo(
    () => ({
      fragments,
      basins: basinsQuery.data?.basins ?? [],
      connections: connectionsQuery.data?.connections ?? [],
      kinds: Array.from(
        new Set(
          fragments.map((f) => {
            const k = f.metadata.kind;
            return typeof k === 'string' ? k : 'unknown';
          }),
        ),
      ).sort(),
      layout,
    }),
    [fragments, basinsQuery.data, connectionsQuery.data, layout],
  );

  return {
    data,
    isLoading: sessions.isLoading || fragmentsQuery.isLoading || basinsQuery.isLoading,
    isError: sessions.isError || fragmentsQuery.isError || basinsQuery.isError,
    totalFragments: stats.data?.total_fragments ?? null,
    totalSessions: stats.data?.total_sessions ?? null,
    truncated: fragmentsQuery.data?.truncated ?? false,
    refetch: () => {
      sessions.refetch();
      if (opts.enabled === false) return;
      void fragmentsQuery.refetch();
      void basinsQuery.refetch();
      void connectionsQuery.refetch();
    },
  };
}
