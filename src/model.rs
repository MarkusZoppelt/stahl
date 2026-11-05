//! The state shared across the window: which portfolio is open and its data.

use std::path::{Path, PathBuf};

use chrono::Local;
use gpui_kit::{App, Context, Global, PathPromptOptions};
use portfolio_rs::{
    config::ThemeMode,
    mcp::dto::Finding,
    state::{
        AllocationItemDto, AppConfigDto, CreatePositionRequest, PerformanceDto,
        PortfolioSummaryDto, PositionDto, UpdatePositionRequest,
    },
};

use crate::{
    backend::{Backend, BackendResult, OpenDocument, Snapshot},
    format, theme, ui,
};

/// Whether amounts are on screen. Per session: it should never start hidden
/// and leave someone wondering where their money went.
#[derive(Default)]
pub struct Privacy(pub bool);

impl Global for Privacy {}

/// Format money, unless privacy mode hides it.
pub fn money(value: f64, currency: &str, cx: &App) -> String {
    if cx.global::<Privacy>().0 {
        format::HIDDEN.to_string()
    } else {
        format::currency(value, currency)
    }
}

/// [`money`], with a sign for changes.
pub fn signed_money(value: f64, currency: &str, cx: &App) -> String {
    if cx.global::<Privacy>().0 {
        format::HIDDEN.to_string()
    } else {
        format::signed_currency(value, currency)
    }
}

pub struct AppModel {
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
    /// Bumped whenever portfolio data changes, for pages deriving from it.
    pub revision: u64,
    pub updated_at: Option<String>,
}

impl AppModel {
    pub fn new(initial_file: Option<String>, cx: &mut Context<Self>) -> Self {
        let model = Self {
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
            revision: 0,
            updated_at: None,
        };
        model.startup(initial_file, cx);
        model
    }

    pub fn currency(&self) -> String {
        self.config
            .as_ref()
            .map_or_else(|| "EUR".to_string(), |config| config.currency.clone())
    }

    pub fn is_workspace(&self) -> bool {
        self.open.workspace_dir.is_some()
    }

    pub fn has_portfolio(&self) -> bool {
        self.open.is_open() && self.open.parse_error.is_none()
    }

    fn startup(&self, initial_file: Option<String>, cx: &mut Context<Self>) {
        ui::run_backend(
            cx,
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
            |model: &mut Self, result, cx| {
                model.starting = false;
                match result {
                    Ok((config, open, snapshot)) => {
                        theme::apply(config.theme, cx);
                        model.config = Some(config);
                        match open {
                            Ok(open) => model.open = open,
                            Err(error) => model.error = Some(error),
                        }
                        if let Some(snapshot) = snapshot {
                            model.apply_snapshot(snapshot);
                        }
                    }
                    Err(error) => model.error = Some(error),
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
        self.revision += 1;
        self.updated_at = Some(Local::now().format("%H:%M").to_string());
    }

    /// Run a change, then re-read everything it can affect.
    fn change_and_reload<F>(
        &mut self,
        cx: &mut Context<Self>,
        record: bool,
        after: impl FnOnce(&mut Self) + 'static,
        work: impl FnOnce(Backend) -> F + Send + 'static,
    ) where
        F: Future<Output = BackendResult<()>> + Send + 'static,
    {
        self.loading = true;
        self.error = None;
        cx.notify();
        ui::run_backend(
            cx,
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
            move |model: &mut Self, result, _| {
                model.loading = false;
                match result {
                    Ok((open, snapshot, config)) => {
                        model.open = open;
                        model.config = Some(config);
                        if let Some(snapshot) = snapshot {
                            model.apply_snapshot(snapshot);
                        }
                        after(model);
                    }
                    Err(error) => model.error = Some(error),
                }
            },
        );
    }

    // --- Opening ---

    pub fn open_file(&mut self, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Open".into()),
        });
        cx.spawn(async move |model, cx| {
            let picked = paths.await.ok().and_then(Result::ok).flatten();
            let Some(path) = picked.and_then(first_path) else {
                return;
            };
            let _ = model.update(cx, |model, cx| model.load_file(path, cx));
        })
        .detach();
    }

    pub fn open_workspace(&mut self, cx: &mut Context<Self>) {
        let dir = pick_directory(cx);
        cx.spawn(async move |model, cx| {
            let Some(dir) = dir.await else {
                return;
            };
            let _ = model.update(cx, |model, cx| model.load_workspace(dir, cx));
        })
        .detach();
    }

    pub fn load_file(&mut self, path: String, cx: &mut Context<Self>) {
        self.change_and_reload(
            cx,
            true,
            |model| model.dirty = false,
            move |backend| async move { backend.load_simple_file(path).await },
        );
    }

    pub fn load_workspace(&mut self, dir: String, cx: &mut Context<Self>) {
        self.change_and_reload(
            cx,
            true,
            |model| model.dirty = false,
            move |backend| async move { backend.load_workspace(dir).await },
        );
    }

    /// Open what was dropped on the window: a folder is a workspace.
    pub fn open_dropped(&mut self, path: &Path, cx: &mut Context<Self>) {
        let path_string = path.to_string_lossy().into_owned();
        if path.is_dir() {
            self.load_workspace(path_string, cx);
        } else {
            self.load_file(path_string, cx);
        }
    }

    /// Read the open file again, after fixing it elsewhere.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.change_and_reload(
            cx,
            true,
            |model| model.dirty = false,
            |backend| async move { backend.reload().await },
        );
    }

    /// Forget the remembered portfolio and go back to the welcome screen.
    pub fn reset(&mut self, cx: &mut Context<Self>) {
        ui::run_backend(
            cx,
            |backend| async move { backend.clear_remembered_path().await },
            |model: &mut Self, result, _| match result {
                Ok(()) => {
                    model.open = OpenDocument::default();
                    model.summary = None;
                    model.positions.clear();
                    model.allocation.clear();
                    model.performance = None;
                    model.findings.clear();
                    model.dirty = false;
                }
                Err(error) => model.error = Some(error),
            },
        );
    }

    // --- Portfolio ---

    pub fn refresh_prices(&mut self, cx: &mut Context<Self>) {
        if self.loading || !self.has_portfolio() {
            return;
        }
        // History follows the file on disk, so unsaved edits don't record.
        let record = !self.dirty;
        self.change_and_reload(
            cx,
            record,
            |_| {},
            |backend| async move {
                backend.refresh_prices().await;
                Ok(())
            },
        );
    }

    pub fn save(&mut self, cx: &mut Context<Self>) {
        if !self.has_portfolio() {
            return;
        }
        self.change_and_reload(
            cx,
            true,
            |model| model.dirty = false,
            |backend| async move { backend.save_portfolio().await },
        );
    }

    pub fn add_position(&mut self, request: CreatePositionRequest, cx: &mut Context<Self>) {
        self.change_and_reload(
            cx,
            false,
            |model| model.dirty = true,
            move |backend| async move { backend.add_position(request).await },
        );
    }

    pub fn update_position(
        &mut self,
        id: usize,
        request: UpdatePositionRequest,
        cx: &mut Context<Self>,
    ) {
        self.change_and_reload(
            cx,
            false,
            |model| model.dirty = true,
            move |backend| async move { backend.update_position(id, request).await },
        );
    }

    pub fn delete_position(&mut self, id: usize, cx: &mut Context<Self>) {
        self.change_and_reload(
            cx,
            false,
            |model| model.dirty = true,
            move |backend| async move { backend.delete_position(id).await },
        );
    }

    // --- Settings ---

    pub fn theme_mode(&self) -> ThemeMode {
        self.config
            .as_ref()
            .map_or(ThemeMode::Dark, |config| config.theme)
    }

    pub fn set_theme(&mut self, mode: ThemeMode, cx: &mut Context<Self>) {
        theme::apply(mode, cx);
        if let Some(config) = &mut self.config {
            config.theme = mode;
        }
        cx.notify();
        ui::run_backend(
            cx,
            move |backend| async move { backend.set_theme(mode).await },
            |model: &mut Self, result, _| {
                if let Err(error) = result {
                    model.error = Some(error);
                }
            },
        );
    }

    /// Values are in the base currency, so changing it re-reads them.
    pub fn set_currency(&mut self, currency: String, cx: &mut Context<Self>) {
        self.change_and_reload(
            cx,
            false,
            |_| {},
            move |backend| async move { backend.set_currency(currency).await },
        );
    }

    pub fn reload_config(&mut self, cx: &mut Context<Self>) {
        ui::run_backend(
            cx,
            |backend| async move { Ok(backend.config().await) },
            |model: &mut Self, result, _| {
                if let Ok(config) = result {
                    model.config = Some(config);
                }
            },
        );
    }
}

fn first_path(paths: Vec<PathBuf>) -> Option<String> {
    let path = paths.into_iter().next()?;
    Some(path.to_string_lossy().into_owned())
}

/// Ask for a folder with the system's open panel.
pub fn pick_directory<V: 'static>(
    cx: &mut Context<V>,
) -> impl Future<Output = Option<String>> + use<V> {
    let paths = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: Some("Choose".into()),
    });
    async move { first_path(paths.await.ok()?.ok()??) }
}
