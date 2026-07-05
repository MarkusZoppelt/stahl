import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AllocationItem,
  AppConfig,
  CreatePositionRequest,
  GeneratedDocument,
  Performance,
  Policy,
  PortfolioContext,
  PortfolioSummary,
  Position,
  RebalanceSimulation,
  Review,
  ValidationReport,
  WorkspaceHealth,
} from "@/types";

export async function loadPortfolio(path: string): Promise<PortfolioSummary> {
  return invoke("load_portfolio", { path });
}

export async function getPortfolioSummary(): Promise<PortfolioSummary> {
  return invoke("get_portfolio_summary");
}

export async function getPositions(): Promise<Position[]> {
  return invoke("get_positions");
}

export async function getAllocation(): Promise<AllocationItem[]> {
  return invoke("get_allocation");
}

export async function getPerformance(): Promise<Performance> {
  return invoke("get_performance");
}

export async function refreshPrices(): Promise<PortfolioSummary> {
  return invoke("refresh_prices");
}

export async function addPosition(request: CreatePositionRequest): Promise<Position> {
  return invoke("add_position", { request });
}

export async function updatePosition(id: number, request: Partial<Position>): Promise<Position> {
  return invoke("update_position", { id, request });
}

export async function deletePosition(id: number): Promise<void> {
  return invoke("delete_position", { id });
}

export async function savePortfolio(): Promise<void> {
  return invoke("save_portfolio");
}

export async function getFilePath(): Promise<string | null> {
  return invoke("get_file_path");
}

export async function getInitialFile(): Promise<string | null> {
  return invoke("get_initial_file");
}

export async function pickPortfolioFile(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: false,
    filters: [
      { name: "Portfolio JSON", extensions: ["json", "gpg"] },
      { name: "All Files", extensions: ["*"] },
    ],
  });
  return selected as string | null;
}

export async function pickWorkspaceDirectory(): Promise<string | null> {
  const selected = await open({
    multiple: false,
    directory: true,
  });
  return selected as string | null;
}

export async function loadWorkspace(dir: string): Promise<PortfolioSummary> {
  return invoke("load_workspace", { dir });
}

export async function loadSimpleFile(path: string): Promise<PortfolioSummary> {
  return invoke("load_simple_file", { path });
}

export async function initWorkspace(dir: string, dryRun = false): Promise<void> {
  return invoke("init_workspace", { dir, dryRun });
}

export async function initAgentFiles(dir: string, dryRun = false): Promise<void> {
  return invoke("init_agent_files", { dir, dryRun });
}

export async function exportSkill(dir: string, dryRun = false): Promise<void> {
  return invoke("export_skill", { dir, dryRun });
}

export async function getWorkspaceDir(): Promise<string | null> {
  return invoke("get_workspace_dir");
}

export async function loadPolicy(path?: string): Promise<Policy> {
  return invoke("load_policy", { path });
}

export async function getPolicy(): Promise<Policy | null> {
  return invoke("get_policy");
}

export async function setPolicyFromToml(toml: string): Promise<Policy> {
  return invoke("set_policy_from_toml", { toml });
}

export async function savePolicy(toml: string): Promise<Policy> {
  return invoke("save_policy", { toml });
}

export async function generatePolicyFromStrategy(strategy: string): Promise<Policy> {
  return invoke("generate_policy_from_strategy", { strategy });
}

export async function getContext(): Promise<PortfolioContext> {
  return invoke("get_context");
}

export async function getReview(): Promise<Review> {
  return invoke("get_review");
}

export async function getSimulation(): Promise<RebalanceSimulation> {
  return invoke("get_simulation");
}

export async function runDoctor(dir?: string): Promise<WorkspaceHealth> {
  return invoke("run_doctor", { dir });
}

export async function validatePortfolio(): Promise<ValidationReport> {
  return invoke("validate_portfolio");
}

export async function generateReport(dir?: string, dryRun = false): Promise<GeneratedDocument> {
  return invoke("generate_report", { dir, dryRun });
}

export async function draftDecision(
  title?: string,
  dir?: string,
  dryRun = false,
): Promise<GeneratedDocument> {
  return invoke("draft_decision", { title, dir, dryRun });
}

export async function getConfig(): Promise<AppConfig> {
  return invoke("get_config");
}

export async function setCurrency(currency: string): Promise<void> {
  return invoke("set_currency", { currency });
}

export async function setTheme(theme: "dark" | "light"): Promise<void> {
  return invoke("set_theme", { theme });
}

export async function setLlmConfig(
  providerUrl?: string,
  apiKey?: string,
  model?: string,
): Promise<void> {
  return invoke("set_llm_config", { providerUrl, apiKey, model });
}

export async function clearRememberedPath(): Promise<void> {
  return invoke("clear_remembered_path");
}

export async function getConfigPath(): Promise<string> {
  return invoke("get_config_path");
}
