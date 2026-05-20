import { createFileRoute } from '@tanstack/react-router';

import { EmptyState } from '@/components/EmptyState';

export const Route = createFileRoute('/sessions/$id')({
  component: SessionDetail,
});

function SessionDetail() {
  const { id } = Route.useParams();
  return (
    <EmptyState
      title={`Drill-down for ${id}`}
      body="Per-session view (goal phases, accomplishments, learnings, todos, blockers) lands in Step 3."
    />
  );
}
