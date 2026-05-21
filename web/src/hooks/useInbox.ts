import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import type { InboxItemMock } from '@/lib/mock-data';
import type { RetrieveHit } from '@/lib/types';
import { agoFrom, isWithinMinutes } from '@/lib/relative-time';

type ValidKind = 'user_action' | 'decision';
const VALID_KINDS: ReadonlySet<string> = new Set<ValidKind>(['user_action', 'decision']);
const VALID_URGENCIES = new Set(['now', 'soon', 'later']);

function basename(p: string | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

function mapHit(hit: RetrieveHit, sessionId: string): InboxItemMock | null {
  const kind = hit.metadata.kind;
  if (!kind || !VALID_KINDS.has(kind)) return null;

  const rawUrgency = hit.metadata.urgency;
  const isDecision = kind === 'decision';
  const awaiting = hit.metadata.awaiting_decision ?? false;

  let urgency: 'now' | 'soon' | 'later';
  if (rawUrgency && VALID_URGENCIES.has(rawUrgency)) {
    urgency = rawUrgency as 'now' | 'soon' | 'later';
  } else if (isDecision && awaiting) {
    urgency = 'now';
  } else {
    urgency = 'later';
  }

  const stored = hit.metadata.ts ?? '';

  return {
    id: hit.id,
    sessionId,
    project: basename(hit.metadata.project_cwd),
    kind: kind as ValidKind,
    awaiting: awaiting || undefined,
    urgency,
    action: hit.content,
    reason: typeof hit.metadata.reason === 'string' ? hit.metadata.reason : '',
    decision: isDecision && hit.metadata.decision_text != null
      ? String(hit.metadata.decision_text)
      : undefined,
    step: typeof hit.metadata.step === 'number' ? hit.metadata.step : 0,
    stored,
    ago: agoFrom(stored),
    isNew: isWithinMinutes(stored, 5) || undefined,
  };
}

async function fetchInbox(): Promise<InboxItemMock[]> {
  const { sessions } = await api.sessions();

  if (sessions.length === 0) return [];

  const perSessionResults = await Promise.all(
    sessions.map(async (s) => {
      try {
        const [userActionsRes, decisionsRes] = await Promise.all([
          api.retrieve({
            session_id: s.id,
            query: 'user_action',
            top_k: 50,
            metadata_filter: { kind: 'user_action' },
          }),
          api.retrieve({
            session_id: s.id,
            query: 'awaiting decision',
            top_k: 50,
            metadata_filter: { kind: 'decision', awaiting_decision: true },
          }),
        ]);
        return { ok: true as const, hits: [...userActionsRes.hits, ...decisionsRes.hits], sessionId: s.id };
      } catch (err) {
        console.warn(`[useInbox] session ${s.id} retrieve failed:`, err);
        return { ok: false as const, err, sessionId: s.id };
      }
    }),
  );

  const failures = perSessionResults.filter((r) => !r.ok);
  if (failures.length === perSessionResults.length) {
    const firstFail = failures[0];
    throw firstFail.err instanceof Error ? firstFail.err : new Error(String(firstFail.err));
  }

  const items: InboxItemMock[] = [];
  for (const result of perSessionResults) {
    if (!result.ok) continue;
    for (const hit of result.hits) {
      const mapped = mapHit(hit, result.sessionId);
      if (mapped) items.push(mapped);
    }
  }

  return items;
}

export type UseInboxResult = {
  data: InboxItemMock[];
  isLoading: boolean;
  isError: boolean;
  error: Error | null;
  refetch: () => void;
};

export function useInbox(): UseInboxResult {
  const query = useQuery({
    queryKey: ['inbox'],
    queryFn: fetchInbox,
    refetchInterval: 30_000,
  });

  return {
    data: query.data ?? [],
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error instanceof Error ? query.error : null,
    refetch: () => { void query.refetch(); },
  };
}
