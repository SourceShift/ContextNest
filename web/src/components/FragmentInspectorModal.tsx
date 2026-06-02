import { useEffect, useMemo } from 'react';
import { useQuery } from '@tanstack/react-query';

import { Icon, KindBadge, ProjBadge, SessionPill } from '@/components/atoms';
import { api } from '@/lib/api';
import type { RetrieveHit } from '@/lib/types';

/**
 * Read-only fragment inspector (Ticket #3 from the coverage epic).
 *
 * Search rows render the snippet + meta-row but truncate aggressively
 * and only show kind + ts. When the user needs the WHY ("which exact
 * fragment matched? what other metadata does it carry? which basin?"),
 * they click → this modal opens with the full payload.
 *
 * Avoids inline expansion because metadata can be wide (timestamps,
 * tool params, refs) and full content can be paragraphs. A modal
 * keeps the search results scrollable beneath.
 *
 * Reads only — promote/discard live on a separate ticket so this
 * stays a pure observability surface.
 */
export function FragmentInspectorModal({
  hit,
  onClose,
}: {
  hit: RetrieveHit | null;
  onClose: () => void;
}) {
  // Escape closes the modal. Re-binds when `hit` becomes non-null so
  // we don't keep a stale listener around on background renders.
  useEffect(() => {
    if (!hit) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [hit, onClose]);

  // Resolve the basin this fragment belongs to (if any) by scanning
  // the session's basin list and matching on session_id presence.
  // Cheap — basins endpoint already returns the full list per session,
  // and TanStack Query caches it across re-opens for the same session.
  const basinsQuery = useQuery({
    queryKey: ['basins-for-fragment', hit?.session_id ?? null],
    queryFn: () =>
      hit?.session_id
        ? api.basins({ session_id: hit.session_id })
        : Promise.resolve({ basins: [] }),
    enabled: !!hit?.session_id,
    staleTime: 60_000,
  });

  const owningBasin = useMemo(() => {
    if (!hit) return null;
    const all = basinsQuery.data?.basins ?? [];
    // BE returns basins with `sessions: string[]`. There's no per-id
    // membership in the public response, so we display the candidate
    // set scoped to this fragment's session — the user picks if there
    // are multiple. Most sessions resolve to a single dominant basin.
    return all.length > 0 ? all : null;
  }, [basinsQuery.data, hit]);

  if (!hit) return null;

  const kind = (hit.metadata.kind as string | undefined) ?? 'unknown';
  const ts = hit.metadata.ts as string | undefined;

  // Pull metadata into [key, value] rows. Stringify nested objects
  // so the table renders without React errors. Order: known keys
  // first (kind, ts, urgency, awaiting_decision), then alphabetical.
  const metaRows = useMemo(() => {
    const known = ['kind', 'ts', 'urgency', 'awaiting_decision', 'step', 'reason', 'task_status'];
    const entries = Object.entries(hit.metadata);
    const knownRows = known
      .map((k) => entries.find(([key]) => key === k))
      .filter((x): x is [string, unknown] => !!x);
    const restRows = entries
      .filter(([k]) => !known.includes(k))
      .sort(([a], [b]) => a.localeCompare(b));
    return [...knownRows, ...restRows].map<[string, string]>(([k, v]) => [
      k,
      typeof v === 'string' ? v : JSON.stringify(v),
    ]);
  }, [hit.metadata]);

  return (
    <div
      role="dialog"
      aria-modal="true"
      aria-labelledby="fragment-inspector-title"
      onClick={onClose}
      style={{
        position: 'fixed',
        inset: 0,
        background: 'rgba(0, 0, 0, 0.55)',
        backdropFilter: 'blur(4px)',
        WebkitBackdropFilter: 'blur(4px)',
        zIndex: 1000,
        display: 'flex',
        alignItems: 'flex-start',
        justifyContent: 'center',
        padding: '6vh 16px 16px',
        overflowY: 'auto',
      }}
    >
      <div
        className="card"
        onClick={(e) => e.stopPropagation()}
        style={{
          width: '100%',
          maxWidth: 760,
          padding: 0,
          display: 'flex',
          flexDirection: 'column',
          maxHeight: '85vh',
        }}
      >
        {/* Header — meta-row + close button. */}
        <div
          style={{
            display: 'flex',
            alignItems: 'flex-start',
            justifyContent: 'space-between',
            gap: 12,
            padding: '14px 16px',
            borderBottom: '1px solid var(--border)',
          }}
        >
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            <div id="fragment-inspector-title" className="meta-row">
              <KindBadge kind={kind} />
              {hit.session_id && <SessionPill id={hit.session_id} />}
              {(() => {
                const p = (hit as RetrieveHit & { project?: string }).project;
                return p ? <ProjBadge p={p} /> : null;
              })()}
              {ts && <span className="dim">· {ts}</span>}
            </div>
            <div className="mono dim" style={{ fontSize: 10.5 }}>
              id · {hit.id}
            </div>
          </div>
          <button
            className="btn btn-ghost sm"
            onClick={onClose}
            type="button"
            aria-label="Close inspector"
            title="Close (Esc)"
          >
            <Icon.X />
          </button>
        </div>

        {/* Body — full content + metadata table + basin section. */}
        <div style={{ overflowY: 'auto', padding: '14px 16px' }}>
          <SectionLabel>Content</SectionLabel>
          <div
            style={{
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              fontSize: 13,
              lineHeight: 1.5,
              padding: '8px 10px',
              background: 'var(--bg-soft)',
              borderRadius: 6,
              border: '1px solid var(--border)',
              marginBottom: 14,
            }}
          >
            {hit.content}
          </div>

          <SectionLabel>Scores</SectionLabel>
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(2, 1fr)',
              gap: 8,
              marginBottom: 14,
            }}
          >
            <ScoreTile label="similarity" value={hit.similarity.toFixed(3)} />
            <ScoreTile label="importance" value={hit.importance.toFixed(3)} />
          </div>

          <SectionLabel>Metadata ({metaRows.length})</SectionLabel>
          <table
            style={{
              width: '100%',
              borderCollapse: 'collapse',
              fontSize: 12.5,
              marginBottom: 14,
            }}
          >
            <tbody>
              {metaRows.map(([k, v]) => (
                <tr key={k} style={{ borderBottom: '1px solid var(--border)' }}>
                  <td
                    className="mono dim"
                    style={{
                      padding: '4px 8px 4px 0',
                      width: '32%',
                      verticalAlign: 'top',
                      fontSize: 11.5,
                    }}
                  >
                    {k}
                  </td>
                  <td
                    className="mono"
                    style={{
                      padding: '4px 0',
                      wordBreak: 'break-all',
                      whiteSpace: 'pre-wrap',
                      color: 'var(--ink)',
                    }}
                  >
                    {v}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>

          {hit.session_id && (
            <>
              <SectionLabel>Basin context</SectionLabel>
              {basinsQuery.isLoading ? (
                <div className="dim mono" style={{ fontSize: 12 }}>
                  loading basins…
                </div>
              ) : owningBasin && owningBasin.length > 0 ? (
                <div
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 6,
                    marginBottom: 6,
                  }}
                >
                  <div className="dim mono" style={{ fontSize: 11 }}>
                    {owningBasin.length} basin{owningBasin.length === 1 ? '' : 's'} touch this
                    fragment's session — the substrate doesn't expose per-fragment basin id, so
                    these are candidates ranked by mass.
                  </div>
                  {owningBasin
                    .slice()
                    .sort((a, b) => b.mass - a.mass)
                    .slice(0, 4)
                    .map((b) => (
                      <div
                        key={b.id}
                        style={{
                          display: 'flex',
                          justifyContent: 'space-between',
                          padding: '6px 8px',
                          border: '1px solid var(--border)',
                          borderRadius: 6,
                          fontSize: 12,
                        }}
                      >
                        <div>
                          <span className="mono dim" style={{ fontSize: 11 }}>
                            {b.source}:
                          </span>{' '}
                          <span style={{ color: 'var(--ink)' }}>{b.label || b.id}</span>
                        </div>
                        <div className="mono dim" style={{ fontSize: 11 }}>
                          mass {b.mass.toFixed(1)}
                        </div>
                      </div>
                    ))}
                </div>
              ) : (
                <div className="dim mono" style={{ fontSize: 12 }}>
                  No basins recorded for this session yet — the
                  consolidation worker may still be processing.
                </div>
              )}
            </>
          )}
        </div>

        {/* Footer — esc hint. */}
        <div
          style={{
            padding: '8px 14px',
            borderTop: '1px solid var(--border)',
            fontSize: 11,
            color: 'var(--ink-faint)',
            display: 'flex',
            justifyContent: 'space-between',
          }}
        >
          <span>Read-only — promote / discard live on tickets #4 + #5</span>
          <span className="mono">Esc to close</span>
        </div>
      </div>
    </div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div
      className="mono dim"
      style={{
        fontSize: 10.5,
        textTransform: 'uppercase',
        marginBottom: 6,
        letterSpacing: 0.4,
      }}
    >
      {children}
    </div>
  );
}

function ScoreTile({ label, value }: { label: string; value: string }) {
  return (
    <div
      style={{
        padding: '8px 10px',
        background: 'var(--bg-soft)',
        border: '1px solid var(--border)',
        borderRadius: 6,
      }}
    >
      <div className="mono dim" style={{ fontSize: 10.5 }}>
        {label}
      </div>
      <div className="mono" style={{ fontSize: 16, color: 'var(--ink)' }}>
        {value}
      </div>
    </div>
  );
}
