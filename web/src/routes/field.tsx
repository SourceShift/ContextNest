import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import { BrandMark, Icon } from '@/components/atoms';
import { useFieldData } from '@/hooks/useFieldData';
import { useSessions } from '@/hooks/useSessions';
import type { FieldFragment } from '@/hooks/useFieldData';
import { api } from '@/lib/api';
import { cosineSimilarity } from '@/lib/pca';
import type { BasinSummary, RetrieveHit, SessionListItem } from '@/lib/types';

export const Route = createFileRoute('/field')({
  component: FieldPage,
  validateSearch: (search: Record<string, unknown>) => ({
    session: typeof search.session === 'string' ? search.session : undefined,
    project: typeof search.project === 'string' ? search.project : undefined,
  }),
});

// =============================================================================
// Visual constants
// =============================================================================

const KIND_COLOR: Record<string, string> = {
  learning: '#a78bfa',
  decision: '#ffd166',
  accomplishment: '#00d4aa',
  fact: '#a1a1aa',
  blocker: '#ff6b6b',
  todo: '#60a5fa',
  goal_phase: '#f472b6',
  user_action: '#ff6b6b',
  state: '#cbd5e1',
  current_task: '#fbbf24',
  summary: '#c4b5fd',
  initial_prompt_window: '#94a3b8',
  session_title: '#a1a1aa',
  ack: '#71717a',
  unknown: '#475569',
};

const VB_W = 1000;
const VB_H = 600;
const VB_MARGIN = 60;

// Pan/zoom state.
type ViewState = { scale: number; tx: number; ty: number };
const ZOOM_MIN = 0.4;
const ZOOM_MAX = 6;
const ZOOM_STEP = 1.4;
const IDENTITY_VIEW: ViewState = { scale: 1, tx: 0, ty: 0 };

function clientToDesign(
  evt: { clientX: number; clientY: number },
  rect: DOMRect,
  view: ViewState,
): { x: number; y: number } {
  const aspect = VB_W / VB_H;
  const containerAspect = rect.width / rect.height;
  let innerW = rect.width;
  let innerH = rect.height;
  let offX = 0;
  let offY = 0;
  if (containerAspect > aspect) {
    innerW = rect.height * aspect;
    offX = (rect.width - innerW) / 2;
  } else {
    innerH = rect.width / aspect;
    offY = (rect.height - innerH) / 2;
  }
  const fx = Math.max(0, Math.min(1, (evt.clientX - rect.left - offX) / innerW));
  const fy = Math.max(0, Math.min(1, (evt.clientY - rect.top - offY) / innerH));
  return {
    x: view.tx + fx * (VB_W / view.scale),
    y: view.ty + fy * (VB_H / view.scale),
  };
}

function clampView(view: ViewState): ViewState {
  const scale = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, view.scale));
  const vbW = VB_W / scale;
  const vbH = VB_H / scale;
  return {
    scale,
    tx: Math.max(-vbW / 2, Math.min(VB_W - vbW / 2, view.tx)),
    ty: Math.max(-vbH / 2, Math.min(VB_H - vbH / 2, view.ty)),
  };
}

// =============================================================================
// Page
// =============================================================================

function FieldPage() {
  const search = Route.useSearch();
  const navigate = Route.useNavigate();
  const focusedSession = search.session ?? undefined;
  const focusedProject = search.project ?? undefined;

  const field = useFieldData({
    sessionId: focusedSession,
    project: focusedProject,
  });
  const sessionsHook = useSessions();
  const svgRef = useRef<SVGSVGElement>(null);

  // === filter helpers ===
  // Project options come from /api/v1/field/basins (sorted by mass).
  // Session options narrow to the chosen project when one is set,
  // otherwise show every session known to the substrate.
  const projectOptions = useMemo(() => {
    return field.data.basins.map((b) => ({ label: b.label, mass: b.mass }));
  }, [field.data.basins]);

  const sessionOptions = useMemo(() => {
    const all = sessionsHook.data;
    if (!focusedProject) return all;
    return all.filter((s) => {
      const cwd = s.project_cwd ?? '';
      const last = cwd.trim().replace(/\/+$/, '').split('/').pop();
      return last === focusedProject;
    });
  }, [sessionsHook.data, focusedProject]);

  // Query-overlay state. Declared HERE (before the debounce useEffect
  // and the dependent hooks below) because `const` bindings are subject
  // to TDZ — referencing `queryText` / `queryDebounced` from a hook's
  // dep array fires synchronously during render, so the bindings must
  // already exist by then. Moving the inputRef alongside keeps related
  // state co-located.
  // UX shape: the user types something (typically the prompt they're
  // about to send to Claude Code) and /field becomes a context picker,
  // highlighting fragments most relevant to the query and dimming the
  // rest hard.
  const [queryText, setQueryText] = useState('');
  const [queryDebounced, setQueryDebounced] = useState('');
  const queryInputRef = useRef<HTMLInputElement>(null);

  // Debounce the typed query so we don't fire /retrieve on every
  // keystroke. 350ms is faster than search.tsx's 250ms because the
  // user typically pastes here rather than types — but a bit longer
  // than zero so paste→edit→paste sequences only fire once.
  useEffect(() => {
    const t = setTimeout(() => setQueryDebounced(queryText), 350);
    return () => clearTimeout(t);
  }, [queryText]);

  // Send the query against the same scope the user has filtered to.
  // Single-session mode if a session is picked; otherwise omit session
  // filters and let the backend search globally from its own session index.
  const querySessionIds = useMemo(() => {
    if (focusedSession) return [focusedSession];
    if (focusedProject) return sessionOptions.map((s) => s.id);
    return [];
  }, [focusedProject, focusedSession, sessionOptions]);

  const queryActive = queryDebounced.trim().length >= 2;
  const queryResults = useQuery({
    queryKey: ['field-query', queryDebounced, querySessionIds, focusedSession],
    enabled: queryActive && (!focusedProject || querySessionIds.length > 0),
    staleTime: 10_000,
    queryFn: async () => {
      // top_k=40 because /field shows a lot of fragments at once; the
      // user wants enough hits to see a cluster, not just the top 5.
      return api.retrieve({
        query: queryDebounced,
        top_k: 40,
        ...(focusedSession
          ? { session_id: focusedSession }
          : focusedProject
            ? { session_ids: querySessionIds }
            : {}),
      });
    },
  });

  const queryHitIds = useMemo(() => {
    const m = new Map<string, number>();
    for (const h of queryResults.data?.hits ?? []) {
      m.set(h.id, h.similarity);
    }
    return m;
  }, [queryResults.data]);

  const setProjectFilter = useCallback(
    (project: string | undefined) => {
      // Changing project also clears session filter (the session
      // might not belong to the new project, which would render an
      // empty field).
      void navigate({
        to: '/field',
        search: { project, session: undefined },
      });
    },
    [navigate],
  );

  const setSessionFilter = useCallback(
    (session: string | undefined) => {
      void navigate({
        to: '/field',
        search: { project: focusedProject, session },
      });
    },
    [navigate, focusedProject],
  );

  // Interaction state.
  const [selectedFragment, setSelectedFragment] = useState<string | null>(null);
  const [selectedBasin, setSelectedBasin] = useState<string | null>(null);
  const [hovered, setHovered] = useState<{
    fragmentId: string;
    x: number;
    y: number;
  } | null>(null);
  const [maxAgeDays, setMaxAgeDays] = useState(30);
  const [scrubberMinutesAgo, setScrubberMinutesAgo] = useState<number | null>(
    null,
  );
  const [disabledKinds, setDisabledKinds] = useState<Set<string>>(new Set());
  // `queryText` / `queryDebounced` / `queryInputRef` are declared near
  // the top of the component (before the debounce useEffect) — see
  // the TDZ note there.
  const [view, setView] = useState<ViewState>(IDENTITY_VIEW);
  const dragRef = useRef<{
    startView: ViewState;
    startClient: { x: number; y: number };
    rect: DOMRect;
  } | null>(null);

  // Pulse only fragments whose stored timestamp is newer than the
  // newest one we have ever seen. This is the SUBSTRATE-growth signal,
  // distinct from "the visible set changed" (which happens on initial
  // mount, filter changes, refetch — none of which should pulse the
  // whole field).
  //
  // The previous implementation diffed against a `seenIdsRef` Set
  // that started empty, so the first render treated every fragment
  // as fresh and pulsed all of them at once. Same bug fired when the
  // folder/session filter changed: the new visible set is "new" to
  // the ref but the user just toggled a filter — nothing about the
  // substrate actually grew.
  //
  // `metadata.ts` is the only honest "when this fragment was stored"
  // signal we have. Tracking the high-water mark gives us:
  //   - first load                : record baseline, no pulse
  //   - filter change              : same baseline, only items newer
  //                                   than it pulse (typically zero)
  //   - new substrate write       : items newer than baseline pulse,
  //                                   baseline advances
  const lastNewestTsRef = useRef<string | null>(null);
  const [pulsingIds, setPulsingIds] = useState<Set<string>>(new Set());
  useEffect(() => {
    if (field.data.fragments.length === 0) return;

    // Compute the newest ts in this batch up-front.
    let batchNewest = '';
    for (const f of field.data.fragments) {
      const ts = typeof f.metadata.ts === 'string' ? (f.metadata.ts as string) : '';
      if (ts > batchNewest) batchNewest = ts;
    }

    // First poll: just set the baseline. No pulse — the user just
    // opened the view; everything they see is incumbent, not new.
    if (lastNewestTsRef.current === null) {
      lastNewestTsRef.current = batchNewest;
      return;
    }

    // Subsequent polls: pulse only items strictly newer than the
    // baseline. Lexicographic compare works for ISO-8601 ts.
    const baseline = lastNewestTsRef.current;
    const fresh: string[] = [];
    for (const f of field.data.fragments) {
      const ts = typeof f.metadata.ts === 'string' ? (f.metadata.ts as string) : '';
      if (ts && ts > baseline) fresh.push(f.id);
    }
    // Advance the baseline regardless (so a filter swap doesn't make
    // the next poll re-pulse the same items).
    lastNewestTsRef.current = batchNewest > baseline ? batchNewest : baseline;

    if (fresh.length > 0) {
      setPulsingIds((prev) => {
        const merged = new Set(prev);
        for (const id of fresh) merged.add(id);
        return merged;
      });
      const t = window.setTimeout(() => {
        setPulsingIds((prev) => {
          const next2 = new Set(prev);
          for (const id of fresh) next2.delete(id);
          return next2;
        });
      }, 2000);
      return () => window.clearTimeout(t);
    }
  }, [field.data.fragments]);

  // ===== layout — embedding-space PCA mapped to canvas pixels =====
  const { fragmentXY, basinXY } = useMemo(() => {
    const xyRaw = field.data.layout.xy;
    if (xyRaw.length === 0) return { fragmentXY: {}, basinXY: {} };
    let minX = Infinity;
    let maxX = -Infinity;
    let minY = Infinity;
    let maxY = -Infinity;
    for (const p of xyRaw) {
      if (p.x < minX) minX = p.x;
      if (p.x > maxX) maxX = p.x;
      if (p.y < minY) minY = p.y;
      if (p.y > maxY) maxY = p.y;
    }
    // Guard zero-extent.
    const dx = Math.max(maxX - minX, 1e-6);
    const dy = Math.max(maxY - minY, 1e-6);
    const sx = (VB_W - VB_MARGIN * 2) / dx;
    const sy = (VB_H - VB_MARGIN * 2) / dy;
    // Preserve aspect ratio so semantic distance isn't stretched on one axis.
    const s = Math.min(sx, sy);
    const projectedW = dx * s;
    const projectedH = dy * s;
    const offsetX = (VB_W - projectedW) / 2 - minX * s;
    const offsetY = (VB_H - projectedH) / 2 - minY * s;

    const fXY: Record<string, { x: number; y: number }> = {};
    field.data.fragments.forEach((f, i) => {
      const p = xyRaw[i];
      fXY[f.id] = {
        x: p.x * s + offsetX,
        y: p.y * s + offsetY,
      };
    });

    // Basin position = mean of its member fragment positions.
    const bXY: Record<string, { x: number; y: number; count: number }> = {};
    for (const f of field.data.fragments) {
      const proj = f.project;
      const p = fXY[f.id];
      if (!p) continue;
      const acc = bXY[proj] ?? { x: 0, y: 0, count: 0 };
      acc.x += p.x;
      acc.y += p.y;
      acc.count += 1;
      bXY[proj] = acc;
    }
    const basinPositions: Record<string, { x: number; y: number }> = {};
    for (const [proj, acc] of Object.entries(bXY)) {
      basinPositions[proj] = { x: acc.x / acc.count, y: acc.y / acc.count };
    }
    return { fragmentXY: fXY, basinXY: basinPositions };
  }, [field.data]);

  // Compute the active fragment set after applying age filter + scrubber.
  const visibleFragments = useMemo(() => {
    const cutoffMs = scrubberMinutesAgo == null
      ? null
      : Date.now() - scrubberMinutesAgo * 60_000;
    return field.data.fragments.filter((f) => {
      if (disabledKinds.has(f.metadata.kind ?? 'unknown')) return false;
      if (f.ageDays != null && f.ageDays > maxAgeDays) return false;
      if (cutoffMs != null) {
        const ts =
          typeof f.metadata.ts === 'string'
            ? Date.parse(f.metadata.ts as string)
            : NaN;
        // If filtering by scrubber, drop fragments newer than cutoff.
        if (!Number.isNaN(ts) && ts > cutoffMs) return false;
      }
      return true;
    });
  }, [field.data.fragments, disabledKinds, maxAgeDays, scrubberMinutesAgo]);
  const visibleIds = useMemo(
    () => new Set(visibleFragments.map((f) => f.id)),
    [visibleFragments],
  );

  // ===== nearest neighbors for the selected/hovered fragment =====
  const neighborIds = useMemo<Set<string>>(() => {
    const focus = selectedFragment ?? hovered?.fragmentId ?? null;
    if (!focus) return new Set();
    const center = field.data.fragments.find((f) => f.id === focus);
    if (!center || !center.embedding || center.embedding.length === 0) {
      return new Set();
    }
    const scores: Array<{ id: string; sim: number }> = [];
    for (const f of field.data.fragments) {
      if (f.id === center.id) continue;
      if (!f.embedding || f.embedding.length === 0) continue;
      scores.push({ id: f.id, sim: cosineSimilarity(center.embedding, f.embedding) });
    }
    scores.sort((a, b) => b.sim - a.sim);
    return new Set(scores.slice(0, 5).map((s) => s.id));
  }, [selectedFragment, hovered, field.data.fragments]);

  // ===== zoom controls =====
  const zoomBy = useCallback(
    (factor: number, anchor?: { x: number; y: number }) => {
      setView((prev) => {
        const newScale = Math.max(ZOOM_MIN, Math.min(ZOOM_MAX, prev.scale * factor));
        if (newScale === prev.scale) return prev;
        const ax = anchor?.x ?? prev.tx + VB_W / prev.scale / 2;
        const ay = anchor?.y ?? prev.ty + VB_H / prev.scale / 2;
        const fx = (ax - prev.tx) / (VB_W / prev.scale);
        const fy = (ay - prev.ty) / (VB_H / prev.scale);
        return clampView({
          scale: newScale,
          tx: ax - fx * (VB_W / newScale),
          ty: ay - fy * (VB_H / newScale),
        });
      });
    },
    [],
  );
  const resetView = useCallback(() => setView(IDENTITY_VIEW), []);

  const handleWheel = useCallback(
    (e: React.WheelEvent<SVGSVGElement>) => {
      e.preventDefault();
      const rect = svgRef.current?.getBoundingClientRect();
      if (!rect) return;
      const anchor = clientToDesign(e, rect, view);
      const factor = Math.pow(1.0015, -e.deltaY);
      zoomBy(factor, anchor);
    },
    [view, zoomBy],
  );

  const handleMouseDown = useCallback(
    (e: React.MouseEvent<SVGSVGElement>) => {
      if (e.button !== 0) return;
      const target = e.target as Element;
      // Don't start panning when clicking a fragment or basin marker.
      if (target.closest('[data-frag]')) return;
      if (target.closest('[data-basin]')) return;
      const rect = svgRef.current?.getBoundingClientRect();
      if (!rect) return;
      dragRef.current = {
        startView: view,
        startClient: { x: e.clientX, y: e.clientY },
        rect,
      };
    },
    [view],
  );

  useEffect(() => {
    const onMove = (e: MouseEvent) => {
      if (!dragRef.current) return;
      const { startView, startClient, rect } = dragRef.current;
      const dx = e.clientX - startClient.x;
      const dy = e.clientY - startClient.y;
      const aspect = VB_W / VB_H;
      const containerAspect = rect.width / rect.height;
      const innerW =
        containerAspect > aspect ? rect.height * aspect : rect.width;
      const innerH =
        containerAspect > aspect ? rect.height : rect.width / aspect;
      const vbW = VB_W / startView.scale;
      const vbH = VB_H / startView.scale;
      setView(
        clampView({
          scale: startView.scale,
          tx: startView.tx - (dx / innerW) * vbW,
          ty: startView.ty - (dy / innerH) * vbH,
        }),
      );
    };
    const onUp = () => {
      dragRef.current = null;
    };
    window.addEventListener('mousemove', onMove);
    window.addEventListener('mouseup', onUp);
    return () => {
      window.removeEventListener('mousemove', onMove);
      window.removeEventListener('mouseup', onUp);
    };
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as Element | null;
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return;
      if (e.key === '+' || e.key === '=') {
        e.preventDefault();
        zoomBy(ZOOM_STEP);
      } else if (e.key === '-' || e.key === '_') {
        e.preventDefault();
        zoomBy(1 / ZOOM_STEP);
      } else if (e.key === '0') {
        e.preventDefault();
        resetView();
      } else if (e.key === 'Escape') {
        setSelectedFragment(null);
        setSelectedBasin(null);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [zoomBy, resetView]);

  // ===== focus mode controls =====
  const exitFocus = useCallback(() => {
    void navigate({
      to: '/field',
      search: { session: undefined, project: undefined },
    });
  }, [navigate]);

  // ===== derived for header / side =====
  const visibleCount = visibleFragments.length;
  const selectedFragmentObj = selectedFragment
    ? field.data.fragments.find((f) => f.id === selectedFragment) ?? null
    : null;
  const selectedBasinObj = selectedBasin
    ? field.data.basins.find((b) => b.id === selectedBasin) ?? null
    : null;

  // Edge filter: at most 250 strongest connections to keep render snappy.
  const visibleEdges = useMemo(() => {
    return field.data.connections
      .filter((e) => visibleIds.has(e.source) && visibleIds.has(e.target))
      .slice(0, 250);
  }, [field.data.connections, visibleIds]);

  const hasEmbeddings = field.data.fragments.some(
    (f) => f.embedding && f.embedding.length > 0,
  );

  const fragmentOpacity = (f: FieldFragment): number => {
    if (!visibleIds.has(f.id)) return 0.05;
    // Query-overlay mode takes priority over every other dimming
    // logic — its whole purpose is to make the answer to "what's
    // relevant to this query" visually impossible to miss.
    if (queryActive) {
      const sim = queryHitIds.get(f.id);
      if (sim == null) return 0.05;
      // Top hits at full opacity; weaker hits still legible.
      return Math.max(0.55, Math.min(1, sim * 1.25 + 0.3));
    }
    // Highlight states take priority over decay.
    if (selectedFragment && f.id === selectedFragment) return 1;
    if (selectedFragment && neighborIds.has(f.id)) return 1;
    if (selectedFragment) return 0.18;
    if (hovered && hovered.fragmentId === f.id) return 1;
    if (hovered && neighborIds.has(f.id)) return 0.95;
    if (selectedBasin && f.project !== selectedBasinObj?.label) return 0.18;
    const ageFade =
      f.ageDays == null ? 0.7 : Math.max(0.35, 1 - f.ageDays / 30);
    return ageFade;
  };

  // Radius bump for fragments that match the active query — makes
  // the matched cluster pop visually without needing to read the
  // sidebar list.
  const fragmentRadius = (f: FieldFragment): number => {
    if (queryActive) {
      const sim = queryHitIds.get(f.id);
      if (sim == null) return 2.2;
      // Linear ramp: a top-similarity hit is ~6px, weakest ~4px.
      return 4 + Math.max(0, Math.min(1, sim)) * 2.5;
    }
    if (selectedFragment === f.id) return 6;
    if (neighborIds.has(f.id)) return 4.5;
    return 3;
  };

  return (
    <div>
      <div className="page-header">
        <div>
          <h1 className="page-title">Field</h1>
          <div className="page-sub">
            {hasEmbeddings ? (
              <>
                Semantic space · top-2 PCA of fragment embeddings ·{' '}
                <span className="mono">
                  {(field.data.layout.varianceRatio * 100).toFixed(0)}%
                </span>{' '}
                variance captured ·{' '}
              </>
            ) : (
              <>Loading embeddings… · </>
            )}
            <span className="mono">{visibleCount}</span> /{' '}
            <span className="mono">{field.data.fragments.length}</span>{' '}
            fragments visible
            {field.totalFragments != null && (
              <>
                {' '}
                · <span className="mono">{field.totalFragments.toLocaleString()}</span>{' '}
                total in substrate
              </>
            )}
            {(focusedProject || focusedSession) && (
              <>
                {' '}
                · focus:{' '}
                {focusedProject && (
                  <span className="mono">project={focusedProject}</span>
                )}
                {focusedProject && focusedSession && ' '}
                {focusedSession && (
                  <span className="mono">session={focusedSession}</span>
                )}{' '}
                <button
                  className="btn btn-ghost sm"
                  onClick={exitFocus}
                  type="button"
                  style={{ marginLeft: 4 }}
                >
                  <Icon.X /> clear all
                </button>
              </>
            )}
          </div>
        </div>
        <div className="page-actions">
          <button
            className="btn"
            onClick={() => field.refetch()}
            type="button"
            title="Re-sample fragments and refresh layout"
          >
            <Icon.Refresh /> Re-sample
          </button>
        </div>
      </div>

      {/* Filter bar — project (folder) + optional session */}
      <div
        className="filter-bar"
        style={{ marginBottom: 14, gap: 12, alignItems: 'center' }}
      >
        <label
          className="mono dim"
          style={{
            fontSize: 10,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
          }}
        >
          folder
        </label>
        <select
          value={focusedProject ?? ''}
          onChange={(e) =>
            setProjectFilter(e.target.value || undefined)
          }
          className="mono"
          style={{
            background: 'var(--surface-2)',
            color: 'var(--ink)',
            border: '1px solid var(--border)',
            borderRadius: 6,
            padding: '5px 10px',
            fontSize: 12,
            minWidth: 220,
          }}
        >
          <option value="">all folders ({projectOptions.length})</option>
          {projectOptions.map((p) => (
            <option key={p.label} value={p.label}>
              {p.label} · {p.mass}
            </option>
          ))}
        </select>

        <label
          className="mono dim"
          style={{
            fontSize: 10,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
            marginLeft: 8,
          }}
        >
          session <span style={{ textTransform: 'none' }}>(optional)</span>
        </label>
        <SessionPicker
          sessions={sessionOptions}
          value={focusedSession}
          onChange={setSessionFilter}
          projectLabel={focusedProject}
        />

        {(focusedProject || focusedSession) && (
          <button
            className="btn btn-ghost sm"
            onClick={exitFocus}
            type="button"
            title="clear all filters"
          >
            <Icon.X /> reset
          </button>
        )}

        <div className="grow" />
        <span className="mono dim" style={{ fontSize: 11 }}>
          {field.data.fragments.length} fragments rendered
          {field.truncated ? ' (truncated)' : ''}
        </span>
      </div>

      {/* Query-overlay bar — type or paste to find relevant past work.
          Lives between the filter row and the field viz so the input
          stays visible while the canvas reacts in real time. */}
      <div className="field-query-bar">
        <Icon.Search />
        <input
          ref={queryInputRef}
          type="text"
          value={queryText}
          onChange={(e) => setQueryText(e.target.value)}
          placeholder="paste your prompt to find relevant past work — e.g. 'context of the auth migration'"
          className="field-query-input"
          spellCheck={false}
          autoComplete="off"
        />
        {queryText && (
          <button
            type="button"
            className="btn btn-ghost sm"
            title="clear query"
            onClick={() => {
              setQueryText('');
              queryInputRef.current?.focus();
            }}
          >
            <Icon.X />
          </button>
        )}
        <span className="mono dim field-query-status">
          {queryActive
            ? queryResults.isLoading
              ? 'searching…'
              : `${queryHitIds.size} relevant · scope ${
                  focusedSession
                    ? '1 session'
                    : `${querySessionIds.length} sessions`
                }`
            : 'type to find context · 2+ characters'}
        </span>
      </div>

      {field.isLoading && field.data.fragments.length === 0 ? (
        <div className="empty with-card">
          <BrandMark size={36} dim />
          <div className="empty-title">Computing layout…</div>
          <div className="empty-body">
            Fetching embeddings and projecting them into 2D. Takes a few seconds the first
            time.
          </div>
        </div>
      ) : field.data.fragments.length === 0 ? (
        <div className="empty with-card">
          <BrandMark size={36} dim />
          <div className="empty-title">No fragments in this view</div>
          <div className="empty-body">
            {focusedSession ? (
              <>
                Session <span className="mono">{focusedSession}</span> has no fragments with
                embeddings yet. <button
                  className="btn-ghost sm"
                  onClick={exitFocus}
                  type="button"
                >
                  clear filter
                </button>
              </>
            ) : (
              <>
                Run <span className="mono">make cn-ingest</span> or wait for live cc_hooks
                to populate the substrate.
              </>
            )}
          </div>
        </div>
      ) : (
        <div className="field-layout">
          <div className="field-canvas-column">
          <div className="field-canvas-wrap">
            <svg
              ref={svgRef}
              className="field-svg"
              viewBox={`${view.tx} ${view.ty} ${VB_W / view.scale} ${
                VB_H / view.scale
              }`}
              preserveAspectRatio="xMidYMid meet"
              onClick={() => {
                setSelectedFragment(null);
                setSelectedBasin(null);
              }}
              onWheel={handleWheel}
              onMouseDown={handleMouseDown}
              style={{ cursor: dragRef.current ? 'grabbing' : 'grab' }}
            >
              <defs>
                {field.data.basins.map((b) => {
                  const dominantKind = Object.entries(b.by_kind).sort(
                    (a, c) => c[1] - a[1],
                  )[0]?.[0];
                  const color = dominantKind
                    ? KIND_COLOR[dominantKind] ?? KIND_COLOR.unknown
                    : KIND_COLOR.unknown;
                  return (
                    <radialGradient
                      key={b.id}
                      id={`basin-grad-${b.id}`}
                      cx="50%"
                      cy="50%"
                      r="50%"
                    >
                      <stop offset="0%" stopColor={color} stopOpacity="0.32" />
                      <stop offset="40%" stopColor={color} stopOpacity="0.12" />
                      <stop offset="100%" stopColor={color} stopOpacity="0" />
                    </radialGradient>
                  );
                })}
                <pattern
                  id="field-grid"
                  width="40"
                  height="40"
                  patternUnits="userSpaceOnUse"
                >
                  <path
                    d="M 40 0 L 0 0 0 40"
                    fill="none"
                    stroke="#1a1a1a"
                    strokeWidth="0.5"
                  />
                </pattern>
              </defs>

              <rect width={VB_W} height={VB_H} fill="url(#field-grid)" />

              {/* Basin halos at the centroid of each basin's fragments */}
              {field.data.basins.map((b) => {
                const p = basinXY[b.label];
                if (!p) return null;
                const r = 40 + Math.min(100, Math.sqrt(b.mass) * 4);
                return (
                  <circle
                    key={b.id}
                    cx={p.x}
                    cy={p.y}
                    r={r}
                    fill={`url(#basin-grad-${b.id})`}
                    style={{ pointerEvents: 'none' }}
                  />
                );
              })}

              {/* Real resonance edges (retrieve co-occurrence) */}
              {visibleEdges.map((e) => {
                const s = fragmentXY[e.source];
                const t = fragmentXY[e.target];
                if (!s || !t) return null;
                const w = Math.min(2, 0.4 + e.count * 0.2);
                const isFocusEdge =
                  selectedFragment != null &&
                  (e.source === selectedFragment || e.target === selectedFragment);
                return (
                  <line
                    key={`${e.source}-${e.target}`}
                    x1={s.x}
                    y1={s.y}
                    x2={t.x}
                    y2={t.y}
                    stroke={isFocusEdge ? '#00d4aa' : '#52525b'}
                    strokeOpacity={
                      selectedFragment ? (isFocusEdge ? 0.7 : 0.05) : 0.25
                    }
                    strokeWidth={w}
                  />
                );
              })}

              {/* Fragment dots */}
              {field.data.fragments.map((f) => {
                const p = fragmentXY[f.id];
                if (!p) return null;
                const kind = f.metadata.kind ?? 'unknown';
                const color = KIND_COLOR[kind] ?? KIND_COLOR.unknown;
                const pulsing = pulsingIds.has(f.id);
                return (
                  <g key={f.id} data-frag={f.id}>
                    {pulsing && (
                      <circle
                        cx={p.x}
                        cy={p.y}
                        r={14}
                        fill="none"
                        stroke={color}
                        strokeOpacity={0.8}
                        strokeWidth={1.5}
                      >
                        <animate
                          attributeName="r"
                          from="4"
                          to="22"
                          dur="1.6s"
                          repeatCount="1"
                        />
                        <animate
                          attributeName="stroke-opacity"
                          from="0.9"
                          to="0"
                          dur="1.6s"
                          repeatCount="1"
                        />
                      </circle>
                    )}
                    <circle
                      cx={p.x}
                      cy={p.y}
                      r={fragmentRadius(f)}
                      fill={color}
                      opacity={fragmentOpacity(f)}
                      stroke={
                        selectedFragment === f.id
                          ? '#ffffff'
                          : queryActive && queryHitIds.has(f.id)
                            ? 'var(--accent)'
                            : 'none'
                      }
                      strokeWidth={
                        selectedFragment === f.id
                          ? 1.5
                          : queryActive && queryHitIds.has(f.id)
                            ? 1.25
                            : 0
                      }
                      style={{
                        cursor: 'pointer',
                        transition: 'opacity 150ms ease, r 150ms ease',
                      }}
                      onMouseEnter={() =>
                        setHovered({ fragmentId: f.id, x: p.x, y: p.y })
                      }
                      onMouseLeave={() => setHovered(null)}
                      onClick={(ev) => {
                        ev.stopPropagation();
                        setSelectedFragment((s) =>
                          s === f.id ? null : f.id,
                        );
                        setSelectedBasin(null);
                      }}
                    />
                  </g>
                );
              })}

              {/* Basin centroid markers + labels */}
              {field.data.basins.map((b) => {
                const p = basinXY[b.label];
                if (!p) return null;
                const dominantKind = Object.entries(b.by_kind).sort(
                  (a, c) => c[1] - a[1],
                )[0]?.[0];
                const color = dominantKind
                  ? KIND_COLOR[dominantKind] ?? KIND_COLOR.unknown
                  : KIND_COLOR.unknown;
                const isSel = selectedBasin === b.id;
                return (
                  <g
                    key={b.id}
                    data-basin={b.id}
                    transform={`translate(${p.x},${p.y})`}
                    style={{
                      cursor: 'pointer',
                      opacity:
                        selectedBasin && !isSel
                          ? 0.4
                          : selectedFragment
                            ? 0.55
                            : 1,
                      transition: 'opacity 200ms ease',
                    }}
                    onClick={(ev) => {
                      ev.stopPropagation();
                      setSelectedBasin((s) => (s === b.id ? null : b.id));
                      setSelectedFragment(null);
                    }}
                  >
                    <circle
                      r={8}
                      fill="#0a0a0a"
                      stroke={color}
                      strokeWidth={1.5}
                      strokeOpacity={0.7}
                    />
                    <text
                      x={0}
                      y={-14}
                      textAnchor="middle"
                      fill={color}
                      fontFamily="JetBrains Mono, monospace"
                      fontSize={10.5}
                      letterSpacing={0.4}
                      style={{ pointerEvents: 'none' }}
                    >
                      {b.label} · {b.mass}
                    </text>
                  </g>
                );
              })}
            </svg>

            {hovered && (() => {
              const f = field.data.fragments.find((x) => x.id === hovered.fragmentId);
              if (!f) return null;
              return (
                <FieldTooltip
                  fragment={f}
                  xPct={(hovered.x / VB_W) * 100}
                  yPct={(hovered.y / VB_H) * 100}
                />
              );
            })()}

            <div className="field-overlay-tr">
              <div className="zoom-nav">
                <button
                  className="znav-btn"
                  onClick={() => zoomBy(ZOOM_STEP)}
                  title="zoom in (+ key or scroll up over canvas)"
                  type="button"
                  disabled={view.scale >= ZOOM_MAX - 0.001}
                >
                  +
                </button>
                <button
                  className="znav-btn"
                  onClick={() => zoomBy(1 / ZOOM_STEP)}
                  title="zoom out (− key or scroll down over canvas)"
                  type="button"
                  disabled={view.scale <= ZOOM_MIN + 0.001}
                >
                  −
                </button>
                <div className="znav-divider" />
                <button
                  className="znav-btn znav-reset"
                  onClick={resetView}
                  title="reset view (0)"
                  type="button"
                  disabled={
                    view.scale === 1 && view.tx === 0 && view.ty === 0
                  }
                >
                  <svg
                    width="11"
                    height="11"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="2"
                  >
                    <path d="M3 9V3h6M21 9V3h-6M3 15v6h6M21 15v6h-6" />
                  </svg>
                </button>
                <div className="znav-zoomlabel">
                  {Math.round(view.scale * 100)}%
                </div>
              </div>

              <div className="field-legend-panel">
                <div className="legend">
                  <div className="legend-row">
                    <span className="legend-label">filter by kind</span>
                    <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                      {field.data.kinds.map((k) => (
                        <button
                          key={k}
                          className="kind-swatch"
                          onClick={() =>
                            setDisabledKinds((prev) => {
                              const next = new Set(prev);
                              if (next.has(k)) next.delete(k);
                              else next.add(k);
                              return next;
                            })
                          }
                          title={`${k}${disabledKinds.has(k) ? ' (hidden)' : ''}`}
                          style={{
                            background: KIND_COLOR[k] ?? KIND_COLOR.unknown,
                            opacity: disabledKinds.has(k) ? 0.2 : 1,
                          }}
                        />
                      ))}
                    </div>
                  </div>
                  <div className="legend-row">
                    <span className="legend-label">axes</span>
                    <span className="legend-decay">
                      semantic similarity (PC1 ↔, PC2 ↕)
                    </span>
                  </div>
                </div>
              </div>
            </div>

            <div className="field-overlay-br">
              <div className="age-slider">
                <div className="age-label">
                  <span>decay window</span>
                  <span className="mono" style={{ color: 'var(--ink)' }}>
                    ≤ {maxAgeDays}d
                  </span>
                </div>
                <input
                  type="range"
                  min={1}
                  max={90}
                  value={maxAgeDays}
                  onChange={(e) => setMaxAgeDays(+e.target.value)}
                  className="field-slider"
                />
                <div className="age-marks">
                  <span>1d</span>
                  <span>30d</span>
                  <span>60d</span>
                  <span>90d</span>
                </div>
              </div>
            </div>

            </div>

            <TimelineScrubber
              minutesAgo={scrubberMinutesAgo}
              onChange={setScrubberMinutesAgo}
            />
          </div>

          <aside className="field-side">
            {queryActive ? (
              <QueryResultsPanel
                query={queryDebounced}
                hits={queryResults.data?.hits ?? []}
                isLoading={queryResults.isLoading}
                onSelectFragment={(id) => {
                  setSelectedFragment(id);
                  setSelectedBasin(null);
                }}
                onClear={() => {
                  setQueryText('');
                  queryInputRef.current?.focus();
                }}
              />
            ) : selectedFragmentObj ? (
              <FragmentDetail
                fragment={selectedFragmentObj}
                neighborIds={neighborIds}
                allFragments={field.data.fragments}
                onClose={() => setSelectedFragment(null)}
                onFocusSession={(sessionId) => setSessionFilter(sessionId)}
              />
            ) : selectedBasinObj ? (
              <BasinDetail
                basin={selectedBasinObj}
                onClose={() => setSelectedBasin(null)}
                onFocusSession={(sessionId) => setSessionFilter(sessionId)}
              />
            ) : (
              <ValuePanel
                hasEmbeddings={hasEmbeddings}
                varianceRatio={field.data.layout.varianceRatio}
                connectionsCount={field.data.connections.length}
                focusedSession={focusedSession}
              />
            )}
          </aside>
        </div>
      )}
    </div>
  );
}

// =============================================================================
// Timeline scrubber
// =============================================================================

// Stops at common-sense intervals: now, 5m, 15m, 1h, 6h, 24h, 7d. Mapped
// to the underlying minutes-ago value with a non-linear curve so the
// "now" end of the slider has more resolution than the deep-past end.
const SCRUBBER_STOPS: Array<{ label: string; minutesAgo: number | null }> = [
  { label: 'now', minutesAgo: null },
  { label: '5m', minutesAgo: 5 },
  { label: '15m', minutesAgo: 15 },
  { label: '1h', minutesAgo: 60 },
  { label: '6h', minutesAgo: 6 * 60 },
  { label: '24h', minutesAgo: 24 * 60 },
  { label: '7d', minutesAgo: 7 * 24 * 60 },
  { label: '30d', minutesAgo: 30 * 24 * 60 },
];

function TimelineScrubber({
  minutesAgo,
  onChange,
}: {
  minutesAgo: number | null;
  onChange: (val: number | null) => void;
}) {
  const idx = SCRUBBER_STOPS.findIndex(
    (s) =>
      (s.minutesAgo == null && minutesAgo == null) ||
      s.minutesAgo === minutesAgo,
  );
  return (
    <div className="field-scrubber">
      <div className="scrubber-label">
        <span className="mono dim" style={{ fontSize: 11 }}>
          Time scrubber — slide right to wind the substrate back
        </span>
        <span className="mono" style={{ fontSize: 11, color: 'var(--ink)' }}>
          {SCRUBBER_STOPS[idx >= 0 ? idx : 0].label}
        </span>
      </div>
      <input
        type="range"
        min={0}
        max={SCRUBBER_STOPS.length - 1}
        value={idx >= 0 ? idx : 0}
        onChange={(e) => onChange(SCRUBBER_STOPS[+e.target.value].minutesAgo)}
        className="field-slider"
      />
      <div className="scrubber-marks">
        {SCRUBBER_STOPS.map((s, i) => (
          <span key={i}>{s.label}</span>
        ))}
      </div>
    </div>
  );
}

// =============================================================================
// Sidebar variants
// =============================================================================

// Query-overlay side panel. Lists the ranked retrieve hits with a
// similarity badge and a "Copy IDs" affordance so the user can lift
// them out of /field and into a Claude prompt verbatim. Hovering
// individual rows focuses the corresponding fragment on the canvas.
function QueryResultsPanel({
  query,
  hits,
  isLoading,
  onSelectFragment,
  onClear,
}: {
  query: string;
  hits: RetrieveHit[];
  isLoading: boolean;
  onSelectFragment: (id: string) => void;
  onClear: () => void;
}) {
  const [copied, setCopied] = useState(false);

  const copyIds = useCallback(() => {
    if (hits.length === 0) return;
    const ids = hits.map((h) => h.id).join('\n');
    void navigator.clipboard.writeText(ids).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    });
  }, [hits]);

  return (
    <div className="field-side-detail field-query-panel">
      <div className="side-detail-header">
        <div
          className="mono dim"
          style={{
            fontSize: 10,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
          }}
        >
          context for query
        </div>
        <button
          type="button"
          className="btn btn-ghost sm"
          onClick={onClear}
          title="exit query mode"
        >
          <Icon.X />
        </button>
      </div>
      <div className="field-query-echo mono">{query}</div>

      <div className="field-query-toolbar">
        <span className="dim mono" style={{ fontSize: 11 }}>
          {isLoading ? 'searching…' : `${hits.length} hits`}
        </span>
        <div className="grow" />
        <button
          type="button"
          className="btn btn-ghost sm"
          disabled={hits.length === 0}
          onClick={copyIds}
          title="copy fragment ids to clipboard"
        >
          {copied ? 'copied' : `copy ${hits.length} ids`}
        </button>
      </div>

      {hits.length === 0 && !isLoading ? (
        <div className="dim" style={{ fontSize: 12, marginTop: 12 }}>
          No matches. Try a more specific phrase, or widen the folder /
          session filter above the canvas.
        </div>
      ) : (
        <div className="field-query-hits">
          {hits.map((h, i) => {
            const kind =
              (h.metadata?.kind as string | undefined) ?? 'unknown';
            const session =
              (h.session_id as string | undefined) ??
              (h.metadata?.src_session as string | undefined) ??
              '';
            const sessionShort = session ? session.slice(0, 8) : '';
            return (
              <button
                key={h.id}
                type="button"
                className="field-query-hit"
                onClick={() => onSelectFragment(h.id)}
                title="click to focus on the canvas"
              >
                <div className="hit-row-top">
                  <span className="hit-rank mono">#{i + 1}</span>
                  <span
                    className="hit-kind mono"
                    style={{ color: `var(--kind-${kind}, var(--ink-muted))` }}
                  >
                    {kind}
                  </span>
                  <span className="hit-sim mono">
                    {h.similarity.toFixed(3)}
                  </span>
                </div>
                <div className="hit-content">{h.content}</div>
                {sessionShort && (
                  <div className="hit-session mono dim">{sessionShort}</div>
                )}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}

function ValuePanel({
  hasEmbeddings,
  varianceRatio,
  connectionsCount,
  focusedSession,
}: {
  hasEmbeddings: boolean;
  varianceRatio: number;
  connectionsCount: number;
  focusedSession: string | undefined;
}) {
  return (
    <div className="field-side-empty">
      <div
        className="mono dim"
        style={{
          fontSize: 10.5,
          letterSpacing: '0.08em',
          textTransform: 'uppercase',
        }}
      >
        ● How to read this view
      </div>
      <div style={{ marginTop: 12 }}>
        <Hint
          title="Position = semantic similarity"
          body={
            hasEmbeddings
              ? `Each dot's (x, y) is the top-2 PCA projection of its 256-d embedding. ${(
                  varianceRatio * 100
                ).toFixed(0)}% of total variance lives in this plane — two dots near each other are about the same thing, regardless of which project they came from.`
              : 'Loading embeddings — once they arrive, position will reflect semantic similarity, not project membership.'
          }
        />
        <Hint
          title="Color = kind"
          body="Each fragment's metadata.kind decides its color. Click swatches in the bottom-left legend to hide/show kinds. The dominant color of a basin halo tells you what that cluster is mostly made of."
        />
        <Hint
          title="Lines = real co-retrieval"
          body={
            connectionsCount > 0
              ? `${connectionsCount} edges drawn from substrate co-retrieval log. Two fragments connected = they were returned together in the same /retrieve call. The more queries that hit both, the stronger the line.`
              : 'No co-retrieval data yet — run a few /retrieve calls (the dashboard does this automatically when polling inbox + sessions) and edges will appear here.'
          }
        />
        <Hint
          title="Hover = nearest neighbors"
          body="Mouse over any dot. Its 5 cosine-nearest neighbors in embedding space light up — even if they're in different basins. This is how you find related work across sessions you forgot existed."
        />
        <Hint
          title="Click = focus + explore"
          body="Click a dot to lock it. The side panel shows its content + neighbors + session. Click a basin (the ring + label) to scope rendering to that project."
        />
        <Hint
          title="Time scrubber"
          body="Drag the slider at the bottom of the canvas backward in time. Fragments stored after that point hide, so you see exactly what the substrate knew at any past moment."
        />
        <Hint
          title="Live pulse"
          body="New fragments since your last refresh pulse briefly when they enter the field. Watch the substrate grow while you work — every Claude Code Stop hook fires one in."
        />
      </div>

      <div className="divider-h" />
      <div
        className="mono dim"
        style={{
          fontSize: 10.5,
          letterSpacing: '0.08em',
          textTransform: 'uppercase',
        }}
      >
        ● What this is for
      </div>
      <div style={{ marginTop: 10, color: 'var(--ink-muted)', fontSize: 12.5, lineHeight: 1.55 }}>
        <p style={{ margin: '0 0 8px' }}>
          <strong style={{ color: 'var(--ink)' }}>See your sessions grow.</strong> The
          field is your substrate's shape rendered live. When you ship a feature, you
          should see the relevant basin densify. When you switch contexts, you'll see a
          new cluster form on the edge.
        </p>
        <p style={{ margin: '0 0 8px' }}>
          <strong style={{ color: 'var(--ink)' }}>Find connections you forgot.</strong>{' '}
          Hover any fragment to see the 5 things in your substrate most similar to it —
          including from sessions you haven't touched in weeks. This is the killer
          interaction for "didn't I do something like this before?"
        </p>
        <p style={{ margin: '0 0 8px' }}>
          <strong style={{ color: 'var(--ink)' }}>
            Audit project boundaries.
          </strong>{' '}
          When two project basins overlap heavily, the work is actually one topic in two
          repos. When one basin has scattered satellites far from its center, those
          fragments are conceptual outliers worth examining.
        </p>
        {focusedSession ? (
          <p style={{ margin: '0 0 8px', color: 'var(--accent)' }}>
            Currently focused on session{' '}
            <span className="mono">{focusedSession}</span>. Clear the filter in the page
            header to see the whole substrate again.
          </p>
        ) : (
          <p style={{ margin: '0 0 8px', color: 'var(--ink-muted)' }}>
            Pass <span className="mono">?session=cc-XXXXXX</span> in the URL (or use the
            "focus" button on a fragment) to scope the field to a single session.
          </p>
        )}
      </div>
    </div>
  );
}

function Hint({ title, body }: { title: string; body: string }) {
  return (
    <div style={{ padding: '8px 0', borderBottom: '1px dashed var(--border-subtle)' }}>
      <div style={{ fontSize: 12.5, color: 'var(--ink)', fontWeight: 500 }}>{title}</div>
      <div style={{ fontSize: 11.5, color: 'var(--ink-muted)', marginTop: 3, lineHeight: 1.5 }}>
        {body}
      </div>
    </div>
  );
}

function BasinDetail({
  basin,
  onClose,
  onFocusSession,
}: {
  basin: BasinSummary;
  onClose: () => void;
  onFocusSession: (id: string) => void;
}) {
  const kindEntries = Object.entries(basin.by_kind).sort((a, b) => b[1] - a[1]);
  return (
    <div className="field-side-detail">
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 8,
        }}
      >
        <div
          className="mono dim"
          style={{
            fontSize: 10.5,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
          }}
        >
          ● Basin · {basin.source}
        </div>
        <button className="btn sm btn-ghost" onClick={onClose} type="button">
          <Icon.X />
        </button>
      </div>
      <div style={{ fontSize: 15, fontWeight: 500, color: 'var(--ink)' }}>
        {basin.label}
      </div>
      <dl className="kv" style={{ marginTop: 12 }}>
        <dt>mass</dt>
        <dd>{basin.mass.toLocaleString()} active fragments</dd>
        <dt>sessions</dt>
        <dd>{basin.sessions.length}</dd>
        <dt>source</dt>
        <dd>
          {basin.source === 'attractor'
            ? 'canonical attractor centroid'
            : 'project_cwd fallback'}
        </dd>
      </dl>
      <div className="section-h" style={{ margin: '16px 0 8px' }}>
        <h3
          style={{
            fontSize: 11.5,
            color: 'var(--ink-muted)',
            textTransform: 'uppercase',
            letterSpacing: '0.06em',
          }}
        >
          kind distribution
        </h3>
      </div>
      <ul style={{ margin: 0, padding: 0, listStyle: 'none' }}>
        {kindEntries.slice(0, 8).map(([k, n]) => (
          <li
            key={k}
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              padding: '4px 0',
              fontSize: 12,
            }}
          >
            <span style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: 999,
                  background: KIND_COLOR[k] ?? KIND_COLOR.unknown,
                }}
              />
              <span className="mono">{k}</span>
            </span>
            <span className="mono dim">{n}</span>
          </li>
        ))}
      </ul>
      <div className="section-h" style={{ margin: '16px 0 8px' }}>
        <h3
          style={{
            fontSize: 11.5,
            color: 'var(--ink-muted)',
            textTransform: 'uppercase',
            letterSpacing: '0.06em',
          }}
        >
          sessions
        </h3>
      </div>
      <ul
        style={{
          margin: 0,
          padding: 0,
          listStyle: 'none',
          maxHeight: 280,
          overflowY: 'auto',
        }}
      >
        {basin.sessions.slice(0, 20).map((s) => (
          <li key={s} style={{ padding: '4px 0' }}>
            <button
              className="btn-ghost sm"
              onClick={() => onFocusSession(s)}
              type="button"
              style={{ fontFamily: 'var(--font-mono)', fontSize: 11.5 }}
            >
              {s} →
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function FragmentDetail({
  fragment,
  neighborIds,
  allFragments,
  onClose,
  onFocusSession,
}: {
  fragment: FieldFragment;
  neighborIds: Set<string>;
  allFragments: FieldFragment[];
  onClose: () => void;
  onFocusSession: (id: string) => void;
}) {
  const kind = (fragment.metadata.kind as string | undefined) ?? 'unknown';
  const color = KIND_COLOR[kind] ?? KIND_COLOR.unknown;
  const neighbors = allFragments.filter((f) => neighborIds.has(f.id));
  return (
    <div className="field-side-detail">
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          marginBottom: 8,
        }}
      >
        <div
          className="mono dim"
          style={{
            fontSize: 10.5,
            letterSpacing: '0.08em',
            textTransform: 'uppercase',
          }}
        >
          ● Fragment · {kind}
        </div>
        <button className="btn sm btn-ghost" onClick={onClose} type="button">
          <Icon.X />
        </button>
      </div>
      <div
        style={{
          display: 'flex',
          gap: 10,
          alignItems: 'flex-start',
          marginTop: 4,
        }}
      >
        <span
          style={{
            width: 10,
            height: 10,
            borderRadius: 999,
            background: color,
            marginTop: 6,
            flexShrink: 0,
          }}
        />
        <div
          style={{
            fontSize: 13.5,
            color: 'var(--ink)',
            lineHeight: 1.5,
            wordBreak: 'break-word',
          }}
        >
          {fragment.content}
        </div>
      </div>
      <dl className="kv" style={{ marginTop: 14 }}>
        <dt>session</dt>
        <dd>
          <button
            className="btn-ghost sm"
            onClick={() => onFocusSession(fragment.session_id)}
            type="button"
            style={{ fontFamily: 'var(--font-mono)', fontSize: 11.5 }}
          >
            {fragment.session_id} →
          </button>
        </dd>
        <dt>project</dt>
        <dd className="mono">{fragment.project}</dd>
        <dt>age</dt>
        <dd>{fragment.ageDays != null ? `${fragment.ageDays}d` : 'unknown'}</dd>
        <dt>importance</dt>
        <dd>{fragment.importance.toFixed(2)}</dd>
      </dl>
      <div className="section-h" style={{ margin: '16px 0 8px' }}>
        <h3
          style={{
            fontSize: 11.5,
            color: 'var(--ink-muted)',
            textTransform: 'uppercase',
            letterSpacing: '0.06em',
          }}
        >
          nearest neighbors in embedding space
        </h3>
      </div>
      {neighbors.length === 0 ? (
        <div className="muted" style={{ fontSize: 11.5 }}>
          No neighbors found — this fragment may be isolated.
        </div>
      ) : (
        <ul style={{ margin: 0, padding: 0, listStyle: 'none' }}>
          {neighbors.map((n) => {
            const nKind =
              (n.metadata.kind as string | undefined) ?? 'unknown';
            return (
              <li
                key={n.id}
                style={{
                  padding: '6px 0',
                  borderBottom: '1px dashed var(--border-subtle)',
                  cursor: 'pointer',
                }}
              >
                <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                  <span
                    style={{
                      width: 6,
                      height: 6,
                      borderRadius: 999,
                      background: KIND_COLOR[nKind] ?? KIND_COLOR.unknown,
                    }}
                  />
                  <span
                    className="mono dim"
                    style={{ fontSize: 10, textTransform: 'uppercase' }}
                  >
                    {nKind}
                  </span>
                  <span
                    className="mono dim"
                    title={n.session_id}
                    style={{ fontSize: 10, marginLeft: 'auto' }}
                  >
                    {n.session_id.length > 16
                      ? `${n.session_id.slice(0, 11)}…${n.session_id.slice(-6)}`
                      : n.session_id}
                  </span>
                </div>
                <div
                  style={{
                    fontSize: 12,
                    color: 'var(--ink-muted)',
                    marginTop: 3,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    display: '-webkit-box',
                    WebkitLineClamp: 2,
                    WebkitBoxOrient: 'vertical',
                  }}
                >
                  {n.content}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

// =============================================================================
// Tooltip
// =============================================================================

function FieldTooltip({
  fragment,
  xPct,
  yPct,
}: {
  fragment: FieldFragment;
  xPct: number;
  yPct: number;
}) {
  const kind = (fragment.metadata.kind as string | undefined) ?? 'unknown';
  return (
    <div
      className="field-tooltip frag"
      style={{
        position: 'absolute',
        left: `${xPct}%`,
        top: `${yPct}%`,
        transform: 'translate(12px, -50%)',
        pointerEvents: 'none',
        zIndex: 5,
      }}
    >
      <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
        <span
          style={{
            width: 6,
            height: 6,
            borderRadius: 999,
            background: KIND_COLOR[kind] ?? KIND_COLOR.unknown,
          }}
        />
        <span
          className="mono"
          style={{
            fontSize: 10,
            color: 'var(--ink-muted)',
            textTransform: 'uppercase',
            letterSpacing: '0.06em',
          }}
        >
          {kind}
        </span>
        {fragment.ageDays != null && (
          <span className="mono dim" style={{ fontSize: 10, marginLeft: 'auto' }}>
            {fragment.ageDays}d ago
          </span>
        )}
      </div>
      <div
        style={{
          fontSize: 12,
          marginTop: 6,
          color: 'var(--ink)',
          maxWidth: 320,
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          display: '-webkit-box',
          WebkitLineClamp: 4,
          WebkitBoxOrient: 'vertical',
        }}
      >
        {fragment.content}
      </div>
      <div className="mono dim" style={{ fontSize: 10, marginTop: 6 }}>
        {fragment.session_id} · {fragment.project}
      </div>
    </div>
  );
}

// =============================================================================
// SessionPicker — searchable combobox for the session filter
// =============================================================================
//
// Replaces the native <select> for the session dropdown because:
//
// 1. 100+ sessions is too many to scroll. Typing to filter is the only
//    usable interaction at that scale.
// 2. The substrate's internal `cc-<8char>` is opaque; users routinely
//    have the full Claude Code UUID in their hand (from `~/.claude/
//    projects/.../<uuid>.jsonl`) and want to paste it. Showing the
//    full UUID as the visible label and accepting it as a search
//    string solves both problems.
//
// Implementation is a deliberately small dependency-free combobox:
// input + popover list + keyboard nav + outside-click close. ~150 LOC.
// If we end up needing this elsewhere (folder picker, tools page, …)
// it gets extracted to web/src/components/SearchableSelect.tsx; right
// now it's the only consumer.
type SessionPickerProps = {
  sessions: SessionListItem[];
  value: string | undefined; // selected cc-id (substrate short form)
  onChange: (id: string | undefined) => void;
  projectLabel: string | undefined; // for the empty-state placeholder
};

function basenameOf(p: string | null | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

function SessionPicker({
  sessions,
  value,
  onChange,
  projectLabel,
}: SessionPickerProps) {
  const [query, setQuery] = useState('');
  const [open, setOpen] = useState(false);
  const [highlight, setHighlight] = useState(0);
  const wrapperRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  // Filter sessions by query — matches the full UUID, the cc-<8char>
  // short id, and the project basename. Case-insensitive substring.
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) => {
      const uuid = (s.src_session_uuid ?? '').toLowerCase();
      const cc = s.id.toLowerCase();
      const proj = basenameOf(s.project_cwd).toLowerCase();
      return uuid.includes(q) || cc.includes(q) || proj.includes(q);
    });
  }, [sessions, query]);

  // Reset highlight whenever the filtered list changes.
  useEffect(() => {
    setHighlight(0);
  }, [filtered.length]);

  // Keep the highlighted row scrolled into view (only when open).
  useEffect(() => {
    if (!open || !listRef.current) return;
    const node = listRef.current.querySelector<HTMLDivElement>(
      `[data-session-row="${highlight}"]`,
    );
    node?.scrollIntoView({ block: 'nearest' });
  }, [highlight, open]);

  // Close on outside click.
  useEffect(() => {
    const onDocClick = (e: MouseEvent) => {
      if (!wrapperRef.current) return;
      if (!wrapperRef.current.contains(e.target as Node)) {
        setOpen(false);
        setQuery('');
      }
    };
    document.addEventListener('mousedown', onDocClick);
    return () => document.removeEventListener('mousedown', onDocClick);
  }, []);

  const selected = value ? sessions.find((s) => s.id === value) : undefined;
  const placeholder = selected
    ? `${selected.src_session_uuid || selected.id} · ${basenameOf(selected.project_cwd)} · ${selected.fragment_count} frags`
    : `all ${sessions.length} session${sessions.length === 1 ? '' : 's'}${projectLabel ? ` in "${projectLabel}"` : ''}`;

  const commitSession = (s: SessionListItem | null) => {
    onChange(s ? s.id : undefined);
    setOpen(false);
    setQuery('');
    inputRef.current?.blur();
  };

  const onKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (!open) setOpen(true);
      setHighlight((h) => Math.min(filtered.length - 1, h + 1));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setHighlight((h) => Math.max(0, h - 1));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      if (filtered.length > 0) commitSession(filtered[highlight] ?? null);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      setOpen(false);
      setQuery('');
      inputRef.current?.blur();
    }
  };

  return (
    <div ref={wrapperRef} style={{ position: 'relative', minWidth: 360 }}>
      <input
        ref={inputRef}
        type="text"
        className="mono"
        // Show placeholder = current selection. When user types, the
        // typed text shadows the placeholder. This avoids the awkward
        // "should we sync the input text to the selection" question
        // that bites every combobox without it.
        placeholder={placeholder}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          if (!open) setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        style={{
          width: '100%',
          background: 'var(--surface-2)',
          color: 'var(--ink)',
          border: '1px solid var(--border)',
          borderRadius: 6,
          padding: '5px 26px 5px 10px',
          fontSize: 12,
          fontFamily: 'var(--font-mono)',
          outline: 'none',
        }}
      />
      {value && (
        <button
          type="button"
          onClick={() => commitSession(null)}
          title="clear session filter"
          style={{
            position: 'absolute',
            right: 6,
            top: '50%',
            transform: 'translateY(-50%)',
            background: 'transparent',
            border: 'none',
            color: 'var(--ink-faint)',
            cursor: 'pointer',
            padding: '2px 6px',
            fontFamily: 'var(--font-mono)',
            fontSize: 12,
            lineHeight: 1,
          }}
        >
          ×
        </button>
      )}
      {open && (
        <div
          ref={listRef}
          style={{
            position: 'absolute',
            left: 0,
            right: 0,
            top: 'calc(100% + 4px)',
            background: 'var(--surface-1, #111111)',
            border: '1px solid var(--border)',
            borderRadius: 6,
            maxHeight: 340,
            overflowY: 'auto',
            zIndex: 50,
            boxShadow: '0 10px 30px rgba(0, 0, 0, 0.5)',
          }}
        >
          <div
            onClick={() => commitSession(null)}
            style={{
              padding: '6px 10px',
              cursor: 'pointer',
              fontFamily: 'var(--font-mono)',
              fontSize: 11.5,
              color: value ? 'var(--ink-muted)' : 'var(--accent)',
              borderBottom: '1px dashed var(--border)',
              background: !value ? 'var(--surface-2)' : undefined,
            }}
          >
            all {sessions.length} session
            {sessions.length === 1 ? '' : 's'}
            {projectLabel ? ` in "${projectLabel}"` : ''}
          </div>
          {filtered.length === 0 ? (
            <div
              style={{
                padding: '14px 10px',
                color: 'var(--ink-faint)',
                fontSize: 11.5,
                fontStyle: 'italic',
              }}
            >
              no sessions match "{query}"
            </div>
          ) : (
            filtered.map((s, i) => {
              const uuid = s.src_session_uuid || s.id;
              const proj = basenameOf(s.project_cwd);
              const isHl = i === highlight;
              const isSel = value === s.id;
              return (
                <div
                  key={s.id}
                  data-session-row={i}
                  onClick={() => commitSession(s)}
                  onMouseEnter={() => setHighlight(i)}
                  style={{
                    padding: '5px 10px',
                    cursor: 'pointer',
                    fontFamily: 'var(--font-mono)',
                    fontSize: 11.5,
                    color: isSel ? 'var(--accent)' : 'var(--ink)',
                    background: isHl
                      ? 'var(--surface-2)'
                      : isSel
                        ? 'var(--accent-soft)'
                        : undefined,
                    display: 'flex',
                    justifyContent: 'space-between',
                    gap: 12,
                    alignItems: 'baseline',
                  }}
                >
                  <span style={{ overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                    {uuid}
                  </span>
                  <span
                    style={{
                      color: 'var(--ink-faint)',
                      fontSize: 10.5,
                      flexShrink: 0,
                    }}
                  >
                    {proj} · {s.fragment_count}
                  </span>
                </div>
              );
            })
          )}
        </div>
      )}
    </div>
  );
}
