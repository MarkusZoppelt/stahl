//! Overview, Holdings, Allocation and Performance.

use std::cmp::Ordering;

use portfolio_rs::{
    mcp::dto::Severity,
    state::{PositionDto, TopMoverDto},
};
use rune::{
    Cx, El, PopupItem, PopupMenu, Tx, col, icon, popup_menu_supported, row, text,
    tok::{TextStyle, motion::LEAD},
};
use rune_kit::{
    Button, ButtonVariant, IconButton, IconButtonVariant, RollingText, SegmentedControl, TextInput,
};

use crate::{
    backend::Period,
    format, icons,
    state::{Confirm, Page, SortBy, State},
    theme::{HERO, NUMBER, Theme},
    view::{charts, ui},
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

fn hero(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    let Some(summary) = &s.summary else {
        return ui::card(t);
    };
    let points = s
        .history
        .as_ref()
        .map_or(&[][..], |history| &history.points[..]);
    let day = t.change(summary.day_change_percent);
    ui::card(t)
        .key("hero")
        .padding(0.0)
        .gap(0.0)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_between()
                .items_start()
                .gap(32.0)
                .padding(24.0)
                .pb(8.0)
                .child(
                    col()
                        .gap(4.0)
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
                        .gap(32.0)
                        .child(ui::stat(
                            "Unrealized PnL",
                            s.signed_money(summary.total_pnl),
                            Some(t.change(summary.total_pnl)),
                            t,
                        ))
                        .child(ui::stat(
                            "Securities",
                            s.money(summary.securities_value),
                            None,
                            t,
                        ))
                        .child(ui::stat("Cash", s.money(summary.cash_value), None, t)),
                ),
        )
        .child(col().w_full().px(24.0).pb(16.0).child(charts::value_chart(
            s,
            points,
            t,
            cx,
            "overview-chart",
            210.0,
        )))
}

fn allocation_card(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let shares: Vec<_> = s
        .allocation
        .iter()
        .map(|item| (item.percent, t.class_color(&item.asset_class)))
        .collect();
    ui::card(t)
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
    ui::card(t)
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
                        .px(6.0)
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
    ui::card(t)
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

pub fn overview(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    col().w_full().gap(16.0).child(hero(s, t, cx)).child(
        ui::columns(ui::is_wide(cx), 3, 2, 16.0)
            .child(ui::entering(allocation_card(s, t), 0))
            .child(ui::entering(movers_card(s, t), 1))
            .child(ui::entering(attention_card(s, t), 2)),
    )
}

// --- Holdings ---

fn header_cell(
    s: &State,
    key: &'static str,
    label: &'static str,
    sort: SortBy,
    right: bool,
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
        .grow(1.0)
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

fn holding_row(s: &State, position: &PositionDto, t: &Theme) -> El {
    let k = &t.kit;
    let id = position.id;
    let selected = s.selected == Some(id);
    let cell = |content: El, right: bool| {
        let cell = row().center_y().grow(1.0).w(0.0).min_w(0.0);
        if right {
            cell.justify_end().child(content)
        } else {
            cell.child(content)
        }
    };
    let name = display_name(position);
    let context_name = name.clone();
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
        .py(8.0)
        .radius(6.0)
        .center_y()
        .fill(if selected {
            k.c.layer_03
        } else {
            k.c.layer_03.alpha(0.0)
        })
        .hover(k.c.hover)
        .motion(LEAD)
        .on_press(move |s: &mut State| s.select_position(id))
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
        .child(cell(
            col()
                .min_w(0.0)
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
            false,
        ))
        .child(cell(
            ui::muted(position.asset_class.clone(), t).truncate(),
            false,
        ))
        .child(cell(
            ui::number(s.money(position.value), k.c.text_base),
            true,
        ))
        .child(cell(
            ui::number(
                format::optional_percent(position.pnl_percent),
                t.optional_change(position.pnl_percent),
            ),
            true,
        ))
        .child(cell(
            ui::number(
                format::optional_percent(position.day_change_percent),
                t.optional_change(position.day_change_percent),
            ),
            true,
        ))
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

pub fn holdings(s: &State, t: &Theme) -> El {
    let k = &t.kit;
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
    col()
        .w_full()
        .gap(16.0)
        .child(
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
                        .child(
                            search_input(s)
                                .width(Some(240.0))
                                .on_edit(|s: &mut State| &mut s.search)
                                .build(k)
                                .key("holdings-search"),
                        )
                        .child(
                            Button::new("Add")
                                .variant(ButtonVariant::Submit)
                                .icon(&icons::PLUS)
                                .shortcut(ui::chord(false, "N"))
                                .on_press(|s: &mut State| s.open_form(None))
                                .build(k)
                                .key("add-position"),
                        ),
                ),
        )
        .child(
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
                        .child(header_cell(s, "sort-name", "Name", SortBy::Name, false, t))
                        .child(header_cell(
                            s,
                            "sort-class",
                            "Class",
                            SortBy::Class,
                            false,
                            t,
                        ))
                        .child(header_cell(
                            s,
                            "sort-value",
                            "Value",
                            SortBy::Value,
                            true,
                            t,
                        ))
                        .child(header_cell(s, "sort-pnl", "PnL", SortBy::Pnl, true, t))
                        .child(header_cell(s, "sort-day", "Today", SortBy::Day, true, t)),
                )
                .children(
                    rows.is_empty()
                        .then(|| ui::muted("No holdings match.", t).padding(12.0)),
                )
                .child(
                    col()
                        .key("holdings-rows")
                        .w_full()
                        .children(rows.into_iter().map(|position| holding_row(s, position, t))),
                ),
        )
}

fn detail(label: &'static str, value: String, t: &Theme) -> El {
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
        s.money(position.price)
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
                        .child(
                            text(name)
                                .font(TextStyle::new(16.0, 600.0))
                                .color(k.c.text_base)
                                .wrap(),
                        )
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
                .child(detail(
                    "Average cost",
                    s.optional_money(position.average_cost),
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
                        .map_or_else(|| "-".to_string(), |price| s.money(price));
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

// --- Allocation ---

pub fn allocation(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
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
    let empty = rows.is_empty();
    ui::columns(ui::is_wide(cx), 2, 1, 16.0)
        .child(ui::card(t).center().child(charts::donut(s, t, cx)))
        .child(
            ui::card(t)
                .child(ui::header(
                    "Breakdown",
                    if review.is_some() {
                        "Compared with your policy targets"
                    } else {
                        "Share of total value"
                    },
                    t,
                ))
                .children(empty.then(|| ui::muted("No allocation data.", t)))
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
                        .child(ui::stat(
                            "Time-weighted return",
                            twr.map_or_else(|| "-".to_string(), format::percent),
                            twr.map(|v| t.change(v)),
                            t,
                        ))
                        .child(ui::stat(
                            "Net contributions",
                            optional(contributions),
                            None,
                            t,
                        ))
                        .child(ui::stat(
                            "Market effect",
                            optional(market),
                            market.map(|v| t.change(v)),
                            t,
                        )),
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
                .child(ui::stat_card(
                    "Invested",
                    s.money(performance.invested),
                    None,
                    t,
                ))
                .child(ui::stat_card(
                    "Unrealized PnL",
                    s.signed_money(performance.unrealized_pnl),
                    Some(t.change(performance.unrealized_pnl)),
                    t,
                ))
                .child(ui::stat_card(
                    "Total return",
                    format::percent(performance.pnl_percent),
                    Some(t.change(performance.pnl_percent)),
                    t,
                ))
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
