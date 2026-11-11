//! The rebalance page: how far to go, and what that does to each asset class.
//!
//! One slider says how far to go from the portfolio as it is to the policy targets. Under it,
//! the plan: a row per class, each on the same scale, with where the class is today, where it
//! would be, its target and its tolerance band. Every control can be dragged or stepped.
//!
//! The slider's thumb, each row's thumb and the plot's slices live in `Live` values
//! (`State::rebalance`), so a pointer move redraws them without running the view. The view runs
//! again only when a figure would change (each half point of the plan), and then the amounts
//! roll to their new digits.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use rune::{Color, Cursor, Cx, Drag, El, Key, KeyEvent, Phase, Spring, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, RollingText, TextInput};

use crate::{
    format,
    rebalance::{self, Class, Plan},
    state::{BadNumber, RebalancePlan, State, parse_number},
    theme::{LEDE, LEDE_COMPACT, NUMBER, Theme},
    view::ui,
};

/// The slider's lane. A finger needs 44 points of it, a mouse fewer.
const RAIL_H: f32 = 32.0;
const RAIL_H_COMPACT: f32 = 44.0;
/// The slider's track and thumb.
const RAIL_BAR: f32 = 8.0;
const RAIL_THUMB: f32 = 22.0;
/// A row's track: the lane, the bar on it, its tolerance band, and the marks.
const TRACK_H: f32 = 32.0;
const TRACK_H_COMPACT: f32 = 44.0;
const TRACK_BAR: f32 = 8.0;
const BAND_H: f32 = 16.0;
const TODAY: f32 = 11.0;
const THUMB: f32 = 13.0;
/// A thumb's hit area. The same for the eye and for a finger.
const HIT: f32 = 44.0;
/// The table's columns: the name, the trade and the share after. The plot uses the same.
const NAME_W: f32 = 120.0;
const TRADE_W: f32 = 170.0;
const SHARE_W: f32 = 56.0;
/// The plot: today's allocation, thinner, above what the plan does.
const GHOST_H: f32 = 6.0;
const PLOT_H: f32 = 18.0;
const SLICE_GAP: f32 = 2.0;
const GHOST_GAP: f32 = 1.0;
/// How close to the halfway mark a drag sticks to it, as a share of the slider.
const DETENT: f64 = 0.012;
/// The finest change a drag commits the figures at: one point of the plan, or half a point of
/// a share.
const GRAIN: f64 = 0.01;
const SHARE_GRAIN: f64 = 0.5;
/// How near its target tick a thumb lands on it, in points of share.
const SNAP: f64 = 1.0;

/// Under a finger or pointer: quick, with a little give.
const FOLLOW: Spring = Spring::new(0.16, 0.18);
/// A step, a reset or a change from elsewhere: the whole control travels together.
const GLIDE: Spring = Spring::new(0.5, 0.08);

/// The page: the question, the plan, and what it costs.
pub fn instrument(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let classes = load_classes(s);
    if classes.is_empty() {
        return ui::caption("Add an asset class to plan a rebalance.", t);
    }
    let plan = shown(s, &classes);
    let narrow = ui::room(s, cx) == ui::Room::Narrow;
    ui::card(t)
        .key("rebalance-card")
        .w_full()
        .gap(12.0)
        .child(sentence(s, t, &classes, &plan))
        .child(rail(s, t, narrow))
        .child(rail_labels(t))
        .child(money_row(s, t))
        .child(ui::rule(t))
        .child(plot(s, t, &classes, narrow))
        .child(key(t))
        .child(rows(s, t, &classes, &plan, narrow))
        .child(closing(s, t, &plan))
}

fn load_classes(s: &State) -> Vec<Class> {
    let mut out = Vec::new();
    if let Some(review) = s.review.data.as_ref() {
        for item in &review.allocations {
            let value = s
                .allocation
                .iter()
                .find(|current| current.asset_class.eq_ignore_ascii_case(&item.asset_class))
                .map_or(item.value, |current| current.value);
            out.push(class(
                &item.asset_class,
                value,
                Some(item.target_percent),
                item.tolerance_percent,
            ));
        }
    }
    for item in &s.allocation {
        if named(&out, &item.asset_class) {
            continue;
        }
        let target = s.simulation.data.as_ref().and_then(|simulation| {
            simulation.scenarios.first().and_then(|scenario| {
                scenario
                    .new_allocation
                    .iter()
                    .find(|share| share.asset_class.eq_ignore_ascii_case(&item.asset_class))
                    .map(|share| share.percent)
            })
        });
        out.push(class(&item.asset_class, item.value, target, None));
    }
    // Cash pays for the rest, so it reads last, whatever order the policy lists it in.
    out.sort_by_key(|class| class.cash);
    out
}

fn class(name: &str, value: f64, target: Option<f64>, tolerance: Option<f64>) -> Class {
    let mut class = Class::new(name, value);
    if let Some(target) = target {
        class = class.target(target);
    }
    if let Some(tolerance) = tolerance {
        class = class.tolerance(tolerance);
    }
    if name.eq_ignore_ascii_case("cash") {
        class = class.cash();
    }
    class
}

fn named(classes: &[Class], name: &str) -> bool {
    classes
        .iter()
        .any(|class| class.name.eq_ignore_ascii_case(name))
}

/// The plan the page is showing, whatever produced it.
fn shown(s: &State, classes: &[Class]) -> Plan {
    match &s.rebalance.plan {
        RebalancePlan::Flow(amount) => {
            rebalance::place(classes, *amount).unwrap_or_else(|_| rebalance::today(classes))
        }
        RebalancePlan::Adjusted { names, values } if matched(classes, names) => {
            rebalance::from_values(classes, values.clone(), 0.0)
        }
        _ => rebalance::blend(classes, s.rebalance.fraction),
    }
}

fn matched(classes: &[Class], names: &[String]) -> bool {
    names.len() == classes.len()
        && names
            .iter()
            .zip(classes)
            .all(|(name, class)| name == &class.name)
}

/// Adjusted by hand, and still about these classes.
fn adjusted(s: &State, classes: &[Class]) -> bool {
    matches!(&s.rebalance.plan, RebalancePlan::Adjusted { names, .. } if matched(classes, names))
}

fn share_of(plan: &Plan, index: usize) -> f64 {
    if plan.total <= 0.0 {
        return 0.0;
    }
    plan.values.get(index).copied().unwrap_or(0.0) / plan.total * 100.0
}

fn total_of(plan: &Plan) -> f64 {
    plan.total.max(1e-9)
}

fn bought(plan: &Plan) -> f64 {
    plan.trades
        .iter()
        .filter(|trade| trade.action == rebalance::Action::Increase)
        .map(|trade| trade.amount)
        .sum()
}

fn sold(plan: &Plan) -> f64 {
    plan.trades
        .iter()
        .filter(|trade| trade.action == rebalance::Action::Reduce)
        .map(|trade| trade.amount)
        .sum()
}

/// The largest share a row's track shows. One scale for every row, and wide enough for what a
/// row could be dragged to without running cash out, so the scale never moves under a drag.
fn top_share(classes: &[Class]) -> f64 {
    let total: f64 = classes.iter().map(|class| class.value.max(0.0)).sum();
    let cash: f64 = classes
        .iter()
        .filter(|class| class.cash)
        .map(|class| class.value.max(0.0))
        .sum();
    let mut top: f64 = 20.0;
    for class in classes {
        let held = if total > 0.0 {
            class.value.max(0.0) / total * 100.0
        } else {
            0.0
        };
        top = top.max(held);
        if let Some(target) = class.target {
            let band = class.tolerance.unwrap_or(rebalance::DEFAULT_TOLERANCE);
            top = top.max(target + band);
        }
        if !class.cash {
            top = top.max(
                held + if total > 0.0 {
                    cash / total * 100.0
                } else {
                    0.0
                },
            );
        }
    }
    (top / 10.0).ceil() * 10.0
}

// --- The question ---

fn sentence(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let (line, reset) = sentence_text(s, plan, adjusted(s, classes));
    row()
        .center_y()
        .w_full()
        .gap(16.0)
        .child(
            text(line)
                .font(if s.compact { LEDE_COMPACT } else { LEDE })
                .color(t.kit.c.text_base)
                .wrap()
                .grow(1.0)
                .shrink(),
        )
        .children(reset.then(|| {
            Button::new("Reset")
                .variant(ButtonVariant::Ghost)
                .help("Back to the slider's blend")
                .on_press(|s: &mut State| {
                    s.rebalance.plan = RebalancePlan::Blend;
                    s.rebalance.step = i64::MIN;
                })
                .build(&t.kit)
                .key("rebalance-reset")
        }))
}

/// The one sentence, in the serif. It says what the plan is and what it costs.
fn sentence_text(s: &State, plan: &Plan, adjusted: bool) -> (String, bool) {
    let (buy, sell) = (bought(plan), sold(plan));
    let trade = match (buy < 0.005, sell < 0.005) {
        (true, true) => "nothing to trade".to_string(),
        (false, true) => format!("buy {}", s.money(buy)),
        (true, false) => format!("sell {}", s.money(sell)),
        (false, false) => format!("buy {}, sell {}", s.money(buy), s.money(sell)),
    };
    match &s.rebalance.plan {
        RebalancePlan::Flow(amount) if *amount >= 0.0 => (
            format!("Invest {} by buying only.", s.money(*amount)),
            false,
        ),
        RebalancePlan::Flow(amount) => (
            format!("Withdraw {} by selling only.", s.money(-*amount)),
            false,
        ),
        _ if adjusted => (format!("Adjusted by hand: {trade}."), true),
        _ => {
            let fraction = s.rebalance.fraction.clamp(0.0, 1.0);
            if fraction < 1e-3 {
                (
                    "Nothing changes yet. Drag to see what it takes.".to_string(),
                    false,
                )
            } else if fraction > 1.0 - 1e-3 {
                (format!("On target: {trade}."), false)
            } else {
                (
                    format!("Go {}% of the way: {trade}.", (fraction * 100.0).round()),
                    false,
                )
            }
        }
    }
}

fn rail(s: &State, t: &Theme, narrow: bool) -> El {
    let height = if narrow { RAIL_H_COMPACT } else { RAIL_H };
    let bar_y = (height - RAIL_BAR) / 2.0;
    let enabled = matches!(
        s.rebalance.plan,
        RebalancePlan::Blend | RebalancePlan::Adjusted { .. }
    );
    let mut rail = el()
        .key("rebalance-rail")
        .w_full()
        .h(height)
        .cursor(if enabled {
            Cursor::Pointer
        } else {
            Cursor::Default
        })
        .role("slider")
        .label("How far to rebalance")
        .description(
            "Left is the portfolio as it is, right is every target met. Return goes to the \
             nearest one.",
        )
        .a11y_value(format!(
            "{:.0} percent of the way",
            s.rebalance.fraction * 100.0
        ))
        .range_value(s.rebalance.fraction * 100.0, 0.0, 100.0)
        .range_step(1.0)
        .focus_ring(2.0, -6.0, t.info)
        .on_layout_tx(|s: &mut State, rect: [f32; 4], tx: &mut Tx| {
            let (width, tall) = (rect[2], rect[3]);
            if s.rebalance.placed
                && (s.rebalance.rail_width - width).abs() < 0.5
                && (s.rebalance.rail_height - tall).abs() < 0.5
            {
                return;
            }
            s.rebalance.rail_width = width;
            s.rebalance.rail_height = tall;
            if !s.rebalance.dragging {
                let classes = load_classes(s);
                let plan = shown(s, &classes);
                paint(s, &classes, &plan.values, false);
            }
            if s.rebalance.placed {
                tx.live_only();
            } else {
                s.rebalance.placed = true;
            }
        })
        .child(
            el().absolute(0.0, bar_y)
                .w_full()
                .h(RAIL_BAR)
                .radius(RAIL_BAR / 2.0)
                .fill(t.kit.c.layer_03),
        )
        .child(
            el().absolute(0.0, 0.0)
                .radius(RAIL_BAR / 2.0)
                .fill(t.accent)
                .live_rect(s.rebalance.rail_fill.smooth(spring(s))),
        )
        .child(detent(t, height))
        .child(
            el().absolute(0.0, 0.0)
                .size(RAIL_THUMB, RAIL_THUMB)
                .radius(RAIL_THUMB / 2.0)
                .fill(t.kit.c.text_base)
                .border(2.0, t.card)
                .live_rect(s.rebalance.rail_thumb.smooth(spring(s))),
        );
    if enabled {
        rail = rail
            .on_drag(|s: &mut State, drag: &mut Drag, tx: &mut Tx| rail_drag(s, drag, tx))
            .on_step(|s: &mut State, by, _| rail_step(s, by))
            .on_step_limit(|s: &mut State, max, _| rail_limit(s, max))
            .on_key(|s: &mut State, event: &KeyEvent, _| {
                if event.key == Key::Enter {
                    rail_snap(s);
                    true
                } else {
                    false
                }
            });
    } else {
        rail = rail.disabled(true).opacity(0.4);
    }
    rail
}

/// The mark at 50% the slider sticks to.
fn detent(t: &Theme, height: f32) -> El {
    row()
        .center_y()
        .absolute(0.0, 0.0)
        .w_full()
        .h(height)
        .inert()
        .a11y_hidden()
        .child(el().w_frac(0.5))
        .child(
            el().w(2.0)
                .h(12.0)
                .radius(1.0)
                .offset(-1.0, 0.0)
                .fill(t.kit.c.text_muted.alpha(0.7)),
        )
}

/// As it is, Halfway, On target: the slider's three stops, at the ends and the middle.
fn rail_labels(t: &Theme) -> El {
    let label = |line: &str, align: rune::Align| {
        text(line.to_string())
            .font(t.kit.ty.text_xsmall)
            .color(t.kit.c.text_muted)
            .align(align)
    };
    row()
        .center_y()
        .w_full()
        .equal_columns(3)
        .child(label("As it is", rune::Align::Start))
        .child(label("Halfway", rune::Align::Center))
        .child(label("On target", rune::Align::End))
}

fn money_row(s: &State, t: &Theme) -> El {
    let input = TextInput::new(&s.rebalance.amount)
        .placeholder("2000")
        .width(Some(160.0))
        .label("I have this much to invest")
        .on_edit(|s: &mut State| &mut s.rebalance.amount)
        .on_change(|s: &mut State, _| apply_amount(s))
        .build(&t.kit)
        .key("rebalance-amount");
    let typed = !s.rebalance.amount.text.trim().is_empty();
    col()
        .w_full()
        .gap(6.0)
        .child(
            row()
                .center_y()
                .w_full()
                .flex_wrap()
                .gaps(8.0, 12.0)
                .child(ui::caption("I have this much to invest", t))
                .child(input)
                .children(typed.then(|| {
                    Button::new("Clear")
                        .variant(ButtonVariant::Ghost)
                        .help("Back to the slider")
                        .on_press(|s: &mut State| clear_amount(s))
                        .build(&t.kit)
                        .key("rebalance-clear")
                }))
                .child(ui::caption("A negative amount is a withdrawal.", t)),
        )
        .children(
            s.rebalance
                .amount_error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t).key("rebalance-amount-error")),
        )
}

// --- The plot ---

/// Today's allocation, thin, over the plan's. The same classes in the same order, on the
/// same columns as the rows, so the two bars line up with every track.
fn plot(s: &State, t: &Theme, classes: &[Class], narrow: bool) -> El {
    // In a row the bar takes the space between the columns; on a phone it has the width of
    // the card, so it lines up with the tracks under it.
    let sized = |bar: El| {
        if narrow {
            bar.w_full()
        } else {
            bar.grow(1.0).w(0.0)
        }
    };
    let ghost = sized(
        el().h(GHOST_H)
            .radius(GHOST_H / 2.0)
            .clip_rounded(GHOST_H / 2.0)
            .children(classes.iter().enumerate().map(|(index, class)| {
                el().absolute(0.0, 0.0)
                    .radius(1.0)
                    .fill(t.class_color(&class.name).alpha(0.4))
                    .live_rect(&s.rebalance.ghosts[index])
            })),
    );
    let after = sized(
        el().key("rebalance-plot")
            .h(PLOT_H)
            .radius(3.0)
            .clip_rounded(3.0)
            .on_layout_tx(|s: &mut State, rect: [f32; 4], _: &mut Tx| {
                if (s.rebalance.bar_width - rect[2]).abs() < 0.5 {
                    return;
                }
                s.rebalance.bar_width = rect[2];
            })
            .children(classes.iter().enumerate().map(|(index, class)| {
                el().absolute(0.0, 0.0)
                    .radius(2.0)
                    .fill(t.class_color(&class.name))
                    .live_rect(s.rebalance.bars[index].smooth(spring(s)))
            })),
    );
    let bar_row = |label: &str, bar: El| {
        if narrow {
            col()
                .w_full()
                .gap(2.0)
                .child(bar_label(label, t))
                .child(bar)
        } else {
            row()
                .center_y()
                .w_full()
                .gap(16.0)
                .child(bar_label(label, t).w(NAME_W))
                .child(bar)
                // The trade and share columns the rows have, so the bars end where the
                // tracks do.
                .child(el().w(TRADE_W))
                .child(el().w(SHARE_W))
        }
    };
    col()
        .w_full()
        .gap(if narrow { 6.0 } else { 4.0 })
        .child(bar_row("Today", ghost))
        .child(bar_row("After", after))
}

fn bar_label(line: &str, t: &Theme) -> El {
    text(line.to_string())
        .font(t.kit.ty.text_xsmall)
        .color(t.kit.c.text_muted)
}

/// What the marks on a track mean, once, and who pays for a change.
fn key(t: &Theme) -> El {
    let item = |mark: El, line: &str| {
        row()
            .center_y()
            .gap(5.0)
            .child(mark)
            .child(ui::caption(line, t))
    };
    let hollow = el()
        .size(TODAY, TODAY)
        .radius(TODAY / 2.0)
        .border(1.5, t.kit.c.text_muted);
    let filled = el().size(TODAY, TODAY).radius(TODAY / 2.0).fill(t.up);
    let tick = el().w(2.0).h(11.0).radius(1.0).fill(tick_color(t));
    let band = el()
        .size(16.0, 9.0)
        .radius(3.0)
        .fill(t.kit.c.text_muted.alpha(0.22));
    row()
        .center_y()
        .w_full()
        .flex_wrap()
        .gaps(8.0, 16.0)
        .child(item(hollow, "today"))
        .child(item(filled, "after"))
        .child(item(tick, "target"))
        .child(item(band, "tolerance"))
        .child(el().grow(1.0))
        .child(ui::caption("Cash takes up the difference.", t))
}

// --- The rows ---

fn rows(s: &State, t: &Theme, classes: &[Class], plan: &Plan, narrow: bool) -> El {
    col().w_full().children(
        classes
            .iter()
            .enumerate()
            .flat_map(|(index, _)| [ui::rule(t), asset_row(s, t, classes, plan, index, narrow)]),
    )
}

#[allow(clippy::too_many_arguments)]
fn asset_row(
    s: &State,
    t: &Theme,
    classes: &[Class],
    plan: &Plan,
    index: usize,
    narrow: bool,
) -> El {
    let class = &classes[index];
    let value = plan.values.get(index).copied().unwrap_or(0.0);
    let share = share_of(plan, index);
    let named = row()
        .center_y()
        .gap(8.0)
        .child(ui::dot(t.class_color(&class.name)))
        .child(
            text(class.name.clone())
                .font(t.kit.ty.ui_medium)
                .color(t.kit.c.text_base),
        );
    let after = row()
        .w(if narrow { 52.0 } else { SHARE_W })
        .justify_end()
        .child(
            RollingText::new(format!("rebalance-share-{index}"), format!("{share:.1}%"))
                .value(share)
                .font(NUMBER)
                .color(t.kit.c.text_muted)
                .trailing()
                .build(&t.kit),
        );
    let track = track(s, t, classes, plan, index, narrow);
    let trade = trade(s, t, class, value, index, narrow);
    if narrow {
        col()
            .key(format!("rebalance-row-{index}"))
            .w_full()
            .gap(6.0)
            .py(10.0)
            .child(
                row()
                    .center_y()
                    .w_full()
                    .gap(8.0)
                    .child(named)
                    .child(after)
                    .child(el().grow(1.0))
                    .child(trade),
            )
            .child(track)
    } else {
        row()
            .key(format!("rebalance-row-{index}"))
            .center_y()
            .w_full()
            .gap(16.0)
            .py(6.0)
            .child(named.w(NAME_W))
            .child(track.grow(1.0).w(0.0))
            .child(row().w(TRADE_W).justify_end().child(trade))
            .child(after)
    }
}

fn trade(s: &State, t: &Theme, class: &Class, value: f64, index: usize, narrow: bool) -> El {
    let delta = value - class.value.max(0.0);
    let (line, ink) = if delta.abs() < 0.005 {
        ("No change".to_string(), t.kit.c.text_muted)
    } else if delta > 0.0 {
        (format!("Buy {}", s.money(delta)), t.up)
    } else {
        (format!("Sell {}", s.money(-delta)), t.down)
    };
    let font = if narrow {
        t.kit.ty.text_small
    } else {
        t.kit.ty.ui
    };
    if s.privacy {
        return text(line).font(font).color(ink);
    }
    RollingText::new(format!("rebalance-trade-{index}"), line)
        .value(delta.abs())
        .font(font)
        .color(ink)
        .trailing()
        .build(&t.kit)
}

fn tick_color(t: &Theme) -> Color {
    t.kit.c.text_base.alpha(0.55)
}

fn track(s: &State, t: &Theme, classes: &[Class], plan: &Plan, index: usize, narrow: bool) -> El {
    let class = &classes[index];
    let height = if narrow { TRACK_H_COMPACT } else { TRACK_H };
    let bar_y = (height - TRACK_BAR) / 2.0;
    let top = top_share(classes).max(1e-6);
    let share = share_of(plan, index);
    let held = class.value.max(0.0) / total_of(plan) * 100.0;
    let fraction = |percent: f64| (percent / top).clamp(0.0, 1.0) as f32;
    let mut track = el()
        .key(format!("rebalance-track-{index}"))
        .w_full()
        .h(height)
        .on_layout_tx(|s: &mut State, rect: [f32; 4], _: &mut Tx| {
            if (s.rebalance.track_width - rect[2]).abs() < 0.5 {
                return;
            }
            s.rebalance.track_width = rect[2];
        })
        .child(
            el().absolute(0.0, bar_y)
                .w_full()
                .h(TRACK_BAR)
                .radius(TRACK_BAR / 2.0)
                .fill(t.kit.c.layer_03),
        );
    if let Some(target) = class.target {
        let band = class.tolerance.unwrap_or(rebalance::DEFAULT_TOLERANCE);
        let from = fraction(target - band);
        let to = fraction(target + band);
        if to > from {
            track = track.child(
                row()
                    .center_y()
                    .absolute(0.0, 0.0)
                    .w_full()
                    .h(height)
                    .inert()
                    .a11y_hidden()
                    .child(el().w_frac(from))
                    .child(
                        el().w_frac(to - from)
                            .h(BAND_H)
                            .radius(BAND_H / 2.0)
                            .fill(t.class_color(&class.name).alpha(0.16)),
                    ),
            );
        }
        track = track.child(
            row()
                .center_y()
                .absolute(0.0, 0.0)
                .w_full()
                .h(height)
                .inert()
                .a11y_hidden()
                .child(el().w_frac(fraction(target)))
                .child(
                    el().w(2.0)
                        .h(12.0)
                        .radius(1.0)
                        .offset(-1.0, 0.0)
                        .fill(tick_color(t)),
                ),
        );
    }
    // Where the class is today: hollow, so the thumb that is filled reads as where it would be.
    track = track.child(
        row()
            .center_y()
            .absolute(0.0, 0.0)
            .w_full()
            .h(height)
            .inert()
            .a11y_hidden()
            .child(el().w_frac(fraction(held)))
            .child(
                el().size(TODAY, TODAY)
                    .radius(TODAY / 2.0)
                    .border(1.5, t.kit.c.text_muted)
                    .offset(-TODAY / 2.0, 0.0),
            ),
    );
    let colour = match rebalance::inside_band(class, plan.values[index], plan.total) {
        Some(true) => t.up,
        Some(false) => t.warn,
        None => t.kit.c.text_muted,
    };
    track.child(thumb(s, t, class, index, height, share, top, colour))
}

#[allow(clippy::too_many_arguments)]
fn thumb(
    s: &State,
    t: &Theme,
    class: &Class,
    index: usize,
    height: f32,
    share: f64,
    top: f64,
    colour: Color,
) -> El {
    let enabled = matches!(
        s.rebalance.plan,
        RebalancePlan::Blend | RebalancePlan::Adjusted { .. }
    );
    let mut thumb = el()
        .key(format!("rebalance-thumb-{index}"))
        .absolute(-HIT / 2.0, (height - HIT) / 2.0)
        .size(HIT, HIT)
        .center()
        .radius(HIT / 2.0)
        .cursor(if enabled {
            Cursor::ResizeColumn
        } else {
            Cursor::Default
        })
        .role("slider")
        .label(format!("{} after rebalancing", class.name))
        .description(match class.target {
            Some(_) => "Arrow keys move it, Return puts it on its target.",
            None => "Arrow keys move it.",
        })
        .a11y_value(format!("{share:.1} percent"))
        .range_value(share, 0.0, top)
        .range_step(1.0)
        .focus_ring(2.0, -15.0, t.info)
        .live_offset(s.rebalance.thumbs[index].smooth(spring(s)))
        .child(
            el().size(THUMB + 6.0, THUMB + 6.0).center().child(
                el().size(THUMB, THUMB)
                    .radius(THUMB / 2.0)
                    .fill(colour)
                    .border(2.0, t.card),
            ),
        );
    if enabled {
        thumb = thumb
            .on_drag(move |s: &mut State, drag: &mut Drag, tx: &mut Tx| {
                pointer(s, drag, tx, index, true)
            })
            .on_step(move |s: &mut State, by, _| step(s, index, by))
            .on_step_limit(move |s: &mut State, max, _| limit(s, index, max))
            .on_key(move |s: &mut State, event: &KeyEvent, _| {
                if event.key == Key::Enter {
                    snap(s, index);
                    true
                } else {
                    false
                }
            });
    } else {
        thumb = thumb.disabled(true);
    }
    thumb
}

// --- The closing lines ---

fn closing(s: &State, t: &Theme, plan: &Plan) -> El {
    let (buy, sell) = (bought(plan), sold(plan));
    let summary = if plan.trades.is_empty() {
        format!("No trades. The total is {}.", s.money(plan.total))
    } else if plan.external.abs() < 0.01 {
        format!(
            "Buys {} and sells {}. The total stays {}.",
            s.money(buy),
            s.money(sell),
            s.money(plan.total)
        )
    } else {
        format!(
            "{} Afterwards {}.",
            match (buy < 0.005, sell < 0.005) {
                (true, true) => "No trades.".to_string(),
                (false, true) => format!("Buys {}.", s.money(buy)),
                (true, false) => format!("Sells {}.", s.money(sell)),
                (false, false) => format!("Buys {} and sells {}.", s.money(buy), s.money(sell)),
            },
            s.money(plan.total)
        )
    };
    col()
        .w_full()
        .gap(4.0)
        .child(
            text(summary)
                .font(t.kit.ty.ui)
                .color(t.kit.c.text_base)
                .wrap(),
        )
        .child(ui::caption(
            "Stahl never trades. This is a plan, not an order.",
            t,
        ))
}

// --- New money ---

/// Read the amount field and, if it is a number, place it. Anything ambiguous stays unparsed.
fn apply_amount(s: &mut State) {
    let text = s.rebalance.amount.text.clone();
    // A sign or a decimal mark on its own is the start of a number, not a mistake.
    if matches!(text.trim(), "-" | "+" | "." | ",") {
        s.rebalance.amount_error = None;
        return;
    }
    match parse_number(&text) {
        Ok(None) => {
            s.rebalance.amount_error = None;
            s.rebalance.plan = RebalancePlan::Blend;
        }
        Ok(Some(amount)) => match rebalance::place(&load_classes(s), amount) {
            Ok(_) => {
                s.rebalance.amount_error = None;
                s.rebalance.plan = RebalancePlan::Flow(amount);
            }
            Err(error) => s.rebalance.amount_error = Some(error.message().to_string()),
        },
        Err(error) => s.rebalance.amount_error = Some(amount_error(&text, error)),
    }
}

fn clear_amount(s: &mut State) {
    s.rebalance.amount.replace_text("");
    s.rebalance.amount_error = None;
    s.rebalance.plan = RebalancePlan::Blend;
}

fn amount_error(text: &str, error: BadNumber) -> String {
    let text = text.trim();
    match error {
        BadNumber::NotANumber => format!(
            "\"{text}\" isn't a number. Use digits, like 2000 or 2000.50, without a currency symbol."
        ),
        BadNumber::Ambiguous { thousands, decimal } => format!(
            "\"{text}\" could mean {thousands} or {decimal}. Write {thousands}, or {decimal}0 for the smaller number."
        ),
    }
}

// --- Markdown ---

/// The plan on screen, as Markdown, whatever produced it.
pub fn markdown(s: &State) -> String {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    let (heading, detail) = plan_copy(s, &plan, adjusted(s, &classes));
    let policy = s
        .simulation
        .data
        .as_ref()
        .map(|simulation| simulation.policy_name.clone())
        .unwrap_or_default();
    rebalance::to_markdown(
        &plan,
        &classes,
        heading,
        &detail,
        &policy,
        &s.currency(),
        format::amount,
    )
}

fn plan_copy(s: &State, plan: &Plan, adjusted: bool) -> (&'static str, String) {
    match &s.rebalance.plan {
        RebalancePlan::Flow(amount) if *amount >= 0.0 => (
            "Where new money goes",
            "Buying only, selling nothing, as close to the targets as that gets.".to_string(),
        ),
        RebalancePlan::Flow(_) => (
            "Where a withdrawal comes from",
            "Selling only, buying nothing, as close to the targets as that gets.".to_string(),
        ),
        _ if adjusted => (
            "Adjusted by hand",
            "The split as it was moved, with cash taking up the difference.".to_string(),
        ),
        _ => {
            let fraction = s.rebalance.fraction.clamp(0.0, 1.0);
            if fraction < 1e-3 {
                (
                    "As it is",
                    "No trades yet. Every class stays where it is.".to_string(),
                )
            } else if fraction > 1.0 - 1e-3 {
                (
                    "On target",
                    "Every asset class at its policy target.".to_string(),
                )
            } else {
                (
                    "Part of the way to target",
                    format!(
                        "Every asset class {}% of the way toward its policy target, {} to move.",
                        (fraction * 100.0).round(),
                        plan.total - plan.external
                    ),
                )
            }
        }
    }
}

// --- Input ---

fn spring(s: &State) -> Spring {
    if s.rebalance.dragging { FOLLOW } else { GLIDE }
}

fn rail_drag(s: &mut State, drag: &mut Drag, tx: &mut Tx) {
    match drag.phase {
        Phase::Down => {
            begin(s);
            s.rebalance.row = None;
            rail_to(s, f64::from(drag.x), true);
        }
        Phase::Move => {
            if !rail_to(s, f64::from(drag.x), false) {
                tx.live_only();
            }
        }
        Phase::Up => {
            rail_to(s, f64::from(drag.x), true);
            s.rebalance.dragging = false;
        }
    }
}

fn begin(s: &mut State) {
    s.rebalance.dragging = true;
    s.rebalance.step = i64::MIN;
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    s.rebalance.press_values = plan.values;
}

/// Move the slider to the pointer's x, snapping to the stops nearby. False when the figures
/// did not change, so the caller can leave the view alone.
fn rail_to(s: &mut State, x: f64, commit: bool) -> bool {
    let width = f64::from(s.rebalance.rail_width.max(1.0));
    let mut fraction = (x / width).clamp(0.0, 1.0);
    // The ends and the middle are stops of their own, so they land exactly.
    if fraction < DETENT {
        fraction = 0.0;
    } else if fraction > 1.0 - DETENT {
        fraction = 1.0;
    } else if (fraction - 0.5).abs() < DETENT {
        fraction = 0.5;
    }
    fraction_to(s, fraction, commit)
}

/// Set the slider, paint the controls, and say whether the figures must run again.
fn fraction_to(s: &mut State, fraction: f64, commit: bool) -> bool {
    s.rebalance.fraction = fraction.clamp(0.0, 1.0);
    s.rebalance.plan = RebalancePlan::Blend;
    let classes = load_classes(s);
    let plan = rebalance::blend(&classes, s.rebalance.fraction);
    paint(s, &classes, &plan.values, true);
    let grain = (s.rebalance.fraction / GRAIN).round() as i64;
    if commit || grain != s.rebalance.step {
        s.rebalance.step = grain;
        true
    } else {
        false
    }
}

/// Arrows move a point, Page Up and Page Down five, as the slider's keyboard.
fn rail_step(s: &mut State, by: i32) {
    if by == 0 {
        return;
    }
    let points = if by.abs() >= 10 { 5.0 } else { 1.0 };
    let fraction = s.rebalance.fraction + by.signum() as f64 * points / 100.0;
    fraction_to(s, fraction, true);
}

fn rail_limit(s: &mut State, max: bool) {
    fraction_to(s, if max { 1.0 } else { 0.0 }, true);
}

/// Return goes to the nearest of the three stops.
fn rail_snap(s: &mut State) {
    let nearest = [0.0, 0.5, 1.0]
        .into_iter()
        .min_by(|a, b| {
            (s.rebalance.fraction - a)
                .abs()
                .partial_cmp(&(s.rebalance.fraction - b).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(0.0);
    fraction_to(s, nearest, true);
}

/// A row's thumb (`by_pointer`: the press was on the thumb, so follow the travel) or its track
/// (the press was on the track, so go to where it landed).
fn pointer(s: &mut State, drag: &mut Drag, tx: &mut Tx, index: usize, by_pointer: bool) {
    match drag.phase {
        Phase::Down => {
            begin(s);
            s.rebalance.row = Some(index);
            let classes = load_classes(s);
            let plan = shown(s, &classes);
            s.rebalance.press_at = share_of(&plan, index);
            s.rebalance.press_values = plan.values;
            if !by_pointer {
                let top = top_share(&classes);
                let width = f64::from(s.rebalance.track_width.max(1.0));
                let want = f64::from(drag.x) / width * top;
                to_share(s, index, want, true);
            }
        }
        Phase::Move => {
            if s.rebalance.row != Some(index) {
                return;
            }
            let classes = load_classes(s);
            let top = top_share(&classes);
            let want = if by_pointer {
                let width = f64::from(s.rebalance.track_width.max(1.0));
                s.rebalance.press_at + f64::from(drag.dx) / width * top
            } else {
                let width = f64::from(s.rebalance.track_width.max(1.0));
                f64::from(drag.x) / width * top
            };
            if !to_share(s, index, want, false) {
                tx.live_only();
            }
        }
        Phase::Up => {
            if s.rebalance.row == Some(index) {
                let classes = load_classes(s);
                let top = top_share(&classes);
                let want = if by_pointer {
                    let width = f64::from(s.rebalance.track_width.max(1.0));
                    s.rebalance.press_at + f64::from(drag.dx) / width * top
                } else {
                    let width = f64::from(s.rebalance.track_width.max(1.0));
                    f64::from(drag.x) / width * top
                };
                to_share(s, index, want, true);
            }
            s.rebalance.dragging = false;
            s.rebalance.row = None;
        }
    }
}

/// Put `index` at `want` percent of the portfolio, snapping onto its target tick nearby. The
/// figures commit only every half point, so a drag doesn't rebuild the page on every move.
fn to_share(s: &mut State, index: usize, want: f64, commit: bool) -> bool {
    let classes = load_classes(s);
    let values = s.rebalance.press_values.clone();
    if index >= classes.len() || values.len() != classes.len() {
        return true;
    }
    let total: f64 = values.iter().sum();
    let top = top_share(&classes);
    let mut want = want.clamp(0.0, top);
    if classes[index].target.is_some() {
        let ticks = rebalance::at_targets(&classes);
        if let Some(tick) = ticks.get(index) {
            let tick = tick / total.max(1e-9) * 100.0;
            if (want - tick).abs() < SNAP {
                want = tick;
            }
        }
    }
    let moved = rebalance::set_class(&classes, &values, index, want / 100.0 * total);
    paint(s, &classes, &moved, true);
    let grain = (want / SHARE_GRAIN).round() as i64;
    if commit || grain != s.rebalance.step {
        s.rebalance.step = grain;
        s.rebalance.plan = RebalancePlan::Adjusted {
            names: classes.iter().map(|class| class.name.clone()).collect(),
            values: moved,
        };
        s.rebalance.amount.replace_text("");
        s.rebalance.amount_error = None;
        true
    } else {
        false
    }
}

fn step(s: &mut State, index: usize, by: i32) {
    if by == 0 {
        return;
    }
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if index >= classes.len() {
        return;
    }
    let points = if by.abs() >= 10 { 5.0 } else { 1.0 };
    let want = share_of(&plan, index) + by.signum() as f64 * points;
    move_row(s, &classes, &plan, index, want);
}

fn limit(s: &mut State, index: usize, max: bool) {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if index >= classes.len() {
        return;
    }
    let top = top_share(&classes);
    let want = if max {
        // As far as cash allows, and no further than the track shows.
        let reach = rebalance::set_class(&classes, &plan.values, index, f64::MAX);
        (reach.get(index).copied().unwrap_or(0.0) / total_of(&plan) * 100.0).min(top)
    } else {
        0.0
    };
    move_row(s, &classes, &plan, index, want);
}

/// Return puts a class on its target tick.
fn snap(s: &mut State, index: usize) {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if classes.get(index).and_then(|class| class.target).is_none() {
        return;
    }
    let want = rebalance::at_targets(&classes)
        .get(index)
        .map(|value| value / total_of(&plan) * 100.0)
        .unwrap_or(0.0)
        .min(top_share(&classes));
    move_row(s, &classes, &plan, index, want);
}

/// Make one class's share the plan, as a step, a limit or a snap does.
fn move_row(s: &mut State, classes: &[Class], plan: &Plan, index: usize, want: f64) {
    s.rebalance.press_values = plan.values.clone();
    s.rebalance.press_at = share_of(plan, index);
    s.rebalance.step = (want / SHARE_GRAIN).round() as i64;
    let moved = rebalance::set_class(classes, &plan.values, index, want / 100.0 * total_of(plan));
    paint(s, classes, &moved, true);
    s.rebalance.plan = RebalancePlan::Adjusted {
        names: classes.iter().map(|class| class.name.clone()).collect(),
        values: moved,
    };
    s.rebalance.amount.replace_text("");
    s.rebalance.amount_error = None;
}

// --- Drawing ---

/// Glide the controls to whatever the plan is now, unless a pointer is on one. Runs before
/// every frame; it costs a hash when nothing changed.
pub fn sync(s: &mut State) {
    if s.rebalance.dragging || !s.rebalance.placed {
        return;
    }
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    let mut hasher = DefaultHasher::new();
    for (class, value) in classes.iter().zip(&plan.values) {
        class.name.hash(&mut hasher);
        value.to_bits().hash(&mut hasher);
    }
    s.rebalance.fraction.to_bits().hash(&mut hasher);
    for width in [
        s.rebalance.rail_width,
        s.rebalance.rail_height,
        s.rebalance.track_width,
        s.rebalance.bar_width,
    ] {
        width.to_bits().hash(&mut hasher);
    }
    let painted = hasher.finish().max(1);
    if painted != s.rebalance.painted {
        paint(s, &classes, &plan.values, s.rebalance.painted != 0);
        s.rebalance.painted = painted;
    }
}

/// Publish the controls' geometry for `values`. `spring` lets the bindings' springs carry it;
/// otherwise it lands at once.
fn paint(s: &mut State, classes: &[Class], values: &[f64], spring: bool) {
    let page = &mut s.rebalance;
    let (rail, tall) = (page.rail_width, page.rail_height);
    if rail > 1.0 {
        let at = RAIL_THUMB / 2.0 + page.fraction.clamp(0.0, 1.0) as f32 * (rail - RAIL_THUMB);
        let rect = [
            at - RAIL_THUMB / 2.0,
            (tall - RAIL_THUMB) / 2.0,
            RAIL_THUMB,
            RAIL_THUMB,
        ];
        let fill = [0.0, (tall - RAIL_BAR) / 2.0, at, RAIL_BAR];
        if spring {
            page.rail_thumb.set(rect);
            page.rail_fill.set(fill);
        } else {
            page.rail_thumb.snap(rect);
            page.rail_fill.snap(fill);
        }
    }
    let total: f64 = values.iter().sum();
    let top = top_share(classes).max(1e-6);
    if total <= 0.0 {
        return;
    }
    let count = values.len().min(page.thumbs.len());
    for (index, value) in values.iter().enumerate().take(count) {
        let share = value / total * 100.0;
        let at = (share / top).clamp(0.0, 1.0) as f32 * page.track_width;
        let offset = [at, 0.0];
        if spring {
            page.thumbs[index].set(offset);
        } else {
            page.thumbs[index].snap(offset);
        }
    }
    for (index, value) in values.iter().enumerate().take(count.min(page.bars.len())) {
        let share = (value / total) as f32;
        let start = values[..index].iter().sum::<f64>() / total;
        let x = start as f32 * page.bar_width;
        let end = (start as f32 + share) * page.bar_width;
        let width = (end - x - SLICE_GAP).max(0.0);
        let rect = [x, 0.0, width, PLOT_H];
        if spring {
            page.bars[index].set(rect);
        } else {
            page.bars[index].snap(rect);
        }
    }
    // Today's bar never moves, so it lands at once either way.
    let today: Vec<f64> = classes.iter().map(|class| class.value.max(0.0)).collect();
    let sum: f64 = today.iter().sum();
    if sum <= 0.0 {
        return;
    }
    let mut x = 0.0;
    for (index, value) in today.iter().enumerate().take(count.min(page.ghosts.len())) {
        let width = (value / sum) as f32 * page.bar_width;
        let rect = [x, 0.0, (width - GHOST_GAP).max(0.0), GHOST_H];
        page.ghosts[index].snap(rect);
        x += width;
    }
}

// --- For the demo's scenes ---

/// The plan's values, in the order the classes are listed. For tests and the demo.
pub fn values(s: &State) -> Vec<f64> {
    let classes = load_classes(s);
    shown(s, &classes).values
}

/// Go `fraction` of the way from the portfolio to its targets, as the slider does.
pub fn set_blend(s: &mut State, fraction: f64) {
    s.rebalance.plan = RebalancePlan::Blend;
    s.rebalance.fraction = fraction.clamp(0.0, 1.0);
    s.rebalance.amount.replace_text("");
    s.rebalance.amount_error = None;
}

/// Put one class at `percent` of the portfolio, with cash taking up the difference.
pub fn set_share(s: &mut State, index: usize, percent: f64) {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if index >= classes.len() {
        return;
    }
    let values = rebalance::set_class(&classes, &plan.values, index, percent / 100.0 * plan.total);
    s.rebalance.plan = RebalancePlan::Adjusted {
        names: classes.iter().map(|class| class.name.clone()).collect(),
        values,
    };
}

/// Invest `amount`, or withdraw it when negative, as the field does.
pub fn set_amount(s: &mut State, amount: f64) {
    s.rebalance.amount.replace_text(format!("{amount}"));
    apply_amount(s);
}
