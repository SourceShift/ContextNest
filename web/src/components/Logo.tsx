import { cn } from '@/lib/cn';

export function Logo({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 24 24"
      className={cn('size-6', className)}
      aria-label="ContextNest"
    >
      <circle cx="12" cy="12" r="6" fill="currentColor" />
      <circle
        cx="12"
        cy="12"
        r="10"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeDasharray="2 3"
        opacity=".6"
      />
    </svg>
  );
}
