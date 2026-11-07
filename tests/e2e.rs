//! End-to-end tests: the real views and handlers on real `portfolio_rs` data with fixture
//! quotes, driven through pointer and keyboard input in Rune's headless runtime.
//!
//! These never touch the user's config: the backend points at a temp directory, and opening goes
//! through `startup`, which doesn't write it.

use std::{env, fs, path::Path, sync::Once, thread, time::Duration};

use chrono::Local;
use portfolio_rs::{
    AppConfig, FixtureProvider,
    services::quotes::AnyProvider,
    state::{AppState, PositionDto},
};
use rune::{Key, KeyEvent, Mods, Runtime};
use stahl::{
    app,
    backend::{Backend, Period},
    demo,
    state::{Confirm, Page, State},
};
use tempfile::TempDir;
use tokio::runtime::Runtime as Tokio;

/// What the Edit form says when purchases decide the amount.
const PURCHASES_NOTE: &str = "Amount is the total of this position's purchases";

struct Harness {
    rt: Runtime<State>,
    dir: TempDir,
    _tokio: Tokio,
}

fn offline() {
    static ONCE: Once = Once::new();
    // Parts of portfolio_rs resolve their own quote provider from this. Tests must never reach
    // the network. SAFETY: before any test here reads the environment.
    ONCE.call_once(|| unsafe { env::set_var("PORTFOLIO_RS_QUOTES", "offline") });
}

impl Harness {
    fn boot(workspace: bool, file: Option<&str>) -> Harness {
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
        let rt = app::headless(backend, initial, (1320.0, 860.0));
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
    assert!(app.has_text("Line 2, column"));
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
    let app = Harness::boot(false, None);
    assert!(app.exists("welcome-workspace"));
    assert!(app.exists("welcome-file"));
    assert!(!app.exists(Page::Overview.key()));
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
