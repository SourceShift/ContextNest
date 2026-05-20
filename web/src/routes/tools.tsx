import { createFileRoute } from '@tanstack/react-router';

import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';

export const Route = createFileRoute('/tools')({
  component: ToolsPage,
});

function ToolsPage() {
  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6">
      <PageHeader
        title="Tools"
        subtitle="Playground for the seven substrate tools: store, retrieve, update, summarize, discard, reconstruct, resonate."
      />
      <EmptyState
        title="Tools playground lands in Step 6"
        body="Tabbed interface per tool with a JSON request editor, response viewer, and canned templates."
      />
    </div>
  );
}
