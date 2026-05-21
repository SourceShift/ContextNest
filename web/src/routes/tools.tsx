import { createFileRoute } from '@tanstack/react-router';
import { useEffect, useMemo, useState } from 'react';

import { Icon } from '@/components/atoms';
import { ApiError, substrateUrl } from '@/lib/api';
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

type RecentCall = {
  t: string;
  verb: ToolName;
  ms: number;
  status: number;
  ok: boolean;
};

// Each tool maps to its `api.<method>` client call. We POST raw JSON
// rather than going through the typed client for every tool because:
// 1. Users on the Tools page are intentionally driving raw API calls
//    and need to see status/timing exactly as the wire returns them.
// 2. The typed client doesn't expose summarize/reconstruct/resonate yet.
// 3. Constructing a generic fetch keeps the playground future-proof:
//    new tools added to the backend's seven-tool API surface here
//    without a client release.
async function sendRaw(
  tool: ToolName,
  body: unknown,
): Promise<{ status: number; body: unknown; ms: number }> {
  const url = `${substrateUrl}/api/v1/tools/${tool}`;
  const start = performance.now();
  const res = await fetch(url, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  const ms = Math.round(performance.now() - start);
  let parsed: unknown = null;
  try {
    parsed = await res.json();
  } catch {
    parsed = { _note: 'non-JSON response body' };
  }
  return { status: res.status, body: parsed, ms };
}

function ToolsPage() {
  const [tool, setTool] = useState<ToolName>('retrieve');
  const [tmpl, setTmpl] = useState('basic');
  const [resp, setResp] = useState<Resp>({ kind: 'idle' });
  const [recent, setRecent] = useState<RecentCall[]>([]);
  const [bodyText, setBodyText] = useState('');
  const [parseErr, setParseErr] = useState<string | null>(null);

  // Default body comes from MOCK.toolTemplates — these are starter
  // values, not "data". Users can edit before Send.
  const defaultText = useMemo(
    () => JSON.stringify(MOCK.toolTemplates[tool] ?? {}, null, 2),
    [tool],
  );

  // When the user switches tool or template, reset the editor to the
  // tool's default body. They've already-edited bodies are not
  // preserved across tool switches (intentional — different tools take
  // different shapes; carrying over creates more confusion than it
  // saves).
  useEffect(() => {
    setBodyText(defaultText);
    setResp({ kind: 'idle' });
    setParseErr(null);
  }, [tool, tmpl, defaultText]);

  // Parse-check on every keystroke so the footer can show validity.
  useEffect(() => {
    try {
      JSON.parse(bodyText);
      setParseErr(null);
    } catch (e) {
      setParseErr(e instanceof Error ? e.message : String(e));
    }
  }, [bodyText]);

  const handleSelectTool = (t: ToolName) => {
    setTool(t);
  };

  const handleSelectTmpl = (t: string) => {
    setTmpl(t);
  };

  const send = async () => {
    if (parseErr) return; // button is disabled, but belt-and-suspenders
    let parsed: unknown;
    try {
      parsed = JSON.parse(bodyText);
    } catch (e) {
      setResp({
        kind: 'err',
        status: 0,
        body: { error: 'request body is not valid JSON', detail: String(e) },
        ms: 0,
      });
      return;
    }

    setResp({ kind: 'sending' });
    try {
      const { status, body, ms } = await sendRaw(tool, parsed);
      const ok = status >= 200 && status < 300;
      setResp({ kind: ok ? 'ok' : 'err', status, body, ms });
      setRecent((prev) =>
        [
          {
            t: new Date().toLocaleTimeString(undefined, { hour12: false }),
            verb: tool,
            ms,
            status,
            ok,
          },
          ...prev,
        ].slice(0, 20),
      );
    } catch (e) {
      const detail =
        e instanceof ApiError ? e.message : e instanceof Error ? e.message : String(e);
      setResp({
        kind: 'err',
        status: 0,
        body: { error: 'network failure', detail },
        ms: 0,
      });
      setRecent((prev) =>
        [
          {
            t: new Date().toLocaleTimeString(undefined, { hour12: false }),
            verb: tool,
            ms: 0,
            status: 0,
            ok: false,
          },
          ...prev,
        ].slice(0, 20),
      );
    }
  };

  // Cmd/Ctrl+Enter to send. Mounting it at the page level so the
  // shortcut works whether the textarea is focused or not.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key === 'Enter') {
        e.preventDefault();
        void send();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
    // send closes over current state via the function identity; React
    // re-creates it each render. Re-binding is cheap.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bodyText, tool, parseErr]);

  const respText =
    resp.kind === 'ok' || resp.kind === 'err' ? JSON.stringify(resp.body, null, 2) : '';

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
              {['basic'].map((t) => (
                <span
                  key={t}
                  className={`chip${tmpl === t ? ' active' : ''}`}
                  onClick={() => handleSelectTmpl(t)}
                  style={{ padding: '2px 7px', fontSize: 10.5 }}
                >
                  ▸ {t}
                </span>
              ))}
              <button
                className="btn btn-ghost sm"
                title="reset to template"
                onClick={() => setBodyText(defaultText)}
                type="button"
              >
                <Icon.Refresh />
              </button>
            </div>
          </div>
          <textarea
            value={bodyText}
            onChange={(e) => setBodyText(e.target.value)}
            spellCheck={false}
            className="json-body"
            style={{
              width: '100%',
              minHeight: 280,
              fontFamily: 'var(--font-mono)',
              fontSize: 12.5,
              padding: 14,
              background: 'var(--surface)',
              color: 'var(--ink)',
              border: 'none',
              borderTop: '1px solid var(--border-subtle)',
              outline: 'none',
              resize: 'vertical',
            }}
          />
          <div className="tools-foot">
            <span
              className="mono dim"
              style={{
                fontSize: 10.5,
                color: parseErr ? 'var(--urg-now)' : undefined,
              }}
            >
              {bodyText.split('\n').length} lines · {bodyText.length} bytes ·{' '}
              {parseErr ? `invalid: ${parseErr}` : 'valid json'}
            </span>
            <button
              className="btn btn-primary"
              onClick={send}
              type="button"
              disabled={!!parseErr || resp.kind === 'sending'}
            >
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
                  {resp.status || 'ERR'} · {resp.ms}ms
                </span>
              )}
              {resp.kind === 'idle' && <span className="status-pill idle">idle</span>}
              {resp.kind === 'sending' && <span className="status-pill idle">sending…</span>}
              <button
                className="btn btn-ghost sm"
                title="copy"
                onClick={() => {
                  if (respText) void navigator.clipboard?.writeText(respText);
                }}
                type="button"
              >
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
                shortcuts · cmd+↵ send
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
          {(resp.kind === 'ok' || resp.kind === 'err') && (
            <div className="json-editor">
              <Gutter text={respText} />
              <pre className="json-body" style={{ margin: 0 }}>
                <JsonHL src={respText} />
              </pre>
            </div>
          )}
        </div>
      </div>

      <div style={{ marginTop: 18 }} className="section-h">
        <h3>Recent calls · this session</h3>
        <span className="hint">last {Math.min(recent.length, 20)} · client-side only</span>
      </div>
      <div className="card" style={{ padding: 0, overflow: 'hidden' }}>
        {recent.length === 0 ? (
          <div className="empty">
            <div className="empty-title">No calls yet</div>
            <div className="empty-sub">
              Press <span className="kbd">⌘ ↵</span> with a valid request body to send your first
              call. Calls are tracked only in this page; reload clears them.
            </div>
          </div>
        ) : (
          recent.map((a, i) => (
            <div className="activity-row" key={i}>
              <span className="t">{a.t}</span>
              <span className={`verb v-${a.verb}`}>{a.verb}</span>
              <span>POST /api/v1/tools/{a.verb}</span>
              <span className="t">{a.ms}ms</span>
              <span className={a.ok ? 'ok' : 'err'}>{a.status || 'ERR'}</span>
            </div>
          ))
        )}
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
