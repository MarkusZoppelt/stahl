import { cn } from "@/lib/utils";
import { InputHTMLAttributes, forwardRef } from "react";

const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          "flex h-9 w-full rounded-lg border border-[var(--border)] bg-[var(--bg)] px-3 py-1 text-sm text-[var(--fg)] shadow-sm transition-colors",
          "placeholder:text-[var(--fg-subtle)]",
          "focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent)]",
          "disabled:cursor-not-allowed disabled:opacity-50",
          className,
        )}
        ref={ref}
        {...props}
      />
    );
  },
);
Input.displayName = "Input";

export { Input };
