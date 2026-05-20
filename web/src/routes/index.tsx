import { createFileRoute } from '@tanstack/react-router';

import { PageHeader } from '@/components/PageHeader';
import { EmptyState } from '@/components/EmptyState';

export const Route = createFileRoute('/')({
  component: InboxPage,
});

function InboxPage() {
  return (
    <div className="mx-auto flex max-w-4xl flex-col gap-6">
      <PageHeader
        title="Inbox"
        subtitle="What Claude needs from you across every session."
      />
      <EmptyState
        title="Wiring up"
        body="The Inbox view lands in Step 2 of the dashboard build — this scaffold is the shell. Run a `contextnest inbox` from the CLI in the meantime."
      />
    </div>
  );
}
