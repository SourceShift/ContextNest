import { createFileRoute } from '@tanstack/react-router';

import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';

export const Route = createFileRoute('/phases')({
  component: PhasesPage,
});

function PhasesPage() {
  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6">
      <PageHeader
        title="Phases"
        subtitle="Goal phases — multi-turn clustered intents across every session."
      />
      <EmptyState
        title="Phase timeline lands in Step 6"
        body="Renders goal_phase memories grouped by session into a vertical timeline with turn counts and time spans."
      />
    </div>
  );
}
