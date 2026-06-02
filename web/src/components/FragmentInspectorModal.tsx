import { useEffect, useMemo, useState } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';

import { Icon, KindBadge, ProjBadge, SessionPill } from '@/components/atoms';
import { api, ApiError } from '@/lib/api';
import type { RetrieveHit } from '@/lib/types';

/**
 * Fragment inspector (Tickets #3 + #4 from the coverage epic).
 *
 * Search rows render the snippet + meta-row but truncate aggressively
 * and only show kind + ts. When the user needs the WHY ("which exact
 * fragment matched? what other metadata does it carry? which basin?"),
 * they click → this modal opens with the full payload.
 *
 * Ticket #4 adds bounded mutation: importance is editable inline, and
 * a confirm-gated discard button removes the fragment (soft by default).
 * Content edits are intentionally NOT exposed yet because BE `update`
 * doesn't re-embed on content change — semantic re-anchoring would
 * silently break and the FE would have no way to surface it.
 */
export function FragmentInspectorModal({
  hit,
  onClose,
}: {
  hit: RetrieveHit | null;
  onClose: () => void;
}) {
  const queryClient = useQueryClient();
  // Local edit state — diff against `hit.importance` to know when to
  // enable the Save button. Reset whenever a new hit is opened.
  const [importanceDraft, setImportanceDraft] = useState<number | null>(null);
  const [confirmingDiscard, setConfirmingDiscard] = useState(false);
  const [discardReason, setDiscardReason] = useState('');
  const [mutationError, setMutationError] = useState<string | null>(null);

  useEffect(() => {
    setImportanceDraft(hit?.importance ?? null);
    setConfirmingDiscard(false);
    setDiscardReason('');
    setMutationError(null);
  }, [hit?.id, hit?.importance]);
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

  // Mutations are declared unconditionally so React's hook order is
  // stable across the early-return below. The `enabled`/guard logic
  // lives in the buttons' onClick handlers, not here.
  const updateMutation = useMutation({
    mutationFn: (importance: number) =>
      api.updateFragment({
        attractor_id: hit?.id ?? '',
        session_id: hit?.session_id ?? '',
        importance,
      }),
    onSuccess: () => {
      // Search results are stale after a successful update; let any
      // open queries re-fetch on next access.
      queryClient.invalidateQueries({ queryKey: ['retrieve'] });
    },
    onError: (err) =>
      setMutationError(err instanceof ApiError ? err.message : String(err)),
  });

  const discardMutation = useMutation({
    mutationFn: () =>
      api.discardFragment({
        attractor_id: hit?.id ?? '',
        session_id: hit?.session_id ?? '',
        soft_delete: true,
        reason: discardReason || undefined,
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['retrieve'] });
      onClose();
    },
    onError: (err) =>
      setMutationError(err instanceof ApiError ? err.message : String(err)),
  });

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
            <ImportanceEditor
              current={hit.importance}
              draft={importanceDraft ?? hit.importance}
              setDraft={setImportanceDraft}
              dirty={
                importanceDraft !== null &&
                Math.abs(importanceDraft - hit.importance) > 0.001
              }
              saving={updateMutation.isPending}
              onSave={() => {
                if (importanceDraft === null) return;
                setMutationError(null);
                updateMutation.mutate(importanceDraft);
              }}
              disabled={!hit.session_id}
            />
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

        {/* Footer — actions + error surface + esc hint. */}
        <div
          style={{
            padding: '10px 14px',
            borderTop: '1px solid var(--border)',
            display: 'flex',
            flexDirection: 'column',
            gap: 8,
          }}
        >
          {mutationError && (
            <div
              className="mono"
              style={{
                fontSize: 11.5,
                color: 'var(--urg-now, #c33)',
                padding: '6px 8px',
                background: 'rgba(220, 50, 50, 0.08)',
                borderRadius: 4,
                border: '1px solid rgba(220, 50, 50, 0.25)',
              }}
            >
              {mutationError}
            </div>
          )}
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              gap: 8,
              flexWrap: 'wrap',
            }}
          >
            {confirmingDiscard ? (
              <DiscardConfirm
                reason={discardReason}
                setReason={setDiscardReason}
                onCancel={() => {
                  setConfirmingDiscard(false);
                  setDiscardReason('');
                  setMutationError(null);
                }}
                onConfirm={() => {
                  setMutationError(null);
                  discardMutation.mutate();
                }}
                pending={discardMutation.isPending}
              />
            ) : (
              <button
                className="btn btn-ghost sm"
                onClick={() => setConfirmingDiscard(true)}
                type="button"
                disabled={!hit.session_id}
                title={
                  hit.session_id
                    ? 'Soft-delete this fragment (recoverable from WAL)'
                    : 'No session_id — discard requires ownership check'
                }
                style={{ color: 'var(--urg-now, #c33)' }}
              >
                <Icon.X /> Discard
              </button>
            )}
            <span className="mono" style={{ fontSize: 11, color: 'var(--ink-faint)' }}>
              Esc to close
            </span>
          </div>
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

/**
 * Importance editor — clamped [0, 1] number input + Save button that
 * activates only when the draft diverges from `current`. Saving runs
 * the parent's `onSave` callback; the parent owns the mutation and
 * loading state so this stays presentational.
 */
function ImportanceEditor({
  current,
  draft,
  setDraft,
  dirty,
  saving,
  onSave,
  disabled,
}: {
  current: number;
  draft: number;
  setDraft: (n: number) => void;
  dirty: boolean;
  saving: boolean;
  onSave: () => void;
  disabled: boolean;
}) {
  return (
    <div
      style={{
        padding: '8px 10px',
        background: 'var(--bg-soft)',
        border: `1px solid ${dirty ? 'var(--accent)' : 'var(--border)'}`,
        borderRadius: 6,
        display: 'flex',
        flexDirection: 'column',
        gap: 4,
      }}
    >
      <div
        style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'baseline' }}
      >
        <div className="mono dim" style={{ fontSize: 10.5 }}>
          importance
        </div>
        {dirty && (
          <div className="mono dim" style={{ fontSize: 9.5 }}>
            was {current.toFixed(3)}
          </div>
        )}
      </div>
      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        <input
          type="number"
          min={0}
          max={1}
          step={0.05}
          value={draft.toFixed(3)}
          onChange={(e) => {
            const v = parseFloat(e.target.value);
            if (Number.isNaN(v)) return;
            setDraft(Math.max(0, Math.min(1, v)));
          }}
          disabled={disabled || saving}
          className="mono"
          style={{
            width: '100%',
            fontSize: 14,
            background: 'transparent',
            border: '1px solid var(--border)',
            borderRadius: 4,
            padding: '2px 6px',
            color: 'var(--ink)',
          }}
        />
        <button
          className="btn sm"
          type="button"
          onClick={onSave}
          disabled={!dirty || saving || disabled}
          title={
            disabled
              ? 'Fragment has no session_id — cannot mutate'
              : !dirty
                ? 'No changes to save'
                : 'Save new importance value'
          }
        >
          {saving ? '…' : 'Save'}
        </button>
      </div>
    </div>
  );
}

/**
 * Discard confirmation inline-row. Replaces the Discard button while
 * the user fills in a reason and clicks Confirm. Cancel restores the
 * original button state.
 */
function DiscardConfirm({
  reason,
  setReason,
  onCancel,
  onConfirm,
  pending,
}: {
  reason: string;
  setReason: (s: string) => void;
  onCancel: () => void;
  onConfirm: () => void;
  pending: boolean;
}) {
  return (
    <div
      style={{
        display: 'flex',
        gap: 6,
        alignItems: 'center',
        flexWrap: 'wrap',
        flex: 1,
      }}
    >
      <input
        type="text"
        placeholder="reason (optional)"
        value={reason}
        onChange={(e) => setReason(e.target.value)}
        autoFocus
        disabled={pending}
        className="mono"
        style={{
          fontSize: 12,
          background: 'transparent',
          border: '1px solid var(--border)',
          borderRadius: 4,
          padding: '4px 8px',
          color: 'var(--ink)',
          flex: 1,
          minWidth: 200,
        }}
      />
      <button
        className="btn sm"
        type="button"
        onClick={onConfirm}
        disabled={pending}
        style={{
          background: 'var(--urg-now, #c33)',
          color: 'white',
          borderColor: 'var(--urg-now, #c33)',
        }}
        title="Soft-delete this fragment (recoverable from WAL)"
      >
        {pending ? '…' : 'Confirm discard'}
      </button>
      <button
        className="btn btn-ghost sm"
        type="button"
        onClick={onCancel}
        disabled={pending}
      >
        Cancel
      </button>
    </div>
  );
}
