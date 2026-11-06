//! The bridge between the UI and `portfolio_rs`.
//!
//! `portfolio_rs` is async on Tokio, while GPUI runs its own executors. Every
//! call therefore runs on a Tokio runtime owned by `main`, and the UI awaits
//! the result through [`Backend::run`]. Almost all domain logic (portfolio
//! math, policy review, simulation, reports) lives in `portfolio_rs`.

use std::{
    fmt::Display,
    fs,
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

use chrono::{Datelike as _, Duration, Local, NaiveDate};
use portfolio_rs::{
    Portfolio, PortfolioPosition, agent_skill,
    config::{AppConfig, AppMode, ThemeMode},
    config_path,
    context::PortfolioContext,
    doctor::WorkspaceHealth,
    mcp::{
        analytics,
        dto::{Finding, Transaction},
        performance,
        snapshots::{self, Snapshot as HistoryPoint},
    },
    policy::{self, Policy},
    review::Review,
    simulate::RebalanceSimulation,
    state::{
        AllocationItemDto, AppConfigDto, AppState, CreatePositionRequest, GeneratedDocument,
        PerformanceDto, PortfolioSummaryDto, PositionDto, UpdatePositionRequest,
    },
    validate::ValidationReport,
    workspace,
};
use serde::Deserialize;
use serde_json::{Error as JsonError, from_value, to_value};
use tokio::{runtime::Handle, sync::RwLock};

pub type BackendResult<T> = Result<T, String>;

fn report<E: Display>(error: E) -> String {
    // `{:#}` includes the whole eyre context chain.
    format!("{error:#}")
}

/// The id `portfolio_rs snapshot` records under by default, so history made
/// here and on the command line is the same history.
const HISTORY_ID: &str = "main";

/// Everything the UI shows about the loaded portfolio, read together.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub summary: PortfolioSummaryDto,
    pub positions: Vec<PositionDto>,
    pub allocation: Vec<AllocationItemDto>,
    pub performance: PerformanceDto,
    pub findings: Vec<Finding>,
    /// connected | partial | offline | fixture
    pub network: String,
}

/// Which portfolio is open, as opposed to its contents.
#[derive(Clone, Debug, Default)]
pub struct OpenDocument {
    pub file_path: Option<String>,
    pub workspace_dir: Option<String>,
    /// The file is open but does not parse. It is never treated as empty.
    pub parse_error: Option<ParseProblem>,
}

impl OpenDocument {
    pub fn is_open(&self) -> bool {
        self.file_path.is_some() || self.workspace_dir.is_some()
    }
}

/// Where a positions file stops being valid.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseProblem {
    pub file: String,
    pub reason: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Period {
    Week,
    #[default]
    Month,
    Ytd,
    Year,
    All,
}

impl Period {
    pub const ALL: [Period; 5] = [
        Period::Week,
        Period::Month,
        Period::Ytd,
        Period::Year,
        Period::All,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Period::Week => "1W",
            Period::Month => "1M",
            Period::Ytd => "YTD",
            Period::Year => "1Y",
            Period::All => "All",
        }
    }

    /// The first day of the period, or `None` for all of history.
    fn start(self, today: NaiveDate) -> Option<NaiveDate> {
        match self {
            Period::Week => Some(today - Duration::days(7)),
            Period::Month => Some(today - Duration::days(30)),
            Period::Ytd => NaiveDate::from_ymd_opt(today.year(), 1, 1),
            Period::Year => Some(today - Duration::days(365)),
            Period::All => None,
        }
    }
}

/// Portfolio value over a period, from stored daily snapshots and today.
#[derive(Clone, Debug, Default)]
pub struct History {
    pub points: Vec<HistoryValue>,
    pub change: Option<f64>,
    pub net_contributions: Option<f64>,
    pub market_effect: Option<f64>,
    /// Time-weighted, so deposits and withdrawals do not count as returns.
    pub twr_percent: Option<f64>,
    /// Today's valuation is written here, unless the portfolio is encrypted.
    pub path: Option<String>,
}

#[derive(Clone, Debug)]
pub struct HistoryValue {
    pub date: String,
    pub value: f64,
}

/// The parts of a [`PortfolioContext`] the Briefing page shows. Its fields are
/// private, so they are read from its serialized form, which is its public
/// contract with agents and GUIs.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Briefing {
    pub currency: String,
    pub summary: BriefingSummary,
    pub allocation: Vec<BriefingAllocation>,
    pub positions: Vec<BriefingPosition>,
    pub risk_flags: Vec<String>,
    pub data_quality_flags: Vec<String>,
    pub cash_buffer: BriefingCash,
    #[serde(skip)]
    pub markdown: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefingSummary {
    pub total_value: f64,
    pub cash_value: f64,
    pub securities_value: f64,
    pub position_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefingAllocation {
    pub asset_class: String,
    pub value: Option<f64>,
    pub percent: Option<f64>,
    pub position_count: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BriefingPosition {
    pub name: String,
    pub asset_class: String,
    pub value: Option<f64>,
    pub weight_percent: Option<f64>,
    pub daily_return_percent: Option<f64>,
    pub is_stale: bool,
}

/// Cash against the policy's minimum. `surplus` is negative for a shortfall.
#[derive(Debug, Deserialize)]
pub struct BriefingCash {
    pub minimum: Option<f64>,
    pub surplus: Option<f64>,
}

impl Briefing {
    fn from_context(context: &PortfolioContext) -> Result<Self, JsonError> {
        let mut briefing: Self = from_value(to_value(context)?)?;
        briefing.markdown = context.to_markdown();
        Ok(briefing)
    }
}

/// Reduce valuations to the ones a period reports on: the last usable one
/// before it starts as the baseline, then everything inside it.
fn points_in_period(points: Vec<HistoryPoint>, start: Option<NaiveDate>) -> Vec<HistoryPoint> {
    let Some(start) = start else {
        return points;
    };
    let start = start.format("%Y-%m-%d").to_string();
    let first_inside = points.partition_point(|point| point.date < start);
    points[first_inside.saturating_sub(1)..].to_vec()
}

/// Turn valuations (oldest first) into what the UI shows. Incomplete ones are
/// skipped: a missing price would read as a sudden loss.
fn build_history(
    points: Vec<HistoryPoint>,
    transactions: &[Transaction],
    path: Option<String>,
) -> History {
    let points: Vec<HistoryPoint> = points.into_iter().filter(|point| point.complete).collect();
    let (twr_percent, _) = performance::twr(&points, transactions);
    let effects = match (points.first(), points.last()) {
        (Some(first), Some(last)) if points.len() >= 2 => {
            performance::position_effects(first, last, transactions)
                .map(|effects| performance::total(&effects))
        }
        _ => None,
    };
    History {
        change: match (points.first(), points.last()) {
            (Some(first), Some(last)) if points.len() >= 2 => {
                Some(last.total_base - first.total_base)
            }
            _ => None,
        },
        net_contributions: effects.as_ref().map(|effects| effects.net_contributions),
        market_effect: effects.as_ref().map(|effects| effects.market_effect),
        twr_percent,
        path,
        points: points
            .into_iter()
            .map(|point| HistoryValue {
                value: point.total_base,
                date: point.date,
            })
            .collect(),
    }
}

#[derive(Clone)]
pub struct Backend {
    state: Arc<AppState>,
    config: Arc<RwLock<AppConfig>>,
    /// Where to persist the config. `None` is the user's default location.
    config_path: Option<PathBuf>,
    runtime: Handle,
}

impl Backend {
    /// Load the user's config, falling back to defaults if it is unreadable.
    pub fn load(runtime: Handle) -> Self {
        let config = AppConfig::load().unwrap_or_else(|error| {
            eprintln!("Warning: failed to load config: {error:#}");
            AppConfig::default()
        });
        let state = AppState::new(config.currency.clone());
        Self::new(state, config, None, runtime)
    }

    pub fn new(
        state: AppState,
        config: AppConfig,
        config_path: Option<PathBuf>,
        runtime: Handle,
    ) -> Self {
        Self {
            state: Arc::new(state),
            config: Arc::new(RwLock::new(config)),
            config_path,
            runtime,
        }
    }

    /// Run `work` on the Tokio runtime, from any executor.
    pub fn run<T, F>(&self, work: F) -> impl Future<Output = BackendResult<T>> + use<T, F>
    where
        T: Send + 'static,
        F: Future<Output = BackendResult<T>> + Send + 'static,
    {
        let task = self.runtime.spawn(work);
        async move {
            task.await
                .map_err(|error| format!("background task failed: {error}"))?
        }
    }

    /// Run something on the Tokio runtime and forget about it.
    pub fn spawn(&self, work: impl Future<Output = ()> + Send + 'static) {
        self.runtime.spawn(work);
    }

    async fn save_config(&self) -> BackendResult<()> {
        // Both paths write the file owner-only, because it can hold an API key.
        let config = self.config.read().await;
        match &self.config_path {
            Some(path) => config.save_to(path),
            None => config.save(),
        }
        .map_err(report)
    }

    // --- Opening a portfolio ---

    /// Reopen what was open last time, or the file named on the command line.
    pub async fn startup(&self, initial_file: Option<String>) -> BackendResult<OpenDocument> {
        if let Some(path) = initial_file {
            self.state.load_file(&path).await.map_err(report)?;
        } else {
            let config = self.config.read().await.clone();
            self.auto_load(&config).await;
        }
        Ok(self.open_document().await)
    }

    /// Failures here only warn: a moved file should land on the welcome
    /// screen, not on an error.
    async fn auto_load(&self, config: &AppConfig) {
        if config.last_mode == AppMode::Workspace {
            let Some(dir) = &config.workspace_dir else {
                return;
            };
            let positions = Path::new(dir).join("positions.json");
            if !positions.exists() {
                return;
            }
            if let Err(error) = self.state.load_file(&positions.to_string_lossy()).await {
                eprintln!("Warning: failed to auto-load workspace portfolio: {error:#}");
                return;
            }
            *self.state.workspace_dir.write().await = Some(dir.clone());
            if let Err(error) = self.state.load_policy(None).await {
                eprintln!("Warning: failed to auto-load workspace policy: {error:#}");
            }
        } else if let Some(file) = config.effective_portfolio_file()
            && Path::new(&file).exists()
            && let Err(error) = self.state.load_file(&file).await
        {
            eprintln!("Warning: failed to auto-load portfolio: {error:#}");
        }
    }

    pub async fn open_document(&self) -> OpenDocument {
        OpenDocument {
            file_path: self.state.file_path().await,
            workspace_dir: self.state.workspace_dir().await,
            parse_error: self
                .state
                .positions_error()
                .await
                .map(|error| ParseProblem {
                    file: error.file,
                    reason: error.reason,
                    line: error.line,
                    column: error.column,
                }),
        }
    }

    /// Read the open file again, for after fixing it in another editor.
    pub async fn reload(&self) -> BackendResult<()> {
        let Some(path) = self.state.file_path().await else {
            return Ok(());
        };
        let workspace = self.state.workspace_dir().await;
        self.state.load_file(&path).await.map_err(report)?;
        // `load_file` leaves workspace mode, so restore it.
        *self.state.workspace_dir.write().await = workspace;
        Ok(())
    }

    pub async fn load_workspace(&self, dir: String) -> BackendResult<()> {
        {
            let mut config = self.config.write().await;
            self.state
                .load_workspace(&dir, &mut config)
                .await
                .map_err(report)?;
        }
        self.save_config().await
    }

    pub async fn load_simple_file(&self, path: String) -> BackendResult<()> {
        {
            let mut config = self.config.write().await;
            self.state
                .load_simple_file(&path, &mut config)
                .await
                .map_err(report)?;
        }
        self.save_config().await
    }

    pub fn init_workspace(&self, dir: &str) -> BackendResult<()> {
        workspace::init_workspace(dir, false).map_err(report)?;
        workspace::init_agent_files(dir, false).map_err(report)
    }

    pub fn export_skill(&self, dir: &str) -> BackendResult<()> {
        agent_skill::skill_export(dir, false).map_err(report)
    }

    // --- Portfolio ---

    pub async fn snapshot(&self) -> Snapshot {
        let portfolio = self.state.portfolio().await;
        Snapshot {
            summary: self.state.get_portfolio_summary().await,
            positions: self.state.get_positions().await,
            allocation: self.state.get_allocation().await,
            performance: self.state.get_performance().await,
            findings: analytics::data_quality(&portfolio),
            network: analytics::network_status(&portfolio).to_string(),
        }
    }

    /// Where this portfolio's daily snapshots live, as for `portfolio_rs
    /// snapshot`. Respects `PORTFOLIO_RS_DATA_DIR`.
    async fn history_path(&self) -> Option<PathBuf> {
        let workspace = self.state.workspace_dir().await;
        snapshots::data_dir(None, workspace.as_deref().map(Path::new))
            .map(|dir| snapshots::snapshots_path(&dir, HISTORY_ID))
    }

    /// Record today's valuation, so history builds up just by using the app.
    ///
    /// Skipped for files that do not parse or are not fully priced, and for
    /// encrypted portfolios, whose values must not land on disk in plaintext.
    pub async fn record_today(&self) -> BackendResult<()> {
        let encrypted = self
            .state
            .file_path()
            .await
            .is_none_or(|path| path.ends_with(".gpg"));
        if encrypted || self.state.positions_error().await.is_some() {
            return Ok(());
        }
        let portfolio = self.state.portfolio().await;
        let snapshot = self.today_point(&portfolio).await;
        if !snapshot.complete {
            return Ok(());
        }
        if let Some(path) = self.history_path().await {
            snapshots::upsert(&path, &snapshot).map_err(report)?;
        }
        Ok(())
    }

    /// History is a convenience: failing to record must not block the app.
    pub async fn record_today_or_warn(&self) {
        if let Err(error) = self.record_today().await {
            eprintln!("Warning: failed to record today's valuation: {error}");
        }
    }

    async fn today_point(&self, portfolio: &Portfolio) -> HistoryPoint {
        let base = self.state.currency().await;
        let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let stale = portfolio.positions.iter().any(PortfolioPosition::is_stale);
        snapshots::from_portfolio(portfolio, &base, &today, stale)
    }

    pub async fn history(&self, period: Period) -> BackendResult<History> {
        let path = self.history_path().await;
        let (mut points, _skipped) = path.as_deref().map(snapshots::read).unwrap_or_default();

        // Stored snapshots may predate a change of base currency; those totals
        // are not comparable, so history stops there.
        let portfolio = self.state.portfolio().await;
        let today = self.today_point(&portfolio).await;
        points
            .retain(|point| point.base_currency == today.base_currency && point.date < today.date);
        points.push(today);

        let start = period.start(Local::now().date_naive());
        let base = self.state.currency().await;
        let transactions = analytics::transactions(&portfolio, &base, None);
        Ok(build_history(
            points_in_period(points, start),
            &transactions,
            path.map(|path| path.to_string_lossy().into_owned()),
        ))
    }

    pub async fn refresh_prices(&self) {
        self.state.refresh_prices().await;
    }

    pub async fn add_position(&self, request: CreatePositionRequest) -> BackendResult<()> {
        self.state
            .add_position(request)
            .await
            .map(drop)
            .map_err(report)
    }

    pub async fn update_position(
        &self,
        id: usize,
        request: UpdatePositionRequest,
    ) -> BackendResult<()> {
        self.state
            .update_position(id, request)
            .await
            .map(drop)
            .map_err(report)
    }

    pub async fn delete_position(&self, id: usize) -> BackendResult<()> {
        self.state.delete_position(id).await.map_err(report)
    }

    pub async fn save_portfolio(&self) -> BackendResult<()> {
        self.state.save().await.map_err(report)
    }

    // --- Policy ---

    pub async fn policy(&self) -> Option<Policy> {
        self.state.policy().await
    }

    pub async fn load_policy(&self) -> BackendResult<Policy> {
        self.state.load_policy(None).await.map_err(report)?;
        self.state
            .policy()
            .await
            .ok_or_else(|| "policy not loaded".to_string())
    }

    /// Validate `toml` and make it the active policy.
    pub async fn set_policy_from_toml(&self, toml: &str) -> BackendResult<Policy> {
        self.state
            .set_policy_from_toml(toml)
            .await
            .map_err(report)?;
        self.state
            .policy()
            .await
            .ok_or_else(|| "policy not set".to_string())
    }

    /// [`Self::set_policy_from_toml`], then write it into the workspace.
    pub async fn save_policy(&self, toml: String) -> BackendResult<Policy> {
        let policy = self.set_policy_from_toml(&toml).await?;
        let path = self
            .state
            .policy_path()
            .await
            .ok_or_else(|| "no workspace loaded to save policy into".to_string())?;
        fs::write(&path, toml).map_err(|error| format!("failed to write policy file: {error}"))?;
        Ok(policy)
    }

    pub fn policy_from_strategy(strategy: &str) -> BackendResult<Policy> {
        policy::policy_from_strategy(strategy)
            .ok_or_else(|| format!("unknown strategy: {strategy}"))
    }

    pub fn policy_to_toml(policy: &Policy) -> BackendResult<String> {
        policy.to_toml().map_err(report)
    }

    // --- Analysis ---

    pub async fn briefing(&self) -> BackendResult<Briefing> {
        let context = self.state.get_context().await.map_err(report)?;
        Briefing::from_context(&context).map_err(report)
    }

    pub async fn review(&self) -> BackendResult<Review> {
        self.state.get_review().await.map_err(report)
    }

    pub async fn simulation(&self) -> BackendResult<RebalanceSimulation> {
        self.state.get_simulation().await.map_err(report)
    }

    pub async fn doctor(&self, dir: String) -> BackendResult<WorkspaceHealth> {
        self.state.run_doctor(Some(dir)).await.map_err(report)
    }

    pub async fn validate(&self) -> BackendResult<ValidationReport> {
        self.state.validate_portfolio().await.map_err(report)
    }

    pub async fn generate_report(
        &self,
        dir: String,
        dry_run: bool,
    ) -> BackendResult<GeneratedDocument> {
        self.state
            .generate_report(Some(dir), dry_run)
            .await
            .map_err(report)
    }

    pub async fn draft_decision(
        &self,
        title: Option<String>,
        dir: String,
        dry_run: bool,
    ) -> BackendResult<GeneratedDocument> {
        self.state
            .draft_decision(title, Some(dir), dry_run)
            .await
            .map_err(report)
    }

    // --- Settings ---

    /// The config as the UI sees it. The API key is write-only and never
    /// included; see [`AppConfigDto::has_llm_api_key`].
    pub async fn config(&self) -> AppConfigDto {
        AppConfigDto::from(&*self.config.read().await)
    }

    pub async fn set_currency(&self, currency: String) -> BackendResult<()> {
        self.state.set_currency(currency.clone()).await;
        self.config.write().await.set_currency(currency);
        self.save_config().await
    }

    pub async fn set_theme(&self, theme: ThemeMode) -> BackendResult<()> {
        self.config.write().await.set_theme(theme);
        self.save_config().await
    }

    /// `None` leaves a value unchanged.
    pub async fn set_llm_config(
        &self,
        provider_url: Option<String>,
        api_key: Option<String>,
        model: Option<String>,
    ) -> BackendResult<()> {
        {
            let mut config = self.config.write().await;
            if let Some(url) = provider_url {
                config.set_llm_provider_url(url);
            }
            if let Some(key) = api_key {
                config.set_llm_api_key(key);
            }
            if let Some(model) = model {
                config.set_llm_model(model);
            }
        }
        self.save_config().await
    }

    pub async fn clear_remembered_path(&self) -> BackendResult<()> {
        self.config.write().await.clear_paths();
        self.save_config().await
    }

    pub fn config_path(&self) -> BackendResult<String> {
        match &self.config_path {
            Some(path) => Ok(path.to_string_lossy().into_owned()),
            None => config_path().map_err(report),
        }
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;

    use portfolio_rs::services::quotes::AnyProvider;
    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    use super::*;

    const CASH_ONLY: &str = r#"[{"Name":"Cash","AssetClass":"Cash","Amount":1000.0}]"#;

    /// A backend that touches neither the network nor the user's config.
    fn offline_backend(runtime: &Runtime, dir: &TempDir) -> Backend {
        Backend::new(
            AppState::with_quotes("EUR".to_string(), AnyProvider::Offline),
            AppConfig::default(),
            Some(dir.path().join("config.yml")),
            runtime.handle().clone(),
        )
    }

    fn write_portfolio(dir: &TempDir) -> String {
        let path = dir.path().join("positions.json");
        fs::write(&path, CASH_ONLY).unwrap();
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn briefing_reads_the_context_contract() {
        // Guards against portfolio_rs renaming a field the page depends on.
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let path = write_portfolio(&dir);

        runtime.block_on(async {
            backend.startup(Some(path)).await.unwrap();
            let briefing = backend.briefing().await.unwrap();
            assert_eq!(briefing.summary.position_count, 1);
            assert_eq!(briefing.summary.total_value, 1000.0);
            assert_eq!(briefing.positions[0].name, "Cash");
            assert!(!briefing.markdown.is_empty());
        });
    }

    #[test]
    fn startup_opens_the_file_from_the_command_line() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let path = write_portfolio(&dir);

        runtime.block_on(async {
            let open = backend.startup(Some(path.clone())).await.unwrap();
            assert_eq!(open.file_path, Some(path));
            assert_eq!(open.workspace_dir, None);
            let snapshot = backend.snapshot().await;
            assert_eq!(snapshot.positions.len(), 1);
            assert_eq!(snapshot.summary.total_value, 1000.0);
        });
    }

    #[test]
    fn startup_without_a_remembered_portfolio_opens_nothing() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);

        let open = runtime.block_on(backend.startup(None)).unwrap();
        assert_eq!(open.file_path, None);
        assert_eq!(open.workspace_dir, None);
    }

    #[test]
    fn position_edits_persist_only_on_save() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let path = write_portfolio(&dir);

        runtime.block_on(async {
            backend.startup(Some(path.clone())).await.unwrap();
            backend
                .add_position(CreatePositionRequest {
                    name: Some("Savings".to_string()),
                    ticker: None,
                    asset_class: "Cash".to_string(),
                    amount: 250.0,
                    purchases: Vec::new(),
                })
                .await
                .unwrap();
            assert_eq!(backend.snapshot().await.positions.len(), 2);
            assert!(!fs::read_to_string(&path).unwrap().contains("Savings"));

            backend.save_portfolio().await.unwrap();
            assert!(fs::read_to_string(&path).unwrap().contains("Savings"));

            backend.delete_position(1).await.unwrap();
            assert_eq!(backend.snapshot().await.positions.len(), 1);
        });
    }

    #[test]
    fn every_strategy_template_round_trips_through_the_toml_editor() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);

        for strategy in [
            "balanced-growth",
            "capital-preservation",
            "aggressive-growth",
            "custom",
        ] {
            let policy = Backend::policy_from_strategy(strategy).unwrap();
            let toml = Backend::policy_to_toml(&policy).unwrap();
            let parsed = runtime.block_on(backend.set_policy_from_toml(&toml));
            let parsed = parsed.unwrap_or_else(|error| panic!("{strategy}: {error}"));
            assert_eq!(parsed.name, policy.name);
            assert_eq!(parsed.allocations.len(), policy.allocations.len());
        }
    }

    #[test]
    fn policy_toml_escapes_quotes_in_names() {
        // The web UI built this TOML by string interpolation and broke here.
        let mut policy = Backend::policy_from_strategy("custom").unwrap();
        policy.name = r#"My "Safe" Plan"#.to_string();
        let toml = Backend::policy_to_toml(&policy).unwrap();
        let parsed: Policy = toml.parse().unwrap();
        assert_eq!(parsed.name, policy.name);
    }

    #[test]
    fn unknown_strategy_is_an_error() {
        assert!(Backend::policy_from_strategy("yolo").is_err());
    }

    #[test]
    fn config_is_saved_owner_only_and_never_exposes_the_key() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);

        runtime.block_on(async {
            backend
                .set_llm_config(None, Some("sk-secret".to_string()), None)
                .await
                .unwrap();
            let config = backend.config().await;
            assert!(config.has_llm_api_key);
            assert_eq!(config.llm_api_key, None);

            // Blank means unchanged here: the Settings page sends `None`.
            backend.set_llm_config(None, None, None).await.unwrap();
            assert!(backend.config().await.has_llm_api_key);
        });

        #[cfg(unix)]
        {
            let path = dir.path().join("config.yml");
            let mode = fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    fn point(date: &str, total: f64, complete: bool) -> HistoryPoint {
        HistoryPoint {
            date: date.to_string(),
            base_currency: "EUR".to_string(),
            positions: Vec::new(),
            cash_base: total,
            total_base: total,
            net_flows_base: 0.0,
            complete,
            data_status: "complete".to_string(),
        }
    }

    #[test]
    fn a_period_keeps_one_baseline_from_before_it_starts() {
        let points = vec![
            point("2026-01-01", 1.0, true),
            point("2026-02-01", 2.0, true),
            point("2026-03-01", 3.0, true),
            point("2026-03-15", 4.0, true),
        ];
        let start = NaiveDate::from_ymd_opt(2026, 3, 1);
        let dates: Vec<String> = points_in_period(points, start)
            .into_iter()
            .map(|point| point.date)
            .collect();
        assert_eq!(dates, ["2026-02-01", "2026-03-01", "2026-03-15"]);
    }

    #[test]
    fn a_period_before_any_history_starts_at_the_first_point() {
        let points = vec![point("2026-03-01", 3.0, true)];
        let start = NaiveDate::from_ymd_opt(2020, 1, 1);
        assert_eq!(points_in_period(points, start).len(), 1);
    }

    #[test]
    fn incomplete_valuations_are_left_out_of_history() {
        let points = vec![
            point("2026-03-01", 100.0, true),
            point("2026-03-02", 1.0, false),
            point("2026-03-03", 110.0, true),
        ];
        let history = build_history(points, &[], None);
        assert_eq!(history.points.len(), 2);
        assert_eq!(history.change, Some(10.0));
    }

    #[test]
    fn one_valuation_has_no_change_yet() {
        let history = build_history(vec![point("2026-03-01", 100.0, true)], &[], None);
        assert_eq!(history.points.len(), 1);
        assert_eq!(history.change, None);
        assert_eq!(history.twr_percent, None);
    }

    #[test]
    fn a_file_that_does_not_parse_is_reported_not_emptied() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let path = dir.path().join("positions.json");
        let broken = "[\n  {\"Name\": \"Cash\",,}\n]";
        fs::write(&path, broken).unwrap();

        runtime.block_on(async {
            let open = backend
                .startup(Some(path.to_string_lossy().into_owned()))
                .await
                .unwrap();
            let problem = open.parse_error.expect("reports the parse error");
            assert_eq!(problem.line, 2);
            assert!(problem.column > 0);

            // Saving must not replace the user's text with an empty list.
            backend.save_portfolio().await.ok();
            assert_eq!(fs::read_to_string(&path).unwrap(), broken);
        });
    }

    #[test]
    fn encrypted_portfolios_never_record_history() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        runtime.block_on(async {
            // No file open behaves like the encrypted case: nothing is written.
            backend.record_today().await.unwrap();
        });
        assert!(!dir.path().join("main").exists());
    }

    #[test]
    fn run_bridges_results_and_panics_back_to_the_caller() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);

        let ok = backend.run(async { Ok(7) });
        assert_eq!(runtime.block_on(ok), Ok(7));

        let panicked = backend.run::<(), _>(async { panic!("boom") });
        assert!(runtime.block_on(panicked).is_err());
    }
}
