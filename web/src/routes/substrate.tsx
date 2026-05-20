import { createFileRoute } from '@tanstack/react-router';
import { useQuery } from '@tanstack/react-query';

import { PageHeader } from '@/components/PageHeader';
import { api, substrateUrl } from '@/lib/api';
import { cn } from '@/lib/cn';

export const Route = createFileRoute('/substrate')({
  component: SubstratePage,
});

function SubstratePage() {
  const health = useQuery({ queryKey: ['health'], queryFn: () => api.health() });
  const status = useQuery({ queryKey: ['status'], queryFn: () => api.status() });

  const healthy = health.data?.healthy === true;

  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6">
      <PageHeader
        title="Substrate"
        subtitle="Health, version, hooks, embedding provider."
      />

      <section className="card flex items-start gap-4">
        <div
          className={cn(
            'mt-1 size-3 shrink-0 rounded-full',
            healthy
              ? 'bg-[--color-accent] shadow-[0_0_12px_var(--color-accent)]'
              : health.isError
                ? 'bg-[--color-urgency-now]'
                : 'bg-[--color-urgency-soon]',
          )}
        />
        <div className="flex flex-1 flex-col gap-1">
          <div className="text-sm font-medium text-[--color-ink]">
            {health.isLoading
              ? 'Checking…'
              : healthy
                ? 'Healthy'
                : health.isError
                  ? 'Unreachable'
                  : 'Degraded'}
          </div>
          <div className="mono text-xs text-[--color-ink-muted]">
            {substrateUrl}
            {status.data
              ? ` · ${status.data.name} ${status.data.version}`
              : ''}
          </div>
        </div>
      </section>

      <section className="card flex flex-col gap-3">
        <h2 className="text-sm font-medium uppercase tracking-wider text-[--color-ink-muted]">
          Coming in Step 5
        </h2>
        <ul className="flex flex-col gap-1 text-sm text-[--color-ink-muted]">
          <li>Fragment + session counts by kind</li>
          <li>Hook wiring panel with reinstall / uninstall</li>
          <li>Embedding provider switch (local / openai / ollama / hf)</li>
          <li>Recent activity feed (last 50 stores + retrieves)</li>
        </ul>
      </section>
    </div>
  );
}
