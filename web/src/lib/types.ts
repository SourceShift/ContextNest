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
