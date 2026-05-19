import { forwardRef, type InputHTMLAttributes, type TextareaHTMLAttributes } from "react";
import { cn } from "@/lib/cn";

const baseFieldStyles = cn(
  "w-full rounded-md border border-[var(--color-border)]",
  "bg-[var(--color-bg)] text-[var(--color-text)]",
  "px-3 py-2 text-sm",
  "placeholder:text-[var(--color-text-subtle)]",
  "focus:border-[var(--color-accent)] focus:outline-none",
  "disabled:opacity-50",
);

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function Input({ className, ...rest }, ref) {
    return (
      <input
        ref={ref}
        className={cn(baseFieldStyles, "h-9", className)}
        {...rest}
      />
    );
  },
);

export const Textarea = forwardRef<
  HTMLTextAreaElement,
  TextareaHTMLAttributes<HTMLTextAreaElement>
>(function Textarea({ className, ...rest }, ref) {
  return (
    <textarea
      ref={ref}
      className={cn(baseFieldStyles, "min-h-24 resize-y leading-relaxed", className)}
      {...rest}
    />
  );
});

export function Label({
  children,
  htmlFor,
  className,
}: {
  children: React.ReactNode;
  htmlFor?: string;
  className?: string;
}) {
  return (
    <label
      htmlFor={htmlFor}
      className={cn(
        "mb-1 block text-xs font-medium text-[var(--color-text-muted)] uppercase tracking-wider",
        className,
      )}
    >
      {children}
    </label>
  );
}
