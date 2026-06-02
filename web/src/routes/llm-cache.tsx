import { createFileRoute } from '@tanstack/react-router';
import { useEffect, useRef, useState } from 'react';
import { useQuery } from '@tanstack/react-query';

import { Icon } from '@/components/atoms';
import { api } from '@/lib/api';
import type { LlmCacheStats } from '@/lib/types';

export const Route = createFileRoute('/llm-cache')({
  component: LlmCachePage,
});

/**
 * LLM proxy cache observability (Ticket #5 from the coverage epic).
 *
 * One-stop view of the cache's behaviour at runtime: total entries
 * stored, raw hit/miss counters, derived hit rate. Auto-refreshes
 * every 5s so an operator running a workload can watch the counters
 * climb live without manual refresh.
 *
 * Also keeps an in-memory client-side sparkline of hit_rate over the
 * page session — gives a quick visual on whether the cache is
 * warming up, stable, or thrashing. Not persisted; refresh and the
 * sparkline resets.
 *
 * Backed by GET /llm/v1/cache/stats.
 */
function LlmCachePage() {
  const { data, isLoading, isError, error, refetch, isFetching } = useQuery({
    queryKey: ['llm-cache-stats'],
    queryFn: () => api.llmCacheStats(),
    refetchInterval: 5000,
    refetchIntervalInBackground: false,
  });

  // Sparkline of hit_rate samples. Capped at 60 points (5min @ 5s) so
  // the rolling history stays bounded without unbounded memory growth.
  const [history, setHistory] = useState<number[]>([]);
  const lastSampleRef = useRef<number | null>(null);

  useEffect(() => {
    if (!data) return;
    // Avoid back-to-back identical samples polluting the sparkline.
    if (lastSampleRef.current === data.hit_rate) return;
    lastSampleRef.current = data.hit_rate;
    setHistory((h) => [...h.slice(-59), data.hit_rate]);
  }, [data]);

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">LLM cache</h1>
          <div className="page-sub">
            Live counters from the OpenAI-compatible proxy ·{' '}
            <span className="mono">GET /llm/v1/cache/stats</span>
            {isFetching && <span className="dim"> · refreshing…</span>}
          </div>
        </div>
        <div className="page-actions">
          <button
            className="btn"
            onClick={() => void refetch()}
            type="button"
            title="Force a refresh (auto-refresh runs every 5s)"
          >
            <Icon.Refresh /> Refresh
          </button>
        </div>
      </div>

      {isError && (
        <div className="card" style={{ padding: 16, marginBottom: 16 }}>
          <div className="empty-title">Could not load cache stats</div>
          <div className="empty-body mono" style={{ fontSize: 12 }}>
            {error instanceof Error ? error.message : String(error)}
          </div>
          <div className="empty-body" style={{ fontSize: 12, marginTop: 8 }}>
            Common cause: no LLM provider configured, so the proxy isn't
            mounted. Check <span className="mono">/config</span> →{' '}
            <strong>LLM provider</strong>; set{' '}
            <span className="mono">ANTHROPIC_API_KEY</span> /{' '}
            <span className="mono">OPENAI_API_KEY</span> and restart.
          </div>
        </div>
      )}

      {isLoading && !data && (
        <div className="card" style={{ padding: 16 }}>
          <div className="dim mono">loading…</div>
        </div>
      )}

      {data && (
        <>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
              gap: 12,
              marginBottom: 14,
            }}
          >
            <StatTile
              label="hit rate"
              value={`${(data.hit_rate * 100).toFixed(1)}%`}
              sub={hitRateNote(data)}
            />
            <StatTile label="total entries" value={data.total_entries.toLocaleString()} />
            <StatTile
              label="total hits"
              value={data.total_hits.toLocaleString()}
              sub="lookups served from cache"
            />
            <StatTile
              label="total misses"
              value={data.total_misses.toLocaleString()}
              sub="lookups that fell through to provider"
            />
          </div>

          <div className="card" style={{ padding: 14, marginBottom: 14 }}>
            <div
              className="mono dim"
              style={{ fontSize: 10.5, textTransform: 'uppercase', marginBottom: 8 }}
            >
              hit rate trend ({history.length} sample{history.length === 1 ? '' : 's'},
              5s interval)
            </div>
            <Sparkline values={history} />
            {history.length < 2 && (
              <div className="dim mono" style={{ fontSize: 11, marginTop: 6 }}>
                Need at least 2 distinct samples — drive some traffic through
                the proxy and the sparkline will populate.
              </div>
            )}
          </div>
        </>
      )}

      <div className="note-banner" style={{ marginTop: 14 }}>
        <span className="dot" style={{ background: 'var(--accent)' }} />
        <span>
          Stats are in-memory only; counters reset on restart. The cache
          itself persists encrypted entries to the WAL when{' '}
          <span className="mono">CONTEXTNEST_LLM_CACHE_ENCRYPTION_KEY</span>{' '}
          is set — see <span className="mono">/config</span> for the
          live encryption state.
        </span>
      </div>
    </div>
  );
}

function hitRateNote(s: LlmCacheStats): string {
  const total = s.total_hits + s.total_misses;
  if (total === 0) return 'no lookups yet';
  if (total < 10) return `only ${total} lookups — not yet statistically meaningful`;
  if (s.hit_rate < 0.2) return 'cold cache or high cardinality';
  if (s.hit_rate > 0.8) return 'cache is doing its job';
  return `${total.toLocaleString()} lookups`;
}

function StatTile({
  label,
  value,
  sub,
}: {
  label: string;
  value: string;
  sub?: string;
}) {
  return (
    <div
      className="card"
      style={{
        padding: '10px 12px',
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
      }}
    >
      <div className="mono dim" style={{ fontSize: 10.5, textTransform: 'uppercase' }}>
        {label}
      </div>
      <div className="mono" style={{ fontSize: 22, color: 'var(--ink)' }}>
        {value}
      </div>
      {sub && (
        <div className="dim" style={{ fontSize: 11, fontStyle: 'italic' }}>
          {sub}
        </div>
      )}
    </div>
  );
}

/**
 * Tiny inline sparkline — SVG path through the value array, scaled to
 * the [min, max] of the window. No external charting dep so the cache
 * stats page stays lightweight (the dashboard's existing field
 * visualisation is the only chart-heavy surface).
 */
function Sparkline({ values }: { values: number[] }) {
  const width = 600;
  const height = 56;
  const padding = 4;
  if (values.length === 0) {
    return (
      <div className="dim mono" style={{ fontSize: 11 }}>
        no samples yet
      </div>
    );
  }
  const min = Math.min(...values);
  const max = Math.max(...values);
  const range = max - min || 1;
  const step = values.length > 1 ? (width - padding * 2) / (values.length - 1) : 0;
  const path = values
    .map((v, i) => {
      const x = padding + i * step;
      const y = height - padding - ((v - min) / range) * (height - padding * 2);
      return `${i === 0 ? 'M' : 'L'} ${x.toFixed(1)} ${y.toFixed(1)}`;
    })
    .join(' ');
  return (
    <svg
      viewBox={`0 0 ${width} ${height}`}
      preserveAspectRatio="none"
      style={{ width: '100%', height, display: 'block' }}
      aria-label="hit rate sparkline"
    >
      <path
        d={path}
        fill="none"
        stroke="var(--accent)"
        strokeWidth={1.5}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
