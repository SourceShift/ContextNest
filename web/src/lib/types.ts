export type Urgency = 'now' | 'soon' | 'later';

export type MemoryKind =
  | 'session_title'
  | 'goal_phase'
  | 'initial_prompt_window'
  | 'accomplishment'
  | 'learning'
  | 'todo'
  | 'user_action'
  | 'decision'
  | 'blocker'
  | 'state'
  | 'current_task'
  | 'summary'
  | 'read_context'
  | 'verification'
  | 'evidence_ref'
  | 'decision_made'
  | 'failure'
  | 'prompt_directive'
  | 'assumption'
  | 'artifact'
  | 'memory_candidate'
  | 'risk_flag';

export type FragmentMetadata = {
  kind?: MemoryKind | string;
  urgency?: Urgency;
  awaiting_decision?: boolean;
  src_session?: string;
  project_cwd?: string;
  ts?: string;
  step?: number;
  reason?: string;
  task_status?: string;
  [key: string]: unknown;
};

export type RetrieveHit = {
  id: string;
  content: string;
  importance: number;
  similarity: number;
  metadata: FragmentMetadata;
  /** Populated by the substrate only when the caller asked for
   * cross-session retrieval (via `session_ids`). Single-session callers
   * receive hits without this field. */
  session_id?: string;
};

export type RetrieveResponse = {
  hits: RetrieveHit[];
};

export type HealthResponse = {
  status: string;
  healthy: boolean;
};

export type StatusResponse = {
  version: string;
  name: string;
  description: string;
};

/** Body of `GET /api/v1/substrate/config` — read-only runtime config
 * snapshot. Used by /config dashboard page (Ticket #6) to answer
 * "what's actually configured on the running binary?" without
 * shelling into the host or grepping config.toml. */
export type SubstrateConfigResponse = {
  /** Compile-time CARGO_PKG_VERSION. */
  version: string;
  /** Build-time git commit short SHA (via CONTEXTNEST_GIT_COMMIT env)
   * or "unknown" when not provided to the compiler. */
  git_commit: string;
  embedding: {
    /** Configured embedding model identifier. "local" for the TF-IDF
     * fallback; otherwise the HuggingFace path like
     * "Qwen/Qwen3-Embedding-0.6B" or OpenAI's "text-embedding-3-small". */
    model: string;
  };
  llm: {
    /** false when no API key is set; summarize falls back to stats-only. */
    enabled: boolean;
    /** "anthropic" / "openai" / "google" / "custom" / null. */
    default_provider: string | null;
    /** Union of default + multi-provider routing kinds. */
    configured_providers: string[];
  };
  llm_cache: {
    encryption_enabled: boolean;
    similarity_threshold: number;
    default_ttl_secs: number;
    total_entries: number;
  };
};

/** Body of `GET /llm/v1/cache/stats` — LLM proxy cache counters.
 * Used by /llm-cache dashboard page (Ticket #5) to answer
 * "is the cache actually saving us money?". Field names mirror
 * `CacheStats` in `src/services/llm_cache.rs` exactly so Prometheus
 * JSON scrapers don't need a translator. */
export type LlmCacheStats = {
  total_entries: number;
  total_hits: number;
  total_misses: number;
  /** hits / (hits + misses), or 0.0 when no lookups have happened. */
  hit_rate: number;
};

export type SessionListItem = {
  id: string;
  fragment_count: number;
  project_cwd: string;
  src_session_uuid: string;
  last_ts: string | null;
  by_kind: Record<string, number>;
};

export type SessionListResponse = {
  sessions: SessionListItem[];
};

export type StatsResponse = {
  total_fragments: number;
  total_sessions: number;
  by_kind: Record<string, number>;
};

export type FragmentRow = {
  id: string;
  session_id: string;
  content: string;
  metadata: FragmentMetadata;
  importance: number;
  embedding?: number[];
};

export type FragmentsResponse = {
  fragments: FragmentRow[];
  truncated: boolean;
};

export type BasinSource = 'attractor' | 'project';

export type BasinSummary = {
  id: string;
  label: string;
  source: BasinSource;
  mass: number;
  centroid: number[];
  by_kind: Record<string, number>;
  sessions: string[];
};

export type BasinsResponse = {
  basins: BasinSummary[];
};

export type ConnectionRow = {
  source: string;
  target: string;
  count: number;
};

export type ConnectionsResponse = {
  connections: ConnectionRow[];
  total_known: number;
};

export type InboxHit = {
  id: string;
  session_id: string;
  content: string;
  importance: number;
  metadata: FragmentMetadata;
};

export type InboxResponse = {
  items: InboxHit[];
};

export type FeatureEntry = {
  session_id: string;
  feature: string;
  ts: string | null;
  files: string[];
  refs: unknown[];
  layer: string | null;
  how_to_test: string | null;
  defs: string[];
};

export type FeaturesResponse = {
  since: string;
  layer: string | null;
  count: number;
  features: FeatureEntry[];
};

/** One match from `GET /api/v1/sessions/by-feature?q=<substring>`. */
export type FeatureHit = {
  session_id: string;
  feature: string;
  ts: string | null;
  files: string[];
  refs: unknown[];
  layer: string | null;
};

export type SessionsByFeatureResponse = {
  query: string;
  hits: FeatureHit[];
};

/** One match from `GET /api/v1/sessions/by-file?path=<substring>`. */
export type FileMatch = {
  session_id: string;
  /** Subset of the session's files_touched list that contained the query
   * substring. Useful when the query is a partial basename and several
   * files in the session could plausibly match. */
  matched_files: string[];
  /** Size of the session's full files_touched list. Disambiguates
   * "session that surgically created this file" (total ≈ 1) from
   * "session that touched it as part of a broader run" (total in dozens). */
  total_files: number;
};

export type SessionsByFileResponse = {
  query: string;
  matches: FileMatch[];
};

/** Body of `GET /api/v1/sessions/:id/summary`. Markdown string body
 * cached by the substrate; regenerated on demand when stale. */
export type SessionSummaryResponse = {
  session_id: string;
  /** Single paragraph (LLM-generated when LLM provider is configured,
   * statistics-only fallback otherwise). May be empty when the
   * session has no fragments yet. */
  summary: string;
  /** Timestamp the summary was generated. Lets clients know if it's
   * stale relative to recent session activity. */
  generated_at: string | null;
};

export type TrajectoryRecord = {
  id: string;
  kind: string;
  content: string;
  ts: string | null;
  phase_idx: number | null;
  metadata: FragmentMetadata;
};

export type TrajectoryPhase = {
  idx: number;
  goal: string;
  start_ts: string | null;
  end_ts: string | null;
  counts: Record<string, number>;
  decisions: TrajectoryRecord[];
  failures: TrajectoryRecord[];
  verifications: TrajectoryRecord[];
  risks: TrajectoryRecord[];
  prompt_directives: TrajectoryRecord[];
  assumptions: TrajectoryRecord[];
};

export type TrajectoryCostProfile = {
  trajectory_records: number;
  turns_estimate: number;
  records_per_turn: number;
  prompt_directives: number;
  memory_candidates: number;
  risk_flags: number;
};

export type BasinLink = {
  basin_id: string;
  members_in_session: number;
  total_members: number;
  heat_24h: number;
  hottest_kind: string | null;
};

export type ResonantBasin = {
  basin_id: string;
  edge_count: number;
  coherence: number;
  sessions_touching: number;
};

export type PromotionCluster = {
  basin_id: string;
  candidates: TrajectoryRecord[];
  coherence: number;
};

export type TrajectoryResponse = {
  session_id: string;
  trajectory_count: number;
  phases: TrajectoryPhase[];
  records: TrajectoryRecord[];
  promotion_queue: TrajectoryRecord[];
  cost_profile: TrajectoryCostProfile;
  basin_links: BasinLink[];
  resonant_basins: ResonantBasin[];
  promotion_clusters: PromotionCluster[];
};

export type PromptPreviewSection = {
  key: string;
  title: string;
  kind: string;
  items: TrajectoryRecord[];
};

export type PromptPreviewResponse = {
  session_id: string;
  sections: PromptPreviewSection[];
  item_count: number;
};
