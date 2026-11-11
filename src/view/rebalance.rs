//! The rebalance bar: a split the owner can drag, with the trades that would get there.
//!
//! The bar's geometry lives in `Live` values (`State::rebalance`), so a pointer move redraws
//! the bar without running the view. The view runs again only when a figure under the bar
//! would change (each quarter point, or when the magnet takes or lets go), and then the
//! amounts roll to their new digits.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    mem,
};

use rune::{Color, Cursor, Cx, Drag, El, Key, KeyEvent, Phase, Spring, Tx, col, el, row, text};
use rune_kit::{RollingText, SegmentedControl, TextInput};

use crate::{
    format,
    rebalance::{self, Class, Plan},
    state::{BadNumber, RebalanceOrigin, State, parse_number},
    theme::{NUMBER, Theme},
    view::ui,
};

/// The lane above the bar for the target marks and their tolerance bands.
const MARK: f32 = 14.0;
/// How far a mark reaches down into the bar.
const NOTCH: f32 = 10.0;
/// The bar's height. Tall enough for a finger.
const BAR_H: f32 = 48.0;
/// The in-band lane under each class, and the space above it.
const LANE: f32 = 3.0;
const LANE_GAP: f32 = 6.0;
const FRAME_H: f32 = MARK + BAR_H + LANE_GAP + LANE;
/// Space between two classes on the bar.
const GAP: f32 = 2.0;
/// A handle's hit width. The grip drawn in it is narrow.
const HANDLE: f32 = 28.0;
const GRIP: f32 = 6.0;
/// How close a press on the bar must be to a boundary to grab it, in points.
const GRAB: f32 = 22.0;
/// The magnet's reach, in points, and how far the pointer must go to pull free.
const ENGAGE: f32 = 12.0;
const RELEASE: f32 = 26.0;

/// Under a finger or pointer: quick, with a little give, so the magnet lands with a bounce.
const FOLLOW: Spring = Spring::new(0.16, 0.18);
/// A preset, new money or a key: the whole bar travels together.
const GLIDE: Spring = Spring::new(0.5, 0.08);

/// The allocation as a bar, and the trades that would reach the split it shows.
pub fn instrument(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let classes = load_classes(s);
    if classes.len() < 2 {
        return ui::caption("Add another asset class to drag a boundary.", t);
    }
    let plan = shown(s, &classes);
    col()
        .w_full()
        .gap(16.0)
        .child(bar_card(s, t, &classes, &plan))
        .child(if ui::is_wide(s, cx) {
            row()
                .w_full()
                .gap(16.0)
                .items_start()
                .child(trades_card(s, t, &classes, &plan).grow(2.0).w(0.0))
                .child(money_card(s, t, &plan).grow(1.0).w(0.0))
        } else {
            col()
                .w_full()
                .gap(16.0)
                .child(money_card(s, t, &plan))
                .child(trades_card(s, t, &classes, &plan))
        })
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

fn shown(s: &State, classes: &[Class]) -> Plan {
    match s.rebalance.origin {
        RebalanceOrigin::Custom if names_match(s, classes) => {
            rebalance::from_values(classes, s.rebalance.values.clone(), 0.0)
        }
        RebalanceOrigin::Flow(amount) => {
            rebalance::place(classes, amount).unwrap_or_else(|_| rebalance::today(classes))
        }
        RebalanceOrigin::Full => rebalance::full(classes),
        RebalanceOrigin::Conservative => rebalance::conservative(classes),
        RebalanceOrigin::Today | RebalanceOrigin::Custom => rebalance::today(classes),
    }
}

fn names_match(s: &State, classes: &[Class]) -> bool {
    s.rebalance.names.len() == classes.len()
        && s.rebalance
            .names
            .iter()
            .zip(classes)
            .all(|(name, class)| name == &class.name)
}

fn share(plan: &Plan, index: usize) -> f64 {
    if plan.total <= 0.0 {
        return 0.0;
    }
    plan.values.get(index).copied().unwrap_or(0.0) / plan.total * 100.0
}

// --- The bar ---

const PRESETS: [&str; 3] = ["As it is", "Full rebalance", "Conservative"];

fn preset_index(origin: RebalanceOrigin) -> Option<usize> {
    match origin {
        RebalanceOrigin::Today => Some(0),
        RebalanceOrigin::Full => Some(1),
        RebalanceOrigin::Conservative => Some(2),
        RebalanceOrigin::Flow(_) | RebalanceOrigin::Custom => None,
    }
}

fn bar_card(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let presets = SegmentedControl::new(&PRESETS, preset_index(s.rebalance.origin))
        .label("Start from")
        .fit()
        .on_pick(|s: &mut State, index, _| {
            choose(
                s,
                match index {
                    1 => RebalanceOrigin::Full,
                    2 => RebalanceOrigin::Conservative,
                    _ => RebalanceOrigin::Today,
                },
            );
        })
        .build(&t.kit)
        .key("rebalance-presets");
    ui::card(t)
        .gap(16.0)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_between()
                .flex_wrap()
                .gaps(12.0, 12.0)
                .child(
                    ui::header(
                        "Allocation",
                        "Drag a boundary. Near its mark, where every class before it is on \
                         target, it snaps. The band above a mark is that class's tolerance.",
                        t,
                    )
                    .grow(1.0)
                    .min_w(240.0)
                    .w(0.0),
                )
                .child(presets),
        )
        .child(bar(s, t, classes, plan))
        .child(legend(t, classes, plan))
        // A phone has no arrow keys to speak of; the finger is the path there.
        .children((!s.compact).then(|| {
            ui::caption(
                "Arrow keys move a focused boundary by half a point, Page Up and Page Down by \
                 five, and Return puts it on its mark.",
                t,
            )
            .wrap()
        }))
}

fn bar(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let spring = if s.rebalance.dragging { FOLLOW } else { GLIDE };
    let placed = s.rebalance.placed;
    let marks = rebalance::target_boundaries(classes).unwrap_or_default();
    let segments = classes.iter().enumerate().map(|(index, class)| {
        let body = el()
            .key(format!("rebalance-seg-{index}"))
            .absolute(0.0, MARK)
            .radius(3.0)
            .fill(t.class_color(&class.name));
        if placed {
            body.live_rect(s.rebalance.rects[index].smooth(spring))
        } else {
            body
        }
    });
    let lanes = classes.iter().enumerate().map(|(index, class)| {
        let value = plan.values.get(index).copied().unwrap_or(0.0);
        let lane = el()
            .key(format!("rebalance-lane-{index}"))
            .absolute(0.0, MARK + BAR_H + LANE_GAP)
            .radius(1.0)
            .fill(band_color(t, class, value, plan.total));
        if placed {
            lane.live_rect(s.rebalance.lanes[index].smooth(spring))
        } else {
            lane
        }
    });
    let handles = (0..classes.len() - 1).map(|index| handle(s, t, classes, plan, index, spring));
    el().key("rebalance-bar")
        .w_full()
        .h(FRAME_H)
        .cursor(Cursor::ResizeColumn)
        .role("group")
        .label("Allocation")
        .on_layout_tx(|s: &mut State, rect: [f32; 4], tx: &mut Tx| {
            let width = rect[2];
            if s.rebalance.placed && (s.rebalance.bar_width - width).abs() < 0.5 {
                return;
            }
            s.rebalance.bar_width = width;
            if !s.rebalance.dragging {
                let values = shown(s, &load_classes(s)).values;
                paint(s, &values, false);
            }
            if s.rebalance.placed {
                tx.live_only();
            } else {
                s.rebalance.placed = true;
            }
        })
        .on_drag(|s: &mut State, drag: &mut Drag, tx: &mut Tx| gesture(s, drag, tx, None))
        .child(
            el().absolute(0.0, MARK)
                .w_full()
                .h(BAR_H)
                .radius(3.0)
                .fill(t.sunken),
        )
        .children(segments)
        .children(lanes)
        .child(guides(t, classes, &marks))
        .children(handles)
}

/// Each boundary's target, as a mark notched into the top of the bar, and above it the band
/// where the class before it is within tolerance (when every class before that is on
/// target).
fn guides(t: &Theme, classes: &[Class], marks: &[f64]) -> El {
    let ink = t.kit.c.text_base;
    let mut layer = el().absolute(0.0, 0.0).w_full().h(MARK + NOTCH);
    for (index, mark) in marks.iter().enumerate() {
        let Some(class) = classes.get(index) else {
            continue;
        };
        let band = class
            .target
            .map(|_| class.tolerance.unwrap_or(rebalance::DEFAULT_TOLERANCE) / 100.0)
            .unwrap_or(0.0);
        let from = (mark - band).max(0.0);
        let to = (mark + band).min(1.0);
        if to > from {
            layer = layer.child(
                row()
                    .absolute(0.0, 3.0)
                    .w_full()
                    .h(4.0)
                    .child(el().w_frac(from as f32))
                    .child(
                        el().w_frac((to - from) as f32)
                            .h_full()
                            .radius(2.0)
                            .fill(t.class_color(&class.name).alpha(0.55)),
                    ),
            );
        }
        layer = layer.child(
            row()
                .absolute(0.0, 0.0)
                .w_full()
                .h(MARK + NOTCH)
                .child(el().w_frac(*mark as f32))
                .child(el().w(2.0).h_full().offset(-1.0, 0.0).radius(1.0).fill(ink)),
        );
    }
    layer.inert().a11y_hidden()
}

fn handle(
    s: &State,
    t: &Theme,
    classes: &[Class],
    plan: &Plan,
    index: usize,
    spring: Spring,
) -> El {
    let (left, right) = (&classes[index], &classes[index + 1]);
    let before: f64 = plan.values.iter().take(index).sum();
    let value = plan.values.get(index).copied().unwrap_or(0.0);
    let pair = value + plan.values.get(index + 1).copied().unwrap_or(0.0);
    let percent = |amount: f64| {
        if plan.total > 0.0 {
            amount / plan.total * 100.0
        } else {
            0.0
        }
    };
    let on_mark = rebalance::snap_points(classes, &plan.values, index, plan.total)
        .first()
        .is_some_and(|point| (point - value).abs() < plan.total * 1e-6);
    let grip = if on_mark { t.up } else { t.kit.c.text_base };
    let mut handle = el()
        .key(format!("rebalance-handle-{index}"))
        .absolute(-HANDLE / 2.0, 0.0)
        .w(HANDLE)
        .h(FRAME_H)
        .cursor(Cursor::ResizeColumn)
        .label(format!("Boundary between {} and {}", left.name, right.name))
        .description("Arrow keys move it. Return puts it on its target.")
        .a11y_value(format!(
            "{} {:.1} percent, {} {:.1} percent",
            left.name,
            share(plan, index),
            right.name,
            share(plan, index + 1)
        ))
        .range_value(
            percent(before + value),
            percent(before),
            percent(before + pair),
        )
        .range_step(0.5)
        .focus_ring(2.0, 2.0, t.info)
        .on_step(move |s: &mut State, delta: i32, _| step(s, index, delta))
        .on_step_limit(move |s: &mut State, max: bool, _| limit(s, index, max))
        .on_key(move |s: &mut State, ev: &KeyEvent, _| {
            if ev.key == Key::Enter {
                snap_to_target(s, index);
                true
            } else {
                false
            }
        })
        .on_drag(move |s: &mut State, drag: &mut Drag, tx: &mut Tx| {
            gesture(s, drag, tx, Some(index));
        })
        .child(
            el().absolute((HANDLE - GRIP) / 2.0, MARK - 4.0)
                .w(GRIP)
                .h(BAR_H + 8.0)
                .radius(3.0)
                .fill(grip)
                .border(1.5, t.sunken),
        );
    if s.rebalance.placed {
        handle = handle.live_offset(s.rebalance.offsets[index].smooth(spring));
    } else {
        handle = handle.opacity(0.0);
    }
    handle
}

fn legend(t: &Theme, classes: &[Class], plan: &Plan) -> El {
    row()
        .w_full()
        .flex_wrap()
        .gaps(10.0, 24.0)
        .children(classes.iter().enumerate().map(|(index, class)| {
            let value = plan.values.get(index).copied().unwrap_or(0.0);
            let percent = share(plan, index);
            let status = band_color(t, class, value, plan.total);
            let (word, target) = match (
                class.target,
                rebalance::inside_band(class, value, plan.total),
            ) {
                (Some(target), Some(inside)) => (
                    if inside { "inside" } else { "outside" },
                    format!(
                        "of {target:.0} ±{:.0}",
                        class.tolerance.unwrap_or(rebalance::DEFAULT_TOLERANCE)
                    ),
                ),
                _ => ("no target", String::new()),
            };
            col()
                .gap(2.0)
                .child(
                    row()
                        .center_y()
                        .gap(6.0)
                        .child(ui::dot(t.class_color(&class.name)))
                        .child(
                            text(class.name.clone())
                                .font(t.kit.ty.ui_medium)
                                .color(t.kit.c.text_base),
                        )
                        .child(
                            RollingText::new(
                                format!("rebalance-pct-{index}"),
                                format!("{percent:.1}%"),
                            )
                            .value(percent)
                            .font(NUMBER)
                            .color(t.kit.c.text_base)
                            .build(&t.kit),
                        ),
                )
                .child(
                    row()
                        .center_y()
                        .gap(4.0)
                        .pl(14.0)
                        .child(text(word).font(t.kit.ty.text_xsmall).color(status))
                        .child(
                            text(target)
                                .font(t.kit.ty.text_xsmall)
                                .color(t.kit.c.text_muted),
                        ),
                )
        }))
}

fn band_color(t: &Theme, class: &Class, value: f64, total: f64) -> Color {
    match rebalance::inside_band(class, value, total) {
        Some(true) => t.up,
        Some(false) => t.warn,
        None => t.kit.c.border_base,
    }
}

// --- Presets and new money ---

fn choose(s: &mut State, origin: RebalanceOrigin) {
    s.rebalance.origin = origin;
    s.rebalance.amount.replace_text("");
    s.rebalance.amount_error = None;
}

/// The plan on screen, as Markdown, whatever produced it.
pub fn markdown(s: &State) -> String {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    let (heading, detail) = plan_copy(s);
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
        detail,
        &policy,
        &s.currency(),
        format::amount,
    )
}

fn plan_copy(s: &State) -> (&'static str, &'static str) {
    match s.rebalance.origin {
        RebalanceOrigin::Flow(amount) if amount >= 0.0 => (
            "Where new money goes",
            "Buying only, selling nothing, as close to the targets as that gets.",
        ),
        RebalanceOrigin::Flow(_) => (
            "Where a withdrawal comes from",
            "Selling only, buying nothing, as close to the targets as that gets.",
        ),
        RebalanceOrigin::Full => (
            "Full Rebalance to Targets",
            "Rebalance all asset classes to policy targets.",
        ),
        RebalanceOrigin::Conservative => (
            "Conservative Partial Rebalance",
            "Only rebalance positions that exceed tolerance bands, by half the drift.",
        ),
        RebalanceOrigin::Custom => (
            "This split",
            "What the bar shows now, and the trades that would get there.",
        ),
        RebalanceOrigin::Today => (
            "As it is",
            "No trades yet. Drag a boundary, pick a starting point, or add new money.",
        ),
    }
}

fn money_card(s: &State, t: &Theme, plan: &Plan) -> El {
    let input = TextInput::new(&s.rebalance.amount)
        .placeholder("2000")
        .width(None)
        .label("New money")
        .on_edit(|s: &mut State| &mut s.rebalance.amount)
        .on_change(|s: &mut State, _| apply_amount(s))
        .build(&t.kit)
        .key("rebalance-amount");
    let mut card = ui::card(t)
        .child(ui::header(
            "I have this much to invest",
            "Where new money should go to get closest to your targets, buying only. A \
             negative amount is a withdrawal, selling only.",
            t,
        ))
        .child(input);
    if let Some(error) = &s.rebalance.amount_error {
        card = card.child(ui::danger(error.clone(), t).key("rebalance-amount-error"));
    } else if let RebalanceOrigin::Flow(amount) = s.rebalance.origin {
        let note = if amount >= 0.0 {
            format!("Buying only. Afterwards {}.", s.money(plan.total))
        } else {
            format!("Selling only. Afterwards {}.", s.money(plan.total))
        };
        card = card.child(ui::caption(note, t).key("rebalance-flow"));
    }
    card
}

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
            s.rebalance.origin = RebalanceOrigin::Today;
        }
        Ok(Some(amount)) => match rebalance::place(&load_classes(s), amount) {
            Ok(_) => {
                s.rebalance.amount_error = None;
                s.rebalance.origin = RebalanceOrigin::Flow(amount);
            }
            Err(error) => s.rebalance.amount_error = Some(error.message().to_string()),
        },
        Err(error) => s.rebalance.amount_error = Some(amount_error(&text, error)),
    }
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

// --- The trades ---

fn trades_card(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let (heading, detail) = plan_copy(s);
    let bought: f64 = plan
        .trades
        .iter()
        .filter(|trade| trade.action == rebalance::Action::Increase)
        .map(|trade| trade.amount)
        .sum();
    let sold: f64 = plan
        .trades
        .iter()
        .filter(|trade| trade.action == rebalance::Action::Reduce)
        .map(|trade| trade.amount)
        .sum();
    let summary = if plan.trades.is_empty() {
        format!("No trades. The total is {}.", s.money(plan.total))
    } else if plan.external.abs() < 0.01 {
        format!(
            "Buys {} and sells {}. The total stays {}.",
            s.money(bought),
            s.money(sold),
            s.money(plan.total)
        )
    } else {
        format!(
            "Buys {} and sells {}. Afterwards {}.",
            s.money(bought),
            s.money(sold),
            s.money(plan.total)
        )
    };
    ui::card(t)
        .key("rebalance-trades")
        .child(ui::header(heading, detail, t))
        .child(trade_table(s, t, classes, plan))
        .child(
            col()
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
                )),
        )
}

fn trade_table(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let rows = classes
        .iter()
        .enumerate()
        .map(|(index, class)| {
            let value = plan.values.get(index).copied().unwrap_or(0.0);
            let delta = value - class.value.max(0.0);
            let (word, ink) = action(delta, t);
            let percent = share(plan, index);
            vec![
                row()
                    .center_y()
                    .gap(8.0)
                    .child(ui::dot(t.class_color(&class.name)))
                    .child(
                        text(class.name.clone())
                            .font(t.kit.ty.ui)
                            .color(t.kit.c.text_base),
                    ),
                if word == "Hold" {
                    ui::caption(word, t)
                } else {
                    ui::badge(word, ink)
                },
                RollingText::new(
                    format!("rebalance-amt-{index}"),
                    if delta.abs() < 0.01 {
                        "—".to_string()
                    } else {
                        s.money(delta.abs())
                    },
                )
                .value(delta.abs())
                .font(NUMBER)
                .color(if word == "Hold" {
                    t.kit.c.text_muted
                } else {
                    ink
                })
                .trailing()
                .build(&t.kit),
                RollingText::new(format!("rebalance-after-{index}"), format!("{percent:.1}%"))
                    .value(percent)
                    .font(NUMBER)
                    .color(t.kit.c.text_muted)
                    .trailing()
                    .build(&t.kit),
            ]
        })
        .collect();
    ui::table(&["Asset class", "Action", "Amount", "Afterwards"], rows, t)
}

fn action(delta: f64, t: &Theme) -> (&'static str, Color) {
    if delta > 0.01 {
        ("Increase", t.up)
    } else if delta < -0.01 {
        ("Reduce", t.down)
    } else {
        ("Hold", t.kit.c.text_muted)
    }
}

// --- Input ---

/// A press on the bar (`grabbed` is `None`) takes the nearest boundary; a press on a handle
/// takes that one.
fn gesture(s: &mut State, drag: &mut Drag, tx: &mut Tx, grabbed: Option<usize>) {
    match drag.phase {
        Phase::Down => begin(s, drag.x, grabbed),
        Phase::Move => {
            if s.rebalance.boundary.is_some() && !apply_move(s, drag.dx) {
                tx.live_only();
            }
        }
        Phase::Up => end(s, drag.cancelled()),
    }
}

fn begin(s: &mut State, x: f32, grabbed: Option<usize>) {
    let classes = load_classes(s);
    if classes.len() < 2 || s.rebalance.bar_width < 1.0 {
        return;
    }
    let plan = shown(s, &classes);
    let Some(index) = grabbed.or_else(|| nearest(&plan, x, s.rebalance.bar_width)) else {
        return;
    };
    s.rebalance.boundary = Some(index);
    s.rebalance.press = plan.values.clone();
    s.rebalance.drag_values = plan.values.clone();
    s.rebalance.held = None;
    s.rebalance.step = quarter(&plan.values, index);
    s.rebalance.dragging = true;
}

/// Follow the pointer. True when the figures under the bar changed and the view must run.
fn apply_move(s: &mut State, dx: f32) -> bool {
    let Some(index) = s.rebalance.boundary else {
        return false;
    };
    let classes = load_classes(s);
    let press = s.rebalance.press.clone();
    if index + 1 >= press.len() || press.len() != classes.len() {
        return false;
    }
    let total: f64 = press.iter().sum();
    let width = f64::from(s.rebalance.bar_width.max(1.0));
    let want = press[index] + f64::from(dx) / width * total;
    let dragged = rebalance::drag(
        &classes,
        &press,
        index,
        want,
        f64::from(ENGAGE) / width,
        s.rebalance.held,
        f64::from(RELEASE) / width,
    );
    let magnet = dragged.held.is_some() != s.rebalance.held.is_some();
    s.rebalance.held = dragged.held;
    s.rebalance.drag_values = dragged.values.clone();
    paint(s, &dragged.values, true);
    let step = quarter(&dragged.values, index);
    if !magnet && step == s.rebalance.step {
        return false;
    }
    s.rebalance.step = step;
    keep(s, &classes, dragged.values);
    true
}

/// The dragged class's share in quarter points: the grain the figures commit at.
fn quarter(values: &[f64], index: usize) -> i64 {
    let total: f64 = values.iter().sum();
    if total <= 0.0 {
        return 0;
    }
    (values.get(index).copied().unwrap_or(0.0) / total * 400.0).round() as i64
}

fn end(s: &mut State, cancel: bool) {
    let classes = load_classes(s);
    let grabbed = s.rebalance.boundary.take().is_some();
    s.rebalance.dragging = false;
    s.rebalance.held = None;
    let (press, dragged) = (
        mem::take(&mut s.rebalance.press),
        mem::take(&mut s.rebalance.drag_values),
    );
    if !grabbed || dragged.len() != classes.len() {
        return;
    }
    if !cancel && dragged != press {
        keep(s, &classes, dragged);
    } else if s.rebalance.origin == RebalanceOrigin::Custom {
        // The figures may have followed the drag; a cancel puts them back.
        keep(s, &classes, press);
    }
}

/// Make `values` the plan on screen, as a split the owner chose.
fn keep(s: &mut State, classes: &[Class], values: Vec<f64>) {
    s.rebalance.names = classes.iter().map(|class| class.name.clone()).collect();
    s.rebalance.values = values;
    s.rebalance.origin = RebalanceOrigin::Custom;
    s.rebalance.amount.replace_text("");
    s.rebalance.amount_error = None;
}

fn nearest(plan: &Plan, x: f32, width: f32) -> Option<usize> {
    if plan.values.len() < 2 || width < 1.0 || plan.total <= 0.0 {
        return None;
    }
    let mut best: Option<(usize, f32)> = None;
    let mut at = 0.0;
    for index in 0..plan.values.len() - 1 {
        at += plan.values[index];
        let distance = ((at / plan.total) as f32 * width - x).abs();
        if distance <= GRAB && best.is_none_or(|(_, near)| distance < near) {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

/// Move boundary `index` by `steps` half points, as the arrow keys do. The demo's scenes use it.
pub fn nudge(s: &mut State, index: usize, steps: i32) {
    step(s, index, steps);
}

fn step(s: &mut State, index: usize, delta: i32) {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if index + 1 >= plan.values.len() || plan.total <= 0.0 {
        return;
    }
    let want = plan.values[index] + plan.total * 0.005 * f64::from(delta);
    move_to(s, &classes, &plan.values, index, want);
}

fn limit(s: &mut State, index: usize, max: bool) {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    move_to(
        s,
        &classes,
        &plan.values,
        index,
        if max { f64::MAX } else { 0.0 },
    );
}

fn snap_to_target(s: &mut State, index: usize) {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if let Some(point) = rebalance::snap_points(&classes, &plan.values, index, plan.total)
        .first()
        .copied()
    {
        move_to(s, &classes, &plan.values, index, point);
    }
}

fn move_to(s: &mut State, classes: &[Class], values: &[f64], index: usize, want: f64) {
    let floors: Vec<f64> = classes.iter().map(rebalance::floor_of).collect();
    keep(
        s,
        classes,
        rebalance::move_boundary(values, &floors, index, want),
    );
}

// --- Drawing ---

/// Glide the bar to whatever the plan on screen is now, unless a finger is on it. Runs
/// before every frame; it costs a hash when nothing changed.
pub fn sync(s: &mut State) {
    if s.rebalance.boundary.is_some() || !s.rebalance.placed {
        return;
    }
    let classes = load_classes(s);
    let values = shown(s, &classes).values;
    let mut hasher = DefaultHasher::new();
    for (class, value) in classes.iter().zip(&values) {
        class.name.hash(&mut hasher);
        value.to_bits().hash(&mut hasher);
    }
    s.rebalance.bar_width.to_bits().hash(&mut hasher);
    let painted = hasher.finish().max(1);
    if painted != s.rebalance.painted {
        paint(s, &values, s.rebalance.painted != 0);
        s.rebalance.painted = painted;
    }
}

/// Publish the bar's geometry for `values`. `spring` lets the bindings' springs carry it;
/// otherwise it lands at once.
fn paint(s: &mut State, values: &[f64], spring: bool) {
    let width = s.rebalance.bar_width;
    let total: f64 = values.iter().copied().sum();
    if width < 1.0 || total <= 0.0 {
        return;
    }
    let count = values.len().min(s.rebalance.rects.len());
    let mut x = 0.0_f32;
    for (index, value) in values.iter().enumerate().take(count) {
        let w = (*value / total) as f32 * width;
        let gap = if index + 1 < count { GAP } else { 0.0 };
        let drawn = (w - gap).max(0.0);
        let rect = [x, MARK, drawn, BAR_H];
        let lane = [x, MARK + BAR_H + LANE_GAP, drawn, LANE];
        let offset = [x + w - gap / 2.0, 0.0];
        let page = &s.rebalance;
        if spring {
            page.rects[index].set(rect);
            page.lanes[index].set(lane);
            page.offsets[index].set(offset);
        } else {
            page.rects[index].snap(rect);
            page.lanes[index].snap(lane);
            page.offsets[index].snap(offset);
        }
        x += w;
    }
}
