import { cn } from "@/lib/utils";
import { ButtonHTMLAttributes, forwardRef } from "react";

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: "default" | "secondary" | "ghost" | "danger";
  size?: "sm" | "md" | "icon";
}

const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant = "default", size = "md", ...props }, ref) => {
    return (
      <button
        ref={ref}
        className={cn(
          "inline-flex items-center justify-center gap-2 rounded-lg font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--accent)] disabled:pointer-events-none disabled:opacity-50",
          variant === "default" &&
            "bg-[var(--accent)] text-[var(--accent-fg)] hover:bg-[var(--accent-hover)]",
          variant === "secondary" &&
            "bg-[var(--bg-tertiary)] text-[var(--fg)] hover:bg-[var(--border)]",
          variant === "ghost" &&
            "bg-transparent text-[var(--fg-muted)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--fg)]",
          variant === "danger" && "bg-[var(--danger)] text-white hover:opacity-90",
          size === "sm" && "h-8 px-3 text-xs",
          size === "md" && "h-9 px-4 text-sm",
          size === "icon" && "h-9 w-9",
          className,
        )}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export { Button };
