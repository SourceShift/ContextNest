import { useQuery } from '@tanstack/react-query';

import { api } from '@/lib/api';
import { cn } from '@/lib/cn';

export function SubstrateBadge() {
  const { data, isError, isLoading } = useQuery({
    queryKey: ['health'],
    queryFn: () => api.health(),
    refetchInterval: 15_000,
  });

  const state = isLoading
    ? 'loading'
    : isError
      ? 'down'
      : data?.healthy
        ? 'healthy'
        : 'degraded';

  const dotClass =
    state === 'healthy'
      ? 'bg-[--color-accent] shadow-[0_0_8px_var(--color-accent)]'
      : state === 'degraded'
        ? 'bg-[--color-urgency-soon]'
        : state === 'down'
          ? 'bg-[--color-urgency-now]'
          : 'bg-[--color-ink-dim]';

  const label =
    state === 'healthy'
      ? 'substrate healthy'
      : state === 'degraded'
        ? 'substrate degraded'
        : state === 'down'
          ? 'substrate unreachable'
          : 'checking…';

  return (
    <div className="flex items-center gap-2 text-xs text-[--color-ink-muted]">
      <span className={cn('size-2 rounded-full transition-colors', dotClass)} />
      <span>{label}</span>
    </div>
  );
}
