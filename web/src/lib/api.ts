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
  SessionsByFeatureResponse,
  SessionsByFileResponse,
  SessionSummaryResponse,
  StatsResponse,
  StatusResponse,
  SubstrateConfigResponse,
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

  /**
   * Read-only snapshot of the substrate's runtime config: embedder
   * model, LLM provider, cache encryption/threshold/TTL, build info.
   * Used by /config (Ticket #6) to surface "what's actually live?"
   * without grepping config.toml.
   */
  substrateConfig: () =>
    request<SubstrateConfigResponse>('/api/v1/substrate/config', {
      method: 'GET',
    }),

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
    /** Drop fragments whose `metadata.kind` is in this list. Pair with
     * `metadata_filter` (inclusion) — both must pass for a fragment to
     * land in results. Use for noise classes like `initial_prompt_window`
     * that every session has and that dominate topic queries via
     * shared substrings under the TF-IDF embedder. */
    exclude_kinds?: string[];
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

  basins: (params: { project?: string; session_id?: string } = {}) => {
    const q = new URLSearchParams();
    if (params.project) q.set('project', params.project);
    if (params.session_id) q.set('session_id', params.session_id);
    const qs = q.toString();
    return request<BasinsResponse>(
      `/api/v1/field/basins${qs ? `?${qs}` : ''}`,
      { method: 'GET' },
    );
  },

  connections: (
    params: { session_id?: string; project?: string; limit?: number } = {},
  ) => {
    const q = new URLSearchParams();
    if (params.session_id) q.set('session_id', params.session_id);
    if (params.project) q.set('project', params.project);
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

  /**
   * Mutate an existing fragment's importance and/or content. The BE
   * checks ownership via `session_index` — passing the wrong session_id
   * returns 404 not 403. Re-embedding on content change is NOT wired
   * through this path; for semantic re-anchoring, prefer `discard` +
   * `store`. See `update` BE doc in src/api/tools.rs.
   */
  updateFragment: (params: {
    attractor_id: string;
    session_id: string;
    importance?: number;
    content?: string;
    metadata?: Record<string, unknown>;
  }) =>
    request<{ updated: boolean }>('/api/v1/tools/update', {
      method: 'POST',
      json: params,
    }),

  /**
   * Remove a fragment. `soft_delete: true` (default) keeps the row in
   * the WAL marked deleted — recoverable via direct WAL inspection.
   * `soft_delete: false` is destructive. `reason` is a free-form audit
   * string (e.g. "noise / boilerplate", "outdated").
   */
  discardFragment: (params: {
    attractor_id: string;
    session_id: string;
    soft_delete?: boolean;
    reason?: string;
  }) =>
    request<{ discarded: boolean; soft_delete: boolean }>('/api/v1/tools/discard', {
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

  /**
   * Substring search across every session's declared
   * `delivered_features[].feature` name. Use this when you remember
   * what you SHIPPED but not WHICH SESSION shipped it.
   * Backed by `GET /api/v1/sessions/by-feature?q=<substring>`.
   */
  sessionsByFeature: (q: string) =>
    request<SessionsByFeatureResponse>(
      `/api/v1/sessions/by-feature?q=${encodeURIComponent(q)}`,
      { method: 'GET' },
    ),

  /**
   * Substring search across every session's `files_touched` list.
   * Use this when you remember the file you edited but not WHICH
   * SESSION edited it. Substring is case-insensitive — pass a
   * basename like `AgentStreamRail.tsx` or a partial path. Backed by
   * `GET /api/v1/sessions/by-file?path=<substr>`. Note the BE query
   * param is `path=` (legacy naming) even though the matching is
   * substring not exact.
   */
  sessionsByFile: (q: string) =>
    request<SessionsByFileResponse>(
      `/api/v1/sessions/by-file?path=${encodeURIComponent(q)}`,
      { method: 'GET' },
    ),

  /**
   * One-paragraph LLM-generated summary of a session. Cheap to call —
   * the substrate caches summaries server-side and regenerates only
   * when stale. Used by the search-result inline preview (ticket #2)
   * and the session detail page header.
   */
  sessionSummary: (sessionId: string) =>
    request<SessionSummaryResponse>(
      `/api/v1/sessions/${encodeURIComponent(sessionId)}/summary`,
      { method: 'GET' },
    ),
};
