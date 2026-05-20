import type { HealthResponse, RetrieveResponse, StatusResponse } from './types';

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
    metadata_filter?: Record<string, unknown>;
  }) =>
    request<RetrieveResponse>('/api/v1/tools/retrieve', {
      method: 'POST',
      json: { top_k: 50, ...params },
    }),

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
};
