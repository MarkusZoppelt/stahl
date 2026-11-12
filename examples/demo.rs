//! Stahl on a made-up portfolio, with no network and none of your data. Good for trying it,
//! and for rendering any screen without a window:
//!
//! ```sh
//! cargo run --example demo
//! cargo run --example demo -- --snap holdings.png --scene holdings --at 2
//! cargo run --example demo -- --list-scenes
//! ```

use std::{env, fs, path::Path, process, time::SystemTime};

use portfolio_rs::{
    config::ThemeMode,
    mcp::dto::{Category, Finding, Severity},
};
use stahl::{
    app, demo,
    state::{Confirm, FolderPrompt, Page, PendingOpen, State, UpdatePhase},
    statement::Since,
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
        // The owner has been to the records room before, so its mark is not on every screen.
        s.navigate(Page::Records);
        s.settle();
        s.navigate(page);
        s.settle();
    }
    /// Today's move set to `percent`, with the amount it means, for the room's light.
    fn day(s: &mut State, percent: f64) {
        if let Some(summary) = s.summary.as_mut() {
            let before = summary.total_value / (1.0 + percent / 100.0);
            summary.day_change = summary.total_value - before;
            summary.day_change_percent = percent;
        }
    }
    /// The plate the wall has at its top, if any.
    fn newest_plate(s: &State) -> Option<String> {
        s.records
            .data
            .as_ref()
            .and_then(|plates| plates.first())
            .map(|plate| plate.id.clone())
    }
    /// One of the wall's plates by what kind of record it is, for opening it.
    fn plate_of(s: &State, caption: &str) -> Option<String> {
        s.records
            .data
            .as_ref()
            .and_then(|plates| plates.iter().find(|plate| plate.kind.caption() == caption))
            .map(|plate| plate.id.clone())
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
    /// A page on the lowest day Stahl recorded, as the time strip or the door would leave it.
    fn past_scene(s: &mut State, page: Page) {
        scene(s, page);
        s.skip_opening();
        let lowest = s
            .time
            .days
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.total.total_cmp(&b.1.total))
            .map(|(index, _)| index);
        s.travel(lowest);
    }
    /// A newer release waiting, as if a check at startup had found it.
    fn update_scene(s: &mut State, installation: Installation, dialog: bool) {
        scene(s, Page::Overview);
        s.updates.updater = demo::updater(installation);
        s.check_for_updates(true);
        s.settle();
        s.updates.dialog = dialog;
    }
    /// What a refresh that moved two prices leaves behind: stocks up, long bonds down. The
    /// demo's quotes are fixed, so the figures on screen stay as they were; this shows the flash.
    fn moved_prices(s: &mut State) {
        let id = |s: &State, ticker: &str| {
            s.positions
                .iter()
                .find(|position| position.ticker.as_deref() == Some(ticker))
                .map(|position| position.id)
        };
        s.price_moves = [
            id(s, "SPY").map(|id| (id, 0.012)),
            id(s, "TLT").map(|id| (id, -0.01)),
        ]
        .into_iter()
        .flatten()
        .collect();
        s.price_updates += 1;
        s.pulse_started = Some(s.now);
    }
    /// Cut the demo's history down to its last `keep` valuations and open it again: a portfolio
    /// whose wall has only just begun.
    fn trim_history(s: &mut State, keep: usize) {
        let Some(dir) = s.open.workspace_dir.clone() else {
            return;
        };
        let path = Path::new(&dir).join("portfolio/history/main/snapshots.jsonl");
        let Ok(text) = fs::read_to_string(&path) else {
            return;
        };
        let lines: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .collect();
        let kept = &lines[lines.len().saturating_sub(keep)..];
        if fs::write(&path, kept.join("\n") + "\n").is_err() {
            return;
        }
        s.load_workspace(dir);
        s.settle();
    }
    /// A page in the phone layout. Render these at an iPhone's size: `--size 390x844`.
    fn phone(s: &mut State, page: Page) {
        s.compact = true;
        scene(s, page);
    }
    /// As on the first day: nothing to compare with yet.
    fn first_look(s: &mut State) {
        s.since = Some(Since::First);
        s.change = None;
        s.history = None;
    }
    /// A portfolio on its targets, so the agenda has nothing to ask.
    fn on_target(s: &mut State) {
        if let Some(review) = &mut s.review.data {
            for class in &mut review.allocations {
                class.actual_percent = class.target_percent + 0.6 * class.drift_percent.signum();
                class.drift_percent = class.actual_percent - class.target_percent;
                class.within_tolerance = true;
            }
        }
    }
    fn mac() -> Installation {
        Installation::MacApp {
            bundle: "/Applications/Stahl Capital.app".into(),
        }
    }
    /// A hand-written answer, to look at every block without a model.
    fn ask_answer(s: &mut State, question: &str, lines: &str) {
        scene(s, Page::Ask);
        let mut turn = stahl::ask::Turn::new(question);
        turn.blocks = stahl::ask::Lines::new().push(lines).into_iter().collect();
        turn.phase = stahl::ask::Phase::Done;
        s.ask.turns.push(turn);
        s.apply_ask_span();
    }
    /// A provider in Settings, so the consent prompt has somewhere to send to.
    fn provider(s: &mut State) {
        s.settings.currency.replace_text("EUR");
        s.settings
            .provider_url
            .replace_text("https://api.example.com/v1");
        s.settings.api_key.replace_text("sk-demo");
        s.settings.model.replace_text("gpt-4o-mini");
        s.save_settings();
        s.settle();
        // The toast would sit over the page in a screenshot.
        s.toast = None;
    }
    /// A question answered by the demo's scripted model.
    fn ask_scripted(s: &mut State, question: &str) {
        scene(s, Page::Ask);
        // The vault door has opened; a question's answer shouldn't arrive behind it.
        s.skip_opening();
        s.ask_question(question);
    }
    const ASK_CHANGE: &str = concat!(
        r#"{"block":"say","text":"Since {since.date}, your total moved {since.change}. Most of it was Bitcoin, which is {class.crypto.share} of the portfolio against a target of {class.crypto.target}."}"#,
        "\n",
        r#"{"block":"figures","ids":["total","since.change","class.crypto.share","cash.value"]}"#,
        "\n",
        r#"{"block":"chart","period":"month"}"#,
        "\n",
        r#"{"block":"say","text":"That is 3.2% of your money in one holding."}"#,
        "\n",
        r#"{"block":"go","steps":[{"label":"Open Rebalance","page":"rebalance"},{"label":"Why is crypto over its target?","ask":"Why is crypto over its target?"}]}"#,
        "\n",
    );
    const ASK_ALLOCATION: &str = concat!(
        r#"{"block":"say","text":"Crypto grew to {class.crypto.share} of the portfolio while stocks fell to {class.stocks.share}. That is why four of five classes sit outside their bands."}"#,
        "\n",
        r#"{"block":"allocation"}"#,
        "\n",
        r#"{"block":"holdings","sort":"today"}"#,
        "\n",
        r#"{"block":"go","steps":[{"label":"Where should new money go?","ask":"Where should new money go?"}]}"#,
        "\n",
    );
    const ASK_SPAN: &str = concat!(
        r#"{"block":"say","text":"The middle of the month did the work. Here is that stretch on its own."}"#,
        "\n",
        r#"{"block":"chart","period":"month","span":[5,20]}"#,
        "\n",
        r#"{"block":"go","steps":[{"label":"What changed this month?","ask":"What changed this month?"}]}"#,
        "\n",
    );
    const ASK_PLAN: &str = concat!(
        r#"{"block":"say","text":"A halving in crypto is the kind of day worth planning for. Here is what it would leave, and what a move toward your targets would ask of you."}"#,
        "\n",
        r#"{"block":"scenario","shocks":[{"class":"Crypto","percent":-50}]}"#,
        "\n",
        r#"{"block":"plan","fraction":0.6}"#,
        "\n",
        r#"{"block":"go","steps":[{"label":"Why is crypto over its target?","ask":"Why is crypto over its target?"}]}"#,
        "\n",
    );
    let result = app::run_with_scenes(
        backend,
        env::args().skip(1).collect(),
        &[
            // A return after some days: the demo's last look was earlier in the week.
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
            // The door thinking: the tumblers turn slowly and the steel breathes, as it does
            // while an answer is being composed.
            ("door-thinking", |s| {
                scene(s, Page::Overview);
                s.skip_opening();
                s.door_busy = true;
            }),
            // A new record: light runs along the total once. The demo's statement is a record.
            ("record", |s| {
                scene(s, Page::Overview);
                s.skip_opening();
            }),
            // The value chart drawing itself in, with the light running along its head.
            ("performance-draw", |s| {
                scene(s, Page::Performance);
                s.skip_opening();
            }),
            // A chart readout, as hovering the line leaves it: the smallest floating panel.
            ("readout", |s| {
                scene(s, Page::Performance);
                s.chart_hover = Some(520.0);
            }),
            ("readout-light", |s| {
                scene(s, Page::Performance);
                s.chart_hover = Some(520.0);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
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
            ("rebalance-light", |s| {
                scene(s, Page::Rebalance);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("rebalance-partway", |s| {
                scene(s, Page::Rebalance);
                stahl::view::rebalance::set_blend(s, 0.6);
            }),
            ("rebalance-partway-light", |s| {
                scene(s, Page::Rebalance);
                stahl::view::rebalance::set_blend(s, 0.6);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("rebalance-target", |s| {
                scene(s, Page::Rebalance);
                stahl::view::rebalance::set_blend(s, 1.0);
            }),
            ("rebalance-adjusted", |s| {
                scene(s, Page::Rebalance);
                stahl::view::rebalance::set_blend(s, 0.6);
                stahl::view::rebalance::set_share(s, 0, 55.0);
            }),
            ("rebalance-invest", |s| {
                scene(s, Page::Rebalance);
                stahl::view::rebalance::set_amount(s, 2000.0);
            }),
            ("rebalance-private", |s| {
                scene(s, Page::Rebalance);
                stahl::view::rebalance::set_blend(s, 0.6);
                s.privacy = true;
            }),
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
            // The palette's glass over the busiest pages: a treemap of colour and a chart.
            ("palette-map", |s| {
                scene(s, Page::Holdings);
                s.holdings_map = true;
                s.open_palette();
            }),
            ("palette-map-light", |s| {
                scene(s, Page::Holdings);
                s.holdings_map = true;
                s.set_theme(ThemeMode::Light);
                s.settle();
                s.open_palette();
            }),
            ("palette-chart", |s| {
                scene(s, Page::Performance);
                s.open_palette();
            }),
            ("palette-chart-light", |s| {
                scene(s, Page::Performance);
                s.set_theme(ThemeMode::Light);
                s.settle();
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
            ("welcome-light", |s| {
                first_run(s);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            // The door turned by hand: the welcome screen's dial a quarter turn round, where a
            // drag and its coast leave it.
            ("door-play", |s| {
                first_run(s);
                s.dial_hand = -1.35;
            }),
            ("door-play-light", |s| {
                first_run(s);
                s.dial_hand = -1.35;
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
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
            ("overview-first-look", |s| {
                scene(s, Page::Overview);
                first_look(s);
            }),
            ("overview-first-light", |s| {
                scene(s, Page::Overview);
                first_look(s);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("overview-clear", |s| {
                scene(s, Page::Overview);
                on_target(s);
            }),
            ("overview-clear-light", |s| {
                scene(s, Page::Overview);
                on_target(s);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("records", |s| scene(s, Page::Records)),
            ("records-light", |s| {
                scene(s, Page::Records);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("records-plate", |s| {
                scene(s, Page::Records);
                if let Some(id) = plate_of(s, "A fall").or_else(|| newest_plate(s)) {
                    s.open_record(id);
                    s.settle();
                }
            }),
            ("records-plate-light", |s| {
                scene(s, Page::Records);
                if let Some(id) = plate_of(s, "A milestone").or_else(|| newest_plate(s)) {
                    s.open_record(id);
                    s.settle();
                }
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("records-plate-old", |s| {
                scene(s, Page::Records);
                if let Some(id) = plate_of(s, "A holding") {
                    s.open_record(id);
                    s.settle();
                }
            }),
            ("records-privacy", |s| {
                scene(s, Page::Records);
                s.privacy = true;
            }),
            ("records-mark", |s| {
                scene(s, Page::Overview);
                s.records_unseen = 2;
            }),
            ("records-first-day", |s| {
                scene(s, Page::Records);
                trim_history(s, 1);
            }),
            ("records-empty", |s| empty(s, Page::Records)),
            // A new plate is set onto the wall and catches the light once: render at several
            // `--at` times to see the strip.
            ("records-arrival", |s| {
                scene(s, Page::Records);
                s.skip_opening();
                if let Some(id) = newest_plate(s) {
                    s.record_arrival = Some((id, 0.0));
                }
            }),
            ("phone-records", |s| phone(s, Page::Records)),
            ("phone-records-mark", |s| {
                phone(s, Page::Overview);
                s.records_unseen = 2;
                s.more = true;
            }),
            ("phone-records-plate", |s| {
                phone(s, Page::Records);
                if let Some(id) = plate_of(s, "A fall").or_else(|| newest_plate(s)) {
                    s.open_record(id);
                    s.settle();
                }
            }),
            ("phone-records-arrival", |s| {
                phone(s, Page::Records);
                s.skip_opening();
                if let Some(id) = newest_plate(s) {
                    s.record_arrival = Some((id, 0.0));
                }
            }),
            ("phone-records-light", |s| {
                phone(s, Page::Records);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("phone-overview", |s| phone(s, Page::Overview)),
            ("phone-overview-first", |s| {
                phone(s, Page::Overview);
                first_look(s);
            }),
            ("phone-overview-clear", |s| {
                phone(s, Page::Overview);
                on_target(s);
            }),
            ("phone-privacy", |s| {
                phone(s, Page::Overview);
                s.privacy = true;
            }),
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
            // The room's light on a day that is down, and on one that didn't move: the demo's
            // prices are fixtures, so the day's move is set on the summary the light reads.
            ("overview-down-day", |s| {
                scene(s, Page::Overview);
                day(s, -0.9);
            }),
            ("overview-flat-day", |s| {
                scene(s, Page::Overview);
                day(s, 0.0);
            }),
            // Launched into a portfolio: the opening has already played, and a refresh turns the
            // mark's tumblers once, so the frames from 0.05 s to 1.2 s show them turning.
            ("door-turn", |s| {
                scene(s, Page::Overview);
                s.skip_opening();
                s.price_updates += 1;
            }),
            // The welcome door opens onto the portfolio. Held on the welcome screen until 0.08 s,
            // after the large door has been painted, then the shell mounts and the door opens
            // from where the welcome door was.
            ("door-open", |s| {
                scene(s, Page::Overview);
                s.hold_welcome_until = Some(0.08);
            }),
            ("door-open-light", |s| {
                scene(s, Page::Overview);
                s.set_theme(ThemeMode::Light);
                s.settle();
                s.hold_welcome_until = Some(0.08);
            }),
            ("phone-door-open", |s| {
                s.compact = true;
                scene(s, Page::Overview);
                s.hold_welcome_until = Some(0.08);
            }),
            // A refresh that moves two prices: what moved flashes once in its colour.
            ("price-moves", |s| {
                scene(s, Page::Allocation);
                moved_prices(s);
            }),
            // The pulse a refresh sends from the door, without the opening over it.
            ("pulse", |s| {
                scene(s, Page::Allocation);
                s.skip_opening();
                moved_prices(s);
            }),
            ("phone-price-moves", |s| {
                phone(s, Page::Allocation);
                moved_prices(s);
            }),
            ("door-attention", |s| {
                scene(s, Page::Overview);
                s.findings.push(Finding {
                    code: "cash_below_minimum_amount".into(),
                    severity: Severity::Warning,
                    category: Category::Policy,
                    message: "Cash is below the minimum your policy sets".into(),
                    position_id: None,
                    asset_class: None,
                    detail: None,
                });
            }),
            ("door-attention-light", |s| {
                scene(s, Page::Overview);
                s.set_theme(ThemeMode::Light);
                s.settle();
                s.findings.push(Finding {
                    code: "cash_below_minimum_amount".into(),
                    severity: Severity::Warning,
                    category: Category::Policy,
                    message: "Cash is below the minimum your policy sets".into(),
                    position_id: None,
                    asset_class: None,
                    detail: None,
                });
            }),
            // The palette answering, before anyone goes anywhere: a class's value, share and day.
            ("palette-answer", |s| {
                scene(s, Page::Overview);
                s.open_palette();
                if let Some(palette) = &mut s.palette {
                    palette.query.replace_text("stocks");
                }
            }),
            ("palette-answer-light", |s| {
                scene(s, Page::Overview);
                s.set_theme(ThemeMode::Light);
                s.settle();
                s.open_palette();
                if let Some(palette) = &mut s.palette {
                    palette.query.replace_text("stocks");
                }
            }),
            ("phone-palette-answer", |s| {
                phone(s, Page::Overview);
                s.open_palette();
                if let Some(palette) = &mut s.palette {
                    palette.query.replace_text("stocks");
                }
            }),
            // Ask: a question answered with the app's own screens.
            ("ask", |s| scene(s, Page::Ask)),
            ("ask-light", |s| {
                scene(s, Page::Ask);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("ask-answer", |s| {
                ask_answer(s, "What changed this month?", ASK_CHANGE)
            }),
            ("ask-answer-light", |s| {
                ask_answer(s, "What changed this month?", ASK_CHANGE);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("ask-blocks", |s| {
                ask_answer(s, "Why is my allocation off?", ASK_ALLOCATION)
            }),
            ("ask-blocks-light", |s| {
                ask_answer(s, "Why is my allocation off?", ASK_ALLOCATION);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("ask-plan", |s| {
                ask_answer(s, "What if crypto halves?", ASK_PLAN)
            }),
            ("ask-plan-light", |s| {
                ask_answer(s, "What if crypto halves?", ASK_PLAN);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("phone-ask", |s| phone(s, Page::Ask)),
            ("phone-ask-answer", |s| {
                s.compact = true;
                ask_answer(s, "What changed this month?", ASK_CHANGE);
            }),
            ("phone-ask-blocks", |s| {
                s.compact = true;
                ask_answer(s, "Why is my allocation off?", ASK_ALLOCATION);
            }),
            ("phone-ask-plan", |s| {
                s.compact = true;
                ask_answer(s, "What if crypto halves?", ASK_PLAN);
            }),
            // The demo's model, answering for real. Render these at --at 2 to see the whole
            // answer, and at --at 0.5 to catch one arriving.
            ("counsel-change", |s| {
                ask_scripted(s, "What changed this month?")
            }),
            ("counsel-change-light", |s| {
                ask_scripted(s, "What changed this month?");
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("counsel-halves", |s| {
                ask_scripted(s, "What if crypto halves?")
            }),
            ("counsel-money", |s| {
                ask_scripted(s, "Where should €2,000 go?")
            }),
            ("counsel-allocation", |s| {
                ask_scripted(s, "Why is my allocation off?")
            }),
            ("counsel-bitcoin", |s| {
                ask_scripted(s, "How is Bitcoin doing?")
            }),
            ("counsel-else", |s| ask_scripted(s, "Should I buy a house?")),
            ("counsel-stream", |s| {
                ask_scripted(s, "What changed this month?")
            }),
            ("counsel-waiting", |s| {
                ask_scripted(s, "What changed this month?")
            }),
            ("counsel-revisit", |s| {
                ask_scripted(s, "What changed this month?");
                s.ask_advance(10.0);
                s.navigate(Page::Holdings);
                s.navigate(Page::Ask);
            }),
            ("counsel-span", |s| {
                ask_answer(s, "When did it move?", ASK_SPAN)
            }),
            ("counsel-failed", |s| {
                ask_scripted(s, "What changed this month?");
                s.ask_fail("The provider didn't answer. Check the address in Settings.");
            }),
            ("counsel-stopped", |s| {
                ask_scripted(s, "What changed this month?");
                s.ask_stop();
            }),
            ("phone-counsel", |s| {
                s.compact = true;
                ask_scripted(s, "What changed this month?");
            }),
            // A provider away from this machine: the question waits for a yes.
            ("counsel-consent", |s| {
                scene(s, Page::Ask);
                s.skip_opening();
                provider(s);
                s.ask_question("What changed this month?");
            }),
            ("counsel-consent-open", |s| {
                scene(s, Page::Ask);
                s.skip_opening();
                provider(s);
                s.ask_question("What changed this month?");
                s.consent_toggle();
            }),
            ("counsel-consent-light", |s| {
                scene(s, Page::Ask);
                s.skip_opening();
                provider(s);
                s.ask_question("What changed this month?");
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("phone-counsel-consent", |s| {
                s.compact = true;
                scene(s, Page::Ask);
                s.skip_opening();
                provider(s);
                s.ask_question("What changed this month?");
            }),
            // No provider: the app answers what it can itself.
            ("counsel-no-model", |s| {
                scene(s, Page::Ask);
                s.skip_opening();
                s.model = std::sync::Arc::new(stahl::ask::Builtin);
                s.ask_question("Should I buy a house?");
            }),
            ("counsel-private", |s| {
                ask_scripted(s, "Why is my allocation off?");
                s.privacy = true;
            }),
            ("counsel-private-amounts", |s| {
                ask_scripted(s, "What changed this month?");
                s.privacy = true;
            }),
            ("counsel-builtin", |s| {
                scene(s, Page::Ask);
                s.skip_opening();
                s.model = std::sync::Arc::new(stahl::ask::Builtin);
                s.ask_question("How much is in crypto?");
            }),
            ("counsel-builtin-light", |s| {
                scene(s, Page::Ask);
                s.skip_opening();
                s.model = std::sync::Arc::new(stahl::ask::Builtin);
                s.ask_question("How much is in crypto?");
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            // Time turned back: the whole app on the lowest recorded day.
            ("past", |s| past_scene(s, Page::Overview)),
            ("past-light", |s| {
                past_scene(s, Page::Overview);
                s.set_theme(ThemeMode::Light);
                s.settle();
            }),
            ("past-holdings", |s| past_scene(s, Page::Holdings)),
            ("past-holdings-map", |s| {
                past_scene(s, Page::Holdings);
                s.holdings_map = true;
            }),
            ("past-allocation", |s| past_scene(s, Page::Allocation)),
            ("past-rebalance", |s| past_scene(s, Page::Rebalance)),
            ("phone-past", |s| {
                s.compact = true;
                past_scene(s, Page::Overview);
            }),
            ("phone-past-holdings", |s| {
                s.compact = true;
                past_scene(s, Page::Holdings);
                s.holdings_map = true;
            }),
            ("past-private", |s| {
                past_scene(s, Page::Overview);
                s.privacy = true;
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
