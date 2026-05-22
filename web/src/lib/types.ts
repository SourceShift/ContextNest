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
  | 'summary';

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
