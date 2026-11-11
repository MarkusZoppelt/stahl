//! The whole app's state, and everything that changes it.
//!
//! Rune re-runs the views when this changes. `portfolio_rs` runs on Tokio, so a change that
//! needs it goes through [`State::run`]: the work runs off the UI thread, and its result lands
//! here in `poll`.

use std::{
    collections::HashMap,
    mem,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    time::{Duration, Instant},
};

use chrono::Local;
use portfolio_rs::{
    config::ThemeMode,
    doctor::WorkspaceHealth,
    mcp::dto::Finding,
    policy::Policy,
    review::Review,
    simulate::RebalanceSimulation,
    state::{
        AllocationItemDto, AppConfigDto, CreatePositionRequest, CreatePurchaseRequest,
        GeneratedDocument, PerformanceDto, PortfolioSummaryDto, PositionDto, UpdatePositionRequest,
    },
    validate::ValidationReport,
};
use rune::{ClipboardContent, Color, FileDialogOptions, Live, LiveText, TextEdit, Tx, Waker};
use rune_kit::shell::PaneState;

use crate::{
    backend::{
        self, Backend, BackendResult, Briefing, History, OpenDocument, OpenedFolder, Period, Quote,
        Snapshot, WorkspaceField, WorkspaceProblem,
    },
    format,
    statement::{self, Change, Facts, Seen, Since, Statement},
    update::{self, Installed, Release, Updater},
};

/// How long a chart takes to draw itself in.
pub const DRAW_IN: f64 = 0.9;
/// How far (points) a page is pulled past its top before letting go refreshes the prices.
pub const PULL_TO_REFRESH: f32 = 72.0;
/// How far a holding's row slides on a phone to show its actions.
pub const SWIPE_ACTIONS: f32 = 152.0;

/// A page pulled down past its top on a phone, in live values, so the pull moves the door that
/// shows it without a re-render.
#[derive(Clone)]
pub struct Pull {
    pub distance: Live<f32>,
    /// 0 to 1 as the pull nears what refreshes.
    pub shown: Live<f32>,
    pub scale: Live<f32>,
}

impl Pull {
    fn new() -> Pull {
        Pull {
            distance: Live::new(0.0),
            shown: Live::new(0.0),
            scale: Live::new(0.6),
        }
    }

    /// The page's scroll offset, negative while it's pulled past its top.
    pub fn track(&self, offset: f32) {
        let distance = (-offset).max(0.0);
        let near = (distance / PULL_TO_REFRESH).min(1.0);
        self.distance.set(distance);
        self.shown.set(near);
        self.scale.set(0.6 + 0.4 * near);
    }

    /// Whether letting go now refreshes.
    pub fn past(&self) -> bool {
        self.distance.get() >= PULL_TO_REFRESH
    }
}

/// How far each holding's row is slid open on a phone, which its scroller reports, and the row a
/// finger moved last.
#[derive(Default)]
pub struct SwipeRows {
    pub offsets: HashMap<usize, f32>,
    pub last: Option<usize>,
}

pub type Swipes = Arc<Mutex<SwipeRows>>;

/// The key of a holding's sliding row on a phone.
pub fn swipe_key(id: usize) -> String {
    format!("swipe-{id}")
}
/// Refresh prices on returning to the app, if the last refresh is older than this.
const STALE_AFTER: Duration = Duration::from_secs(15 * 60);
/// Into the welcome door's flight, when its dial starts turning: as it nears the mark.
pub const DOOR_TURN_AT: f64 = 0.45;
/// When the flight hands over to the mark: once the dial has turned and settled.
pub const DOOR_FLIGHT_ENDS: f64 = DOOR_TURN_AT + 1.35;

const TOAST_FOR: Duration = Duration::from_millis(2600);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Overview,
    Holdings,
    Allocation,
    Performance,
    Review,
    Rebalance,
    Policy,
    Documents,
    Briefing,
    Health,
    Settings,
}

impl Page {
    pub const PORTFOLIO: [Page; 4] = [
        Page::Overview,
        Page::Holdings,
        Page::Allocation,
        Page::Performance,
    ];
    pub const PLAN: [Page; 4] = [Page::Review, Page::Rebalance, Page::Policy, Page::Documents];
    pub const INSIGHTS: [Page; 2] = [Page::Briefing, Page::Health];
    /// The pages with a tab of their own on a phone. Everything else is under More.
    pub const TABS: [Page; 3] = [Page::Overview, Page::Holdings, Page::Performance];

    pub fn label(self) -> &'static str {
        match self {
            Page::Overview => "Overview",
            Page::Holdings => "Holdings",
            Page::Allocation => "Allocation",
            Page::Performance => "Performance",
            Page::Review => "Review",
            Page::Rebalance => "Rebalance",
            Page::Policy => "Policy",
            Page::Documents => "Documents",
            Page::Briefing => "Briefing",
            Page::Health => "Data Health",
            Page::Settings => "Settings",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Page::Overview => "nav-overview",
            Page::Holdings => "nav-holdings",
            Page::Allocation => "nav-allocation",
            Page::Performance => "nav-performance",
            Page::Review => "nav-review",
            Page::Rebalance => "nav-rebalance",
            Page::Policy => "nav-policy",
            Page::Documents => "nav-documents",
            Page::Briefing => "nav-briefing",
            Page::Health => "nav-health",
            Page::Settings => "nav-settings",
        }
    }

    pub fn needs_workspace(self) -> bool {
        Self::PLAN.contains(&self)
    }

    /// Its digit with the main modifier, for the pages that have one.
    pub fn shortcut(self) -> Option<char> {
        Some(match self {
            Page::Overview => '1',
            Page::Holdings => '2',
            Page::Allocation => '3',
            Page::Performance => '4',
            Page::Review => '5',
            Page::Rebalance => '6',
            Page::Policy => '7',
            Page::Documents => '8',
            Page::Briefing => '9',
            Page::Health => '0',
            Page::Settings => ',',
        })
    }

    pub fn from_shortcut(key: char) -> Option<Page> {
        Self::PORTFOLIO
            .into_iter()
            .chain(Self::PLAN)
            .chain(Self::INSIGHTS)
            .chain([Page::Settings])
            .find(|page| page.shortcut() == Some(key))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortBy {
    Name,
    Class,
    Value,
    Pnl,
    Day,
    /// How much of the portfolio's move today came from the holding.
    Impact,
}

/// A span measured on the value chart. The band and the readout are live, so a drag does not
/// rebuild the view on every move. `span` is what stays after the pointer lifts.
pub struct ChartMeasure {
    pub span: Option<(usize, usize)>,
    /// Set for the gesture, so hover doesn't rebuild the view underneath it. The view doesn't read it.
    pub dragging: bool,
    pub band: Live<[f32; 4]>,
    pub shown: Live<f32>,
    pub pin: Live<[f32; 2]>,
    pub ink: Live<Color>,
    pub dates: LiveText,
    pub change: LiveText,
    pub detail: LiveText,
}

impl Default for ChartMeasure {
    fn default() -> Self {
        Self::new()
    }
}

impl ChartMeasure {
    pub fn new() -> Self {
        Self {
            span: None,
            dragging: false,
            band: Live::new([0.0, 0.0, 0.0, 0.0]),
            shown: Live::new(0.0),
            pin: Live::new([0.0, 0.0]),
            ink: Live::new(Color::TRANSPARENT),
            dates: LiveText::new(""),
            change: LiveText::new(""),
            detail: LiveText::new(""),
        }
    }

    pub fn clear(&mut self) {
        self.span = None;
        self.dragging = false;
        self.shown.set(0.0);
    }
}

/// Data a page fetches when it opens. A failed reload keeps the last result.
pub struct Loadable<T> {
    pub data: Option<T>,
    pub loading: bool,
    pub error: Option<String>,
}

impl<T> Default for Loadable<T> {
    fn default() -> Self {
        Self {
            data: None,
            loading: false,
            error: None,
        }
    }
}

impl<T> Loadable<T> {
    fn finish(&mut self, result: BackendResult<T>) {
        self.loading = false;
        match result {
            Ok(data) => {
                self.data = Some(data);
                self.error = None;
            }
            Err(error) => self.error = Some(error),
        }
    }
}

/// The add/edit position form.
pub struct PositionForm {
    pub existing: Option<usize>,
    /// Purchases on the position being edited. If it has any, their total is its amount.
    pub purchases: usize,
    pub name: TextEdit,
    pub ticker: TextEdit,
    pub asset_class: TextEdit,
    pub amount: TextEdit,
    pub quantity: TextEdit,
    pub price: TextEdit,
    pub error: Option<FieldError>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FormField {
    Amount,
    Quantity,
    Price,
}

impl FormField {
    fn label(self) -> &'static str {
        match self {
            FormField::Amount => "Amount",
            FormField::Quantity => "Quantity",
            FormField::Price => "Price",
        }
    }
}

/// What's wrong with a form, and which field to mark.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FieldError {
    pub field: FormField,
    pub message: String,
}

impl PositionForm {
    fn new(existing: Option<&PositionDto>) -> PositionForm {
        let edit = |text: String| TextEdit::new(&text);
        let mut name = edit(existing.and_then(|p| p.name.clone()).unwrap_or_default());
        name.focused = true;
        PositionForm {
            existing: existing.map(|p| p.id),
            purchases: existing.map_or(0, |p| p.purchases.len()),
            name,
            ticker: edit(existing.and_then(|p| p.ticker.clone()).unwrap_or_default()),
            asset_class: edit(
                // As the policy templates spell it: Review matches asset classes by exact name.
                existing.map_or_else(|| "Stocks".to_string(), |p| p.asset_class.clone()),
            ),
            amount: edit(existing.map_or_else(|| "0".to_string(), |p| editable_number(p.amount))),
            quantity: TextEdit::new(""),
            price: TextEdit::new(""),
            error: None,
        }
    }

    fn number(&self, field: FormField) -> Result<Option<f64>, FieldError> {
        let text = match field {
            FormField::Amount => &self.amount.text,
            FormField::Quantity => &self.quantity.text,
            FormField::Price => &self.price.text,
        };
        let label = field.label();
        parse_number(text).map_err(|error| FieldError {
            field,
            message: match error {
                BadNumber::NotANumber => format!(
                    "{label} \"{}\" isn't a number. Use digits, like 1234.50, without a currency symbol.",
                    text.trim()
                ),
                BadNumber::Ambiguous { thousands, decimal } => format!(
                    "{label} \"{}\" could mean {thousands} or {decimal}. Write {thousands}, or {decimal}0 for the smaller number.",
                    text.trim()
                ),
            },
        })
    }

    /// A purchase can't have a negative quantity or price.
    fn purchase_number(&self, field: FormField) -> Result<Option<f64>, FieldError> {
        match self.number(field)? {
            Some(number) if number < 0.0 => Err(FieldError {
                field,
                message: format!("{} can't be negative.", field.label()),
            }),
            number => Ok(number),
        }
    }

    /// `portfolio_rs` takes the amount from the purchases, so an edit to it would have no effect.
    pub fn amount_from_purchases(&self) -> bool {
        self.purchases > 0
    }

    /// Amount, quantity and price. Quantity and price only exist on a new position.
    fn numbers(&self) -> Result<(f64, Option<f64>, Option<f64>), FieldError> {
        let amount = self.number(FormField::Amount)?.ok_or(FieldError {
            field: FormField::Amount,
            message: "Amount is required.".to_string(),
        })?;
        if self.existing.is_some() {
            return Ok((amount, None, None));
        }
        Ok((
            amount,
            self.purchase_number(FormField::Quantity)?,
            self.purchase_number(FormField::Price)?,
        ))
    }
}

/// The New Workspace form.
pub struct NewWorkspaceForm {
    pub name: TextEdit,
    pub location: TextEdit,
    /// Copy the positions of the open file, when there is one to copy.
    pub keep_positions: bool,
    pub problem: Option<WorkspaceProblem>,
    /// Creating one is under way, so a second press mustn't start another.
    pub busy: bool,
}

/// Replacing the open portfolio, held until unsaved edits are saved or dropped.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum PendingOpen {
    /// Show the positions-file dialog.
    File,
    /// Show the workspace dialog.
    Workspace,
    /// Open this file, or this folder as a workspace.
    Path(String),
    /// Back to the welcome screen.
    Close,
    /// Create the workspace the open form describes.
    CreateWorkspace,
    /// Turn this existing folder into a workspace and open it.
    StartIn(String),
    /// Read the file again from disk.
    Reload,
    /// Finish launching: the command line, or the portfolio remembered from last time.
    Startup,
}

/// The chosen folder is not a workspace. Shown instead of whatever was open, until they decide.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FolderPrompt {
    pub dir: String,
    pub empty: bool,
    pub error: Option<String>,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Confirm {
    Delete {
        id: usize,
        name: String,
    },
    Quit,
    /// Save, drop, or keep the edits before [`PendingOpen`] replaces the portfolio.
    Unsaved(PendingOpen),
}

/// What the command palette can do.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Go(Page),
    Reveal(usize),
    Refresh,
    Save,
    OpenFile,
    OpenWorkspace,
    NewWorkspace,
    Guide,
    Privacy,
    Theme,
    Sidebar,
    AddPosition,
    CopyBriefing,
    CheckForUpdates,
    Close,
}

pub struct Palette {
    pub query: TextEdit,
    pub selected: usize,
}

pub struct SettingsForm {
    pub currency: TextEdit,
    pub provider_url: TextEdit,
    pub api_key: TextEdit,
    pub model: TextEdit,
    pub message: Option<String>,
    pub error: Option<String>,
}

/// How many classes the rebalance bar prepares live values for.
const REBALANCE_SLOTS: usize = 12;

/// What produced the split the bar is showing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RebalanceOrigin {
    /// The portfolio as it is.
    Today,
    /// New money, or a withdrawal, placed by buying or selling only.
    Flow(f64),
    /// Every class at its policy target.
    Full,
    /// Halfway back, and only for classes outside their band.
    Conservative,
    /// A boundary the owner dragged or stepped.
    Custom,
}

/// The allocation being dragged on the Rebalance page. Live values move the bar without
/// rebuilding the view; the view must not read `press`, `held`, `drag_values`, `step` or
/// `painted`.
pub struct RebalancePage {
    /// Class names `values` belong to. A mismatch means the portfolio changed under a drag.
    pub names: Vec<String>,
    pub values: Vec<f64>,
    pub origin: RebalanceOrigin,
    /// New money, or a withdrawal. Parsed with [`parse_number`].
    pub amount: TextEdit,
    pub amount_error: Option<String>,
    /// A boundary is held. The bar follows the pointer on a quicker spring.
    pub dragging: bool,
    /// The bar has been measured, so its segments can follow live rects.
    pub placed: bool,
    pub bar_width: f32,
    pub boundary: Option<usize>,
    pub press: Vec<f64>,
    pub held: Option<f64>,
    pub drag_values: Vec<f64>,
    /// The quarter point the dragged class last committed at, so a drag rebuilds the
    /// view only when the figures under the bar would change.
    pub step: i64,
    /// What the bar last drew, so a change from anywhere else glides it there.
    pub painted: u64,
    /// Each class's bar, its in-band lane under it, and the boundary after it.
    pub rects: Vec<Live<[f32; 4]>>,
    pub lanes: Vec<Live<[f32; 4]>>,
    pub offsets: Vec<Live<[f32; 2]>>,
}

impl RebalancePage {
    fn new() -> Self {
        let rects = || (0..REBALANCE_SLOTS).map(|_| Live::new([0.0; 4])).collect();
        Self {
            names: Vec::new(),
            values: Vec::new(),
            origin: RebalanceOrigin::Today,
            amount: TextEdit::new(""),
            amount_error: None,
            dragging: false,
            placed: false,
            bar_width: 0.0,
            boundary: None,
            press: Vec::new(),
            held: None,
            drag_values: Vec::new(),
            step: 0,
            painted: 0,
            rects: rects(),
            lanes: rects(),
            offsets: (0..REBALANCE_SLOTS)
                .map(|_| Live::new([0.0, 0.0]))
                .collect(),
        }
    }
}

pub struct PolicyPage {
    pub policy: Option<Policy>,
    pub editor: TextEdit,
    pub loading: bool,
    pub message: Option<String>,
    pub error: Option<String>,
}

pub struct DocumentsPage {
    pub title: TextEdit,
    pub generated: Option<GeneratedDocument>,
    pub loading: bool,
    pub message: Option<String>,
    pub error: Option<String>,
}

/// How far installing an update has got.
#[derive(Clone, PartialEq, Debug)]
pub enum UpdatePhase {
    Idle,
    Installing,
    /// The new version is in place, and starts on the next launch.
    Installed,
    /// A verified package is here, for someone with the rights to install it.
    Saved(PathBuf),
    Failed(String),
}

pub struct Updates {
    pub updater: Updater,
    /// The newest release this copy could move to.
    pub available: Option<Release>,
    /// Asked not to hear about it again until the next launch.
    pub dismissed: bool,
    pub dialog: bool,
    pub phase: UpdatePhase,
    pub checking: bool,
    /// Nobody asked for the check that's running, so it only speaks up about a new version.
    pub check_is_quiet: bool,
    /// The update worked, but the restart didn't.
    pub restart_error: Option<String>,
}

type Job = Box<dyn FnOnce(&mut State) + Send>;

/// What a picker is for: an existing file, or an existing folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickKind {
    File,
    Folder,
}

/// What to do with a picked path.
type PickThen = Box<dyn FnOnce(&mut State, String)>;

/// A question for the platform's own picker, where it shows one in place of Rune's dialogs (iOS,
/// whose embedded views can't show those). The platform answers with [`State::picked`].
pub struct Pick {
    pub kind: PickKind,
    pub title: String,
    asked: bool,
    then: PickThen,
}

pub struct State {
    pub backend: Backend,
    jobs_tx: Sender<Job>,
    jobs_rx: Receiver<Job>,
    pending: usize,
    /// Work landed outside `poll`, so the next `poll` must report a change.
    settled: bool,
    waker: Option<Waker>,
    /// Reopens the window after a close with unsaved changes.
    pub reopen: Option<Box<dyn Fn() + Send + Sync>>,
    initial_file: Option<String>,
    started: bool,

    // What's open.
    pub config: Option<AppConfigDto>,
    pub open: OpenDocument,
    pub summary: Option<PortfolioSummaryDto>,
    pub positions: Vec<PositionDto>,
    /// What [`Self::positions`] leaves out, at the index that is the position's id.
    pub quotes: Vec<Quote>,
    pub allocation: Vec<AllocationItemDto>,
    pub performance: Option<PerformanceDto>,
    pub findings: Vec<Finding>,
    /// connected | partial | offline | fixture
    pub network: String,
    pub has_policy: bool,
    /// Position changes not written to disk yet.
    pub dirty: bool,
    pub loading: bool,
    pub starting: bool,
    pub error: Option<String>,
    pub updated_at: Option<String>,
    /// How many times prices have landed, to mark the moment on screen.
    pub price_updates: u64,
    /// One full turn when a portfolio's door first appears: as it lands from the welcome screen,
    /// or on its own when the app opened straight into a portfolio. Not a price refresh, and not
    /// replayed by one.
    pub door_arrivals: u64,
    /// The compositor draws this window, so views paint the door with the shader rather than the
    /// picture. The compositor sets it on the first frame it renders: false in tests, which never
    /// draw, and on iOS, which keeps the picture.
    pub shaders: bool,
    /// The view has been built since `shaders` turned true.
    shader_view: bool,
    /// The door has been painted at rest for this portfolio, so the next frame can start the turn.
    door_seen: bool,
    /// The arrival turn has started for the portfolio now open.
    door_landed: bool,
    /// Show the welcome screen until this time even with a portfolio open, so a scene can paint
    /// the large door and then let it fly. `None` in the running app.
    pub hold_welcome_until: Option<f64>,
    /// When the welcome door started its flight to the mark, while it is still travelling.
    pub door_flight: Option<f64>,
    /// The flying door has been drawn where the welcome door was, and now heads for the mark.
    pub door_released: bool,
    /// Where the welcome door and the mark were last laid out, in window points: the flight's
    /// two ends.
    pub welcome_door: Option<[f32; 4]>,
    pub mark_door: Option<[f32; 4]>,
    /// The welcome screen was showing at the last poll.
    welcome_showing: bool,
    /// What the last refresh moved: each position (by id) whose price changed, and by how much,
    /// as a fraction. Empty when it moved nothing.
    pub price_moves: Vec<(usize, f64)>,
    refreshed: Option<Instant>,

    // The window.
    pub page: Page,
    /// The page before this one, so the way between them can show which way the owner went.
    pub previous_page: Page,
    /// The phone layout: a tab bar instead of the sidebar, one column, the details in place of the
    /// table. The platform decides (on iOS, UIKit's horizontal size class), not the window's width.
    pub compact: bool,
    /// In the phone layout, the More tab's list of pages is showing instead of a page.
    pub more: bool,
    /// Quarter turns of the door in the phone's top bar. Four seal the vault.
    pub door_turns: u32,
    /// A page pulled past its top on a phone, and whether letting go started the refresh running.
    pub pull: Pull,
    pub pull_refreshing: bool,
    /// Holdings' rows slid open on a phone to show their actions.
    pub swipes: Swipes,
    /// Whether opening the app asks for Face ID first: `None` where there's no such lock.
    pub lock: Option<bool>,
    /// What the platform calls its lock: "Face ID", "Touch ID" or "Passcode".
    pub lock_name: String,
    /// A software keyboard covers the bottom of the screen: the phone layout hides its tab bar.
    pub keyboard: bool,
    /// The platform shows its own picker for files and folders: see [`Pick`].
    pub platform_picker: bool,
    pub pick: Option<Pick>,
    pub privacy: bool,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub drop_hover: bool,
    pub palette: Option<Palette>,
    pub form: Option<PositionForm>,
    pub new_workspace: Option<NewWorkspaceForm>,
    pub guide: bool,
    /// Set when a folder was opened and it is not a workspace. The previous portfolio stays in
    /// memory, and is not drawn, until this is cleared.
    pub folder_prompt: Option<FolderPrompt>,
    pub confirm: Option<Confirm>,
    /// Runs once a save or a revert of unsaved edits has finished.
    resume: Option<PendingOpen>,
    pub toast: Option<(String, Instant)>,
    toast_armed: bool,
    title_shown: Option<String>,
    quit_requested: bool,

    // Charts.
    pub now: f64,
    pub draw_started: f64,
    restart_draw: bool,
    pub period: Period,
    pub history: Option<History>,
    pub history_error: Option<String>,
    /// The newest request. Answers to older ones are for a period nobody is looking at.
    history_request: u64,
    pub chart_hover: Option<f32>,
    /// The chart's point nearest the pointer or finger, for a tick as it passes each one.
    pub chart_point: Option<usize>,
    /// A span dragged out on the value chart, kept until dismissed.
    pub measure: ChartMeasure,
    pub donut_hover: Option<usize>,

    // Pages.
    pub search: TextEdit,
    /// Holdings drawn as a treemap instead of the table.
    pub holdings_map: bool,
    pub sort: SortBy,
    pub ascending: bool,
    pub selected: Option<usize>,
    pub review: Loadable<Review>,
    pub simulation: Loadable<RebalanceSimulation>,
    /// The allocation the Rebalance page is being played with.
    pub rebalance: RebalancePage,
    pub briefing: Loadable<Briefing>,
    pub validation: Loadable<ValidationReport>,
    pub doctor: Loadable<WorkspaceHealth>,
    pub policy: PolicyPage,
    pub documents: DocumentsPage,
    pub settings: SettingsForm,
    pub has_api_key: bool,
    pub updates: Updates,

    // Since the last look.
    /// The portfolio this session's look was noted for, so reopening it doesn't count again.
    seen_for: Option<String>,
    /// When the owner last looked before this time: `None` until it's known.
    pub since: Option<Since>,
    /// The portfolio since then, from the stored valuations.
    pub change: Option<Change>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum BadNumber {
    NotANumber,
    /// `1.000` is one to some readers and a thousand to others. Holds both readings.
    Ambiguous {
        thousands: String,
        decimal: String,
    },
}

/// Marks that only ever separate thousands: `1 000`, `1'000`, and the spaces locales format with.
fn is_group_mark(mark: char) -> bool {
    matches!(mark, ' ' | '\u{a0}' | '\u{202f}' | '\'' | '\u{2019}')
}

/// A blank field is `None`; anything else must be a finite number, in digits.
///
/// Both `.` and `,` work as the decimal mark, because the app can't know which one someone means.
/// This is financial data, so a mark is only ever read one way when no other reading is possible:
///
/// - With two different marks, the last one is the decimal: `1,000.50` and `1.000,50`.
/// - A mark that repeats separates thousands: `1.000.000`. So does a space or an apostrophe.
/// - A lone `.` or `,` is the decimal, like `1,5`, `0.125` or `1234.567`, unless exactly three
///   digits follow up to three others. `1.000` and `12,345` could each be off by a thousand, so
///   those are refused rather than guessed. Leading zeros don't count: the form's default `0` can
///   turn a typed `1.000` into `01.000`.
pub fn parse_number(text: &str) -> Result<Option<f64>, BadNumber> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let (sign, body) = match text.strip_prefix('-') {
        Some(body) => ("-", body),
        None => ("", text.strip_prefix('+').unwrap_or(text)),
    };
    let marks: Vec<char> = body.chars().filter(|c| !c.is_ascii_digit()).collect();
    let is_mark = |mark: &char| matches!(mark, '.' | ',') || is_group_mark(*mark);
    if !body.contains(|c: char| c.is_ascii_digit()) || !marks.iter().all(is_mark) {
        return Err(BadNumber::NotANumber);
    }

    let decimal = match marks.as_slice() {
        [] => None,
        [only] => (!is_group_mark(*only)).then_some(*only),
        [first, .., last] => (first != last).then_some(*last),
    };
    if decimal.is_some_and(is_group_mark) {
        return Err(BadNumber::NotANumber);
    }
    let (whole, fraction) = decimal
        .and_then(|mark| body.rsplit_once(mark))
        .unwrap_or((body, ""));

    // Whatever is left in the whole part separates groups of three, and is one kind of mark.
    let mut group_marks = whole.chars().filter(|c| !c.is_ascii_digit());
    let digits = match group_marks.next() {
        None => whole.to_string(),
        Some(mark) => {
            let mut groups = whole.split(mark);
            let first = groups.next().unwrap_or_default();
            let well_formed = group_marks.all(|other| other == mark)
                && !first.is_empty()
                && first.trim_start_matches('0').len() <= 3
                && groups.all(|group| group.len() == 3);
            if !well_formed {
                return Err(BadNumber::NotANumber);
            }
            whole.chars().filter(char::is_ascii_digit).collect()
        }
    };
    let digits = digits.trim_start_matches('0');

    if marks.len() == 1 && fraction.len() == 3 && (1..=3).contains(&digits.len()) {
        return Err(BadNumber::Ambiguous {
            thousands: format!("{sign}{digits}{fraction}"),
            decimal: format!("{sign}{digits}.{fraction}"),
        });
    }
    match format!("{sign}0{digits}.{fraction}0").parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(Some(number)),
        _ => Err(BadNumber::NotANumber),
    }
}

/// A number as the form shows it, so that [`parse_number`] reads back exactly that number.
fn editable_number(number: f64) -> String {
    let text = number.to_string();
    match parse_number(&text) {
        // An extra digit takes `12.345` out of the range that could mean thousands.
        Err(BadNumber::Ambiguous { .. }) => text + "0",
        _ => text,
    }
}

/// Whether applying `request` would leave `position` any different.
fn changes(request: &UpdatePositionRequest, position: &PositionDto) -> bool {
    request
        .name
        .as_ref()
        .is_some_and(|name| position.name.as_ref() != Some(name))
        || request
            .ticker
            .as_ref()
            .is_some_and(|ticker| position.ticker.as_ref() != Some(ticker))
        || request
            .asset_class
            .as_ref()
            .is_some_and(|class| *class != position.asset_class)
        || request
            .amount
            .is_some_and(|amount| amount != position.amount)
}

fn text_or_none(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

impl State {
    pub fn new(backend: Backend, initial_file: Option<String>) -> State {
        let (jobs_tx, jobs_rx) = channel();
        State {
            backend,
            jobs_tx,
            jobs_rx,
            pending: 0,
            settled: false,
            waker: None,
            reopen: None,
            initial_file,
            started: false,
            config: None,
            open: OpenDocument::default(),
            summary: None,
            positions: Vec::new(),
            quotes: Vec::new(),
            allocation: Vec::new(),
            performance: None,
            findings: Vec::new(),
            network: String::new(),
            has_policy: false,
            dirty: false,
            loading: false,
            starting: true,
            error: None,
            updated_at: None,
            price_updates: 0,
            door_arrivals: 0,
            shaders: false,
            shader_view: false,
            door_seen: false,
            door_landed: false,
            hold_welcome_until: None,
            door_flight: None,
            door_released: false,
            welcome_door: None,
            mark_door: None,
            welcome_showing: false,
            price_moves: Vec::new(),
            refreshed: None,
            page: Page::Overview,
            previous_page: Page::Overview,
            compact: false,
            more: false,
            door_turns: 0,
            pull: Pull::new(),
            pull_refreshing: false,
            swipes: Swipes::default(),
            lock: None,
            lock_name: String::new(),
            keyboard: false,
            platform_picker: false,
            pick: None,
            privacy: false,
            sidebar: PaneState::new(224.0).range(170.0, 340.0),
            inspector: PaneState::new(320.0).range(260.0, 460.0).collapsed(),
            drop_hover: false,
            palette: None,
            form: None,
            new_workspace: None,
            guide: false,
            folder_prompt: None,
            confirm: None,
            resume: None,
            toast: None,
            toast_armed: false,
            title_shown: None,
            quit_requested: false,
            now: 0.0,
            draw_started: f64::NEG_INFINITY,
            restart_draw: false,
            period: Period::default(),
            history: None,
            history_error: None,
            history_request: 0,
            chart_hover: None,
            chart_point: None,
            measure: ChartMeasure::new(),
            donut_hover: None,
            search: TextEdit::new(""),
            holdings_map: false,
            sort: SortBy::Value,
            ascending: false,
            selected: None,
            review: Loadable::default(),
            simulation: Loadable::default(),
            rebalance: RebalancePage::new(),
            briefing: Loadable::default(),
            validation: Loadable::default(),
            doctor: Loadable::default(),
            policy: PolicyPage {
                policy: None,
                editor: TextEdit::new(""),
                loading: false,
                message: None,
                error: None,
            },
            documents: DocumentsPage {
                title: TextEdit::new(""),
                generated: None,
                loading: false,
                message: None,
                error: None,
            },
            settings: SettingsForm {
                currency: TextEdit::new(""),
                provider_url: TextEdit::new(""),
                api_key: TextEdit::new("").secure(),
                model: TextEdit::new(""),
                message: None,
                error: None,
            },
            has_api_key: false,
            updates: Updates {
                updater: Updater::disabled(),
                available: None,
                dismissed: false,
                dialog: false,
                phase: UpdatePhase::Idle,
                checking: false,
                check_is_quiet: false,
                restart_error: None,
            },
            seen_for: None,
            since: None,
            change: None,
        }
    }

    /// Off by default, so only the app itself ever reaches GitHub or replaces anything.
    pub fn with_updater(mut self, updater: Updater) -> State {
        self.updates.updater = updater;
        self
    }

    pub fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    // --- Running work off the UI thread ---

    fn run<T, F>(
        &mut self,
        work: impl FnOnce(Backend) -> F,
        done: impl FnOnce(&mut State, BackendResult<T>) + Send + 'static,
    ) where
        T: Send + 'static,
        F: Future<Output = BackendResult<T>> + Send + 'static,
    {
        self.pending += 1;
        let backend = self.backend.clone();
        let task = backend.run(work(backend.clone()));
        let jobs = self.jobs_tx.clone();
        let waker = self.waker.clone();
        backend.spawn(async move {
            let result = task.await;
            // The window may be gone by now, and then nobody wants the answer.
            let _ = jobs.send(Box::new(move |state: &mut State| done(state, result)));
            if let Some(waker) = waker {
                waker();
            }
        });
    }

    /// Apply finished work. True when any landed.
    pub fn drain(&mut self) -> bool {
        let mut landed = false;
        while let Ok(job) = self.jobs_rx.try_recv() {
            self.pending -= 1;
            job(self);
            landed = true;
        }
        landed
    }

    /// Wait for all work in flight, for headless runs that need the result now.
    pub fn settle(&mut self) {
        while self.pending > 0 {
            match self.jobs_rx.recv_timeout(Duration::from_secs(30)) {
                Ok(job) => {
                    self.pending -= 1;
                    job(self);
                    self.settled = true;
                }
                Err(_) => break,
            }
        }
    }

    /// Runs at the start of every frame. True when something on screen changed.
    pub fn poll(&mut self, now: f64, tx: &mut Tx) -> bool {
        self.now = now;
        let from_welcome = self.welcome_showing;
        crate::view::rebalance::sync(self);
        let mut changed = self.drain() || mem::take(&mut self.settled);
        // The compositor found the shader door on its first frame: build the view that uses it.
        if self.shaders && !self.shader_view {
            self.shader_view = true;
            changed = true;
        }
        changed |= self.door_arrives(now, from_welcome, tx);
        self.welcome_showing = self.on_welcome();
        if self.quit_requested {
            tx.quit();
        }
        if self.restart_draw {
            self.restart_draw = false;
            self.draw_started = now;
            changed = true;
        }
        // One frame past the end, or the last rebuild can land just short of finished.
        if now - self.draw_started < DRAW_IN + 0.1 {
            changed = true;
        }
        if self.toast_armed {
            self.toast_armed = false;
            tx.wake_after(TOAST_FOR + Duration::from_millis(50));
        }
        if self
            .toast
            .as_ref()
            .is_some_and(|(_, shown)| shown.elapsed() >= TOAST_FOR)
        {
            self.toast = None;
            changed = true;
        }
        // A save or revert asked for from the unsaved-changes dialog finishes in `drain`, above.
        if self.resume.is_some() && !self.loading {
            let next = self.resume.take().expect("resume is set");
            if self.error.is_none() && !self.dirty {
                self.perform(next, tx);
                changed = true;
            }
        }
        let title = if self.dirty {
            "Stahl Capital — Edited"
        } else {
            "Stahl Capital"
        };
        if self.title_shown.as_deref() != Some(title) {
            tx.set_title(title);
            self.title_shown = Some(title.to_string());
        }
        changed
    }

    pub fn show_toast(&mut self, message: impl Into<String>) {
        self.toast = Some((message.into(), Instant::now()));
        self.toast_armed = true;
    }

    /// A tap on the door in the phone's top bar turns it a quarter. A full turn seals the vault.
    pub fn turn_door(&mut self) {
        self.door_turns += 1;
        if self.door_turns.is_multiple_of(4) {
            self.show_toast("Vault sealed. One client, safe inside.");
        }
    }

    /// Shaking the phone hides the amounts, or shows them again, from anywhere.
    pub fn shaken(&mut self) {
        self.privacy = !self.privacy;
        self.show_toast(if self.privacy {
            "Amounts hidden"
        } else {
            "Amounts shown"
        });
    }

    /// A finger let go of a page: pulled far enough past its top, the prices refresh. Returns
    /// whether they do.
    pub fn pull_released(&mut self) -> bool {
        if !self.pull.past() || self.loading || !self.has_portfolio() {
            return false;
        }
        self.refresh_prices();
        self.pull_refreshing = self.loading;
        self.pull_refreshing
    }

    /// Settle the rows a finger slid: the one it moved last opens if it's more than half open,
    /// and every other row shuts, so one row's actions show at a time.
    pub fn settle_swipes(&mut self, tx: &mut Tx) {
        let Ok(rows) = self.swipes.lock() else {
            return;
        };
        let open = rows.last.filter(|id| {
            rows.offsets
                .get(id)
                .is_some_and(|x| *x > SWIPE_ACTIONS / 2.0)
        });
        for (id, offset) in &rows.offsets {
            let target = if Some(*id) == open {
                SWIPE_ACTIONS
            } else {
                0.0
            };
            if (offset - target).abs() > 0.5 {
                tx.scroll_to(swipe_key(*id), target, 0.0);
            }
        }
    }

    /// The system's back gesture, as a swipe in from the left edge of a phone: it closes what
    /// floats over the page, then a holding's details, then a page reached through More.
    /// Returns whether there was anywhere to go back to.
    pub fn go_back(&mut self) -> bool {
        if self.palette.take().is_some()
            || self.form.take().is_some()
            || self.confirm.take().is_some()
            || self.new_workspace.take().is_some()
        {
            return true;
        }
        if self.guide {
            self.guide = false;
            return true;
        }
        if !self.compact {
            return false;
        }
        if self.page == Page::Holdings && self.selected.is_some() {
            self.selected = None;
            self.inspector.set_open(false);
            return true;
        }
        if !self.more && !Page::TABS.contains(&self.page) {
            self.more = true;
            return true;
        }
        false
    }

    /// Ask for Face ID on opening the app, or stop asking.
    pub fn toggle_lock(&mut self) {
        let Some(on) = self.lock.map(|on| !on) else {
            return;
        };
        self.lock = Some(on);
        self.show_toast(if on {
            format!("{} locks the vault", self.lock_name)
        } else {
            "The vault opens without asking".to_string()
        });
    }

    /// Whether the field with focus takes a number, so a phone can offer digits.
    pub fn number_focused(&self) -> bool {
        self.form
            .as_ref()
            .is_some_and(|form| form.amount.focused || form.quantity.focused || form.price.focused)
            || self.rebalance.amount.focused
    }

    /// 0 to 1 through a chart's draw-in.
    pub fn draw_progress(&self, reduced_motion: bool) -> f32 {
        if reduced_motion {
            return 1.0;
        }
        let t = ((self.now - self.draw_started) / DRAW_IN).clamp(0.0, 1.0) as f32;
        1.0 - (1.0 - t).powi(3)
    }

    // --- Formatting ---

    pub fn currency(&self) -> String {
        self.config
            .as_ref()
            .map_or_else(|| "EUR".to_string(), |config| config.currency.clone())
    }

    pub fn money(&self, value: f64) -> String {
        if self.privacy {
            format::HIDDEN.to_string()
        } else {
            format::currency(value, &self.currency())
        }
    }

    pub fn signed_money(&self, value: f64) -> String {
        if self.privacy {
            format::HIDDEN.to_string()
        } else {
            format::signed_currency(value, &self.currency())
        }
    }

    pub fn optional_money(&self, value: Option<f64>) -> String {
        value.map_or_else(|| "-".to_string(), |value| self.money(value))
    }

    fn quote(&self, position: &PositionDto) -> Option<&Quote> {
        // The positions and quotes are read one after the other, so check they still line up:
        // better no currency than another holding's.
        self.quotes
            .get(position.id)
            .filter(|quote| quote.ticker == position.ticker)
    }

    /// A holding's price, average cost or purchase price, in its own currency. Without a known
    /// currency it's a bare number: never one with the wrong symbol.
    pub fn unit_money(&self, position: &PositionDto, value: f64) -> String {
        if self.privacy {
            return format::HIDDEN.to_string();
        }
        match self
            .quote(position)
            .and_then(|quote| quote.currency.as_ref())
        {
            Some(currency) => format::currency(value, currency),
            None => format::amount(value),
        }
    }

    /// A holding's price in the base currency, when its quote is in another.
    pub fn base_price(&self, position: &PositionDto) -> Option<f64> {
        self.quote(position).and_then(|quote| quote.base_price)
    }

    // --- Opening ---

    pub fn is_workspace(&self) -> bool {
        self.open.workspace_dir.is_some()
    }

    pub fn has_portfolio(&self) -> bool {
        // A folder prompt is a decision about a folder that did not open. The portfolio
        // underneath is not what's on screen, and commands must not change it.
        self.folder_prompt.is_none() && self.open.is_open() && self.open.parse_error.is_none()
    }

    /// The shell is up: a portfolio is open, and a scene isn't still holding the welcome screen.
    fn shell_showing(&self) -> bool {
        self.has_portfolio() && self.hold_welcome_until.is_none_or(|at| self.now >= at)
    }

    /// The welcome screen, with its large door, is what the window shows.
    fn on_welcome(&self) -> bool {
        !self.starting
            && self.folder_prompt.is_none()
            && self.open.parse_error.is_none()
            && !self.shell_showing()
    }

    /// The door's arrival when a portfolio's shell first shows. From the welcome screen, the
    /// welcome door flies to the mark and its dial turns as it lands; launched straight into a
    /// portfolio, the mark's dial just turns. Once per opened portfolio: a refresh, a page
    /// change or a redraw never comes through here again. Returns whether anything changed.
    fn door_arrives(&mut self, now: f64, from_welcome: bool, tx: &mut Tx) -> bool {
        if !self.shell_showing() {
            if !self.has_portfolio() {
                self.door_seen = false;
                self.door_landed = false;
                self.door_flight = None;
            }
            return false;
        }
        let mut changed = false;
        if !self.door_seen {
            // The first poll with the shell up. The turn waits a frame at least, so the dial is
            // painted at rest before it turns.
            self.door_seen = true;
            if from_welcome && self.welcome_door.is_some() {
                self.door_flight = Some(now);
                self.door_released = false;
                tx.wake_after(Duration::from_millis(20));
                tx.wake_at(now + DOOR_TURN_AT);
                tx.wake_at(now + DOOR_FLIGHT_ENDS);
                changed = true;
            } else {
                tx.wake_after(Duration::from_millis(30));
            }
        } else if self.door_flight.is_some() && !self.door_released {
            // It has been drawn at the welcome door's place: now it can set off.
            if self.mark_door.is_some() {
                self.door_released = true;
                changed = true;
            } else {
                tx.wake_after(Duration::from_millis(20));
            }
        } else if !self.door_landed
            && self
                .door_flight
                .is_none_or(|start| now >= start + DOOR_TURN_AT)
        {
            self.door_landed = true;
            self.door_arrivals += 1;
            changed = true;
        }
        if self
            .door_flight
            .is_some_and(|start| now >= start + DOOR_FLIGHT_ENDS)
        {
            self.door_flight = None;
            changed = true;
        }
        changed
    }

    pub fn theme_mode(&self) -> ThemeMode {
        self.config
            .as_ref()
            .map_or(ThemeMode::Dark, |config| config.theme)
    }

    pub fn is_dark(&self) -> bool {
        self.theme_mode() == ThemeMode::Dark
    }

    pub fn startup(&mut self) {
        if self.started {
            return;
        }
        if self.blocked_by_edits(PendingOpen::Startup) {
            return;
        }
        self.started = true;
        let initial_file = self.initial_file.take();
        self.run(
            move |backend| async move {
                let config = backend.config().await;
                let open = backend.startup(initial_file).await;
                let readable = open
                    .as_ref()
                    .is_ok_and(|open| open.is_open() && open.parse_error.is_none());
                let snapshot = if readable {
                    backend.record_today_or_warn().await;
                    Some(backend.snapshot().await)
                } else {
                    None
                };
                Ok((config, open, snapshot))
            },
            |state, result| {
                state.starting = false;
                match result {
                    Ok((config, open, snapshot)) => {
                        state.config = Some(config);
                        match open {
                            Ok(open) => state.open = open,
                            Err(error) => state.error = Some(error),
                        }
                        if let Some(snapshot) = snapshot {
                            state.apply_snapshot(snapshot);
                        }
                    }
                    Err(error) => state.error = Some(error),
                }
            },
        );
        if self.updates.updater.checks_on_startup() {
            self.check_for_updates(true);
        }
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.summary = Some(snapshot.summary);
        self.positions = snapshot.positions;
        self.quotes = snapshot.quotes;
        self.allocation = snapshot.allocation;
        self.performance = Some(snapshot.performance);
        self.findings = snapshot.findings;
        self.network = snapshot.network;
        self.has_policy = snapshot.has_policy;
        self.refreshed = Some(Instant::now());
        self.updated_at = Some(Local::now().format("%H:%M").to_string());
        self.load_page();
        self.load_since();
    }

    /// Run a change, then re-read everything it can affect.
    fn change_and_reload<F>(
        &mut self,
        record: bool,
        after: impl FnOnce(&mut State) + Send + 'static,
        work: impl FnOnce(Backend) -> F + Send + 'static,
    ) where
        F: Future<Output = BackendResult<()>> + Send + 'static,
    {
        self.loading = true;
        self.error = None;
        self.run(
            move |backend| async move {
                work(backend.clone()).await?;
                let open = backend.open_document().await;
                if record && open.parse_error.is_none() {
                    backend.record_today_or_warn().await;
                }
                let snapshot = if open.parse_error.is_none() {
                    Some(backend.snapshot().await)
                } else {
                    None
                };
                Ok((open, snapshot, backend.config().await))
            },
            move |state, result| {
                state.loading = false;
                match result {
                    Ok((open, snapshot, config)) => {
                        state.open = open;
                        state.config = Some(config);
                        if let Some(snapshot) = snapshot {
                            state.apply_snapshot(snapshot);
                        }
                        after(state);
                        if !state.has_portfolio() {
                            state.form = None;
                        }
                    }
                    Err(error) => state.error = Some(error),
                }
            },
        );
    }

    /// True when unsaved edits have to be dealt with before `next`. The dialog is up.
    fn blocked_by_edits(&mut self, next: PendingOpen) -> bool {
        if !self.dirty {
            return false;
        }
        self.palette = None;
        self.confirm = Some(Confirm::Unsaved(next));
        true
    }

    /// Save or drop the edits the dialog is asking about, then do what was waiting.
    pub fn confirm_unsaved(&mut self, save: bool) {
        let Some(Confirm::Unsaved(next)) = self.confirm.take() else {
            return;
        };
        if save && !self.has_portfolio() {
            self.confirm = Some(Confirm::Unsaved(next));
            self.error =
                Some("These changes can't be saved, because no portfolio is open.".to_string());
            return;
        }
        self.resume = Some(next);
        if save {
            self.save();
        } else {
            self.revert_edits();
        }
    }

    /// Put the open file back to what's on disk, so dropping edits doesn't leave them on screen.
    fn revert_edits(&mut self) {
        self.change_and_reload(
            false,
            |state| state.dirty = false,
            |backend| async move { backend.reload().await },
        );
    }

    fn perform(&mut self, next: PendingOpen, tx: &mut Tx) {
        match next {
            PendingOpen::File => self.show_file_dialog(tx),
            PendingOpen::Workspace => self.show_workspace_dialog(tx),
            PendingOpen::Path(path) => self.open_path_now(&path),
            PendingOpen::Close => self.reset_now(),
            PendingOpen::CreateWorkspace => self.submit_new_workspace(),
            PendingOpen::StartIn(dir) => self.start_workspace_now(dir),
            PendingOpen::Reload => self.reload(),
            PendingOpen::Startup => self.startup(),
        }
    }

    pub fn load_file(&mut self, path: String) {
        if self.blocked_by_edits(PendingOpen::Path(path.clone())) {
            return;
        }
        self.load_file_now(path);
    }

    fn load_file_now(&mut self, path: String) {
        self.change_and_reload(true, State::opened_another, move |backend| async move {
            backend.load_simple_file(path).await
        });
    }

    pub fn load_workspace(&mut self, dir: String) {
        if self.blocked_by_edits(PendingOpen::Path(dir.clone())) {
            return;
        }
        self.open_folder_now(dir);
    }

    fn open_path_now(&mut self, path: &str) {
        if Path::new(path).is_dir() {
            self.open_folder_now(path.to_string());
        } else {
            self.load_file_now(path.to_string());
        }
    }

    fn open_folder_now(&mut self, dir: String) {
        self.loading = true;
        self.error = None;
        self.run(
            move |backend| async move {
                match backend.open_folder(dir).await? {
                    OpenedFolder::NeedsStart { dir, empty } => Ok(Err((dir, empty))),
                    OpenedFolder::Loaded => {
                        let open = backend.open_document().await;
                        let snapshot = if open.parse_error.is_none() {
                            backend.record_today_or_warn().await;
                            Some(backend.snapshot().await)
                        } else {
                            None
                        };
                        Ok(Ok((open, snapshot, backend.config().await)))
                    }
                }
            },
            |state, result| {
                state.loading = false;
                match result {
                    Ok(Ok((open, snapshot, config))) => {
                        state.folder_prompt = None;
                        state.open = open;
                        state.config = Some(config);
                        if let Some(snapshot) = snapshot {
                            state.apply_snapshot(snapshot);
                        }
                        state.opened_another();
                    }
                    Ok(Err((dir, empty))) => {
                        state.folder_prompt = Some(FolderPrompt {
                            dir,
                            empty,
                            error: None,
                        });
                        state.error = None;
                        state.palette = None;
                        state.form = None;
                        state.new_workspace = None;
                    }
                    Err(error) => state.error = Some(error),
                }
            },
        );
    }

    /// Start a workspace in a folder that was opened and wasn't one, then open it.
    pub fn start_prompted_workspace(&mut self) {
        let Some(dir) = self.folder_prompt.as_ref().map(|prompt| prompt.dir.clone()) else {
            return;
        };
        if self.blocked_by_edits(PendingOpen::StartIn(dir.clone())) {
            return;
        }
        self.start_workspace_now(dir);
    }

    fn start_workspace_now(&mut self, dir: String) {
        self.loading = true;
        self.error = None;
        if let Some(prompt) = &mut self.folder_prompt {
            prompt.error = None;
        }
        self.run(
            move |backend| async move {
                backend::start_workspace(Path::new(&dir))?;
                backend.load_workspace(dir).await?;
                backend.record_today_or_warn().await;
                let open = backend.open_document().await;
                let snapshot = if open.parse_error.is_none() {
                    Some(backend.snapshot().await)
                } else {
                    None
                };
                Ok((open, snapshot, backend.config().await))
            },
            |state, result| {
                state.loading = false;
                match result {
                    Ok((open, snapshot, config)) => {
                        state.folder_prompt = None;
                        state.open = open;
                        state.config = Some(config);
                        if let Some(snapshot) = snapshot {
                            state.apply_snapshot(snapshot);
                        }
                        state.page = Page::Overview;
                        state.opened_another();
                        state.show_toast("Workspace created");
                    }
                    Err(error) => {
                        if let Some(prompt) = &mut state.folder_prompt {
                            prompt.error = Some(error);
                        } else {
                            state.error = Some(error);
                        }
                    }
                }
            },
        );
    }

    /// A different portfolio is open now, so nothing fetched for the last one still applies.
    fn opened_another(&mut self) {
        self.dirty = false;
        self.form = None;
        self.folder_prompt = None;
        self.selected = None;
        self.inspector.set_open(false);
        self.search.clear();
        self.history = None;
        self.review = Loadable::default();
        self.simulation = Loadable::default();
        self.briefing = Loadable::default();
        self.validation = Loadable::default();
        self.doctor = Loadable::default();
        self.policy.policy = None;
        self.policy.editor.clear();
        self.policy.message = None;
        self.policy.error = None;
        self.documents.generated = None;
        self.documents.message = None;
        self.documents.error = None;
        if self.page.needs_workspace() && !self.is_workspace() {
            self.page = Page::Overview;
        }
        self.load_page();
    }

    pub fn open_dropped(&mut self, path: &Path) {
        let string = path.to_string_lossy().into_owned();
        if self.blocked_by_edits(PendingOpen::Path(string.clone())) {
            return;
        }
        self.open_path_now(&string);
    }

    pub fn ask_for_file(&mut self, tx: &mut Tx) {
        if self.blocked_by_edits(PendingOpen::File) {
            return;
        }
        self.show_file_dialog(tx);
    }

    fn show_file_dialog(&mut self, tx: &mut Tx) {
        let directory = self.open.file_path.as_deref().and_then(|file| {
            let parent = Path::new(file).parent()?;
            parent.is_dir().then(|| parent.to_path_buf())
        });
        let options = FileDialogOptions {
            title: "Open Portfolio".into(),
            directory,
            ..Default::default()
        }
        .filter("Portfolio", &["json", "gpg"]);
        if self.platform_picker {
            self.ask_platform(PickKind::File, options.title, |state, path| {
                state.load_file(path)
            });
            return;
        }
        tx.open_file(options, |state: &mut State, result, _| match result {
            Ok(Some(path)) => state.load_file(path.to_string_lossy().into_owned()),
            Ok(None) => {}
            Err(error) => state.error = Some(error.to_string()),
        });
    }

    pub fn ask_for_workspace(&mut self, tx: &mut Tx) {
        if self.blocked_by_edits(PendingOpen::Workspace) {
            return;
        }
        self.show_workspace_dialog(tx);
    }

    fn show_workspace_dialog(&mut self, tx: &mut Tx) {
        let open = self
            .folder_prompt
            .as_ref()
            .map(|prompt| PathBuf::from(&prompt.dir))
            .or_else(|| self.open.workspace_dir.clone().map(PathBuf::from));
        self.ask_for_folder(
            tx,
            "Open Workspace",
            backend::folder_dialog_start(open.as_deref()),
            |state, dir| state.load_workspace(dir),
        );
    }

    /// The file a new workspace can take its positions from: one open on its own, with something
    /// in it, and not encrypted, because a copy would be plaintext.
    pub fn positions_to_keep(&self) -> Option<&str> {
        let file = self.open.file_path.as_deref()?;
        (!self.is_workspace()
            && self.has_portfolio()
            && !self.positions.is_empty()
            && !file.ends_with(".gpg"))
        .then_some(file)
    }

    pub fn open_new_workspace(&mut self) {
        self.palette = None;
        let mut name = TextEdit::new("");
        name.focused = true;
        self.new_workspace = Some(NewWorkspaceForm {
            name,
            location: TextEdit::new(backend::default_workspace_location()),
            keep_positions: true,
            problem: None,
            busy: false,
        });
    }

    pub fn choose_workspace_location(&mut self, tx: &mut Tx) {
        self.ask_for_folder(tx, "Choose a Location", None, |state, dir| {
            if let Some(form) = &mut state.new_workspace {
                form.location.replace_text(dir);
                form.problem = None;
            }
        });
    }

    /// Create the workspace the form describes and open it. Problems stay in the form.
    pub fn submit_new_workspace(&mut self) {
        let Some(form) = &self.new_workspace else {
            return;
        };
        if form.busy {
            return;
        }
        if self.blocked_by_edits(PendingOpen::CreateWorkspace) {
            return;
        }
        let keep = self.positions_to_keep().map(str::to_string);
        let Some(form) = &mut self.new_workspace else {
            return;
        };
        let name = form.name.text.clone();
        let location = form.location.text.clone();
        let keep = keep.filter(|_| form.keep_positions);
        form.problem = None;
        form.busy = true;
        self.loading = true;
        self.error = None;
        self.run(
            move |backend| async move {
                let dir = match backend::create_workspace(&name, &location, keep.as_deref()) {
                    Ok(dir) => dir,
                    Err(problem) => return Ok(Err(problem)),
                };
                if let Err(message) = backend.load_workspace(dir).await {
                    return Ok(Err(WorkspaceProblem {
                        field: WorkspaceField::Location,
                        message,
                    }));
                }
                backend.record_today_or_warn().await;
                let snapshot = backend.snapshot().await;
                Ok(Ok((
                    backend.open_document().await,
                    snapshot,
                    backend.config().await,
                )))
            },
            |state, result| {
                state.loading = false;
                match result {
                    Ok(Ok((open, snapshot, config))) => {
                        state.new_workspace = None;
                        state.open = open;
                        state.config = Some(config);
                        state.apply_snapshot(snapshot);
                        state.page = Page::Overview;
                        state.opened_another();
                        state.show_toast("Workspace created");
                    }
                    Ok(Err(problem)) => {
                        if let Some(form) = &mut state.new_workspace {
                            form.problem = Some(problem);
                            form.busy = false;
                        }
                    }
                    Err(error) => {
                        state.new_workspace = None;
                        state.error = Some(error);
                    }
                }
            },
        );
    }

    fn ask_for_folder(
        &mut self,
        tx: &mut Tx,
        title: &str,
        directory: Option<PathBuf>,
        then: impl FnOnce(&mut State, String) + 'static,
    ) {
        if self.platform_picker {
            self.ask_platform(PickKind::Folder, title.to_string(), then);
            return;
        }
        let options = FileDialogOptions {
            title: title.into(),
            directory,
            ..Default::default()
        };
        tx.open_folder(options, move |state: &mut State, result, _| match result {
            Ok(Some(path)) => then(state, path.to_string_lossy().into_owned()),
            Ok(None) => {}
            Err(error) => state.error = Some(error.to_string()),
        });
    }

    /// Ask the platform's picker, which runs `then` with what was picked. One question at a time,
    /// as with Rune's dialogs: a second while one is open is dropped.
    fn ask_platform(
        &mut self,
        kind: PickKind,
        title: String,
        then: impl FnOnce(&mut State, String) + 'static,
    ) {
        if self.pick.is_some() {
            return;
        }
        self.palette = None;
        self.pick = Some(Pick {
            kind,
            title,
            asked: false,
            then: Box::new(then),
        });
    }

    /// The question for the platform's picker to show, once: nothing while it shows.
    pub fn take_pick(&mut self) -> Option<(PickKind, String)> {
        let pick = self.pick.as_mut().filter(|pick| !pick.asked)?;
        pick.asked = true;
        Some((pick.kind, pick.title.clone()))
    }

    /// The platform's picker answered: `path`, or `None` when it was cancelled.
    pub fn picked(&mut self, path: Option<String>) {
        if let (Some(pick), Some(path)) = (self.pick.take(), path) {
            (pick.then)(self, path);
        }
    }

    /// Read the open file again, after fixing it elsewhere.
    pub fn reload(&mut self) {
        if self.blocked_by_edits(PendingOpen::Reload) {
            return;
        }
        self.change_and_reload(
            true,
            |state| state.dirty = false,
            |backend| async move { backend.reload().await },
        );
    }

    /// Forget the remembered portfolio and go back to the welcome screen.
    ///
    /// Unsaved edits ask first. The same question covers every other way of leaving this portfolio.
    pub fn reset(&mut self) {
        if self.open.is_open() && self.blocked_by_edits(PendingOpen::Close) {
            return;
        }
        self.reset_now();
    }

    fn reset_now(&mut self) {
        self.folder_prompt = None;
        self.form = None;
        self.run(
            |backend| async move { backend.clear_remembered_path().await },
            |state, result| match result {
                Ok(()) => {
                    state.open = OpenDocument::default();
                    state.summary = None;
                    state.positions.clear();
                    state.quotes.clear();
                    state.allocation.clear();
                    state.performance = None;
                    state.findings.clear();
                    state.has_policy = false;
                    state.opened_another();
                    state.page = Page::Overview;
                }
                Err(error) => state.error = Some(error),
            },
        );
    }

    // --- Navigating ---

    pub fn navigate(&mut self, page: Page) {
        if page.needs_workspace() && !self.is_workspace() {
            return;
        }
        if page != self.page {
            self.previous_page = self.page;
        }
        self.page = page;
        self.more = false;
        self.chart_hover = None;
        self.chart_point = None;
        self.measure.clear();
        self.donut_hover = None;
        self.palette = None;
        if page != Page::Holdings {
            self.inspector.set_open(false);
        } else if self.selected.is_some() {
            self.inspector.set_open(true);
        }
        self.load_page();
    }

    /// Fetch what the current page shows.
    pub fn load_page(&mut self) {
        match self.page {
            Page::Overview => {
                self.load_history();
                // The agenda reads the same review as the Review page, so they never disagree.
                self.load_review();
            }
            Page::Holdings | Page::Performance => self.load_history(),
            Page::Allocation | Page::Review => self.load_review(),
            Page::Rebalance => {
                self.load_simulation();
                // Targets and tolerance bands live on the review, not the simulation.
                self.load_review();
            }
            Page::Briefing => self.load_briefing(),
            Page::Health => self.load_health(),
            Page::Policy => self.load_policy_page(),
            Page::Settings => self.load_settings(),
            Page::Documents => {}
        }
    }

    fn load_history(&mut self) {
        // Holdings sparklines use the same month Overview charts, so the two pages agree.
        let period = if self.page == Page::Performance {
            self.period
        } else {
            Period::Month
        };
        self.history_request += 1;
        let request = self.history_request;
        self.run(
            move |backend| async move { backend.history(period).await },
            move |state, result| state.finish_history(request, result),
        );
    }

    fn finish_history(&mut self, request: u64, result: BackendResult<History>) {
        if self.history_request != request {
            return;
        }
        match result {
            Ok(history) => {
                self.history = Some(history);
                self.history_error = None;
                self.restart_draw = true;
            }
            Err(error) => self.history_error = Some(error),
        }
    }

    pub fn set_period(&mut self, period: Period) {
        self.period = period;
        self.chart_hover = None;
        self.measure.clear();
        self.load_history();
    }

    pub fn load_review(&mut self) {
        if !self.has_policy {
            self.review = Loadable::default();
            return;
        }
        self.review.loading = true;
        self.restart_draw = true;
        self.run(
            |backend| async move { backend.review().await },
            |state, result| state.review.finish(result),
        );
    }

    /// Prices again, and the bar goes back to the portfolio as it is.
    pub fn refresh_rebalance(&mut self) {
        self.rebalance.origin = RebalanceOrigin::Today;
        self.rebalance.amount.replace_text("");
        self.rebalance.amount_error = None;
        self.load_simulation();
        self.load_review();
    }

    pub fn load_simulation(&mut self) {
        if !self.has_policy {
            self.simulation = Loadable::default();
            return;
        }
        self.simulation.loading = true;
        self.run(
            |backend| async move { backend.simulation().await },
            |state, result| state.simulation.finish(result),
        );
    }

    pub fn load_briefing(&mut self) {
        self.briefing.loading = true;
        self.run(
            |backend| async move { backend.briefing().await },
            |state, result| state.briefing.finish(result),
        );
    }

    pub fn load_health(&mut self) {
        self.validation.loading = true;
        self.run(
            |backend| async move { backend.validate().await },
            |state, result| state.validation.finish(result),
        );
        if let Some(dir) = self.open.workspace_dir.clone() {
            self.doctor.loading = true;
            self.run(
                move |backend| async move { backend.doctor(dir).await },
                |state, result| state.doctor.finish(result),
            );
        }
    }

    // --- Since the last look ---

    /// Note this look at a newly opened portfolio, then measure what changed since the last one.
    /// After that, every snapshot measures again, because the prices just moved.
    fn load_since(&mut self) {
        let key = self
            .open
            .workspace_dir
            .clone()
            .or_else(|| self.open.file_path.clone());
        // An empty portfolio isn't a look at anything yet.
        if key.is_none() || self.positions.is_empty() {
            return;
        }
        if key != self.seen_for {
            self.seen_for = key;
            self.since = None;
            self.change = None;
            self.run(
                |backend| async move {
                    let previous = backend.visit().await?;
                    let since = Seen::since(previous, Local::now().date_naive());
                    let change = match since {
                        Since::Day(day) => backend.change_since(Some(day)).await?,
                        Since::First => backend.change_since(None).await?,
                        Since::EarlierToday => None,
                    };
                    Ok((since, change))
                },
                |state, result| {
                    if let Ok((since, change)) = result {
                        state.since = Some(since);
                        state.change = change;
                    }
                },
            );
            return;
        }
        let from = match self.since {
            Some(Since::Day(day)) => Some(day),
            Some(Since::First) => None,
            _ => return,
        };
        self.run(
            move |backend| async move { backend.change_since(from).await },
            |state, result| {
                if let Ok(change) = result {
                    state.change = change;
                }
            },
        );
    }

    /// What changed since the owner last looked, in words, or `None` with nothing worth saying.
    pub fn statement(&self) -> Option<Statement> {
        let summary = self.summary.as_ref()?;
        let currency = self.currency();
        let encrypted = self
            .open
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with(".gpg"));
        statement::compose(&Facts {
            today: Local::now().date_naive(),
            since: self.since?,
            currency: &currency,
            hidden: self.privacy,
            complete: summary.complete,
            encrypted,
            change: self.change.clone(),
            today_move: Some(statement::today_move(
                summary.day_change,
                summary.day_change_percent,
                &self.positions,
            )),
        })
    }

    // --- Portfolio ---

    pub fn refresh_prices(&mut self) {
        if self.loading || !self.has_portfolio() {
            return;
        }
        // History follows the file on disk, so unsaved edits don't record.
        let record = !self.dirty;
        let before: Vec<(usize, f64)> = self
            .positions
            .iter()
            .map(|position| (position.id, position.price))
            .collect();
        self.change_and_reload(
            record,
            move |state| {
                state.price_moves = price_moves(&before, &state.positions);
                state.price_updates += 1;
                state.show_toast("Prices updated");
            },
            |backend| async move {
                backend.refresh_prices().await;
                Ok(())
            },
        );
    }

    /// On coming back to the app, catch up if the prices are old.
    pub fn refresh_if_stale(&mut self) {
        if self
            .refreshed
            .is_some_and(|refreshed| refreshed.elapsed() >= STALE_AFTER)
        {
            self.refresh_prices();
        }
    }

    pub fn save(&mut self) {
        if !self.has_portfolio() {
            return;
        }
        self.change_and_reload(
            true,
            |state| {
                state.dirty = false;
                state.show_toast("Saved");
            },
            |backend| async move { backend.save_portfolio().await },
        );
    }

    pub fn open_form(&mut self, existing: Option<usize>) {
        if !self.has_portfolio() {
            return;
        }
        let position = existing.and_then(|id| self.positions.iter().find(|p| p.id == id));
        self.form = Some(PositionForm::new(position));
    }

    pub fn submit_form(&mut self) {
        if !self.has_portfolio() {
            self.form = None;
            return;
        }
        let Some(form) = &mut self.form else {
            return;
        };
        let (amount, quantity, price) = match form.numbers() {
            Ok(numbers) => numbers,
            Err(error) => {
                form.error = Some(error);
                return;
            }
        };
        let name = text_or_none(&form.name.text);
        let ticker = text_or_none(&form.ticker.text);
        let asset_class = text_or_none(&form.asset_class.text);
        let amount_from_purchases = form.amount_from_purchases();
        match form.existing {
            Some(id) => {
                let request = UpdatePositionRequest {
                    name,
                    ticker,
                    asset_class,
                    amount: (!amount_from_purchases).then_some(amount),
                    purchases: None,
                };
                self.form = None;
                let current = self.positions.iter().find(|p| p.id == id);
                if current.is_some_and(|position| !changes(&request, position)) {
                    return;
                }
                self.change_and_reload(
                    false,
                    |state| state.dirty = true,
                    move |backend| async move { backend.update_position(id, request).await },
                );
            }
            None => {
                let purchases = quantity
                    .filter(|quantity| *quantity > 0.0)
                    .map(|quantity| CreatePurchaseRequest {
                        date: None,
                        quantity,
                        price,
                        fees: None,
                    })
                    .into_iter()
                    .collect();
                let request = CreatePositionRequest {
                    name,
                    ticker,
                    asset_class: asset_class.unwrap_or_else(|| "Stock".to_string()),
                    amount,
                    purchases,
                };
                self.form = None;
                self.change_and_reload(
                    false,
                    |state| state.dirty = true,
                    move |backend| async move { backend.add_position(request).await },
                );
            }
        }
    }

    pub fn delete_position(&mut self, id: usize) {
        self.selected = None;
        self.inspector.set_open(false);
        self.change_and_reload(
            false,
            |state| state.dirty = true,
            move |backend| async move { backend.delete_position(id).await },
        );
    }

    pub fn select_position(&mut self, id: usize) {
        if self.selected == Some(id) {
            self.selected = None;
            self.inspector.set_open(false);
        } else {
            self.selected = Some(id);
            self.inspector.set_open(true);
        }
    }

    pub fn reveal_position(&mut self, id: usize) {
        self.selected = Some(id);
        self.navigate(Page::Holdings);
        self.inspector.set_open(true);
    }

    pub fn set_sort(&mut self, sort: SortBy) {
        if self.sort == sort {
            self.ascending = !self.ascending;
        } else {
            self.sort = sort;
            // Text reads A to Z; numbers read biggest first.
            self.ascending = matches!(sort, SortBy::Name | SortBy::Class);
        }
    }

    // --- Quitting ---

    /// Quit, first asking about changes that aren't on disk.
    pub fn quit_or_ask(&mut self, tx: &mut Tx) {
        if self.update_is_installing() {
            self.show_toast("An update is installing. Quit when it has finished.");
        } else if self.dirty {
            self.confirm = Some(Confirm::Quit);
        } else {
            tx.quit();
        }
    }

    /// The window is closing. It can't be stopped, so with unsaved changes it comes back to ask.
    pub fn on_window_close(&mut self, tx: &mut Tx) {
        if self.close_quits() {
            tx.quit();
        }
    }

    /// True when closing the window should quit. Otherwise it has reopened, asking what to do.
    fn close_quits(&mut self) -> bool {
        if self.update_is_installing() {
            self.show_toast("An update is installing. Quit when it has finished.");
        } else if self.dirty {
            self.confirm = Some(Confirm::Quit);
        } else {
            return true;
        }
        if let Some(reopen) = &self.reopen {
            reopen();
        }
        false
    }

    fn update_is_installing(&self) -> bool {
        self.updates.phase == UpdatePhase::Installing
    }

    pub fn save_and_quit(&mut self) {
        self.confirm = None;
        self.loading = true;
        self.run(
            |backend| async move { backend.save_portfolio().await },
            |state, result| {
                state.loading = false;
                match result {
                    Ok(()) => {
                        state.dirty = false;
                        state.quit_requested = true;
                    }
                    // Stay open: quitting now would lose what they asked to keep.
                    Err(error) => state.error = Some(error),
                }
            },
        );
    }

    // --- Commands ---

    pub fn run_command(&mut self, command: Command, tx: &mut Tx) {
        self.palette = None;
        match command {
            Command::Go(page) => self.navigate(page),
            Command::Reveal(id) => self.reveal_position(id),
            Command::Refresh => self.refresh_prices(),
            Command::Save => self.save(),
            Command::OpenFile => self.ask_for_file(tx),
            Command::OpenWorkspace => self.ask_for_workspace(tx),
            Command::NewWorkspace => self.open_new_workspace(),
            Command::Guide => self.toggle_guide(),
            Command::Privacy => self.privacy = !self.privacy,
            Command::Theme => self.toggle_theme(),
            Command::Sidebar => self.sidebar.toggle(),
            Command::AddPosition => {
                if !self.has_portfolio() {
                    return;
                }
                self.navigate(Page::Holdings);
                self.open_form(None);
            }
            Command::CopyBriefing => self.copy_briefing(tx),
            Command::CheckForUpdates => self.check_for_updates(false),
            Command::Close => self.reset(),
        }
    }

    pub fn toggle_guide(&mut self) {
        self.palette = None;
        self.guide = !self.guide;
    }

    pub fn open_palette(&mut self) {
        let mut query = TextEdit::new("");
        query.focused = true;
        self.palette = Some(Palette { query, selected: 0 });
    }

    pub fn copy(&mut self, text: String, tx: &mut Tx) {
        tx.copy_rich(ClipboardContent::new(text));
        self.show_toast("Copied");
    }

    fn copy_briefing(&mut self, tx: &mut Tx) {
        if let Some(briefing) = &self.briefing.data {
            let markdown = briefing.markdown.clone();
            self.copy(markdown, tx);
            return;
        }
        // Not loaded yet: open the page, which has the button once it is.
        self.navigate(Page::Briefing);
    }

    // --- Updates ---

    /// Ask GitHub for a newer version. A `quiet` check is the one at startup: nobody asked for it,
    /// so it says nothing unless there is something to say, and failing is not news.
    pub fn check_for_updates(&mut self, quiet: bool) {
        if !self.updates.updater.is_enabled() {
            if !quiet {
                self.show_toast("Updates are turned off in this build");
            }
            return;
        }
        if self.updates.checking {
            // Asking during the startup check gets that check's answer.
            self.updates.check_is_quiet &= quiet;
            return;
        }
        self.updates.checking = true;
        self.updates.check_is_quiet = quiet;
        let updater = self.updates.updater.clone();
        self.run(
            move |_| updater.check(),
            |state, result| state.finish_update_check(result),
        );
    }

    fn finish_update_check(&mut self, result: BackendResult<Option<Release>>) {
        let updates = &mut self.updates;
        updates.checking = false;
        let quiet = updates.check_is_quiet;
        match result {
            Ok(Some(release)) => {
                // What's installed or installing stays the release in the dialog.
                if matches!(updates.phase, UpdatePhase::Idle | UpdatePhase::Failed(_)) {
                    updates.available = Some(release);
                    updates.phase = UpdatePhase::Idle;
                }
                if !quiet {
                    updates.dismissed = false;
                    updates.dialog = true;
                }
            }
            Ok(None) => {
                // A release that was offered and has since been pulled is no longer on offer.
                if matches!(updates.phase, UpdatePhase::Idle | UpdatePhase::Failed(_)) {
                    updates.available = None;
                    updates.dialog = false;
                    updates.phase = UpdatePhase::Idle;
                }
                if !quiet {
                    let current = updates.updater.current().clone();
                    self.show_toast(format!("You're up to date ({current})"));
                }
            }
            Err(_) if !quiet => {
                self.show_toast("Couldn't check for updates. Try again later.");
            }
            Err(_) => {}
        }
    }

    pub fn open_update_dialog(&mut self) {
        self.updates.dialog = true;
    }

    /// Close the dialog, unless something is happening in it. True if it was open.
    pub fn close_update_dialog(&mut self) -> bool {
        if !self.updates.dialog {
            return false;
        }
        if self.updates.phase != UpdatePhase::Installing {
            self.updates.dialog = false;
        }
        true
    }

    /// Don't mention it again until the next launch. An on-demand check can still find it.
    pub fn dismiss_update(&mut self) {
        if self.updates.phase != UpdatePhase::Installing {
            self.updates.dialog = false;
            self.updates.dismissed = true;
        }
    }

    /// The update button itself. Nothing else calls this.
    pub fn install_update(&mut self) {
        if matches!(
            self.updates.phase,
            UpdatePhase::Installing | UpdatePhase::Installed | UpdatePhase::Saved(_)
        ) {
            return;
        }
        let Some(release) = self.updates.available.clone() else {
            return;
        };
        self.updates.phase = UpdatePhase::Installing;
        self.updates.restart_error = None;
        let updater = self.updates.updater.clone();
        self.run(
            move |_| updater.install(release),
            |state, result| {
                state.updates.phase = match result {
                    Ok(Installed::Replaced) => UpdatePhase::Installed,
                    Ok(Installed::Saved(path)) => UpdatePhase::Saved(path),
                    Err(error) => UpdatePhase::Failed(format!("The update failed: {error}")),
                };
            },
        );
    }

    /// Start the new version and quit this one, saving first. A failed save keeps this one open,
    /// because quitting would lose what's unsaved.
    pub fn restart_after_update(&mut self) {
        // `loading`: the save this started is still running, and a second one would restart twice.
        if self.updates.phase != UpdatePhase::Installed || self.loading {
            return;
        }
        self.updates.restart_error = None;
        if !self.dirty {
            self.relaunch();
            return;
        }
        self.loading = true;
        self.run(
            |backend| async move { backend.save_portfolio().await },
            |state, result| {
                state.loading = false;
                match result {
                    Ok(()) => {
                        state.dirty = false;
                        state.relaunch();
                    }
                    Err(error) => {
                        state.updates.restart_error =
                            Some(format!("Couldn't save your changes: {error}"));
                    }
                }
            },
        );
    }

    fn relaunch(&mut self) {
        match self.updates.updater.relaunch() {
            Ok(()) => self.quit_requested = true,
            Err(error) => {
                self.updates.restart_error = Some(format!(
                    "The update is installed, but {error}. Quit and open Stahl Capital again."
                ));
            }
        }
    }

    /// The release's page, for reading more or installing by hand.
    pub fn open_release_page(&mut self, tx: &mut Tx) {
        let page = self
            .updates
            .available
            .as_ref()
            .map_or(update::RELEASES_PAGE, |release| release.page.as_str());
        if tx.open_url(page).is_err() {
            self.show_toast("Couldn't open the release page");
        }
    }

    /// A link in the release notes. Only ever a web page, and only when someone clicks it.
    pub fn open_notes_link(&mut self, url: &str, tx: &mut Tx) {
        if !url.starts_with("https://") || tx.open_url(url).is_err() {
            self.show_toast("Couldn't open that link");
        }
    }

    pub fn is_quitting(&self) -> bool {
        self.quit_requested
    }

    // --- Settings ---

    pub fn set_theme(&mut self, mode: ThemeMode) {
        if let Some(config) = &mut self.config {
            config.theme = mode;
        }
        self.run(
            move |backend| async move { backend.set_theme(mode).await },
            |state, result| {
                if let Err(error) = result {
                    state.error = Some(error);
                }
            },
        );
    }

    pub fn toggle_theme(&mut self) {
        self.set_theme(match self.theme_mode() {
            ThemeMode::Dark => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
        });
    }

    fn load_settings(&mut self) {
        self.run(
            |backend| async move { Ok(backend.config().await) },
            |state, result: BackendResult<AppConfigDto>| {
                let Ok(config) = result else {
                    return;
                };
                state.settings.currency.replace_text(&config.currency);
                state
                    .settings
                    .provider_url
                    .replace_text(config.llm_provider_url.as_deref().unwrap_or(""));
                state
                    .settings
                    .model
                    .replace_text(config.llm_model.as_deref().unwrap_or(""));
                state.has_api_key = config.has_llm_api_key;
            },
        );
    }

    pub fn save_settings(&mut self) {
        let form = &mut self.settings;
        let currency = form.currency.text.trim().to_uppercase();
        if currency.is_empty() {
            form.error = Some("Currency can't be empty.".to_string());
            form.message = None;
            return;
        }
        let provider_url = text_or_none(&form.provider_url.text);
        let api_key = text_or_none(&form.api_key.text);
        let model = text_or_none(&form.model.text);
        self.change_and_reload(
            false,
            |state| {
                state.settings.api_key.clear();
                state.settings.error = None;
                state.show_toast("Settings saved");
            },
            move |backend| async move {
                backend.set_currency(currency).await?;
                backend.set_llm_config(provider_url, api_key, model).await
            },
        );
    }

    pub fn export_skill(&mut self, tx: &mut Tx) {
        self.ask_for_folder(tx, "Export Agent Skill", None, |state, dir| {
            match state.backend.export_skill(&dir) {
                Ok(()) => state.show_toast("Skill exported"),
                Err(error) => state.settings.error = Some(error),
            }
        });
    }

    // --- Policy ---

    fn show_policy(&mut self, policy: Policy) {
        match Backend::policy_to_toml(&policy) {
            Ok(toml) => self.policy.editor.replace_text(&toml),
            Err(error) => self.policy.error = Some(error),
        }
        self.policy.policy = Some(policy);
    }

    fn load_policy_page(&mut self) {
        self.run(
            |backend| async move { Ok(backend.policy().await) },
            |state, result: BackendResult<Option<Policy>>| {
                if let Ok(Some(policy)) = result {
                    state.show_policy(policy);
                }
            },
        );
    }

    pub fn use_strategy(&mut self, strategy: &str) {
        match Backend::policy_from_strategy(strategy) {
            Ok(policy) => {
                self.show_policy(policy);
                self.policy.message =
                    Some("Template loaded. Press Save to Workspace to keep it.".to_string());
                self.policy.error = None;
            }
            Err(error) => self.policy.error = Some(error),
        }
    }

    pub fn reload_policy(&mut self) {
        self.policy.loading = true;
        self.policy.message = None;
        self.policy.error = None;
        self.run(
            |backend| async move { backend.load_policy().await },
            |state, result| {
                state.policy.loading = false;
                match result {
                    Ok(policy) => {
                        state.has_policy = true;
                        state.show_policy(policy);
                        state.policy.message = Some("Policy loaded".to_string());
                    }
                    Err(error) => state.policy.error = Some(error),
                }
            },
        );
    }

    fn finish_policy(&mut self, result: BackendResult<Policy>, success: &str) {
        self.policy.loading = false;
        match result {
            Ok(policy) => {
                self.has_policy = true;
                self.policy.policy = Some(policy);
                self.policy.message = Some(success.to_string());
            }
            Err(error) => self.policy.error = Some(error),
        }
    }

    pub fn validate_policy(&mut self) {
        let toml = self.policy.editor.text.clone();
        self.policy.loading = true;
        self.policy.message = None;
        self.policy.error = None;
        self.run(
            |backend| async move { backend.set_policy_from_toml(&toml).await },
            |state, result| state.finish_policy(result, "Policy is valid"),
        );
    }

    pub fn save_policy(&mut self) {
        let toml = self.policy.editor.text.clone();
        self.policy.loading = true;
        self.policy.message = None;
        self.policy.error = None;
        self.run(
            |backend| async move { backend.save_policy(toml).await },
            |state, result| state.finish_policy(result, "Policy saved"),
        );
    }

    // --- Documents ---

    fn finish_document(&mut self, result: BackendResult<GeneratedDocument>) {
        self.documents.loading = false;
        match result {
            Ok(document) => {
                self.documents.message = Some(if document.dry_run {
                    "Preview only. Nothing was written.".to_string()
                } else {
                    format!("Saved to {}", document.file_path)
                });
                self.documents.error = None;
                self.documents.generated = Some(document);
            }
            Err(error) => {
                self.documents.message = None;
                self.documents.error = Some(error);
            }
        }
    }

    pub fn generate_report(&mut self, dry_run: bool) {
        let Some(dir) = self.open.workspace_dir.clone() else {
            return;
        };
        self.documents.loading = true;
        self.run(
            move |backend| async move { backend.generate_report(dir, dry_run).await },
            |state, result| state.finish_document(result),
        );
    }

    pub fn draft_decision(&mut self, dry_run: bool) {
        let Some(dir) = self.open.workspace_dir.clone() else {
            return;
        };
        let title = text_or_none(&self.documents.title.text);
        self.documents.loading = true;
        self.run(
            move |backend| async move { backend.draft_decision(title, dir, dry_run).await },
            |state, result| state.finish_document(result),
        );
    }
}

/// What opens on launch: a path on the command line, unless it's a Rune tooling flag's value.
pub fn initial_file(args: &[String]) -> (Option<String>, Vec<String>) {
    match args.first() {
        Some(first) if !first.starts_with('-') => (Some(first.clone()), args[1..].to_vec()),
        _ => (None, args.to_vec()),
    }
}

/// The file name in a path, from either path style.
pub fn file_name(path: &str) -> &str {
    path.trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(path)
}

/// Which positions a refresh moved, and by what fraction of their old price. A position that
/// wasn't there before, or had no price, didn't move: nothing is compared with nothing.
pub fn price_moves(before: &[(usize, f64)], after: &[PositionDto]) -> Vec<(usize, f64)> {
    after
        .iter()
        .filter_map(|position| {
            let (_, old) = before.iter().find(|(id, _)| *id == position.id)?;
            let change = (position.price - old) / old;
            (old.abs() > f64::EPSILON && change.is_finite() && change.abs() > 1e-9)
                .then_some((position.id, change))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_refresh_moves_only_the_prices_that_changed() {
        let position = |id: usize, price: f64| {
            serde_json::from_value::<PositionDto>(serde_json::json!({
                "id": id, "assetClass": "Stocks", "amount": 1.0, "price": price, "value": price,
                "purchases": []
            }))
            .unwrap()
        };
        let before = [(0, 100.0), (1, 50.0), (2, 0.0)];
        let after = [
            position(0, 101.0),
            position(1, 50.0),
            position(2, 3.0),
            position(3, 9.0),
        ];
        let moves = price_moves(&before, &after);
        assert_eq!(
            moves.len(),
            1,
            "unchanged, unpriced and new positions didn't move"
        );
        assert_eq!(moves[0].0, 0);
        assert!((moves[0].1 - 0.01).abs() < 1e-12);
    }

    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use tokio::runtime::Runtime;

    use super::*;
    use crate::demo;

    fn test_state(dir: &Path, runtime: &Runtime) -> State {
        State::new(demo::backend(dir, None, runtime.handle().clone()), None)
    }

    #[test]
    fn startup_asks_before_replacing_unsaved_edits() {
        let runtime = Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut state = test_state(dir.path(), &runtime);
        state.dirty = true;
        state.startup();
        assert_eq!(state.confirm, Some(Confirm::Unsaved(PendingOpen::Startup)));
        state.settle();
        assert!(state.starting, "launch waits for the answer");

        state.confirm = None;
        state.dirty = false;
        state.startup();
        state.settle();
        assert!(!state.starting);
    }

    #[test]
    fn add_position_is_refused_with_nothing_open() {
        let runtime = Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut state = test_state(dir.path(), &runtime);
        state.open_form(None);
        assert!(state.form.is_none());
        state.submit_form();
        assert!(!state.dirty);
    }

    #[test]
    fn closing_quits_unless_there_are_unsaved_changes_to_ask_about() {
        let runtime = Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut state = test_state(dir.path(), &runtime);
        let reopened = Arc::new(AtomicBool::new(false));
        let flag = reopened.clone();
        state.reopen = Some(Box::new(move || flag.store(true, Ordering::SeqCst)));

        assert!(state.close_quits(), "nothing to lose");
        assert!(!reopened.load(Ordering::SeqCst));
        assert_eq!(state.confirm, None);

        state.dirty = true;
        assert!(!state.close_quits(), "unsaved changes keep the app alive");
        assert!(reopened.load(Ordering::SeqCst), "the window comes back");
        assert_eq!(state.confirm, Some(Confirm::Quit));
    }

    #[test]
    fn a_late_history_answer_cannot_replace_a_newer_request() {
        let runtime = Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut state = test_state(dir.path(), &runtime);
        state.history_request = 2;

        state.finish_history(1, Err("older".to_string()));
        assert_eq!(state.history_error, None);
        state.finish_history(2, Err("newest".to_string()));
        assert_eq!(state.history_error.as_deref(), Some("newest"));
    }

    #[test]
    fn numbers_are_validated_not_coerced() {
        assert_eq!(parse_number(" 1.5 "), Ok(Some(1.5)));
        assert_eq!(parse_number(""), Ok(None));
        assert_eq!(parse_number("   "), Ok(None));
        for text in [
            "NaN",
            "inf",
            "abc",
            "-",
            ".",
            ",",
            "1e3",
            "\u{20ac}150",
            "150 \u{20ac}",
            "$150",
            "1.5x",
            "\u{661}\u{662}",
        ] {
            assert_eq!(parse_number(text), Err(BadNumber::NotANumber), "{text:?}");
        }
    }

    #[test]
    fn numbers_read_the_one_way_they_can() {
        for (text, wanted) in [
            ("10", 10.0),
            ("-3", -3.0),
            ("+4", 4.0),
            ("010", 10.0),
            // A lone mark that can't be separating thousands.
            ("1,5", 1.5),
            ("150,00", 150.0),
            ("1.5", 1.5),
            ("0.125", 0.125),
            ("0,125", 0.125),
            ("00.125", 0.125),
            (".125", 0.125),
            (",5", 0.5),
            ("5.", 5.0),
            ("1,0000", 1.0),
            ("1234.567", 1234.567),
            ("1234,567", 1234.567),
            ("-1,5", -1.5),
            // Two marks: the last is the decimal.
            ("1,000.50", 1000.5),
            ("1.000,50", 1000.5),
            ("1,000.500", 1000.5),
            ("1 000,50", 1000.5),
            ("1'000.50", 1000.5),
            ("1,234,567.89", 1234567.89),
            ("1.234.567,89", 1234567.89),
            // A repeated or a space-like mark: thousands.
            ("1,000,000", 1_000_000.0),
            ("1.000.000", 1_000_000.0),
            ("01.000.000", 1_000_000.0),
            ("0100.000.000", 100_000_000.0),
            ("1 000", 1000.0),
            ("1\u{a0}000", 1000.0),
            ("1\u{202f}000", 1000.0),
            ("1'000", 1000.0),
        ] {
            assert_eq!(parse_number(text), Ok(Some(wanted)), "{text:?}");
        }
    }

    #[test]
    fn a_number_that_could_be_off_by_a_thousand_is_refused() {
        for (text, thousands, decimal) in [
            ("1.000", "1000", "1.000"),
            ("1,000", "1000", "1.000"),
            ("12.345", "12345", "12.345"),
            ("123,456", "123456", "123.456"),
            ("-1.000", "-1000", "-1.000"),
            // What typing after the form's default 0 produces.
            ("01.000", "1000", "1.000"),
            ("0100,000", "100000", "100.000"),
        ] {
            let ambiguous = BadNumber::Ambiguous {
                thousands: thousands.to_string(),
                decimal: decimal.to_string(),
            };
            assert_eq!(parse_number(text), Err(ambiguous), "{text:?}");
        }
    }

    #[test]
    fn malformed_grouping_is_refused() {
        for text in [
            "1,00.50",
            "1.2.3",
            "1,000.000,5",
            "1,000,",
            ",000,000",
            "1,0000,000",
            "1000,000,000",
            "1,5 000",
            "1 000'000",
            "1 00",
            " - 5",
        ] {
            assert_eq!(parse_number(text), Err(BadNumber::NotANumber), "{text:?}");
        }
    }

    #[test]
    fn the_form_reads_back_every_number_it_shows() {
        for number in [
            0.0,
            1.0,
            0.45,
            12.345,
            1.5,
            100.0,
            123.456,
            -12.345,
            999.999,
            1000.125,
            30000.0,
            0.000001,
            1e21,
            1234567.891,
        ] {
            let shown = editable_number(number);
            assert_eq!(parse_number(&shown), Ok(Some(number)), "{shown:?}");
        }
        assert_eq!(editable_number(12.345), "12.3450");
        assert_eq!(editable_number(0.125), "0.125");
    }

    #[test]
    fn only_a_leading_path_is_the_initial_file() {
        let args = |list: &[&str]| list.iter().map(ToString::to_string).collect::<Vec<_>>();
        assert_eq!(
            initial_file(&args(&["p.json"])),
            (Some("p.json".to_string()), vec![])
        );
        assert_eq!(
            initial_file(&args(&["--snap", "out.png"])),
            (None, args(&["--snap", "out.png"]))
        );
    }

    #[test]
    fn file_names_come_from_either_path_style() {
        assert_eq!(file_name("/home/me/positions.json"), "positions.json");
        assert_eq!(file_name("/home/me/portfolio/"), "portfolio");
        assert_eq!(file_name("C:\\Users\\me\\positions.json"), "positions.json");
    }

    #[test]
    fn shortcuts_reach_every_page() {
        for page in Page::PORTFOLIO
            .into_iter()
            .chain(Page::PLAN)
            .chain(Page::INSIGHTS)
        {
            assert_eq!(Page::from_shortcut(page.shortcut().unwrap()), Some(page));
        }
    }
}
