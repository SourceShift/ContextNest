import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';

import { Icon } from '@/components/atoms';
import { api } from '@/lib/api';

export const Route = createFileRoute('/config')({
  component: ConfigPage,
});

/**
 * Read-only substrate config viewer (Ticket #6 from
 * docs/todos/20260602-1230-cn-value-prop-coverage-epic.md).
 *
 * Single answer to "is the latest binary actually running?" — shows
 * the version + git commit + which embedder model is live + which
 * LLM providers are configured + whether the LLM cache is encrypted.
 * No editing — env vars and config.toml stay the sources of truth.
 *
 * Backed by `GET /api/v1/substrate/config`.
 */
function ConfigPage() {
  const { data, isLoading, isError, error, refetch } = useQuery({
    queryKey: ['substrate-config'],
    queryFn: () => api.substrateConfig(),
    staleTime: 30_000,
  });

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Config</h1>
          <div className="page-sub">
            Read-only snapshot of what the running substrate has loaded ·{' '}
            <span className="mono">GET /api/v1/substrate/config</span>
          </div>
        </div>
        <div className="page-actions">
          <button
            className="btn"
            onClick={() => void refetch()}
            type="button"
            title="Re-fetch the snapshot"
          >
            <Icon.Refresh /> Refresh
          </button>
        </div>
      </div>

      {isError && (
        <div className="card" style={{ padding: 16, marginBottom: 16 }}>
          <div className="empty-title">Could not load config</div>
          <div className="empty-body mono" style={{ fontSize: 12 }}>
            {error instanceof Error ? error.message : String(error)}
          </div>
        </div>
      )}

      {isLoading && !data && (
        <div className="card" style={{ padding: 16 }}>
          <div className="dim mono">loading…</div>
        </div>
      )}

      {data && (
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
            gap: 12,
          }}
        >
          <ConfigCard
            title="Build"
            rows={[
              ['version', data.version],
              ['git_commit', data.git_commit || 'unknown'],
            ]}
            note={
              data.git_commit === 'unknown'
                ? "git_commit reports 'unknown' — rebuild with CONTEXTNEST_GIT_COMMIT=$(git rev-parse --short HEAD) to see the actual SHA"
                : undefined
            }
          />

          <ConfigCard
            title="Embedding"
            rows={[
              ['model', data.embedding.model],
              [
                'mode',
                data.embedding.model === 'local'
                  ? 'TF-IDF fallback'
                  : 'remote provider',
              ],
            ]}
            note={
              data.embedding.model === 'local'
                ? 'Local TF-IDF embedder is running. Similarity scores will be uniformly high — switch to a real embedder (Qwen / OpenAI / DeepInfra) in config.toml for meaningful semantic ranking.'
                : undefined
            }
          />

          <ConfigCard
            title="LLM provider"
            rows={[
              ['enabled', String(data.llm.enabled)],
              ['default', data.llm.default_provider ?? '(none)'],
              [
                'configured',
                data.llm.configured_providers.length === 0
                  ? '(none)'
                  : data.llm.configured_providers.join(', '),
              ],
            ]}
            note={
              !data.llm.enabled
                ? 'No LLM provider configured. Summarize falls through to stats-only; chat-completion proxy returns 503. Set ANTHROPIC_API_KEY / OPENAI_API_KEY / GOOGLE_API_KEY in env to enable.'
                : undefined
            }
          />

          <ConfigCard
            title="LLM cache"
            rows={[
              ['encryption_enabled', String(data.llm_cache.encryption_enabled)],
              ['similarity_threshold', data.llm_cache.similarity_threshold.toFixed(2)],
              ['default_ttl_secs', String(data.llm_cache.default_ttl_secs)],
              ['total_entries', data.llm_cache.total_entries.toLocaleString()],
            ]}
            note={
              !data.llm_cache.encryption_enabled
                ? 'Cache entries are stored as plaintext in the WAL. Set CONTEXTNEST_LLM_CACHE_ENCRYPTION_KEY to a 32-byte hex string to enable AES-256-GCM at-rest encryption.'
                : undefined
            }
          />
        </div>
      )}

      <div className="note-banner" style={{ marginTop: 14 }}>
        <span className="dot" style={{ background: 'var(--accent)' }} />
        <span>
          Settings are read-only here. Edit{' '}
          <span className="mono">config.toml</span> + restart, or change env vars
          and restart, to mutate. Multi-provider routing rules &amp; per-kind
          weights ship via the substrate's compiled defaults (see{' '}
          <span className="mono">CONTEXTNEST_KIND_WEIGHT_*</span> overrides).
        </span>
      </div>
    </div>
  );
}

function ConfigCard({
  title,
  rows,
  note,
}: {
  title: string;
  rows: Array<[string, string]>;
  note?: string;
}) {
  return (
    <div
      className="card"
      style={{
        padding: '12px 14px',
        display: 'flex',
        flexDirection: 'column',
        gap: 6,
      }}
    >
      <div className="mono dim" style={{ fontSize: 10.5, textTransform: 'uppercase' }}>
        {title}
      </div>
      <table
        style={{
          width: '100%',
          borderCollapse: 'collapse',
          fontSize: 12.5,
        }}
      >
        <tbody>
          {rows.map(([k, v]) => (
            <tr key={k} style={{ borderBottom: '1px solid var(--border)' }}>
              <td
                className="mono dim"
                style={{ padding: '4px 0', width: '45%', fontSize: 11.5 }}
              >
                {k}
              </td>
              <td
                className="mono"
                style={{
                  padding: '4px 0',
                  color: 'var(--ink)',
                  wordBreak: 'break-all',
                }}
              >
                {v}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      {note && (
        <div
          style={{
            fontSize: 11.5,
            color: 'var(--ink-muted)',
            fontStyle: 'italic',
            marginTop: 4,
            paddingLeft: 6,
            borderLeft: '2px solid var(--urg-soon)',
            lineHeight: 1.4,
          }}
        >
          {note}
        </div>
      )}
    </div>
  );
}
