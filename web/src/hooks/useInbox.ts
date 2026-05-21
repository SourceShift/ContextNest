import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import type { InboxItemMock } from '@/lib/mock-data';
import type { InboxHit } from '@/lib/types';
import { agoFrom, isWithinMinutes } from '@/lib/relative-time';

type ValidKind = 'user_action' | 'decision' | 'todo';
const VALID_KINDS: ReadonlySet<string> = new Set<ValidKind>([
  'user_action',
  'decision',
  'todo',
]);
const VALID_URGENCIES = new Set(['now', 'soon', 'later']);

function basename(p: string | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

function mapHit(hit: InboxHit): InboxItemMock | null {
  const kind = hit.metadata.kind;
  if (!kind || !VALID_KINDS.has(kind)) return null;

  const rawUrgency = hit.metadata.urgency;
  const isDecision = kind === 'decision';
  const isTodo = kind === 'todo';
  const awaiting = hit.metadata.awaiting_decision ?? false;

  let urgency: 'now' | 'soon' | 'later';
  if (rawUrgency && VALID_URGENCIES.has(rawUrgency)) {
    urgency = rawUrgency as 'now' | 'soon' | 'later';
  } else if (isDecision && awaiting) {
    urgency = 'now';
  } else if (isTodo) {
    // Open todos default to `soon` — they're actionable but not as
    // blocking as a decision that's literally awaiting the user.
    urgency = 'soon';
  } else {
    urgency = 'later';
  }

  const stored = hit.metadata.ts ?? '';

  return {
    id: hit.id,
    sessionId: hit.session_id,
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

// Single round-trip — replaces the previous `1 sessions + 2N retrieves` fan-out.
// All inbox filtering (kind, awaiting_decision, soft-delete visibility) is done
// server-side in `GET /api/v1/inbox` so the dashboard pays one HTTP call per
// poll regardless of how many sessions the substrate is tracking.
async function fetchInbox(): Promise<InboxItemMock[]> {
  const { items } = await api.inbox();

  const result: InboxItemMock[] = [];
  for (const hit of items) {
    const mapped = mapHit(hit);
    if (mapped) result.push(mapped);
  }
  return result;
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
