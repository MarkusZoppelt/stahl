import stahlMark from "@/assets/stahl-mark.svg";
import { Button } from "@/components/ui/button";
import { ThemeToggle } from "@/components/ThemeToggle";
import {
  BarChart3,
  Briefcase,
  CheckCircle,
  ClipboardList,
  FileText,
  LayoutDashboard,
  PieChart,
  Settings,
  Stethoscope,
  TrendingUp,
} from "lucide-react";
import { ReactNode } from "react";

export type View =
  | "dashboard"
  | "positions"
  | "allocation"
  | "performance"
  | "workspace"
  | "policy"
  | "context"
  | "review"
  | "simulate"
  | "validate"
  | "documents"
  | "settings";

interface NavItem {
  id: View;
  label: string;
  icon: React.ElementType;
}

const coreNavItems: NavItem[] = [
  { id: "dashboard", label: "Dashboard", icon: LayoutDashboard },
  { id: "positions", label: "Positions", icon: BarChart3 },
  { id: "allocation", label: "Allocation", icon: PieChart },
  { id: "performance", label: "Performance", icon: TrendingUp },
  { id: "context", label: "Context", icon: ClipboardList },
  { id: "validate", label: "Validate", icon: CheckCircle },
  { id: "settings", label: "Settings", icon: Settings },
];

const workspaceNavItems: NavItem[] = [
  { id: "workspace", label: "Workspace", icon: Briefcase },
  { id: "policy", label: "Policy", icon: FileText },
  { id: "review", label: "Review", icon: Stethoscope },
  { id: "simulate", label: "Simulate", icon: TrendingUp },
  { id: "documents", label: "Documents", icon: FileText },
];

interface LayoutProps {
  active: View;
  onNavigate: (view: View) => void;
  filePath: string | null;
  workspaceDir: string | null;
  isWorkspace: boolean;
  onOpenFile: () => void;
  onOpenWorkspace: () => void;
  onSave: () => void;
  children: ReactNode;
  statusBar: ReactNode;
}

export function Layout({
  active,
  onNavigate,
  filePath,
  workspaceDir,
  isWorkspace,
  onOpenFile,
  onOpenWorkspace,
  onSave,
  children,
  statusBar,
}: LayoutProps) {
  const renderNavItem = (item: NavItem) => {
    const Icon = item.icon;
    const isActive = active === item.id;
    return (
      <button
        key={item.id}
        onClick={() => onNavigate(item.id)}
        className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
          isActive
            ? "bg-[var(--accent)] text-[var(--accent-fg)]"
            : "text-[var(--fg-muted)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--fg)]"
        }`}
      >
        <Icon className="h-4 w-4" />
        {item.label}
      </button>
    );
  };

  return (
    <div className="flex h-screen w-full overflow-hidden bg-[var(--bg)] text-[var(--fg)]">
      {/* Sidebar */}
      <aside className="flex w-56 flex-col border-r border-[var(--border)] bg-[var(--bg-secondary)]">
        <div className="flex items-center gap-3 px-5 py-4">
          <img src={stahlMark} alt="" className="h-8 w-8 rounded-lg" />
          <span className="font-semibold tracking-tight">Stahl Capital</span>
        </div>

        <nav className="flex-1 space-y-1 px-3 py-2">
          {coreNavItems.map(renderNavItem)}

          {isWorkspace ? (
            <>
              <div className="my-2 border-t border-[var(--border)]" />
              {workspaceNavItems.map(renderNavItem)}
            </>
          ) : (
            <div className="mt-4 rounded-lg border border-[var(--border)] bg-[var(--bg-tertiary)]/50 px-3 py-2">
              <p className="text-[10px] font-semibold uppercase tracking-wider text-[var(--fg-subtle)]">
                Workspace features
              </p>
              <p className="mt-1 text-xs leading-relaxed text-[var(--fg-muted)]">
                Open a workspace to unlock Policy, Review, Simulate, and Documents.
              </p>
            </div>
          )}
        </nav>

        <div className="border-t border-[var(--border)] p-3 space-y-2">
          <Button variant="secondary" size="sm" className="w-full" onClick={onOpenWorkspace}>
            Open Workspace
          </Button>
          <Button variant="secondary" size="sm" className="w-full" onClick={onOpenFile}>
            Open Portfolio File
          </Button>
          <Button variant="secondary" size="sm" className="w-full" onClick={onSave}>
            Save
          </Button>
          {(filePath || workspaceDir) && (
            <p
              className="truncate px-1 text-[10px] text-[var(--fg-subtle)]"
              title={workspaceDir ?? filePath ?? undefined}
            >
              {workspaceDir
                ? `Workspace: ${workspaceDir.split("/").pop()}`
                : filePath?.split("/").pop()}
            </p>
          )}
          <div className="flex justify-end pt-1">
            <ThemeToggle />
          </div>
        </div>
      </aside>

      {/* Main content */}
      <div className="flex flex-1 flex-col overflow-hidden">
        <main className="flex-1 overflow-auto p-6">{children}</main>
        {statusBar}
      </div>
    </div>
  );
}

export function StatusBar({
  filePath,
  workspaceDir,
  positionCount,
  totalValue,
  currency,
}: {
  filePath: string | null;
  workspaceDir: string | null;
  positionCount: number;
  totalValue: number | null;
  currency: string;
}) {
  function formatCurrency(value: number, currencyCode = "EUR"): string {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: currencyCode,
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(value);
  }

  return (
    <div className="flex items-center justify-between border-t border-[var(--border)] bg-[var(--bg-secondary)] px-4 py-1.5 text-xs text-[var(--fg-muted)]">
      <span className="truncate max-w-md" title={workspaceDir ?? filePath ?? undefined}>
        {workspaceDir ?? filePath ?? "No portfolio loaded"}
      </span>
      <div className="flex items-center gap-4">
        <span>{positionCount} positions</span>
        {totalValue !== null && <span>{formatCurrency(totalValue, currency)}</span>}
      </div>
    </div>
  );
}
