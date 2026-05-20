import { createFileRoute } from '@tanstack/react-router';

import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';

export const Route = createFileRoute('/sessions')({
  component: SessionsPage,
});

function SessionsPage() {
  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6">
      <PageHeader
        title="Sessions"
        subtitle="Every Claude Code session this substrate has seen."
      />
      <EmptyState
        title="Sessions list lands in Step 3"
        body="Will show every session under ~/.claude/projects with goal-phase, accomplishment counts, and last-activity timestamps."
      />
    </div>
  );
}
