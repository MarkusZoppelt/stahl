//! The rebalance bar: a split the owner can drag, with the trades that would get there.

use rune::{Color, Cursor, Drag, El, Key, KeyEvent, Phase, Spring, Tx, col, el, row, text};
use rune_kit::RollingText;

use crate::{
    format,
    rebalance::{self, Class, Plan},
    state::State,
    theme::{NUMBER, Theme},
    view::ui,
};

/// The bar's height. Tall enough for a finger.
const BAR_H: f32 = 48.0;
/// A handle's hit width. The grip drawn in it is a hairline.
const HANDLE: f32 = 28.0;
const GRIP: f32 = 6.0;
/// How close a press must be to a boundary to grab it, in points.
const GRAB: f32 = 22.0;

/// The magnet: quick, with a small bounce, so a boundary settles onto its target.
const SNAP: Spring = Spring::new(0.18, 0.22);
/// A jump from the keyboard, or a resize, glides rather than pops.
const GLIDE: Spring = Spring::new(0.42, 0.06);

/// The allocation as a bar, and the trades that would reach the split it shows.
pub fn instrument(s: &State, t: &Theme) -> El {
    let classes = load_classes(s);
    if classes.len() < 2 {
        return ui::caption("Add another asset class to drag a boundary.", t);
    }
    let plan = shown(s, &classes);
    col()
        .w_full()
        .gap(16.0)
        .child(bar_card(s, t, &classes, &plan))
        .child(trades_card(s, t, &classes, &plan))
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
    if out.is_empty()
        && let Some(scenario) = s
            .simulation
            .data
            .as_ref()
            .and_then(|simulation| simulation.scenarios.first())
    {
        for item in &scenario.new_allocation {
            out.push(class(&item.asset_class, 0.0, Some(item.percent), None));
        }
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
    if s.rebalance.custom
        && s.rebalance.names.len() == classes.len()
        && s.rebalance
            .names
            .iter()
            .zip(classes)
            .all(|(name, class)| name == &class.name)
    {
        rebalance::from_values(classes, s.rebalance.values.clone(), 0.0)
    } else {
        rebalance::today(classes)
    }
}

fn bar_card(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    ui::card(t)
        .child(ui::header(
            "Allocation",
            "Drag a boundary. It snaps to its target when it gets close. Arrow keys move a \
             focused boundary, and Enter snaps it.",
            t,
        ))
        .child(policy_track(classes, t))
        .child(bar(s, t, classes, plan))
        .child(legend(s, t, classes, plan))
}

fn policy_track(classes: &[Class], t: &Theme) -> El {
    let targeted: Vec<_> = classes
        .iter()
        .filter(|class| class.target.is_some())
        .collect();
    let sum: f64 = targeted.iter().filter_map(|class| class.target).sum();
    if sum <= 0.0 {
        return el();
    }
    col()
        .w_full()
        .gap(6.0)
        .child(ui::caption("Targets", t))
        .child(
            row()
                .center_y()
                .w_full()
                .h(8.0)
                .radius(2.0)
                .clip_rounded(2.0)
                .a11y_hidden()
                .children(targeted.iter().filter_map(|class| {
                    let share = class.target? / sum;
                    Some(
                        el().h_full()
                            .w_frac(share as f32)
                            .fill(t.class_color(&class.name).alpha(0.55)),
                    )
                })),
        )
}

fn bar(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    // The frame is taller than the bar, so handles and target ticks can stand proud of the clip.
    el().w_full()
        .h(BAR_H + 16.0)
        .child(track(s, t, classes, plan))
        .child(ticks(t, classes))
        .child(handles(s, t, classes, plan))
}

fn track(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let total = plan.total.max(0.0);
    el().key("rebalance-bar")
        .absolute(0.0, 8.0)
        .w_full()
        .h(BAR_H)
        .radius(8.0)
        .clip_rounded(8.0)
        .fill(t.sunken)
        .cursor(Cursor::ResizeColumn)
        .role("group")
        .label("Allocation")
        .on_layout_tx(|s: &mut State, rect: [f32; 4], tx: &mut Tx| {
            let width = rect[2];
            let first = !s.rebalance.placed;
            if !first && (s.rebalance.bar_width - width).abs() < 0.5 {
                return;
            }
            s.rebalance.bar_width = width;
            if !s.rebalance.dragging {
                let classes = load_classes(s);
                let plan = shown(s, &classes);
                paint(s, &plan.values, false);
            }
            if first {
                s.rebalance.placed = true;
            } else {
                tx.live_only();
            }
        })
        .on_drag(|s: &mut State, drag: &mut Drag, tx: &mut Tx| gesture(s, drag, tx, drag.x))
        .child(
            row().center_y().w_full().h_full().children(
                classes
                    .iter()
                    .enumerate()
                    .map(|(index, class)| segment(s, t, class, index, share(plan, index, total))),
            ),
        )
}

fn share(plan: &Plan, index: usize, total: f64) -> f32 {
    if total <= 0.0 {
        return 0.0;
    }
    (plan.values.get(index).copied().unwrap_or(0.0) / total).clamp(0.0, 1.0) as f32
}

fn segment(s: &State, t: &Theme, class: &Class, index: usize, fraction: f32) -> El {
    let mut body = el()
        .key(format!("rebalance-seg-{index}"))
        .h_full()
        .w_frac(fraction)
        .fill(t.class_color(&class.name))
        .motion(GLIDE)
        .on_drag(move |s: &mut State, drag: &mut Drag, tx: &mut Tx| {
            gesture(s, drag, tx, segment_x(s, index, drag.x));
        });
    if s.rebalance.placed {
        body = body.live_rect(s.rebalance.rects[index].smooth(SNAP));
    }
    let edge = el()
        .pin(Some(0.0), Some(0.0), Some(0.0), None)
        .h(3.0)
        .fill(band_color(t, class, shown_value(s, index), shown_total(s)));
    body.child(if s.rebalance.dragging {
        edge.live_fill(&s.rebalance.bands[index])
    } else {
        edge
    })
}

fn ticks(t: &Theme, classes: &[Class]) -> El {
    let mut cum = 0.0;
    let mut marks = Vec::new();
    for (index, class) in classes.iter().enumerate() {
        let Some(target) = class.target else {
            break;
        };
        cum += target / 100.0;
        if index + 1 == classes.len() {
            break;
        }
        let at = cum as f32;
        marks.push(
            row()
                .center_y()
                .absolute(0.0, 0.0)
                .w_full()
                .h(8.0)
                .a11y_hidden()
                .child(el().w_frac(at).h_full())
                .child(
                    el().w(2.0)
                        .h_full()
                        .radius(1.0)
                        .fill(t.kit.c.text_base.alpha(0.7))
                        .on_drag(move |s: &mut State, drag: &mut Drag, tx: &mut Tx| {
                            let x = at * s.rebalance.bar_width + drag.x;
                            gesture(s, drag, tx, x);
                        }),
                ),
        );
    }
    col().w_full().h_full().absolute(0.0, 0.0).children(marks)
}

fn handles(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let total = plan.total.max(0.0);
    let mut cum = 0.0_f64;
    let mut row_handles = Vec::new();
    for index in 0..classes.len().saturating_sub(1) {
        cum += plan.values.get(index).copied().unwrap_or(0.0);
        let at = if total > 0.0 { cum / total } else { 0.0 };
        let left = &classes[index];
        let right = &classes[index + 1];
        row_handles.push(
            row()
                .center_y()
                .absolute(0.0, 0.0)
                .w_full()
                .h(BAR_H + 16.0)
                .child(el().w_frac(at as f32).h_full())
                .child(handle(s, t, index, left, right, plan)),
        );
    }
    col()
        .w_full()
        .h_full()
        .absolute(0.0, 0.0)
        .children(row_handles)
}

fn handle(s: &State, t: &Theme, index: usize, left: &Class, right: &Class, plan: &Plan) -> El {
    let total = plan.total.max(0.0);
    let before: f64 = plan.values.iter().take(index).sum();
    let pair = plan.values.get(index).copied().unwrap_or(0.0)
        + plan.values.get(index + 1).copied().unwrap_or(0.0);
    let (min, now, max) = if total > 0.0 {
        (
            before / total * 100.0,
            (before + plan.values.get(index).copied().unwrap_or(0.0)) / total * 100.0,
            (before + pair) / total * 100.0,
        )
    } else {
        (0.0, 0.0, 100.0)
    };
    let label = format!("Boundary between {} and {}", left.name, right.name);
    let value = format!(
        "{} {:.1} percent, {} {:.1} percent",
        left.name,
        share_of(s, index),
        right.name,
        share_of(s, index + 1)
    );
    el().key(format!("rebalance-handle-{index}"))
        .w(HANDLE)
        .h(BAR_H + 16.0)
        .offset(-HANDLE / 2.0, 0.0)
        .cursor(Cursor::ResizeColumn)
        .label(label)
        .description("Left and right move the boundary. Enter snaps it to the target.")
        .a11y_value(value)
        .range_value(now, min, max)
        .range_step(0.5)
        .live_offset(&s.rebalance.offsets[index])
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
            gesture(s, drag, tx, handle_x(s, index, drag.x));
        })
        .child(
            el().w(GRIP)
                .h(BAR_H + 12.0)
                .radius(3.0)
                .fill(t.kit.c.text_base)
                .absolute((HANDLE - GRIP) / 2.0, 2.0),
        )
}

fn share_of(s: &State, index: usize) -> f64 {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if plan.total <= 0.0 {
        return 0.0;
    }
    plan.values.get(index).copied().unwrap_or(0.0) / plan.total * 100.0
}

fn legend(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let total = plan.total.max(0.0);
    row()
        .center_y()
        .flex_wrap()
        .gaps(6.0, 16.0)
        .children(classes.iter().enumerate().map(|(index, class)| {
            let value = plan.values.get(index).copied().unwrap_or(0.0);
            let percent = if total > 0.0 {
                value / total * 100.0
            } else {
                0.0
            };
            let band = band_label(class, value, total);
            let color = band_color(t, class, value, total);
            let dot = ui::dot(color);
            row()
                .center_y()
                .gap(6.0)
                .child(if s.rebalance.dragging {
                    dot.live_fill(&s.rebalance.bands[index])
                } else {
                    dot
                })
                .child(
                    text(class.name.clone())
                        .font(t.kit.ty.ui)
                        .color(t.kit.c.text_base),
                )
                .child(percent_label(s, t, index, percent))
                .child(text(band).font(t.kit.ty.text_xsmall).color(color))
        }))
}

fn percent_label(s: &State, t: &Theme, index: usize, percent: f64) -> El {
    if s.rebalance.dragging {
        text("100.0%")
            .font(NUMBER)
            .color(t.kit.c.text_muted)
            .live_text(&s.rebalance.percents[index])
    } else {
        RollingText::new(format!("rebalance-pct-{index}"), format!("{percent:.1}%"))
            .value(percent)
            .font(NUMBER)
            .color(t.kit.c.text_muted)
            .build(&t.kit)
    }
}

fn band_label(class: &Class, value: f64, total: f64) -> String {
    match (class.target, rebalance::inside_band(class, value, total)) {
        (Some(target), Some(true)) => format!("inside {target:.0}%"),
        (Some(target), Some(false)) => format!("outside {target:.0}%"),
        _ => "no target".to_string(),
    }
}

fn band_color(t: &Theme, class: &Class, value: f64, total: f64) -> Color {
    match rebalance::inside_band(class, value, total) {
        Some(true) => t.up,
        Some(false) => t.warn,
        None => t.kit.c.text_muted,
    }
}

fn shown_total(s: &State) -> f64 {
    shown(s, &load_classes(s)).total
}

fn shown_value(s: &State, index: usize) -> f64 {
    shown(s, &load_classes(s))
        .values
        .get(index)
        .copied()
        .unwrap_or(0.0)
}

fn trades_card(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let heading = if s.rebalance.custom {
        "This split"
    } else {
        "As it is"
    };
    let detail = if s.rebalance.custom {
        "What the bar is showing now. Nothing has been traded."
    } else {
        "The portfolio as it is. Drag a boundary to see the trades."
    };
    ui::card(t)
        .child(ui::header(heading, detail, t))
        .child(trade_table(s, t, classes, plan))
        .child(ui::caption("Stahl never trades.", t))
}

fn trade_table(s: &State, t: &Theme, classes: &[Class], plan: &Plan) -> El {
    let total = plan.total.max(0.0);
    let rows = classes
        .iter()
        .enumerate()
        .map(|(index, class)| {
            let value = plan.values.get(index).copied().unwrap_or(0.0);
            let delta = value - class.value.max(0.0);
            let (word, ink) = action(delta, t);
            let percent = if total > 0.0 {
                value / total * 100.0
            } else {
                0.0
            };
            vec![
                text(class.name.clone())
                    .font(t.kit.ty.ui)
                    .color(t.kit.c.text_base),
                action_cell(s, t, index, word, ink),
                amount_cell(s, t, index, delta, ink),
                percent_label(s, t, index, percent),
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

fn action_cell(s: &State, t: &Theme, index: usize, word: &str, ink: Color) -> El {
    if s.rebalance.dragging {
        text("Increase")
            .font(NUMBER)
            .color(ink)
            .live_text(&s.rebalance.actions[index])
            .live_text_color(&s.rebalance.inks[index])
    } else if word == "Hold" {
        ui::caption(word, t)
    } else {
        ui::badge(word, ink)
    }
}

fn amount_cell(s: &State, t: &Theme, index: usize, delta: f64, ink: Color) -> El {
    if s.rebalance.dragging {
        text("€000,000.00")
            .font(NUMBER)
            .color(ink)
            .live_text(&s.rebalance.amounts[index])
            .live_text_color(&s.rebalance.inks[index])
    } else if delta.abs() < 0.01 {
        ui::caption("—", t)
    } else {
        RollingText::new(format!("rebalance-amt-{index}"), s.money(delta.abs()))
            .value(delta.abs())
            .font(NUMBER)
            .color(ink)
            .build(&t.kit)
    }
}

fn gesture(s: &mut State, drag: &mut Drag, tx: &mut Tx, bar_x: f32) {
    match drag.phase {
        Phase::Down => begin(s, bar_x),
        Phase::Move => {
            if s.rebalance.boundary.is_some() {
                apply_move(s, drag.dx);
                tx.live_only();
            }
        }
        Phase::Up => end(s, drag.cancelled()),
    }
}

fn begin(s: &mut State, bar_x: f32) {
    let classes = load_classes(s);
    if classes.len() < 2 || s.rebalance.bar_width < 1.0 {
        return;
    }
    let plan = shown(s, &classes);
    let Some(index) = nearest(&plan, bar_x, s.rebalance.bar_width) else {
        return;
    };
    s.rebalance.boundary = Some(index);
    s.rebalance.press = plan.values.clone();
    s.rebalance.drag_values = plan.values.clone();
    s.rebalance.held = None;
    s.rebalance.dragging = true;
    write_live(s, &classes, &plan.values);
    paint(s, &plan.values, false);
    for offset in &s.rebalance.offsets {
        offset.snap([0.0, 0.0]);
    }
}

fn apply_move(s: &mut State, dx: f32) {
    let Some(index) = s.rebalance.boundary else {
        return;
    };
    let classes = load_classes(s);
    let press = s.rebalance.press.clone();
    if index >= press.len() {
        return;
    }
    let total: f64 = press.iter().sum();
    let width = s.rebalance.bar_width.max(1.0);
    let want = press[index] + f64::from(dx) / f64::from(width) * total;
    let engage = f64::from(14.0 / width).max(0.008);
    let dragged = rebalance::drag(
        &classes,
        &press,
        index,
        want,
        engage,
        s.rebalance.held,
        engage * 2.2,
    );
    s.rebalance.held = dragged.held;
    s.rebalance.drag_values = dragged.values.clone();
    paint(s, &dragged.values, dragged.held.is_some());
    write_live(s, &classes, &dragged.values);
}

fn end(s: &mut State, cancel: bool) {
    let classes = load_classes(s);
    if cancel || s.rebalance.boundary.is_none() {
        s.rebalance.boundary = None;
        s.rebalance.held = None;
        s.rebalance.dragging = false;
        let plan = shown(s, &classes);
        paint(s, &plan.values, false);
        for offset in &s.rebalance.offsets {
            offset.snap([0.0, 0.0]);
        }
        return;
    }
    let values = s.rebalance.drag_values.clone();
    s.rebalance.names = classes.iter().map(|class| class.name.clone()).collect();
    s.rebalance.values = values;
    s.rebalance.custom = true;
    s.rebalance.dragging = false;
    s.rebalance.boundary = None;
    s.rebalance.held = None;
    for offset in &s.rebalance.offsets {
        offset.snap([0.0, 0.0]);
    }
    let values = s.rebalance.values.clone();
    paint(s, &values, false);
}

fn nearest(plan: &Plan, bar_x: f32, width: f32) -> Option<usize> {
    if plan.values.len() < 2 || width < 1.0 || plan.total <= 0.0 {
        return None;
    }
    let mut best: Option<(usize, f32)> = None;
    let mut cum = 0.0;
    for index in 0..plan.values.len() - 1 {
        cum += plan.values[index];
        let x = (cum / plan.total) as f32 * width;
        let distance = (x - bar_x).abs();
        if distance <= GRAB && best.is_none_or(|(_, near)| distance < near) {
            best = Some((index, distance));
        }
    }
    best.map(|(index, _)| index)
}

fn segment_x(s: &State, index: usize, local: f32) -> f32 {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    let width = s.rebalance.bar_width;
    if plan.total <= 0.0 {
        return local;
    }
    let cum: f64 = plan.values.iter().take(index).sum();
    (cum / plan.total) as f32 * width + local
}

fn handle_x(s: &State, index: usize, local: f32) -> f32 {
    segment_x(s, index + 1, 0.0) - HANDLE / 2.0 + local
}

fn step(s: &mut State, index: usize, delta: i32) {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if index + 1 >= plan.values.len() || plan.total <= 0.0 {
        return;
    }
    let want = plan.values[index] + plan.total * 0.005 * f64::from(delta);
    commit_move(s, &classes, &plan.values, index, want);
}

fn limit(s: &mut State, index: usize, max: bool) {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    commit_move(
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
    let points = rebalance::snap_points(&classes, &plan.values, index, plan.total);
    let want = plan.values.get(index).copied().unwrap_or(0.0);
    let Some(point) = points.into_iter().min_by(|left, right| {
        (want - left)
            .abs()
            .partial_cmp(&(want - right).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    }) else {
        return;
    };
    commit_move(s, &classes, &plan.values, index, point);
}

fn commit_move(s: &mut State, classes: &[Class], values: &[f64], index: usize, want: f64) {
    let floors: Vec<f64> = classes.iter().map(rebalance::floor_of).collect();
    let next = rebalance::move_boundary(values, &floors, index, want);
    s.rebalance.names = classes.iter().map(|class| class.name.clone()).collect();
    s.rebalance.values = next.clone();
    s.rebalance.custom = true;
    s.rebalance.dragging = false;
    for offset in &s.rebalance.offsets {
        offset.snap([0.0, 0.0]);
    }
    // A keyboard move glides. A drag has already landed.
    paint(s, &next, true);
}

fn paint(s: &mut State, values: &[f64], spring: bool) {
    let width = s.rebalance.bar_width;
    let total: f64 = values.iter().copied().sum();
    if width < 1.0 || total <= 0.0 {
        return;
    }
    let layout = layout_values(s, values.len());
    let layout_total: f64 = layout.iter().sum();
    let mut x = 0.0_f32;
    let mut layout_x = 0.0_f32;
    for (index, value) in values.iter().enumerate().take(s.rebalance.rects.len()) {
        let w = (*value / total) as f32 * width;
        let rect = [x, 0.0, w.max(0.0), BAR_H];
        if spring {
            s.rebalance.rects[index].set(rect);
        } else {
            s.rebalance.rects[index].snap(rect);
        }
        x += w;
        if index + 1 < values.len() && layout_total > 0.0 {
            layout_x += (layout[index] / layout_total) as f32 * width;
            let dx = x - layout_x;
            if spring {
                s.rebalance.offsets[index].set([dx, 0.0]);
            } else {
                s.rebalance.offsets[index].snap([dx, 0.0]);
            }
        }
    }
}

/// Where the handles are laid out: the committed plan, or these values if it isn't one yet.
fn layout_values(s: &State, len: usize) -> Vec<f64> {
    let classes = load_classes(s);
    let plan = shown(s, &classes);
    if plan.values.len() == len {
        plan.values
    } else {
        vec![0.0; len]
    }
}

fn write_live(s: &mut State, classes: &[Class], values: &[f64]) {
    let total: f64 = values.iter().sum();
    let theme = Theme::new(s.is_dark());
    let hidden = s.privacy;
    let currency = s.currency();
    for (index, class) in classes.iter().enumerate().take(s.rebalance.amounts.len()) {
        let value = values.get(index).copied().unwrap_or(0.0);
        let delta = value - class.value.max(0.0);
        let (word, ink) = action(delta, &theme);
        s.rebalance.actions[index].set(word);
        s.rebalance.inks[index].set(ink);
        let amount = if hidden {
            format::HIDDEN.to_string()
        } else if delta.abs() < 0.01 {
            "—".to_string()
        } else {
            format::currency(delta.abs(), &currency)
        };
        s.rebalance.amounts[index].set(amount);
        let percent = if total > 0.0 {
            value / total * 100.0
        } else {
            0.0
        };
        s.rebalance.percents[index].set(format!("{percent:.1}%"));
        s.rebalance.bands[index].set(match rebalance::inside_band(class, value, total) {
            Some(true) => theme.up,
            Some(false) => theme.warn,
            None => theme.kit.c.text_muted,
        });
    }
}
