//! End-to-end tests: the real views and handlers on real `portfolio_rs` data with fixture
//! quotes, driven through pointer and keyboard input in Rune's headless runtime.
//!
//! These never touch the user's config: the backend points at a temp directory, and opening goes
//! through `startup`, which doesn't write it.

use std::{
    env, fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Once,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::Duration,
};

use chrono::Local;
use rune::{Key, KeyEvent, Mods, Runtime};
use stahl::{
    app,
    backend::{Backend, Period},
    demo::{self, FakeReleases},
    state::{Confirm, Page, State, UpdatePhase},
    update::{self, Installation, Updater},
};
use tempfile::TempDir;
use tokio::runtime::Runtime as Tokio;
use zip::{ZipWriter, write::SimpleFileOptions};

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
