import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Library } from "lucide-react";

export function MemoriesRoute() {
  return (
    <div className="mx-auto max-w-3xl">
      <header className="mb-6">
        <h1 className="text-2xl font-semibold text-[var(--color-text)]">
          Memories
        </h1>
        <p className="mt-2 text-sm text-[var(--color-text-muted)]">
          Browse stored fragments for the active session. Lands in phase 3
          of the v0.2-ui milestone.
        </p>
      </header>

      <Card>
        <CardHeader>
          <div className="flex items-center gap-2 text-[var(--color-accent)]">
            <Library className="size-5" />
            <CardTitle>Coming in phase 3</CardTitle>
          </div>
          <CardDescription>
            Per-session fragment list with filters (importance threshold,
            basin id, date range), inline importance editing, soft-delete
            UI with undo, and a session switcher in the top bar.
          </CardDescription>
        </CardHeader>
      </Card>

      <p className="mt-6 text-xs text-[var(--color-text-subtle)]">
        Track progress in{" "}
        <a
          href="/docs/roadmap/v0.2-ui-dashboard.md"
          className="underline underline-offset-4 hover:text-[var(--color-text-muted)]"
        >
          docs/roadmap/v0.2-ui-dashboard.md
        </a>
        .
      </p>
    </div>
  );
}
