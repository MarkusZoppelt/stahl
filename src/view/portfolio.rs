//! Overview, Holdings, Allocation and Performance.

use std::cmp::Ordering;

use portfolio_rs::state::{PositionDto, TopMoverDto};
use rune::{
    Beat, Color, Cx, El, Icon, Key, KeyEvent, PopupItem, PopupMenu, ScrollPos, Sequence, Tx, col,
    el, icon, popup_menu_supported, row, text, tok::motion::LEAD,
};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, RollingText, SegmentedControl, TextInput,
};

use crate::{
    backend::Period,
    format, icons,
    state::{Confirm, Page, SWIPE_ACTIONS, SortBy, State, file_name, swipe_key},
    theme::{HERO, HERO_COMPACT, HOLDING_TITLE, NUMBER, Theme},
    view::{charts, guide, overview, ui},
};

fn display_name(position: &PositionDto) -> String {
    position
        .name
        .clone()
        .or_else(|| position.ticker.clone())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn compare(a: &PositionDto, b: &PositionDto, sort: SortBy) -> Ordering {
    // Missing numbers sort as lowest, so they never lead a table.
    let number =
        |x: Option<f64>, y: Option<f64>| x.unwrap_or(f64::MIN).total_cmp(&y.unwrap_or(f64::MIN));
    match sort {
        SortBy::Name => display_name(a)
            .to_lowercase()
            .cmp(&display_name(b).to_lowercase()),
        SortBy::Class => a.asset_class.cmp(&b.asset_class),
        SortBy::Value => a.value.total_cmp(&b.value),
        SortBy::Pnl => number(a.pnl_percent, b.pnl_percent),
        SortBy::Day => number(a.day_change_percent, b.day_change_percent),
        SortBy::Impact => number(day_amount(a), day_amount(b)),
    }
}

/// A holding's change today, in the portfolio's currency.
///
/// `day_change_percent` is the price move, so the amount is that move applied to yesterday's
/// value. Cash and anything without a previous close contribute nothing. A total loss can't be
/// recovered from today's value alone, so it is left unknown.
pub fn day_amount(position: &PositionDto) -> Option<f64> {
    let percent = position.day_change_percent?;
    if !position.value.is_finite() || !percent.is_finite() || percent <= -100.0 {
        return None;
    }
    let factor = 1.0 + percent / 100.0;
    Some(position.value * (percent / 100.0) / factor)
}

/// `amount` as percentage points of the portfolio's value at the previous close.
///
/// A holding that is 40% of the portfolio and down 0.6% moves it by about 0.24 points, which is
/// more than a 5% holding up 2%. `None` when yesterday's total isn't positive.
pub fn day_points(amount: Option<f64>, previous_total: f64) -> Option<f64> {
    let amount = amount?;
    (previous_total > 0.0 && amount.is_finite()).then_some(amount / previous_total * 100.0)
}

/// The portfolio's value at the previous close, from today's total and today's move.
pub fn previous_total(total: f64, day_change: f64) -> f64 {
    if total.is_finite() && day_change.is_finite() {
        total - day_change
    } else {
        0.0
    }
}

/// Whether a position matches every word of a search.
pub fn matches_search(position: &PositionDto, query: &str) -> bool {
    let haystack = format!(
        "{} {} {}",
        position.name.as_deref().unwrap_or(""),
        position.ticker.as_deref().unwrap_or(""),
        position.asset_class
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

// --- Overview ---

const FLASH: &[Beat] = &[
    Beat::new(0.0).opacity(0.0),
    Beat::new(0.12).opacity(1.0),
    Beat::new(0.9).opacity(0.0),
];

/// The total, with today's move under it. The hero of the page.
fn hero_total(s: &State, t: &Theme, narrow: bool) -> El {
    let k = &t.kit;
    let Some(summary) = &s.summary else {
        return col();
    };
    let day = t.change(summary.day_change_percent);
    col()
        .gap(2.0)
        .child(ui::caption("Total value", t))
        .child(
            RollingText::new("hero-total", s.money(summary.total_value))
                .font(if narrow { HERO_COMPACT } else { HERO })
                .color(k.c.text_base)
                .value(summary.total_value)
                .build(k)
                .key("total-value"),
        )
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(
                    RollingText::new(
                        "hero-day",
                        format!(
                            "{} ({})",
                            s.signed_money(summary.day_change),
                            format::percent(summary.day_change_percent),
                        ),
                    )
                    .font(k.ty.ui_medium)
                    .color(day)
                    .value(summary.day_change)
                    .build(k),
                )
                .child(ui::muted("today", t)),
        )
}

/// How the total splits. Beside it when there's room; under it, or below the agenda on a phone.
fn hero_figures(s: &State, t: &Theme, room: ui::Room) -> El {
    let Some(summary) = &s.summary else {
        return col();
    };
    let figures = [
        ui::stat(
            "Unrealized PnL",
            s.signed_money(summary.total_pnl),
            Some(t.change(summary.total_pnl)),
            t,
        )
        .help(guide::UNREALIZED_PNL),
        ui::stat("Securities", s.money(summary.securities_value), None, t),
        ui::stat("Cash", s.money(summary.cash_value), None, t),
    ];
    match room {
        ui::Room::Wide => row().center_y().gap(36.0).pb(2.0).children(figures),
        _ => ui::columns(room, [3, 3, 2], 16.0).children(figures),
    }
}

fn hero(s: &State, t: &Theme, cx: &Cx<State>, with_figures: bool) -> El {
    let Some(summary) = &s.summary else {
        return col();
    };
    let room = ui::room(s, cx);
    let narrow = room == ui::Room::Narrow;
    let day = t.change(summary.day_change_percent);
    let total = hero_total(s, t, narrow);
    let body = if !with_figures {
        total
    } else if room == ui::Room::Wide {
        row()
            .center_y()
            .w_full()
            .justify_between()
            .items_end()
            .gap(32.0)
            .child(total)
            .child(hero_figures(s, t, room))
    } else {
        col()
            .w_full()
            .gap(16.0)
            .child(total)
            .child(hero_figures(s, t, room))
    };
    col()
        .key("hero")
        .w_full()
        .child(
            // A new key each refresh, which replays it: the numbers just changed, look here.
            el().key(format!("hero-flash-{}", s.price_updates))
                .absolute(0.0, 0.0)
                .w_full()
                .h_full()
                .radius(12.0)
                .fill(day.alpha(0.06))
                .a11y_hidden()
                .sequence(Sequence::new(FLASH)),
        )
        .child(body)
}

fn allocation_card(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let shares: Vec<_> = s
        .allocation
        .iter()
        .map(|item| (item.percent, t.class_color(&item.asset_class)))
        .collect();
    ui::ledger(t)
        .role("button")
        .label("Open Allocation")
        .focusable(true)
        .key("overview-allocation")
        .hover(k.c.hover)
        .on_press(|s: &mut State| s.navigate(Page::Allocation))
        .child(ui::title("Allocation", t))
        .child(ui::stacked_bar(&shares, t))
        .child(
            col()
                .gap(8.0)
                .children(s.allocation.iter().enumerate().map(|(index, item)| {
                    row()
                        .center_y()
                        .key(format!("legend-{index}"))
                        .matched(format!("class-{}", item.asset_class))
                        .w_full()
                        .justify_between()
                        .gap(12.0)
                        .child(
                            row()
                                .center_y()
                                .gap(8.0)
                                .center_y()
                                .shrink()
                                .child(ui::dot(t.class_color(&item.asset_class)))
                                .child(
                                    text(item.asset_class.clone())
                                        .font(k.ty.ui)
                                        .color(k.c.text_base)
                                        .truncate(),
                                ),
                        )
                        .child(
                            row()
                                .center_y()
                                .gap(12.0)
                                .child(ui::number(s.money(item.value), k.c.text_muted))
                                .child(row().center_y().w(48.0).justify_end().child(ui::number(
                                    format!("{:.1}%", item.percent),
                                    k.c.text_base,
                                ))),
                        )
                })),
        )
}

fn movers_card(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let mut movers: Vec<&PositionDto> = s
        .positions
        .iter()
        .filter(|position| {
            position
                .day_change_percent
                .is_some_and(|change| change != 0.0)
        })
        .collect();
    movers.sort_by(|a, b| {
        let size = |position: &PositionDto| position.day_change_percent.unwrap_or(0.0).abs();
        size(b).total_cmp(&size(a))
    });
    ui::ledger(t)
        .child(ui::title("Top movers today", t))
        .children(
            movers
                .is_empty()
                .then(|| ui::muted("No price movements today.", t)),
        )
        .child(
            col()
                .gap(6.0)
                .children(movers.into_iter().take(5).map(|position| {
                    let change = position.day_change_percent.unwrap_or(0.0);
                    let id = position.id;
                    row()
                        .center_y()
                        .key(format!("mover-{id}"))
                        .matched(format!("position-{id}"))
                        .role("button")
                        .label(display_name(position))
                        .focusable(true)
                        .w_full()
                        .justify_between()
                        .gap(12.0)
                        .py(2.0)
                        .radius(6.0)
                        .hover(k.c.hover)
                        .on_press(move |s: &mut State| s.reveal_position(id))
                        .child(
                            text(display_name(position))
                                .font(k.ty.ui)
                                .color(k.c.text_base)
                                .truncate()
                                .shrink(),
                        )
                        .child(ui::badge(format::percent(change), t.change(change)))
                })),
        )
}

/// One thing to do next, with the button that does it.
fn step(number: usize, heading: &str, detail: String, action: Option<El>, t: &Theme) -> El {
    let k = &t.kit;
    row()
        .center_y()
        .w_full()
        .gap(14.0)
        .py(10.0)
        .child(
            col()
                .size(24.0, 24.0)
                .radius(12.0)
                .center()
                .fill(k.c.layer_03)
                .a11y_hidden()
                .child(ui::number(number.to_string(), k.c.text_base)),
        )
        .child(
            col()
                .grow(1.0)
                .w(0.0)
                .min_w(0.0)
                .gap(2.0)
                .child(ui::title(heading, t))
                .child(ui::caption(detail, t).wrap()),
        )
        .children(action)
}

/// Overview before there's anything to add up: what to do, in order.
fn first_steps(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let plan = if s.is_workspace() {
        (!s.has_policy).then(|| {
            (
                "Set a policy",
                "Say how you want your money split. Review and Rebalance measure against it.",
                Button::new("Open Policy")
                    .on_press(|s: &mut State| s.navigate(Page::Policy))
                    .build(k)
                    .key("first-steps-policy"),
            )
        })
    } else {
        Some((
            "Create a workspace",
            "A single file can't hold a policy, reports or decisions. A workspace can.",
            Button::new("New Workspace…")
                .on_press(|s: &mut State| s.open_new_workspace())
                .build(k)
                .key("first-steps-workspace"),
        ))
    };
    let file = s
        .open
        .file_path
        .as_deref()
        .map_or("your positions file", file_name);
    let mut steps = vec![
        step(
            1,
            "Add your first position",
            "A share, a fund, crypto or a cash balance. Add the rest the same way.".to_string(),
            Some(ui::add_position_button("first-steps-add", t)),
            t,
        ),
        step(
            2,
            "Save",
            if s.compact {
                format!(
                    "Positions reach {file} when you tap Save at the top, or when you leave the app."
                )
            } else {
                format!(
                    "Positions reach {file} when you save, with {}.",
                    ui::chord(false, "S")
                )
            },
            None,
            t,
        ),
    ];
    if let Some((heading, detail, action)) = plan {
        steps.push(step(3, heading, detail.to_string(), Some(action), t));
    }
    steps.push(step(
        steps.len() + 1,
        "Check your currency",
        format!(
            "Totals are shown in {}. Stahl converts other currencies for you.",
            s.currency()
        ),
        Some(
            Button::new("Open Settings")
                .on_press(|s: &mut State| s.navigate(Page::Settings))
                .build(k)
                .key("first-steps-settings"),
        ),
        t,
    ));
    ui::entering(
        ui::card(t)
            .key("first-steps")
            .w_full()
            .gap(4.0)
            .child(ui::header(
                "Your portfolio is empty",
                "Once it has positions, this page says what changed, where you stand, and what to \
                 look at.",
                t,
            ))
            .child(col().w_full().mt(8.0).children(steps)),
        0,
    )
}

/// Overview reads top to bottom: what happened, where you stand, what to do. The month's chart
/// is on Performance, so a usual window shows this whole page without the agenda falling off it.
pub fn overview(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    if s.positions.is_empty() {
        return first_steps(s, t);
    }
    let room = ui::room(s, cx);
    let narrow = room == ui::Room::Narrow;
    // On a phone the three standing figures follow the agenda, so what to do is on screen
    // without scrolling. The total stays the hero, above it.
    col()
        .w_full()
        .gap(if narrow { 16.0 } else { 24.0 })
        .children(overview::statement(s, t, cx).map(|statement| ui::entering(statement, 0)))
        .child(hero(s, t, cx, !narrow))
        .child(ui::entering(overview::agenda(s, t, narrow), 1))
        .children(narrow.then(|| hero_figures(s, t, room)))
        .child(
            ui::columns(room, [2, 2, 1], if narrow { 16.0 } else { 32.0 })
                .child(ui::entering(allocation_card(s, t), 2))
                .child(ui::entering(movers_card(s, t), 3)),
        )
}

// --- Holdings ---

/// Which columns fit a table in `width` points of the content area. The details pane has them all.
#[derive(Clone, Copy)]
struct Columns {
    class: bool,
    pnl: bool,
    today: bool,
    /// How much of the portfolio's move today came from the holding. Narrower tables fold it
    /// under Today's own percentage instead of dropping it.
    impact: bool,
    /// Each holding's past month, when there's room and any holding has one.
    lines: bool,
}

/// How wide a row's line is, and its column.
const LINE_W: f32 = 72.0;

impl Columns {
    fn for_width(width: f32) -> Columns {
        Columns {
            class: width >= 720.0,
            pnl: width >= 560.0,
            today: width >= 340.0,
            // Beside the details pane the table already drops Class and PnL; this goes with them.
            impact: width >= 880.0,
            lines: false,
        }
    }

    /// With a column gone, the name takes more of what's left.
    fn name_grow(self) -> f32 {
        if self.class { 1.0 } else { 1.5 }
    }
}

/// Whether the Holdings table still works beside the details pane in a content area this wide:
/// less than this and the details take the table's place.
pub const TABLE_BESIDE_PANE: f32 = 300.0;

/// Below this, the search takes the row above the table, without the count beside it.
const ROOM_FOR_COUNT: f32 = 480.0;

fn header_cell(
    s: &State,
    key: &'static str,
    label: &'static str,
    sort: SortBy,
    right: bool,
    grow: f32,
    t: &Theme,
) -> El {
    let k = &t.kit;
    let active = s.sort == sort;
    let cell = row()
        .center_y()
        .key(key)
        .role("button")
        .label(format!("Sort by {label}"))
        .focusable(true)
        .grow(grow)
        .w(0.0)
        .min_w(0.0)
        .gap(4.0)
        .center_y()
        .on_press(move |s: &mut State| s.set_sort(sort))
        .child(text(label).font(k.ty.text_xsmall_medium).color(if active {
            k.c.text_base
        } else {
            k.c.text_muted
        }))
        .children(active.then(|| {
            icon(
                if s.ascending {
                    &icons::ARROW_UP
                } else {
                    &icons::ARROW_DOWN
                },
                11.0,
            )
            .color(k.c.text_base)
        }));
    if right { cell.justify_end() } else { cell }
}

/// One holding's share of today's move, for the table and the details.
struct TodayShare {
    /// Change in the portfolio's currency. `None` when it can't be known.
    amount: Option<f64>,
    /// Percentage points of yesterday's portfolio. `None` when it can't be known.
    points: Option<f64>,
}

fn today_share(position: &PositionDto, previous: f64) -> TodayShare {
    let amount = day_amount(position);
    TodayShare {
        points: day_points(amount, previous),
        amount,
    }
}

/// How far a move reaches from the centre, as a fraction of one half. A loss and a gain of the
/// same size reach equally far, on opposite sides.
pub(super) fn diverge_reach(amount: Option<f64>, largest: f64) -> (f32, bool) {
    let Some(amount) = amount.filter(|amount| amount.is_finite() && *amount != 0.0) else {
        return (0.0, true);
    };
    let reach = if largest > 0.0 {
        (amount.abs() / largest) as f32
    } else {
        1.0
    };
    (reach.clamp(0.0, 1.0), amount > 0.0)
}

/// Who moved the portfolio today: losses grow left of a centre line, gains right, both scaled
/// to the largest move.
fn share_bar(share: &TodayShare, largest: f64, t: &Theme) -> El {
    let (reach, gain) = diverge_reach(share.amount, largest);
    let fill = |on: bool, color: Color| {
        el().h(3.0)
            .w_frac(if on { reach } else { 0.0 })
            .radius(1.0)
            .fill(color)
            .motion(LEAD)
    };
    row()
        .center_y()
        .w_full()
        .h(7.0)
        .a11y_hidden()
        .child(
            row()
                .center_y()
                .grow(1.0)
                .h_full()
                .justify_end()
                .child(fill(!gain, t.down)),
        )
        .child(el().w(1.0).h_full().fill(t.kit.c.text_faint))
        .child(row().center_y().grow(1.0).h_full().child(fill(gain, t.up)))
}

struct RowFace<'a> {
    columns: Columns,
    share: &'a TodayShare,
    largest: f64,
    progress: f32,
    first: bool,
    last: bool,
}

fn holding_row(s: &State, position: &PositionDto, face: RowFace<'_>, t: &Theme) -> El {
    let RowFace {
        columns,
        share,
        largest,
        progress,
        first,
        last,
    } = face;
    let k = &t.kit;
    let id = position.id;
    let selected = s.selected == Some(id);
    let cell = |content: El, right: bool, grow: f32| {
        let cell = row().center_y().grow(grow).w(0.0).min_w(0.0);
        if right {
            cell.justify_end().child(content)
        } else {
            cell.child(content)
        }
    };
    let number = |value: String, color| ui::number(value, color).truncate().shrink();
    let name = display_name(position);
    let context_name = name.clone();
    let class_color = t.class_color(&position.asset_class);
    let share_label = share
        .points
        .map_or_else(|| "-".to_string(), format::percent);
    let named = if share.points.is_some() {
        format!("{name}, {share_label} of today's move")
    } else {
        name.clone()
    };
    let total = s
        .summary
        .as_ref()
        .map_or(0.0, |summary| summary.total_value);
    let weight = if total > 0.0 {
        (position.value / total).clamp(0.0, 1.0) as f32
    } else {
        0.0
    };
    row()
        .center_y()
        .key(format!("holding-{id}"))
        .matched(format!("position-{id}"))
        .role("row")
        .label(named)
        .focusable(true)
        .w_full()
        .gap(12.0)
        .px(12.0)
        .h(52.0)
        .radius(6.0)
        .center_y()
        .fill(if selected {
            k.c.layer_03
        } else {
            k.c.layer_03.alpha(0.0)
        })
        .hover(k.c.hover)
        .motion(LEAD)
        .press_scale(0.995)
        // The first stop in the table when Tab comes back to it, and where its arrows start.
        .focus_selected(selected)
        .on_press(move |s: &mut State| s.select_position(id))
        .on_key(
            move |s: &mut State, event: &KeyEvent, _: &mut Tx| match &event.key {
                Key::Enter if plain(event) => {
                    s.open_form(Some(id));
                    true
                }
                // The group presses the row a key lands on, even this one, and a press toggles. So
                // keep the keys that can't leave this row from the group: at an end of the table,
                // and letters, which would type-select.
                Key::Up | Key::Home => first,
                Key::Down | Key::End => last,
                Key::Text(typed) => !typed.trim().is_empty(),
                _ => false,
            },
        )
        .on_secondary_press(move |s: &mut State, press, tx: &mut Tx| {
            // Only macOS has pop-up menus. Elsewhere, the details pane has the same actions.
            if !popup_menu_supported() {
                s.reveal_position(id);
                return;
            }
            let name = context_name.clone();
            let menu = PopupMenu::new()
                .item(PopupItem::new("edit", "Edit…"))
                .item(PopupItem::new("copy", "Copy Name"))
                .separator()
                .item(PopupItem::new("delete", "Delete…"));
            tx.popup_menu(menu, press, move |s: &mut State, picked, tx| match picked {
                Some("edit") => s.open_form(Some(id)),
                Some("copy") => s.copy(name.clone(), tx),
                Some("delete") => {
                    s.confirm = Some(Confirm::Delete {
                        id,
                        name: name.clone(),
                    });
                }
                _ => {}
            });
        })
        // How much of the portfolio this is, along the bottom of the row.
        .child(
            el().key(format!("weight-{id}"))
                .pin(Some(12.0), None, Some(12.0), Some(0.0))
                .h(1.0)
                .a11y_hidden()
                .child(
                    el().h_full()
                        .w_frac(weight)
                        .radius(1.0)
                        .fill(class_color.alpha(0.75))
                        .motion(LEAD),
                ),
        )
        .child(cell(
            row()
                .center_y()
                .gap(12.0)
                .min_w(0.0)
                .shrink()
                .child(ui::dot(class_color))
                .child(
                    col()
                        .min_w(0.0)
                        .shrink()
                        .child(
                            text(name)
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate(),
                        )
                        .children(
                            position
                                .ticker
                                .clone()
                                .map(|ticker| ui::caption(ticker, t).truncate()),
                        ),
                ),
            false,
            columns.name_grow(),
        ))
        // A holding without a line keeps the column's width, so the others stay aligned.
        .children(columns.lines.then(|| {
            sparkline(s, position, LINE_W, 18.0, progress, t)
                .unwrap_or_else(|| el().w(LINE_W).h(18.0).a11y_hidden())
        }))
        .children(columns.class.then(|| {
            cell(
                ui::muted(position.asset_class.clone(), t)
                    .truncate()
                    .shrink(),
                false,
                1.0,
            )
        }))
        .child(cell(
            number(s.money(position.value), k.c.text_base),
            true,
            1.0,
        ))
        .children(columns.pnl.then(|| {
            cell(
                number(
                    format::optional_percent(position.pnl_percent),
                    t.optional_change(position.pnl_percent),
                ),
                true,
                1.0,
            )
        }))
        .children(columns.today.then(|| {
            let own = number(
                format::optional_percent(position.day_change_percent),
                t.optional_change(position.day_change_percent),
            );
            // No room for the column: the bar under the holding's own percentage still says
            // which move mattered. The number is a tap away, in the details.
            let content = if columns.impact {
                own
            } else {
                col()
                    .w_full()
                    .items_end()
                    .gap(4.0)
                    .child(own)
                    .child(share_bar(share, largest, t).max_w(72.0))
                    .help("The bar is this holding's share of today's move")
            };
            cell(content, true, 1.0)
        }))
        .children(
            columns
                .impact
                .then(|| cell(share_figure(s, share, largest, &share_label, t), true, 1.0)),
        )
}

/// The holding's line over the past month, or nothing when it has no series.
fn sparkline(
    s: &State,
    position: &PositionDto,
    width: f32,
    height: f32,
    progress: f32,
    t: &Theme,
) -> Option<El> {
    let values = position.position_id.as_deref().and_then(|id| {
        s.history
            .as_ref()
            .and_then(|history| history.series(id))
            .map(<[f64]>::to_vec)
    })?;
    Some(
        charts::sparkline(
            &values,
            width,
            height,
            charts::spark_color(&values, t),
            progress,
        )
        .key(format!("spark-{}", position.id)),
    )
}

/// The share of today's move: the points, and a bar against the largest holding.
fn share_figure(s: &State, share: &TodayShare, largest: f64, label: &str, t: &Theme) -> El {
    let color = share
        .points
        .map_or(t.kit.c.text_muted, |points| t.change(points));
    let help = match share.amount {
        Some(amount) if !s.privacy => {
            format!("Moved the portfolio by {} today", s.signed_money(amount))
        }
        _ => "How many percentage points of the portfolio this holding moved today".to_string(),
    };
    col()
        .w_full()
        .min_w(88.0)
        .items_end()
        .gap(4.0)
        .help(help)
        .child(ui::number(label.to_string(), color))
        .child(share_bar(share, largest, t))
}

/// A holding's row on a phone, which slides left to show Edit and Delete, as rows in iOS do.
/// Where it comes to rest is settled when the finger lifts (`State::settle_swipes`).
fn swipe_row(s: &State, position: &PositionDto, content: El, t: &Theme) -> El {
    let k = &t.kit;
    let id = position.id;
    let name = display_name(position);
    let swipes = s.swipes.clone();
    let action = |key: String, glyph: &'static Icon, label: &str, fill: Color, ink: Color| {
        col()
            .key(key)
            .role("button")
            .label(label.to_string())
            .grow(1.0)
            .w(0.0)
            .h(44.0)
            .gap(2.0)
            .center()
            .radius(6.0)
            .fill(fill)
            .press_scale(0.95)
            .child(icon(glyph, 16.0).color(ink))
            .child(
                text(label.to_string())
                    .font(k.ty.text_xsmall_medium)
                    .color(ink),
            )
    };
    let shut = move |tx: &mut Tx| tx.scroll_to(swipe_key(id), 0.0, 0.0);
    let actions = row()
        .center_y()
        .w(SWIPE_ACTIONS)
        .h(52.0)
        .gap(6.0)
        .pl(8.0)
        .child(
            action(
                format!("swipe-edit-{id}"),
                &icons::PENCIL,
                "Edit",
                k.c.layer_03,
                k.c.text_base,
            )
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                shut(tx);
                s.open_form(Some(id));
            }),
        )
        .child(
            action(
                format!("swipe-delete-{id}"),
                &icons::TRASH,
                "Delete",
                t.down,
                k.c.bg_base,
            )
            .on_press_tx(move |s: &mut State, tx: &mut Tx| {
                shut(tx);
                s.confirm = Some(Confirm::Delete {
                    id,
                    name: name.clone(),
                });
            }),
        );
    row()
        .key(swipe_key(id))
        .w_full()
        .scroll_x()
        .on_scroll_offset(move |at: ScrollPos| {
            if let Ok(mut rows) = swipes.lock() {
                let before = rows.offsets.insert(id, at.offset[0]);
                // Every row reports where it starts; only a row that moved was slid.
                if before.is_some_and(|x| (x - at.offset[0]).abs() > 0.5) {
                    rows.last = Some(id);
                }
            }
        })
        .child(content.w_full())
        .child(actions)
}

fn search_input(s: &State) -> TextInput {
    let input = TextInput::new(&s.search)
        .placeholder("Search holdings")
        .leading_icon(&icons::SEARCH);
    if s.search.text.is_empty() {
        input
    } else {
        input.clear_button()
    }
}

/// The rows of the Holdings table, top to bottom.
pub(super) fn holding_rows(s: &State) -> Vec<&PositionDto> {
    let mut rows: Vec<&PositionDto> = s
        .positions
        .iter()
        .filter(|position| matches_search(position, &s.search.text))
        .collect();
    let (sort, ascending) = (s.sort, s.ascending);
    rows.sort_by(|a, b| {
        let order = compare(a, b, sort);
        if ascending { order } else { order.reverse() }
    });
    rows
}

fn plain(event: &KeyEvent) -> bool {
    let mods = event.mods;
    !(mods.shift || mods.ctrl || mods.alt || mods.cmd)
}

/// The table's keys while nothing has focus: a selection made from Overview or the palette has no
/// focused row yet. Once a row has focus, its group and its own handler take these, and any other
/// focused control keeps them, so this never runs for a search field or a button.
pub fn holdings_key(s: &mut State, event: &KeyEvent, tx: &mut Tx) -> bool {
    let idle = s.page == Page::Holdings
        && s.has_portfolio()
        && s.open.parse_error.is_none()
        && s.palette.is_none()
        && s.form.is_none()
        && s.confirm.is_none();
    if !idle || !plain(event) || tx.focused().is_some() {
        return false;
    }
    if event.key == Key::Enter {
        let Some(id) = s.selected else {
            return false;
        };
        s.open_form(Some(id));
        return true;
    }
    if s.holdings_map {
        return matches!(event.key, Key::Up | Key::Down | Key::Left | Key::Right)
            && super::treemap::focus_tile(s, tx);
    }
    if !matches!(event.key, Key::Up | Key::Down) {
        return false;
    }
    let rows: Vec<usize> = holding_rows(s)
        .into_iter()
        .map(|position| position.id)
        .collect();
    let Some(last) = rows.len().checked_sub(1) else {
        return false;
    };
    let down = event.key == Key::Down;
    let current = rows.iter().position(|id| Some(*id) == s.selected);
    let target = rows[match current {
        Some(index) if down => (index + 1).min(last),
        Some(index) => index.saturating_sub(1),
        None if down => 0,
        None => last,
    }];
    // `select_position` toggles, and at an end of the table this is the row already selected.
    if s.selected != Some(target) {
        s.select_position(target);
    }
    tx.scroll_into_view(format!("holding-{target}"));
    true
}

pub fn holdings(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    if s.positions.is_empty() {
        return ui::empty_state(
            "holdings-empty",
            &icons::WALLET,
            "No positions yet",
            "A position is one thing you own: a share, a fund, crypto or a cash balance. Each one \
             gets a row here with its value and gain or loss.",
            ui::add_position_button("holdings-empty-add", t),
            t,
        );
    }
    let rows = holding_rows(s);
    let end = rows.len().saturating_sub(1);
    let width = cx.layout_size("main").map_or(f32::MAX, |[width, _]| width);
    let has_lines = s
        .history
        .as_ref()
        .is_some_and(|history| !history.series.is_empty());
    let columns = Columns {
        // On a phone the line would push the value into an ellipsis: the map has the lines there.
        lines: has_lines && Columns::for_width(width).pnl,
        ..Columns::for_width(width)
    };
    let previous = s.summary.as_ref().map_or(0.0, |summary| {
        previous_total(summary.total_value, summary.day_change)
    });
    // Against what's on screen, so a holding the search hid doesn't shrink the bars.
    let largest = rows
        .iter()
        .filter_map(|position| day_amount(position))
        .map(f64::abs)
        .fold(0.0, f64::max);
    let shares: Vec<TodayShare> = rows
        .iter()
        .map(|position| today_share(position, previous))
        .collect();
    let roomy = width >= ROOM_FOR_COUNT;
    let search = search_input(s)
        .width(roomy.then_some(240.0))
        .on_edit(|s: &mut State| &mut s.search)
        .build(k)
        .key("holdings-search");
    let add = Button::new("Add")
        .variant(ButtonVariant::Submit)
        .icon(&icons::PLUS)
        .shortcut(ui::chord(false, "N"))
        .on_press(|s: &mut State| s.open_form(None))
        .build(k)
        .key("add-position");
    let view = SegmentedControl::new(&["Table", "Map"], Some(usize::from(s.holdings_map)))
        .label("View")
        .fit()
        .on_pick(|s: &mut State, index, tx: &mut Tx| {
            s.holdings_map = index == 1;
            if s.holdings_map
                && let Some(id) = s.selected
            {
                tx.focus(format!("tile-{id}"), false);
            }
        })
        .build(k)
        .key("holdings-view");
    let top = if roomy {
        row()
            .center_y()
            .w_full()
            .justify_between()
            .gap(16.0)
            .child(ui::muted(format!("{} positions", s.positions.len()), t))
            .child(
                row()
                    .center_y()
                    .gap(8.0)
                    .child(view)
                    .child(search)
                    .child(add),
            )
    } else {
        col().w_full().gap(8.0).child(view).child(
            row()
                .center_y()
                .w_full()
                .gap(8.0)
                .child(search.grow(1.0).min_w(0.0))
                .child(add),
        )
    };
    let map = s.holdings_map.then(|| {
        let measured = cx.layout_size("holdings-map").map(|[width, _]| width);
        let map_width = measured.unwrap_or((width - 64.0).max(1.0));
        col()
            .w_full()
            .gap(8.0)
            .child(
                ui::caption(
                    if has_lines {
                        "Area is value, the tint is today's change, and lines are the past month."
                    } else {
                        "Area is value, and the tint is today's change."
                    },
                    t,
                )
                .wrap()
                .key("map-about"),
            )
            .child(super::treemap::view(
                s,
                t,
                map_width,
                super::treemap::height(s, cx.layout_size("main")),
                s.draw_progress(cx.is_reduced_motion()),
            ))
    });
    let table = (!s.holdings_map).then(|| {
        ui::card(t)
            .padding(8.0)
            .pt(12.0)
            .gap(0.0)
            .child(
                row()
                    .center_y()
                    .w_full()
                    .gap(12.0)
                    .px(12.0)
                    .pb(8.0)
                    .child(header_cell(
                        s,
                        "sort-name",
                        "Name",
                        SortBy::Name,
                        false,
                        columns.name_grow(),
                        t,
                    ))
                    .children(columns.lines.then(|| {
                        row().w(LINE_W).child(
                            text("Past month")
                                .font(k.ty.text_xsmall_medium)
                                .color(k.c.text_muted)
                                .truncate(),
                        )
                    }))
                    .children(columns.class.then(|| {
                        header_cell(s, "sort-class", "Class", SortBy::Class, false, 1.0, t)
                    }))
                    .child(header_cell(
                        s,
                        "sort-value",
                        "Value",
                        SortBy::Value,
                        true,
                        1.0,
                        t,
                    ))
                    .children(
                        columns
                            .pnl
                            .then(|| header_cell(s, "sort-pnl", "PnL", SortBy::Pnl, true, 1.0, t)),
                    )
                    .children(
                        columns.today.then(|| {
                            header_cell(s, "sort-day", "Today", SortBy::Day, true, 1.0, t)
                        }),
                    )
                    .children(columns.impact.then(|| {
                        header_cell(s, "sort-impact", "Of today", SortBy::Impact, true, 1.0, t).help(
                            "How many percentage points of the portfolio this holding moved today",
                        )
                    })),
            )
            .children(
                rows.is_empty()
                    .then(|| ui::muted("No holdings match.", t).padding(12.0)),
            )
            .child(
                col()
                    .key("holdings-rows")
                    .role("list")
                    .label("Holdings")
                    .focus_group()
                    .focus_follows()
                    .w_full()
                    .children(rows.into_iter().zip(shares).enumerate().map(
                        |(index, (position, share))| {
                            let row = holding_row(
                                s,
                                position,
                                RowFace {
                                    columns,
                                    share: &share,
                                    largest,
                                    progress: s.draw_progress(cx.is_reduced_motion()),
                                    first: index == 0,
                                    last: index == end,
                                },
                                t,
                            );
                            if s.compact {
                                swipe_row(s, position, row, t)
                            } else {
                                row
                            }
                        },
                    )),
            )
    });
    col()
        .w_full()
        .gap(16.0)
        .child(top)
        .children(map)
        .children(table)
}

fn detail(label: impl Into<String>, value: String, t: &Theme) -> El {
    col()
        .min_w(0.0)
        .gap(2.0)
        .child(ui::caption(label, t))
        .child(
            text(value)
                .font(NUMBER)
                .color(t.kit.c.text_base)
                .truncate()
                .selectable(true),
        )
}

/// The detail pane beside the table.
pub fn inspector(s: &State, position: &PositionDto, t: &Theme) -> El {
    let k = &t.kit;
    let id = position.id;
    let name = display_name(position);
    let subtitle = match &position.ticker {
        Some(ticker) => format!("{} · {ticker}", position.asset_class),
        None => position.asset_class.clone(),
    };
    let price = if position.price > 0.0 {
        s.unit_money(position, position.price)
    } else {
        "-".to_string()
    };
    let pnl = position
        .pnl
        .map_or_else(|| "-".to_string(), |pnl| s.signed_money(pnl));
    let confirm_name = name.clone();
    col()
        .key("inspector")
        .w_full()
        .h_full()
        .padding(20.0)
        .gap(16.0)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_between()
                .items_start()
                .child(
                    col()
                        .shrink()
                        .child(text(name).font(HOLDING_TITLE).color(k.c.text_base).wrap())
                        .child(ui::caption(subtitle, t)),
                )
                .child(
                    IconButton::new(k.glyph.close)
                        .variant(IconButtonVariant::Ghost)
                        .label("Close")
                        .on_press(move |s: &mut State| s.select_position(id))
                        .build(k)
                        .key("close-inspector"),
                ),
        )
        .child(ui::stat("Value", s.money(position.value), None, t))
        .child(
            ui::columns(ui::Room::Wide, [2, 2, 2], 12.0)
                .flex_wrap()
                .child(detail("Quantity", format!("{:.4}", position.amount), t))
                .child(detail("Price", price, t))
                .children(
                    s.base_price(position).map(|price| {
                        detail(format!("Price ({})", s.currency()), s.money(price), t)
                    }),
                )
                .child(detail(
                    "Average cost",
                    position
                        .average_cost
                        .map_or_else(|| "-".to_string(), |cost| s.unit_money(position, cost)),
                    t,
                ))
                .child(detail("Invested", s.optional_money(position.invested), t))
                .child(detail("PnL", pnl, t))
                .child(detail(
                    "Today",
                    format::optional_percent(position.day_change_percent),
                    t,
                ))
                .child(detail(
                    "Of today",
                    day_points(
                        day_amount(position),
                        s.summary.as_ref().map_or(0.0, |summary| {
                            previous_total(summary.total_value, summary.day_change)
                        }),
                    )
                    .map_or_else(|| "-".to_string(), format::percent),
                    t,
                )),
        )
        .children((!position.purchases.is_empty()).then(|| {
            col()
                .gap(6.0)
                .child(ui::caption(
                    format!("Purchases ({})", position.purchases.len()),
                    t,
                ))
                .children(position.purchases.iter().map(|purchase| {
                    let price = purchase
                        .price
                        .map_or_else(|| "-".to_string(), |price| s.unit_money(position, price));
                    row()
                        .center_y()
                        .w_full()
                        .justify_between()
                        .gap(8.0)
                        .child(ui::caption(
                            purchase
                                .date
                                .clone()
                                .unwrap_or_else(|| "Undated".to_string()),
                            t,
                        ))
                        .child(ui::number(
                            format!("{} @ {price}", purchase.quantity),
                            k.c.text_base,
                        ))
                }))
        }))
        .child(
            row()
                .center_y()
                .w_full()
                .gap(8.0)
                .child(
                    Button::new("Edit")
                        .icon(&icons::PENCIL)
                        .on_press(move |s: &mut State| s.open_form(Some(id)))
                        .build(k)
                        .key("edit-position")
                        .grow(1.0),
                )
                .child(
                    Button::new("Delete")
                        .variant(ButtonVariant::Danger)
                        .icon(&icons::TRASH)
                        .on_press(move |s: &mut State| {
                            s.confirm = Some(Confirm::Delete {
                                id,
                                name: confirm_name.clone(),
                            });
                        })
                        .build(k)
                        .key("delete-position"),
                ),
        )
}

/// The details in place of the table, for a window too narrow to show both.
pub fn details_page(s: &State, position: &PositionDto, t: &Theme) -> El {
    ui::card(t)
        .padding(0.0)
        .gap(0.0)
        .clip_rounded(14.0)
        .child(inspector(s, position, t))
}

// --- Allocation ---

pub fn allocation(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    if s.positions.is_empty() {
        return ui::empty_state(
            "allocation-empty",
            &icons::CHART_PIE,
            "Nothing to split yet",
            "This page shows how your money divides across asset classes such as stocks, bonds \
             and cash. Add a position to see it.",
            ui::add_position_button("allocation-empty-add", t),
            t,
        );
    }
    let review = s.review.data.as_ref();
    // On a phone the target takes a line of its own, so the class keeps its name.
    let narrow = ui::room(s, cx) == ui::Room::Narrow;
    let rows = s.allocation.iter().enumerate().map(|(index, item)| {
        let target = review.and_then(|review| {
            review
                .allocations
                .iter()
                .find(|target| target.asset_class == item.asset_class)
        });
        let hovered = s.donut_hover == Some(index);
        let badge = |on_its_own_line: bool| {
            target.filter(|_| narrow == on_its_own_line).map(|target| {
                ui::badge(
                    format!(
                        "target {:.0}% · {}",
                        target.target_percent,
                        format::percent(target.drift_percent)
                    ),
                    if target.within_tolerance {
                        t.up
                    } else {
                        t.down
                    },
                )
            })
        };
        col()
            .key(format!("allocation-{index}"))
            .matched(format!("class-{}", item.asset_class))
            .w_full()
            .gap(6.0)
            .px(8.0)
            .py(6.0)
            .radius(8.0)
            .fill(if hovered {
                k.c.hover
            } else {
                k.c.hover.alpha(0.0)
            })
            .motion(LEAD)
            .on_hover_change(move |s: &mut State, over, _| {
                s.donut_hover = over.then_some(index);
            })
            .children(ui::class_flash(s, &item.asset_class, 8.0, t))
            .child(
                row()
                    .center_y()
                    .w_full()
                    .justify_between()
                    .gap(12.0)
                    .child(
                        row()
                            .center_y()
                            .gap(8.0)
                            .center_y()
                            .shrink()
                            .child(ui::dot(t.class_color(&item.asset_class)))
                            .child(
                                text(item.asset_class.clone())
                                    .font(k.ty.ui_medium)
                                    .color(k.c.text_base)
                                    .truncate(),
                            )
                            .children(badge(false)),
                    )
                    .child(
                        row()
                            .center_y()
                            .gap(12.0)
                            .child(ui::number(s.money(item.value), k.c.text_muted))
                            .child(ui::number(format!("{:.1}%", item.percent), k.c.text_base)),
                    ),
            )
            .children(badge(true).map(|badge| row().child(badge)))
            .child(ui::share_bar(
                format!("share-{index}"),
                item.percent,
                target.map(|target| target.target_percent),
                t.class_color(&item.asset_class),
                t,
            ))
    });
    let rows: Vec<El> = rows.collect();
    ui::columns(ui::room(s, cx), [2, 1, 1], 16.0)
        .child(ui::card(t).center().child(charts::donut(s, t, cx)))
        .child(
            ui::card(t)
                .child(ui::header(
                    "Breakdown",
                    if review.is_some() {
                        "The outer ring shows your policy targets"
                    } else if s.is_workspace() {
                        "Share of total value. Set a policy to see your targets here."
                    } else {
                        "Share of total value"
                    },
                    t,
                ))
                .child(col().w_full().gap(4.0).children(rows)),
        )
}

// --- Performance ---

fn movers(
    title: &'static str,
    empty: &'static str,
    movers: &[TopMoverDto],
    s: &State,
    t: &Theme,
) -> El {
    let k = &t.kit;
    ui::card(t)
        .child(ui::title(title, t))
        .children(movers.is_empty().then(|| ui::muted(empty, t)))
        .children(movers.iter().map(|mover| {
            row()
                .center_y()
                .w_full()
                .justify_between()
                .gap(12.0)
                .child(
                    text(mover.name.clone())
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .truncate()
                        .shrink(),
                )
                .child(
                    row()
                        .center_y()
                        .gap(12.0)
                        .child(ui::number(s.signed_money(mover.pnl), k.c.text_muted))
                        .child(ui::badge(
                            format::percent(mover.percent),
                            t.change(mover.percent),
                        )),
                )
        }))
}

pub fn performance(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    if s.positions.is_empty() {
        return ui::empty_state(
            "performance-empty",
            &icons::TRENDING_UP,
            "No history yet",
            "Once you have positions, Stahl records their total value once a day while you use \
             it, and charts it here.",
            ui::add_position_button("performance-empty-add", t),
            t,
        );
    }
    let Some(performance) = &s.performance else {
        return col();
    };
    let labels = Period::ALL.map(Period::label);
    let selected = Period::ALL.iter().position(|period| *period == s.period);
    let selector = SegmentedControl::new(&labels, selected)
        .label("Period")
        .fit()
        .on_pick(|s: &mut State, index, _| s.set_period(Period::ALL[index]))
        .build(&t.kit)
        .key("period");
    let history = s.history.as_ref();
    let optional =
        |value: Option<f64>| value.map_or_else(|| "-".to_string(), |value| s.signed_money(value));
    let change = history.and_then(|history| history.change);
    let twr = history.and_then(|history| history.twr_percent);
    let contributions = history.and_then(|history| history.net_contributions);
    let market = history.and_then(|history| history.market_effect);
    let room = ui::room(s, cx);
    let points = history.map_or(&[][..], |history| &history.points[..]);
    col()
        .w_full()
        .gap(16.0)
        .children(
            s.history_error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t)),
        )
        .child(
            ui::card(t)
                .gap(16.0)
                .child(
                    row()
                        .center_y()
                        .w_full()
                        .justify_between()
                        .flex_wrap()
                        .gaps(16.0, 12.0)
                        .child(ui::title("Value over time", t))
                        .child(selector),
                )
                .child(
                    ui::columns(room, [4, 2, 2], 12.0)
                        .child(ui::stat(
                            "Change",
                            optional(change),
                            change.map(|v| t.change(v)),
                            t,
                        ))
                        .child(
                            ui::stat(
                                "Time-weighted return",
                                twr.map_or_else(|| "-".to_string(), format::percent),
                                twr.map(|v| t.change(v)),
                                t,
                            )
                            .help(guide::TWR),
                        )
                        .child(
                            ui::stat("Net contributions", optional(contributions), None, t)
                                .help(guide::NET_CONTRIBUTIONS),
                        )
                        .child(
                            ui::stat(
                                "Market effect",
                                optional(market),
                                market.map(|v| t.change(v)),
                                t,
                            )
                            .help(guide::MARKET_EFFECT),
                        ),
                )
                .child(charts::value_chart(
                    s,
                    points,
                    t,
                    cx,
                    "performance-chart",
                    280.0,
                ))
                .children(
                    history
                        .and_then(|history| history.path.clone())
                        .map(|path| {
                            ui::caption(format!("History is stored in {}", format::place(&path)), t)
                                .truncate_middle()
                        }),
                ),
        )
        .child(
            ui::columns(room, [4, 2, 1], 12.0)
                .child(
                    ui::stat_card("Invested", s.money(performance.invested), None, t)
                        .help(guide::INVESTED),
                )
                .child(
                    ui::stat_card(
                        "Unrealized PnL",
                        s.signed_money(performance.unrealized_pnl),
                        Some(t.change(performance.unrealized_pnl)),
                        t,
                    )
                    .help(guide::UNREALIZED_PNL),
                )
                .child(
                    ui::stat_card(
                        "Total return",
                        format::percent(performance.pnl_percent),
                        Some(t.change(performance.pnl_percent)),
                        t,
                    )
                    .help(guide::TOTAL_RETURN),
                )
                .child(ui::stat_card(
                    "Today",
                    format!(
                        "{} ({})",
                        s.signed_money(performance.day_pnl),
                        format::percent(performance.day_percent),
                    ),
                    Some(t.change(performance.day_pnl)),
                    t,
                )),
        )
        .child(
            ui::columns(room, [2, 2, 1], 16.0)
                .child(movers(
                    "Top gainers today",
                    "No gainers today.",
                    &performance.top_gainers,
                    s,
                    t,
                ))
                .child(movers(
                    "Top losers today",
                    "No losers today.",
                    &performance.top_losers,
                    s,
                    t,
                )),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(name: Option<&str>, ticker: Option<&str>, pnl: Option<f64>) -> PositionDto {
        PositionDto {
            id: 0,
            position_id: None,
            name: name.map(str::to_string),
            ticker: ticker.map(str::to_string),
            asset_class: "Stock".to_string(),
            amount: 1.0,
            price: 1.0,
            value: 1.0,
            pnl: None,
            pnl_percent: pnl,
            day_change_percent: None,
            average_cost: None,
            invested: None,
            purchases: Vec::new(),
        }
    }

    #[test]
    fn search_matches_every_word_across_fields() {
        let apple = position(Some("Apple Inc"), Some("AAPL"), None);
        assert!(matches_search(&apple, ""));
        assert!(matches_search(&apple, "aapl"));
        assert!(matches_search(&apple, "apple stock"));
        assert!(!matches_search(&apple, "apple bond"));
    }

    #[test]
    fn sorting_puts_missing_numbers_lowest() {
        let known = position(Some("A"), None, Some(-50.0));
        let unknown = position(Some("B"), None, None);
        assert_eq!(compare(&known, &unknown, SortBy::Pnl), Ordering::Greater);
    }

    fn holding(value: f64, day: Option<f64>) -> PositionDto {
        PositionDto {
            value,
            day_change_percent: day,
            ..position(Some("Held"), None, None)
        }
    }

    #[test]
    fn a_large_holding_moves_the_portfolio_more_than_a_lively_small_one() {
        // 40% of yesterday's 100, down 0.6%: about -0.24 points.
        let heavy = holding(40.0 * 0.994, Some(-0.6));
        // 5% of it, up 2%: about +0.10 points.
        let light = holding(5.0 * 1.02, Some(2.0));
        let heavy_points = day_points(day_amount(&heavy), 100.0).unwrap();
        let light_points = day_points(day_amount(&light), 100.0).unwrap();
        assert!(
            (heavy_points + 0.24).abs() < 0.01,
            "heavy moved {heavy_points}"
        );
        assert!(
            (light_points - 0.10).abs() < 0.01,
            "light moved {light_points}"
        );
        assert!(heavy_points.abs() > light_points.abs());
        assert_eq!(
            compare(&heavy, &light, SortBy::Impact),
            Ordering::Less,
            "the loss sorts below the gain"
        );
    }

    #[test]
    fn a_loss_and_a_gain_reach_opposite_ways_from_the_centre() {
        let (loss, loss_gain) = diverge_reach(Some(-240.0), 450.0);
        let (gain, is_gain) = diverge_reach(Some(100.0), 450.0);
        let (full, _) = diverge_reach(Some(450.0), 450.0);
        assert!(!loss_gain && is_gain);
        assert!((loss - 240.0 / 450.0).abs() < 0.001);
        assert!(gain < loss, "the smaller move reaches less far");
        assert!(
            (full - 1.0).abs() < 0.001,
            "the largest move fills its half"
        );
        assert_eq!(diverge_reach(None, 450.0).0, 0.0);
        assert_eq!(diverge_reach(Some(0.0), 450.0).0, 0.0);
    }

    #[test]
    fn cash_and_a_total_loss_contribute_nothing_known() {
        assert_eq!(day_amount(&holding(30_000.0, None)), None);
        assert_eq!(day_amount(&holding(0.0, Some(-100.0))), None);
        assert_eq!(day_points(None, 100.0), None);
        assert_eq!(day_points(Some(1.0), 0.0), None);
        assert_eq!(previous_total(107_400.0, 280.0), 107_120.0);
    }

    #[test]
    fn names_fall_back_to_the_ticker() {
        assert_eq!(display_name(&position(None, Some("SPY"), None)), "SPY");
        assert_eq!(display_name(&position(None, None, None)), "Unknown");
    }
}
