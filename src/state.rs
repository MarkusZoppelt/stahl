//! The whole app's state, and everything that changes it.
//!
//! Rune re-runs the views when this changes. `portfolio_rs` runs on Tokio, so a change that
//! needs it goes through [`State::run`]: the work runs off the UI thread, and its result lands
//! here in `poll`.

use std::{
    mem,
    path::Path,
    sync::mpsc::{Receiver, Sender, channel},
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
use rune::{ClipboardContent, FileDialogOptions, TextEdit, Tx, Waker};
use rune_kit::shell::PaneState;

use crate::{
    backend::{Backend, BackendResult, Briefing, History, OpenDocument, Period, Snapshot},
    format,
};

/// How long a chart takes to draw itself in.
pub const DRAW_IN: f64 = 0.9;
/// Refresh prices on returning to the app, if the last refresh is older than this.
const STALE_AFTER: Duration = Duration::from_secs(15 * 60);
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
                existing.map_or_else(|| "Stock".to_string(), |p| p.asset_class.clone()),
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

#[derive(Clone, PartialEq, Debug)]
pub enum Confirm {
    Delete { id: usize, name: String },
    Quit,
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
    Privacy,
    Theme,
    Sidebar,
    AddPosition,
    CopyBriefing,
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

type Job = Box<dyn FnOnce(&mut State) + Send>;

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
    pub allocation: Vec<AllocationItemDto>,
    pub performance: Option<PerformanceDto>,
    pub findings: Vec<Finding>,
    /// connected | partial | offline | fixture
    pub network: String,
    /// Position changes not written to disk yet.
    pub dirty: bool,
    pub loading: bool,
    pub starting: bool,
    pub error: Option<String>,
    pub updated_at: Option<String>,
    /// How many times prices have landed, to mark the moment on screen.
    pub price_updates: u64,
    refreshed: Option<Instant>,

    // The window.
    pub page: Page,
    pub privacy: bool,
    pub sidebar: PaneState,
    pub inspector: PaneState,
    pub drop_hover: bool,
    pub palette: Option<Palette>,
    pub form: Option<PositionForm>,
    pub confirm: Option<Confirm>,
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
    pub donut_hover: Option<usize>,

    // Pages.
    pub search: TextEdit,
    pub sort: SortBy,
    pub ascending: bool,
    pub selected: Option<usize>,
    pub review: Loadable<Review>,
    pub simulation: Loadable<RebalanceSimulation>,
    pub briefing: Loadable<Briefing>,
    pub validation: Loadable<ValidationReport>,
    pub doctor: Loadable<WorkspaceHealth>,
    pub policy: PolicyPage,
    pub documents: DocumentsPage,
    pub settings: SettingsForm,
    pub has_api_key: bool,
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
            allocation: Vec::new(),
            performance: None,
            findings: Vec::new(),
            network: String::new(),
            dirty: false,
            loading: false,
            starting: true,
            error: None,
            updated_at: None,
            price_updates: 0,
            refreshed: None,
            page: Page::Overview,
            privacy: false,
            sidebar: PaneState::new(224.0).range(170.0, 340.0),
            inspector: PaneState::new(320.0).range(260.0, 460.0).collapsed(),
            drop_hover: false,
            palette: None,
            form: None,
            confirm: None,
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
            donut_hover: None,
            search: TextEdit::new(""),
            sort: SortBy::Value,
            ascending: false,
            selected: None,
            review: Loadable::default(),
            simulation: Loadable::default(),
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
        }
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
        let mut changed = self.drain() || mem::take(&mut self.settled);
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

    // --- Opening ---

    pub fn is_workspace(&self) -> bool {
        self.open.workspace_dir.is_some()
    }

    pub fn has_portfolio(&self) -> bool {
        self.open.is_open() && self.open.parse_error.is_none()
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
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.summary = Some(snapshot.summary);
        self.positions = snapshot.positions;
        self.allocation = snapshot.allocation;
        self.performance = Some(snapshot.performance);
        self.findings = snapshot.findings;
        self.network = snapshot.network;
        self.refreshed = Some(Instant::now());
        self.updated_at = Some(Local::now().format("%H:%M").to_string());
        self.load_page();
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
                    }
                    Err(error) => state.error = Some(error),
                }
            },
        );
    }

    pub fn load_file(&mut self, path: String) {
        self.change_and_reload(
            true,
            |state| state.dirty = false,
            move |backend| async move { backend.load_simple_file(path).await },
        );
    }

    pub fn load_workspace(&mut self, dir: String) {
        self.change_and_reload(
            true,
            |state| state.dirty = false,
            move |backend| async move { backend.load_workspace(dir).await },
        );
    }

    pub fn open_dropped(&mut self, path: &Path) {
        let string = path.to_string_lossy().into_owned();
        if path.is_dir() {
            self.load_workspace(string);
        } else {
            self.load_file(string);
        }
    }

    pub fn ask_for_file(&mut self, tx: &mut Tx) {
        let options = FileDialogOptions {
            title: "Open Portfolio".into(),
            ..Default::default()
        }
        .filter("Portfolio", &["json", "gpg"]);
        tx.open_file(options, |state: &mut State, result, _| match result {
            Ok(Some(path)) => state.load_file(path.to_string_lossy().into_owned()),
            Ok(None) => {}
            Err(error) => state.error = Some(error.to_string()),
        });
    }

    pub fn ask_for_workspace(&mut self, tx: &mut Tx) {
        self.ask_for_folder(tx, "Open Workspace", |state, dir| state.load_workspace(dir));
    }

    fn ask_for_folder(
        &mut self,
        tx: &mut Tx,
        title: &str,
        then: impl FnOnce(&mut State, String) + 'static,
    ) {
        let options = FileDialogOptions {
            title: title.into(),
            ..Default::default()
        };
        tx.open_folder(options, move |state: &mut State, result, _| match result {
            Ok(Some(path)) => then(state, path.to_string_lossy().into_owned()),
            Ok(None) => {}
            Err(error) => state.error = Some(error.to_string()),
        });
    }

    /// Read the open file again, after fixing it elsewhere.
    pub fn reload(&mut self) {
        self.change_and_reload(
            true,
            |state| state.dirty = false,
            |backend| async move { backend.reload().await },
        );
    }

    /// Forget the remembered portfolio and go back to the welcome screen.
    pub fn reset(&mut self) {
        self.run(
            |backend| async move { backend.clear_remembered_path().await },
            |state, result| match result {
                Ok(()) => {
                    state.open = OpenDocument::default();
                    state.summary = None;
                    state.positions.clear();
                    state.allocation.clear();
                    state.performance = None;
                    state.findings.clear();
                    state.dirty = false;
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
        self.page = page;
        self.chart_hover = None;
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
            Page::Overview | Page::Performance => self.load_history(),
            Page::Allocation | Page::Review => self.load_review(),
            Page::Rebalance => self.load_simulation(),
            Page::Briefing => self.load_briefing(),
            Page::Health => self.load_health(),
            Page::Policy => self.load_policy_page(),
            Page::Settings => self.load_settings(),
            Page::Holdings | Page::Documents => {}
        }
    }

    fn load_history(&mut self) {
        let period = if self.page == Page::Overview {
            Period::Month
        } else {
            self.period
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
        self.load_history();
    }

    pub fn load_review(&mut self) {
        self.review.loading = true;
        self.restart_draw = true;
        self.run(
            |backend| async move { backend.review().await },
            |state, result| state.review.finish(result),
        );
    }

    pub fn load_simulation(&mut self) {
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

    // --- Portfolio ---

    pub fn refresh_prices(&mut self) {
        if self.loading || !self.has_portfolio() {
            return;
        }
        // History follows the file on disk, so unsaved edits don't record.
        let record = !self.dirty;
        self.change_and_reload(
            record,
            |state| {
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
        let position = existing.and_then(|id| self.positions.iter().find(|p| p.id == id));
        self.form = Some(PositionForm::new(position));
    }

    pub fn submit_form(&mut self) {
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
        if self.dirty {
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
        if !self.dirty {
            return true;
        }
        self.confirm = Some(Confirm::Quit);
        if let Some(reopen) = &self.reopen {
            reopen();
        }
        false
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
            Command::Privacy => self.privacy = !self.privacy,
            Command::Theme => self.toggle_theme(),
            Command::Sidebar => self.sidebar.toggle(),
            Command::AddPosition => {
                self.navigate(Page::Holdings);
                self.open_form(None);
            }
            Command::CopyBriefing => self.copy_briefing(tx),
        }
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

    pub fn create_workspace(&mut self, tx: &mut Tx) {
        self.ask_for_folder(tx, "Create Workspace", |state, dir| {
            match state.backend.init_workspace(&dir) {
                Ok(()) => {
                    state.show_toast("Workspace created");
                    state.load_workspace(dir);
                }
                Err(error) => state.settings.error = Some(error),
            }
        });
    }

    pub fn export_skill(&mut self, tx: &mut Tx) {
        self.ask_for_folder(tx, "Export Agent Skill", |state, dir| {
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
            Ok(policy) => self.show_policy(policy),
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

#[cfg(test)]
mod tests {
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
