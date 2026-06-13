import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import type { PromptPreviewResponse, RetrieveHit, TrajectoryResponse } from '@/lib/types';

/**
 * Per-session detail view: fans out one `/api/v1/tools/retrieve` call
 * per memory kind so the session detail page can render each section
 * in parallel.
 *
 * ## Why fan-out instead of a single call
 *
 * The substrate's retrieve endpoint already filters by metadata, so a
 * single call with no filter would return everything for the session
 * but capped at `top_k=50`. For populated sessions that cap drops
 * blockers, todos, and user actions that the UI needs to render.
 * Fanning out one call per kind gives each section its own top_k=50
 * budget — the user sees up to 50 todos, 50 learnings, etc.
 *
 * ## Why a generic query string
 *
 * Retrieve needs a `query` for similarity ranking, but we don't have
 * a user-supplied search term on the detail page. We pass the kind
 * name itself as the query — it usually matches the kind metadata
 * which keeps the similarity scores reasonably stable, but the
 * `metadata_filter` is what guarantees correctness (kind=X items only).
 */
export type SessionDetail = {
  sessionId: string;
  goalPhases: RetrieveHit[];
  accomplishments: RetrieveHit[];
  learnings: RetrieveHit[];
  todos: RetrieveHit[];
  decisions: RetrieveHit[];
  blockers: RetrieveHit[];
  userActions: RetrieveHit[];
  trajectory: TrajectoryResponse | null;
  promptPreview: PromptPreviewResponse | null;
};

async function fetchKind(
  sessionId: string,
  kind: string,
  extraFilter?: Record<string, unknown>,
): Promise<RetrieveHit[]> {
  const filter: Record<string, unknown> = { kind, ...(extraFilter ?? {}) };
  try {
    const res = await api.retrieve({
      session_id: sessionId,
      query: kind,
      top_k: 50,
      metadata_filter: filter,
    });
    return res.hits;
  } catch {
    return [];
  }
}

/**
 * Fan out across multiple kind names and merge, deduplicating by id.
 *
 * The extractor emits two parallel taxonomies that share UI buckets:
 *
 * - Decisions: legacy `decision` + trajectory-style `decision_made`
 * - Blockers : legacy `blocker` + trajectory-style `failure` + `risk_flag`
 *
 * Older sessions only carry the legacy kind; newer sessions only carry the
 * trajectory kinds (see `src/ingest/claude_code/extractor.rs` —
 * `MemoryKind::Decision` vs `MemoryKind::DecisionMade`). Querying only one
 * name dropped every non-empty bucket for sessions written under the other
 * taxonomy — the UI then showed "no decisions / no blockers" while the DB
 * had hundreds. Merge here so both populations render. Sort by `ts` desc
 * after the merge so the section reads chronologically regardless of which
 * kind dominated.
 */
async function fetchKinds(
  sessionId: string,
  kinds: string[],
  extraFilter?: Record<string, unknown>,
): Promise<RetrieveHit[]> {
  const buckets = await Promise.all(
    kinds.map((k) => fetchKind(sessionId, k, extraFilter)),
  );
  const seen = new Set<string>();
  const merged: RetrieveHit[] = [];
  for (const bucket of buckets) {
    for (const hit of bucket) {
      if (seen.has(hit.id)) continue;
      seen.add(hit.id);
      merged.push(hit);
    }
  }
  merged.sort((a, b) => {
    const at = typeof a.metadata.ts === 'string' ? a.metadata.ts : '';
    const bt = typeof b.metadata.ts === 'string' ? b.metadata.ts : '';
    return bt.localeCompare(at);
  });
  return merged;
}

/**
 * Race a promise against a hard timeout. The original promise keeps
 * running on the network, but the caller resolves with `fallback`
 * after `ms` so it never blocks a Promise.all sibling.
 *
 * Needed because some BE endpoints (notably `/sessions/:id/trajectory`)
 * scale super-linearly with fragment count and can hang for >30s on
 * sessions with 1000+ fragments — long enough to keep the whole
 * session-detail Promise.all in flight and render every section as
 * `loading=true`. With this race, a slow trajectory just yields a
 * null trajectory section while every other section paints.
 */
function raceTimeout<T>(p: Promise<T>, ms: number, fallback: T): Promise<T> {
  return new Promise<T>((resolve) => {
    const t = setTimeout(() => resolve(fallback), ms);
    p.then(
      (v) => {
        clearTimeout(t);
        resolve(v);
      },
      () => {
        clearTimeout(t);
        resolve(fallback);
      },
    );
  });
}

async function fetchTrajectory(sessionId: string): Promise<TrajectoryResponse | null> {
  try {
    return await raceTimeout(api.sessionTrajectory(sessionId), 6_000, null);
  } catch {
    return null;
  }
}

async function fetchPromptPreview(sessionId: string): Promise<PromptPreviewResponse | null> {
  try {
    return await raceTimeout(api.sessionPromptPreview(sessionId), 6_000, null);
  } catch {
    return null;
  }
}

export function useSessionDetail(sessionId: string) {
  return useQuery({
    queryKey: ['sessionDetail', sessionId],
    enabled: !!sessionId,
    staleTime: 10_000,
    refetchInterval: 30_000,
    queryFn: async (): Promise<SessionDetail> => {
      const [
        goalPhases,
        accomplishments,
        learnings,
        todos,
        decisions,
        blockers,
        userActions,
        trajectory,
        promptPreview,
      ] = await Promise.all([
        fetchKind(sessionId, 'goal_phase'),
        fetchKind(sessionId, 'accomplishment'),
        fetchKind(sessionId, 'learning'),
        fetchKind(sessionId, 'todo'),
        fetchKinds(sessionId, ['decision', 'decision_made']),
        fetchKinds(sessionId, ['blocker', 'failure', 'risk_flag']),
        fetchKind(sessionId, 'user_action'),
        fetchTrajectory(sessionId),
        fetchPromptPreview(sessionId),
      ]);
      return {
        sessionId,
        goalPhases,
        accomplishments,
        learnings,
        todos,
        decisions,
        blockers,
        userActions,
        trajectory,
        promptPreview,
      };
    },
  });
}

/** Format an ISO timestamp into a short "MM-DD · HH:MM" or "HH:MM" string. */
export function shortStamp(ts: string | undefined): string {
  if (!ts) return '';
  // Today: HH:MM. Otherwise: MM-DD HH:MM.
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return ts.slice(0, 16);
  const now = new Date();
  const same =
    d.getFullYear() === now.getFullYear() &&
    d.getMonth() === now.getMonth() &&
    d.getDate() === now.getDate();
  const hh = d.getHours().toString().padStart(2, '0');
  const mm = d.getMinutes().toString().padStart(2, '0');
  if (same) return `${hh}:${mm}`;
  const mo = (d.getMonth() + 1).toString().padStart(2, '0');
  const dd = d.getDate().toString().padStart(2, '0');
  return `${mo}-${dd} ${hh}:${mm}`;
}
