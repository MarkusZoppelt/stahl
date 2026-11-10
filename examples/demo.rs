//! Stahl on a made-up portfolio, with no network and none of your data. Good for trying it,
//! and for rendering any screen without a window:
//!
//! ```sh
//! cargo run --example demo
//! cargo run --example demo -- --snap holdings.png --scene holdings --at 2
//! cargo run --example demo -- --list-scenes
//! ```

use std::{env, fs, process, time::SystemTime};

use portfolio_rs::config::ThemeMode;
use stahl::{
    app, demo,
    state::{Confirm, FolderPrompt, Page, PendingOpen, State, UpdatePhase},
    update::Installation,
};
use tokio::runtime::Builder;

fn main() {
    let stamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let dir = env::temp_dir().join(format!("stahl-demo-{}-{stamp}", process::id()));
    let workspace = dir.join("portfolio");
    demo::seed_workspace(&workspace);

    // Parts of portfolio_rs resolve their own quote provider from this.
    let quotes = dir.join("quotes.json");
    demo::write_quotes(&quotes);
    // SAFETY: before any other thread exists.
    unsafe {
        env::set_var(
            "PORTFOLIO_RS_QUOTES",
            format!("fixture:{}", quotes.display()),
        )
    };

    let runtime = Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to start the Tokio runtime");
    let backend = demo::backend(&dir, Some(&workspace), runtime.handle().clone());

    fn scene(s: &mut State, page: Page) {
        s.startup();
        s.settle();
        s.navigate(page);
        s.settle();
    }
    /// What someone sees on first opening the app, with nothing to reopen.
    fn first_run(s: &mut State) {
        s.reset();
        s.settle();
        s.starting = false;
    }
    /// A form that shows no real path, with no name yet.
    fn example_form(s: &mut State) {
        if let Some(form) = &mut s.new_workspace {
            form.location.replace_text("/Users/you/Documents");
        }
    }
    /// A page of a new workspace with nothing in it yet.
    fn empty(s: &mut State, page: Page) {
        s.startup();
        s.settle();
        let dir = env::temp_dir()
            .join(format!("stahl-empty-{}", process::id()))
            .join("My Portfolio");
        demo::seed_empty_workspace(&dir);
        s.load_workspace(dir.to_string_lossy().into_owned());
        s.settle();
        s.navigate(page);
        s.settle();
    }
    /// A newer release waiting, as if a check at startup had found it.
    fn update_scene(s: &mut State, installation: Installation, dialog: bool) {
        scene(s, Page::Overview);
        s.updates.updater = demo::updater(installation);
        s.check_for_updates(true);
        s.settle();
        s.updates.dialog = dialog;
    }
    /// A page in the phone layout. Render these at an iPhone's size: `--size 390x844`.
    fn phone(s: &mut State, page: Page) {
        s.compact = true;
        scene(s, page);
    }
    fn mac() -> Installation {
        Installation::MacApp {
            bundle: "/Applications/Stahl Capital.app".into(),
        }
    }
    let result = app::run_with_scenes(
        backend,
        env::args().skip(1).collect(),
        &[
            ("overview", |s| scene(s, Page::Overview)),
            ("holdings", |s| scene(s, Page::Holdings)),
            ("holdings-map", |s| {
                scene(s, Page::Holdings);
                s.holdings_map = true;
            }),
            ("holdings-map-light", |s| {
                scene(s, Page::Holdings);
                s.holdings_map = true;
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("holdings-light", |s| {
                scene(s, Page::Holdings);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("holding-details", |s| {
                scene(s, Page::Holdings);
                s.select_position(0);
            }),
            ("sidebar-collapsed", |s| {
                scene(s, Page::Holdings);
                s.sidebar.toggle();
            }),
            ("allocation", |s| scene(s, Page::Allocation)),
            ("performance", |s| scene(s, Page::Performance)),
            ("performance-span", |s| {
                scene(s, Page::Performance);
                s.measure.span = Some((2, 14));
            }),
            ("performance-span-light", |s| {
                scene(s, Page::Performance);
                s.measure.span = Some((2, 14));
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("review", |s| scene(s, Page::Review)),
            ("rebalance", |s| scene(s, Page::Rebalance)),
            ("policy", |s| scene(s, Page::Policy)),
            ("documents", |s| {
                scene(s, Page::Documents);
                s.generate_report(true);
                s.settle();
            }),
            ("briefing", |s| scene(s, Page::Briefing)),
            ("health", |s| scene(s, Page::Health)),
            ("settings", |s| scene(s, Page::Settings)),
            ("palette", |s| {
                scene(s, Page::Overview);
                s.open_palette();
            }),
            ("add-position", |s| {
                scene(s, Page::Holdings);
                s.open_form(None);
            }),
            ("add-position-invalid", |s| {
                scene(s, Page::Holdings);
                s.open_form(None);
                let form = s.form.as_mut().expect("the form just opened");
                form.quantity.replace_text("1,000");
                s.submit_form();
            }),
            ("edit-position", |s| {
                scene(s, Page::Holdings);
                s.open_form(Some(4));
            }),
            ("edit-position-purchases", |s| {
                scene(s, Page::Holdings);
                s.open_form(Some(0));
            }),
            ("welcome", first_run),
            ("new-workspace", |s| {
                first_run(s);
                s.open_new_workspace();
                example_form(s);
                if let Some(form) = &mut s.new_workspace {
                    form.name.replace_text("My Portfolio");
                }
            }),
            ("new-workspace-problem", |s| {
                first_run(s);
                s.open_new_workspace();
                example_form(s);
                s.submit_new_workspace();
                s.settle();
            }),
            ("guide", |s| {
                scene(s, Page::Holdings);
                s.toggle_guide();
            }),
            ("guide-first-run", |s| {
                first_run(s);
                s.toggle_guide();
            }),
            ("not-a-workspace", |s| {
                scene(s, Page::Overview);
                s.folder_prompt = Some(FolderPrompt {
                    dir: "/tmp/empty-folder".into(),
                    empty: true,
                    error: None,
                });
            }),
            ("unsaved-changes", |s| {
                scene(s, Page::Overview);
                s.dirty = true;
                s.confirm = Some(Confirm::Unsaved(PendingOpen::Workspace));
            }),
            ("parse-failure", |s| {
                s.startup();
                s.settle();
                let file = env::temp_dir().join(format!("stahl-broken-{}.json", process::id()));
                fs::write(&file, "[\n  {\"Name\": \"Cash\",,}\n]").expect("temp dir is writable");
                s.load_file(file.to_string_lossy().into_owned());
                s.settle();
            }),
            ("empty-overview", |s| empty(s, Page::Overview)),
            ("empty-holdings", |s| empty(s, Page::Holdings)),
            ("empty-allocation", |s| empty(s, Page::Allocation)),
            ("empty-performance", |s| empty(s, Page::Performance)),
            ("empty-review", |s| empty(s, Page::Review)),
            ("empty-rebalance", |s| empty(s, Page::Rebalance)),
            ("empty-policy", |s| empty(s, Page::Policy)),
            ("empty-documents", |s| empty(s, Page::Documents)),
            ("empty-briefing", |s| empty(s, Page::Briefing)),
            ("empty-health", |s| empty(s, Page::Health)),
            ("privacy", |s| {
                scene(s, Page::Overview);
                s.privacy = true;
            }),
            ("phone-overview", |s| phone(s, Page::Overview)),
            ("phone-holdings", |s| phone(s, Page::Holdings)),
            ("phone-holdings-map", |s| {
                phone(s, Page::Holdings);
                s.holdings_map = true;
            }),
            ("phone-holding-details", |s| {
                phone(s, Page::Holdings);
                s.select_position(0);
            }),
            ("phone-allocation", |s| phone(s, Page::Allocation)),
            ("phone-performance", |s| phone(s, Page::Performance)),
            ("phone-review", |s| phone(s, Page::Review)),
            ("phone-rebalance", |s| phone(s, Page::Rebalance)),
            ("phone-policy", |s| phone(s, Page::Policy)),
            ("phone-documents", |s| {
                phone(s, Page::Documents);
                s.generate_report(true);
                s.settle();
            }),
            ("phone-briefing", |s| phone(s, Page::Briefing)),
            ("phone-health", |s| phone(s, Page::Health)),
            ("phone-settings", |s| phone(s, Page::Settings)),
            ("phone-more", |s| {
                phone(s, Page::Overview);
                s.more = true;
            }),
            ("phone-add-position", |s| {
                phone(s, Page::Holdings);
                s.open_form(None);
            }),
            ("phone-palette", |s| {
                phone(s, Page::Overview);
                s.open_palette();
            }),
            ("phone-guide", |s| {
                phone(s, Page::Overview);
                s.toggle_guide();
            }),
            ("phone-welcome", |s| {
                s.compact = true;
                first_run(s);
            }),
            ("phone-light", |s| {
                phone(s, Page::Overview);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("light", |s| {
                scene(s, Page::Overview);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("update-banner", |s| update_scene(s, mac(), false)),
            ("update-dialog", |s| update_scene(s, mac(), true)),
            ("update-installing", |s| {
                update_scene(s, mac(), true);
                s.updates.phase = UpdatePhase::Installing;
            }),
            ("update-installed", |s| {
                update_scene(s, mac(), true);
                s.updates.phase = UpdatePhase::Installed;
            }),
            ("update-failed", |s| {
                update_scene(s, mac(), true);
                s.updates.phase = UpdatePhase::Failed(
                    "the download doesn't match the release's checksum".to_string(),
                );
            }),
            ("update-nix", |s| update_scene(s, Installation::Nix, true)),
            ("update-deb", |s| update_scene(s, Installation::Deb, true)),
            ("update-deb-saved", |s| {
                update_scene(s, Installation::Deb, true);
                s.updates.phase =
                    UpdatePhase::Saved("/home/me/Downloads/stahl_0.3.0_amd64.deb".into());
            }),
        ],
    );
    let _ = fs::remove_dir_all(&dir);
    if let Err(error) = result {
        eprintln!("error: {error}");
        process::exit(1);
    }
}
