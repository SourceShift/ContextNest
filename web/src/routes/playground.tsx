import { Card, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { FlaskConical } from "lucide-react";

export function PlaygroundRoute() {
  return (
    <div className="mx-auto max-w-3xl">
      <header className="mb-6">
        <h1 className="text-2xl font-semibold text-[var(--color-text)]">
          Playground
        </h1>
        <p className="mt-2 text-sm text-[var(--color-text-muted)]">
          Try the seven memory tools against your live substrate. Lands in
          phase 2 of the v0.2-ui milestone.
        </p>
      </header>

      <Card>
        <CardHeader>
          <div className="flex items-center gap-2 text-[var(--color-accent)]">
            <FlaskConical className="size-5" />
            <CardTitle>Coming in phase 2</CardTitle>
          </div>
          <CardDescription>
            This screen will host forms for store / retrieve / update /
            summarize / discard / reconstruct / resonate with response
            inspection (markdown rendering for summarize and reconstruct
            output), session-id picker persisted to localStorage, and
            shareable curl snippets for every executed call.
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
