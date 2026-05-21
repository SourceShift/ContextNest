import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';

import { Icon } from '@/components/atoms';
import { api, substrateUrl } from '@/lib/api';
import { useStats } from '@/hooks/useStats';

export const Route = createFileRoute('/substrate')({
  component: SubstratePage,
});

// Stable color palette for the kind-distribution bar. Any kind not in
// this table falls through to a neutral muted color — better than
// jumping around when new kinds appear in the substrate.
const KIND_COLOR: Record<string, string> = {
  learning: '#a78bfa',
  accomplishment: '#00d4aa',
  fact: '#a1a1aa',
  decision: '#ffd166',
  user_action: '#ff6b6b',
  todo: '#60a5fa',
  goal_phase: '#f472b6',
  blocker: '#ff6b6b',
  session_title: '#a1a1aa',
  ack: '#71717a',
  initial_prompt_window: '#94a3b8',
  state: '#cbd5e1',
  current_task: '#fbbf24',
  summary: '#c4b5fd',
  unknown: '#475569',
};

function SubstratePage() {
  const health = useQuery({ queryKey: ['health'], queryFn: () => api.health() });
  const status = useQuery({ queryKey: ['status'], queryFn: () => api.status() });
  const stats = useStats();

  const down = health.isError || (!health.isLoading && !health.data?.healthy);

  // Derived view: kinds sorted by count desc so the longest bars sit at
  // the top of the panel. Missing stats → empty array so the panel renders
  // an empty state rather than NaN-width bars while loading.
  const byKindEntries = stats.data
    ? Object.entries(stats.data.by_kind).sort((a, b) => b[1] - a[1])
    : [];
  const totalKinds = stats.data?.total_fragments ?? 0;
  const maxKind = byKindEntries.length > 0 ? byKindEntries[0][1] : 1;

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Substrate</h1>
          <div className="page-sub">
            Health · fragments · hooks · embedding provider · activity log
          </div>
        </div>
        <div className="page-actions">
          <button className="btn" onClick={() => health.refetch()} type="button">
            <Icon.Refresh /> Refresh now
          </button>
          <button className="btn btn-ghost" type="button">
            <Icon.Info /> docs/usage.md
          </button>
        </div>
      </div>

      <div className="grid-2">
        <div className={`card health-card${down ? ' down' : ''}`}>
          <div className="big-dot" />
          <div className="stack">
            <div className="status-text">{down ? 'Unreachable' : 'Operational'}</div>
            <div className="sub">
              {down
                ? `no response · ${substrateUrl} · last seen recently`
                : `${substrateUrl} · ${status.data?.name ?? 'contextnest'} ${status.data?.version ?? 'v0.1.0'} · live`}
            </div>
            <div className="sub" style={{ marginTop: 6, color: 'var(--ink-muted)' }}>
              {down
                ? 'Substrate is a local process you control. Restart with the command shown in the inbox banner.'
                : `${totalKinds.toLocaleString()} fragments · ${stats.data?.total_sessions ?? '?'} sessions · 256-dim embeddings`}
            </div>
          </div>
        </div>

        <div className="card" style={{ padding: 16 }}>
          <div className="section-h" style={{ margin: '0 0 12px' }}>
            <h3>Embedding provider</h3>
            <span className="hint">active</span>
          </div>
          <dl className="kv">
            <dt>provider</dt>
            <dd>local TF-IDF (hash)</dd>
            <dt>dim</dt>
            <dd>256</dd>
            <dt>throughput</dt>
            <dd>3,200 q/s</dd>
            <dt>quality</dt>
            <dd style={{ color: 'var(--urg-soon)' }}>★★☆☆☆ uniform sim ~0.99</dd>
          </dl>
          <div
            style={{
              display: 'flex',
              gap: 8,
              marginTop: 14,
              paddingTop: 14,
              borderTop: '1px dashed var(--border)',
            }}
          >
            <button className="btn btn-primary" type="button">
              Switch to Ollama
            </button>
            <button className="btn" type="button">
              Switch to OpenAI
            </button>
          </div>
        </div>
      </div>

      <div className="section-h">
        <h3>Fragments by kind</h3>
        <span className="hint">
          {totalKinds.toLocaleString()} total · {byKindEntries.length} known kinds
        </span>
      </div>
      <div className="card" style={{ padding: '14px 18px' }}>
        {stats.isLoading && byKindEntries.length === 0 ? (
          <div className="empty">
            <div className="empty-title">Loading…</div>
          </div>
        ) : byKindEntries.length === 0 ? (
          <div className="empty">
            <div className="empty-title">No fragments stored yet</div>
            <div className="empty-sub">
              Run <span className="mono">make cn-ingest SINCE=7d</span> to backfill from
              <span className="mono"> ~/.claude/projects</span>, or wait for live hook ingest.
            </div>
          </div>
        ) : (
          byKindEntries.map(([kind, count]) => (
            <div className="kind-bar-row" key={kind}>
              <span className="lbl">{kind}</span>
              <div className="kind-bar">
                <span
                  style={{
                    width: `${(count / maxKind) * 100}%`,
                    background: KIND_COLOR[kind] ?? KIND_COLOR.unknown,
                  }}
                />
              </div>
              <span className="v">{count.toLocaleString()}</span>
            </div>
          ))
        )}
      </div>

      <div className="section-h">
        <h3>Claude Code hooks</h3>
        <span className="hint">read-only · v1</span>
      </div>
      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <HookRow event="SessionStart" url={`${substrateUrl}/api/v1/cc/hook/session_start`} wired />
        <HookRow
          event="UserPromptSubmit"
          url={`${substrateUrl}/api/v1/cc/hook/user_prompt_submit`}
          wired
        />
        <HookRow event="Stop" url={`${substrateUrl}/api/v1/cc/hook/stop`} wired />
        <HookRow
          event="TaskCompleted"
          url={`${substrateUrl}/api/v1/cc/hook/task_completed`}
          wired
        />
        <div
          style={{
            padding: '10px 14px',
            borderTop: '1px solid var(--border)',
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            background: 'var(--surface-2)',
          }}
        >
          <span className="mono dim" style={{ fontSize: 11 }}>
            ~/.claude/settings.json
          </span>
          <div style={{ display: 'flex', gap: 6 }}>
            <button className="btn sm" type="button">
              $ install-hooks
            </button>
            <button className="btn sm btn-ghost" type="button">
              $ uninstall-hooks
            </button>
          </div>
        </div>
      </div>

      <div className="section-h">
        <h3>Recent activity</h3>
        <span className="hint">backend tracking not yet wired</span>
      </div>
      <div className="card" style={{ padding: '14px 18px' }}>
        <div className="empty">
          <div className="empty-title">No activity log yet</div>
          <div className="empty-sub">
            Per-operation timing isn't tracked server-side in v0.1. Check
            <span className="mono"> ~/.contextnest/wal.jsonl</span> for the append-only history
            of every successful store, or watch the server stdout for the
            <span className="mono"> Request completed</span> tracing lines.
          </div>
        </div>
      </div>
    </div>
  );
}

function HookRow({ event, url, wired }: { event: string; url: string; wired: boolean }) {
  return (
    <div
      style={{
        display: 'grid',
        gridTemplateColumns: '180px 1fr 90px',
        gap: 14,
        padding: '12px 14px',
        borderBottom: '1px solid var(--border-subtle)',
        alignItems: 'center',
        fontSize: 12.5,
      }}
    >
      <div style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
        <span
          className="urg-dot"
          style={{
            background: wired ? 'var(--accent)' : 'var(--urg-now)',
            boxShadow: wired ? '0 0 0 3px var(--accent-soft)' : '0 0 0 3px var(--urg-now-soft)',
            marginTop: 0,
          }}
        />
        <span style={{ fontWeight: 500 }}>{event}</span>
      </div>
      <span className="mono dim" style={{ fontSize: 11 }}>
        {url}
      </span>
      <span
        className="mono"
        style={{
          fontSize: 10.5,
          textAlign: 'right',
          color: wired ? 'var(--accent)' : 'var(--urg-now)',
        }}
      >
        {wired ? '● WIRED' : '○ MISSING'}
      </span>
    </div>
  );
}
