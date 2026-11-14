//! The future as a fan, under the value chart on the Performance page.
//!
//! A band from the tenth to the ninetieth percentile, a line at the median, two sliders and an
//! optional goal. The assumptions the fan is drawn on sit directly under it, always visible:
//! this is the bank's model, not a promise about the market.

use rune::{Cursor, Cx, El, Shape, canvas, col, el, row, text};
use rune_kit::TextInput;

use crate::{
    outlook::Outlook,
    state::{State, parse_number},
    theme::{FIGURE, LEDE, LEDE_COMPACT, NUMBER, Theme},
    view::ui,
};

/// The fan's height on a page, and on a phone.
const FAN_H: f32 = 220.0;
const FAN_H_COMPACT: f32 = 180.0;
/// The sliders' lane, and the track and thumb on it.
const LANE_H: f32 = 28.0;
const TRACK_H: f32 = 8.0;
const THUMB: f32 = 14.0;
/// The monthly amounts the slider offers.
const MONTHLY_MAX: f64 = 5_000.0;
const MONTHLY_STEP: f64 = 50.0;

/// The whole section, ready to sit under the value chart.
pub fn section(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let outlook = &s.outlook.simulated;
    ui::card(t)
        .key("outlook-section")
        .w_full()
        .gap(16.0)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_between()
                .flex_wrap()
                .gaps(16.0, 12.0)
                .child(ui::title("Where this could go", t))
                .child(ui::caption("An assumption, not a forecast.", t)),
        )
        .child(sentence(s, t, outlook))
        .child(fan(s, t, cx, outlook))
        .child(figures(s, t, outlook))
        .child(controls(s, t, cx))
        .children(goal_note(s, t, outlook))
        .child(ui::rule(t))
        .child(assumptions(s, t, outlook))
}

/// The horizon as the sentence says it.
fn horizon(years: u32) -> String {
    if years == 1 {
        "A year from now".to_string()
    } else {
        format!("{years} years from now")
    }
}

/// One sentence, in the bank's voice: a range, and never a promise.
fn sentence(s: &State, t: &Theme, outlook: &Outlook) -> El {
    let Some(band) = outlook.horizon() else {
        return ui::muted("There is nothing to project yet.", t);
    };
    let when = horizon(s.outlook.years);
    let line = if s.privacy {
        format!(
            "{when}, the portfolio could sit anywhere across this range, with the median path \
             in the middle."
        )
    } else {
        format!(
            "{when}, the portfolio could sit between {} and {}, with {} the middle of the range.",
            s.money_thousands(band.low),
            s.money_thousands(band.high),
            s.money_thousands(band.median),
        )
    };
    text(line)
        .font(if s.compact { LEDE_COMPACT } else { LEDE })
        .color(t.kit.c.text_base)
        .wrap()
}

/// The tenth, median and ninetieth percentile at the horizon.
fn figures(s: &State, t: &Theme, outlook: &Outlook) -> El {
    let Some(band) = outlook.horizon() else {
        return el();
    };
    let figure = |key: &'static str, label: &'static str, value: String| {
        col()
            .gap(4.0)
            .min_w(0.0)
            .child(ui::caption(label, t))
            .child(
                text(value)
                    .key(key)
                    .font(FIGURE)
                    .color(t.kit.c.text_base)
                    .selectable(true),
            )
    };
    row()
        .center_y()
        .w_full()
        .flex_wrap()
        .gaps(16.0, 32.0)
        .child(figure(
            "outlook-low",
            "Ten paths in a hundred below",
            s.money(band.low),
        ))
        .child(figure("outlook-median", "Median", s.money(band.median)))
        .child(figure(
            "outlook-high",
            "Ten paths in a hundred above",
            s.money(band.high),
        ))
}

/// The fan itself: the band, its edges, the median, the goal and the axis under it.
fn fan(s: &State, t: &Theme, cx: &Cx<State>, outlook: &Outlook) -> El {
    let key = "outlook-fan";
    let height = if s.compact { FAN_H_COMPACT } else { FAN_H };
    let axis = row()
        .w_full()
        .justify_between()
        .child(ui::caption("Today", t))
        .child(ui::caption(format!("{} years", s.outlook.years), t));
    let today = outlook.months.first().map_or(0.0, |band| band.median);
    let mut low = today;
    let mut high = today;
    for band in &outlook.months {
        low = low.min(band.low);
        high = high.max(band.high);
    }
    if let Some(goal) = s.outlook.goal_amount {
        low = low.min(goal);
        high = high.max(goal);
    }
    let margin = ((high - low) * 0.08).max(high.abs() * 0.005).max(1.0);
    let (low, high) = (low - margin, high + margin);
    let range = (high - low).max(1e-6);
    let Some([width, _]) = cx.layout_size(key) else {
        return col()
            .w_full()
            .gap(4.0)
            .child(el().key(key).w_full().h(height))
            .child(axis);
    };
    let count = outlook.months.len().max(2);
    let point = |index: usize, value: f64| -> [f32; 2] {
        [
            index as f32 / (count - 1) as f32 * width,
            height - (((value - low) / range).clamp(0.0, 1.0) as f32) * height,
        ]
    };
    let mut band_points: Vec<[f32; 2]> = outlook
        .months
        .iter()
        .enumerate()
        .map(|(index, band)| point(index, band.low))
        .collect();
    band_points.extend(
        outlook
            .months
            .iter()
            .enumerate()
            .rev()
            .map(|(index, band)| point(index, band.high)),
    );
    let low_edge: Vec<[f32; 2]> = outlook
        .months
        .iter()
        .enumerate()
        .map(|(index, band)| point(index, band.low))
        .collect();
    let high_edge: Vec<[f32; 2]> = outlook
        .months
        .iter()
        .enumerate()
        .map(|(index, band)| point(index, band.high))
        .collect();
    let shape = Shape::new()
        .fill(band_points, t.info.alpha(0.16))
        .stroke(low_edge, 1.0, t.info.alpha(0.4))
        .stroke(high_edge, 1.0, t.info.alpha(0.4));
    let progress = s.draw_progress(cx.is_reduced_motion());
    let median = Shape::new()
        .stroke(
            outlook
                .months
                .iter()
                .enumerate()
                .map(|(index, band)| point(index, band.median))
                .collect::<Vec<_>>(),
            1.5,
            t.info,
        )
        .drawn(progress);
    let mut lines = Shape::new();
    for line in 1..4 {
        let y = height * line as f32 / 4.0;
        lines = lines.stroke(
            vec![[0.0, y], [width, y]],
            1.0,
            t.kit.c.text_base.alpha(0.05),
        );
    }
    let goal = s.outlook.goal_amount.map(|goal| {
        let y = point(0, goal)[1];
        (
            y,
            Shape::new().stroke(
                vec![[0.0, y], [width, y]],
                1.0,
                t.kit.c.text_muted.alpha(0.7),
            ),
        )
    });
    let start = point(0, today);
    let label = format!(
        "Projected value over {} years, from the tenth to the ninetieth percentile.",
        s.outlook.years
    );
    let chart = el()
        .key(key)
        .role("img")
        .label(label)
        .w_full()
        .h(height)
        .child(canvas(lines).absolute(0.0, 0.0).size(width, height))
        .child(
            canvas(shape)
                .absolute(0.0, 0.0)
                .size(width, height)
                .opacity(progress),
        )
        .child(canvas(median).absolute(0.0, 0.0).size(width, height))
        .children(goal.map(|(y, line)| {
            col()
                .absolute(0.0, 0.0)
                .size(width, height)
                .inert()
                .child(canvas(line).absolute(0.0, 0.0).size(width, height))
                .child(
                    ui::caption("Goal", t)
                        .absolute((width - 32.0).max(0.0), (y - 16.0).max(0.0))
                        .a11y_hidden(),
                )
        }))
        .child(
            el().absolute(start[0] - 3.0, start[1] - 3.0)
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(t.info)
                .a11y_hidden(),
        );
    col().w_full().gap(4.0).child(chart).child(axis)
}

/// The two sliders and the goal field.
fn controls(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let mut input = TextInput::new(&s.outlook.goal)
        .placeholder("250000")
        .width(None)
        .label("Goal, optional")
        .on_edit(|s: &mut State| &mut s.outlook.goal)
        .on_change(|s: &mut State, _| apply_goal(s))
        .build(&t.kit)
        .key("outlook-goal");
    if s.privacy {
        input = input.secure();
    }
    let goal = col()
        .w_full()
        .gap(6.0)
        .child(ui::caption("Goal (optional)", t))
        .child(input);
    ui::columns(ui::room(s, cx), [3, 2, 1], 24.0)
        .child(slider(
            t,
            "outlook-years",
            "Years",
            "How far ahead to look, from one to thirty years.",
            f64::from(s.outlook.years),
            (1.0, 30.0),
            1.0,
            format!("{} years", s.outlook.years),
            |s, by| {
                s.outlook.years = (s.outlook.years as i32 + by).clamp(1, 30) as u32;
                s.refresh_outlook();
            },
            |s, max| {
                s.outlook.years = if max { 30 } else { 1 };
                s.refresh_outlook();
            },
        ))
        .child(slider(
            t,
            "outlook-monthly",
            "Every month",
            "What is added to the portfolio each month.",
            s.outlook.monthly,
            (0.0, MONTHLY_MAX),
            MONTHLY_STEP,
            s.money(s.outlook.monthly),
            |s, by| {
                s.outlook.monthly =
                    (s.outlook.monthly + f64::from(by) * MONTHLY_STEP).clamp(0.0, MONTHLY_MAX);
                s.refresh_outlook();
            },
            |s, max| {
                s.outlook.monthly = if max { MONTHLY_MAX } else { 0.0 };
                s.refresh_outlook();
            },
        ))
        .child(goal)
}

/// A slider: its label and figure above, a track and a thumb below. Arrows step it, Home and
/// End go to the ends.
#[allow(clippy::too_many_arguments)]
fn slider(
    t: &Theme,
    key: &'static str,
    label: &'static str,
    description: &'static str,
    value: f64,
    (min, max): (f64, f64),
    step: f64,
    shown: String,
    change: fn(&mut State, i32),
    limit: fn(&mut State, bool),
) -> El {
    let fraction = ((value - min) / (max - min)).clamp(0.0, 1.0) as f32;
    let bar_y = (LANE_H - TRACK_H) / 2.0;
    col()
        .w_full()
        .gap(6.0)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_between()
                .gap(8.0)
                .child(ui::caption(label, t))
                .child(
                    text(shown.clone())
                        .font(NUMBER)
                        .color(t.kit.c.text_base)
                        .truncate(),
                ),
        )
        .child(
            el().key(key)
                .w_full()
                .h(LANE_H)
                .cursor(Cursor::Pointer)
                .role("slider")
                .label(label)
                .description(description)
                .a11y_value(shown)
                .range_value(value, min, max)
                .range_step(step)
                .focus_ring(2.0, -4.0, t.info)
                .on_step(move |s: &mut State, by, _| change(s, by))
                .on_step_limit(move |s: &mut State, max, _| limit(s, max))
                .child(
                    el().absolute(0.0, bar_y)
                        .w_full()
                        .h(TRACK_H)
                        .radius(3.0)
                        .fill(t.kit.c.layer_03),
                )
                .child(
                    el().absolute(0.0, bar_y)
                        .w_frac(fraction)
                        .h(TRACK_H)
                        .radius(3.0)
                        .fill(t.info),
                )
                .child(
                    row()
                        .center_y()
                        .absolute(0.0, 0.0)
                        .w_full()
                        .h(LANE_H)
                        .inert()
                        .a11y_hidden()
                        .child(el().w_frac(fraction))
                        .child(
                            el().size(THUMB, THUMB)
                                .radius(THUMB / 2.0)
                                .offset(-THUMB / 2.0, 0.0)
                                .fill(t.info)
                                .border(2.0, t.card),
                        ),
                ),
        )
}

/// `N in 10 paths reach it`, when there is a goal.
fn goal_note(s: &State, t: &Theme, outlook: &Outlook) -> Option<El> {
    let goal = s.outlook.goal_amount?;
    let reached = outlook.reached?;
    let in_ten = (reached * 10.0).round().clamp(0.0, 10.0) as u32;
    let line = if s.privacy {
        format!("{in_ten} in 10 paths reach your goal.")
    } else {
        format!("{in_ten} in 10 paths reach {}.", s.money(goal))
    };
    Some(ui::caption(line, t).key("outlook-goal-note").wrap())
}

/// A percentage for the assumptions: whole points, and a decimal where a whole point would say
/// nothing.
fn percent(value: f64) -> String {
    let points = value * 100.0;
    if points > 0.0 && points < 1.0 {
        format!("{points:.1}%")
    } else {
        format!("{points:.0}%")
    }
}

/// The assumptions, directly under the fan: what was used, and whose they are.
fn assumptions(s: &State, t: &Theme, outlook: &Outlook) -> El {
    let items: Vec<El> = outlook
        .used
        .iter()
        .map(|used| {
            let assumption = used.assumption;
            ui::caption(
                format!(
                    "{} {} a year, give or take {}{}",
                    used.asset_class,
                    percent(assumption.annual_return),
                    percent(assumption.volatility),
                    if used.from_policy { ", yours" } else { "" },
                ),
                t,
            )
        })
        .collect();
    let line = if outlook.used.iter().any(|used| used.from_policy) {
        "Assumptions, not a forecast. Those marked yours come from your policy; the rest are \
         the bank's."
    } else if s.is_workspace() {
        "Assumptions, not a forecast. Set your own in [outlook] in the policy."
    } else {
        "Assumptions, not a forecast."
    };
    col()
        .w_full()
        .gap(6.0)
        .child(ui::caption(line, t).wrap())
        .child(row().flex_wrap().gaps(6.0, 16.0).children(items))
}

/// Read the goal field. Anything that is not a positive number means no goal.
fn apply_goal(s: &mut State) {
    let text = s.outlook.goal.text.clone();
    s.outlook.goal_amount = match parse_number(&text) {
        Ok(Some(amount)) if amount > 0.0 => Some(amount),
        _ => None,
    };
    s.refresh_outlook();
}

/// Set the goal, as the demo's scenes do.
pub fn set_goal(s: &mut State, goal: Option<f64>) {
    match goal {
        Some(goal) => {
            s.outlook.goal.replace_text(format!("{goal:.0}"));
            s.outlook.goal_amount = Some(goal);
        }
        None => {
            s.outlook.goal.replace_text("");
            s.outlook.goal_amount = None;
        }
    }
    s.refresh_outlook();
}

/// The median at the horizon, as the page shows it. For tests.
pub fn median(s: &State) -> Option<f64> {
    s.outlook.simulated.horizon().map(|band| band.median)
}
