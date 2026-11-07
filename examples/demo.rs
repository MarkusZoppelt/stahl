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
    state::{Page, State},
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
    app::run_with_scenes(
        backend,
        env::args().skip(1).collect(),
        &[
            ("overview", |s| scene(s, Page::Overview)),
            ("holdings", |s| scene(s, Page::Holdings)),
            ("holding-details", |s| {
                scene(s, Page::Holdings);
                s.select_position(0);
            }),
            ("allocation", |s| scene(s, Page::Allocation)),
            ("performance", |s| scene(s, Page::Performance)),
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
            ("welcome", |s| {
                s.reset();
                s.settle();
                s.starting = false;
            }),
            ("parse-failure", |s| {
                s.startup();
                s.settle();
                let file = env::temp_dir().join(format!("stahl-broken-{}.json", process::id()));
                fs::write(&file, "[\n  {\"Name\": \"Cash\",,}\n]").expect("temp dir is writable");
                s.load_file(file.to_string_lossy().into_owned());
                s.settle();
            }),
            ("privacy", |s| {
                scene(s, Page::Overview);
                s.privacy = true;
            }),
            ("light", |s| {
                scene(s, Page::Overview);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
        ],
    );
    let _ = fs::remove_dir_all(&dir);
}
