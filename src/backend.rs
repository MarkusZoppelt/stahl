//! The bridge between the UI and `portfolio_rs`.
//!
//! `portfolio_rs` is async on Tokio, while GPUI runs its own executors. Every
//! call therefore runs on a Tokio runtime owned by `main`, and the UI awaits
//! the result through [`Backend::run`]. Almost all domain logic (portfolio
//! math, policy review, simulation, reports) lives in `portfolio_rs`.

use std::{
    env,
    ffi::OsStr,
    fmt::Display,
    fs,
    future::Future,
    io::ErrorKind,
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
    services::atomic_write,
    simulate::RebalanceSimulation,
    state::{
        AllocationItemDto, AppConfigDto, AppState, CreatePositionRequest, GeneratedDocument,
        PerformanceDto, PortfolioSummaryDto, PositionDto, UpdatePositionRequest,
    },
    validate::ValidationReport,
    workspace,
};
use serde::{Deserialize, Serialize};
use serde_json::{Error as JsonError, from_value, to_value};
use tokio::{runtime::Handle, sync::RwLock};

use crate::statement::{Change, Contribution, Seen};

pub type BackendResult<T> = Result<T, String>;

fn report<E: Display>(error: E) -> String {
    // `{:#}` includes the whole eyre context chain.
    format!("{error:#}")
}

/// The id `portfolio_rs snapshot` records under by default, so history made
/// here and on the command line is the same history.
const HISTORY_ID: &str = "main";

/// Beside a portfolio's history: the days its owner looked at it.
const SEEN_FILE: &str = "seen.json";

#[derive(Serialize, Deserialize)]
struct SeenFile {
    last: String,
    #[serde(default)]
    before: Option<String>,
}

fn read_seen(text: &str) -> Option<Seen> {
    let file: SeenFile = serde_json::from_str(text).ok()?;
    Some(Seen {
        last: snapshots::strict_date(&file.last)?,
        before: file.before.as_deref().and_then(snapshots::strict_date),
    })
}

fn write_seen(seen: Seen) -> String {
    let day = |day: NaiveDate| day.format("%Y-%m-%d").to_string();
    let file = SeenFile {
        last: day(seen.last),
        before: seen.before.map(day),
    };
    serde_json::to_string(&file).unwrap_or_default() + "\n"
}

/// What a positions file holds before anything is added to it.
const NO_POSITIONS: &str = "[]\n";

/// Which field of the New Workspace form a problem belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceField {
    Name,
    Location,
}

/// Why a workspace can't be created, in words for the person filling in the form.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceProblem {
    pub field: WorkspaceField,
    pub message: String,
}

impl WorkspaceProblem {
    fn name(message: impl Into<String>) -> Self {
        Self {
            field: WorkspaceField::Name,
            message: message.into(),
        }
    }

    fn location(message: impl Into<String>) -> Self {
        Self {
            field: WorkspaceField::Location,
            message: message.into(),
        }
    }
}

/// Where a workspace with this name and location would go. Touches nothing on disk, so the form
/// can show it while you type.
pub fn plan_workspace(name: &str, location: &str) -> Result<PathBuf, WorkspaceProblem> {
    let name = name.trim();
    if name.is_empty() {
        return Err(WorkspaceProblem::name("Give the workspace a name."));
    }
    let forbidden = |c: char| c.is_control() || r#"/\:*?"<>|"#.contains(c);
    if name.contains(forbidden) || name.ends_with('.') {
        return Err(WorkspaceProblem::name(
            r#"A name can't contain / \ : * ? " < > | or end with a dot."#,
        ));
    }
    let location = location.trim();
    if location.is_empty() {
        return Err(WorkspaceProblem::location(
            "Choose where to create the workspace.",
        ));
    }
    let location = expand_home(location);
    if !location.is_absolute() {
        return Err(WorkspaceProblem::location(
            "The location must be a full path, starting from the top of the disk.",
        ));
    }
    Ok(location.join(name))
}

/// `~` and `~/x` as the home directory, as a path typed into a shell would be.
fn expand_home(path: &str) -> PathBuf {
    let rest = path
        .strip_prefix('~')
        .filter(|rest| rest.is_empty() || rest.starts_with(['/', '\\']));
    match (rest, env::home_dir()) {
        (Some(rest), Some(home)) => home.join(rest.trim_start_matches(['/', '\\'])),
        _ => PathBuf::from(path),
    }
}

/// Create a workspace in the format `portfolio_rs` uses, and return its directory.
///
/// `positions_from` is a file to copy the positions from, or `None` to start with no positions.
/// The directory must not exist yet, so this can never overwrite anything. If it fails part way,
/// what it created is removed again.
pub fn create_workspace(
    name: &str,
    location: &str,
    positions_from: Option<&str>,
) -> Result<String, WorkspaceProblem> {
    let dir = plan_workspace(name, location)?;
    let shown = dir.display();
    let parent = dir.parent().unwrap_or(&dir);
    let parent_shown = parent.display();
    if !parent.exists() {
        return Err(WorkspaceProblem::location(format!(
            "{parent_shown} doesn't exist. Choose an existing folder."
        )));
    }
    if !parent.is_dir() {
        return Err(WorkspaceProblem::location(format!(
            "{parent_shown} is a file, not a folder."
        )));
    }
    let exists = || {
        WorkspaceProblem::name(format!(
            "{shown} already exists. Choose another name, or open it as a workspace."
        ))
    };
    // Also true of a dangling link, which `create_dir` would fail on anyway.
    if fs::symlink_metadata(&dir).is_ok() {
        return Err(exists());
    }
    let Some(root) = dir.to_str() else {
        return Err(WorkspaceProblem::name(
            "This name or location has characters Stahl can't store.",
        ));
    };
    match fs::create_dir(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Err(exists()),
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::PermissionDenied | ErrorKind::ReadOnlyFilesystem
            ) =>
        {
            return Err(WorkspaceProblem::location(format!(
                "You can't write to {parent_shown}. Choose another location."
            )));
        }
        Err(error) => {
            return Err(WorkspaceProblem::location(format!(
                "Couldn't create {shown}: {error}"
            )));
        }
    }
    let filled = || -> BackendResult<()> {
        workspace::init_workspace(root, false).map_err(report)?;
        let positions = match positions_from {
            Some(file) => fs::read(file).map_err(report)?,
            None => NO_POSITIONS.as_bytes().to_vec(),
        };
        atomic_write::write_new_private(&dir.join("positions.json"), &positions).map_err(report)
    };
    filled().map_err(|error| {
        // Only ever what this call just made: `create_dir` fails on anything already there.
        let _ = fs::remove_dir_all(&dir);
        WorkspaceProblem::location(format!("Couldn't create {shown}: {error}"))
    })?;
    Ok(root.to_string())
}

/// The chosen folder was opened, or it needs the person to decide to start a workspace there.
pub enum OpenedFolder {
    Loaded,
    NeedsStart { dir: String, empty: bool },
}

/// What a chosen folder is, once a `portfolio/` subdirectory has been walked back up to the
/// workspace that contains it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FolderKind {
    /// `positions.json` is here.
    Workspace(String),
    /// Laid out by `portfolio_rs init-workspace`, which writes no `positions.json`.
    PortfolioRs(String),
    /// Not a workspace. `empty` means the folder has nothing in it.
    Other { dir: String, empty: bool },
}

/// Decide how to open `dir`.
///
/// A folder named `portfolio` with no positions file is the inside of a workspace (history,
/// policy, diary), which is what a folder chooser returns when it reopens inside the last
/// workspace and its only child is that directory. The workspace is the parent.
pub fn classify_folder(dir: &Path) -> Result<FolderKind, String> {
    if !dir.exists() {
        return Err(format!("{} doesn't exist.", dir.display()));
    }
    if !dir.is_dir() {
        return Err(format!("{} is a file, not a folder.", dir.display()));
    }
    let root = workspace_root(dir);
    let shown = path_string(&root)?;
    if positions_file(&root).is_file() {
        return Ok(FolderKind::Workspace(shown));
    }
    if is_portfolio_rs_workspace(&root) {
        return Ok(FolderKind::PortfolioRs(shown));
    }
    Ok(FolderKind::Other {
        dir: shown,
        empty: folder_is_empty(&root)?,
    })
}

/// Where the folder dialog should open so it is not already inside a workspace.
///
/// The parent of the open workspace (or of a folder that was just refused) shows the workspace
/// itself as a choice. Opening inside it highlights `portfolio/`, and Select returns that.
pub fn folder_dialog_start(open: Option<&Path>) -> Option<PathBuf> {
    let path = open?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())?;
    parent.is_dir().then(|| parent.to_path_buf())
}

fn file_is_blank(path: &str) -> bool {
    fs::read_to_string(path).is_ok_and(|text| text.trim().is_empty())
}

/// True when the file is JSON, and that JSON is not a list.
fn json_is_not_a_list(path: &str) -> bool {
    let Ok(text) = fs::read_to_string(path) else {
        return false;
    };
    matches!(
        serde_json::from_str::<serde_json::Value>(&text),
        Ok(value) if !value.is_array()
    )
}

fn positions_file(dir: &Path) -> PathBuf {
    dir.join("positions.json")
}

fn path_string(path: &Path) -> Result<String, String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| "This folder has characters Stahl can't store.".to_string())
}

/// `dir` when it is already a workspace, or its parent when `dir` is that workspace's
/// `portfolio/` directory.
fn workspace_root(dir: &Path) -> PathBuf {
    if positions_file(dir).is_file() {
        return dir.to_path_buf();
    }
    let Some(parent) = dir.parent() else {
        return dir.to_path_buf();
    };
    let named_portfolio = dir.file_name() == Some(OsStr::new("portfolio"));
    if named_portfolio && (positions_file(parent).is_file() || is_portfolio_rs_workspace(parent)) {
        parent.to_path_buf()
    } else {
        dir.to_path_buf()
    }
}

/// The files `portfolio_rs init-workspace` always writes, and Stahl does not invent.
fn is_portfolio_rs_workspace(dir: &Path) -> bool {
    dir.join("portfolio/watchlist.json").is_file()
        || (dir.join("INVESTMENT_POLICY.md").is_file() && dir.join("portfolio").is_dir())
}

fn folder_is_empty(dir: &Path) -> Result<bool, String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|error| format!("Couldn't look inside {}: {error}", dir.display()))?;
    Ok(entries.next().is_none())
}

/// Make `dir` a workspace in place. Files already there are kept: `portfolio_rs` skips what
/// exists, and `positions.json` is only written when it is missing.
pub fn start_workspace(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a folder.", dir.display()));
    }
    let root = path_string(dir)?;
    workspace::init_workspace(&root, false).map_err(report)?;
    let positions = positions_file(dir);
    if positions.exists() {
        return Ok(());
    }
    atomic_write::write_new_private(&positions, NO_POSITIONS.as_bytes()).map_err(report)
}

/// Where the New Workspace form suggests putting one: Documents, or else the home directory.
pub fn default_workspace_location() -> String {
    let Some(home) = env::home_dir() else {
        return String::new();
    };
    let documents = home.join("Documents");
    let location = if documents.is_dir() { documents } else { home };
    location.to_string_lossy().into_owned()
}

/// Everything the UI shows about the loaded portfolio, read together.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub summary: PortfolioSummaryDto,
    pub positions: Vec<PositionDto>,
    /// One per position, at the index that is the position's id.
    pub quotes: Vec<Quote>,
    pub allocation: Vec<AllocationItemDto>,
    pub performance: PerformanceDto,
    pub findings: Vec<Finding>,
    /// connected | partial | offline | fixture
    pub network: String,
    /// Whether there's a policy to review the portfolio against.
    pub has_policy: bool,
}

/// What a holding's per-unit figures are in, which [`PositionDto`] doesn't say.
///
/// Its `price`, `average_cost` and purchase prices are in the quote currency, while its `value`,
/// `invested` and `pnl` are in the base currency.
#[derive(Clone, Debug, PartialEq)]
pub struct Quote {
    pub ticker: Option<String>,
    /// The currency of the per-unit figures. `None` for a ticker that no quote has priced yet.
    pub currency: Option<String>,
    /// The price in the base currency, when that differs from the quote currency and the FX rate
    /// is known: exactly `value / quantity`.
    pub base_price: Option<f64>,
}

impl Quote {
    fn of(position: &PortfolioPosition, base: &str) -> Self {
        // Like portfolio_rs: a holding without a ticker is entered in the base currency, and so
        // is what a live quote that names no currency reports.
        let in_base = position.get_ticker().is_none() || position.has_market_price();
        Self {
            ticker: position.get_ticker().map(String::from),
            currency: position
                .price_currency()
                .filter(|_| position.get_ticker().is_some())
                .map(String::from)
                .or_else(|| in_base.then(|| base.to_string())),
            base_price: position
                .fx_rate()
                .filter(|_| position.has_market_price())
                .map(|rate| position.market_price() * rate),
        }
    }
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
    /// What the parser said, unchanged.
    pub reason: String,
    pub line: usize,
    pub column: usize,
}

impl ParseProblem {
    /// A sentence for the screen. [`Self::reason`] stays available as the raw detail.
    pub fn summary(&self) -> String {
        // An empty file, or one that is only whitespace, fails before any value. A file that
        // starts a value and then stops is not empty, even when the parser also says EOF.
        if self.reason == "EOF while parsing a value" && file_is_blank(&self.file) {
            return "The file is empty.".to_string();
        }
        // Valid JSON of the wrong shape: an object, a string, a number. Invalid text that
        // merely starts with `{` gets the same parser message, and is not a list either, but
        // it is reported as JSON that doesn't parse.
        if self.reason.contains("expected a sequence") && json_is_not_a_list(&self.file) {
            return "Expected a list of positions.".to_string();
        }
        format!(
            "This isn't valid JSON (line {}, column {}).",
            self.line, self.column
        )
    }
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

    /// A period as a block or a prompt names it, defaulting to the month the Overview charts.
    pub fn from_name(name: &str) -> Period {
        match name.trim().to_lowercase().as_str() {
            "week" | "1w" | "seven days" => Period::Week,
            "ytd" | "year to date" => Period::Ytd,
            "year" | "1y" | "twelve months" => Period::Year,
            "all" | "everything" => Period::All,
            _ => Period::Month,
        }
    }

    /// The period's name as a sentence would have it.
    pub fn phrase(self) -> &'static str {
        match self {
            Period::Week => "the past week",
            Period::Month => "the past month",
            Period::Ytd => "this year",
            Period::Year => "the past year",
            Period::All => "all of it",
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
    /// One line per holding, for the sparklines. Price when the snapshot recorded one.
    pub series: Vec<HoldingSeries>,
    pub change: Option<f64>,
    pub net_contributions: Option<f64>,
    pub market_effect: Option<f64>,
    /// Time-weighted, so deposits and withdrawals do not count as returns.
    pub twr_percent: Option<f64>,
    /// Today's valuation is written here, unless the portfolio is encrypted.
    pub path: Option<String>,
}

/// A holding's path over the same days as [`History::points`].
#[derive(Clone, Debug, PartialEq)]
pub struct HoldingSeries {
    pub position_id: String,
    /// Quote-currency price when the snapshot had one, so a purchase does not read as a jump.
    /// Value in the base currency only when no price was recorded (cash, a manual holding).
    pub values: Vec<f64>,
    pub priced: bool,
}

impl History {
    /// The series for a holding, if it has at least two points to draw.
    pub fn series(&self, position_id: &str) -> Option<&[f64]> {
        self.series
            .iter()
            .find(|series| series.position_id == position_id && series.values.len() >= 2)
            .map(|series| series.values.as_slice())
    }
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
        series: series_from(&points),
        points: points
            .into_iter()
            .map(|point| HistoryValue {
                value: point.total_base,
                date: point.date,
            })
            .collect(),
    }
}

/// One series per position that appears in the snapshots.
///
/// Price, in the quote currency, when at least two days recorded one. A buy changes the
/// quantity and the value, not the price, so the line is the market and not the purchase.
/// Value in the base currency is the fallback for cash and anything with no price; a deposit
/// then does show as a step, because that is the only series those holdings have.
fn series_from(points: &[HistoryPoint]) -> Vec<HoldingSeries> {
    let mut ids = Vec::new();
    for point in points {
        for position in &point.positions {
            if !ids.contains(&position.position_id) {
                ids.push(position.position_id.clone());
            }
        }
    }
    ids.into_iter()
        .filter_map(|id| {
            let prices: Vec<f64> = points
                .iter()
                .filter_map(|point| {
                    point
                        .positions
                        .iter()
                        .find(|position| position.position_id == id)
                        .and_then(|position| position.price)
                })
                .filter(|price| price.is_finite())
                .collect();
            if prices.len() >= 2 {
                return Some(HoldingSeries {
                    position_id: id,
                    values: prices,
                    priced: true,
                });
            }
            let values: Vec<f64> = points
                .iter()
                .filter_map(|point| {
                    point
                        .positions
                        .iter()
                        .find(|position| position.position_id == id)
                        .and_then(|position| position.value_base)
                })
                .filter(|value| value.is_finite())
                .collect();
            (values.len() >= 2).then_some(HoldingSeries {
                position_id: id,
                values,
                priced: false,
            })
        })
        .collect()
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

    /// Open a workspace and remember it.
    ///
    /// This does what `AppState::load_workspace` does, except that it saves the config through
    /// [`Self::save_config`]: that one always writes to the user's default location, so a backend
    /// with its config elsewhere would overwrite the real one. It also drops the policy of whatever
    /// was open before, which would otherwise be reviewed against the new portfolio.
    pub async fn load_workspace(&self, dir: String) -> BackendResult<()> {
        let root = Path::new(&dir);
        let positions = root.join("positions.json");
        if !positions.exists() {
            return Err(format!(
                "{dir} is not a workspace, because it has no positions.json. Pick another folder, \
                 or create a new workspace."
            ));
        }
        let policy_path = root.join("portfolio/policy.toml");
        let policy = if policy_path.exists() {
            let policy = Policy::from_file(&policy_path.to_string_lossy())
                .map_err(|error| format!("failed to load policy: {error:#}"))?;
            Some(policy)
        } else {
            None
        };
        self.state
            .load_file(&positions.to_string_lossy())
            .await
            .map_err(report)?;
        *self.state.workspace_dir.write().await = Some(dir.clone());
        *self.state.policy.write().await = policy;
        self.config.write().await.set_workspace(dir);
        self.save_config().await
    }

    /// Open `dir` as a workspace.
    ///
    /// A `portfolio/` directory inside a workspace opens the workspace. A folder made with
    /// `portfolio_rs init-workspace` gets an empty `positions.json` and then opens. Anything else
    /// is left untouched and reported as [`OpenedFolder::NeedsStart`], so the caller can offer to
    /// start one there.
    pub async fn open_folder(&self, dir: String) -> BackendResult<OpenedFolder> {
        match classify_folder(Path::new(&dir))? {
            FolderKind::Other { dir, empty } => Ok(OpenedFolder::NeedsStart { dir, empty }),
            FolderKind::Workspace(root) | FolderKind::PortfolioRs(root) => {
                if !positions_file(Path::new(&root)).is_file() {
                    start_workspace(Path::new(&root))?;
                }
                self.load_workspace(root).await?;
                Ok(OpenedFolder::Loaded)
            }
        }
    }

    /// Open a single positions file and remember it. See [`Self::load_workspace`] for why this
    /// doesn't use `AppState::load_simple_file`.
    pub async fn load_simple_file(&self, path: String) -> BackendResult<()> {
        self.state.load_file(&path).await.map_err(report)?;
        *self.state.policy.write().await = None;
        self.config.write().await.set_portfolio_file(path);
        self.save_config().await
    }

    pub fn export_skill(&self, dir: &str) -> BackendResult<()> {
        agent_skill::skill_export(dir, false).map_err(report)
    }

    // --- Portfolio ---

    pub async fn snapshot(&self) -> Snapshot {
        let portfolio = self.state.portfolio().await;
        let base = self.state.currency().await;
        Snapshot {
            summary: self.state.get_portfolio_summary().await,
            positions: self.state.get_positions().await,
            quotes: portfolio
                .positions
                .iter()
                .map(|position| Quote::of(position, &base))
                .collect(),
            allocation: self.state.get_allocation().await,
            performance: self.state.get_performance().await,
            findings: analytics::data_quality(&portfolio),
            network: analytics::network_status(&portfolio).to_string(),
            has_policy: self.state.policy().await.is_some(),
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
    /// Skipped for files that do not parse or are not fully priced, for encrypted portfolios,
    /// whose values must not land on disk in plaintext, and for empty portfolios, where history
    /// would start with a zero that reads as a jump once there are positions.
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
        if portfolio.positions.is_empty() {
            return Ok(());
        }
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

    // --- Since the last look ---

    /// Where the days the owner looked at this portfolio are kept: beside its history, so a
    /// workspace keeps its own, and a test's data directory keeps the test's. Dates only.
    async fn seen_path(&self) -> Option<PathBuf> {
        Some(self.history_path().await?.parent()?.join(SEEN_FILE))
    }

    /// Note a look at the open portfolio today, and return the record as it stood before it.
    pub async fn visit(&self) -> BackendResult<Option<Seen>> {
        let Some(path) = self.seen_path().await else {
            return Ok(None);
        };
        let previous = fs::read_to_string(&path)
            .ok()
            .and_then(|text| read_seen(&text));
        let next = Seen::after_visit(previous, Local::now().date_naive());
        if Some(next) != previous {
            if let Some(dir) = path.parent() {
                fs::create_dir_all(dir).map_err(report)?;
            }
            atomic_write::write_atomic_private(&path, write_seen(next).as_bytes())
                .map_err(report)?;
        }
        Ok(previous)
    }

    /// The portfolio between the last stored valuation on or before `since` (the oldest one, for
    /// `None`) and now. `None` when there is nothing to compare: no such valuation, a price
    /// missing today, a flow of unknown size, or an encrypted portfolio, which has no history.
    pub async fn change_since(&self, since: Option<NaiveDate>) -> BackendResult<Option<Change>> {
        let encrypted = self
            .state
            .file_path()
            .await
            .is_none_or(|path| path.ends_with(".gpg"));
        let Some(path) = self.history_path().await.filter(|_| !encrypted) else {
            return Ok(None);
        };
        let (mut points, _skipped) = snapshots::read(&path);
        let portfolio = self.state.portfolio().await;
        let today = self.today_point(&portfolio).await;
        if !today.complete {
            return Ok(None);
        }
        points.retain(|point| {
            point.complete && point.base_currency == today.base_currency && point.date < today.date
        });
        let start = match since {
            Some(day) => {
                let day = day.format("%Y-%m-%d").to_string();
                points.iter().rev().find(|point| point.date <= day)
            }
            None => points.first(),
        };
        let Some(start) = start else {
            return Ok(None);
        };
        let Some(from) = snapshots::strict_date(&start.date) else {
            return Ok(None);
        };
        let base = self.state.currency().await;
        let transactions = analytics::transactions(&portfolio, &base, None);
        let Some(effects) = performance::position_effects(start, &today, &transactions) else {
            return Ok(None);
        };
        let total = performance::total(&effects);
        let market = today.total_base - start.total_base - total.net_contributions;
        let names = self.state.get_positions().await;
        let leader = effects
            .iter()
            .map(|(id, (_, effect))| (id, effect.market_effect + effect.manual_revaluation))
            .filter(|(_, amount)| amount * market > 0.0)
            .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
            .and_then(|(id, amount)| {
                let position = names
                    .iter()
                    .find(|position| position.position_id.as_deref() == Some(id.as_str()))?;
                let name = position.name.clone().or_else(|| position.ticker.clone())?;
                Some(Contribution { name, amount })
            });
        Ok(Some(Change {
            from,
            start: start.total_base,
            end: today.total_base,
            added: total.net_contributions,
            leader,
            previous_high: points
                .iter()
                .map(|point| point.total_base)
                .max_by(f64::total_cmp),
            history_from: points
                .first()
                .and_then(|point| snapshots::strict_date(&point.date)),
        }))
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

    use portfolio_rs::{doctor::HealthStatus, services::quotes::AnyProvider};
    use tempfile::TempDir;
    use tokio::runtime::Runtime;

    use super::*;
    use crate::demo;

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

    fn folder(dir: &TempDir) -> String {
        dir.path().to_string_lossy().into_owned()
    }

    #[test]
    fn a_new_workspace_opens_in_the_portfolio_rs_format() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);

        let root = create_workspace(" My Portfolio ", &folder(&dir), None).unwrap();
        assert!(
            root.ends_with("My Portfolio"),
            "the name is trimmed: {root}"
        );

        runtime.block_on(async {
            backend.load_workspace(root.clone()).await.unwrap();
            let open = backend.open_document().await;
            assert_eq!(open.workspace_dir, Some(root.clone()));
            assert_eq!(open.parse_error, None);
            assert!(backend.snapshot().await.positions.is_empty());

            backend.record_today().await.unwrap();
            let history = Path::new(&root).join("portfolio/history/main/snapshots.jsonl");
            assert!(!history.exists(), "an empty portfolio has no history yet");

            // `portfolio_rs doctor` agrees it's a workspace, and it only lacks a policy so far.
            let health = backend.doctor(root.clone()).await.unwrap();
            assert_eq!(health.issue_count, 0, "{:?}", health.checks);
            let warned: Vec<&str> = health
                .checks
                .iter()
                .filter(|check| check.status == HealthStatus::Warning)
                .map(|check| check.name.as_str())
                .collect();
            assert_eq!(warned, ["Policy file", "Encryption"]);
        });
    }

    #[test]
    fn a_new_workspace_can_start_from_an_open_file() {
        let dir = TempDir::new().unwrap();
        let file = write_portfolio(&dir);
        let root = create_workspace("Copied", &folder(&dir), Some(&file)).unwrap();
        let copied = fs::read_to_string(Path::new(&root).join("positions.json")).unwrap();
        assert_eq!(copied, CASH_ONLY);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            CASH_ONLY,
            "left as it was"
        );
    }

    #[test]
    fn a_workspace_never_replaces_what_is_there() {
        let dir = TempDir::new().unwrap();
        let taken = dir.path().join("Taken");
        fs::create_dir(&taken).unwrap();
        fs::write(taken.join("keep.txt"), "mine").unwrap();

        let problem = create_workspace("Taken", &folder(&dir), None).unwrap_err();
        assert_eq!(problem.field, WorkspaceField::Name);
        assert!(problem.message.contains("already exists"), "{problem:?}");
        assert_eq!(fs::read_to_string(taken.join("keep.txt")).unwrap(), "mine");
        assert_eq!(fs::read_dir(&taken).unwrap().count(), 1);
    }

    #[test]
    fn a_failed_workspace_leaves_nothing_behind() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("no-such-file.json");
        let problem =
            create_workspace("Half", &folder(&dir), Some(&missing.to_string_lossy())).unwrap_err();
        assert!(problem.message.contains("Couldn't create"), "{problem:?}");
        assert!(!dir.path().join("Half").exists());
    }

    #[test]
    fn workspace_names_and_locations_are_checked() {
        let dir = TempDir::new().unwrap();
        let location = folder(&dir);
        let problem = |name: &str, location: &str| plan_workspace(name, location).unwrap_err();

        assert_eq!(problem("  ", &location).field, WorkspaceField::Name);
        for name in ["a/b", r"a\b", "..", "a:b", "tail."] {
            let problem = problem(name, &location);
            assert_eq!(problem.field, WorkspaceField::Name, "{name}");
            assert!(problem.message.contains("can't contain"), "{name}");
        }
        assert_eq!(problem("a", "  ").field, WorkspaceField::Location);
        assert!(problem("a", "relative/path").message.contains("full path"));
        assert_eq!(
            plan_workspace("a", &location).unwrap(),
            dir.path().join("a")
        );

        let missing = dir.path().join("missing");
        let problem = create_workspace("a", &missing.to_string_lossy(), None).unwrap_err();
        assert_eq!(problem.field, WorkspaceField::Location);
        assert!(problem.message.contains("doesn't exist"), "{problem:?}");

        let file = write_portfolio(&dir);
        let problem = create_workspace("a", &file, None).unwrap_err();
        assert!(problem.message.contains("not a folder"), "{problem:?}");
    }

    #[test]
    fn a_home_location_expands() {
        let Some(home) = env::home_dir() else {
            return;
        };
        assert_eq!(plan_workspace("a", "~").unwrap(), home.join("a"));
        assert_eq!(
            plan_workspace("a", "~/b").unwrap(),
            home.join("b").join("a")
        );
        assert!(
            plan_workspace("a", "~elsewhere").is_err(),
            "not this user's"
        );
    }

    #[test]
    fn opening_saves_the_config_where_the_backend_keeps_it() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let path = write_portfolio(&dir);

        runtime
            .block_on(backend.load_simple_file(path.clone()))
            .unwrap();
        let saved = AppConfig::load_from(&dir.path().join("config.yml")).unwrap();
        assert_eq!(saved.portfolio_file, Some(path));

        let workspace = dir.path().to_string_lossy().into_owned();
        runtime
            .block_on(backend.load_workspace(workspace.clone()))
            .unwrap();
        let saved = AppConfig::load_from(&dir.path().join("config.yml")).unwrap();
        assert_eq!(saved.workspace_dir, Some(workspace));
    }

    #[test]
    fn opening_a_folder_without_positions_says_what_is_wrong() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let folder = dir.path().to_string_lossy().into_owned();

        let error = runtime
            .block_on(backend.load_workspace(folder))
            .unwrap_err();
        assert!(error.contains("no positions.json"), "{error}");
    }

    #[test]
    fn the_portfolio_directory_inside_a_workspace_is_that_workspace() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let root = create_workspace("My Portfolio", &folder(&dir), None).unwrap();
        // Opening records history here, which is the only child a chooser then sees.
        runtime
            .block_on(backend.load_workspace(root.clone()))
            .unwrap();
        let inner = Path::new(&root).join("portfolio");
        assert!(inner.is_dir());

        let again = classify_folder(&inner).unwrap();
        assert_eq!(again, FolderKind::Workspace(root.clone()));
        runtime
            .block_on(backend.open_folder(inner.to_string_lossy().into_owned()))
            .unwrap();
        let open = runtime.block_on(backend.open_document());
        assert_eq!(open.workspace_dir.as_deref(), Some(root.as_str()));
    }

    #[test]
    fn a_folder_from_the_portfolio_rs_command_opens_without_positions() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let root = dir.path().join("cli");
        workspace::init_workspace(&root.to_string_lossy(), false).unwrap();
        assert!(!root.join("positions.json").exists());
        let policy = fs::read_to_string(root.join("INVESTMENT_POLICY.md")).unwrap();

        let opened = runtime
            .block_on(backend.open_folder(root.to_string_lossy().into_owned()))
            .unwrap();
        assert!(matches!(opened, OpenedFolder::Loaded));
        assert_eq!(
            fs::read_to_string(root.join("positions.json")).unwrap(),
            "[]\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("INVESTMENT_POLICY.md")).unwrap(),
            policy,
            "the command line's files stay as they were"
        );
        let open = runtime.block_on(backend.open_document());
        assert_eq!(open.workspace_dir.as_deref(), Some(root.to_str().unwrap()));
        assert!(runtime.block_on(backend.snapshot()).positions.is_empty());
    }

    #[test]
    fn an_empty_folder_is_not_opened_as_a_workspace() {
        let dir = TempDir::new().unwrap();
        let empty = dir.path().join("empty");
        fs::create_dir(&empty).unwrap();
        let kind = classify_folder(&empty).unwrap();
        assert_eq!(
            kind,
            FolderKind::Other {
                dir: empty.to_string_lossy().into_owned(),
                empty: true,
            }
        );

        fs::write(dir.path().join("notes.txt"), "keep").unwrap();
        let kind = classify_folder(dir.path()).unwrap();
        assert!(matches!(kind, FolderKind::Other { empty: false, .. }));
    }

    #[test]
    fn the_folder_dialog_starts_beside_the_open_workspace() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("My Portfolio");
        fs::create_dir(&root).unwrap();
        assert_eq!(
            folder_dialog_start(Some(&root)).as_deref(),
            Some(dir.path())
        );
        assert_eq!(folder_dialog_start(None), None);
        assert_eq!(
            folder_dialog_start(Some(Path::new("/no/such/workspace"))),
            None
        );
    }

    #[test]
    fn a_positions_file_that_does_not_parse_is_explained() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);

        let explain = |name: &str, body: &str| {
            let path = dir.path().join(name);
            fs::write(&path, body).unwrap();
            runtime
                .block_on(backend.startup(Some(path.to_string_lossy().into_owned())))
                .unwrap()
                .parse_error
                .expect(name)
        };

        let empty = explain("empty.json", "");
        assert_eq!(empty.summary(), "The file is empty.");
        assert!(empty.reason.contains("EOF"), "{}", empty.reason);

        let whitespace = explain("blank.json", " \n\t");
        assert_eq!(whitespace.summary(), "The file is empty.");

        let broken = explain("broken.json", "{not json");
        assert!(
            broken.summary().starts_with("This isn't valid JSON (line "),
            "{}",
            broken.summary()
        );
        assert!(broken.summary().contains("column "), "{}", broken.summary());
        assert!(
            !broken.summary().contains("EOF"),
            "the raw parser text stays in the detail"
        );
        assert!(!broken.reason.is_empty());

        let object = explain("object.json", "{\"Name\":\"Cash\"}\n");
        assert_eq!(object.summary(), "Expected a list of positions.");
        assert!(
            object.reason.contains("sequence"),
            "raw detail kept: {}",
            object.reason
        );

        let cut_off = explain("cutoff.json", "[\n");
        assert!(
            cut_off
                .summary()
                .starts_with("This isn't valid JSON (line "),
            "a file that starts and then stops is not empty: {}",
            cut_off.summary()
        );
    }

    #[test]
    fn a_policy_does_not_follow_you_to_the_next_portfolio() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let backend = offline_backend(&runtime, &dir);
        let path = write_portfolio(&dir);
        let policy = Backend::policy_from_strategy("balanced-growth").unwrap();
        fs::create_dir_all(dir.path().join("portfolio")).unwrap();
        fs::write(
            dir.path().join("portfolio/policy.toml"),
            Backend::policy_to_toml(&policy).unwrap(),
        )
        .unwrap();

        runtime.block_on(async {
            let workspace = dir.path().to_string_lossy().into_owned();
            backend.load_workspace(workspace).await.unwrap();
            assert!(backend.policy().await.is_some());
            backend.load_simple_file(path).await.unwrap();
            assert!(backend.policy().await.is_none());
        });
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

    fn position(
        id: &str,
        price: Option<f64>,
        value: Option<f64>,
    ) -> portfolio_rs::mcp::snapshots::SnapshotPosition {
        portfolio_rs::mcp::snapshots::SnapshotPosition {
            position_id: id.to_string(),
            kind: if price.is_some() { "market" } else { "cash" }.to_string(),
            asset_class: "Stocks".to_string(),
            quantity: 1.0,
            price,
            price_currency: price.map(|_| "EUR".to_string()),
            fx_rate: Some(1.0),
            value_base: value,
            ledger_base: None,
            stale: Some(false),
        }
    }

    #[test]
    fn a_sparkline_uses_price_so_a_purchase_is_not_a_jump() {
        let mut first = point("2026-03-01", 100.0, true);
        let mut second = point("2026-03-02", 320.0, true);
        // Bought another share: value doubles-and-some, price only moves 10%.
        first.positions = vec![position("pos_spy", Some(100.0), Some(100.0))];
        second.positions = vec![position("pos_spy", Some(110.0), Some(220.0))];
        first.positions.push(position("pos_cash", None, Some(50.0)));
        second
            .positions
            .push(position("pos_cash", None, Some(50.0)));
        let mut once = point("2026-03-03", 1.0, true);
        once.positions = vec![position("pos_new", Some(5.0), Some(5.0))];

        let history = build_history(vec![first, second, once], &[], None);
        let spy = history.series("pos_spy").unwrap();
        assert_eq!(spy, [100.0, 110.0], "price, not the value after the buy");
        assert!(
            history
                .series
                .iter()
                .any(|series| series.position_id == "pos_spy" && series.priced)
        );
        assert_eq!(history.series("pos_cash"), Some([50.0, 50.0].as_slice()));
        assert!(
            history
                .series
                .iter()
                .any(|series| series.position_id == "pos_cash" && !series.priced),
            "cash has no price, so the line is its value"
        );
        assert_eq!(history.series("pos_new"), None, "one point is not a line");
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
    fn a_look_is_remembered_beside_the_history_and_measured_from() {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("portfolio");
        demo::seed_workspace(&root);
        let seen = root.join("portfolio/history/main").join(SEEN_FILE);
        fs::remove_file(&seen).ok();
        let backend = demo::backend(dir.path(), Some(&root), runtime.handle().clone());
        let today = Local::now().date_naive();

        runtime.block_on(async {
            backend.startup(None).await.unwrap();
            assert_eq!(backend.visit().await.unwrap(), None, "the first look");
            let again = backend.visit().await.unwrap();
            assert_eq!(
                again,
                Some(Seen {
                    last: today,
                    before: None
                })
            );
            let written = fs::read_to_string(&seen).unwrap();
            assert_eq!(
                written,
                format!(
                    "{{\"last\":\"{}\",\"before\":null}}\n",
                    today.format("%Y-%m-%d")
                ),
                "dates and nothing else"
            );

            let week = backend
                .change_since(Some(today - Duration::days(7)))
                .await
                .unwrap()
                .expect("the demo has 90 days of history");
            assert_eq!(week.from, today - Duration::days(7));
            assert!(week.added.abs() < 0.01, "only prices moved: {}", week.added);
            assert!(week.leader.is_some());
            assert_eq!(week.history_from, Some(today - Duration::days(90)));

            let oldest = backend.change_since(None).await.unwrap().unwrap();
            assert_eq!(oldest.from, today - Duration::days(90));
            assert!(
                backend
                    .change_since(Some(today - Duration::days(400)))
                    .await
                    .unwrap()
                    .is_none(),
                "nothing that old to compare with"
            );
        });
    }

    #[test]
    fn a_broken_record_of_looks_counts_as_none() {
        assert_eq!(read_seen("not json"), None);
        assert_eq!(read_seen(r#"{"last":"2026-11-10T00:00"}"#), None);
        let day = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let seen = Seen {
            last: day,
            before: Some(day - Duration::days(3)),
        };
        assert_eq!(read_seen(&write_seen(seen)), Some(seen));
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
