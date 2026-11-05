//! End-to-end tests: the real shell and pages, on real `portfolio_rs` data with
//! fixture quotes, driven through clicks and shortcuts in a headless window.
//!
//! Set `STAHL_SCREENSHOTS=<dir>` to also render every screen to PNG through
//! GPUI's offscreen Metal renderer, for looking at the result.
//!
//! macOS only for now: the headless platform there is the one exercised. These
//! never touch the user's config: `Backend` is pointed at a temp directory, and
//! opening goes through `startup`, which does not write it.

#![cfg_attr(not(target_os = "macos"), allow(dead_code, unused_imports))]

use std::{
    env, fs,
    path::Path,
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use chrono::{Duration as Days, Local};
use gpui_kit::{
    AnyWindowHandle, AssetSource as _, Bounds, ElementId, Entity, HeadlessAppContext, WindowBounds,
    WindowOptions, assets::IconName, component::WindowExt as _, platform, px, size,
    test::TestWindowExt as _,
};
use portfolio_rs::{AppConfig, FixtureProvider, services::quotes::AnyProvider, state::AppState};
use tempfile::TempDir;
use tokio::runtime::Runtime;

use stahl::{
    app::{self, Shell},
    assets::StahlAssets,
    backend::{Backend, Period},
    format,
    model::{self, AppModel, Privacy},
    pages::View,
};

const POSITIONS: &str = r#"[
  {"Id": "pos_btc", "Name": "Bitcoin", "AssetClass": "Crypto", "Ticker": "BTC-USD", "Amount": 0.45,
   "Purchases": [{"Date": "2025/05/28", "Quantity": 0.45, "Price": 42000.0}]},
  {"Id": "pos_spy", "Name": "S&P 500", "AssetClass": "Stocks", "Ticker": "SPY", "Amount": 80.0,
   "Purchases": [{"Date": "2024/01/10", "Quantity": 80.0, "Price": 410.0}]},
  {"Id": "pos_tlt", "Name": "Long Bonds", "AssetClass": "Bonds", "Ticker": "TLT", "Amount": 60.0,
   "Purchases": [{"Date": "2024/06/03", "Quantity": 60.0, "Price": 95.0}]},
  {"Id": "pos_gld", "Name": "Gold", "AssetClass": "Gold", "Ticker": "GLD", "Amount": 25.0,
   "Purchases": [{"Date": "2025/01/15", "Quantity": 25.0, "Price": 170.0}]},
  {"Id": "pos_cash", "Name": "Savings", "AssetClass": "Cash", "Amount": 30000.0}
]"#;

const POLICY: &str = r#"version = "1.0"
name = "Balanced Growth"
base_currency = "EUR"
time_horizon_years = 15
risk_profile = "moderate"

[constraints]
minimum_cash_amount = 20000.0
single_position_limit_percent = 40.0

[[allocations]]
asset_class = "Stocks"
target_percent = 50.0
tolerance_percent = 5.0

[[allocations]]
asset_class = "Bonds"
target_percent = 15.0
tolerance_percent = 5.0

[[allocations]]
asset_class = "Gold"
target_percent = 5.0
tolerance_percent = 2.0

[[allocations]]
asset_class = "Crypto"
target_percent = 10.0
tolerance_percent = 3.0

[[allocations]]
asset_class = "Cash"
target_percent = 20.0
tolerance_percent = 5.0
"#;

const QUOTES: &str = r#"{
  "latest": {
    "BTC-USD": {"price": 60000.0, "currency": "EUR"},
    "SPY": {"price": 500.0, "currency": "EUR"},
    "TLT": {"price": 90.0, "currency": "EUR"},
    "GLD": {"price": 200.0, "currency": "EUR"}
  },
  "previous_close": {"BTC-USD": 59000.0, "SPY": 503.0, "TLT": 90.5, "GLD": 196.0}
}"#;

/// A workspace with 90 days of history behind it, in a temp directory.
fn seed_workspace(dir: &Path) {
    fs::create_dir_all(dir.join("portfolio/history/main")).unwrap();
    fs::write(dir.join("positions.json"), POSITIONS).unwrap();
    fs::write(dir.join("portfolio/policy.toml"), POLICY).unwrap();
    let today = Local::now().date_naive();
    let holdings = [
        ("pos_btc", "Crypto", 0.45, 60000.0),
        ("pos_spy", "Stocks", 80.0, 500.0),
        ("pos_tlt", "Bonds", 60.0, 90.0),
        ("pos_gld", "Gold", 25.0, 200.0),
    ];
    // The same holdings every day: only prices move, so no money flows in.
    let lines: Vec<String> = (1..=90)
        .rev()
        .map(|days_ago: i64| {
            let date = today - Days::days(days_ago);
            let wave = 1.0 - days_ago as f64 * 0.0012 + (days_ago as f64 / 8.0).sin() * 0.02;
            let mut positions = vec![
                r#"{"positionId":"pos_cash","kind":"cash","assetClass":"Cash","quantity":30000.0,"valueBase":30000.0}"#
                    .to_string(),
            ];
            let mut total = 30000.0;
            for (id, class, quantity, price) in holdings {
                let price = ((price * wave) * 100.0_f64).round() / 100.0;
                let value = ((quantity * price) * 100.0_f64).round() / 100.0;
                total += value;
                positions.push(format!(
                    r#"{{"positionId":"{id}","kind":"market","assetClass":"{class}","quantity":{quantity},"price":{price},"priceCurrency":"EUR","fxRate":1.0,"valueBase":{value}}}"#
                ));
            }
            format!(
                r#"{{"date":"{date}","baseCurrency":"EUR","positions":[{}],"cashBase":30000.0,"totalBase":{total:.2},"netFlowsBase":0.0,"complete":true,"dataStatus":"complete"}}"#,
                positions.join(",")
            )
        })
        .collect();
    fs::write(
        dir.join("portfolio/history/main/snapshots.jsonl"),
        lines.join("\n") + "\n",
    )
    .unwrap();
}

struct Harness {
    cx: HeadlessAppContext,
    window: AnyWindowHandle,
    shell: Entity<Shell>,
    model: Entity<AppModel>,
    _runtime: Runtime,
    dir: TempDir,
}

impl Harness {
    /// Boot the app. `workspace` chooses a seeded workspace over `file`.
    fn boot(workspace: bool, file: Option<&str>) -> Self {
        let runtime = Runtime::new().unwrap();
        let dir = TempDir::new().unwrap();

        let mut config = AppConfig::default();
        if workspace {
            let workspace = dir.path().join("portfolio");
            seed_workspace(&workspace);
            config.set_workspace(workspace.to_string_lossy().into_owned());
        }
        let initial_file = file.map(|contents| {
            let path = dir.path().join("portfolio.json");
            fs::write(&path, contents).unwrap();
            path.to_string_lossy().into_owned()
        });
        let quotes = AnyProvider::Fixture(Box::new(FixtureProvider::from_json(QUOTES).unwrap()));
        let backend = Backend::new(
            AppState::with_quotes("EUR".to_string(), quotes),
            config,
            Some(dir.path().join("config.yml")),
            runtime.handle().clone(),
        );

        let mut cx = HeadlessAppContext::with_platform(
            platform::current_platform(true).text_system(),
            Arc::new(StahlAssets),
            platform::current_headless_renderer,
        );
        cx.allow_parking();
        let (window, shell) = cx.update(|cx| {
            app::install(backend, cx);
            let bounds = Bounds {
                origin: Default::default(),
                size: size(px(1320.), px(860.)),
            };
            app::open_main_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    focus: true,
                    show: false,
                    ..Default::default()
                },
                initial_file,
                cx,
            )
        });
        let model = cx.update(|cx| shell.read(cx).model.clone());
        let mut harness = Self {
            cx,
            window,
            shell,
            model,
            _runtime: runtime,
            dir,
        };
        harness.settle_until(|model| !model.starting);
        harness
    }

    fn read<R>(&mut self, read: impl FnOnce(&AppModel) -> R) -> R {
        let model = self.model.clone();
        self.cx.update(|cx| read(model.read(cx)))
    }

    /// Let UI tasks and the Tokio-side backend finish, then draw a frame.
    fn settle(&mut self) {
        for _ in 0..6 {
            thread::sleep(Duration::from_millis(15));
            self.cx.run_until_parked();
        }
        self.cx.advance_clock(Duration::from_secs(2));
        self.cx.run_until_parked();
        self.cx
            .update_window(self.window, |_, window, cx| window.render_frame(cx))
            .unwrap();
    }

    fn settle_until(&mut self, done: impl Fn(&AppModel) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.read(&done) {
            assert!(Instant::now() < deadline, "timed out waiting for the model");
            self.settle();
        }
        self.settle();
    }

    fn click(&mut self, id: impl Into<ElementId>) {
        let id = id.into();
        self.cx
            .update_window(self.window, |_, window, cx| window.click(id, cx))
            .unwrap();
        self.settle();
    }

    fn press(&mut self, key: &'static str) {
        self.cx
            .update_window(self.window, |_, window, cx| window.press(key, cx))
            .unwrap();
        self.settle();
    }

    fn type_text(&mut self, text: &'static str) {
        self.cx
            .update_window(self.window, |_, window, cx| window.input(text, cx))
            .unwrap();
        self.settle();
    }

    fn exists(&mut self, id: impl Into<ElementId>) -> bool {
        let id = id.into();
        self.cx
            .update_window(self.window, |_, window, _| window.try_find(id).is_some())
            .unwrap()
    }

    /// Wait for something a background task will bring onto the screen.
    fn wait_for(&mut self, id: &'static str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.exists(id) {
            assert!(Instant::now() < deadline, "timed out waiting for {id}");
            self.settle();
        }
    }

    fn current_view(&mut self) -> View {
        let shell = self.shell.clone();
        self.cx.update(|cx| shell.read(cx).view())
    }

    /// Render to PNG, when asked to with `STAHL_SCREENSHOTS`.
    fn screenshot(&mut self, name: &str) {
        let Ok(dir) = env::var("STAHL_SCREENSHOTS") else {
            return;
        };
        fs::create_dir_all(&dir).unwrap();
        self.settle();
        let image = self.cx.capture_screenshot(self.window).unwrap();
        image
            .save(Path::new(&dir).join(format!("{name}.png")))
            .unwrap();
    }
}

fn every_page_opens_on_a_real_workspace() {
    let mut app = Harness::boot(true, None);
    assert!(app.read(AppModel::is_workspace));
    assert_eq!(app.read(|model| model.positions.len()), 5);
    app.settle();
    app.screenshot("01-overview");

    for (view, name) in [
        (View::Holdings, "02-holdings"),
        (View::Allocation, "03-allocation"),
        (View::Performance, "04-performance"),
        (View::Review, "05-review"),
        (View::Rebalance, "06-rebalance"),
        (View::Policy, "07-policy"),
        (View::Documents, "08-documents"),
        (View::Briefing, "09-briefing"),
        (View::Health, "10-health"),
        (View::Settings, "11-settings"),
    ] {
        app.click(view.id());
        assert_eq!(app.current_view(), view);
        app.settle();
        app.screenshot(name);
    }
}

fn shortcuts_navigate_and_workspace_pages_stay_locked_without_one() {
    let mut app = Harness::boot(false, Some(POSITIONS));
    assert!(!app.read(AppModel::is_workspace));

    app.press("cmd-2");
    assert_eq!(app.current_view(), View::Holdings);
    app.press("cmd-0");
    assert_eq!(app.current_view(), View::Health);

    // Plan pages need a workspace.
    app.press("cmd-5");
    assert_eq!(app.current_view(), View::Health);
    assert!(!app.exists("nav-review"));
    assert!(app.exists("nav-unlock"));
}

fn opening_records_today_and_history_reaches_the_chart() {
    let app = Harness::boot(true, None);
    let stored = app
        .dir
        .path()
        .join("portfolio/portfolio/history/main/snapshots.jsonl");
    let text = fs::read_to_string(stored).unwrap();
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    assert_eq!(
        text.lines().count(),
        91,
        "one seeded line a day, plus today"
    );
    assert!(text.lines().last().unwrap().contains(&today));
}

fn price_only_moves_are_market_effect_not_contributions() {
    let mut app = Harness::boot(true, None);
    let backend = app.cx.update(|cx| cx.global::<Backend>().clone());
    let history = app
        ._runtime
        .block_on(backend.history(Period::Month))
        .unwrap();
    assert!(history.points.len() > 25, "{}", history.points.len());
    let net = history.net_contributions.expect("contributions are known");
    let market = history.market_effect.expect("market effect is known");
    assert!(net.abs() < 1.0, "no money moved, but net was {net}");
    assert!(
        market.abs() > 100.0,
        "prices moved, but effect was {market}"
    );
    let change = history.change.expect("change is known");
    assert!(
        (change - (net + market)).abs() < 1.0,
        "{change} != {net} + {market}"
    );
    assert!(history.twr_percent.is_some());
}

fn a_broken_file_shows_where_it_broke_and_recovers_on_reload() {
    let broken = "[\n  {\"Name\": \"Cash\",,}\n]";
    let mut app = Harness::boot(false, Some(broken));
    assert!(app.read(|model| model.open.parse_error.is_some()));
    assert_eq!(app.read(|model| model.positions.len()), 0);
    assert!(app.exists("reload-file"));
    assert!(
        !app.exists("sidebar-nav"),
        "not shown as an empty portfolio"
    );
    app.screenshot("20-parse-error");

    let path = app.dir.path().join("portfolio.json");
    assert_eq!(fs::read_to_string(&path).unwrap(), broken, "left untouched");
    fs::write(&path, POSITIONS).unwrap();

    app.click("reload-file");
    app.settle_until(|model| model.open.parse_error.is_none() && !model.loading);
    assert_eq!(app.read(|model| model.positions.len()), 5);
    assert!(app.exists("nav-overview"));
}

fn adding_a_position_marks_unsaved_until_save_writes_it() {
    let mut app = Harness::boot(false, Some(POSITIONS));
    app.press("cmd-2");
    assert!(!app.exists("save"));

    app.click("add-position");
    app.screenshot("21-add-position");
    app.click("form-submit");
    app.settle_until(|model| model.dirty && !model.loading);
    assert_eq!(app.read(|model| model.positions.len()), 6);
    assert!(app.exists("save"));

    let path = app.dir.path().join("portfolio.json");
    assert!(!fs::read_to_string(&path).unwrap().contains("Stock\""));

    app.press("cmd-s");
    app.settle_until(|model| !model.dirty && !model.loading);
    assert!(fs::read_to_string(&path).unwrap().contains("Stock\""));
    assert!(!app.exists("save"));
}

fn an_invalid_amount_keeps_the_form_open_and_changes_nothing() {
    let mut app = Harness::boot(false, Some(POSITIONS));
    app.press("cmd-2");
    app.click("add-position");

    // Focus moves to the Amount field by tabbing from Name.
    for _ in 0..4 {
        app.press("tab");
    }
    app.press("cmd-a");
    app.type_text("abc");
    app.click("form-submit");

    assert!(app.exists("form-submit"), "the form stays open");
    assert_eq!(app.read(|model| model.positions.len()), 5);
    assert!(!app.read(|model| model.dirty));
}

fn selecting_a_holding_opens_its_details_and_search_filters() {
    let mut app = Harness::boot(false, Some(POSITIONS));
    app.press("cmd-2");
    assert!(!app.exists("edit-position"));

    app.click(("holding", 0usize));
    assert!(app.exists("edit-position"));
    app.screenshot("22-holding-details");
    app.click(("holding", 0usize));
    assert!(!app.exists("edit-position"));

    // Bitcoin is position 0 and Gold is position 3.
    app.click("holdings-search");
    app.type_text("gold");
    assert!(!app.exists(("holding", 0usize)));
    assert!(app.exists(("holding", 3usize)));
}

fn a_generated_document_previews_without_writing_anything() {
    let mut app = Harness::boot(true, None);
    app.press("cmd-8");
    assert_eq!(app.current_view(), View::Documents);
    let reports = app.dir.path().join("portfolio/portfolio/reports");

    app.click("report-preview");
    app.wait_for("copy-document");
    app.screenshot("27-document-preview");
    assert!(!reports.exists(), "a preview writes nothing");

    app.click("report-save");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !reports.exists() {
        assert!(Instant::now() < deadline, "saving should write the report");
        app.settle();
    }
    assert_eq!(fs::read_dir(&reports).unwrap().count(), 1);
}

fn privacy_mode_hides_money_everywhere_it_is_formatted() {
    let mut app = Harness::boot(false, Some(POSITIONS));
    let hidden = |app: &mut Harness| app.cx.update(|cx| cx.global::<Privacy>().0);
    assert!(!hidden(&mut app));
    app.press("cmd-shift-h");
    assert!(hidden(&mut app));
    let text = app.cx.update(|cx| model::money(1234.5, "EUR", cx));
    assert_eq!(text, format::HIDDEN);
    app.screenshot("23-privacy");
    app.click("privacy");
    assert!(!hidden(&mut app));
}

fn the_theme_switches_and_persists() {
    let mut app = Harness::boot(false, Some(POSITIONS));
    app.click("toggle-theme");
    app.settle();
    app.screenshot("24-light");
    let config = fs::read_to_string(app.dir.path().join("config.yml")).unwrap();
    assert!(config.contains("light"), "saved: {config}");
}

fn the_command_palette_opens_from_the_keyboard() {
    let mut app = Harness::boot(false, Some(POSITIONS));
    app.press("cmd-k");
    let open = app
        .cx
        .update_window(app.window, |_, window, cx| window.has_active_dialog(cx))
        .unwrap();
    assert!(open);
    app.screenshot("25-palette");

    // Choosing an entry runs it and closes the palette.
    app.press("down");
    app.press("enter");
    assert_eq!(app.current_view(), View::Holdings);
    let open = app
        .cx
        .update_window(app.window, |_, window, cx| window.has_active_dialog(cx))
        .unwrap();
    assert!(!open);
}

fn no_portfolio_lands_on_the_welcome_screen() {
    let mut app = Harness::boot(false, None);
    assert!(app.exists("welcome-workspace"));
    assert!(app.exists("welcome-file"));
    assert!(!app.exists("nav-overview"));
    app.screenshot("26-welcome");
}

/// Every icon the app can draw must be in the embedded assets, or it silently
/// renders as nothing.
fn every_icon_resolves() {
    let views = [
        View::Overview,
        View::Holdings,
        View::Allocation,
        View::Performance,
        View::Review,
        View::Rebalance,
        View::Policy,
        View::Documents,
        View::Briefing,
        View::Health,
        View::Settings,
    ];
    let icons = views.map(View::icon).into_iter().chain([
        IconName::RefreshCw,
        IconName::Save,
        IconName::FileBraces,
        IconName::FolderOpen,
        IconName::EyeOff,
        IconName::Eye,
        IconName::Moon,
        IconName::Sun,
        IconName::Search,
        IconName::Copy,
        IconName::SquarePen,
        IconName::Trash,
        IconName::Plus,
        IconName::Shield,
        IconName::FileCheck,
        IconName::CircleCheck,
        IconName::CircleX,
        IconName::TriangleAlert,
    ]);
    for icon in icons {
        let path = icon.path();
        let data = StahlAssets.load(&path).unwrap_or_else(|_| panic!("{path}"));
        assert!(data.is_some_and(|data| !data.is_empty()), "{path}");
    }
}

fn main() {
    if cfg!(not(target_os = "macos")) {
        println!("e2e: skipped, headless rendering is only exercised on macOS");
        return;
    }
    let tests: &[(&str, fn())] = &[
        (
            "a_generated_document_previews_without_writing_anything",
            a_generated_document_previews_without_writing_anything,
        ),
        (
            "price_only_moves_are_market_effect_not_contributions",
            price_only_moves_are_market_effect_not_contributions,
        ),
        (
            "every_page_opens_on_a_real_workspace",
            every_page_opens_on_a_real_workspace,
        ),
        (
            "shortcuts_navigate_and_workspace_pages_stay_locked_without_one",
            shortcuts_navigate_and_workspace_pages_stay_locked_without_one,
        ),
        (
            "opening_records_today_and_history_reaches_the_chart",
            opening_records_today_and_history_reaches_the_chart,
        ),
        (
            "a_broken_file_shows_where_it_broke_and_recovers_on_reload",
            a_broken_file_shows_where_it_broke_and_recovers_on_reload,
        ),
        (
            "adding_a_position_marks_unsaved_until_save_writes_it",
            adding_a_position_marks_unsaved_until_save_writes_it,
        ),
        (
            "an_invalid_amount_keeps_the_form_open_and_changes_nothing",
            an_invalid_amount_keeps_the_form_open_and_changes_nothing,
        ),
        (
            "selecting_a_holding_opens_its_details_and_search_filters",
            selecting_a_holding_opens_its_details_and_search_filters,
        ),
        (
            "privacy_mode_hides_money_everywhere_it_is_formatted",
            privacy_mode_hides_money_everywhere_it_is_formatted,
        ),
        (
            "the_theme_switches_and_persists",
            the_theme_switches_and_persists,
        ),
        (
            "the_command_palette_opens_from_the_keyboard",
            the_command_palette_opens_from_the_keyboard,
        ),
        (
            "no_portfolio_lands_on_the_welcome_screen",
            no_portfolio_lands_on_the_welcome_screen,
        ),
        ("every_icon_resolves", every_icon_resolves),
    ];
    // Parts of portfolio_rs resolve their own quote provider from this. Tests
    // must never reach the network.
    // SAFETY: first thing in main, before any other thread exists.
    unsafe { env::set_var("PORTFOLIO_RS_QUOTES", "offline") };

    let filter = env::args().nth(1).filter(|arg| !arg.starts_with('-'));
    for (name, test) in tests {
        if filter.as_deref().is_none_or(|filter| name.contains(filter)) {
            test();
            println!("test {name} ... ok");
        }
    }
}
