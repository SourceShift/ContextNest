import type { SVGProps } from 'react';

type IconProps = SVGProps<SVGSVGElement>;

const ICON_PROPS = {
  fill: 'none',
  stroke: 'currentColor',
  strokeWidth: 1.75,
  strokeLinecap: 'round' as const,
  strokeLinejoin: 'round' as const,
};

export const Icon = {
  Inbox: (p: IconProps) => (
    <svg width="14" height="14" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <path d="M22 12h-6l-2 3h-4l-2-3H2" />
      <path d="M5.45 5.11 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11Z" />
    </svg>
  ),
  List: (p: IconProps) => (
    <svg width="14" height="14" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <line x1="8" y1="6" x2="21" y2="6" />
      <line x1="8" y1="12" x2="21" y2="12" />
      <line x1="8" y1="18" x2="21" y2="18" />
      <line x1="3" y1="6" x2="3.01" y2="6" />
      <line x1="3" y1="12" x2="3.01" y2="12" />
      <line x1="3" y1="18" x2="3.01" y2="18" />
    </svg>
  ),
  Search: (p: IconProps) => (
    <svg width="14" height="14" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <circle cx="11" cy="11" r="7" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  ),
  Layers: (p: IconProps) => (
    <svg width="14" height="14" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <polygon points="12 2 2 7 12 12 22 7 12 2" />
      <polyline points="2 17 12 22 22 17" />
      <polyline points="2 12 12 17 22 12" />
    </svg>
  ),
  Atom: (p: IconProps) => (
    <svg width="14" height="14" viewBox="0 0 24 24" {...ICON_PROPS} strokeWidth={1.5} {...p}>
      <circle cx="12" cy="12" r="1.6" fill="currentColor" />
      <ellipse cx="12" cy="12" rx="10" ry="4.2" />
      <ellipse cx="12" cy="12" rx="10" ry="4.2" transform="rotate(60 12 12)" />
      <ellipse cx="12" cy="12" rx="10" ry="4.2" transform="rotate(-60 12 12)" />
    </svg>
  ),
  Terminal: (p: IconProps) => (
    <svg width="14" height="14" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <polyline points="4 17 10 11 4 5" />
      <line x1="12" y1="19" x2="20" y2="19" />
    </svg>
  ),
  Cpu: (p: IconProps) => (
    <svg width="14" height="14" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <rect x="4" y="4" width="16" height="16" rx="2" />
      <rect x="9" y="9" width="6" height="6" />
      <line x1="9" y1="2" x2="9" y2="4" />
      <line x1="15" y1="2" x2="15" y2="4" />
      <line x1="9" y1="20" x2="9" y2="22" />
      <line x1="15" y1="20" x2="15" y2="22" />
      <line x1="20" y1="9" x2="22" y2="9" />
      <line x1="20" y1="15" x2="22" y2="15" />
      <line x1="2" y1="9" x2="4" y2="9" />
      <line x1="2" y1="15" x2="4" y2="15" />
    </svg>
  ),
  Refresh: (p: IconProps) => (
    <svg width="13" height="13" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <polyline points="23 4 23 10 17 10" />
      <polyline points="1 20 1 14 7 14" />
      <path d="M3.51 9a9 9 0 0 1 14.85-3.36L23 10M1 14l4.64 4.36A9 9 0 0 0 20.49 15" />
    </svg>
  ),
  Filter: (p: IconProps) => (
    <svg width="13" height="13" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
    </svg>
  ),
  Plus: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} strokeWidth={2} {...p}>
      <line x1="12" y1="5" x2="12" y2="19" />
      <line x1="5" y1="12" x2="19" y2="12" />
    </svg>
  ),
  X: (p: IconProps) => (
    <svg width="10" height="10" viewBox="0 0 24 24" {...ICON_PROPS} strokeWidth={2.5} {...p}>
      <line x1="18" y1="6" x2="6" y2="18" />
      <line x1="6" y1="6" x2="18" y2="18" />
    </svg>
  ),
  Check: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} strokeWidth={2.5} {...p}>
      <polyline points="20 6 9 17 4 12" />
    </svg>
  ),
  Clock: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <circle cx="12" cy="12" r="10" />
      <polyline points="12 6 12 12 16 14" />
    </svg>
  ),
  Chevron: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} strokeWidth={2} {...p}>
      <polyline points="9 18 15 12 9 6" />
    </svg>
  ),
  Question: (p: IconProps) => (
    <svg width="12" height="12" viewBox="0 0 24 24" {...ICON_PROPS} strokeWidth={2} {...p}>
      <circle cx="12" cy="12" r="10" />
      <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  ),
  ArrowRight: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} strokeWidth={2} {...p}>
      <line x1="5" y1="12" x2="19" y2="12" />
      <polyline points="12 5 19 12 12 19" />
    </svg>
  ),
  Info: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="16" x2="12" y2="12" />
      <line x1="12" y1="8" x2="12.01" y2="8" />
    </svg>
  ),
  Alert: (p: IconProps) => (
    <svg width="12" height="12" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0Z" />
      <line x1="12" y1="9" x2="12" y2="13" />
      <line x1="12" y1="17" x2="12.01" y2="17" />
    </svg>
  ),
  Copy: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  ),
  Folder: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
    </svg>
  ),
  Send: (p: IconProps) => (
    <svg width="12" height="12" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <line x1="22" y1="2" x2="11" y2="13" />
      <polygon points="22 2 15 22 11 13 2 9 22 2" />
    </svg>
  ),
  Hash: (p: IconProps) => (
    <svg width="11" height="11" viewBox="0 0 24 24" {...ICON_PROPS} {...p}>
      <line x1="4" y1="9" x2="20" y2="9" />
      <line x1="4" y1="15" x2="20" y2="15" />
      <line x1="10" y1="3" x2="8" y2="21" />
      <line x1="16" y1="3" x2="14" y2="21" />
    </svg>
  ),
};

export function BrandMark({
  size = 22,
  color = '#00d4aa',
  dim = false,
}: {
  size?: number;
  color?: string;
  dim?: boolean;
}) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      style={{ opacity: dim ? 0.4 : 1 }}
    >
      <circle cx="12" cy="12" r="10.5" stroke={color} strokeOpacity="0.25" strokeWidth="1" />
      <circle cx="12" cy="12" r="7" stroke={color} strokeOpacity="0.5" strokeWidth="1" />
      <circle cx="12" cy="12" r="3.5" stroke={color} strokeWidth="1.25" />
      <circle cx="12" cy="12" r="1.6" fill={color} />
      <circle cx="20.5" cy="6.5" r="1.1" fill={color} fillOpacity="0.6" />
    </svg>
  );
}

export type Urgency = 'now' | 'soon' | 'later';

export function UrgencyDot({ urg }: { urg: Urgency }) {
  return <span className={`urg-dot urg-${urg}`} aria-label={urg} />;
}

export function UrgencyLabel({ urg }: { urg: Urgency }) {
  return <span className={`urg-label ${urg}`}>{urg}</span>;
}

export function KindBadge({ kind }: { kind: string }) {
  return <span className={`kind-badge ${kind}`}>{kind.replace('_', ' ')}</span>;
}

/** Visually compact `cc-1234abcd…01234567` form for a 39-char canonical
 *  session id. Long-form ids are abbreviated with a horizontal ellipsis;
 *  short / unknown ids pass through untouched. Pure presentation — the
 *  underlying `id` stays the canonical full UUID. */
function shortenSessionId(id: string): string {
  if (id.length <= 16) return id;
  const head = id.slice(0, 11);
  const tail = id.slice(-8);
  return `${head}…${tail}`;
}

export function SessionPill({
  id,
  onClick,
  showFull = false,
}: {
  /** The canonical full session id, e.g. `cc-9b8a1f3e-51e2-bc40-89ab-cdef01234567`. */
  id: string;
  /** Override the default copy-to-clipboard behaviour. Receives no args
   *  — the parent is expected to know `id` already. When set, the
   *  caller takes ownership of the click handler entirely. */
  onClick?: () => void;
  /** Render the full id without truncation. Useful in pages that have
   *  the horizontal real estate, e.g. session detail header. */
  showFull?: boolean;
}) {
  const visible = showFull ? id : shortenSessionId(id);
  const handleClick = onClick
    ? onClick
    : () => {
        // Default: copy the canonical id to clipboard. No toast — the
        // tooltip already says "click to copy", and a 39-char string
        // landing in the user's paste buffer is its own confirmation.
        if (typeof navigator !== 'undefined' && navigator.clipboard) {
          void navigator.clipboard.writeText(id).catch(() => {
            /* clipboard API can fail under non-secure contexts; swallow */
          });
        }
      };
  const title = onClick ? id : `${id}\n(click to copy)`;
  return (
    <span
      className="mono"
      onClick={handleClick}
      title={title}
      style={{
        color: 'var(--ink)',
        background: 'var(--surface-2)',
        border: '1px solid var(--border)',
        padding: '1px 6px',
        borderRadius: '5px',
        fontSize: '11px',
        cursor: 'pointer',
        userSelect: 'all',
      }}
    >
      {visible}
    </span>
  );
}

export function ProjBadge({ p }: { p: string }) {
  return (
    <span className="mono" style={{ color: 'var(--ink-muted)', fontSize: '11px' }}>
      <Icon.Folder style={{ marginRight: 4, verticalAlign: '-1px', color: 'var(--ink-faint)' }} />
      {p}
    </span>
  );
}

export function Sparkline({
  data,
  w = 180,
  h = 18,
  color = 'var(--accent-dim)',
}: {
  data: number[];
  w?: number;
  h?: number;
  color?: string;
}) {
  const max = Math.max(...data, 1);
  const pts = data.map((v, i) => {
    const x = (i / (data.length - 1)) * w;
    const y = h - (v / max) * (h - 2) - 1;
    return [x, y];
  });
  const d = pts.map((p, i) => (i === 0 ? `M${p[0]},${p[1]}` : `L${p[0]},${p[1]}`)).join(' ');
  const area = d + ` L${w},${h} L0,${h} Z`;
  return (
    <svg className="sparkline" width={w} height={h} viewBox={`0 0 ${w} ${h}`}>
      <path d={area} fill={color} fillOpacity="0.12" />
      <path d={d} stroke={color} strokeWidth="1.25" fill="none" />
    </svg>
  );
}
