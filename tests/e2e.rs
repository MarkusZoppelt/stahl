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
        Self::boot_at(workspace, file, updater, (1320.0, 860.0))
    }

    /// A window tall enough to show the whole Overview, for a hold near the bottom of it.
    fn boot_tall(workspace: bool) -> Harness {
        Self::boot_at(workspace, None, Updater::disabled(), (1320.0, 1600.0))
    }

    fn boot_at(workspace: bool, file: Option<&str>, updater: Updater, size: (f32, f32)) -> Harness {
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
        let rt = app::headless_with_updater(backend, initial, size, updater);
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

    /// Click the first visible element whose text is exactly `label`, for a control with no key.
    fn click_label(&mut self, label: &str) {
        let (x, y) = self
            .rt
            .elements()
            .into_iter()
            .find(|element| {
                element.text.as_deref() == Some(label)
                    && element.visible[2] > 0.0
                    && element.visible[3] > 0.0
            })
            .map(|element| {
                let [x, y, width, height] = element.rect;
                (x + width / 2.0, y + height / 2.0)
            })
            .unwrap_or_else(|| panic!("nothing to click labelled {label}"));
        self.rt.pointer_move(x, y);
        self.rt.frame(1.0 / 60.0);
        self.rt.pointer_down(x, y);
        self.rt.pointer_up(x, y);
        self.settle();
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
        (Page::Rebalance, "I have this much to invest"),
        (Page::Policy, "Balanced Growth"),
        (Page::Documents, "Weekly report"),
        (Page::Briefing, "Cash buffer"),
        (Page::Health, "Portfolio file is valid"),
        (Page::Settings, "AI provider"),
        (Page::Records, "The first valuation"),
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

/// A broker's file from `tests/intake/`, dropped on the window as the platform hands it over.
fn dropped(app: &mut Harness, name: &str) -> PathBuf {
    let path = app.dir.path().join(name);
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/intake")
            .join(name),
        &path,
    )
    .unwrap();
    app.rt.drop_files(vec![path.clone()], Some((660.0, 430.0)));
    app.settle();
    path
}

/// Every position's id and count, which an import changes and an undo puts back.
fn counts(app: &Harness) -> Vec<(String, f64)> {
    app.rt
        .state
        .positions
        .iter()
        .map(|position| {
            (
                position.position_id.clone().unwrap_or_default(),
                position.amount,
            )
        })
        .collect()
}

#[test]
fn accepting_a_broker_file_changes_the_positions_and_undo_puts_them_back() {
    let mut app = Harness::boot(true, None);
    let before = counts(&app);

    dropped(&mut app, "de-transactions.csv");
    assert!(app.rt.state.intake.is_some(), "the review sheet is up");
    assert!(app.has_text("Two purchases, one sale and one holding we have not seen before."));
    assert!(app.exists("intake-accept-all"));
    assert!(app.exists("intake-accept-0"));
    assert!(
        app.has_text("line 5:"),
        "each change shows the file's own row"
    );

    app.click("intake-accept-all");
    assert!(
        app.rt.state.intake.is_none(),
        "taking everything puts the sheet away"
    );
    assert!(app.rt.state.dirty, "an import is unsaved until it is saved");
    assert!(app.rt.state.intake_undo.is_some());
    assert!(app.exists("toast"));
    let after = counts(&app);
    assert_ne!(before, after, "the positions changed");
    assert!(
        app.rt
            .state
            .positions
            .iter()
            .any(|position| position.ticker.as_deref() == Some("NGSC")),
        "the holding the file added is on record"
    );

    app.click_label("Undo");
    assert!(!app.rt.state.dirty, "the undo leaves nothing unsaved");
    assert!(app.rt.state.intake_undo.is_none(), "one undo only");
    assert_eq!(counts(&app), before, "the positions are as they were");
}

#[test]
fn one_row_can_be_accepted_on_its_own() {
    let mut app = Harness::boot(true, None);
    let before = counts(&app);
    dropped(&mut app, "de-transactions.csv");

    app.click("intake-accept-0");
    assert!(
        app.rt.state.intake.is_some(),
        "the sheet stays open for the rest"
    );
    assert!(app.has_text("Recorded"), "the row says what happened to it");
    assert!(app.rt.state.dirty);
    assert_ne!(counts(&app), before, "that one change is on record");
    assert!(app.rt.state.intake_undo.is_some());

    app.click("intake-leave-1");
    assert!(!app.exists("intake-row-1"), "a row left is not asked again");
    app.click("intake-dismiss");
    assert!(app.rt.state.intake.is_none());
}

#[test]
fn the_same_broker_file_twice_proposes_nothing_the_second_time() {
    let mut app = Harness::boot(true, None);
    let path = dropped(&mut app, "de-transactions.csv");
    app.click("intake-accept-all");
    assert!(app.rt.state.dirty);

    app.rt.drop_files(vec![path], Some((660.0, 430.0)));
    app.settle();
    let review = app.rt.state.intake.as_ref().expect("the sheet still opens");
    let ledger = review.ledger.as_ref().expect("the file still reads");
    assert!(
        ledger.proposals.iter().all(|proposal| !proposal.ready),
        "{:#?}",
        ledger.proposals
    );
    assert!(ledger.agreed > 0);
    assert!(!app.exists("intake-accept-all"), "there is nothing to take");
    assert!(app.has_text("every line is already on your record"));
}

#[test]
fn an_unreadable_file_says_why_and_offers_nothing_to_take() {
    let mut app = Harness::boot(true, None);
    app.command('2', false);
    let path = app.dir.path().join("statement.pdf");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/intake/statement.pdf"),
        &path,
    )
    .unwrap();
    app.click("import-intake");
    assert_eq!(
        app.rt.file_dialog().expect("the import dialog is open").0,
        FileDialogKind::Open
    );
    app.rt
        .resolve_file_dialog(Some(path))
        .expect("the dialog is answered");
    app.settle();

    assert!(app.has_text("This is a PDF."));
    assert!(!app.exists("intake-accept-all"));
    app.click("intake-cancel");
    assert!(app.rt.state.intake.is_none(), "Close puts the sheet away");
    assert!(!app.rt.state.dirty, "an unreadable file changes nothing");
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
fn overview_reads_as_what_happened_where_you_stand_and_what_to_do() {
    let app = Harness::boot(true, None);
    assert!(app.exists("statement-text"), "what happened");
    assert!(app.exists("total-value"), "where you stand");
    assert!(app.exists("agenda"), "what to do");
    assert!(app.exists("overview-allocation"));
    assert!(app.exists("mover-0"));
    assert!(
        !app.exists("overview-chart"),
        "the month's chart lives on Performance, so the agenda stays on this page"
    );
}

#[test]
fn overview_agrees_with_the_review_and_leads_to_the_fix() {
    let mut app = Harness::boot(true, None);
    // The demo is four classes out of tolerance: Overview must not call that all clear.
    assert!(!app.has_text("Nothing right now"));
    assert!(app.has_text("4 of 5 asset classes are outside their tolerance"));
    assert_eq!(app.rt.state.agenda_len(), 1);

    app.click("matter-adjust-rebalance");
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

fn palette_row(app: &Harness, key: &str) -> String {
    app.rt
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some(key))
        .and_then(|element| element.label)
        .unwrap_or_default()
}

#[test]
fn the_palette_answers_a_figure_and_opens_its_page() {
    let mut app = Harness::boot(true, None);
    app.command('k', false);
    app.type_text("today");
    assert_eq!(
        palette_row(&app, "palette-0"),
        "Today's change, +€280.00",
        "the amount is in the row, before going anywhere"
    );
    app.press(Key::Enter);
    assert!(app.rt.state.palette.is_none());
    assert_eq!(app.rt.state.page, Page::Overview);

    // An asset class answers with its value, and Enter opens Allocation.
    app.command('k', false);
    app.type_text("gold");
    assert_eq!(palette_row(&app, "palette-0"), "Gold, €5,000.00");
    app.press(Key::Enter);
    assert!(app.rt.state.palette.is_none());
    assert_eq!(app.rt.state.page, Page::Allocation);

    // A holding's row carries its value, and Enter opens that holding.
    app.command('k', false);
    app.type_text("spy");
    assert_eq!(palette_row(&app, "palette-0"), "S&P 500, €40,000.00");
    let spy = app
        .rt
        .state
        .positions
        .iter()
        .find(|position| position.ticker.as_deref() == Some("SPY"))
        .unwrap()
        .id;
    app.press(Key::Enter);
    assert_eq!(app.rt.state.page, Page::Holdings);
    assert_eq!(app.rt.state.selected, Some(spy));

    app.command('h', true);
    app.command('k', false);
    app.type_text("total");
    assert_eq!(
        palette_row(&app, "palette-0"),
        "Total value, ••••••",
        "privacy hides the amount in the row"
    );
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
        (Page::Records, "The room is empty", "records-empty-add"),
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
        app.has_text("as a table or a map"),
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
fn dragging_the_chart_measures_that_span_until_escape() {
    let mut app = Harness::boot(true, None);
    app.command('4', false);
    app.settle();
    let (x, y) = app.rt.find("performance-chart").unwrap();
    app.rt.pointer_down(x - 120.0, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_move(x + 80.0, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_up(x + 80.0, y);
    app.settle();
    let span = app.rt.state.measure.span.expect("the drag is kept");
    assert!(span.0 < span.1, "{span:?}");
    assert!(app.has_text("days"), "the readout says how long");
    app.press(Key::Escape);
    assert!(app.rt.state.measure.span.is_none(), "escape dismisses it");

    // Shift and the arrows measure too, once the chart has focus.
    app.click("performance-chart");
    app.rt.key(KeyEvent {
        key: Key::Left,
        mods: Mods {
            shift: true,
            ..Mods::default()
        },
    });
    app.settle();
    assert!(
        app.rt.state.measure.span.is_some(),
        "shift and an arrow measures"
    );
}

#[test]
fn the_outlook_years_slider_moves_the_median_by_key() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Performance.key());
    let median = |app: &Harness| {
        app.rt
            .elements()
            .into_iter()
            .find(|element| element.key.as_deref() == Some("outlook-median"))
            .and_then(|element| element.text.clone())
            .expect("the outlook median")
    };
    let before = median(&app);
    app.rt.update(|_, tx| tx.focus("outlook-years", true));
    app.rt.frame(1.0 / 60.0);
    app.press(Key::Right);
    assert_eq!(app.rt.state.outlook.years, 11);
    let after = median(&app);
    assert_ne!(before, after, "a longer horizon moves the median");
    app.press(Key::Left);
    assert_eq!(app.rt.state.outlook.years, 10);
    assert_eq!(median(&app), before, "and back again");
    // The monthly amount steps with the arrows too.
    app.rt.update(|_, tx| tx.focus("outlook-monthly", true));
    app.rt.frame(1.0 / 60.0);
    app.press(Key::Right);
    assert_eq!(app.rt.state.outlook.monthly, 550.0);
}

#[test]
fn a_phone_measures_on_the_rail_and_a_vertical_drag_still_scrolls() {
    let mut app = Harness::phone();
    app.click(Page::Performance.key());
    app.tap("performance-chart");
    let (x, y) = app.rt.find("performance-chart").unwrap();
    // The chart itself is not a drag target, so a finger down the page still scrolls.
    app.rt.touch(1, TouchPhase::Started, x, y, 0.0);
    app.rt.touch(1, TouchPhase::Moved, x, y + 90.0, 0.08);
    app.rt.touch(1, TouchPhase::Ended, x, y + 90.0, 0.12);
    app.rt.frame(1.0 / 60.0);
    assert!(
        app.rt.state.measure.span.is_none(),
        "a vertical drag on the chart is a scroll"
    );

    app.tap("chart-rail");
    let (rx, ry) = app.rt.find("chart-rail").unwrap();
    app.rt.touch(1, TouchPhase::Started, rx - 60.0, ry, 0.0);
    app.rt.touch(1, TouchPhase::Moved, rx + 60.0, ry, 0.08);
    app.rt.touch(1, TouchPhase::Ended, rx + 60.0, ry, 0.12);
    app.settle();
    assert!(
        app.rt.state.measure.span.is_some(),
        "the rail measures without taking the page"
    );
}

#[test]
fn a_chart_follows_the_pointer() {
    let mut app = Harness::boot(true, None);
    // The month's chart is on Performance. Overview names the change instead of drawing it again.
    app.click(Page::Performance.key());
    let (x, y) = app.rt.find("performance-chart").unwrap();
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
    let mut app = Harness::boot_tall(true);
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
        "sort-impact",
    ]
    .into_iter()
    .filter(|key| app.exists(key))
    .collect()
}

#[test]
fn the_table_drops_columns_rather_than_overlap_them() {
    let mut app = on_holdings();
    assert_eq!(columns_of(&app).len(), 6, "a wide window has them all");

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
fn of_today_weights_a_holding_by_how_much_of_the_portfolio_it_is() {
    let mut app = on_holdings();
    assert!(app.has_text("Of today"));
    // S&P 500 is down 0.60% but is most of the book, so it moves the portfolio more than Gold,
    // which is up 2.04% on a small slice. The figures are percentage points of yesterday.
    assert!(app.has_text("-0.22%"), "S&P 500's share of the move");
    assert!(app.has_text("+0.42%"), "Bitcoin's share of the move");
    assert!(app.has_text("+0.09%"), "Gold's share of the move");

    app.click("sort-impact");
    let y = |id: usize| app.rt.find(&format!("holding-{id}")).unwrap().1;
    assert!(
        y(0) < y(3) && y(3) < y(1),
        "Bitcoin, then Gold, then S&P 500"
    );
    app.click("holding-1");
    assert!(app.has_text("Of today"), "the details name it too");
}

#[test]
fn the_holdings_map_selects_like_a_row_and_arrows_cross_it() {
    let mut app = on_holdings();
    app.click("segment1");
    assert!(app.rt.state.holdings_map);
    assert!(app.exists("tile-1"), "a holding becomes a tile");
    assert!(!app.exists("sort-name"), "the table is the other view");
    assert!(app.has_text("Area is value"));

    app.click("tile-1");
    assert_eq!(app.rt.state.selected, Some(1));
    assert!(app.exists("edit-position"), "a tile opens the same details");
    app.press(Key::Right);
    assert_ne!(
        app.rt.state.selected,
        Some(1),
        "right leaves the tile on the left"
    );

    app.click("segment0");
    assert!(!app.rt.state.holdings_map);
    assert!(app.exists("holding-1"), "back to the row");
}

#[test]
fn a_phone_can_open_the_holdings_map() {
    let mut app = Harness::phone();
    app.click(Page::Holdings.key());
    app.tap("segment1");
    app.click("segment1");
    assert!(app.exists("tile-0"));
    app.tap("tile-0");
    app.click("tile-0");
    assert_eq!(app.rt.state.selected, Some(0));
    assert!(
        app.exists("edit-position"),
        "the details take the map's place"
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
        (Page::Rebalance, "I have this much to invest"),
        (Page::Policy, "Balanced Growth"),
        (Page::Documents, "Weekly report"),
        (Page::Briefing, "Cash buffer"),
        (Page::Health, "Portfolio file is valid"),
        (Page::Settings, "AI provider"),
        (Page::Records, "The first valuation"),
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

fn element(app: &Harness, key: &str) -> rune::ElementInfo {
    app.rt
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some(key))
        .unwrap_or_else(|| panic!("no element {key}"))
}

#[test]
fn the_door_opens_once_from_welcome_and_a_refresh_does_not_replay_it() {
    let mut app = Harness::boot(true, None);
    let dir = app
        .rt
        .state
        .open
        .workspace_dir
        .clone()
        .expect("the demo workspace");
    app.rt.update(|state, _| state.reset());
    app.settle();
    assert!(app.exists("door-welcome"), "back on the welcome screen");
    assert!(
        app.rt.state.opening.is_none(),
        "nothing is opening on the welcome screen"
    );
    let from = element(&app, "door-welcome").rect;
    assert!(
        from[2] > 100.0,
        "the welcome door is the large one: {from:?}"
    );

    let arrivals = app.rt.state.door_arrivals;
    app.rt.update(|state, _| state.load_workspace(dir.clone()));
    app.rt.state.settle();
    app.rt.frame(1.0 / 60.0);
    app.rt.frame(1.0 / 60.0);
    let opening = app.rt.state.opening.expect("the door opens");
    assert!(
        (opening.center[0] - (from[0] + from[2] / 2.0)).abs() < 1.0
            && (opening.center[1] - (from[1] + from[3] / 2.0)).abs() < 1.0,
        "it starts where the welcome door was: {from:?} -> {:?}",
        opening.center
    );
    assert!(
        (opening.radius - from[2] * (0.5 - 1.0 / 512.0)).abs() < 0.5,
        "at the welcome door's size"
    );
    assert!(
        app.has_text("Total value"),
        "the page is laid out under the door"
    );
    assert_eq!(
        app.rt.state.door_arrivals,
        arrivals + 1,
        "one arrival per opened portfolio"
    );

    // Clicks go through to the page while the door opens.
    // Clicks go through to the page once the halves have started to part.
    for _ in 0..66 {
        app.rt.frame(1.0 / 60.0);
    }
    assert!(
        app.rt.state.opening.is_some(),
        "the door is still opening at 1.1 s"
    );
    app.click(Page::Holdings.key());
    assert_eq!(app.rt.state.page, Page::Holdings);
    assert!(
        app.rt.state.opening.is_none(),
        "the opening has ended after two seconds"
    );

    // A refresh does not open the door again, nor count as an arrival.
    let updates = app.rt.state.price_updates;
    let (x, y) = app.rt.find("refresh-prices").unwrap();
    app.rt.pointer_move(x, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(x, y);
    app.rt.pointer_up(x, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.state.settle();
    for _ in 0..30 {
        app.rt.frame(1.0 / 60.0);
    }
    assert_eq!(app.rt.state.price_updates, updates + 1);
    assert!(
        app.rt.state.opening.is_none(),
        "a refresh does not open the door again"
    );
    assert_eq!(
        app.rt.state.door_arrivals,
        arrivals + 1,
        "nor counts as an arrival"
    );

    // Closing and opening another portfolio opens the door once more.
    app.rt.update(|state, _| state.reset());
    app.settle();
    assert!(app.exists("door-welcome"));
    app.rt.update(|state, _| state.load_workspace(dir));
    app.rt.state.settle();
    app.rt.frame(1.0 / 60.0);
    assert!(
        app.rt.state.opening.is_some(),
        "a new portfolio opens the door again"
    );
}

#[test]
fn launched_into_a_portfolio_opens_the_door_once_and_marks_what_needs_attention() {
    use portfolio_rs::mcp::dto::{Category, Finding, Severity};

    let mut app = Harness::boot(true, None);
    // Launched straight into a portfolio: the door opened in the window's middle, once.
    assert!(!app.exists("door-opening"));
    assert_eq!(app.rt.state.door_arrivals, 1);
    assert!(
        app.rt.state.opening.is_none(),
        "the opening has played and ended"
    );
    app.click(Page::Holdings.key());
    assert_eq!(app.rt.state.page, Page::Holdings);

    let door = element(&app, "door-sidebar");
    assert_eq!(
        door.label.as_deref(),
        Some("Stahl Capital. 1 thing needs attention"),
        "the demo's drift past its policy, which the agenda lists too"
    );
    assert_eq!(app.rt.state.agenda_len(), 1);
    assert!(app.exists("door-attention-sidebar"));

    // A refresh turns the tumblers once, and it is not an opening.
    let turns = app.rt.state.price_updates + app.rt.state.door_arrivals;
    let updates = app.rt.state.price_updates;
    let (x, y) = app.rt.find("refresh-prices").unwrap();
    app.rt.pointer_move(x, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(x, y);
    app.rt.pointer_up(x, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.state.settle();
    app.rt.frame(1.0 / 60.0);
    assert_eq!(app.rt.state.price_updates, updates + 1);
    assert_eq!(
        app.rt.state.price_updates + app.rt.state.door_arrivals,
        turns + 1,
        "the tumblers have one more turn to make"
    );
    assert!(
        app.rt.state.opening.is_none(),
        "a refresh is not an opening"
    );

    app.rt.update(|state, _| {
        state.findings.push(Finding {
            code: "cash_below_minimum_amount".into(),
            severity: Severity::Warning,
            category: Category::Policy,
            message: "Cash is below the minimum your policy sets".into(),
            position_id: None,
            asset_class: None,
            detail: None,
        })
    });
    app.settle();
    assert!(app.exists("door-attention-sidebar"));
    // The agenda lists it as a notice, and a notice doesn't count on the door.
    assert_eq!(app.rt.state.agenda_len(), 2);
    assert_eq!(
        element(&app, "door-sidebar").label.as_deref(),
        Some("Stahl Capital. 1 thing needs attention")
    );
}

#[test]
fn moving_between_pages_knows_which_way_and_a_refresh_flashes_what_moved() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Review.key());
    assert_eq!(app.rt.state.previous_page, Page::Overview);
    app.click(Page::Allocation.key());
    assert_eq!(app.rt.state.previous_page, Page::Review);

    // The demo's quotes are fixed, so a refresh moves nothing and nothing flashes.
    app.click("refresh-prices");
    assert!(app.rt.state.price_moves.is_empty());
    let flashes = |app: &Harness| {
        app.rt
            .elements()
            .into_iter()
            .filter(|element| {
                element
                    .key
                    .as_deref()
                    .is_some_and(|key| key.contains("-flash-"))
            })
            .count()
    };
    assert_eq!(flashes(&app), 0);

    // When one does move, its asset class washes once in its colour.
    let id = app.rt.state.positions[0].id;
    app.rt.update(move |state, _| {
        state.price_moves = vec![(id, 0.01)];
        state.price_updates += 1;
    });
    app.rt.frame(1.0 / 60.0);
    assert!(flashes(&app) > 0, "what moved flashes");
}

#[test]
fn the_rebalance_slider_moves_the_plan_and_the_total_stays() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Rebalance.key());
    assert!(app.has_text("Stahl never trades."));
    assert!(app.has_text("Nothing changes yet"));
    let rail = app
        .rt
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("rebalance-rail"))
        .expect("the slider");
    assert_eq!(rail.role, "slider");
    assert_eq!(rail.label.as_deref(), Some("How far to rebalance"));
    let [x, y, w, h] = rail.rect;
    let mid = y + h / 2.0;
    // Drag the slider all the way to the right: every class goes to its target.
    app.rt.pointer_move(x + 2.0, mid);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(x + 2.0, mid);
    app.rt.frame(1.0 / 60.0);
    let skipped = app.rt.live_only_events();
    // Small moves: the thumbs follow, and the view only runs when a figure would change.
    let mut at = x + 2.0;
    for _ in 0..30 {
        at += 3.0;
        app.rt.pointer_move(at, mid);
        app.rt.frame(1.0 / 60.0);
    }
    assert!(
        app.rt.live_only_events() > skipped,
        "a drag must not rebuild the view on every move"
    );
    for _ in 0..12 {
        at += w * 0.08;
        app.rt.pointer_move(at, mid);
        app.rt.frame(1.0 / 60.0);
    }
    app.rt.pointer_up(x + w + 40.0, mid);
    app.settle();
    assert_eq!(
        app.rt.state.rebalance.plan,
        stahl::state::RebalancePlan::Blend,
        "letting go keeps the blend"
    );
    assert!((app.rt.state.rebalance.fraction - 1.0).abs() < 0.02);
    assert!(app.has_text("On target"));
    assert!(app.has_text("Buy €13,700.00"), "Stocks goes to 50%");
    assert!(app.has_text("Sell €16,260.00"), "Crypto comes down to 10%");
    let values = stahl::view::rebalance::values(&app.rt.state);
    assert!((values.iter().sum::<f64>() - 107_400.0).abs() < 0.01);
    assert!(values.iter().all(|value| *value >= -1e-9));
    // What gets copied is the plan on screen.
    let text = stahl::view::rebalance::markdown(&app.rt.state);
    assert!(text.contains("## On target"), "{text}");
    assert!(
        text.contains("| Stocks | Increase | 13,700.00 EUR |"),
        "{text}"
    );
    assert!(text.contains("Stahl never trades."), "{text}");

    // The keyboard: a point an arrow, five on Page keys, the ends on Home and End.
    app.rt.update(|_, tx| tx.focus("rebalance-rail", true));
    app.rt.frame(1.0 / 60.0);
    app.press(Key::Home);
    assert!(app.rt.state.rebalance.fraction < 1e-9);
    assert!(app.has_text("Nothing changes yet"));
    app.press(Key::Right);
    assert!((app.rt.state.rebalance.fraction - 0.01).abs() < 1e-9);
    app.press(Key::PageUp);
    assert!((app.rt.state.rebalance.fraction - 0.06).abs() < 1e-9);
    app.press(Key::End);
    assert!((app.rt.state.rebalance.fraction - 1.0).abs() < 1e-9);
}

#[test]
fn a_row_moves_by_pointer_and_by_key_and_cash_takes_the_difference() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Rebalance.key());
    let thumb = app
        .rt
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("rebalance-thumb-0"))
        .expect("the Stocks thumb");
    assert_eq!(thumb.role, "slider");
    assert_eq!(
        thumb.label.as_deref(),
        Some("Stocks after rebalancing"),
        "the thumb names what it sets"
    );
    let [x, y, w, h] = thumb.rect;
    assert!(h >= 44.0, "a finger needs 44 points: {h}");
    let (mid, start) = (y + h / 2.0, x + w / 2.0);
    app.rt.pointer_move(start, mid);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(start, mid);
    app.rt.frame(1.0 / 60.0);
    for step in 1..=8 {
        app.rt.pointer_move(start + step as f32 * 14.0, mid);
        app.rt.frame(1.0 / 60.0);
    }
    app.rt.pointer_up(start + 112.0, mid);
    app.settle();
    assert!(matches!(
        app.rt.state.rebalance.plan,
        stahl::state::RebalancePlan::Adjusted { .. }
    ));
    assert!(app.has_text("Adjusted by hand"));
    let values = stahl::view::rebalance::values(&app.rt.state);
    assert!((values.iter().sum::<f64>() - 107_400.0).abs() < 0.01);
    assert!(
        values[0] > 40_000.0 && values[4] < 30_000.0,
        "Stocks grew out of cash: {values:?}"
    );

    // Return puts the row on its target tick.
    app.rt.update(|_, tx| tx.focus("rebalance-thumb-0", true));
    app.rt.frame(1.0 / 60.0);
    app.press(Key::Enter);
    let values = stahl::view::rebalance::values(&app.rt.state);
    assert!(
        (values[0] - 53_700.0).abs() < 1.0,
        "Stocks on 50%: {values:?}"
    );

    // An arrow is a point of the portfolio, Page Up five.
    app.press(Key::Left);
    let values = stahl::view::rebalance::values(&app.rt.state);
    assert!(
        (values[0] - 52_626.0).abs() < 3.0,
        "a point down: {values:?}"
    );
    app.press(Key::PageDown);
    let values = stahl::view::rebalance::values(&app.rt.state);
    assert!(
        (values[0] - 47_256.0).abs() < 3.0,
        "five points down: {values:?}"
    );

    // Dragged to within a point of its tick, the thumb lands on it.
    let track = app
        .rt
        .elements()
        .into_iter()
        .find(|element| element.key.as_deref() == Some("rebalance-track-0"))
        .expect("the Stocks track")
        .rect;
    // The demo's tracks run from 0 to 70%. Stop four tenths of a point short of Stocks' 50%
    // target: the thumb should take the last step itself.
    let at = stahl::view::rebalance::values(&app.rt.state)[0] / 107_400.0 * 100.0;
    let to_tick = ((49.6 - at) / 70.0 * f64::from(track[2])) as f32;
    let (tx, ty) = app.rt.find("rebalance-thumb-0").unwrap();
    app.rt.pointer_move(tx, ty);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(tx, ty);
    app.rt.frame(1.0 / 60.0);
    for step in 1..=8 {
        app.rt.pointer_move(tx + to_tick * step as f32 / 8.0, ty);
        app.rt.frame(1.0 / 60.0);
    }
    app.rt.pointer_up(tx + to_tick, ty);
    app.settle();
    let values = stahl::view::rebalance::values(&app.rt.state);
    assert!(
        (values[0] - 53_700.0).abs() < 1.0,
        "a thumb near its target lands on it: {values:?}"
    );

    // Reset goes back to the slider.
    app.click("rebalance-reset");
    assert_eq!(
        app.rt.state.rebalance.plan,
        stahl::state::RebalancePlan::Blend
    );
    assert!(app.has_text("Nothing changes yet"));
}

#[test]
fn a_row_cannot_spend_more_cash_than_there_is() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Rebalance.key());
    stahl::view::rebalance::set_share(&mut app.rt.state, 0, 90.0);
    app.settle();
    let values = stahl::view::rebalance::values(&app.rt.state);
    assert!(
        values.iter().all(|value| *value >= -1e-9),
        "nothing goes negative: {values:?}"
    );
    assert!((values.iter().sum::<f64>() - 107_400.0).abs() < 0.01);
    assert!(values[4] < 0.01, "cash ran out: {values:?}");
}

#[test]
fn on_a_phone_a_thumb_drags_across_and_a_row_still_scrolls_the_page() {
    let mut app = Harness::phone();
    app.click("tab-more");
    app.tap(Page::Rebalance.key());
    app.click(Page::Rebalance.key());
    assert_eq!(app.rt.state.page, Page::Rebalance);
    app.tap("rebalance-thumb-0");

    let (x, y) = app.rt.find("rebalance-thumb-0").unwrap();
    // A finger across the thumb sets the class; it must not pan the page.
    app.rt.touch(1, TouchPhase::Started, x, y, 0.0);
    app.rt.touch(1, TouchPhase::Moved, x + 60.0, y, 0.08);
    app.rt.touch(1, TouchPhase::Ended, x + 60.0, y, 0.12);
    app.settle();
    assert!(
        matches!(
            app.rt.state.rebalance.plan,
            stahl::state::RebalancePlan::Adjusted { .. }
        ),
        "a finger across the thumb moves the class"
    );

    // A finger down the row, away from the thumb, is the page's to scroll: it takes no
    // class with it. (Rune pans the scroll view because only the thumb drags.)
    let before = stahl::view::rebalance::values(&app.rt.state);
    let (rx, ry) = app.rt.find("rebalance-track-0").unwrap();
    // Right of the thumb, which sits just past the middle of the track.
    app.rt.touch(2, TouchPhase::Started, rx + 130.0, ry, 0.0);
    app.rt
        .touch(2, TouchPhase::Moved, rx + 130.0, ry - 80.0, 0.08);
    app.rt
        .touch(2, TouchPhase::Ended, rx + 130.0, ry - 80.0, 0.12);
    app.settle();
    assert_eq!(
        stahl::view::rebalance::values(&app.rt.state),
        before,
        "a finger down the row leaves the plan alone"
    );
}

#[test]
fn new_money_is_placed_by_buying_only_and_an_ambiguous_amount_is_refused() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Rebalance.key());
    app.tap("rebalance-amount");
    app.click("rebalance-amount");
    app.type_text("2000");
    assert_eq!(
        app.rt.state.rebalance.plan,
        stahl::state::RebalancePlan::Flow(2000.0)
    );
    assert!(app.has_text("Invest €2,000.00 by buying only"));
    assert!(app.has_text("Buy €2,000.00"), "the rows say where it goes");
    assert!(app.exists("rebalance-clear"), "there is a way back");

    app.command('a', false);
    app.press(Key::Backspace);
    app.type_text("1.000");
    assert!(
        app.has_text("could mean"),
        "an ambiguous amount is refused, not guessed"
    );

    app.command('a', false);
    app.press(Key::Backspace);
    app.type_text("-500000");
    assert!(app.has_text("That's more than the portfolio."));

    app.command('a', false);
    app.press(Key::Backspace);
    app.type_text("-500");
    assert!(
        app.has_text("Sell €500.00"),
        "a withdrawal sells only: {}",
        stahl::view::rebalance::markdown(&app.rt.state)
    );

    app.click("rebalance-clear");
    assert_eq!(
        app.rt.state.rebalance.plan,
        stahl::state::RebalancePlan::Blend
    );
    assert!(app.has_text("Nothing changes yet"));
}

#[test]
fn the_door_can_be_turned_by_hand_and_by_the_keyboard() {
    use std::f32::consts::PI;

    let mut app = Harness::boot(false, None);
    let (x, y) = app
        .rt
        .find("door-welcome")
        .expect("the welcome screen's door");
    // A hand on the dial: press to the right of its centre and carry it a quarter turn up.
    let radius = 60.0;
    app.rt.pointer_move(x + radius, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(x + radius, y);
    for step in 1..=12 {
        let angle = -(step as f32 / 12.0) * PI / 2.0;
        app.rt
            .pointer_move(x + radius * angle.cos(), y + radius * angle.sin());
        app.rt.frame(1.0 / 60.0);
    }
    app.rt.pointer_up(x, y - radius);
    app.settle();
    assert!(
        app.rt.state.dial_hand < -1.2,
        "the dial followed the hand: {}",
        app.rt.state.dial_hand
    );
    assert!(app.rt.state.dial_drag.is_none(), "the hand let go");

    // The keyboard turns it a notch at a time: the press above gave the door focus.
    let before = app.rt.state.dial_hand;
    app.press(Key::Right);
    app.press(Key::Right);
    assert!(
        app.rt.state.dial_hand > before + 0.4,
        "two notches to the right: {}",
        app.rt.state.dial_hand
    );
    app.press(Key::Left);
    assert!(app.rt.state.dial_hand < before + 0.6, "and one back");

    // From a cold start, Tab reaches the door and the arrows turn it there too.
    let mut cold = Harness::boot(false, None);
    cold.press(Key::Tab);
    cold.press(Key::Right);
    assert!(
        cold.rt.state.dial_hand > 0.2,
        "a keyboard reaches the dial: {}",
        cold.rt.state.dial_hand
    );
}

// --- Counsel ---

#[test]
fn counsel_answers_a_typed_question_with_blocks() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Ask.key());
    assert_eq!(app.rt.state.page, Page::Ask);
    assert!(app.has_text("The door is open, and the light is on."));

    app.click("ask-input");
    app.type_text("What changed this month?");
    app.press(Key::Enter);
    app.settle();
    app.settle();

    let turn = &app.rt.state.ask.turns[0];
    assert_eq!(turn.question, "What changed this month?");
    assert_eq!(turn.phase, stahl::ask::Phase::Done);
    assert!(
        turn.blocks.len() >= 4,
        "the answer is made of several blocks: {:?}",
        turn.blocks
    );
    assert!(
        app.has_text("Bitcoin did most of the work"),
        "the first sentence is on the page"
    );
    assert!(app.exists("ask-0-2-chart"), "the chart block is drawn");
    assert!(
        app.has_text("€107,400.00"),
        "the figure block shows the app's own total"
    );
    assert!(!app.rt.state.thinking(), "nothing is still in flight");
}

#[test]
fn the_first_sentence_arrives_word_by_word() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("What changed this month?");
    let sentence = |app: &Harness| {
        app.rt
            .elements()
            .iter()
            .find(|element| element.key.as_deref() == Some("ask-0-0-say"))
            .and_then(|element| element.text.clone())
            .unwrap_or_default()
    };
    app.rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    // The block lands a third of a second after the question, with its first words only.
    for _ in 0..21 {
        app.rt.frame(1.0 / 60.0);
    }
    let early = sentence(&app);
    let words = early.split_whitespace().count();
    assert!(
        (1..6).contains(&words),
        "the sentence has only started: {early:?}"
    );
    for _ in 0..12 {
        app.rt.frame(1.0 / 60.0);
    }
    let later = sentence(&app);
    assert!(
        later.split_whitespace().count() > words,
        "more words have arrived: {early:?} then {later:?}"
    );
    // And it finishes, as the sentence that was arriving from its start. The day it names is
    // the last one looked at, so it moves with the calendar.
    app.settle();
    let finished = sentence(&app);
    assert!(
        finished.starts_with(&later),
        "what is shown is the sentence from its start: {later:?} then {finished:?}"
    );
    assert!(
        finished.starts_with("Since ")
            && finished.ends_with(
                " you are up €291.41. Bitcoin did most of the work, which will surprise nobody."
            ),
        "{finished:?}"
    );
}

#[test]
fn a_loss_is_said_plainly_and_never_with_the_joke() {
    let mut app = Harness::boot(true, None);
    let change = app
        .rt
        .state
        .change
        .as_mut()
        .expect("the demo looked at this portfolio before");
    change.start = change.end + 300.0;
    app.rt.state.model = Arc::new(stahl::ask::Builtin);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("What changed this month?");
    app.press(Key::Enter);
    app.settle();
    app.settle();
    assert!(
        app.has_text("you are down €300.00"),
        "the loss is stated, without a second sign"
    );
    assert!(
        !app.has_text("surprise nobody"),
        "the joke is only for a gain"
    );
    assert!(!app.has_text("you are up"));
}

#[test]
fn an_empty_page_offers_questions_the_app_can_answer_without_a_model() {
    let mut app = Harness::boot(true, None);
    app.rt.state.model = Arc::new(stahl::ask::Builtin);
    app.click(Page::Ask.key());
    let asked: Vec<String> = (0..6)
        .filter(|index| app.exists(&format!("ask-suggestion-{index}")))
        .filter_map(|index| {
            app.rt
                .elements()
                .iter()
                .find(|element| element.key.as_deref() == Some(&format!("ask-suggestion-{index}")))
                .and_then(|element| element.label.clone())
        })
        .collect();
    assert!(
        (4..=6).contains(&asked.len()),
        "four to six questions: {asked:?}"
    );
    assert!(
        asked.iter().any(|question| question.starts_with("What if")),
        "a what-if is one of them: {asked:?}"
    );

    // The what-if is answered by the app itself: the shock is the question's, the rest is the
    // app's arithmetic.
    let at = asked
        .iter()
        .position(|question| question.starts_with("What if"))
        .expect("a what-if");
    app.click(&format!("ask-suggestion-{at}"));
    app.settle();
    app.settle();
    assert!(
        app.has_text("here is what it would leave"),
        "the app answers a what-if with no model"
    );
    assert!(app.exists("ask-0-1-toggle"), "and the change can be played");
    assert!(app.exists("ask-0-1-bar"));
}

#[test]
fn thinking_is_true_from_sending_until_the_answer_finishes() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("How is Bitcoin doing?");
    app.rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    app.rt.frame(1.0 / 60.0);
    assert!(app.rt.state.thinking(), "the app says it is working");
    assert!(app.exists("ask-stop"), "there is a way to stop it");
    app.settle();
    app.settle();
    assert!(!app.rt.state.thinking());
    assert!(
        app.exists("ask-send"),
        "the input asks again when it is done"
    );
}

#[test]
fn escape_stops_an_answer_in_flight() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("What changed this month?");
    app.rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    for _ in 0..20 {
        app.rt.frame(1.0 / 60.0);
    }
    assert!(app.rt.state.ask.streaming, "the first block has landed");
    let shown = app.rt.state.ask.turns[0].blocks.len();
    assert!((1..5).contains(&shown), "the answer was still arriving");

    app.press(Key::Escape);
    assert_eq!(app.rt.state.ask.turns[0].phase, stahl::ask::Phase::Stopped);
    assert!(!app.rt.state.thinking());
    assert!(app.has_text("Stopped. Ask again whenever you like."));
    // Nothing else arrives after the stop.
    let stopped = app.rt.state.ask.turns[0].blocks.len();
    for _ in 0..90 {
        app.rt.frame(1.0 / 60.0);
    }
    assert_eq!(app.rt.state.ask.turns[0].blocks.len(), stopped);
}

/// A model that writes a figure itself, to check what the page does with it.
struct MadeUp;

impl stahl::ask::Model for MadeUp {
    fn ask(&self, _request: stahl::ask::Request) -> stahl::ask::Reply {
        stahl::ask::Reply::Ready(
            r#"{"block":"say","text":"Crypto is 31.4% of the portfolio."}"#.to_string(),
        )
    }
}

/// A model that only quotes the fact table.
struct Computed;

impl stahl::ask::Model for Computed {
    fn ask(&self, _request: stahl::ask::Request) -> stahl::ask::Reply {
        stahl::ask::Reply::Ready(
            r#"{"block":"say","text":"Crypto is {class.crypto.share} of the portfolio."}"#
                .to_string(),
        )
    }
}

#[test]
fn a_figure_the_model_made_up_is_marked_unverified() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Ask.key());
    app.click("ask-input");
    let help = |app: &Harness, key: &str| {
        app.rt
            .elements()
            .iter()
            .find(|element| element.key.as_deref() == Some(key))
            .and_then(|element| element.help.clone())
            .unwrap_or_default()
    };

    app.rt.state.model = Arc::new(MadeUp);
    app.type_text("How much is in crypto?");
    app.press(Key::Enter);
    app.settle();
    app.settle();
    assert!(app.has_text("Crypto is 31.4% of the portfolio."));
    assert!(
        help(&app, "ask-0-0-say").contains("Stahl didn't compute"),
        "the invented figure is marked unverified"
    );

    app.rt.state.model = Arc::new(Computed);
    app.click("ask-input");
    app.type_text("And again?");
    app.press(Key::Enter);
    app.settle();
    app.settle();
    assert!(app.has_text("Crypto is 25.1% of the portfolio."));
    assert!(
        help(&app, "ask-1-0-say").is_empty(),
        "a figure from the fact table is not marked"
    );
}

#[test]
fn the_what_if_toggle_shows_today_and_what_the_shock_leaves() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("What if crypto halves?");
    app.press(Key::Enter);
    app.settle();
    app.settle();

    // The answer settles on what the shock would leave.
    assert!(app.has_text("What would be left"));
    assert!(app.has_text("€93,900.00"), "the shocked total");
    assert!(app.has_text("€13,500.00"), "the difference beside it");

    // "Now" goes back to today.
    app.click("ask-0-1-now");
    assert!(app.has_text("€107,400.00"), "today's total");
    assert!(!app.has_text("€13,500.00"), "no difference to state");
    assert!(
        app.rt
            .state
            .ask
            .what_if_at(0, 1)
            .is_some_and(|what_if| what_if.now)
    );

    // The right arrow goes forward again, the left one back: the toggle has the keyboard.
    app.press(Key::Right);
    assert!(app.has_text("€93,900.00"));
    app.press(Key::Left);
    assert!(app.has_text("€107,400.00"));

    // And the details table is a disclosure, closed until asked for.
    assert!(!app.exists("ask-0-1-details-body"));
    app.click("ask-0-1-details");
    assert!(app.exists("ask-0-1-details-body"));
    assert!(
        app.has_text("€27,000.00"),
        "the table has what crypto is worth today"
    );
    app.click("ask-0-1-details");
    assert!(!app.exists("ask-0-1-details-body"));
}

#[test]
fn dragging_the_plan_in_an_answer_changes_its_trades_and_carries_the_fraction_over() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("Why is my allocation off?");
    app.press(Key::Enter);
    app.settle();
    app.settle();
    assert!(app.has_text("50% of the way to your targets"));

    // Drag the answer's own slider to about 80% of the way.
    let (x, y) = app.rt.find("ask-0-3-rail").expect("the slider is there");
    let width = app
        .rt
        .state
        .ask
        .plan_at(0, 3)
        .map_or(0.0, |play| play.width);
    assert!(width > 100.0, "the slider measured itself: {width}");
    let want = x + width * 0.3;
    app.rt.pointer_move(x, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(x, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_move(want, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_up(want, y);
    app.settle();

    let fraction = app
        .rt
        .state
        .ask
        .plan_at(0, 3)
        .and_then(|play| play.fraction)
        .expect("the slider took the drag");
    assert!((0.7..0.9).contains(&fraction), "{fraction}");
    let percent = (fraction * 100.0).round() as i64;
    assert!(
        app.has_text(&format!("{percent}% of the way to your targets")),
        "the plan is worked out again at the new fraction"
    );
    assert!(!app.has_text("50% of the way to your targets"));

    // And the Rebalance page opens at the fraction the owner dragged to.
    app.click("ask-0-3-open-plan");
    assert_eq!(app.rt.state.page, Page::Rebalance);
    assert!((app.rt.state.rebalance.fraction - fraction).abs() < 1e-9);
    assert!(app.has_text(&format!("Go {percent}% of the way")));
}

#[test]
fn a_plan_in_an_answer_opens_in_rebalance_as_the_same_plan() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("Why is my allocation off?");
    app.press(Key::Enter);
    app.settle();
    app.settle();

    app.click("ask-0-3-open-plan");
    assert_eq!(app.rt.state.page, Page::Rebalance);
    assert_eq!(
        app.rt.state.rebalance.plan,
        stahl::state::RebalancePlan::Blend
    );
    assert!(
        (app.rt.state.rebalance.fraction - 0.5).abs() < 1e-9,
        "half the way, as the answer said"
    );
    assert!(app.has_text("Go 50% of the way"));
}

#[test]
fn the_palette_asks_counsel_what_was_typed() {
    let mut app = Harness::boot(true, None);
    app.command('k', false);
    app.type_text("what changed this month?");
    assert!(
        app.has_text("Ask counsel: what changed this month?"),
        "the typed question is the first row"
    );
    app.press(Key::Enter);
    assert_eq!(app.rt.state.page, Page::Ask);
    assert_eq!(app.rt.state.ask.turns.len(), 1);
    app.settle();
    app.settle();
    assert!(app.has_text("Bitcoin did most of the work"));
}

#[test]
fn the_overview_line_asks_what_was_typed_there() {
    let mut app = Harness::boot(true, None);
    app.click("overview-ask");
    app.type_text("How is Bitcoin doing?");
    app.press(Key::Enter);
    assert_eq!(app.rt.state.page, Page::Ask);
    app.settle();
    app.settle();
    assert!(app.has_text("Bitcoin is €27,000.00"));
}

#[test]
fn a_suggested_question_on_the_overview_asks_it() {
    let mut app = Harness::boot(true, None);
    assert!(app.exists("overview-ask-0"));
    app.click("overview-ask-0");
    assert_eq!(app.rt.state.page, Page::Ask);
    assert_eq!(app.rt.state.ask.turns.len(), 1);
    assert_eq!(
        app.rt.state.ask.turns[0].question,
        "Why is Crypto off its target?"
    );
    app.settle();
    app.settle();
    assert!(app.has_text("Because prices moved and you did not."));
}

#[test]
fn a_question_to_a_provider_asks_before_it_goes() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Settings.key());
    app.click("settings-url");
    app.type_text("https://api.example.com/v1");
    app.click("settings-model");
    app.type_text("a-model");
    app.click("settings-save");

    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("What changed this month?");
    app.press(Key::Enter);
    assert!(app.rt.state.consent.is_some(), "the prompt asks first");
    assert!(app.exists("consent-panel"));
    assert!(app.has_text("api.example.com"));
    assert!(
        app.has_text("Authorization header"),
        "the key is not in the body, so the sheet says where it goes"
    );

    app.click("consent-show");
    assert!(
        app.has_text("You are Counsel"),
        "the exact system prompt is shown"
    );
    assert!(app.has_text("\"stream\": true"));

    app.click("consent-cancel");
    assert!(app.rt.state.consent.is_none());
    assert_eq!(
        app.rt.state.ask.input.text, "What changed this month?",
        "the question goes back in the input"
    );
    assert!(app.rt.state.ask.turns.is_empty(), "nothing was sent");
    assert!(
        !app.dir.path().join("ask-consent.json").exists(),
        "nothing was remembered"
    );
}

#[test]
fn a_remote_provider_must_be_https() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Settings.key());
    app.click("settings-url");
    app.type_text("http://example.com/v1");
    app.click("settings-save");
    app.settle();
    let error = app.rt.state.settings.error.clone().expect("refused");
    assert!(error.contains("https"), "{error}");
    assert!(app.has_text("example.com would be reached without encryption"));
    assert_eq!(
        app.rt.state.backend.llm_now().map(|llm| llm.url),
        None,
        "nothing was saved"
    );
}

#[test]
fn a_question_to_this_computer_needs_no_prompt() {
    let mut app = Harness::boot(true, None);
    app.click(Page::Settings.key());
    app.click("settings-url");
    app.type_text("http://127.0.0.1:11434/v1");
    app.click("settings-model");
    app.type_text("a-model");
    app.click("settings-save");

    app.click(Page::Ask.key());
    assert!(
        app.has_text("on this computer"),
        "the line under the input says where it goes"
    );
    app.click("ask-input");
    app.type_text("What changed this month?");
    app.press(Key::Enter);
    assert!(
        app.rt.state.consent.is_none(),
        "a provider on this machine is not asked about"
    );
    // It goes straight out: nothing to allow, and a turn is on its way.
    app.settle();
    assert_eq!(app.rt.state.ask.turns.len(), 1);
    assert_eq!(
        app.rt.state.ask.turns[0].question,
        "What changed this month?"
    );
}

#[test]
fn without_a_model_the_app_answers_what_it_knows() {
    let mut app = Harness::boot(true, None);
    app.rt.state.model = Arc::new(stahl::ask::Builtin);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("How much is in crypto?");
    app.press(Key::Enter);
    app.settle();
    app.settle();
    assert!(app.has_text("Crypto is €27,000.00"));
    assert!(app.has_text("or 25.1% of the portfolio"));
    assert!(
        !app.has_text("needs a model"),
        "a figure the app knows needs no model"
    );
    assert!(app.exists("ask-0-1-figure-0"), "the figures are drawn");
}

#[test]
fn without_a_model_an_unknown_question_says_how_to_get_one() {
    let mut app = Harness::boot(true, None);
    app.rt.state.model = Arc::new(stahl::ask::Builtin);
    app.click(Page::Ask.key());
    app.click("ask-input");
    app.type_text("Should I buy a house?");
    app.press(Key::Enter);
    app.settle();
    app.settle();
    assert!(app.has_text("needs a model"));
    assert!(app.has_text("local one keeps everything on this computer"));
    app.click("ask-0-1-go-0");
    assert_eq!(app.rt.state.page, Page::Settings);
}

#[test]
fn a_new_record_marks_the_sidebar_until_the_wall_is_seen() {
    let mut app = Harness::boot(true, None);
    assert!(
        app.rt.state.records_unseen > 0,
        "a portfolio with history has plates the owner has not been shown"
    );
    let seen = app
        .dir
        .path()
        .join("portfolio/portfolio/history/main/records-seen.json");
    assert!(
        !seen.exists(),
        "nothing is written before the wall is opened"
    );

    app.click("nav-records");
    assert_eq!(app.rt.state.page, Page::Records);
    assert_eq!(app.rt.state.records_unseen, 0, "the mark goes once seen");
    assert!(
        app.rt.state.record_arrival.is_some(),
        "the newest plate is set onto the wall once"
    );
    assert!(app.has_text("newest first"));

    // What was shown is remembered beside the history, by id, with no amounts in it.
    let written = fs::read_to_string(&seen).unwrap();
    assert!(written.contains(":begins"), "{written}");
    assert!(!written.contains('\u{20ac}'), "{written}");

    // Coming back, the wall is simply there: no second ceremony, no mark.
    app.click(Page::Overview.key());
    app.click("nav-records");
    assert_eq!(app.rt.state.records_unseen, 0);
    assert!(app.rt.state.record_arrival.is_none());
}

#[test]
fn a_plate_opens_to_its_moment_and_closes() {
    let mut app = Harness::boot(true, None);
    app.click("nav-records");
    let plates = app.rt.state.records.data.clone().expect("the wall");
    assert!(plates.len() >= 2, "the demo has a wall: {}", plates.len());
    let id = plates[0].id.clone();

    app.click(&format!("plate-{id}"));
    assert_eq!(app.rt.state.record_plate.as_deref(), Some(id.as_str()));
    assert!(app.exists("record-back"), "a way back to the wall");
    assert!(app.exists("record-chart"), "the moment, on the chart");

    app.press(Key::Escape);
    assert!(app.rt.state.record_plate.is_none());
    assert!(app.exists(&format!("plate-{id}")), "the plate is back");
}

// --- Time ---

/// The strip's left edge and middle height, in window points.
fn strip_start(app: &Harness) -> (f32, f32) {
    let (x, y) = app
        .rt
        .find("time-strip")
        .expect("the time strip is in the toolbar");
    (x - stahl::time::STRIP / 2.0, y)
}

#[test]
fn dragging_the_time_strip_turns_the_whole_app_back_and_escape_returns() {
    let mut app = Harness::boot(true, None);
    assert!(app.has_text("€107,400.00"), "today's total");
    let days = app.rt.state.time.days.len();
    assert!(days >= 60, "the demo's history is read: {days}");
    let (left, y) = strip_start(&app);
    let (x, _) = app.rt.find("time-strip").unwrap();
    app.rt.pointer_down(x + stahl::time::STRIP / 2.0 - 1.0, y);
    app.rt.frame(1.0 / 60.0);
    for step in 1..=8 {
        app.rt
            .pointer_move(x + stahl::time::STRIP / 2.0 - step as f32 * 30.0, y);
        app.rt.frame(1.0 / 60.0);
    }
    app.rt.pointer_move(left, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_up(left, y);
    app.settle();
    let state = &app.rt.state;
    assert_eq!(
        state.time.at,
        Some(0),
        "the far left is the first recorded day"
    );
    let first = state.time.days[0].total;
    let total = state.summary.as_ref().unwrap().total_value;
    assert!((total - first).abs() < 0.01, "the total is that day's");
    assert!(!app.has_text("€107,400.00"), "today's total is gone");
    assert!(app.exists("time-band"), "the band says it is the past");
    assert!(
        app.has_text("you held"),
        "the statement speaks about that day"
    );
    // The allocation follows too.
    let shares: f64 = state.allocation.iter().map(|item| item.percent).sum();
    assert!((shares - 100.0).abs() < 0.01);

    app.press(Key::Escape);
    assert!(!app.rt.state.in_past(), "escape returns to today");
    assert!(app.has_text("€107,400.00"));
    assert!(!app.exists("time-band"));
}

#[test]
fn the_keyboard_walks_through_recorded_days() {
    let mut app = Harness::boot(true, None);
    let days = app.rt.state.time.days.len();
    let (x, y) = app.rt.find("time-strip").unwrap();
    // Focus the strip without moving time: a press at today.
    app.rt.pointer_down(x + stahl::time::STRIP / 2.0 - 1.0, y);
    app.rt.pointer_up(x + stahl::time::STRIP / 2.0 - 1.0, y);
    app.settle();
    assert!(!app.rt.state.in_past());
    app.press(Key::Left);
    assert_eq!(
        app.rt.state.time.at,
        Some(days - 1),
        "one recorded day back"
    );
    app.rt.key(KeyEvent {
        key: Key::Left,
        mods: Mods {
            shift: true,
            ..Mods::default()
        },
    });
    app.settle();
    assert_eq!(
        app.rt.state.time.at,
        Some(days - 8),
        "a week more with shift"
    );
    app.press(Key::End);
    assert!(!app.rt.state.in_past(), "End is today");
    // And from anywhere, with the main modifier and the brackets.
    app.command('[', false);
    assert_eq!(app.rt.state.time.at, Some(days - 1));
    app.command(']', false);
    assert!(!app.rt.state.in_past());
}

#[test]
fn turning_the_door_back_turns_time_back() {
    let mut app = Harness::boot(true, None);
    let days = app.rt.state.time.days.len();
    let (cx, cy) = app.rt.find("door-sidebar").unwrap();
    // A quarter turn the other way round from the right to the top: six notches back.
    app.rt.pointer_down(cx + 10.0, cy);
    app.rt.frame(1.0 / 60.0);
    for step in 1..=12 {
        let angle = -(step as f32) / 12.0 * std::f32::consts::FRAC_PI_2;
        app.rt
            .pointer_move(cx + 10.0 * angle.cos(), cy + 10.0 * angle.sin());
        app.rt.frame(1.0 / 60.0);
    }
    app.rt.pointer_up(cx, cy - 10.0);
    app.settle();
    let at = app.rt.state.time.at.expect("the door turned time back");
    assert!(
        (days - 7..=days - 5).contains(&at),
        "a notch a day: {at} of {days}"
    );
    assert!(app.exists("time-band"));
}

#[test]
fn a_page_about_today_says_so_in_the_past() {
    let mut app = Harness::boot(true, None);
    app.command('[', false);
    app.click(Page::Rebalance.key());
    assert!(app.has_text("Rebalance is about today"));
    assert!(!app.has_text("I have this much to invest"), "no stale plan");
    app.click("time-now-only-today");
    assert!(!app.rt.state.in_past());
    assert!(app.has_text("I have this much to invest"));
}

#[test]
fn editing_in_the_past_happens_today() {
    let mut app = Harness::boot(true, None);
    app.command('[', false);
    assert!(app.rt.state.in_past());
    app.command('n', false);
    assert!(app.rt.state.form.is_some(), "the form opens");
    assert!(!app.rt.state.in_past(), "for today's positions");
}

// --- Matters for signature ---

/// Press and hold a Sign control for `seconds`, driving real frames between press and release.
fn hold(app: &mut Harness, key: &str, seconds: f64) {
    let (x, y) = app
        .rt
        .find(key)
        .unwrap_or_else(|| panic!("nothing to hold at {key}"));
    app.rt.pointer_move(x, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(x, y);
    for _ in 0..((seconds * 60.0).round() as usize) {
        app.rt.frame(1.0 / 60.0);
    }
    app.rt.pointer_up(x, y);
    // One frame shows the hint or the seal; the write lands without running the breath away.
    app.rt.frame(1.0 / 60.0);
    app.rt.state.settle();
}

fn decision_files(app: &Harness) -> Vec<PathBuf> {
    let dir = app.dir.path().join("portfolio/portfolio/decisions");
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect()
}

#[test]
fn a_held_sign_records_a_decision_and_documents_lists_it() {
    let mut app = Harness::boot_tall(true);
    assert!(
        app.exists("matter-rebalance"),
        "the bank prepared a rebalance"
    );
    assert!(app.has_text("Stahl never trades."));

    hold(&mut app, "matter-sign-rebalance", 1.2);

    let ledger = &app.rt.state.matters.ledger;
    assert_eq!(ledger.entries.len(), 1, "one decision recorded");
    assert_eq!(ledger.entries[0].status, stahl::matters::Status::Signed);
    assert_eq!(ledger.entries[0].id, "rebalance");

    // The card is stamped, then files itself away.
    assert!(app.exists("matter-seal"), "the seal is on the card");
    assert!(app.has_text("Signed"));
    assert!(
        !app.exists("matter-sign-rebalance"),
        "the signed card has no Sign control while its seal shows"
    );
    for _ in 0..150 {
        app.rt.frame(1.0 / 60.0);
    }
    app.settle();
    assert!(
        !app.exists("matter-rebalance"),
        "the signed card has filed itself away"
    );
    assert!(app.has_text("Nothing needs your signature today."));

    // The record is on disk, in the shape the Documents page reads.
    let files = decision_files(&app);
    assert_eq!(files.len(), 1, "{files:?}");
    let written = fs::read_to_string(&files[0]).unwrap();
    assert!(written.starts_with("# Decision: Rebalance halfway to your targets\n"));
    assert!(written.contains("Status: Signed"));
    assert!(written.contains("## Decision"));
    assert!(written.contains("Stahl never trades."));

    // Documents lists it, newest first, and opens it.
    app.click(Page::Documents.key());
    assert!(app.has_text("Decision records"));
    assert!(app.has_text("Rebalance halfway to your targets"));
    assert!(app.has_text("Signed"));
    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    app.click(&format!("decision-{name}"));
    assert!(app.rt.state.matters.open_decision.is_some());
    assert!(app.has_text("Context"), "the record opens as Markdown");
    assert!(app.has_text("Rationale"));
}

#[test]
fn an_early_release_signs_nothing_and_explains_the_hold() {
    let mut app = Harness::boot_tall(true);
    hold(&mut app, "matter-sign-rebalance", 0.3);
    assert!(app.rt.state.matters.ledger.entries.is_empty());
    assert!(decision_files(&app).is_empty());
    assert!(app.has_text("Hold to sign."));
    assert!(app.exists("matter-rebalance"), "the matter is still there");
}

#[test]
fn the_keyboard_holds_the_same_way() {
    let mut app = Harness::boot_tall(true);
    // A click focuses the control and lets go at once; Return then starts the hold.
    let (x, y) = app
        .rt
        .find("matter-sign-rebalance")
        .expect("the Sign control");
    app.rt.pointer_move(x, y);
    app.rt.frame(1.0 / 60.0);
    app.rt.pointer_down(x, y);
    app.rt.pointer_up(x, y);
    app.rt.frame(1.0 / 60.0);
    assert!(app.rt.state.matters.hold.is_none());
    app.rt.key(KeyEvent {
        key: Key::Enter,
        mods: Mods::default(),
    });
    app.rt.frame(1.0 / 60.0);
    assert!(
        app.rt.state.matters.hold.is_some(),
        "Return holds the control"
    );
    for _ in 0..75 {
        app.rt.frame(1.0 / 60.0);
    }
    app.settle();
    assert_eq!(
        app.rt
            .state
            .matters
            .ledger
            .entries
            .first()
            .map(|entry| entry.status),
        Some(stahl::matters::Status::Signed)
    );
}

#[test]
fn a_declined_matter_stays_hidden_across_a_reload() {
    let mut app = Harness::boot_tall(true);
    app.click("matter-decline-rebalance");
    app.click("matter-reason-rebalance");
    app.type_text("Not this month.");
    app.click("matter-decline-yes-rebalance");
    assert!(app.rt.state.matters.ledger.entries[0].status == stahl::matters::Status::Declined);
    assert!(
        !app.exists("matter-rebalance"),
        "declined matters leave the list"
    );
    // The alternative allocation matter is still there: declining is not signing.
    assert!(app.exists("matter-cash-place"));

    let files = decision_files(&app);
    assert_eq!(files.len(), 1);
    let written = fs::read_to_string(&files[0]).unwrap();
    assert!(written.contains("Status: Declined"));
    assert!(written.contains("Not this month."));

    // Reloading the same workspace keeps it hidden: the ledger is beside the history.
    let dir = app.rt.state.open.workspace_dir.clone().unwrap();
    app.rt.state.load_workspace(dir);
    app.settle();
    assert!(!app.exists("matter-rebalance"));
    assert!(app.rt.state.matters.ledger.entries[0].status == stahl::matters::Status::Declined);
}

#[test]
fn a_material_change_brings_a_declined_matter_back() {
    let mut app = Harness::boot_tall(true);
    app.click("matter-decline-rebalance");
    app.click("matter-decline-yes-rebalance");
    assert!(!app.exists("matter-rebalance"));

    // Crypto drifts four more points: past its three-point tolerance.
    app.rt.update(|state, _| {
        if let Some(review) = &mut state.review.data {
            for class in &mut review.allocations {
                if class.asset_class == "Crypto" {
                    class.actual_percent += 4.0;
                    class.drift_percent += 4.0;
                }
            }
        }
    });
    app.settle();
    assert!(
        app.exists("matter-rebalance"),
        "a material change reopens it"
    );
}

#[test]
fn signing_without_a_workspace_explains_that_records_need_one() {
    let app = Harness::boot(false, Some(demo::POSITIONS));
    // A single file has no policy, so there is nothing to sign; the section says so instead.
    assert!(app.has_text("Nothing needs your signature today."));
    assert!(!app.exists("matter-sign-rebalance"));
}

#[test]
fn reduced_motion_asks_plainly_instead_of_holding() {
    let mut app = Harness::boot_tall(true);
    app.rt.set_system_reduced_motion(true);
    app.settle();
    // The control is a plain press now: no hold, a question.
    app.click("matter-sign-rebalance");
    assert!(app.rt.state.matters.hold.is_none());
    assert!(app.exists("matter-confirm-sign"));
    app.press(Key::Escape);
    assert!(
        !app.exists("matter-confirm-sign"),
        "Escape closes the question"
    );
    app.click("matter-sign-rebalance");
    app.click("matter-confirm-sign");
    assert_eq!(
        app.rt
            .state
            .matters
            .ledger
            .entries
            .first()
            .map(|entry| entry.status),
        Some(stahl::matters::Status::Signed)
    );
}
