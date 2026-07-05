import stahlMark from "@/assets/stahl-mark.svg";
import { Layout, StatusBar, type View } from "@/components/Layout";
import {
  addPosition,
  clearRememberedPath,
  deletePosition,
  getAllocation,
  getConfig,
  getFilePath,
  getInitialFile,
  getPerformance,
  getPortfolioSummary,
  getPositions,
  getWorkspaceDir,
  loadPortfolio,
  loadSimpleFile,
  loadWorkspace,
  pickPortfolioFile,
  pickWorkspaceDirectory,
  refreshPrices,
  savePortfolio,
  updatePosition,
} from "@/lib/tauri";
import { Allocation } from "@/pages/Allocation";
import { Context } from "@/pages/Context";
import { Dashboard } from "@/pages/Dashboard";
import { PerformancePage } from "@/pages/Performance";
import { Policy } from "@/pages/Policy";
import { Positions } from "@/pages/Positions";
import { Review } from "@/pages/Review";
import { Settings } from "@/pages/Settings";
import { Documents } from "@/pages/Documents";
import { Simulate } from "@/pages/Simulate";
import { Validate } from "@/pages/Validate";
import { Welcome } from "@/pages/Welcome";
import { Workspace } from "@/pages/Workspace";
import type {
  AllocationItem,
  AppConfig,
  CreatePositionRequest,
  Performance,
  PortfolioSummary,
  Position,
} from "@/types";
import { useCallback, useEffect, useState } from "react";

function App() {
  const [view, setView] = useState<View>("dashboard");
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [filePath, setFilePath] = useState<string | null>(null);
  const [workspaceDir, setWorkspaceDir] = useState<string | null>(null);
  const [summary, setSummary] = useState<PortfolioSummary | null>(null);
  const [positions, setPositions] = useState<Position[]>([]);
  const [allocation, setAllocation] = useState<AllocationItem[]>([]);
  const [performance, setPerformance] = useState<Performance | null>(null);
  const [loading, setLoading] = useState(false);
  const [initialLoading, setInitialLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const isWorkspace = !!workspaceDir;
  const workspaceOnlyViews: View[] = ["workspace", "policy", "review", "simulate", "documents"];

  useEffect(() => {
    if (!isWorkspace && workspaceOnlyViews.includes(view)) {
      setView("dashboard");
    }
  }, [isWorkspace, view]);

  const refreshAll = useCallback(async () => {
    try {
      const [s, p, a, perf] = await Promise.all([
        getPortfolioSummary(),
        getPositions(),
        getAllocation(),
        getPerformance(),
      ]);
      setSummary(s);
      setPositions(p);
      setAllocation(a);
      setPerformance(perf);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const loadConfigAndAutoOpen = useCallback(async () => {
    try {
      const cfg = await getConfig();
      setConfig(cfg);
      document.documentElement.classList.toggle("light", cfg.theme === "light");

      const initial = await getInitialFile();
      if (initial) {
        setLoading(true);
        const s = await loadPortfolio(initial);
        setFilePath(initial);
        setSummary(s);
        await refreshAll();
        setLoading(false);
        setInitialLoading(false);
        return;
      }

      const currentFile = await getFilePath();
      const currentWorkspace = await getWorkspaceDir();
      if (currentFile || currentWorkspace) {
        setFilePath(currentFile);
        setWorkspaceDir(currentWorkspace);
        await refreshAll();
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setInitialLoading(false);
    }
  }, [refreshAll]);

  useEffect(() => {
    loadConfigAndAutoOpen();
  }, [loadConfigAndAutoOpen]);

  const handleOpenFile = async () => {
    setError(null);
    try {
      const path = await pickPortfolioFile();
      if (!path) return;
      setLoading(true);
      const s = await loadSimpleFile(path);
      setWorkspaceDir(null);
      setFilePath(path);
      setSummary(s);
      await refreshAll();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleOpenWorkspace = async () => {
    setError(null);
    try {
      const dir = await pickWorkspaceDirectory();
      if (!dir) return;
      await handleLoadWorkspace(dir);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleLoadWorkspace = async (dir: string) => {
    setLoading(true);
    try {
      const s = await loadWorkspace(dir);
      setFilePath(null);
      setWorkspaceDir(dir);
      setSummary(s);
      await refreshAll();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleRefresh = async () => {
    setLoading(true);
    try {
      const s = await refreshPrices();
      setSummary(s);
      await refreshAll();
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  const handleSave = async () => {
    try {
      await savePortfolio();
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      const meta = e.metaKey || e.ctrlKey;
      if (meta && e.key.toLowerCase() === "r") {
        e.preventDefault();
        handleRefresh();
      }
      if (meta && e.key.toLowerCase() === "o") {
        e.preventDefault();
        handleOpenFile();
      }
      if (meta && e.key.toLowerCase() === "s") {
        e.preventDefault();
        handleSave();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [handleRefresh, handleOpenFile, handleSave]);

  const handleUpdatePosition = async (id: number, data: Partial<Position>) => {
    try {
      await updatePosition(id, data);
      await refreshAll();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleAddPosition = async (request: CreatePositionRequest) => {
    try {
      await addPosition(request);
      await refreshAll();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleDeletePosition = async (id: number) => {
    try {
      await deletePosition(id);
      await refreshAll();
    } catch (e) {
      setError(String(e));
    }
  };

  const handleReset = async () => {
    try {
      await clearRememberedPath();
      setFilePath(null);
      setWorkspaceDir(null);
      setSummary(null);
      setPositions([]);
      setAllocation([]);
      setPerformance(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const hasData = !!filePath || !!workspaceDir;
  const currency = config?.currency ?? "EUR";

  if (initialLoading) {
    return (
      <div className="flex h-screen w-full items-center justify-center bg-[var(--bg)] text-[var(--fg)]">
        <div className="text-center">
          <img src={stahlMark} alt="" className="mx-auto mb-4 h-12 w-12 rounded-2xl" />
          <div className="mx-auto mb-4 h-8 w-8 animate-spin rounded-full border-2 border-[var(--border)] border-t-[var(--accent)]" />
          <p className="text-sm text-[var(--fg-muted)]">Loading Stahl Capital...</p>
        </div>
      </div>
    );
  }

  if (!hasData) {
    return (
      <div className="h-screen w-full bg-[var(--bg)] text-[var(--fg)]">
        <Welcome onOpenWorkspace={handleOpenWorkspace} onOpenFile={handleOpenFile} />
        {error && (
          <div className="absolute bottom-4 left-4 right-4 rounded-lg border border-[var(--danger)] bg-[var(--danger-bg)] px-4 py-3 text-sm text-[var(--danger)]">
            {error}
          </div>
        )}
      </div>
    );
  }

  return (
    <Layout
      active={view}
      onNavigate={setView}
      filePath={filePath}
      workspaceDir={workspaceDir}
      isWorkspace={isWorkspace}
      onOpenFile={handleOpenFile}
      onOpenWorkspace={handleOpenWorkspace}
      onSave={handleSave}
      statusBar={
        <StatusBar
          filePath={filePath}
          workspaceDir={workspaceDir}
          positionCount={positions.length}
          totalValue={summary?.totalValue ?? null}
          currency={config?.currency ?? "EUR"}
        />
      }
    >
      {loading && (
        <div className="fixed right-4 top-4 z-50 rounded-lg bg-[var(--accent)] px-3 py-1 text-xs font-medium text-[var(--accent-fg)]">
          Loading...
        </div>
      )}
      {error && (
        <div className="mb-4 rounded-lg border border-[var(--danger)] bg-[var(--danger-bg)] px-4 py-3 text-sm text-[var(--danger)]">
          {error}
        </div>
      )}

      <div className="mb-4 flex items-center justify-between">
        <div />
        <button
          onClick={handleRefresh}
          disabled={loading}
          className="rounded-lg bg-[var(--bg-tertiary)] px-3 py-1.5 text-sm font-medium text-[var(--fg)] hover:bg-[var(--border)] disabled:opacity-50"
        >
          Refresh Prices
        </button>
      </div>

      {view === "dashboard" && <Dashboard summary={summary} currency={currency} />}
      {view === "positions" && (
        <Positions
          positions={positions}
          currency={currency}
          onAdd={handleAddPosition}
          onUpdate={handleUpdatePosition}
          onDelete={handleDeletePosition}
        />
      )}
      {view === "allocation" && (
        <Allocation summary={summary} allocation={allocation} currency={currency} />
      )}
      {view === "performance" && <PerformancePage performance={performance} currency={currency} />}
      {view === "workspace" && (
        <Workspace workspaceDir={workspaceDir} onWorkspaceLoaded={handleLoadWorkspace} />
      )}
      {view === "policy" && <Policy workspaceDir={workspaceDir} />}
      {view === "context" && <Context />}
      {view === "review" && <Review />}
      {view === "simulate" && <Simulate />}
      {view === "validate" && <Validate />}
      {view === "documents" && <Documents workspaceDir={workspaceDir} />}
      {view === "settings" && <Settings onReset={handleReset} />}
    </Layout>
  );
}

export default App;
