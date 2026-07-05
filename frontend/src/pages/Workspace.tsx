import { Button } from "@/components/ui/button";
import {
  exportSkill,
  initAgentFiles,
  initWorkspace,
  pickWorkspaceDirectory,
  runDoctor,
} from "@/lib/tauri";
import type { WorkspaceHealth } from "@/types";
import { FolderOpen, RefreshCw, Shield } from "lucide-react";
import { useEffect, useState } from "react";

interface WorkspaceProps {
  workspaceDir: string | null;
  onWorkspaceLoaded: (dir: string) => Promise<void>;
}

export function Workspace({ workspaceDir, onWorkspaceLoaded }: WorkspaceProps) {
  const [health, setHealth] = useState<WorkspaceHealth | null>(null);
  const [loading, setLoading] = useState(false);
  const [message, setMessage] = useState<string | null>(null);

  useEffect(() => {
    if (workspaceDir) {
      handleDoctor();
    }
  }, [workspaceDir]);

  const handleOpenWorkspace = async () => {
    try {
      const dir = await pickWorkspaceDirectory();
      if (!dir) return;
      await onWorkspaceLoaded(dir);
    } catch (e) {
      setMessage(String(e));
    }
  };

  const handleInitWorkspace = async () => {
    const dir = await pickWorkspaceDirectory();
    if (!dir) return;
    setLoading(true);
    try {
      await initWorkspace(dir);
      await initAgentFiles(dir);
      setMessage(`Workspace initialized at ${dir}`);
      await onWorkspaceLoaded(dir);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleDoctor = async () => {
    if (!workspaceDir) return;
    setLoading(true);
    try {
      const h = await runDoctor(workspaceDir);
      setHealth(h);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleExportSkill = async () => {
    const dir = await pickWorkspaceDirectory();
    if (!dir) return;
    setLoading(true);
    try {
      await exportSkill(dir);
      setMessage(`Skill exported to ${dir}/portfolio-rs-finance/SKILL.md`);
    } catch (e) {
      setMessage(String(e));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-6">
      <div className="flex items-center justify-between">
        <h1 className="text-2xl font-semibold">Workspace</h1>
        <Button onClick={handleOpenWorkspace} disabled={loading}>
          <FolderOpen className="mr-2 h-4 w-4" />
          Open Workspace
        </Button>
      </div>

      {workspaceDir ? (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
          <p className="text-sm text-[var(--fg-muted)]">Current workspace</p>
          <p className="mt-1 font-mono text-sm">{workspaceDir}</p>
        </div>
      ) : (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
          <p className="text-[var(--fg-muted)]">
            No workspace loaded. Open an existing workspace or create a new one.
          </p>
          <div className="mt-4 flex gap-3">
            <Button onClick={handleInitWorkspace} disabled={loading}>
              Create New Workspace
            </Button>
            <Button variant="secondary" onClick={handleExportSkill} disabled={loading}>
              <Shield className="mr-2 h-4 w-4" />
              Export Agent Skill
            </Button>
          </div>
        </div>
      )}

      {health && (
        <div className="rounded-xl border border-[var(--border)] bg-[var(--bg-secondary)] p-6">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="text-lg font-medium">Health Check</h2>
            <Button variant="secondary" size="sm" onClick={handleDoctor} disabled={loading}>
              <RefreshCw className="mr-2 h-4 w-4" />
              Refresh
            </Button>
          </div>
          <div className="space-y-2">
            {health.checks.map((check, i) => (
              <div
                key={i}
                className={`flex items-start gap-3 rounded-lg border border-[var(--border)] p-3 ${
                  check.status === "Ok"
                    ? "bg-[var(--success-bg)]"
                    : check.status === "Warning"
                      ? "bg-[var(--warning-bg)]"
                      : "bg-[var(--danger-bg)]"
                }`}
              >
                <span className="text-lg">
                  {check.status === "Ok" ? "✅" : check.status === "Warning" ? "⚠️" : "❌"}
                </span>
                <div>
                  <p className="text-sm font-medium">{check.name}</p>
                  <p className="text-xs text-[var(--fg-muted)]">{check.message}</p>
                </div>
              </div>
            ))}
          </div>
          {(health.issueCount > 0 || health.warningCount > 0) && (
            <p className="mt-4 text-sm text-[var(--fg-muted)]">
              {health.issueCount} issues, {health.warningCount} warnings
            </p>
          )}
        </div>
      )}

      {message && (
        <div className="rounded-lg border border-[var(--border)] bg-[var(--bg-tertiary)] px-4 py-3 text-sm">
          {message}
        </div>
      )}
    </div>
  );
}
