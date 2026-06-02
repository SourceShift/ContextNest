import type { SessionListItem } from './types';

export type ProjectOption = {
  label: string;
  sessions: number;
  fragments: number;
};

export function basenameOf(p: string | null | undefined): string {
  if (!p) return '?';
  const segs = p.replace(/\/+$/, '').split('/');
  return segs[segs.length - 1] || '?';
}

export function buildProjectOptions(sessions: SessionListItem[]): ProjectOption[] {
  const byProject = new Map<string, ProjectOption>();
  for (const s of sessions) {
    const label = basenameOf(s.project_cwd);
    const existing = byProject.get(label);
    if (existing) {
      existing.sessions += 1;
      existing.fragments += s.fragment_count;
    } else {
      byProject.set(label, {
        label,
        sessions: 1,
        fragments: s.fragment_count,
      });
    }
  }
  return Array.from(byProject.values()).sort(
    (a, b) => b.fragments - a.fragments || a.label.localeCompare(b.label),
  );
}
