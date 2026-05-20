import { createFileRoute } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import { Icon } from '@/components/atoms';
import { MOCK } from '@/lib/mock-data';

export const Route = createFileRoute('/tools')({
  component: ToolsPage,
});

const TOOLS = [
  'store',
  'retrieve',
  'update',
  'summarize',
  'discard',
  'reconstruct',
  'resonate',
] as const;
type ToolName = (typeof TOOLS)[number];

const TOOL_DESCRIPTIONS: Record<ToolName, string> = {
  store: 'Persist a memory fragment with kind + metadata. Returns the assigned fragment_id.',
  retrieve: 'Vector + metadata search within a single session. Top-k cap 50.',
  update: 'Patch the metadata of an existing fragment. Content is immutable.',
  summarize: 'Generate a derived summary fragment from a windowed slice of the session.',
  discard:
    'Soft-delete · tags discarded=true. The fragment stays queryable with show_discarded=true.',
  reconstruct: "Replay the substrate's state at a past timestamp for debugging.",
  resonate:
    'BFS from a seed fragment along learned similarity edges. Returns the resonance subgraph.',
};

type Resp =
  | { kind: 'idle' }
  | { kind: 'sending' }
  | { kind: 'ok'; body: unknown; status: number; ms: number }
  | { kind: 'err'; body: unknown; status: number; ms: number };

function ToolsPage() {
  const [tool, setTool] = useState<ToolName>('retrieve');
  const [tmpl, setTmpl] = useState('basic');
  const [resp, setResp] = useState<Resp>({ kind: 'idle' });

  const reqText = useMemo(
    () => JSON.stringify(MOCK.toolTemplates[tool] ?? {}, null, 2),
    // tmpl participates in the request key so swapping it forces a fresh
    // template body, even though we don't yet vary the body by tmpl.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tool, tmpl],
  );

  const handleSelectTool = (t: ToolName) => {
    setTool(t);
    setResp({ kind: 'idle' });
  };

  const handleSelectTmpl = (t: string) => {
    setTmpl(t);
    setResp({ kind: 'idle' });
  };

  const send = () => {
    setResp({ kind: 'sending' });
    setTimeout(() => {
      const r = (MOCK.toolResponses as Record<string, unknown>)[tool];
      if (r) setResp({ kind: 'ok', body: r, status: 200, ms: 142 });
      else
        setResp({
          kind: 'ok',
          body: { ok: true, note: `${tool} executed (mocked response)` },
          status: 200,
          ms: 88,
        });
    }, 600);
  };

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Tools</h1>
          <div className="page-sub">
            Power-user playground for the seven-tool substrate API ·{' '}
            <span className="mono">/api/v1/tools/&lt;tool&gt;</span>
          </div>
        </div>
        <div className="page-actions">
          <span className="mono dim" style={{ fontSize: 11 }}>
            Replaces curl. All requests stay on localhost.
          </span>
        </div>
      </div>

      <div className="tools-tabs">
        {TOOLS.map((t) => (
          <button
            key={t}
            className={tool === t ? 'active' : ''}
            onClick={() => handleSelectTool(t)}
            type="button"
          >
            {t}
          </button>
        ))}
      </div>

      <div className="note-banner" style={{ marginBottom: 14 }}>
        <Icon.Info style={{ color: 'var(--ink-dim)' }} />
        <span>
          <span className="mono" style={{ color: 'var(--accent)' }}>
            {tool}
          </span>{' '}
          · {TOOL_DESCRIPTIONS[tool]}
        </span>
      </div>

      <div className="tools-grid">
        <div className="tools-pane">
          <div className="tools-pane-head">
            <span>Request · POST /api/v1/tools/{tool}</span>
            <div className="right">
              <span className="mono dim" style={{ fontSize: 10 }}>
                templates:
              </span>
              {['basic', 'inbox-item', 'goal-phase'].map((t) => (
                <span
                  key={t}
                  className={`chip${tmpl === t ? ' active' : ''}`}
                  onClick={() => handleSelectTmpl(t)}
                  style={{ padding: '2px 7px', fontSize: 10.5 }}
                >
                  ▸ {t}
                </span>
              ))}
            </div>
          </div>
          <div className="json-editor">
            <Gutter text={reqText} />
            <pre className="json-body" style={{ margin: 0 }}>
              <JsonHL src={reqText} />
            </pre>
          </div>
          <div className="tools-foot">
            <span className="mono dim" style={{ fontSize: 10.5 }}>
              {reqText.split('\n').length} lines · {reqText.length} bytes · valid json
            </span>
            <button className="btn btn-primary" onClick={send} type="button">
              <Icon.Send /> Send{' '}
              <span className="kbd" style={{ marginLeft: 4 }}>
                ⌘↵
              </span>
            </button>
          </div>
        </div>

        <div className="tools-pane">
          <div className="tools-pane-head">
            <span>Response</span>
            <div className="right">
              {resp.kind === 'ok' && (
                <span className="status-pill ok">
                  {resp.status} · {resp.ms}ms
                </span>
              )}
              {resp.kind === 'err' && (
                <span className="status-pill err">
                  {resp.status} · {resp.ms}ms
                </span>
              )}
              {resp.kind === 'idle' && <span className="status-pill idle">idle</span>}
              {resp.kind === 'sending' && <span className="status-pill idle">sending…</span>}
              <button className="btn btn-ghost sm" title="copy" type="button">
                <Icon.Copy />
              </button>
            </div>
          </div>
          {resp.kind === 'idle' && (
            <div
              style={{
                padding: 30,
                color: 'var(--ink-dim)',
                fontSize: 12,
                textAlign: 'center',
                flex: 1,
                display: 'flex',
                flexDirection: 'column',
                justifyContent: 'center',
                gap: 8,
              }}
            >
              <div className="mono dim" style={{ fontSize: 11 }}>
                ↳ Send a request to see the typed response here.
              </div>
              <div className="mono" style={{ fontSize: 10, color: 'var(--ink-faint)' }}>
                shortcuts · cmd+↵ send · cmd+/ focus body · cmd+1..7 switch tool
              </div>
            </div>
          )}
          {resp.kind === 'sending' && (
            <div style={{ padding: 30, flex: 1, display: 'flex', flexDirection: 'column', gap: 8 }}>
              {[1, 2, 3, 4].map((i) => (
                <div
                  key={i}
                  className="skel"
                  style={{ height: 14, width: `${[80, 60, 90, 55][i - 1]}%` }}
                />
              ))}
            </div>
          )}
          {resp.kind === 'ok' && (
            <div className="json-editor">
              <Gutter text={JSON.stringify(resp.body, null, 2)} />
              <pre className="json-body" style={{ margin: 0 }}>
                <JsonHL src={JSON.stringify(resp.body, null, 2)} />
              </pre>
            </div>
          )}
        </div>
      </div>

      <div style={{ marginTop: 18 }} className="section-h">
        <h3>Recent calls · this session</h3>
        <span className="hint">last 5</span>
      </div>
      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        {MOCK.recentActivity.slice(0, 5).map((a, i) => (
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

function Gutter({ text }: { text: string }) {
  const n = text.split('\n').length;
  return (
    <div className="json-gutter">
      {Array.from({ length: n }).map((_, i) => (
        <div key={i}>{i + 1}</div>
      ))}
    </div>
  );
}

function JsonHL({ src }: { src: string }) {
  const tokens: Array<{ t: string; v: string }> = [];
  const re =
    /("[^"\\]*(?:\\.[^"\\]*)*")(\s*:)?|\b(true|false|null)\b|(-?\d+(?:\.\d+)?)|([{}[\],])/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(src)) !== null) {
    if (m.index > last) tokens.push({ t: 'raw', v: src.slice(last, m.index) });
    if (m[1]) {
      tokens.push({ t: m[2] ? 'key' : 'str', v: m[1] });
      if (m[2]) tokens.push({ t: 'punct', v: m[2] });
    } else if (m[3]) tokens.push({ t: 'bool', v: m[3] });
    else if (m[4]) tokens.push({ t: 'num', v: m[4] });
    else if (m[5]) tokens.push({ t: 'punct', v: m[5] });
    last = m.index + m[0].length;
  }
  if (last < src.length) tokens.push({ t: 'raw', v: src.slice(last) });

  return (
    <>
      {tokens.map((tk, i) => {
        if (tk.t === 'key')
          return (
            <span key={i} className="json-key">
              {tk.v}
            </span>
          );
        if (tk.t === 'str')
          return (
            <span key={i} className="json-str">
              {tk.v}
            </span>
          );
        if (tk.t === 'bool')
          return (
            <span key={i} className="json-bool">
              {tk.v}
            </span>
          );
        if (tk.t === 'num')
          return (
            <span key={i} className="json-num">
              {tk.v}
            </span>
          );
        if (tk.t === 'punct')
          return (
            <span key={i} className="json-punct">
              {tk.v}
            </span>
          );
        return <span key={i}>{tk.v}</span>;
      })}
    </>
  );
}
