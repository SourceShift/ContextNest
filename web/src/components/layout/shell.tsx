import { Outlet } from "@tanstack/react-router";
import { useQuery } from "@tanstack/react-query";
import { Circle } from "lucide-react";

import { api } from "@/lib/api";
import { Sidebar } from "@/components/layout/sidebar";
import { cn } from "@/lib/cn";

/**
 * Outer chrome — sidebar + a top status strip + a main content area for
 * the routed page. The status strip shows whether the substrate is
 * reachable so users notice a dead `/health` before they hit a 502 on
 * the first API call.
 */
export function Shell() {
  return (
    <div className="flex h-full">
      <Sidebar />
      <main className="flex h-full flex-1 flex-col overflow-hidden">
        <SubstrateStatus />
        <div className="flex-1 overflow-y-auto px-8 py-6">
          <Outlet />
        </div>
      </main>
    </div>
  );
}

function SubstrateStatus() {
  const { data, isError, isLoading } = useQuery({
    queryKey: ["health"],
    queryFn: () => api.health(),
    refetchInterval: 15_000,
  });

  const state = isLoading ? "loading" : isError ? "error" : "ok";
  const label =
    state === "loading"
      ? "Connecting to substrate..."
      : state === "error"
        ? "Substrate unreachable on /health — is `cargo run -- serve` running?"
        : `Substrate healthy${data?.ready === false ? " (warming up)" : ""}`;
  const color =
    state === "ok"
      ? "var(--color-success)"
      : state === "error"
        ? "var(--color-danger)"
        : "var(--color-warning)";

  return (
    <div
      className={cn(
        "flex items-center gap-2 border-b border-[var(--color-border)]",
        "bg-[var(--color-bg-subtle)] px-8 py-2",
        "text-xs text-[var(--color-text-muted)]",
      )}
    >
      <Circle className="size-2 fill-current" style={{ color }} />
      <span>{label}</span>
    </div>
  );
}
