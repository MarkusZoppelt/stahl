export interface Position {
  id: number;
  name: string | null;
  ticker: string | null;
  assetClass: string;
  amount: number;
  price: number;
  value: number;
  pnl: number | null;
  pnlPercent: number | null;
  dayChangePercent: number | null;
  averageCost: number | null;
  invested: number | null;
  purchases: Purchase[];
}

export interface Purchase {
  date: string | null;
  quantity: number;
  price: number | null;
  fees: number | null;
}

export interface PortfolioSummary {
  totalValue: number;
  totalInvested: number;
  totalPnl: number;
  pnlPercent: number;
  dayChange: number;
  dayChangePercent: number;
  cashValue: number;
  securitiesValue: number;
  allocation: Record<string, number>;
  positions: Position[];
}

export interface AllocationItem {
  assetClass: string;
  percent: number;
  value: number;
}

export interface TopMover {
  name: string;
  percent: number;
  pnl: number;
}

export interface Performance {
  totalValue: number;
  securitiesValue: number;
  cashValue: number;
  invested: number;
  unrealizedPnl: number;
  pnlPercent: number;
  dayPnl: number;
  dayPercent: number;
  topGainers: TopMover[];
  topLosers: TopMover[];
}

export type AppMode = "simple" | "workspace";
export type ThemeMode = "dark" | "light";

export interface AppConfig {
  version: number;
  currency: string;
  theme: ThemeMode;
  lastMode: AppMode;
  portfolioFile: string | null;
  workspaceDir: string | null;
  llmProviderUrl: string | null;
  llmApiKey: string | null;
  llmModel: string | null;
}

export interface Policy {
  version: string;
  name: string;
  baseCurrency: string;
  timeHorizonYears: number;
  riskProfile: "conservative" | "moderate" | "aggressive";
  constraints: {
    minimumCashMonths?: number;
    minimumCashAmount?: number;
    singlePositionLimitPercent?: number;
    assetClassLimitPercent?: number;
  };
  allocations: Array<{
    assetClass: string;
    targetPercent: number;
    tolerancePercent?: number;
  }>;
}

export interface ContextSummary {
  totalValue: number;
  cashValue: number;
  securitiesValue: number;
  cashPercent: number;
  positionCount: number;
  largestPosition: string | null;
  largestPositionPercent: number | null;
}

export interface AllocationContext {
  assetClass: string;
  value: number;
  percent: number;
  positionCount: number;
}

export interface PositionContext {
  name: string;
  ticker: string | null;
  assetClass: string;
  quantity: number;
  price: number | null;
  value: number;
  weightPercent: number;
  averageCost: number | null;
  invested: number | null;
  pnl: number | null;
  historicReturnPercent: number | null;
  dailyReturnPercent: number | null;
}

export interface PortfolioContext {
  generatedAt: string;
  currency: string;
  networkStatus: string;
  summary: ContextSummary;
  allocation: AllocationContext[];
  positions: PositionContext[];
  riskFlags: string[];
  dataQualityFlags: string[];
  followUpCommands: string[];
}

export type Severity = "Info" | "Warning" | "Critical";

export interface Finding {
  severity: Severity;
  category: string;
  message: string;
  detail: string | null;
}

export interface AllocationReview {
  assetClass: string;
  targetPercent: number;
  tolerancePercent: number | null;
  actualPercent: number;
  driftPercent: number;
  withinTolerance: boolean;
  value: number;
}

export interface ConstraintCheck {
  name: string;
  passed: boolean;
  message: string;
}

export interface DataQualityIssue {
  severity: Severity;
  position: string;
  issue: string;
}

export interface Review {
  portfolioValue: number;
  currency: string;
  policyName: string;
  findings: Finding[];
  allocations: AllocationReview[];
  constraintChecks: ConstraintCheck[];
  dataQuality: DataQualityIssue[];
  suggestedActions: string[];
}

export interface Trade {
  assetClass: string;
  action: string;
  amount: number;
  percentOfPortfolio: number;
}

export interface AllocationItemSim {
  assetClass: string;
  percent: number;
}

export interface RebalanceScenario {
  name: string;
  description: string;
  trades: Trade[];
  newAllocation: AllocationItemSim[];
  cashImpact: number;
}

export interface RebalanceSimulation {
  portfolioValue: number;
  currency: string;
  policyName: string;
  scenarios: RebalanceScenario[];
}

export type HealthStatus = "Ok" | "Warning" | "Error";

export interface HealthCheck {
  name: string;
  status: HealthStatus;
  message: string;
}

export interface WorkspaceHealth {
  dir: string;
  checks: HealthCheck[];
  issueCount: number;
  warningCount: number;
}

export type ValidationSeverity = "Error" | "Warning";

export interface ValidationIssue {
  severity: ValidationSeverity;
  position: string | null;
  message: string;
}

export interface ValidationReport {
  filePath: string;
  valid: boolean;
  positionCount: number;
  issues: ValidationIssue[];
  errorCount: number;
  warningCount: number;
}

export interface CreatePurchase {
  date: string | null;
  quantity: number;
  price: number | null;
  fees: number | null;
}

export interface CreatePositionRequest {
  name: string | null;
  ticker: string | null;
  assetClass: string;
  amount: number;
  purchases: CreatePurchase[];
}

export interface GeneratedDocument {
  filePath: string;
  content: string;
  dryRun: boolean;
}
