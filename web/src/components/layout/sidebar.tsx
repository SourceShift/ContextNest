import { Link, useRouterState } from "@tanstack/react-router";
import { Boxes, FlaskConical, Library } from "lucide-react";
import { cn } from "@/lib/cn";

interface NavItem {
  to: "/" | "/playground" | "/memories";
  label: string;
  icon: React.ReactNode;
  hint: string;
}

const NAV: NavItem[] = [
  {
    to: "/",
    label: "Providers",
    icon: <Boxes className="size-4" />,
    hint: "LLM + embedding catalog",
  },
  {
    to: "/playground",
    label: "Playground",
    icon: <FlaskConical className="size-4" />,
    hint: "Try the seven tools",
  },
  {
    to: "/memories",
    label: "Memories",
    icon: <Library className="size-4" />,
    hint: "Browse stored fragments",
  },
];

export function Sidebar() {
  const pathname = useRouterState({ select: (s) => s.location.pathname });

  return (
    <aside
      className={cn(
        "flex h-full w-60 shrink-0 flex-col",
        "border-r border-[var(--color-border)] bg-[var(--color-bg-subtle)]",
        "px-3 py-4",
      )}
    >
      <div className="mb-6 flex items-center gap-2 px-2">
        <div className="size-8 rounded-md bg-[var(--color-accent-bg)] grid place-items-center">
          <div className="size-3 rounded-full bg-[var(--color-accent)]" />
        </div>
        <div>
          <div className="text-sm font-semibold leading-none">ContextNest</div>
          <div className="text-xs text-[var(--color-text-subtle)] leading-none mt-0.5">
            v0.2-ui
          </div>
        </div>
      </div>

      <nav className="flex flex-col gap-1">
        {NAV.map((item) => {
          const active = pathname === item.to;
          return (
            <Link
              key={item.to}
              to={item.to}
              className={cn(
                "flex items-center gap-3 rounded-md px-3 py-2 text-sm",
                "transition-colors duration-150",
                active
                  ? "bg-[var(--color-accent-bg)] text-[var(--color-accent)]"
                  : "text-[var(--color-text-muted)] hover:bg-[var(--color-bg-elevated)] hover:text-[var(--color-text)]",
              )}
            >
              {item.icon}
              <div className="flex flex-col leading-tight">
                <span className="font-medium">{item.label}</span>
                <span className="text-[10px] uppercase tracking-wider text-[var(--color-text-subtle)]">
                  {item.hint}
                </span>
              </div>
            </Link>
          );
        })}
      </nav>

      <div className="mt-auto px-2 pt-4 text-[10px] text-[var(--color-text-subtle)]">
        <a
          href="https://github.com/SourceShift/ContextNest"
          target="_blank"
          rel="noreferrer noopener"
          className="hover:text-[var(--color-text-muted)] underline-offset-4 hover:underline"
        >
          github.com/SourceShift/ContextNest
        </a>
      </div>
    </aside>
  );
}
