import type {
  BasinsResponse,
  ConnectionsResponse,
  FeaturesResponse,
  FragmentsResponse,
  HealthResponse,
  InboxResponse,
  PromptPreviewResponse,
  RetrieveResponse,
  SessionListResponse,
  StatsResponse,
  StatusResponse,
  TrajectoryResponse,
} from './types';

const SUBSTRATE_URL = import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:28080';

const trimmed = SUBSTRATE_URL.replace(/\/+$/, '');

export const substrateUrl = trimmed;

async function request<T>(path: string, init?: RequestInit & { json?: unknown }): Promise<T> {
  const { json, ...rest } = init ?? {};
  const res = await fetch(`${trimmed}${path}`, {
    ...rest,
    headers: {
      'Content-Type': 'application/json',
      ...(rest.headers ?? {}),
    },
    body: json !== undefined ? JSON.stringify(json) : rest.body,
  });
  if (!res.ok) {
    const body = await res.text().catch(() => '');
    throw new ApiError(`${res.status} ${res.statusText}: ${body}`, res.status);
  }
  return (await res.json()) as T;
}

export class ApiError extends Error {
  status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

export const api = {
  health: () => request<HealthResponse>('/api/health', { method: 'GET' }),

  status: () => request<StatusResponse>('/api/status', { method: 'GET' }),

  retrieve: (params: {
    query: string;
    top_k?: number;
    session_id?: string;
    /** Cross-session mode: when set (and non-empty) the substrate
     * snapshots all active fragments under one lock and filters to the
     * listed sessions. Each `RetrieveHit` then carries its owning
     * `session_id`. `session_id` (singular) is ignored when this is set. */
    session_ids?: string[];
    metadata_filter?: Record<string, unknown>;
  }) =>
    request<RetrieveResponse>('/api/v1/tools/retrieve', {
      method: 'POST',
      json: { top_k: 50, ...params },
    }),

  sessions: () => request<SessionListResponse>('/api/v1/sessions', { method: 'GET' }),

  sessionTrajectory: (sessionId: string) =>
    request<TrajectoryResponse>(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/trajectory`,
      { method: 'GET' },
    ),

  sessionPromptPreview: (sessionId: string) =>
    request<PromptPreviewResponse>(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/prompt-preview`,
      { method: 'GET' },
    ),

  inbox: () => request<InboxResponse>('/api/v1/inbox', { method: 'GET' }),

  stats: () => request<StatsResponse>('/api/v1/stats', { method: 'GET' }),

  fragments: (params: {
    session_id?: string;
    project?: string;
    kind?: string;
    with_embedding?: boolean;
    limit?: number;
  } = {}) => {
    const q = new URLSearchParams();
    if (params.session_id) q.set('session_id', params.session_id);
    if (params.project) q.set('project', params.project);
    if (params.kind) q.set('kind', params.kind);
    if (params.with_embedding) q.set('with_embedding', 'true');
    if (params.limit) q.set('limit', String(params.limit));
    const qs = q.toString();
    return request<FragmentsResponse>(
      `/api/v1/fragments${qs ? `?${qs}` : ''}`,
      { method: 'GET' },
    );
  },

  basins: () =>
    request<BasinsResponse>('/api/v1/field/basins', { method: 'GET' }),

  connections: (params: { session_id?: string; limit?: number } = {}) => {
    const q = new URLSearchParams();
    if (params.session_id) q.set('session_id', params.session_id);
    if (params.limit) q.set('limit', String(params.limit));
    const qs = q.toString();
    return request<ConnectionsResponse>(
      `/api/v1/connections${qs ? `?${qs}` : ''}`,
      { method: 'GET' },
    );
  },

  store: (params: {
    content: string;
    importance?: number;
    session_id?: string;
    metadata?: Record<string, unknown>;
  }) =>
    request<{ attractor_id: string | null; stored: boolean }>('/api/v1/tools/store', {
      method: 'POST',
      json: params,
    }),

  features: (params: { since?: string; layer?: string } = {}) => {
    const q = new URLSearchParams();
    if (params.since) q.set('since', params.since);
    if (params.layer) q.set('layer', params.layer);
    const qs = q.toString();
    return request<FeaturesResponse>(
      `/api/v1/features${qs ? `?${qs}` : ''}`,
      { method: 'GET' },
    );
  },
};
