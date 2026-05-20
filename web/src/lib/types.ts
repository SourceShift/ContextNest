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
};

export type SessionListResponse = {
  sessions: SessionListItem[];
};
