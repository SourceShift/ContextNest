/**
 * Relative-time helpers — no external deps.
 */

export function agoFrom(iso: string): string {
  if (!iso) return '';
  const stored = new Date(iso);
  if (isNaN(stored.getTime())) return '';

  const now = Date.now();
  const diffMs = now - stored.getTime();
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHrs = Math.floor(diffMin / 60);
  const diffDays = Math.floor(diffHrs / 24);

  if (diffSec < 60) return 'just now';
  if (diffMin < 60) return `${diffMin} min ago`;
  if (diffHrs < 24) {
    const remMin = diffMin % 60;
    return remMin > 0 ? `${diffHrs}h ${remMin}m ago` : `${diffHrs}h ago`;
  }
  if (diffDays === 1) return 'yesterday';
  return `${diffDays}d ago`;
}

export function isWithinMinutes(iso: string, minutes: number): boolean {
  if (!iso) return false;
  const stored = new Date(iso);
  if (isNaN(stored.getTime())) return false;
  return Date.now() - stored.getTime() < minutes * 60 * 1000;
}
