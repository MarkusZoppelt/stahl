//! The phone layout: a slim bar on top, one page at a time, and a tab bar along the bottom. The
//! pages are the desktop's own, in one column; the pages without a tab live under More.

use std::f32::consts::FRAC_PI_2;

use rune::{Cx, El, Icon, LoopSpec, Spring, col, el, icon, row, text};
use rune_kit::{IconButtonVariant, Spinner};

use crate::{
    icons,
    state::{Page, State},
    theme::{PAGE_TITLE, Theme},
    view::{
        ask, door, freshness, guide, page_column, page_content, page_heading, page_icon, tool, ui,
    },
};

/// Side margin of a page on a phone.
const MARGIN: f32 = 16.0;

pub fn shell(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let content = if s.more {
        page_column(
            s,
            t,
            "page-more".to_string(),
            text("More")
                .font(PAGE_TITLE)
                .color(t.kit.c.text_base)
                .role("heading"),
            None,
            more_list(s, t),
            MARGIN,
        )
    } else {
        let heading = if Page::TABS.contains(&s.page) {
            page_heading(s, t)
        } else {
            col()
                .gap(6.0)
                .child(back_to_more(t))
                .child(page_heading(s, t))
        };
        // Overview says what the page is for in its own sentence. On a phone, repeating it
        // in a caption pushes what to do under the tab bar.
        let about = (s.page != Page::Overview).then(|| guide::about(s.page));
        if s.page == Page::Ask && s.ai && !s.in_past() {
            ask::page(s, t, cx)
        } else {
            page_column(
                s,
                t,
                format!("page-{}", s.page.key()),
                heading,
                about,
                page_content(s, t, cx, false),
                MARGIN,
            )
        }
    };
    col()
        .key("window")
        .w_full()
        .h_full()
        .child(top_bar(s, t))
        .children(super::past::band(s, t))
        .child(
            col()
                .key("main-frame")
                .w_full()
                .grow(1.0)
                .min_h(0.0)
                .child(ui::light(s, t, 0.0))
                .child(pull_door(s))
                .child(content),
        )
        .children((!s.keyboard).then(|| tab_bar(s, t)))
}

/// The door behind the top of a page pulled past its top: it grows into view with the pull, and
/// spins while the prices it asked for come in.
fn pull_door(s: &State) -> El {
    let door = if s.shaders {
        crate::fx::pull(28.0)
    } else {
        ui::door(28.0)
    };
    let door = if s.pull_refreshing && s.loading {
        door.looping(LoopSpec::rotation(1.2))
    } else {
        door.live_opacity(&s.pull.shown).live_scale(&s.pull.scale)
    };
    row()
        .key("pull-door")
        .absolute(0.0, 14.0)
        .w_full()
        .h(28.0)
        .center()
        .a11y_hidden()
        .child(door)
}

/// The mark, how fresh the prices are, and the two things worth a tap from anywhere.
fn top_bar(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let refresh = if s.loading {
        Spinner::new().size(14.0).build(k).key("refreshing").mx(7.0)
    } else {
        tool(&icons::REFRESH, "Refresh Prices", t)
            .on_press(|s: &mut State| s.refresh_prices())
            .build(k)
            .key("refresh-prices")
    };
    row()
        .center_y()
        .key("top-bar")
        .w_full()
        .h(44.0)
        .px(MARGIN)
        .gap(8.0)
        .justify_between()
        .child(
            row()
                .center_y()
                .gap(10.0)
                .shrink()
                .child(
                    // The vault's door turns at a tap, on a heavy spring.
                    col()
                        .key("vault-door")
                        .role("button")
                        .label("Turn the vault door")
                        .center()
                        .size(30.0, 30.0)
                        .rotate(s.door_turns as f32 * FRAC_PI_2)
                        .rotate_motion(Spring::new(0.7, 0.35))
                        .press_scale(0.9)
                        .on_press(|s: &mut State| s.turn_door())
                        .child(door::mark(s, t, 22.0, "phone")),
                )
                .children((!s.positions.is_empty()).then(|| freshness(s, t))),
        )
        .child(
            row()
                .center_y()
                .gap(2.0)
                .children(s.dirty.then(|| {
                    tool(&icons::SAVE, "Save Changes", t)
                        .variant(IconButtonVariant::Submit)
                        .on_press(|s: &mut State| s.save())
                        .build(k)
                        .key("save")
                }))
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
                    .on_press(|s: &mut State| s.privacy = !s.privacy)
                    .build(k)
                    .key("privacy"),
                )
                .child(refresh),
        )
}

fn tab(key: &'static str, label: &'static str, glyph: &'static Icon, on: bool, t: &Theme) -> El {
    let k = &t.kit;
    let color = if on { k.c.text_base } else { k.c.text_faint };
    col()
        .key(key)
        .role("tab")
        .label(label)
        .a11y_value(if on { "Selected" } else { "" })
        .focusable(true)
        .grow(1.0)
        .w(0.0)
        .items_center()
        .gap(3.0)
        .pt(8.0)
        .pb(4.0)
        .radius(8.0)
        .press_scale(0.96)
        .child(icon(glyph, 22.0).color(color))
        .child(text(label).font(k.ty.text_xsmall_medium).color(color))
}

fn tab_bar(s: &State, t: &Theme) -> El {
    let on_page = |page: Page| !s.more && s.page == page;
    let more = s.more || !Page::TABS.contains(&s.page);
    row()
        .center_y()
        .key("tab-bar")
        .role("tablist")
        .w_full()
        .px(8.0)
        .fill(t.kit.c.bg_base)
        .child(ui::rule(t).absolute(0.0, 0.0))
        .children(Page::TABS.map(|page| {
            tab(page.key(), page.label(), page_icon(page), on_page(page), t)
                .on_press(move |s: &mut State| s.navigate(page))
        }))
        .child(
            tab("tab-more", "More", &icons::ELLIPSIS, more, t).on_press(|s: &mut State| {
                s.palette = None;
                s.more = true;
            }),
        )
}

/// Back from a page reached through More.
fn back_to_more(t: &Theme) -> El {
    let k = &t.kit;
    row()
        .center_y()
        .key("back-to-more")
        .role("button")
        .label("Back to More")
        .focusable(true)
        .gap(2.0)
        .ml(-6.0)
        .py(4.0)
        .pr(8.0)
        .radius(6.0)
        .hover(k.c.hover)
        .on_press(|s: &mut State| s.more = true)
        .child(icon(&icons::CHEVRON_LEFT, 18.0).color(k.c.text_muted))
        .child(text("More").font(k.ty.ui).color(k.c.text_muted))
}

/// One row in More: a page, or something to do. `mark` sits between the label and the chevron.
fn more_row(
    key: &'static str,
    glyph: &'static Icon,
    label: &str,
    mark: Option<El>,
    t: &Theme,
) -> El {
    let k = &t.kit;
    row()
        .center_y()
        .key(key)
        .role("button")
        .label(label.to_string())
        .focusable(true)
        .w_full()
        .h(48.0)
        .px(12.0)
        .gap(12.0)
        .radius(6.0)
        .hover(k.c.hover)
        .child(icon(glyph, 18.0).color(k.c.text_muted))
        .child(
            text(label.to_string())
                .font(k.ty.ui)
                .color(k.c.text_base)
                .grow(1.0)
                .truncate(),
        )
        .children(mark)
        .child(icon(&icons::CHEVRON_RIGHT, 16.0).color(k.c.text_faint))
}

fn page_row(page: Page, t: &Theme) -> El {
    page_row_marked(page, None, t)
}

/// A page's row, with the quiet mark a page may carry.
fn page_row_marked(page: Page, mark: Option<El>, t: &Theme) -> El {
    more_row(page.key(), page_icon(page), page.label(), mark, t)
        .on_press(move |s: &mut State| s.navigate(page))
}

/// A section of More: a caption over a box of rows, with hairlines between them.
fn section(title: &str, rows: Vec<El>, t: &Theme) -> El {
    let mut list = ui::card(t).padding(4.0).gap(0.0);
    for (index, item) in rows.into_iter().enumerate() {
        if index > 0 {
            // Under the labels, not the icons.
            list = list.child(
                row()
                    .w_full()
                    .pl(42.0)
                    .child(el().grow(1.0).h(1.0).fill(t.kit.c.border_muted)),
            );
        }
        list = list.child(item);
    }
    col()
        .w_full()
        .gap(8.0)
        .child(ui::caption(title.to_string(), t).px(4.0))
        .child(list)
}

/// Every page without a tab, and what the desktop keeps in its toolbar.
fn more_list(s: &State, t: &Theme) -> El {
    let plan = if s.is_workspace() {
        Page::PLAN.map(|page| page_row(page, t)).to_vec()
    } else {
        vec![
            more_row(
                "more-create-workspace",
                &icons::FOLDER_PLUS,
                "Create a workspace",
                None,
                t,
            )
            .on_press(|s: &mut State| s.open_new_workspace()),
        ]
    };
    let theme = if t.dark {
        ("Light appearance", &icons::SUN)
    } else {
        ("Dark appearance", &icons::MOON)
    };
    col()
        .w_full()
        .gap(24.0)
        .children(s.ai.then(|| section("Counsel", vec![page_row(Page::Ask, t)], t)))
        .child(section("Portfolio", vec![page_row(Page::Allocation, t)], t))
        .child(section("Plan", plan, t))
        .child(section(
            "Insights",
            Page::INSIGHTS
                .map(|page| {
                    let mark = (page == Page::Records && s.records_unseen > 0).then(|| {
                        ui::dot(t.accent)
                            .mr(6.0)
                            .role("img")
                            .label("A new record")
                            .transition(ui::appear())
                    });
                    page_row_marked(page, mark, t)
                })
                .to_vec(),
            t,
        ))
        .child(section(
            "Stahl",
            vec![
                more_row("more-search", &icons::SEARCH, "Search", None, t)
                    .on_press(|s: &mut State| s.open_palette()),
                more_row("more-guide", &icons::CIRCLE_HELP, "Guide", None, t)
                    .on_press(|s: &mut State| s.toggle_guide()),
                more_row("more-theme", theme.1, theme.0, None, t)
                    .on_press(|s: &mut State| s.toggle_theme()),
                page_row(Page::Settings, t),
            ],
            t,
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_has_a_tab_or_a_row_in_more() {
        let in_more = [Page::Allocation, Page::Settings]
            .into_iter()
            .chain(Page::PLAN)
            .chain(Page::INSIGHTS)
            .chain([Page::Ask]);
        let mut reachable: Vec<Page> = Page::TABS.into_iter().chain(in_more).collect();
        reachable.sort_by_key(|page| page.key());
        reachable.dedup();
        assert_eq!(reachable.len(), Page::ALL.len(), "{reachable:?}");
    }
}
