//! End-to-end tests: the real views and handlers on real `portfolio_rs` data with fixture
//! quotes, driven through pointer and keyboard input in Rune's headless runtime.
//!
//! These never touch the user's config: the backend points at a temp directory, and opening goes
//! through `startup`, which doesn't write it.

use std::{env, fs, path::Path, sync::Once, thread, time::Duration};

use chrono::Local;
use rune::{Key, KeyEvent, Mods, Runtime};
use stahl::{
    app,
    backend::{Backend, Period},
    demo,
    state::{Confirm, Page, State},
};
use tempfile::TempDir;
use tokio::runtime::Runtime as Tokio;

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
        (Page::Allocation, "Compared with your policy targets"),
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
    assert!(app.has_text("Amount must be a number."));
    assert_eq!(app.rt.state.positions.len(), 5);
    assert!(!app.rt.state.dirty);
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
