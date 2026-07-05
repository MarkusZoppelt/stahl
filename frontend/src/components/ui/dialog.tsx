import { X } from "lucide-react";
import { ReactNode, useEffect, useRef } from "react";

export interface DialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string;
  children: ReactNode;
}

export function Dialog({ open, onOpenChange, title, description, children }: DialogProps) {
  const overlayRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") onOpenChange(false);
    }
    if (open) {
      document.addEventListener("keydown", onKey);
      document.body.style.overflow = "hidden";
    }
    return () => {
      document.removeEventListener("keydown", onKey);
      document.body.style.overflow = "";
    };
  }, [open, onOpenChange]);

  if (!open) return null;

  return (
    <div
      ref={overlayRef}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4"
      onClick={(e) => {
        if (e.target === overlayRef.current) onOpenChange(false);
      }}
    >
      <div className="w-full max-w-lg rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6 shadow-xl">
        <div className="mb-4 flex items-start justify-between">
          <div>
            <h2 className="text-lg font-semibold text-[var(--fg)]">{title}</h2>
            {description && <p className="mt-1 text-sm text-[var(--fg-muted)]">{description}</p>}
          </div>
          <button
            onClick={() => onOpenChange(false)}
            className="rounded-md p-1 text-[var(--fg-muted)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--fg)]"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        {children}
      </div>
    </div>
  );
}
