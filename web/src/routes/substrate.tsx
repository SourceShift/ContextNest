import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';

import { Icon } from '@/components/atoms';
import { api, substrateUrl } from '@/lib/api';
import { MOCK } from '@/lib/mock-data';

export const Route = createFileRoute('/substrate')({
  component: SubstratePage,
});

function SubstratePage() {
  const health = useQuery({ queryKey: ['health'], queryFn: () => api.health() });
  const status = useQuery({ queryKey: ['status'], queryFn: () => api.status() });

  const down = health.isError || (!health.isLoading && !health.data?.healthy);
  const totalKinds = MOCK.kindCounts.reduce((a, k) => a + k.n, 0);
  const maxKind = Math.max(...MOCK.kindCounts.map((k) => k.n));

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
                : '619 fragments · 256-dim embeddings · TF-IDF (default)'}
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
        <span className="hint">{totalKinds} total · 12 known kinds</span>
      </div>
      <div className="card" style={{ padding: '14px 18px' }}>
        {MOCK.kindCounts.map((k, i) => (
          <div className="kind-bar-row" key={i}>
            <span className="lbl">{k.k}</span>
            <div className="kind-bar">
              <span style={{ width: `${(k.n / maxKind) * 100}%`, background: k.color }} />
            </div>
            <span className="v">{k.n}</span>
          </div>
        ))}
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
        <span className="hint">last 10 ops · live</span>
      </div>
      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        <div
          className="activity-row"
          style={{
            background: 'var(--surface-2)',
            color: 'var(--ink-faint)',
            textTransform: 'uppercase',
            letterSpacing: '0.08em',
            fontSize: 10,
            borderBottom: '1px solid var(--border)',
          }}
        >
          <span>time</span>
          <span>op</span>
          <span>target</span>
          <span style={{ textAlign: 'right' }}>ms</span>
          <span style={{ textAlign: 'right' }}>code</span>
        </div>
        {MOCK.recentActivity.map((a, i) => (
          <div className="activity-row" key={i}>
            <span className="t">{a.t}</span>
            <span className={`verb v-${a.verb}`}>{a.verb}</span>
            <span>{a.target}</span>
            <span className="t">{a.ms}ms</span>
            <span className="ok">200</span>
          </div>
        ))}
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
