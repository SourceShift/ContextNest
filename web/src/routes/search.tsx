import { createFileRoute } from '@tanstack/react-router';

import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';

export const Route = createFileRoute('/search')({
  component: SearchPage,
});

function SearchPage() {
  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6">
      <PageHeader
        title="Search"
        subtitle="Semantic + metadata-filtered retrieval across every session."
      />
      <EmptyState
        title="Search lands in Step 4"
        body="Will run debounced /api/v1/tools/retrieve queries with chip-based metadata filters (kind, urgency, project)."
      />
    </div>
  );
}
