//! Overview, Holdings, Allocation and Performance.

use std::cmp::Ordering;

use portfolio_rs::{
    mcp::dto::Severity,
    state::{PositionDto, TopMoverDto},
};
use rune::{
    Beat, Cx, El, Key, KeyEvent, PopupItem, PopupMenu, Sequence, Tx, col, el, icon,
    popup_menu_supported, row, text, tok::motion::LEAD,
};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, RollingText, SegmentedControl, TextInput,
};

use crate::{
    backend::Period,
    format, icons,
    state::{Confirm, Page, SortBy, State, file_name},
    theme::{HERO, HOLDING_TITLE, NUMBER, Theme},
    view::{charts, guide, ui},
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

fn hero(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    let Some(summary) = &s.summary else {
        return col();
    };
    let points = s
        .history
        .as_ref()
        .map_or(&[][..], |history| &history.points[..]);
    let day = t.change(summary.day_change_percent);
    col()
        .key("hero")
        .w_full()
        .gap(12.0)
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
        .child(
            row()
                .center_y()
                .w_full()
                .justify_between()
                .items_end()
                .gap(32.0)
                .child(
                    col()
                        .gap(2.0)
                        .child(ui::caption("Total value", t))
                        .child(
                            RollingText::new("hero-total", s.money(summary.total_value))
                                .font(HERO)
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
                        ),
                )
                .child(
                    row()
                        .center_y()
                        .gap(36.0)
                        .pb(2.0)
                        .child(
                            ui::stat(
                                "Unrealized PnL",
                                s.signed_money(summary.total_pnl),
                                Some(t.change(summary.total_pnl)),
                                t,
                            )
                            .help(guide::UNREALIZED_PNL),
                        )
                        .child(ui::stat(
                            "Securities",
                            s.money(summary.securities_value),
                            None,
                            t,
                        ))
                        .child(ui::stat("Cash", s.money(summary.cash_value), None, t)),
                ),
        )
        .child(col().w_full().child(charts::value_chart(
            s,
            points,
            t,
            cx,
            "overview-chart",
            230.0,
        )))
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

fn attention_card(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let mut findings: Vec<_> = s
        .findings
        .iter()
        .filter(|finding| !matches!(finding.severity, Severity::Info))
        .collect();
    findings.sort_by_key(|finding| !matches!(finding.severity, Severity::Critical));
    ui::ledger(t)
        .child(ui::title("Needs attention", t))
        .children(findings.is_empty().then(|| {
            row()
                .center_y()
                .gap(8.0)
                .center_y()
                .child(icon(&icons::CHECK_CIRCLE, 16.0).color(t.up))
                .child(
                    text("Nothing right now. Your data looks good.")
                        .font(k.ty.ui)
                        .color(k.c.text_muted)
                        .wrap()
                        .shrink(),
                )
        }))
        .child(
            col()
                .gap(8.0)
                .children(findings.into_iter().take(4).map(|finding| {
                    let color = match finding.severity {
                        Severity::Critical => t.down,
                        _ => t.warn,
                    };
                    ui::finding(color, finding.message.clone(), None, t)
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
            format!(
                "Positions reach {file} when you save, with {}.",
                ui::chord(false, "S")
            ),
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
                "Once it has positions, this page shows their total value, how it has moved, and \
                 anything that needs a look.",
                t,
            ))
            .child(col().w_full().mt(8.0).children(steps)),
        0,
    )
}

pub fn overview(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    if s.positions.is_empty() {
        return first_steps(s, t);
    }
    col().w_full().gap(28.0).child(hero(s, t, cx)).child(
        ui::columns(ui::is_wide(cx), 3, 2, 32.0)
            .child(ui::entering(allocation_card(s, t), 0))
            .child(ui::entering(movers_card(s, t), 1))
            .child(ui::entering(attention_card(s, t), 2)),
    )
}

// --- Holdings ---

/// Which columns fit a table in `width` points of the content area. The details pane has them all.
#[derive(Clone, Copy)]
struct Columns {
    class: bool,
    pnl: bool,
    today: bool,
}

impl Columns {
    fn for_width(width: f32) -> Columns {
        Columns {
            class: width >= 720.0,
            pnl: width >= 560.0,
            today: width >= 400.0,
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

fn holding_row(
    s: &State,
    position: &PositionDto,
    columns: Columns,
    first: bool,
    last: bool,
    t: &Theme,
) -> El {
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
        .label(name.clone())
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
            cell(
                number(
                    format::optional_percent(position.day_change_percent),
                    t.optional_change(position.day_change_percent),
                ),
                true,
                1.0,
            )
        }))
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
fn holding_rows(s: &State) -> Vec<&PositionDto> {
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
    let columns = Columns::for_width(width);
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
    let top = if roomy {
        row()
            .center_y()
            .w_full()
            .justify_between()
            .gap(16.0)
            .child(ui::muted(format!("{} positions", s.positions.len()), t))
            .child(row().center_y().gap(8.0).child(search).child(add))
    } else {
        row()
            .center_y()
            .w_full()
            .gap(8.0)
            .child(search.grow(1.0).min_w(0.0))
            .child(add)
    };
    col().w_full().gap(16.0).child(top).child(
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
                    ),
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
                    .children(rows.into_iter().enumerate().map(|(index, position)| {
                        holding_row(s, position, columns, index == 0, index == end, t)
                    })),
            ),
    )
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
            ui::columns(true, 2, 2, 12.0)
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
    let rows = s.allocation.iter().enumerate().map(|(index, item)| {
        let target = review.and_then(|review| {
            review
                .allocations
                .iter()
                .find(|target| target.asset_class == item.asset_class)
        });
        let hovered = s.donut_hover == Some(index);
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
                            .children(target.map(|target| {
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
                            })),
                    )
                    .child(
                        row()
                            .center_y()
                            .gap(12.0)
                            .child(ui::number(s.money(item.value), k.c.text_muted))
                            .child(ui::number(format!("{:.1}%", item.percent), k.c.text_base)),
                    ),
            )
            .child(ui::share_bar(
                format!("share-{index}"),
                item.percent,
                target.map(|target| target.target_percent),
                t.class_color(&item.asset_class),
                t,
            ))
    });
    let rows: Vec<El> = rows.collect();
    ui::columns(ui::is_wide(cx), 2, 1, 16.0)
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
    let wide = ui::is_wide(cx);
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
                        .child(ui::title("Value over time", t))
                        .child(selector),
                )
                .child(
                    ui::columns(true, 4, 4, 12.0)
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
                            ui::caption(format!("History is stored in {path}"), t).truncate_middle()
                        }),
                ),
        )
        .child(
            ui::columns(wide, 4, 2, 12.0)
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
            ui::columns(true, 2, 2, 16.0)
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

    #[test]
    fn names_fall_back_to_the_ticker() {
        assert_eq!(display_name(&position(None, Some("SPY"), None)), "SPY");
        assert_eq!(display_name(&position(None, None, None)), "Unknown");
    }
}
