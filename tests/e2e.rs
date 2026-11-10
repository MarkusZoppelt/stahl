//! End-to-end tests: the real views and handlers on real `portfolio_rs` data with fixture
//! quotes, driven through pointer and keyboard input in Rune's headless runtime.
//!
//! These never touch the user's config: the backend keeps it in a temp directory.

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::{
    env, fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process,
    sync::{
        Arc, Once,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use chrono::{Days, Local};
use portfolio_rs::{
    AppConfig, FixtureProvider,
    services::quotes::AnyProvider,
    state::{AppState, PositionDto},
};
use rune::{FileDialogKind, Key, KeyEvent, Mods, Runtime, TouchPhase};
use stahl::{
    app,
    backend::{Backend, Period},
    demo::{self, FakeReleases},
    state::{Command, Confirm, Page, PendingOpen, PickKind, State, UpdatePhase},
    update::{self, Installation, Updater},
    view::guide,
};
use tempfile::TempDir;
use tokio::runtime::Runtime as Tokio;
use zip::{ZipWriter, write::SimpleFileOptions};

/// What the Edit form says when purchases decide the amount.
const PURCHASES_NOTE: &str = "Amount is the total of this position's purchases";

struct Harness {
    rt: Runtime<State>,
    dir: TempDir,
    _tokio: Tokio,
}

fn offline() {
    static ONCE: Once = Once::new();
    // Parts of portfolio_rs resolve their own quote provider from the first, and a portfolio with
    // no workspace keeps its history under the second. Tests must never reach the network or the
    // real history. SAFETY: before any test here reads the environment.
    ONCE.call_once(|| unsafe {
        env::set_var("PORTFOLIO_RS_QUOTES", "offline");
        let data = env::temp_dir().join(format!("stahl-e2e-{}", process::id()));
        env::set_var("XDG_DATA_HOME", data);
    });
}

impl Harness {
    fn boot(workspace: bool, file: Option<&str>) -> Harness {
        Self::boot_with(workspace, file, Updater::disabled())
    }

    fn boot_with(workspace: bool, file: Option<&str>, updater: Updater) -> Harness {
        offline();
        let tokio = Tokio::new().unwrap();
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("portfolio");
        if workspace {
            demo::seed_workspace(&root);
        }
        let initial = file.map(|contents| {
            let path = dir.path().join("portfolio.json");
            fs::write(&path, contents).unwrap();
            path.to_string_lossy().into_owned()
        });
        let backend = demo::backend(
            dir.path(),
            workspace.then_some(root.as_path()),
            tokio.handle().clone(),
        );
        let rt = app::headless_with_updater(backend, initial, (1320.0, 860.0), updater);
        let mut harness = Harness {
            rt,
            dir,
            _tokio: tokio,
        };
        harness.settle();
        harness
    }

    /// Open `contents` with base currency EUR and prices from a fixture of `quotes`.
    fn boot_with_quotes(contents: &str, quotes: &str) -> Harness {
        offline();
        let tokio = Tokio::new().unwrap();
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("portfolio.json");
        fs::write(&path, contents).unwrap();
        let provider = FixtureProvider::from_json(quotes).unwrap();
        let backend = Backend::new(
            AppState::with_quotes("EUR".to_string(), AnyProvider::Fixture(Box::new(provider))),
            AppConfig::default(),
            Some(dir.path().join("config.yml")),
            tokio.handle().clone(),
        );
        let initial = Some(path.to_string_lossy().into_owned());
        let rt = app::headless(backend, initial, (1320.0, 860.0));
        let mut harness = Harness {
            rt,
            dir,
            _tokio: tokio,
        };
        harness.settle();
        harness
    }

    /// Let finished work land and animations run.
    fn settle(&mut self) {
        self.rt.state.settle();
        for _ in 0..90 {
            self.rt.frame(1.0 / 60.0);
        }
        self.rt.state.settle();
        for _ in 0..30 {
            self.rt.frame(1.0 / 60.0);
        }
    }

    fn click(&mut self, key: &str) {
        let (x, y) = self
            .rt
            .find(key)
            .unwrap_or_else(|| panic!("nothing to click at {key}"));
        self.rt.pointer_move(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_down(x, y);
        self.rt.pointer_up(x, y);
        self.settle();
    }

    fn command(&mut self, key: char, shift: bool) {
        let mods = Mods {
            cmd: true,
            shift,
            ..Mods::default()
        };
        self.rt.key(KeyEvent {
            key: Key::Command(key),
            mods,
        });
        self.settle();
    }

    fn press(&mut self, key: Key) {
        self.rt.key(KeyEvent {
            key,
            mods: Mods::default(),
        });
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            self.rt.key(KeyEvent {
                key: Key::Text(character.to_string()),
                mods: Mods::default(),
            });
        }
        self.settle();
    }

    fn exists(&self, key: &str) -> bool {
        self.rt.find(key).is_some()
    }

    fn has_text(&self, wanted: &str) -> bool {
        self.rt.elements().iter().any(|element| {
            element
                .text
                .as_deref()
                .is_some_and(|text| text.contains(wanted))
        })
    }
}

#[test]
fn every_page_opens_on_a_real_workspace() {
    let mut app = Harness::boot(true, None);
    assert!(app.rt.state.is_workspace());
    assert_eq!(app.rt.state.positions.len(), 5);
    assert!(app.has_text("€107,400.00"), "the hero shows the total");

    for (page, wanted) in [
        (Page::Holdings, "Bitcoin"),
        (Page::Allocation, "The outer ring shows your policy targets"),
        (Page::Performance, "Time-weighted return"),
        (Page::Review, "Allocation vs target"),
        (Page::Rebalance, "Full Rebalance to Targets"),
        (Page::Policy, "Balanced Growth"),
        (Page::Documents, "Weekly report"),
        (Page::Briefing, "Cash buffer"),
        (Page::Health, "Portfolio file is valid"),
        (Page::Settings, "AI provider"),
        (Page::Overview, "Top movers today"),
    ] {
        app.click(page.key());
        assert_eq!(app.rt.state.page, page);
        assert!(app.has_text(wanted), "{page:?} should show {wanted:?}");
    }
}

#[test]
fn shortcuts_navigate_and_workspace_pages_stay_locked_without_one() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    assert!(!app.rt.state.is_workspace());
    app.command('2', false);
    assert_eq!(app.rt.state.page, Page::Holdings);
    app.command('0', false);
    assert_eq!(app.rt.state.page, Page::Health);

    app.command('5', false);
    assert_eq!(app.rt.state.page, Page::Health, "Review needs a workspace");
    assert!(!app.exists(Page::Review.key()));
    assert!(app.exists("nav-unlock"));
}

#[test]
fn opening_records_today_once() {
    let mut app = Harness::boot(true, None);
    let stored = app
        .dir
        .path()
        .join("portfolio/portfolio/history/main/snapshots.jsonl");
    let today = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let lines = |path: &Path| fs::read_to_string(path).unwrap().lines().count();
    assert_eq!(lines(&stored), 91, "one seeded line a day, plus today");
    assert!(
        fs::read_to_string(&stored)
            .unwrap()
            .lines()
            .last()
            .unwrap()
            .contains(&today)
    );

    app.command('r', false);
    assert_eq!(lines(&stored), 91, "refreshing replaces today's line");
}

fn backend_of(app: &Harness) -> Backend {
    app.rt.state.backend.clone()
}

#[test]
fn price_only_moves_are_market_effect_not_contributions() {
    let app = Harness::boot(true, None);
    let backend = backend_of(&app);
    let history = app._tokio.block_on(backend.history(Period::Month)).unwrap();
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

#[test]
fn a_broken_file_shows_where_it_broke_and_recovers_on_reload() {
    let broken = "[\n  {\"Name\": \"Cash\",,}\n]";
    let mut app = Harness::boot(false, Some(broken));
    assert!(app.rt.state.open.parse_error.is_some());
    assert!(app.rt.state.positions.is_empty());
    assert!(app.exists("reload-file"));
    assert!(
        app.has_text("isn't valid JSON (line 2, column"),
        "the line and column, in words"
    );
    assert!(
        app.has_text("Details:"),
        "the parser's own text stays available"
    );
    assert!(
        !app.exists(Page::Overview.key()),
        "not shown as an empty portfolio"
    );

    let path = app.dir.path().join("portfolio.json");
    assert_eq!(fs::read_to_string(&path).unwrap(), broken, "left untouched");
    fs::write(&path, demo::POSITIONS).unwrap();

    app.click("reload-file");
    assert!(app.rt.state.open.parse_error.is_none());
    assert_eq!(app.rt.state.positions.len(), 5);
    assert!(app.exists(Page::Overview.key()));
}

#[test]
fn adding_a_position_is_unsaved_until_saved() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('2', false);
    assert!(!app.exists("save"));

    app.click("add-position");
    assert!(app.exists("form-submit"));
    assert!(app.has_text("Asset class"), "fields have visible labels");
    let dialog = app
        .rt
        .presented("position-dialog")
        .expect("the dialog is laid out")
        .0;
    let ticker = app
        .rt
        .presented("form-ticker")
        .expect("the field is laid out")
        .0;
    assert!(
        ticker[0] + ticker[2] <= dialog[0] + dialog[2],
        "fields stay inside the dialog: {ticker:?} in {dialog:?}"
    );
    app.type_text("Apple");
    app.click("form-submit");
    assert_eq!(app.rt.state.positions.len(), 6);
    assert!(app.rt.state.dirty);
    assert!(app.exists("save"));

    let path = app.dir.path().join("portfolio.json");
    assert!(!fs::read_to_string(&path).unwrap().contains("Apple"));

    app.command('s', false);
    assert!(!app.rt.state.dirty);
    assert!(fs::read_to_string(&path).unwrap().contains("Apple"));
    assert!(!app.exists("save"));
}

#[test]
fn an_invalid_amount_keeps_the_form_open_and_changes_nothing() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('n', false);
    app.rt
        .state
        .form
        .as_mut()
        .unwrap()
        .amount
        .replace_text("abc");
    app.settle();
    app.click("form-submit");

    assert!(app.rt.state.form.is_some(), "the form stays open");
    assert!(app.has_text("Amount \"abc\" isn't a number."));
    assert_eq!(app.rt.state.positions.len(), 5);
    assert!(!app.rt.state.dirty);
}

/// Type into the real dialog through pointer and keys. Amount starts out holding its default `0`,
/// and the caret lands after it, as it does for someone clicking the field.
fn fill_position_form(app: &mut Harness, amount: &str, quantity: &str, price: &str) {
    app.command('n', false);
    app.type_text("Zed");
    for (key, text) in [
        ("form-amount", amount),
        ("form-quantity", quantity),
        ("form-price", price),
    ] {
        if !text.is_empty() {
            app.click(key);
            app.type_text(text);
        }
    }
    app.click("form-submit");
}

fn added_position(app: &Harness) -> Option<&PositionDto> {
    app.rt
        .state
        .positions
        .iter()
        .find(|position| position.name.as_deref() == Some("Zed"))
}

#[test]
fn numbers_with_a_decimal_comma_or_thousands_separators_are_read() {
    for (typed, wanted) in [
        ("1,5", 1.5),
        ("150,00", 150.0),
        ("1,000.50", 1000.5),
        ("1.000,50", 1000.5),
        ("1.000.000", 1_000_000.0),
        ("1 000", 1000.0),
        ("0,125", 0.125),
        ("1.5", 1.5),
    ] {
        let mut app = Harness::boot(true, None);
        fill_position_form(&mut app, typed, "", "");
        let added = added_position(&app);
        assert_eq!(added.map(|p| p.amount), Some(wanted), "amount {typed:?}");
    }

    let mut app = Harness::boot(true, None);
    fill_position_form(&mut app, "", "2,5", "150,50");
    let added = added_position(&app).expect("the position was added");
    assert_eq!(added.amount, 2.5);
    assert_eq!(added.average_cost, Some(150.5));
}

#[test]
fn a_number_that_could_be_off_by_a_thousand_is_refused() {
    for (amount, quantity, price, wanted) in [
        (
            "1.000",
            "",
            "",
            "Amount \"01.000\" could mean 1000 or 1.000",
        ),
        (
            "1,000",
            "",
            "",
            "Amount \"01,000\" could mean 1000 or 1.000",
        ),
        (
            "",
            "1,000",
            "",
            "Quantity \"1,000\" could mean 1000 or 1.000",
        ),
        (
            "",
            "",
            "12.345",
            "Price \"12.345\" could mean 12345 or 12.345",
        ),
    ] {
        let mut app = Harness::boot(true, None);
        fill_position_form(&mut app, amount, quantity, price);
        assert!(app.rt.state.form.is_some(), "the form stays open");
        assert!(app.has_text(wanted), "should explain {wanted:?}");
        assert_eq!(app.rt.state.positions.len(), 5, "nothing was added");
    }
}

#[test]
fn a_bad_number_is_blamed_on_its_own_field() {
    for (quantity, price, wanted) in [
        ("abc", "", "Quantity \"abc\" isn't a number."),
        ("", "\u{20ac}150", "Price \"\u{20ac}150\" isn't a number."),
        ("-3", "100", "Quantity can't be negative."),
        ("3", "-100", "Price can't be negative."),
    ] {
        let mut app = Harness::boot(true, None);
        fill_position_form(&mut app, "", quantity, price);
        assert!(app.has_text(wanted), "should say {wanted:?}");
        assert_eq!(app.rt.state.positions.len(), 5, "nothing was added");
    }
}

#[test]
fn editing_saves_an_amount_with_three_decimals_unchanged() {
    let mut app = Harness::boot(true, None);
    fill_position_form(&mut app, "12.3450", "", "");
    let id = added_position(&app).expect("the position was added").id;
    assert_eq!(added_position(&app).map(|p| p.amount), Some(12.345));

    app.command('2', false);
    app.click(&format!("holding-{id}"));
    app.click("edit-position");
    app.click("form-submit");
    assert!(app.rt.state.form.is_none(), "saved without complaint");
    assert_eq!(added_position(&app).map(|p| p.amount), Some(12.345));
}

/// Open the real Edit dialog. In the demo data, 0 is Bitcoin, which has one purchase of 0.45, and
/// 4 is Savings, 30000 with no purchases.
fn edit_position(app: &mut Harness, id: usize) {
    app.command('2', false);
    app.click(&format!("holding-{id}"));
    app.click("edit-position");
    assert!(app.rt.state.form.is_some(), "the form opened");
}

fn amount_of(app: &Harness, id: usize) -> f64 {
    app.rt
        .state
        .positions
        .iter()
        .find(|position| position.id == id)
        .expect("the position exists")
        .amount
}

#[test]
fn an_amount_that_purchases_decide_isnt_silently_ignored() {
    let mut app = Harness::boot(true, None);
    edit_position(&mut app, 0);
    app.click("form-amount");
    app.type_text("1");
    app.click("form-submit");

    assert!(app.rt.state.form.is_none(), "the form closed");
    assert_eq!(amount_of(&app, 0), 0.45, "purchases still decide");
    assert!(!app.rt.state.dirty, "nothing changed, so it isn't Edited");
    assert!(!app.exists("save"));

    app.click("edit-position");
    assert_eq!(app.rt.state.form.as_ref().unwrap().amount.text, "0.45");
}

#[test]
fn an_amount_that_purchases_decide_is_read_only_and_says_why() {
    let mut app = Harness::boot(true, None);
    edit_position(&mut app, 0);
    assert!(app.has_text(PURCHASES_NOTE), "the form explains itself");
    assert_eq!(app.rt.state.form.as_ref().unwrap().amount.text, "0.45");

    app.click("form-amount");
    app.type_text("1");
    assert_eq!(
        app.rt.state.form.as_ref().unwrap().amount.text,
        "0.45",
        "typing can't reach it"
    );
}

#[test]
fn a_position_with_purchases_can_still_be_renamed() {
    let mut app = Harness::boot(true, None);
    edit_position(&mut app, 0);
    app.type_text(" Core");
    app.press(Key::Enter);

    assert!(app.rt.state.form.is_none(), "enter saves");
    let position = &app.rt.state.positions[0];
    assert_eq!(position.name.as_deref(), Some("Bitcoin Core"));
    assert_eq!(position.amount, 0.45);
    assert_eq!(position.purchases.len(), 1, "no purchase was made up");
    assert_eq!(position.invested, Some(18900.0), "cost basis is untouched");
    assert!(app.rt.state.dirty);
}

#[test]
fn an_amount_without_purchases_can_still_be_edited() {
    let mut app = Harness::boot(true, None);
    edit_position(&mut app, 4);
    assert!(!app.has_text(PURCHASES_NOTE));
    app.click("form-amount");
    app.type_text("5");
    app.click("form-submit");

    assert_eq!(amount_of(&app, 4), 300005.0);
    assert!(app.rt.state.dirty);

    let path = app.dir.path().join("portfolio/positions.json");
    assert!(!fs::read_to_string(&path).unwrap().contains("300005"));
    app.command('s', false);
    assert!(fs::read_to_string(&path).unwrap().contains("300005"));
}

#[test]
fn saving_an_unchanged_form_isnt_an_edit() {
    for id in [0, 4] {
        let mut app = Harness::boot(true, None);
        edit_position(&mut app, id);
        app.click("form-submit");
        assert!(app.rt.state.form.is_none(), "the form closed");
        assert!(!app.rt.state.dirty, "position {id} didn't change");
    }
}

#[test]
fn deleting_asks_first() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('2', false);
    app.click("holding-0");
    app.click("delete-position");
    assert!(matches!(app.rt.state.confirm, Some(Confirm::Delete { .. })));
    assert_eq!(app.rt.state.positions.len(), 5, "nothing yet");

    app.click("confirm-delete");
    assert_eq!(app.rt.state.positions.len(), 4);
    assert!(app.rt.state.dirty);
    assert!(app.rt.state.selected.is_none());
}

#[test]
fn selecting_a_holding_opens_its_details_and_search_filters() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('2', false);
    assert!(!app.exists("edit-position"));

    app.click("holding-0");
    assert!(app.exists("edit-position"));
    assert!(app.has_text("Average cost"));
    app.click("holding-0");
    assert!(app.rt.state.selected.is_none());

    // Bitcoin is position 0 and Gold is position 3.
    app.click("holdings-search");
    app.type_text("gold");
    assert!(!app.exists("holding-0"));
    assert!(app.exists("holding-3"));
}

/// A holding quoted in dollars, in a portfolio that reports in euros.
const BITCOIN: &str = r#"[
  {"Id": "pos_btc", "Name": "Bitcoin", "AssetClass": "Crypto", "Ticker": "BTC-USD", "Amount": 0.45,
   "Purchases": [{"Date": "2025/05/28", "Quantity": 0.45, "Price": 50000.0}]}
]"#;

/// 1 EUR buys 1.25 USD, so 1 USD is €0.80.
const DOLLAR_QUOTES: &str = r#"{
  "latest": {
    "BTC-USD": {"price": 60000.0, "currency": "USD"},
    "EURUSD=X": {"price": 1.25, "currency": "USD"}
  },
  "previous_close": {"BTC-USD": 59000.0}
}"#;

#[test]
fn a_foreign_holding_shows_each_figure_in_its_own_currency() {
    let mut app = Harness::boot_with_quotes(BITCOIN, DOLLAR_QUOTES);
    app.command('2', false);
    app.click("holding-0");

    // In the quote currency.
    assert!(app.has_text("$60,000.00"), "the price");
    assert!(app.has_text("$50,000.00"), "the average cost");
    assert!(app.has_text("0.45 @ $50,000.00"), "the purchase");
    // In the base currency: 0.45 × $60,000 × 0.8 and 0.45 × $50,000 × 0.8.
    assert!(app.has_text("€21,600.00"), "the value");
    assert!(app.has_text("€18,000.00"), "what was invested");
    assert!(app.has_text("+€3,600.00"), "the PnL");
    // The price that multiplies by the quantity to give the value.
    assert!(app.has_text("€48,000.00"), "the price in the base currency");
    // Never a dollar amount with a euro sign.
    assert!(!app.has_text("€60,000.00"));
    assert!(!app.has_text("€50,000.00"));
}

#[test]
fn sorting_reverses_on_a_second_click() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('2', false);
    let order = |app: &Harness| {
        let (_, top_gold) = app.rt.find("holding-3").unwrap();
        let (_, top_spy) = app.rt.find("holding-1").unwrap();
        top_spy < top_gold
    };
    assert!(order(&app), "biggest value first");
    app.click("sort-value");
    assert!(!order(&app), "then smallest first");
}

#[test]
fn privacy_mode_hides_money() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    assert!(app.has_text("€107,400.00"));
    app.command('h', true);
    assert!(app.rt.state.privacy);
    assert!(!app.has_text("€107,400.00"));
    app.click("privacy");
    assert!(app.has_text("€107,400.00"));
}

#[test]
fn overview_says_what_changed_since_the_last_look() {
    let mut app = Harness::boot(true, None);
    let today = Local::now().date_naive();
    let last = today - Days::new(5);
    let since = format!("Since {},", last.format("%A"));
    let statement = |app: &Harness| {
        app.rt
            .elements()
            .iter()
            .find(|element| element.key.as_deref() == Some("statement-text"))
            .and_then(|element| element.text.clone())
            .expect("the Statement is on screen")
    };
    let said = statement(&app);
    assert!(said.starts_with(&since), "{said}");
    assert!(said.contains('€'), "{said}");

    // This look is noted beside the history, as a date and nothing else.
    let seen = app
        .dir
        .path()
        .join("portfolio/portfolio/history/main/seen.json");
    let written = fs::read_to_string(seen).unwrap();
    assert!(written.contains(&today.format("%Y-%m-%d").to_string()));
    assert!(written.contains(&last.format("%Y-%m-%d").to_string()));

    // Privacy keeps amounts out of the sentences too.
    app.command('h', true);
    let hidden = statement(&app);
    assert!(hidden.starts_with(&since), "{hidden}");
    assert!(!hidden.contains('€'), "{hidden}");
}

#[test]
fn overview_agrees_with_the_review_and_leads_to_the_fix() {
    let mut app = Harness::boot(true, None);
    // The demo is four classes out of tolerance: Overview must not call that all clear.
    assert!(!app.has_text("Nothing right now"));
    assert!(app.has_text("4 of 5 asset classes are outside their tolerance"));
    assert_eq!(app.rt.state.agenda_len(), 1);

    app.click("agenda-action-drift");
    assert_eq!(app.rt.state.page, Page::Rebalance);
}

#[test]
fn an_overview_without_a_policy_still_checks_the_prices() {
    let app = Harness::boot(false, Some(demo::POSITIONS));
    assert_eq!(app.rt.state.agenda_len(), 0);
    assert!(app.has_text("Every price is current."));
}

#[test]
fn the_theme_switches_and_persists() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    assert!(app.rt.state.is_dark());
    app.click("toggle-theme");
    assert!(!app.rt.state.is_dark());
    let config = fs::read_to_string(app.dir.path().join("config.yml")).unwrap();
    assert!(config.contains("light"), "saved: {config}");
}

#[test]
fn the_palette_filters_runs_commands_and_finds_holdings() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('k', false);
    assert!(app.exists("palette-panel"));
    let list = app
        .rt
        .presented("palette-list")
        .expect("the list is laid out")
        .0;
    let first = app
        .rt
        .presented("palette-0")
        .expect("the first row is laid out")
        .0;
    assert!(list[3] > 100.0, "the list has height, not {}", list[3]);
    assert!(
        first[1] + first[3] <= list[1] + list[3],
        "the first row shows inside the list"
    );

    app.type_text("perf");
    app.press(Key::Enter);
    assert!(app.rt.state.palette.is_none());
    assert_eq!(app.rt.state.page, Page::Performance);

    // Holdings are in there too: it opens the page with the position selected.
    app.command('k', false);
    app.type_text("bitcoin");
    app.press(Key::Enter);
    assert_eq!(app.rt.state.page, Page::Holdings);
    assert_eq!(app.rt.state.selected, Some(0));

    app.command('k', false);
    app.press(Key::Escape);
    assert!(app.rt.state.palette.is_none());
}

#[test]
fn a_generated_document_previews_without_writing_anything() {
    let mut app = Harness::boot(true, None);
    app.command('8', false);
    let reports = app.dir.path().join("portfolio/portfolio/reports");

    app.click("report-preview");
    assert!(
        app.exists("document-preview"),
        "{:?}",
        app.rt.state.documents.error
    );
    assert!(!reports.exists(), "a preview writes nothing");

    app.click("report-save");
    assert_eq!(fs::read_dir(&reports).unwrap().count(), 1);
}

#[test]
fn quitting_with_unsaved_changes_asks() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('n', false);
    app.click("form-submit");
    assert!(app.rt.state.dirty);

    app.command('q', false);
    assert_eq!(app.rt.state.confirm, Some(Confirm::Quit));
    assert!(app.exists("quit-save"));
    app.click("confirm-cancel");
    assert_eq!(app.rt.state.confirm, None);
}

#[test]
fn no_portfolio_lands_on_the_welcome_screen() {
    let mut app = Harness::boot(false, None);
    assert!(app.has_text("Track what you own"), "says what this is");
    assert!(app.exists("welcome-workspace"));
    assert!(app.exists("welcome-file"));
    assert!(!app.exists(Page::Overview.key()));

    app.click("welcome-create");
    assert!(app.exists("workspace-create"));
}

/// A workspace with nothing in it, open as it is right after creating one.
fn new_workspace() -> Harness {
    let mut app = Harness::boot(false, None);
    let location = app.dir.path().to_path_buf();
    app.open_new_workspace_in(&location);
    app.type_text("My Portfolio");
    app.click("workspace-create");
    assert!(app.rt.state.is_workspace(), "{}", app.problem());
    app
}

#[test]
fn every_empty_page_says_what_belongs_there() {
    let mut app = new_workspace();
    assert_eq!(app.rt.state.page, Page::Overview);
    assert!(app.has_text("Your portfolio is empty"));
    assert!(app.exists("first-steps-add"));
    assert!(!app.has_text("€0.00"), "no zeros standing in for a value");

    for (page, wanted, action) in [
        (Page::Holdings, "No positions yet", "holdings-empty-add"),
        (
            Page::Allocation,
            "Nothing to split yet",
            "allocation-empty-add",
        ),
        (Page::Performance, "No history yet", "performance-empty-add"),
        (
            Page::Briefing,
            "Nothing to summarize yet",
            "briefing-empty-add",
        ),
        (Page::Review, "No policy yet", "review-set-policy"),
        (Page::Rebalance, "No policy yet", "rebalance-set-policy"),
    ] {
        app.click(page.key());
        assert_eq!(app.rt.state.page, page);
        assert!(app.has_text(wanted), "{page:?} should show {wanted:?}");
        assert!(app.exists(action), "{page:?} should offer {action}");
        assert!(
            !app.has_text("no investment policy"),
            "{page:?} shows a raw error"
        );
    }
}

#[test]
fn the_first_steps_lead_to_a_first_position_and_a_policy() {
    let mut app = new_workspace();
    app.click("first-steps-add");
    assert!(app.rt.state.form.is_some());
    app.type_text("Apple");
    app.click("form-submit");
    assert_eq!(app.rt.state.positions.len(), 1);
    assert!(
        !app.exists("first-steps"),
        "done with once there's a position"
    );
    assert!(app.exists("hero"));

    app.click(Page::Review.key());
    app.click("review-set-policy");
    assert_eq!(app.rt.state.page, Page::Policy);
    app.click("strategy-balanced-growth");
    assert!(
        app.has_text("Press Save to Workspace"),
        "says it isn't kept yet"
    );
    app.click("policy-save");
    assert!(app.rt.state.has_policy, "{:?}", app.rt.state.policy.error);

    app.click(Page::Review.key());
    assert!(
        app.has_text("Allocation vs target"),
        "{:?}",
        app.rt.state.review.error
    );
}

#[test]
fn the_guide_opens_from_anywhere_and_never_on_its_own() {
    let mut app = Harness::boot(false, None);
    assert!(!app.rt.state.guide, "not forced on a first run");
    app.click("welcome-guide");
    assert!(app.exists("guide-body"));
    assert!(app.has_text("Valuation history"), "explains the terms");
    assert!(app.has_text("Getting started"));
    app.press(Key::Escape);
    assert!(!app.rt.state.guide);

    let mut app = Harness::boot(true, None);
    assert!(!app.rt.state.guide, "not forced on a returning user");
    app.command('2', false);
    assert!(
        app.has_text("one row per position"),
        "the page says what it is"
    );

    app.command('/', false);
    assert!(app.rt.state.guide);
    assert!(app.has_text("You're on Holdings"), "starts where you are");
    app.command('/', false);
    assert!(!app.rt.state.guide);

    app.click("guide");
    assert!(app.rt.state.guide);
    app.press(Key::Escape);

    app.command('k', false);
    app.type_text("guide");
    app.press(Key::Enter);
    assert!(app.rt.state.guide, "the palette reaches it");
}

#[test]
fn the_end_of_the_guide_can_be_scrolled_to_in_the_smallest_window() {
    offline();
    let tokio = Tokio::new().unwrap();
    let dir = TempDir::new().unwrap();
    let backend = demo::backend(dir.path(), None, tokio.handle().clone());
    let mut rt = app::headless(backend, None, (960.0, 640.0));
    rt.state.toggle_guide();
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let body = rt.presented("guide-body").expect("the guide is laid out").0;
    assert!(
        body[1] >= 0.0 && body[1] + body[3] <= 640.0,
        "the guide fits the window: {body:?}"
    );
    let last = "guide-shortcut-Hide amounts";
    let (x, y) = rt.find("guide-body").unwrap();
    for _ in 0..40 {
        rt.wheel(x, y, -200.0);
        rt.frame(1.0 / 60.0);
    }
    for _ in 0..60 {
        rt.frame(1.0 / 60.0);
    }
    let row = rt.presented(last).expect("the last row is laid out").0;
    assert!(
        row[1] + row[3] <= body[1] + body[3] + 1.0,
        "the last row scrolls into view: {row:?} in {body:?}"
    );
}

#[test]
fn a_german_keyboard_reaches_the_guide_too() {
    let mut app = Harness::boot(true, None);
    app.command('/', true);
    assert!(app.rt.state.guide, "Shift is how some layouts type a slash");
}

#[test]
fn every_page_has_its_own_explanation() {
    let pages: Vec<Page> = Page::PORTFOLIO
        .into_iter()
        .chain(Page::PLAN)
        .chain(Page::INSIGHTS)
        .chain([Page::Settings])
        .collect();
    let about: Vec<&str> = pages.iter().map(|page| guide::about(*page)).collect();
    let mut unique = about.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), pages.len(), "{about:?}");
}

#[test]
fn a_search_with_no_results_is_not_mistaken_for_an_empty_portfolio() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('2', false);
    app.click("holdings-search");
    app.type_text("nothing like this");
    assert!(app.has_text("No holdings match."));
    assert!(!app.exists("holdings-empty"));
}

#[test]
fn a_new_position_defaults_to_the_class_the_templates_use() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('n', false);
    assert_eq!(
        app.rt.state.form.as_ref().unwrap().asset_class.text,
        "Stocks"
    );
}

#[test]
fn a_chart_follows_the_pointer() {
    let mut app = Harness::boot(true, None);
    let (x, y) = app.rt.find("overview-chart").unwrap();
    app.rt.pointer_move(x, y);
    thread::sleep(Duration::from_millis(1));
    app.settle();
    assert!(
        app.exists("chart-readout"),
        "hovering shows the value at that date"
    );
}

#[test]
fn the_same_position_and_class_fly_between_pages_without_clashing() {
    let mut app = Harness::boot(true, None);
    assert!(app.exists("mover-0"));
    assert!(app.exists("legend-0"));
    app.click("mover-0");
    assert_eq!(app.rt.state.page, Page::Holdings);
    assert!(app.exists("holding-0"));
    assert!(!app.exists("mover-0"));
    app.click(Page::Allocation.key());
    assert!(app.exists("allocation-0"));
    assert!(!app.exists("legend-0"));
}

#[test]
fn a_foreign_holding_without_an_fx_rate_still_shows_its_quote() {
    let quotes = r#"{"latest": {"BTC-USD": {"price": 60000.0, "currency": "USD"}}}"#;
    let mut app = Harness::boot_with_quotes(BITCOIN, quotes);
    app.command('2', false);
    app.click("holding-0");

    assert!(app.has_text("$60,000.00"), "the price");
    assert!(app.has_text("$50,000.00"), "the average cost");
    assert!(!app.has_text("Price (EUR)"), "nothing to convert with");
}

#[test]
fn a_holding_nobody_has_priced_shows_no_currency() {
    let mut app = Harness::boot_with_quotes(BITCOIN, r#"{"latest": {}}"#);
    app.command('2', false);
    app.click("holding-0");

    // The purchase price was recorded in BTC-USD's quote currency, which isn't known here.
    assert!(app.has_text("0.45 @ 50,000.00"), "the purchase");
    assert!(!app.has_text("€50,000.00"));
    assert!(!app.has_text("$50,000.00"));
}

#[test]
fn a_minor_unit_quote_is_not_read_as_its_major_unit() {
    let positions =
        r#"[{"Name": "Vodafone", "AssetClass": "Stocks", "Ticker": "VOD.L", "Amount": 10.0}]"#;
    let quotes = r#"{"latest": {
        "VOD.L": {"price": 7500.0, "currency": "GBp"},
        "EURGBP=X": {"price": 0.8, "currency": "GBP"}
    }}"#;
    let mut app = Harness::boot_with_quotes(positions, quotes);
    app.command('2', false);
    app.click("holding-0");

    assert!(app.has_text("GBp\u{a0}7,500.00"), "the price, in pence");
    // 7500p is £75, and 1 GBP is €1.25.
    assert!(app.has_text("€93.75"), "the price in the base currency");
    assert!(app.has_text("€937.50"), "the value");
}

#[test]
fn a_holding_in_the_base_currency_has_one_price() {
    let positions = r#"[
      {"Name": "Fund", "AssetClass": "Stocks", "Ticker": "FUND", "Amount": 2.0,
       "Purchases": [{"Date": "2025/05/28", "Quantity": 2.0, "Price": 90.0}]},
      {"Name": "Flat", "AssetClass": "Real Estate", "Amount": 500.0,
       "Purchases": [{"Date": "2025/05/28", "Quantity": 5.0, "Price": 80.0}]}
    ]"#;
    let quotes = r#"{"latest": {"FUND": {"price": 100.0, "currency": "EUR"}}}"#;
    let mut app = Harness::boot_with_quotes(positions, quotes);
    app.command('2', false);

    app.click("holding-0");
    assert!(app.has_text("€100.00"), "the price");
    assert!(app.has_text("2 @ €90.00"), "the purchase");
    assert!(!app.has_text("Price (EUR)"));

    // Without a ticker, what was entered is in the base currency.
    app.click("holding-1");
    assert!(app.has_text("5 @ €80.00"), "the purchase");
}

#[test]
fn privacy_mode_hides_a_foreign_holdings_prices() {
    let mut app = Harness::boot_with_quotes(BITCOIN, DOLLAR_QUOTES);
    app.command('2', false);
    app.click("holding-0");
    assert!(app.has_text("$60,000.00"));

    app.click("privacy");
    for shown in ["$60,000.00", "$50,000.00", "€48,000.00"] {
        assert!(!app.has_text(shown), "{shown}");
    }
}

#[test]
fn the_form_only_names_a_currency_it_knows() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('2', false);
    app.click("add-position");
    assert!(app.has_text("Price (EUR)"));

    app.click("form-ticker");
    app.type_text("BTC-USD");
    assert!(app.has_text("Price (quote currency)"));
    assert!(!app.has_text("Price (EUR)"));
}

/// Position ids top to bottom: biggest value first. S&P 500, Savings, Bitcoin, Long Bonds, Gold.
const BY_VALUE: [usize; 5] = [1, 4, 0, 2, 3];

fn on_holdings() -> Harness {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('2', false);
    app
}

#[test]
fn arrow_keys_move_the_selection_after_a_click() {
    let mut app = on_holdings();
    app.click(&format!("holding-{}", BY_VALUE[0]));
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[0]));

    for id in &BY_VALUE[1..] {
        app.press(Key::Down);
        assert_eq!(app.rt.state.selected, Some(*id));
        assert!(app.exists("edit-position"), "the details follow the row");
    }
    app.press(Key::Down);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[4]), "stops at the end");
    app.press(Key::End);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[4]), "End stays too");
    app.press(Key::Up);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[3]));
    app.press(Key::Home);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[0]));
    app.press(Key::Up);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[0]), "stops at the top");
    app.press(Key::Text("s".into()));
    assert_eq!(
        app.rt.state.selected,
        Some(BY_VALUE[0]),
        "letters do nothing"
    );
}

#[test]
fn arrow_keys_move_the_selection_with_nothing_focused() {
    let mut app = on_holdings();
    app.press(Key::Down);
    assert_eq!(
        app.rt.state.selected,
        Some(BY_VALUE[0]),
        "starts at the top"
    );
    app.press(Key::Down);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[1]));
    app.press(Key::Up);
    app.press(Key::Up);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[0]), "stops at the top");
    assert!(app.exists("edit-position"));
}

#[test]
fn arrow_keys_follow_a_selection_from_elsewhere_without_toggling_it() {
    let mut app = on_holdings();
    app.click(&format!("holding-{}", BY_VALUE[0]));
    // Selected from the palette, while the first row still has focus.
    app.command('k', false);
    app.type_text("savings");
    app.press(Key::Enter);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[1]));
    app.press(Key::Down);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[2]));
}

#[test]
fn arrow_keys_follow_the_sort_and_the_search() {
    let mut app = on_holdings();
    app.click("sort-name");
    // A to Z: Bitcoin, Gold, Long Bonds, S&P 500, Savings.
    app.click("holding-0");
    app.press(Key::Down);
    assert_eq!(app.rt.state.selected, Some(3));

    app.click("holdings-search");
    app.type_text("o");
    // The arrows stay in the field.
    let selected = app.rt.state.selected;
    app.press(Key::Down);
    app.press(Key::Up);
    assert_eq!(app.rt.state.selected, selected);
    assert_eq!(app.rt.state.search.text, "o");
}

#[test]
fn arrow_keys_leave_other_controls_and_pages_alone() {
    let mut app = on_holdings();
    app.command('k', false);
    app.press(Key::Down);
    assert!(
        app.rt.state.selected.is_none(),
        "the palette has the arrows"
    );
    app.press(Key::Escape);

    app.command('n', false);
    app.press(Key::Down);
    assert!(app.rt.state.selected.is_none(), "the form has the arrows");
    app.press(Key::Escape);

    app.command('1', false);
    app.press(Key::Down);
    assert!(
        app.rt.state.selected.is_none(),
        "only Holdings has a selection"
    );
}

#[test]
fn return_edits_the_selected_holding_and_escape_comes_back() {
    let mut app = on_holdings();
    app.click(&format!("holding-{}", BY_VALUE[2]));
    app.press(Key::Enter);
    assert!(
        app.rt.state.form.is_some(),
        "Return on a focused row edits it"
    );
    assert_eq!(
        app.rt.state.selected,
        Some(BY_VALUE[2]),
        "and keeps it selected"
    );

    app.press(Key::Escape);
    assert!(app.rt.state.form.is_none());
    app.press(Key::Down);
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[3]));
}

#[test]
fn return_edits_a_selection_with_nothing_focused() {
    let mut app = on_holdings();
    app.press(Key::Enter);
    assert!(app.rt.state.form.is_none(), "no selection, nothing to edit");
    app.press(Key::Down);
    app.press(Key::Enter);
    assert!(app.rt.state.form.is_some());
}

#[test]
fn tab_stops_once_in_the_table_and_space_still_toggles() {
    let mut app = on_holdings();
    app.click(&format!("holding-{}", BY_VALUE[1]));
    app.press(Key::Text(" ".into()));
    assert!(
        app.rt.state.selected.is_none(),
        "Space presses the focused row"
    );
    app.press(Key::Text(" ".into()));
    assert_eq!(app.rt.state.selected, Some(BY_VALUE[1]));
    app.press(Key::Tab);
    app.press(Key::Down);
    assert_eq!(
        app.rt.state.selected,
        Some(BY_VALUE[1]),
        "Tab left the table, so the arrows no longer move it"
    );
}

fn narrowed_to(app: &mut Harness, width: f32, height: f32) {
    app.rt.set_viewport((width, height), 2.0);
    app.settle();
}

fn columns_of(app: &Harness) -> Vec<&'static str> {
    [
        "sort-name",
        "sort-class",
        "sort-value",
        "sort-pnl",
        "sort-day",
    ]
    .into_iter()
    .filter(|key| app.exists(key))
    .collect()
}

#[test]
fn the_table_drops_columns_rather_than_overlap_them() {
    let mut app = on_holdings();
    assert_eq!(columns_of(&app).len(), 5, "a wide window has them all");

    // The smallest window Stahl allows, with the pane open beside the table.
    narrowed_to(&mut app, 960.0, 640.0);
    assert_eq!(columns_of(&app).len(), 5, "no pane yet");
    app.click("holding-0");
    assert_eq!(columns_of(&app), ["sort-name", "sort-value", "sort-day"]);
    assert!(app.exists("edit-position"), "the details show beside it");
    assert!(app.exists("add-position"));
    let add = app.rt.presented("add-position").unwrap().0;
    let pane = app.rt.presented("inspector").unwrap().0;
    assert!(
        add[0] + add[2] <= pane[0],
        "the Add button stays clear of the pane"
    );
}

#[test]
fn a_window_too_narrow_for_both_shows_the_details_in_place_of_the_table() {
    let mut app = on_holdings();
    narrowed_to(&mut app, 646.0, 450.0);
    assert_eq!(columns_of(&app), ["sort-name", "sort-value", "sort-day"]);

    app.click("holding-1");
    assert!(!app.exists("holding-1"), "the details replace the table");
    assert!(app.exists("edit-position"));

    // The arrows still work, and Edit is still a key away.
    app.press(Key::Down);
    assert_eq!(app.rt.state.selected, Some(4));
    app.press(Key::Enter);
    assert!(app.rt.state.form.is_some());
    app.press(Key::Escape);

    app.click("close-inspector");
    assert!(app.rt.state.selected.is_none());
    assert!(
        app.exists("holding-1"),
        "closing them brings the table back"
    );

    // With the sidebar out of the way, both fit.
    app.click("holding-1");
    app.command('b', true);
    assert!(app.exists("holding-1"));
    assert!(app.exists("edit-position"));
    assert_eq!(columns_of(&app), ["sort-name", "sort-value"]);
}

#[test]
fn collapsing_the_sidebar_hides_every_row_on_every_page() {
    // What stays is the toolbar's title for the open page, which isn't part of the sidebar.
    let mut app = Harness::boot(true, None);
    let rows = [
        "nav-overview",
        "nav-holdings",
        "nav-allocation",
        "nav-performance",
        "nav-review",
        "nav-rebalance",
        "nav-policy",
        "nav-documents",
        "nav-briefing",
        "nav-health",
        "nav-settings",
    ];
    for key in rows {
        app.click(key);
        app.click("sidebar-toggle");
        assert!(!app.rt.state.sidebar.is_open());
        for row in rows {
            let (_, opacity) = app.rt.presented(row).expect("the row stays laid out");
            assert_eq!(opacity, 0.0, "{row} shows with {key} open");
        }
        app.click("sidebar-toggle");
    }
}

// --- Updates. A fake release source and temp files: no network, and the running binary is safe. ---

const ZIP: &str = "stahl-capital-windows-x64.zip";

fn windows_zip(exe: &[u8]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .start_file("stahl.exe", SimpleFileOptions::default())
        .unwrap();
    writer.write_all(exe).unwrap();
    writer.finish().unwrap().into_inner()
}

/// A release with an installer, and a checksum for `published`, which may not be what's served.
fn windows_release(served: &[u8], published: &[u8]) -> Arc<FakeReleases> {
    Arc::new(FakeReleases::offering(
        demo::next_version(),
        vec![
            (ZIP.to_string(), served.to_vec()),
            (
                format!("{ZIP}.sha256"),
                demo::checksum_line(ZIP, published).into_bytes(),
            ),
        ],
    ))
}

/// A throwaway "installed" copy of the app, standing in for the real one.
struct FakeInstall {
    _dir: TempDir,
    exe: PathBuf,
    downloads: PathBuf,
    relaunches: Arc<AtomicUsize>,
}

impl FakeInstall {
    fn new() -> FakeInstall {
        let dir = TempDir::new().unwrap();
        let exe = dir.path().join("stahl.exe");
        fs::write(&exe, b"MZ old").unwrap();
        let downloads = dir.path().join("Downloads");
        fs::create_dir(&downloads).unwrap();
        FakeInstall {
            _dir: dir,
            exe,
            downloads,
            relaunches: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn windows(&self, source: Arc<FakeReleases>) -> Updater {
        self.updater(
            source,
            Installation::WindowsExe {
                exe: self.exe.clone(),
            },
        )
    }

    fn updater(&self, source: Arc<FakeReleases>, installation: Installation) -> Updater {
        let relaunches = self.relaunches.clone();
        Updater::new(
            source,
            installation,
            update::current_version(),
            self.downloads.clone(),
        )
        .relaunch_with(Arc::new(move |_| {
            relaunches.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }))
    }

    fn exe_contents(&self) -> Vec<u8> {
        fs::read(&self.exe).unwrap()
    }

    fn relaunches(&self) -> usize {
        self.relaunches.load(Ordering::SeqCst)
    }
}

#[test]
fn startup_mentions_a_new_version_and_changes_nothing_until_asked() {
    let install = FakeInstall::new();
    let source = windows_release(&windows_zip(b"MZ new"), &windows_zip(b"MZ new"));
    let mut app = Harness::boot_with(
        false,
        Some(demo::POSITIONS),
        install.windows(source.clone()),
    );

    let next = demo::next_version();
    assert!(app.exists("update-banner"));
    assert!(app.has_text(&format!("Stahl Capital {next} is available")));
    assert!(!app.exists("update-install"), "installing takes a decision");
    assert_eq!(source.downloads(), 0, "and nothing is fetched before one");
    assert_eq!(install.exe_contents(), b"MZ old");

    app.click("update-view");
    assert!(app.has_text("What's Changed"), "the dialog has the notes");
    assert_eq!(source.downloads(), 0, "reading about it fetches nothing");

    app.press(Key::Escape);
    assert!(!app.rt.state.updates.dialog);
    assert!(app.exists("update-banner"), "closing isn't dismissing");
    assert_eq!(install.exe_contents(), b"MZ old");
}

#[test]
fn a_dismissed_update_stays_away_until_asked_for() {
    let install = FakeInstall::new();
    let source = windows_release(b"", b"");
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    app.click("update-dismiss");
    assert!(!app.exists("update-banner"));
    assert!(!app.exists("update-dialog"));

    app.rt.state.navigate(Page::Holdings);
    app.settle();
    assert!(!app.exists("update-banner"), "it stays dismissed");

    app.rt.state.check_for_updates(false);
    app.settle();
    assert!(app.exists("update-install"), "asking brings it back");
}

#[test]
fn the_banner_also_shows_without_a_portfolio() {
    let install = FakeInstall::new();
    let source = windows_release(b"", b"");
    let app = Harness::boot_with(false, None, install.windows(source));
    assert!(app.exists("welcome-workspace"));
    assert!(app.exists("update-banner"));
}

#[test]
fn a_failed_check_at_startup_is_silent_and_a_requested_one_says_so() {
    let install = FakeInstall::new();
    let mut app = Harness::boot_with(
        false,
        Some(demo::POSITIONS),
        install.windows(Arc::new(FakeReleases::failing())),
    );
    assert!(!app.exists("update-banner"));
    assert!(app.rt.state.toast.is_none());
    assert!(app.rt.state.error.is_none());
    assert!(!app.rt.state.updates.checking);
    assert_eq!(app.rt.state.positions.len(), 5, "and the app works");

    app.rt.state.check_for_updates(false);
    app.settle();
    assert!(app.has_text("Couldn't check for updates"));
    assert!(!app.exists("update-banner"));
    assert!(app.rt.state.error.is_none());
}

#[test]
fn being_up_to_date_is_only_mentioned_when_asked() {
    let install = FakeInstall::new();
    let current = update::current_version();
    let source = Arc::new(FakeReleases::offering(current.clone(), Vec::new()));
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    assert!(!app.exists("update-banner"), "the same version isn't news");
    assert!(app.rt.state.toast.is_none());

    app.rt.state.check_for_updates(false);
    app.settle();
    assert!(app.has_text(&format!("You're up to date ({current})")));
}

#[test]
fn the_palette_and_settings_can_check_for_updates() {
    let install = FakeInstall::new();
    let source = windows_release(b"", b"");
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    app.click("update-dismiss");

    app.command('k', false);
    app.type_text("check for updates");
    app.press(Key::Enter);
    assert!(app.rt.state.palette.is_none());
    assert!(app.exists("update-install"), "the dialog opens");
    app.click("update-later");

    app.command(',', false);
    assert!(app.exists("settings-updates"));
    assert!(app.has_text("Updates replace stahl.exe"));
    // It's further down the page.
    app.rt.wheel(800.0, 400.0, -2000.0);
    app.settle();
    app.click("settings-check-updates");
    assert!(app.exists("update-install"));
}

#[test]
fn installing_swaps_the_app_and_restarts_only_when_told_to() {
    let install = FakeInstall::new();
    let archive = windows_zip(b"MZ new");
    let source = windows_release(&archive, &archive);
    let mut app = Harness::boot_with(
        false,
        Some(demo::POSITIONS),
        install.windows(source.clone()),
    );
    app.click("update-view");
    app.click("update-install");

    assert_eq!(app.rt.state.updates.phase, UpdatePhase::Installed);
    assert_eq!(install.exe_contents(), b"MZ new");
    assert_eq!(source.downloads(), 2, "the installer and its checksum");
    assert_eq!(install.relaunches(), 0, "installing doesn't restart");
    assert!(!app.rt.state.is_quitting());

    // Later leaves it installed, and it's still reachable.
    app.click("update-later");
    assert_eq!(install.relaunches(), 0);
    assert_eq!(install.exe_contents(), b"MZ new");
    app.command('k', false);
    app.type_text("check for updates");
    app.press(Key::Enter);
    assert_eq!(
        app.rt.state.updates.phase,
        UpdatePhase::Installed,
        "checking again doesn't lose it"
    );

    app.click("update-restart");
    assert_eq!(install.relaunches(), 1);
    assert!(app.rt.state.is_quitting());
}

#[test]
fn restarting_saves_first_and_a_failed_save_stays_open() {
    let install = FakeInstall::new();
    let archive = windows_zip(b"MZ new");
    let source = windows_release(&archive, &archive);
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    app.click("update-dismiss");
    app.command('n', false);
    app.type_text("Apple");
    app.click("form-submit");
    assert!(app.rt.state.dirty);

    app.rt.state.check_for_updates(false);
    app.settle();
    app.click("update-install");
    assert!(app.has_text("Save and Restart"), "it says what will happen");

    // A save that can't work: the portfolio's file is now a directory.
    let path = app.dir.path().join("portfolio.json");
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    app.click("update-restart");
    assert_eq!(install.relaunches(), 0, "no restart without the save");
    assert!(!app.rt.state.is_quitting());
    assert!(app.rt.state.dirty);
    assert!(app.has_text("Couldn't save your changes"));

    fs::remove_dir(&path).unwrap();
    app.click("update-restart");
    assert!(fs::read_to_string(&path).unwrap().contains("Apple"));
    assert_eq!(install.relaunches(), 1);
    assert!(app.rt.state.is_quitting());
}

#[test]
fn pressing_save_and_restart_twice_restarts_once() {
    let install = FakeInstall::new();
    let archive = windows_zip(b"MZ new");
    let source = windows_release(&archive, &archive);
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    app.click("update-dismiss");
    app.command('n', false);
    app.type_text("Apple");
    app.click("form-submit");
    app.rt.state.check_for_updates(false);
    app.settle();
    app.click("update-install");

    app.rt.state.restart_after_update();
    app.rt.state.restart_after_update();
    app.settle();
    assert_eq!(install.relaunches(), 1);
}

#[test]
fn asking_during_the_startup_check_gets_its_answer() {
    let install = FakeInstall::new();
    let source = Arc::new(FakeReleases::offering(
        update::current_version(),
        Vec::new(),
    ));
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    app.rt.state.check_for_updates(true);
    app.rt.state.check_for_updates(false);
    app.settle();
    assert!(app.has_text("You're up to date"));
}

#[test]
fn a_release_that_was_pulled_is_no_longer_offered() {
    let install = FakeInstall::new();
    let archive = windows_zip(b"MZ new");
    let source = windows_release(&archive, &archive);
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    assert!(app.rt.state.updates.available.is_some());

    let empty = Arc::new(FakeReleases::offering(
        update::current_version(),
        Vec::new(),
    ));
    app.rt.state.updates.updater = install.windows(empty);
    app.rt.state.check_for_updates(false);
    app.settle();
    assert!(app.rt.state.updates.available.is_none());
    assert!(!app.exists("update-banner"));
    assert!(app.has_text("You're up to date"));
}

#[test]
fn quitting_waits_for_an_update_that_is_installing() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.rt.state.updates.phase = UpdatePhase::Installing;
    app.command('q', false);
    assert!(!app.rt.state.is_quitting());
    assert!(app.has_text("An update is installing"));
}

#[test]
fn a_download_that_fails_its_checksum_is_never_installed() {
    let install = FakeInstall::new();
    let source = windows_release(&windows_zip(b"MZ evil"), &windows_zip(b"MZ new"));
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    app.click("update-view");
    app.click("update-install");

    assert!(matches!(
        &app.rt.state.updates.phase,
        UpdatePhase::Failed(error) if error.contains("doesn't match")
    ));
    assert_eq!(install.exe_contents(), b"MZ old");
    assert!(app.has_text("Nothing was changed"));
    assert!(!app.exists("update-restart"));
    assert!(app.exists("update-page"), "there's still a way forward");
    assert_eq!(install.relaunches(), 0);
}

#[test]
fn a_failure_after_verifying_leaves_the_old_copy_working_and_can_be_retried() {
    let install = FakeInstall::new();
    // A correctly checksummed archive holding the wrong thing.
    let archive = windows_zip(b"#!/bin/sh");
    let source = windows_release(&archive, &archive);
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    app.click("update-view");
    app.click("update-install");
    assert!(matches!(app.rt.state.updates.phase, UpdatePhase::Failed(_)));
    assert_eq!(install.exe_contents(), b"MZ old");

    app.click("update-retry");
    assert!(matches!(app.rt.state.updates.phase, UpdatePhase::Failed(_)));
    assert_eq!(install.exe_contents(), b"MZ old");
}

#[test]
fn a_release_without_a_checksum_only_gets_a_link() {
    let install = FakeInstall::new();
    let source = Arc::new(FakeReleases::offering(
        demo::next_version(),
        vec![(ZIP.to_string(), windows_zip(b"MZ new"))],
    ));
    let mut app = Harness::boot_with(
        false,
        Some(demo::POSITIONS),
        install.windows(source.clone()),
    );
    app.click("update-view");
    assert!(!app.exists("update-install"));
    assert!(app.has_text("can't be verified"));

    // Even a stray call can't get around the dialog.
    app.rt.state.install_update();
    app.settle();
    assert!(matches!(app.rt.state.updates.phase, UpdatePhase::Failed(_)));
    assert_eq!(source.downloads(), 0);
    assert_eq!(install.exe_contents(), b"MZ old");
}

#[test]
fn nix_gets_told_about_updates_but_is_never_touched() {
    let install = FakeInstall::new();
    let archive = windows_zip(b"MZ new");
    let source = windows_release(&archive, &archive);
    let mut app = Harness::boot_with(
        false,
        Some(demo::POSITIONS),
        install.updater(source.clone(), Installation::Nix),
    );
    assert!(app.exists("update-banner"));
    app.click("update-view");
    assert!(!app.exists("update-install"));
    assert!(app.has_text("Nix manages this copy"));

    app.click("update-page");
    let opened = app.rt.take_url_requests();
    assert_eq!(opened.len(), 1);
    assert!(
        opened[0].starts_with(update::RELEASES_PAGE),
        "{}",
        opened[0]
    );

    app.rt.state.install_update();
    app.settle();
    assert_eq!(source.downloads(), 0, "a stray call fetches nothing");
    assert_eq!(install.relaunches(), 0);
}

#[test]
fn a_deb_is_verified_and_saved_never_installed() {
    let install = FakeInstall::new();
    let deb = "stahl_9.9.9_amd64.deb";
    let source = Arc::new(FakeReleases::offering(
        demo::next_version(),
        vec![
            (deb.to_string(), b"package".to_vec()),
            (
                format!("{deb}.sha256"),
                demo::checksum_line(deb, b"package").into_bytes(),
            ),
        ],
    ));
    let mut app = Harness::boot_with(
        false,
        Some(demo::POSITIONS),
        install.updater(source, Installation::Deb),
    );
    app.click("update-view");
    assert!(app.has_text("Download Package"));
    app.click("update-install");

    let saved = install.downloads.join(deb);
    assert_eq!(
        app.rt.state.updates.phase,
        UpdatePhase::Saved(saved.clone())
    );
    assert_eq!(fs::read(&saved).unwrap(), b"package");
    assert!(app.has_text("sudo apt install"));
    assert!(!app.exists("update-restart"), "dpkg's job, not ours");
    assert_eq!(install.relaunches(), 0);
    assert_eq!(install.exe_contents(), b"MZ old");
}

#[test]
fn a_tampered_deb_is_not_saved() {
    let install = FakeInstall::new();
    let deb = "stahl_9.9.9_amd64.deb";
    let source = Arc::new(FakeReleases::offering(
        demo::next_version(),
        vec![
            (deb.to_string(), b"evil".to_vec()),
            (
                format!("{deb}.sha256"),
                demo::checksum_line(deb, b"package").into_bytes(),
            ),
        ],
    ));
    let mut app = Harness::boot_with(
        false,
        Some(demo::POSITIONS),
        install.updater(source, Installation::Deb),
    );
    app.click("update-view");
    app.click("update-install");
    assert!(matches!(app.rt.state.updates.phase, UpdatePhase::Failed(_)));
    assert_eq!(fs::read_dir(&install.downloads).unwrap().count(), 0);
}

#[test]
fn the_dialog_cannot_be_closed_mid_install() {
    let install = FakeInstall::new();
    let source = windows_release(b"", b"");
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), install.windows(source));
    app.click("update-view");
    app.rt.state.updates.phase = UpdatePhase::Installing;
    app.press(Key::Escape);
    assert!(app.rt.state.updates.dialog);
    app.rt.state.dismiss_update();
    assert!(app.rt.state.updates.dialog);
    assert!(!app.rt.state.updates.dismissed);
}

#[test]
fn the_demos_release_can_never_install_anything() {
    let install = FakeInstall::new();
    let updater = demo::updater(Installation::WindowsExe {
        exe: install.exe.clone(),
    });
    let mut app = Harness::boot_with(false, Some(demo::POSITIONS), updater);
    app.click("update-view");
    app.click("update-install");
    assert!(matches!(app.rt.state.updates.phase, UpdatePhase::Failed(_)));
    assert_eq!(install.exe_contents(), b"MZ old");
}

#[test]
fn the_default_updater_never_checks_anything() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    assert!(!app.rt.state.updates.updater.is_enabled());
    assert!(!app.rt.state.updates.checking);
    app.command('k', false);
    app.type_text("check for updates");
    app.press(Key::Enter);
    assert!(app.has_text("Updates are turned off"));
}

impl Harness {
    /// The form's location, set to a folder in this test's temp directory.
    fn open_new_workspace_in(&mut self, location: &Path) {
        self.command('n', true);
        assert!(self.exists("workspace-create"), "the form opens");
        self.rt
            .state
            .new_workspace
            .as_mut()
            .unwrap()
            .location
            .replace_text(location.to_string_lossy().as_ref());
        self.settle();
    }

    fn problem(&self) -> String {
        let form = self.rt.state.new_workspace.as_ref();
        let problem = form.and_then(|form| form.problem.as_ref());
        problem.map_or_else(String::new, |problem| problem.message.clone())
    }

    fn saved_config(&self) -> String {
        fs::read_to_string(self.dir.path().join("config.yml")).unwrap_or_default()
    }
}

#[test]
fn a_new_user_creates_a_workspace_and_adds_a_first_position() {
    let mut app = Harness::boot(false, None);
    let location = app.dir.path().to_path_buf();
    app.open_new_workspace_in(&location);

    app.type_text("My Portfolio");
    assert!(app.has_text("Creates "), "says where it will go");
    app.click("workspace-create");

    let root = location.join("My Portfolio");
    assert!(app.rt.state.new_workspace.is_none(), "{}", app.problem());
    assert_eq!(
        app.rt.state.open.workspace_dir.as_deref(),
        Some(root.to_string_lossy().as_ref())
    );
    for file in [
        "positions.json",
        "INVESTMENT_POLICY.md",
        "AGENTS.md",
        ".gitignore",
        "portfolio/watchlist.json",
    ] {
        assert!(root.join(file).is_file(), "{file}");
    }
    for folder in ["diary", "decisions", "theses", "reports"] {
        assert!(root.join("portfolio").join(folder).is_dir(), "{folder}");
    }
    assert!(app.exists(Page::Review.key()), "workspace pages are open");
    assert!(
        app.saved_config().contains("My Portfolio"),
        "reopens next time: {}",
        app.saved_config()
    );

    app.command('n', false);
    app.type_text("Savings");
    app.rt
        .state
        .form
        .as_mut()
        .unwrap()
        .amount
        .replace_text("1000");
    app.rt
        .state
        .form
        .as_mut()
        .unwrap()
        .asset_class
        .replace_text("Cash");
    app.settle();
    app.click("form-submit");
    assert_eq!(app.rt.state.positions.len(), 1);
    app.command('s', false);
    let positions = fs::read_to_string(root.join("positions.json")).unwrap();
    assert!(positions.contains("Savings"), "{positions}");
}

#[test]
fn a_workspace_needs_a_name_and_a_place_that_can_hold_it() {
    let mut app = Harness::boot(false, None);
    let location = app.dir.path().to_path_buf();
    fs::create_dir(location.join("Taken")).unwrap();
    app.open_new_workspace_in(&location);

    app.click("workspace-create");
    assert_eq!(app.problem(), "Give the workspace a name.");
    assert!(app.exists("workspace-problem"), "the problem shows");

    app.click("workspace-name");
    app.type_text("a/b");
    app.click("workspace-create");
    assert!(app.problem().contains("can't contain"), "{}", app.problem());
    assert!(!location.join("a").exists());

    let form = app.rt.state.new_workspace.as_mut().unwrap();
    form.name.replace_text("Taken");
    app.settle();
    app.click("workspace-create");
    assert!(
        app.problem().contains("already exists"),
        "{}",
        app.problem()
    );
    assert!(!app.rt.state.is_workspace());

    let form = app.rt.state.new_workspace.as_mut().unwrap();
    form.name.replace_text("New");
    form.location
        .replace_text(location.join("nowhere").to_string_lossy().as_ref());
    app.settle();
    app.click("workspace-create");
    assert!(app.problem().contains("doesn't exist"), "{}", app.problem());

    let form = app.rt.state.new_workspace.as_mut().unwrap();
    form.location.clear();
    app.settle();
    app.click("workspace-create");
    assert!(app.problem().contains("Choose where"), "{}", app.problem());

    assert_eq!(app.rt.state.open.workspace_dir, None, "nothing was opened");
    app.click("workspace-cancel");
    assert!(app.rt.state.new_workspace.is_none());
    assert!(
        !app.saved_config().contains("workspace_dir: "),
        "nothing was remembered: {}",
        app.saved_config()
    );
}

#[cfg(unix)]
#[test]
fn a_workspace_in_a_folder_you_cannot_write_says_so() {
    let mut app = Harness::boot(false, None);
    let locked = app.dir.path().join("locked");
    fs::create_dir(&locked).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();
    // Root can write anywhere, so this can't be tested as root.
    let writable = fs::write(locked.join("probe"), "").is_ok();
    if !writable {
        app.open_new_workspace_in(&locked);
        app.type_text("New");
        app.click("workspace-create");
        assert!(app.problem().contains("can't write"), "{}", app.problem());
        assert!(app.rt.state.new_workspace.is_some(), "the form stays open");
    }
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn unsaved_changes_are_not_lost_to_a_new_workspace() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('n', false);
    app.click("form-submit");
    assert!(app.rt.state.dirty);

    let location = app.dir.path().to_path_buf();
    app.open_new_workspace_in(&location);
    app.type_text("New");
    app.click("workspace-create");
    assert!(
        matches!(
            app.rt.state.confirm,
            Some(Confirm::Unsaved(PendingOpen::CreateWorkspace))
        ),
        "the same save, discard and cancel question as quitting"
    );
    assert!(app.exists("unsaved-save"));
    app.click("confirm-cancel");
    assert_eq!(app.rt.state.confirm, None);
    assert!(!location.join("New").exists());
    assert_eq!(app.rt.state.positions.len(), 6);
    assert!(app.rt.state.dirty, "still there to save");
}

#[test]
fn a_new_workspace_can_bring_the_open_file_along() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    let location = app.dir.path().to_path_buf();
    app.open_new_workspace_in(&location);
    assert!(app.has_text("Bring my 5 positions from portfolio.json"));
    app.type_text("Kept");
    app.click("workspace-create");
    assert_eq!(app.rt.state.positions.len(), 5);
    assert!(app.rt.state.is_workspace());

    // From a workspace there's nothing to offer, and leaving the box unticked starts empty.
    app.open_new_workspace_in(&location);
    assert!(!app.exists("workspace-keep"));
    app.type_text("Fresh");
    app.click("workspace-create");
    assert!(app.rt.state.positions.is_empty());

    assert_eq!(
        fs::read_to_string(location.join("portfolio.json")).unwrap(),
        demo::POSITIONS,
        "the original is untouched"
    );
}

#[test]
fn ticking_off_the_open_file_starts_empty() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    let location = app.dir.path().to_path_buf();
    app.open_new_workspace_in(&location);
    app.type_text("Fresh");
    app.click("workspace-keep");
    app.click("workspace-create");
    assert!(app.rt.state.positions.is_empty());
}

#[test]
fn the_palette_reaches_a_new_workspace_and_offers_only_what_works() {
    let mut app = Harness::boot(false, None);
    app.command('k', false);
    assert!(app.has_text("New Workspace"));
    assert!(
        !app.has_text("Add Position"),
        "there's nothing to add to yet"
    );
    assert!(!app.has_text("Close"), "nothing open to close");
    assert!(!app.has_text("Holdings"), "no pages without a portfolio");
    app.type_text("new work");
    app.press(Key::Enter);
    assert!(app.rt.state.palette.is_none());
    assert!(app.exists("workspace-create"));
    app.press(Key::Escape);
    assert!(app.rt.state.new_workspace.is_none());
}

#[test]
fn opening_another_portfolio_forgets_the_last_ones_results() {
    let mut app = Harness::boot(true, None);
    app.command('5', false);
    assert!(app.rt.state.review.data.is_some());
    assert!(app.rt.state.has_policy);

    let location = app.dir.path().to_path_buf();
    app.open_new_workspace_in(&location);
    app.type_text("Next");
    app.click("workspace-create");
    assert!(!app.rt.state.has_policy, "the policy stayed behind");
    assert!(app.rt.state.review.data.is_none());
    assert_eq!(app.rt.state.page, Page::Overview);
}

fn edited(app: &mut Harness) {
    let before = app.rt.state.positions.len();
    app.command('n', false);
    app.type_text("Widget");
    app.click("form-submit");
    assert!(app.rt.state.dirty);
    assert_eq!(app.rt.state.positions.len(), before + 1);
}

#[test]
fn reopening_a_workspace_accepts_its_portfolio_folder() {
    let mut app = Harness::boot(true, None);
    let root = app.dir.path().join("portfolio");
    let inner = root.join("portfolio");
    assert!(inner.is_dir(), "history lives in the only subfolder");
    assert_eq!(app.rt.state.positions.len(), 5);

    app.command('o', true);
    let start = app
        .rt
        .file_dialog()
        .expect("the workspace dialog is open")
        .1
        .directory
        .clone();
    assert_eq!(app.rt.file_dialog().unwrap().0, FileDialogKind::Folder);
    assert_eq!(
        start.as_deref(),
        Some(app.dir.path()),
        "the dialog starts beside the workspace, not inside it"
    );

    // Select lands on portfolio/ when the chooser is already inside the workspace.
    app.rt
        .resolve_file_dialog(Some(inner.clone()))
        .expect("the dialog is answered");
    app.settle();
    assert_eq!(
        app.rt.state.open.workspace_dir.as_deref(),
        Some(root.to_str().unwrap())
    );
    assert!(app.rt.state.error.is_none(), "{:?}", app.rt.state.error);
    assert!(app.rt.state.folder_prompt.is_none());
    assert_eq!(app.rt.state.positions.len(), 5);
    assert!(!app.has_text("is not a workspace"));
}

#[test]
fn an_empty_folder_offers_a_way_forward_and_hides_the_open_portfolio() {
    let mut app = Harness::boot(true, None);
    let empty = app.dir.path().join("empty");
    fs::create_dir(&empty).unwrap();
    let root = app.dir.path().join("portfolio");

    app.rt
        .state
        .load_workspace(empty.to_string_lossy().into_owned());
    app.settle();
    assert!(app.exists("folder-start"));
    assert!(app.exists("folder-choose"));
    assert!(app.exists("folder-cancel"));
    assert!(
        !app.exists(Page::Overview.key()),
        "the previous holdings are not left looking loaded"
    );
    assert!(!app.has_text("Bitcoin"));
    assert_eq!(
        app.rt.state.open.workspace_dir.as_deref(),
        Some(root.to_str().unwrap()),
        "the open portfolio is kept until they decide"
    );

    app.click("folder-cancel");
    assert!(app.has_text("Bitcoin"));
    assert!(!app.exists("folder-start"));

    app.rt
        .state
        .load_workspace(empty.to_string_lossy().into_owned());
    app.settle();
    app.click("folder-start");
    assert!(app.rt.state.is_workspace(), "{:?}", app.rt.state.error);
    assert_eq!(
        app.rt.state.open.workspace_dir.as_deref(),
        Some(empty.to_str().unwrap())
    );
    assert!(empty.join("positions.json").is_file());
    assert!(empty.join("INVESTMENT_POLICY.md").is_file());
    assert!(app.rt.state.positions.is_empty());
    assert!(!app.has_text("Bitcoin"));
}

#[test]
fn a_portfolio_rs_folder_without_positions_opens() {
    let mut app = Harness::boot(false, None);
    let root = app.dir.path().join("cli");
    portfolio_rs::workspace::init_workspace(&root.to_string_lossy(), false).unwrap();
    assert!(!root.join("positions.json").exists());

    app.rt
        .state
        .load_workspace(root.to_string_lossy().into_owned());
    app.settle();
    assert!(app.rt.state.folder_prompt.is_none());
    assert!(app.rt.state.is_workspace(), "{:?}", app.rt.state.error);
    assert_eq!(
        fs::read_to_string(root.join("positions.json")).unwrap(),
        "[]\n"
    );
    assert!(app.rt.state.positions.is_empty());
    assert!(app.exists(Page::Overview.key()));
}

#[test]
fn a_picked_portfolio_folder_of_a_command_line_workspace_opens_the_parent() {
    let mut app = Harness::boot(false, None);
    let root = app.dir.path().join("cli");
    portfolio_rs::workspace::init_workspace(&root.to_string_lossy(), false).unwrap();
    let inner = root.join("portfolio");
    app.rt
        .state
        .load_workspace(inner.to_string_lossy().into_owned());
    app.settle();
    assert_eq!(
        app.rt.state.open.workspace_dir.as_deref(),
        Some(root.to_str().unwrap())
    );
    assert!(root.join("positions.json").is_file());
    assert!(!inner.join("positions.json").exists());
}

#[test]
fn opening_another_portfolio_asks_before_discarding_edits() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    let original = app.dir.path().join("portfolio.json");
    edited(&mut app);

    app.command('o', false);
    assert!(
        app.rt.file_dialog().is_none(),
        "the dialog waits for an answer"
    );
    assert!(matches!(
        app.rt.state.confirm,
        Some(Confirm::Unsaved(PendingOpen::File))
    ));
    app.click("confirm-cancel");
    assert!(app.rt.state.dirty);
    assert_eq!(app.rt.state.positions.len(), 6);

    app.command('o', false);
    app.click("unsaved-save");
    assert!(
        app.rt.file_dialog().is_some(),
        "saving continues into the dialog"
    );
    assert!(!app.rt.state.dirty);
    assert_eq!(
        fs::read_to_string(&original)
            .unwrap()
            .matches("Widget")
            .count(),
        1
    );
    app.rt.resolve_file_dialog(None).unwrap();
    app.settle();
    assert_eq!(
        app.rt.state.positions.len(),
        6,
        "cancelling the dialog keeps the save"
    );

    edited(&mut app);
    let other = app.dir.path().join("other.json");
    fs::write(&other, "[]\n").unwrap();
    let dropped = other.clone();
    app.rt.update(move |state, _| state.open_dropped(&dropped));
    app.settle();
    assert!(matches!(
        app.rt.state.confirm,
        Some(Confirm::Unsaved(PendingOpen::Path(_)))
    ));
    assert_eq!(app.rt.state.positions.len(), 7, "nothing opened yet");
    app.click("unsaved-discard");
    assert_eq!(
        app.rt.state.open.file_path.as_deref(),
        Some(other.to_str().unwrap())
    );
    assert!(app.rt.state.positions.is_empty());
    // The first edit was saved. The second was not.
    assert_eq!(
        fs::read_to_string(&original)
            .unwrap()
            .matches("Widget")
            .count(),
        1
    );

    edited(&mut app);
    let workspace = app.dir.path().join("elsewhere");
    fs::create_dir(&workspace).unwrap();
    app.command('o', true);
    assert!(matches!(
        app.rt.state.confirm,
        Some(Confirm::Unsaved(PendingOpen::Workspace))
    ));
    app.click("unsaved-discard");
    assert_eq!(
        app.rt.file_dialog().map(|dialog| dialog.0),
        Some(FileDialogKind::Folder)
    );
    app.rt.resolve_file_dialog(Some(workspace.clone())).unwrap();
    app.settle();
    assert!(app.exists("folder-start"), "{:?}", app.rt.state.error);
    assert!(!app.has_text("Apple"));
    app.click("folder-cancel");
    assert_eq!(
        app.rt.state.open.file_path.as_deref(),
        Some(other.to_str().unwrap()),
        "cancel returns to the portfolio that was open"
    );
}

#[test]
fn closing_returns_to_the_welcome_screen() {
    let mut app = Harness::boot(true, None);
    app.command('k', false);
    app.type_text("close");
    assert!(app.has_text("Close Workspace"));
    app.press(Key::Enter);
    assert!(!app.rt.state.open.is_open(), "{:?}", app.rt.state.error);
    assert!(app.exists("welcome-create"));
    assert!(!app.has_text("Bitcoin"));

    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    edited(&mut app);
    app.command('w', true);
    assert!(matches!(
        app.rt.state.confirm,
        Some(Confirm::Unsaved(PendingOpen::Close))
    ));
    app.click("unsaved-discard");
    assert!(app.exists("welcome-create"));
    assert!(!app.rt.state.dirty);
    assert!(
        !fs::read_to_string(app.dir.path().join("portfolio.json"))
            .unwrap()
            .contains("Widget")
    );
}

#[test]
fn add_position_does_nothing_without_a_portfolio() {
    let mut app = Harness::boot(false, None);
    app.rt
        .update(|state, tx| state.run_command(Command::AddPosition, tx));
    app.settle();
    assert!(app.rt.state.form.is_none());
    assert!(!app.rt.state.dirty);
    assert!(app.rt.state.positions.is_empty());
    assert!(app.exists("welcome-create"));

    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.command('n', false);
    assert!(app.rt.state.form.is_some());
    app.rt.state.open = stahl::backend::OpenDocument::default();
    app.rt.update(|state, _| state.submit_form());
    app.settle();
    assert!(app.rt.state.form.is_none());
    assert_eq!(app.rt.state.positions.len(), 5);
    assert!(!app.rt.state.dirty);
}

#[test]
fn an_empty_positions_file_says_so() {
    let mut app = Harness::boot(false, Some(""));
    assert!(app.has_text("The file is empty."));
    assert!(app.has_text("EOF while parsing a value"));
    assert!(!app.exists(Page::Overview.key()));

    let object = app.dir.path().join("object.json");
    fs::write(&object, "{\"Name\":\"Cash\"}\n").unwrap();
    app.rt
        .state
        .load_file(object.to_string_lossy().into_owned());
    app.settle();
    assert!(app.has_text("Expected a list of positions."));
    assert!(app.has_text("expected a sequence"), "the raw detail stays");
}

impl Harness {
    /// The phone layout on a real workspace, at an iPhone's size.
    fn phone() -> Harness {
        let mut app = Harness::boot(true, None);
        app.rt.set_viewport((390.0, 844.0), 3.0);
        app.rt.state.compact = true;
        app.settle();
        app
    }

    /// Scroll `key` into view: a phone shows little of a page at once.
    fn tap(&mut self, key: &str) {
        let key = key.to_string();
        self.rt.update(move |_, tx| tx.scroll_into_view(key));
        self.settle();
    }
}

#[test]
fn a_phone_reaches_every_page_through_the_tabs_or_more() {
    let mut app = Harness::phone();
    assert!(app.exists("tab-bar"));
    assert!(!app.has_text("Stahl Capital"), "no sidebar on a phone");

    for (page, wanted) in [
        (Page::Holdings, "Bitcoin"),
        (Page::Performance, "Time-weighted return"),
        (Page::Allocation, "The outer ring shows your policy targets"),
        (Page::Review, "Allocation vs target"),
        (Page::Rebalance, "Full Rebalance to Targets"),
        (Page::Policy, "Balanced Growth"),
        (Page::Documents, "Weekly report"),
        (Page::Briefing, "Cash buffer"),
        (Page::Health, "Portfolio file is valid"),
        (Page::Settings, "AI provider"),
        (Page::Overview, "Top movers today"),
    ] {
        let tabbed = [Page::Overview, Page::Holdings, Page::Performance].contains(&page);
        if !tabbed {
            app.click("tab-more");
            assert!(app.rt.state.more, "More lists the pages without a tab");
            app.tap(page.key());
        }
        app.click(page.key());
        assert_eq!(app.rt.state.page, page);
        assert!(!app.rt.state.more);
        assert!(app.has_text(wanted), "{page:?} should show {wanted:?}");
        assert_eq!(app.exists("back-to-more"), !tabbed, "{page:?}");
    }

    app.click("tab-more");
    app.tap(Page::Briefing.key());
    app.click(Page::Briefing.key());
    app.click("back-to-more");
    assert!(app.rt.state.more, "back from a page under More is More");
}

#[test]
fn on_a_phone_a_holding_s_details_take_the_table_s_place() {
    let mut app = Harness::phone();
    app.click(Page::Holdings.key());
    assert!(app.exists("holdings-rows"));
    let id = app.rt.state.positions[0].id;
    app.click(&format!("holding-{id}"));
    assert!(app.exists("inspector"));
    assert!(!app.exists("holdings-rows"), "no room for both");
    app.click("close-inspector");
    assert!(app.exists("holdings-rows"));
    assert!(!app.exists("inspector"));
}

#[test]
fn a_platform_picker_opens_what_it_picks_and_nothing_when_cancelled() {
    let mut app = Harness::boot(false, Some(demo::POSITIONS));
    app.rt.state.platform_picker = true;
    let workspace = app.dir.path().join("picked");
    demo::seed_workspace(&workspace);

    app.command('o', true);
    assert_eq!(
        app.rt.state.take_pick().map(|(kind, _)| kind),
        Some(PickKind::Folder),
        "Open Workspace asks the platform, not Rune"
    );
    assert_eq!(app.rt.state.take_pick(), None, "asked once");
    app.rt.state.picked(None);
    app.settle();
    assert!(app.rt.state.pick.is_none());
    assert!(!app.rt.state.is_workspace(), "cancelling opens nothing");

    app.command('o', true);
    assert!(app.rt.state.take_pick().is_some());
    let picked = workspace.to_string_lossy().into_owned();
    app.rt.update(move |state, _| state.picked(Some(picked)));
    app.settle();
    assert!(app.rt.state.is_workspace());
    assert_eq!(
        app.rt.state.open.workspace_dir.as_deref(),
        Some(workspace.to_string_lossy().as_ref())
    );

    app.command('o', false);
    assert_eq!(
        app.rt.state.take_pick().map(|(kind, _)| kind),
        Some(PickKind::File)
    );
}

#[test]
fn a_phone_s_door_turns_and_a_shake_hides_the_amounts() {
    let mut app = Harness::phone();
    for _ in 0..4 {
        app.click("vault-door");
    }
    assert_eq!(app.rt.state.door_turns, 4);
    assert!(app.has_text("Vault sealed"), "a full turn seals the vault");

    assert!(!app.rt.state.privacy);
    app.rt.update(|state, _| state.shaken());
    app.settle();
    assert!(app.rt.state.privacy);
    assert!(app.has_text("Amounts hidden"));
}

#[test]
fn a_phone_knows_when_a_field_takes_a_number() {
    let mut app = Harness::phone();
    app.click(Page::Holdings.key());
    app.click("add-position");
    assert!(!app.rt.state.number_focused(), "the name has focus first");
    app.click("form-amount");
    assert!(app.rt.state.number_focused());
    app.click("form-name");
    assert!(!app.rt.state.number_focused());
}

impl Harness {
    /// A finger along a straight line, over a few frames, then lifted.
    fn swipe(&mut self, from: (f32, f32), to: (f32, f32)) {
        let mut time = 1.0;
        self.rt.touch(9, TouchPhase::Started, from.0, from.1, time);
        self.rt.pointer_move(from.0, from.1);
        for step in 1..=12 {
            let f = step as f32 / 12.0;
            time += 1.0 / 60.0;
            let (x, y) = (from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f);
            self.rt.touch(9, TouchPhase::Moved, x, y, time);
            // As the iOS shell does, so a finger on a chart reads it out.
            self.rt.pointer_move(x, y);
            self.rt.frame(1.0 / 60.0);
        }
    }

    fn lift(&mut self, at: (f32, f32)) {
        self.rt.touch(9, TouchPhase::Ended, at.0, at.1, 2.0);
        self.rt.pointer_leave();
        self.rt.frame(1.0 / 60.0);
    }
}

#[test]
fn back_on_a_phone_closes_then_returns() {
    let mut app = Harness::phone();
    app.rt.update(|state, _| state.open_palette());
    assert!(app.rt.update_if(|state, _| state.go_back()));
    assert!(app.rt.state.palette.is_none());

    app.click(Page::Holdings.key());
    let id = app.rt.state.positions[0].id;
    app.click(&format!("holding-{id}"));
    assert!(app.rt.update_if(|state, _| state.go_back()));
    assert_eq!(app.rt.state.selected, None, "the details close first");

    app.click("tab-more");
    app.tap(Page::Briefing.key());
    app.click(Page::Briefing.key());
    assert!(app.rt.update_if(|state, _| state.go_back()));
    assert!(app.rt.state.more, "a page from More goes back to More");
    app.click(Page::Overview.key());
    assert!(
        !app.rt.update_if(|state, _| state.go_back()),
        "a tab has nowhere to go back to"
    );
}

#[test]
fn pulling_a_phone_s_page_far_enough_refreshes_the_prices() {
    let mut app = Harness::phone();
    app.swipe((195.0, 300.0), (195.0, 340.0));
    assert!(!app.rt.state.pull.past(), "a nudge isn't a pull");
    app.lift((195.0, 340.0));
    assert!(!app.rt.update_if(|state, _| state.pull_released()));
    app.settle();

    app.swipe((195.0, 250.0), (195.0, 650.0));
    assert!(
        app.rt.state.pull.past(),
        "pulled {}",
        app.rt.state.pull.distance.get()
    );
    app.lift((195.0, 650.0));
    let updates = app.rt.state.price_updates;
    assert!(app.rt.update_if(|state, _| state.pull_released()));
    assert!(app.rt.state.pull_refreshing);
    app.settle();
    assert_eq!(app.rt.state.price_updates, updates + 1);
}

#[test]
fn a_phone_s_holding_slides_open_to_edit_or_delete() {
    let mut app = Harness::phone();
    app.click(Page::Holdings.key());
    let id = app.rt.state.positions[0].id;
    let (x, y) = app.rt.find(&format!("holding-{id}")).unwrap();
    let edit = format!("swipe-edit-{id}");
    let hidden = |app: &Harness| app.rt.find(&edit).is_none_or(|(x, _)| x > 390.0);
    assert!(hidden(&app), "the actions start out of sight");

    app.swipe((x + 120.0, y), (x - 20.0, y));
    app.lift((x - 20.0, y));
    app.rt.update(|state, tx| state.settle_swipes(tx));
    app.settle();
    assert!(!hidden(&app), "a long enough slide opens the row");
    assert_eq!(app.rt.state.selected, None, "a slide isn't a tap");

    app.click(&format!("swipe-delete-{id}"));
    assert!(
        matches!(app.rt.state.confirm, Some(Confirm::Delete { id: deleting, .. }) if deleting == id)
    );
    app.settle();
    assert!(hidden(&app), "an action shuts its row");
}

#[test]
fn a_phone_can_lock_the_vault_and_ticks_along_the_chart() {
    let mut app = Harness::phone();
    app.rt.state.lock = Some(false);
    app.rt.state.lock_name = "Face ID".to_string();
    app.click("tab-more");
    app.tap(Page::Settings.key());
    app.click(Page::Settings.key());
    app.tap("settings-lock");
    app.click("settings-lock");
    assert_eq!(app.rt.state.lock, Some(true));
    assert!(app.has_text("Face ID locks the vault"));

    // On a phone, Overview's chart is below the fold; Performance's is near the top.
    app.click(Page::Performance.key());
    let (x, y) = app.rt.find("performance-chart").unwrap();
    app.rt.pointer_move(x - 100.0, y);
    app.rt.frame(1.0 / 60.0);
    let first = app.rt.state.chart_point;
    app.rt.pointer_move(x + 100.0, y);
    app.rt.frame(1.0 / 60.0);
    assert!(first.is_some() && app.rt.state.chart_point > first);
}
