import { cn } from "@/lib/utils";
import { HTMLAttributes, forwardRef } from "react";

export interface BadgeProps extends HTMLAttributes<HTMLSpanElement> {
  variant?: "default" | "success" | "danger" | "warning" | "info" | "purple";
}

const Badge = forwardRef<HTMLSpanElement, BadgeProps>(
  ({ className, variant = "default", ...props }, ref) => {
    return (
      <span
        ref={ref}
        className={cn(
          "inline-flex items-center rounded-full px-2 py-0.5 text-xs font-medium",
          variant === "default" && "bg-[var(--bg-tertiary)] text-[var(--fg)]",
          variant === "success" && "bg-[var(--success-bg)] text-[var(--success)]",
          variant === "danger" && "bg-[var(--danger-bg)] text-[var(--danger)]",
          variant === "warning" && "bg-[var(--warning-bg)] text-[var(--warning)]",
          variant === "info" && "bg-[var(--info-bg)] text-[var(--info)]",
          variant === "purple" && "bg-[var(--purple)]/10 text-[var(--purple)]",
          className,
        )}
        {...props}
      />
    );
  },
);
Badge.displayName = "Badge";

export { Badge };
