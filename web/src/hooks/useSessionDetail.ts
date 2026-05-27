import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import type { RetrieveHit } from '@/lib/types';

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
    return normalizeHits(res.hits);
  } catch {
    return [];
  }
}

function normalizeHits(hits: RetrieveHit[]): RetrieveHit[] {
  const byKey = new Map<string, RetrieveHit>();
  for (const hit of hits) {
    const key = logicalHitKey(hit);
    const current = byKey.get(key);
    if (!current || compareByTimestampDesc(hit, current) < 0) {
      byKey.set(key, hit);
    }
  }
  return [...byKey.values()].sort(compareByTimestampDesc);
}

function logicalHitKey(hit: RetrieveHit): string {
  const kind = typeof hit.metadata.kind === 'string' ? hit.metadata.kind : '';
  const source = typeof hit.metadata.source === 'string' ? hit.metadata.source : '';
  const srcSession =
    typeof hit.metadata.src_session === 'string'
      ? hit.metadata.src_session
      : hit.session_id ?? '';
  const taskId = typeof hit.metadata.task_id === 'string' ? hit.metadata.task_id : '';
  const ts =
    source === 'TaskCompleted'
      ? ''
      : typeof hit.metadata.ts === 'string'
        ? hit.metadata.ts
        : '';
  const text = hit.content.trim().replace(/\s+/g, ' ');
  return [kind, source, srcSession, taskId, ts, text].join('\u001f');
}

function compareByTimestampDesc(a: RetrieveHit, b: RetrieveHit): number {
  const at = timestampMillis(a);
  const bt = timestampMillis(b);
  if (at !== bt) return bt - at;
  if (a.similarity !== b.similarity) return b.similarity - a.similarity;
  if (a.importance !== b.importance) return b.importance - a.importance;
  return a.id.localeCompare(b.id);
}

function timestampMillis(hit: RetrieveHit): number {
  const ts = typeof hit.metadata.ts === 'string' ? hit.metadata.ts : '';
  const parsed = Date.parse(ts);
  return Number.isNaN(parsed) ? Number.NEGATIVE_INFINITY : parsed;
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
      ] = await Promise.all([
        fetchKind(sessionId, 'goal_phase'),
        fetchKind(sessionId, 'accomplishment'),
        fetchKind(sessionId, 'learning'),
        fetchKind(sessionId, 'todo'),
        fetchKind(sessionId, 'decision'),
        fetchKind(sessionId, 'blocker'),
        fetchKind(sessionId, 'user_action'),
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
