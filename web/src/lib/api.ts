/**
 * Typed client for the seven-tool memory API.
 *
 * The substrate exposes:
 *   POST /api/v1/tools/store
 *   POST /api/v1/tools/retrieve
 *   POST /api/v1/tools/update
 *   POST /api/v1/tools/summarize
 *   POST /api/v1/tools/discard
 *   POST /api/v1/tools/reconstruct
 *   POST /api/v1/tools/resonate
 *   GET  /health
 *
 * Request and response shapes mirror `src/api/tools.rs` on the Rust side.
 * Schema is duplicated by hand for v0.2-ui because we don't ship the
 * TypeScript SDK yet — that lands as `@contextnest/sdk` in a follow-up.
 */

const API_BASE = ""; // Vite proxies /api/* in dev; same-origin in prod.

// ---------------------------------------------------------------------------
// Request/response shapes (matches src/api/tools.rs)
// ---------------------------------------------------------------------------

export interface StoreRequest {
  content: string;
  importance?: number;
  session_id?: string;
}

export interface StoreResponse {
  fragment_id: string;
  attractor_basin?: string;
  edges_added?: number;
  stored_at?: string;
}

export interface RetrieveRequest {
  query: string;
  top_k?: number;
  session_id?: string;
}

export interface RetrievedAttractor {
  fragment_id: string;
  content: string;
  importance: number;
  basin_id?: string;
  activation_score?: number;
}

export interface RetrieveResponse {
  attractors: RetrievedAttractor[];
}

export interface ReconstructRequest {
  partial_cue: string;
  session_id?: string;
}

export interface ReconstructResponse {
  reconstructed: string;
  confidence: number;
}

export interface ResonateRequest {
  query: string;
  session_id?: string;
}

export interface ResonancePattern {
  basins: string[];
  coherence: number;
  summary?: string;
}

export interface ResonateResponse {
  patterns: ResonancePattern[];
}

export interface SummarizeRequest {
  session_id?: string;
  region?: "all" | string;
  target_size?: number;
}

export interface SummarizeResponse {
  summary: string;
  fragment_id?: string;
}

export interface DiscardRequest {
  fragment_id: string;
  session_id?: string;
  hard?: boolean;
}

export interface DiscardResponse {
  discarded: string;
  hard: boolean;
}

export interface UpdateRequest {
  fragment_id: string;
  content?: string;
  importance?: number;
  session_id?: string;
}

export interface UpdateResponse {
  fragment_id: string;
  updated_at?: string;
}

export interface HealthResponse {
  status: "healthy" | "degraded" | "unhealthy" | string;
  ready?: boolean;
  [k: string]: unknown;
}

// ---------------------------------------------------------------------------
// Client
// ---------------------------------------------------------------------------

class ApiError extends Error {
  status: number;
  body: string;

  constructor(status: number, body: string, message: string) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.body = body;
  }
}

async function call<TReq, TRes>(path: string, body: TReq): Promise<TRes> {
  const res = await fetch(`${API_BASE}${path}`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text().catch(() => "");
    throw new ApiError(
      res.status,
      text,
      `${path} returned ${res.status}: ${text || res.statusText}`,
    );
  }
  return (await res.json()) as TRes;
}

export const api = {
  store: (body: StoreRequest) =>
    call<StoreRequest, StoreResponse>("/api/v1/tools/store", body),

  retrieve: (body: RetrieveRequest) =>
    call<RetrieveRequest, RetrieveResponse>("/api/v1/tools/retrieve", body),

  update: (body: UpdateRequest) =>
    call<UpdateRequest, UpdateResponse>("/api/v1/tools/update", body),

  summarize: (body: SummarizeRequest) =>
    call<SummarizeRequest, SummarizeResponse>(
      "/api/v1/tools/summarize",
      body,
    ),

  discard: (body: DiscardRequest) =>
    call<DiscardRequest, DiscardResponse>("/api/v1/tools/discard", body),

  reconstruct: (body: ReconstructRequest) =>
    call<ReconstructRequest, ReconstructResponse>(
      "/api/v1/tools/reconstruct",
      body,
    ),

  resonate: (body: ResonateRequest) =>
    call<ResonateRequest, ResonateResponse>("/api/v1/tools/resonate", body),

  health: async (): Promise<HealthResponse> => {
    const res = await fetch(`${API_BASE}/api/health`);
    if (!res.ok) {
      throw new ApiError(res.status, "", `health returned ${res.status}`);
    }
    return res.json() as Promise<HealthResponse>;
  },
};

export { ApiError };
