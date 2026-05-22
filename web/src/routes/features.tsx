import { createFileRoute } from '@tanstack/react-router';
import { useMemo, useState } from 'react';

import { Icon } from '@/components/atoms';
import { useFeatures } from '@/hooks/useFeatures';
import type { FeatureEntry } from '@/lib/types';

export const Route = createFileRoute('/features')({
  component: FeaturesPage,
});

// Hand-picked windows. The default 24h answers the daily-driver
// "what shipped today" question; everything else is a slow climb
// out into longer histories. `all` maps to 30d which is the upper
// bound the backend's `since` parser accepts.
const WINDOWS = [
  { k: '1h', label: '1h' },
  { k: '6h', label: '6h' },
  { k: '24h', label: '24h' },
  { k: '7d', label: '7d' },
  { k: '30d', label: '30d' },
] as const;

const LAYERS = ['frontend', 'backend', 'infra', 'docs', 'tests', 'other'] as const;

const LAYER_COLOR: Record<string, string> = {
  frontend: '#a78bfa',
  backend: '#00d4aa',
  infra: '#60a5fa',
  docs: '#cbd5e1',
  tests: '#ffd166',
  other: '#71717a',
};

function FeaturesPage() {
  const [since, setSince] = useState<string>('24h');
  const [layer, setLayer] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const { data, isLoading, isError, error, refetch } = useFeatures({
    since,
    layer: layer ?? undefined,
  });

  // Group features by calendar day for the day-heading rendering.
  // Newest day first because the response is already sorted by ts desc.
  const grouped = useMemo(() => {
    const out: { day: string; items: FeatureEntry[] }[] = [];
    let cur: { day: string; items: FeatureEntry[] } | null = null;
    for (const f of data?.features ?? []) {
      const day = (f.ts ?? '').slice(0, 10) || 'unknown';
      if (!cur || cur.day !== day) {
        cur = { day, items: [] };
        out.push(cur);
      }
      cur.items.push(f);
    }
    return out;
  }, [data]);

  const copy = (text: string, id: string) => {
    void navigator.clipboard.writeText(text).then(() => {
      setCopiedId(id);
      setTimeout(() => setCopiedId(null), 1400);
    });
  };

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Features</h1>
          <div className="page-sub">
            Daily inventory of agent-declared deliverables · replayable{' '}
            <span className="mono">how_to_test</span> per feature ·{' '}
            <span className="mono">GET /api/v1/features?since={since}{layer ? `&layer=${layer}` : ''}</span>
          </div>
        </div>
        <div className="page-actions">
          <button
            className="btn"
            onClick={() => refetch()}
            type="button"
          >
            <Icon.Refresh /> Refresh
          </button>
        </div>
      </div>

      <div className="features-filter-bar">
        <span className="filter-label mono">window</span>
        {WINDOWS.map((w) => (
          <button
            key={w.k}
            className={`btn-chip${since === w.k ? ' active' : ''}`}
            onClick={() => setSince(w.k)}
            type="button"
          >
            {w.label}
          </button>
        ))}
        <span className="filter-label mono" style={{ marginLeft: 16 }}>
          layer
        </span>
        <button
          className={`btn-chip${layer === null ? ' active' : ''}`}
          onClick={() => setLayer(null)}
          type="button"
        >
          all
        </button>
        {LAYERS.map((l) => (
          <button
            key={l}
            className={`btn-chip${layer === l ? ' active' : ''}`}
            onClick={() => setLayer(l)}
            type="button"
            style={{
              borderColor:
                layer === l ? LAYER_COLOR[l] : undefined,
            }}
          >
            {l}
          </button>
        ))}
        <div className="grow" />
        <span className="mono dim">
          {isLoading
            ? 'loading…'
            : isError
              ? 'error'
              : `${data?.count ?? 0} feature${(data?.count ?? 0) === 1 ? '' : 's'}`}
        </span>
      </div>

      {isError && (
        <div className="empty with-card">
          <div className="empty-title">Could not load features</div>
          <div className="empty-body">
            {error?.message ?? 'Unknown error'}
          </div>
        </div>
      )}

      {!isError && (data?.features ?? []).length === 0 && !isLoading && (
        <div className="empty with-card">
          <div className="empty-title">No features in this window</div>
          <div className="empty-body">
            Sessions only show up here when their assistant turn emits a{' '}
            <span className="mono">delivered_features[]</span> entry in a
            z-insight block. Widen the window or update your CLAUDE.md
            z-insight protocol so future sessions populate this view.
          </div>
        </div>
      )}

      {grouped.map((group) => (
        <div key={group.day} className="features-day-group">
          <div className="features-day-header mono">
            <Icon.Clock /> {group.day === 'unknown' ? 'undated' : group.day}
            <span className="dim">· {group.items.length}</span>
          </div>
          <div className="features-list">
            {group.items.map((f, i) => {
              const cardId = `${f.session_id}-${i}`;
              const layerColor =
                f.layer && LAYER_COLOR[f.layer] ? LAYER_COLOR[f.layer] : '#71717a';
              return (
                <div key={cardId} className="feature-card">
                  <div className="feature-card-top">
                    <span className="feature-title">{f.feature}</span>
                    {f.layer && (
                      <span
                        className="feature-layer-chip mono"
                        style={{ background: `${layerColor}22`, color: layerColor }}
                      >
                        {f.layer}
                      </span>
                    )}
                    <span className="dim mono feature-ts">
                      {f.ts ? new Date(f.ts).toLocaleTimeString() : '—'}
                    </span>
                  </div>

                  <div className="feature-card-meta mono dim">
                    session{' '}
                    <span style={{ color: 'var(--ink-muted)' }}>
                      {f.session_id.slice(0, 8)}
                    </span>
                    {f.files.length > 0 && (
                      <>
                        <span style={{ margin: '0 6px' }}>·</span>
                        {f.files.length} file{f.files.length === 1 ? '' : 's'}
                      </>
                    )}
                    {f.defs.length > 0 && (
                      <>
                        <span style={{ margin: '0 6px' }}>·</span>
                        {f.defs.length} def{f.defs.length === 1 ? '' : 's'}
                      </>
                    )}
                  </div>

                  {f.files.length > 0 && (
                    <ul className="feature-files">
                      {f.files.map((path) => (
                        <li key={path} className="mono">
                          <Icon.Folder /> {path}
                        </li>
                      ))}
                    </ul>
                  )}

                  {f.defs.length > 0 && (
                    <div className="feature-defs mono">
                      {f.defs.map((d) => (
                        <span key={d} className="feature-def-chip">
                          {d}
                        </span>
                      ))}
                    </div>
                  )}

                  {f.how_to_test && (
                    <div className="feature-how-block">
                      <div className="feature-how-label mono dim">
                        how_to_test
                        <button
                          className="btn-chip-tiny"
                          onClick={() =>
                            f.how_to_test && copy(f.how_to_test, cardId)
                          }
                          type="button"
                          title="copy recipe to clipboard"
                        >
                          {copiedId === cardId ? 'copied' : 'copy'}
                        </button>
                      </div>
                      <pre className="feature-how mono">{f.how_to_test}</pre>
                    </div>
                  )}

                  {Array.isArray(f.refs) && f.refs.length > 0 && (
                    <div className="feature-refs mono dim">
                      refs:{' '}
                      {f.refs
                        .map((r) =>
                          typeof r === 'string' ? r : JSON.stringify(r),
                        )
                        .join(' · ')}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      ))}
    </div>
  );
}
