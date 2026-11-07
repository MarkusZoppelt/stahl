//! The window: pure functions from [`State`] to elements.

use std::{env, mem};

use rune::{
    Align, Cx, El, Icon, Key, KeyEvent, ScrollSpec, Transition, Tx, col, el, icon, row, text,
    tok::motion::LEAD,
};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, RollingText, Spinner, root,
    shell::{Sidebar, SidebarRow, SidebarSection, SplitView, Toolbar, is_sidebar_toggle},
};

use crate::{
    icons,
    state::{Page, State, file_name},
    theme::{PAGE_TITLE, STATEMENT, STATEMENT_COMPACT, Theme, WORDMARK},
};

mod charts;
mod compact;
pub mod guide;
mod insights;
mod overlays;
mod plan;
mod portfolio;
pub mod ui;
mod updates;

pub fn page_icon(page: Page) -> &'static Icon {
    match page {
        Page::Overview => &icons::LAYOUT_DASHBOARD,
        Page::Holdings => &icons::CHART_COLUMN,
        Page::Allocation => &icons::CHART_PIE,
        Page::Performance => &icons::TRENDING_UP,
        Page::Review => &icons::STETHOSCOPE,
        Page::Rebalance => &icons::SCALE,
        Page::Policy => &icons::SHIELD,
        Page::Documents => &icons::FILE_TEXT,
        Page::Briefing => &icons::CLIPBOARD_LIST,
        Page::Health => &icons::HEART_PULSE,
        Page::Settings => &icons::SETTINGS,
    }
}

/// Keys nothing focused wanted, on every platform; the native menu shows the same chords on macOS.
pub fn on_key(s: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    if is_sidebar_toggle(event) {
        s.sidebar.toggle();
        return true;
    }
    if event.key == Key::Escape {
        return s.palette.take().is_some()
            || s.form.take().is_some()
            || s.new_workspace.take().is_some()
            || s.confirm.take().is_some()
            || s.folder_prompt.take().is_some()
            || mem::take(&mut s.guide)
            || s.close_update_dialog();
    }
    if portfolio::holdings_key(s, event, tx) {
        return true;
    }
    let Key::Command(key) = event.key else {
        return false;
    };
    let shift = event.mods.shift;
    match (key, shift) {
        ('k', false) => {
            if s.palette.take().is_none() {
                s.open_palette();
            }
        }
        ('o', false) => s.ask_for_file(tx),
        ('o', true) => s.ask_for_workspace(tx),
        ('s', false) => s.save(),
        ('r', false) => s.refresh_prices(),
        ('h', true) => s.privacy = !s.privacy,
        ('n', false) if s.has_portfolio() => s.open_form(None),
        ('n', true) => s.open_new_workspace(),
        ('w', true) => s.reset(),
        // Some layouts need Shift to type a slash, so it doesn't matter here.
        ('/' | '?', _) => s.toggle_guide(),
        ('q', false) => s.quit_or_ask(tx),
        (key, false) => match Page::from_shortcut(key) {
            Some(page) if s.has_portfolio() => s.navigate(page),
            _ => return false,
        },
        _ => return false,
    }
    true
}

pub fn view(s: &State, cx: &mut Cx<State>) -> El {
    let t = Theme::new(s.is_dark());
    let mut layer = Vec::new();
    let body = if s.starting {
        splash(&t)
    } else if s.folder_prompt.is_some() {
        // Not the shell: the previous portfolio is still in memory, and must not look loaded.
        folder_prompt(s, &t)
    } else if s.open.parse_error.is_some() {
        overlays::parse_failure(s, &t)
    } else if !s.has_portfolio() {
        welcome(s, &t)
    } else {
        shell(s, &t, cx, &mut layer)
    };
    overlays::build(s, &t, &mut layer);

    let body = body
        .fill(t.kit.c.bg_base)
        .on_drop_files(|s: &mut State, files, _| {
            s.drop_hover = false;
            if let Some(path) = files.first() {
                let path = path.clone();
                s.open_dropped(&path);
            }
        })
        .on_drop_hover(|s: &mut State, over, _| s.drop_hover = over);
    let body = if s.drop_hover {
        body.border(2.0, t.accent)
    } else {
        body
    };
    root(body, layer)
}

fn splash(t: &Theme) -> El {
    col()
        .w_full()
        .h_full()
        .center()
        .gap(16.0)
        .child(ui::wash(t, 0.0))
        .child(ui::orbiting_mark(72.0, t))
        .child(ui::muted("Loading your portfolio…", t))
}

/// Rune's windowing library reports no file drags on Wayland, so don't offer them there.
fn drops_work() -> bool {
    !cfg!(target_os = "linux") || env::var_os("WAYLAND_DISPLAY").is_none()
}

fn welcome(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let compact = s.compact;
    let choice = |key: &'static str,
                  glyph: &'static Icon,
                  primary: bool,
                  title: &str,
                  detail: &str,
                  index: usize| {
        let mark = col()
            .size(40.0, 40.0)
            .radius(8.0)
            .continuous_corners()
            .center()
            .fill(if primary { t.accent } else { k.c.layer_03 })
            .child(icon(glyph, 20.0).color(if primary { k.c.bg_base } else { k.c.text_base }));
        let words = col()
            .gap(4.0)
            .min_w(0.0)
            .shrink()
            .child(
                text(title.to_string())
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base),
            )
            .child(ui::muted(detail, t).wrap());
        let card = ui::card(t)
            .key(key)
            .role("button")
            .focusable(true)
            .grow(1.0)
            .hover(k.c.hover)
            .press_scale(0.98);
        // A column of cards on a phone, each a row; side by side on a desktop, each a column.
        let card = if compact {
            card.padding(14.0)
                .child(row().center_y().gap(14.0).child(mark).child(words))
        } else {
            card.child(mark).child(words)
        };
        ui::entering(card, index)
    };
    col()
        .w_full()
        .h_full()
        .center()
        .gap(10.0)
        .padding(if compact { 16.0 } else { 24.0 })
        .child(ui::wash(t, 0.0))
        .child(ui::door(if compact { 112.0 } else { 168.0 }).mb(14.0))
        .child(
            text("Stahl Capital has one client.")
                .font(if compact { STATEMENT_COMPACT } else { STATEMENT })
                .color(k.c.text_base)
                .wrap()
                .align(Align::Center),
        )
        .child(
            col().w_full().max_w(620.0).items_center().mb(26.0).child(
                ui::muted(
                    "Track what you own, see how it has done, and check it against your plan. \
                     Your portfolio stays in plain files on this computer. Looking up prices \
                     sends only the ticker symbols.",
                    t,
                )
                .wrap()
                .align(Align::Center),
            ),
        )
        .child(
            row()
                .w_full()
                .max_w(880.0)
                .gap(if compact { 10.0 } else { 16.0 })
                .items_stretch()
                .equal_columns(if compact { 1 } else { 3 })
                .child(
                    choice(
                        "welcome-create",
                        &icons::FOLDER_PLUS,
                        true,
                        "Create a workspace",
                        "Start here if you're new. A folder for your positions, policy, reports and decisions.",
                        0,
                    )
                    .on_press(|s: &mut State| s.open_new_workspace()),
                )
                .child(
                    choice(
                        "welcome-workspace",
                        &icons::FOLDER_OPEN,
                        false,
                        "Open a workspace",
                        "Pick up a folder you made here or with the portfolio_rs command line.",
                        1,
                    )
                    .on_press_tx(|s: &mut State, tx: &mut Tx| s.ask_for_workspace(tx)),
                )
                .child(
                    choice(
                        "welcome-file",
                        &icons::FILE_BRACES,
                        false,
                        "Open a positions file",
                        "Have a positions.json already? See it right away. Planning needs a workspace.",
                        2,
                    )
                    .on_press_tx(|s: &mut State, tx: &mut Tx| s.ask_for_file(tx)),
                ),
        )
        .children(
            (drops_work() && !compact).then(|| {
                ui::caption("or drop a file or folder anywhere on this window", t).mt(24.0)
            }),
        )
        .child(
            Button::new("What's in Stahl?")
                .variant(ButtonVariant::Ghost)
                .icon(&icons::CIRCLE_HELP)
                .shortcut(ui::chord(false, "/"))
                .on_press(|s: &mut State| s.toggle_guide())
                .build(k)
                .key("welcome-guide")
                .mt(8.0),
        )
        .children(
            s.error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t).mt(16.0).max_w(880.0)),
        )
}

/// A folder was chosen and it is not a workspace. The previous portfolio is not drawn.
fn folder_prompt(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let Some(prompt) = &s.folder_prompt else {
        return col();
    };
    let name = file_name(&prompt.dir);
    let detail = if prompt.empty {
        format!(
            "{name} is empty, so there is nothing to open. Start a workspace in this folder, or choose a different one."
        )
    } else {
        format!(
            "{name} has no positions.json, and it isn't a portfolio from the portfolio_rs command line. Starting a workspace here adds those files and leaves everything else in place."
        )
    };
    col()
        .w_full()
        .h_full()
        .center()
        .gap(12.0)
        .padding(24.0)
        .child(ui::wash(t, 0.0))
        .child(
            text("This folder isn't a workspace")
                .font(PAGE_TITLE)
                .color(k.c.text_base),
        )
        .child(
            col()
                .w_full()
                .max_w(640.0)
                .items_center()
                .child(ui::muted(detail, t).wrap().align(Align::Center)),
        )
        .child(
            ui::caption(prompt.dir.clone(), t)
                .key("folder-path")
                .truncate_middle()
                .max_w(640.0),
        )
        .children(
            prompt
                .error
                .clone()
                .map(|error| ui::danger(error, t).max_w(640.0)),
        )
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(
                    Button::new("Start a workspace here")
                        .variant(ButtonVariant::Submit)
                        .icon(&icons::FOLDER_PLUS)
                        .on_press(|s: &mut State| s.start_prompted_workspace())
                        .build(k)
                        .key("folder-start"),
                )
                .child(
                    Button::new("Choose another folder")
                        .icon(&icons::FOLDER_OPEN)
                        .on_press_tx(|s: &mut State, tx: &mut Tx| s.ask_for_workspace(tx))
                        .build(k)
                        .key("folder-choose"),
                )
                .child(
                    Button::new("Cancel")
                        .on_press(|s: &mut State| s.folder_prompt = None)
                        .build(k)
                        .key("folder-cancel"),
                ),
        )
}

fn nav_row(s: &State, page: Page, t: &Theme) -> El {
    SidebarRow::new(page.label())
        .icon(page_icon(page))
        .selected(s.page == page)
        .on_select(move |s: &mut State, _| s.navigate(page))
        .build(&t.kit)
        .key(page.key())
}

fn sidebar(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let document = s
        .open
        .workspace_dir
        .as_deref()
        .or(s.open.file_path.as_deref())
        .map(file_name)
        .unwrap_or_default()
        .to_string();
    let header = row()
        .center_y()
        .gap(10.0)
        .px(8.0)
        .pb(8.0)
        .child(ui::door(28.0))
        .child(
            col()
                .shrink()
                .child(text("Stahl Capital").font(WORDMARK).color(k.c.text_base))
                .child(
                    text(document)
                        .font(k.ty.text_xsmall)
                        .color(k.c.text_faint)
                        .truncate(),
                ),
        );
    let mut bar = Sidebar::new().label("Sections").header(header).section(
        SidebarSection::new("Portfolio").rows(Page::PORTFOLIO.map(|page| nav_row(s, page, t))),
    );
    bar = if s.is_workspace() {
        bar.section(SidebarSection::new("Plan").rows(Page::PLAN.map(|page| nav_row(s, page, t))))
    } else {
        bar.child(
            col()
                .key("nav-unlock")
                .role("button")
                .label("Create a workspace")
                .focusable(true)
                .px(10.0)
                .py(8.0)
                .gap(2.0)
                .radius(6.0)
                .hover(k.c.hover)
                .on_press(|s: &mut State| s.open_new_workspace())
                .child(text("Plan").font(k.ty.text_xsmall_medium).color(k.c.text_faint))
                .child(
                    ui::caption(
                        "Create a workspace to set a policy, review drift, and simulate rebalancing.",
                        t,
                    )
                    .wrap(),
                ),
        )
    };
    bar.section(
        SidebarSection::new("Insights").rows(Page::INSIGHTS.map(|page| nav_row(s, page, t))),
    )
    .footer(nav_row(s, Page::Settings, t))
    .build(k)
}

/// How much to trust the numbers on screen, at a glance.
fn freshness_label(network: &str, data_status: &str) -> (&'static str, Freshness) {
    match (network, data_status) {
        ("offline", _) => ("Offline", Freshness::Doubtful),
        (_, "stale") => ("Stale prices", Freshness::Doubtful),
        ("partial", _) | (_, "partial") => ("Partial prices", Freshness::Doubtful),
        ("fixture", _) => ("Fixture prices", Freshness::Demo),
        _ => ("Live", Freshness::Good),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Freshness {
    Good,
    Doubtful,
    Demo,
}

fn freshness(s: &State, t: &Theme) -> El {
    let status = s
        .summary
        .as_ref()
        .map_or("complete", |summary| summary.data_status.as_str());
    let (label, level) = freshness_label(&s.network, status);
    let color = match level {
        Freshness::Good => t.up,
        Freshness::Doubtful => t.warn,
        Freshness::Demo => t.info,
    };
    let time = s.updated_at.as_deref().unwrap_or("");
    row()
        .center_y()
        .key("freshness")
        .role("status")
        .gap(6.0)
        .center_y()
        .child(
            el().key(format!("freshness-{label}"))
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(color)
                .motion(LEAD)
                .cycle(ui::BREATHE),
        )
        .child(
            RollingText::new("freshness-text", format!("{label} · {time}"))
                .font(t.kit.ty.text_xsmall)
                .color(t.kit.c.text_muted)
                .build(&t.kit),
        )
        .help(guide::FRESHNESS)
}

fn tool(icon: &'static Icon, label: &str, t: &Theme) -> IconButton {
    let _ = t;
    IconButton::new(icon)
        .variant(IconButtonVariant::Ghost)
        .label(label.to_string())
        .help(label.to_string())
}

fn toolbar(s: &State, t: &Theme) -> Toolbar {
    let k = &t.kit;
    let refresh = if s.loading {
        Spinner::new().size(14.0).build(k).key("refreshing").mx(7.0)
    } else {
        tool(&icons::REFRESH, "Refresh Prices", t)
            .shortcut(ui::chord(false, "R"))
            .on_press(|s: &mut State| s.refresh_prices())
            .build(k)
            .key("refresh-prices")
    };
    let trailing = row()
        .center_y()
        .gap(2.0)
        .center_y()
        .children(s.dirty.then(|| {
            Button::new("Save Changes")
                .variant(ButtonVariant::Submit)
                .icon(&icons::SAVE)
                .help("Your edits are not on disk yet")
                .on_press(|s: &mut State| s.save())
                .build(k)
                .key("save")
                .mr(6.0)
                .transition(Transition::scale(0.9).combined(Transition::opacity()))
        }))
        .child(
            tool(&icons::CIRCLE_HELP, "Guide", t)
                .shortcut(ui::chord(false, "/"))
                .on_press(|s: &mut State| s.toggle_guide())
                .build(k)
                .key("guide"),
        )
        .child(
            tool(&icons::SEARCH, "Command Palette", t)
                .shortcut(ui::chord(false, "K"))
                .on_press(|s: &mut State| s.open_palette())
                .build(k)
                .key("palette"),
        )
        .child(
            tool(
                if s.privacy {
                    &icons::EYE_OFF
                } else {
                    &icons::EYE
                },
                if s.privacy {
                    "Show Amounts"
                } else {
                    "Hide Amounts"
                },
                t,
            )
            .shortcut(ui::chord(true, "H"))
            .on_press(|s: &mut State| s.privacy = !s.privacy)
            .build(k)
            .key("privacy"),
        )
        .child(
            tool(
                if t.dark { &icons::SUN } else { &icons::MOON },
                if t.dark {
                    "Switch to Light Mode"
                } else {
                    "Switch to Dark Mode"
                },
                t,
            )
            .on_press(|s: &mut State| s.toggle_theme())
            .build(k)
            .key("toggle-theme"),
        )
        .child(refresh);
    // With nothing to price, there's nothing to say about prices.
    Toolbar::new()
        .leading(if s.positions.is_empty() {
            el()
        } else {
            freshness(s, t)
        })
        .trailing(trailing)
}

/// Where you are: the section in small type, the page in large.
fn page_heading(s: &State, t: &Theme) -> El {
    let section = if Page::PLAN.contains(&s.page) {
        Some("Plan")
    } else if Page::PORTFOLIO.contains(&s.page) {
        Some("Portfolio")
    } else if Page::INSIGHTS.contains(&s.page) {
        Some("Insights")
    } else {
        None
    };
    col()
        .gap(2.0)
        .children(section.map(|section| ui::caption(section, t)))
        .child(
            text(s.page.label())
                .font(PAGE_TITLE)
                .color(t.kit.c.text_base)
                .role("heading"),
        )
}

/// Whether the details pane leaves the Holdings table enough room. It goes by the pane's usual
/// width, so that dragging the pane wider can't swap the layout under the pointer.
fn pane_fits(s: &State, cx: &Cx<State>) -> bool {
    cx.layout_size("window").is_none_or(|[width, _]| {
        width - s.sidebar.shown_width() - s.inspector.default_width()
            >= portfolio::TABLE_BESIDE_PANE
    })
}

/// The current page's content. Without `pane_fits`, a selected holding's details take the
/// table's place.
fn page_content(s: &State, t: &Theme, cx: &Cx<State>, pane_fits: bool) -> El {
    let selected = s
        .selected
        .and_then(|id| s.positions.iter().find(|position| position.id == id));
    match s.page {
        Page::Overview => portfolio::overview(s, t, cx),
        Page::Holdings => match selected {
            Some(position) if !pane_fits => portfolio::details_page(s, position, t),
            _ => portfolio::holdings(s, t, cx),
        },
        Page::Allocation => portfolio::allocation(s, t, cx),
        Page::Performance => portfolio::performance(s, t, cx),
        Page::Review => plan::review(s, t),
        Page::Rebalance => plan::rebalance(s, t, cx),
        Page::Policy => plan::policy(s, t, cx),
        Page::Documents => plan::documents(s, t, cx),
        Page::Briefing => insights::briefing(s, t, cx),
        Page::Health => insights::health(s, t),
        Page::Settings => insights::settings(s, t),
    }
}

/// The scrolling page: any error, its heading, what it's for, then `content`. `key` names it, so a
/// new page replaces the old one with a transition. `margin` is its side margin.
fn page_column(
    s: &State,
    t: &Theme,
    key: String,
    heading: El,
    about: Option<&str>,
    content: El,
    margin: f32,
) -> El {
    // A scroller per page, so a page opens at its top rather than where the last one was left.
    // The frame around it keeps one key, which the pages measure their room by.
    let scroller = col()
        .key(format!("{key}-scroll"))
        .w_full()
        .h_full()
        .min_h(0.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        });
    col().key("main").w_full().h_full().min_h(0.0).child(
        scroller.child(
            col()
                .key(key)
                .w_full()
                .max_w(1240.0)
                .px(margin)
                .pt(20.0)
                .pb(40.0)
                .gap(24.0)
                .transition(
                    Transition::offset(0.0, 8.0)
                        .combined(Transition::blur(3.0))
                        .combined(Transition::opacity()),
                )
                .children(s.error.as_ref().map(|error| ui::danger(error.clone(), t)))
                .child(heading)
                .children(about.map(|about| ui::caption(about, t).key("page-about").wrap()))
                .child(content),
        ),
    )
}

fn shell(s: &State, t: &Theme, cx: &mut Cx<State>, layer: &mut Vec<El>) -> El {
    let _ = &layer;
    if s.compact {
        return compact::shell(s, t, cx);
    }
    let selected = s
        .selected
        .and_then(|id| s.positions.iter().find(|position| position.id == id));
    let pane_fits = pane_fits(s, cx);
    let scroller = page_column(
        s,
        t,
        format!("page-{}", s.page.key()),
        page_heading(s, t),
        Some(guide::about(s.page)),
        page_content(s, t, cx, pane_fits),
        32.0,
    );
    let main = col()
        .key("main-frame")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(ui::wash(t, 12.0))
        .child(scroller);
    let mut split = SplitView::new(main)
        .sidebar(&s.sidebar, |s: &mut State| &mut s.sidebar, sidebar(s, t))
        .toolbar(toolbar(s, t))
        .titlebar_inset(cx.titlebar_inset())
        .reduced_motion(cx.is_reduced_motion());
    if let Some(position) = selected.filter(|_| pane_fits) {
        split = split.inspector(
            &s.inspector,
            |s: &mut State| &mut s.inspector,
            portfolio::inspector(s, position, t),
        );
    }
    col()
        .key("window")
        .w_full()
        .h_full()
        .child(split.build(&t.kit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freshness_never_calls_doubtful_prices_live() {
        assert_eq!(freshness_label("connected", "complete").0, "Live");
        assert_eq!(freshness_label("offline", "complete").0, "Offline");
        assert_eq!(freshness_label("connected", "stale").0, "Stale prices");
        assert_eq!(freshness_label("partial", "complete").0, "Partial prices");
        assert_eq!(freshness_label("connected", "partial").0, "Partial prices");
        assert_eq!(freshness_label("fixture", "complete").0, "Fixture prices");
    }
}
