//! The blocks of an answer the owner can play with.
//!
//! A what-if used to be a table of percentages. Here it is the portfolio moving: the total rolls
//! down to what the shock would leave, the allocation bar reshapes into the shocked shares, and a
//! Now/Then toggle lets the owner push the change back and forth. A plan block carries the
//! Rebalance page's own slider in small, so the fraction can be dragged right there in the answer.

use rune::{Color, Cursor, El, Key, Phase, Spring, col, el, icon, row, text, tok::motion::LEAD};
use rune_kit::{Button, ButtonVariant, ControlSize, RollingText};

use crate::{
    ask, icons,
    rebalance::{self, Trade},
    scenario::{Line, Outcome, Shock},
    state::{Page, State},
    theme::Theme,
    view::{ask::Slot, rebalance as plan, ui},
};

/// The bar reshaping into the shocked allocation: about a second, heavy, no bounce, so the eye
/// follows the change instead of being startled by it.
const RESHAPE: Spring = Spring::new(0.95, 0.0);

/// How wide the word that marks a class outside its band may be.
const MARK: f32 = 46.0;

/// The slider's thumb, and how tall the whole control is.
const THUMB: f32 = 12.0;
const RAIL: f32 = 24.0;

/// A what-if, played: today on the left of the toggle, what the shock would leave on the right.
pub fn scenario(s: &State, t: &Theme, shocks: &[Shock], slot: &Slot, reduced: bool) -> El {
    let classes = plan::load_classes(s);
    let outcome = crate::scenario::project(&classes, shocks);
    let then = showing_then(s, slot, reduced);
    let open = s
        .ask
        .what_if_at(slot.turn, slot.at)
        .is_some_and(|what_if| what_if.details);
    let key = slot.key.clone();
    let total = if then {
        outcome.total_after
    } else {
        outcome.total_before
    };
    let change = outcome.change();
    let tone = t.change(change);
    let note = if shocks.is_empty() {
        "Nothing changes.".to_string()
    } else {
        let list = shocks.iter().map(name).collect::<Vec<_>>().join(", ");
        format!("{list}.")
    };
    col()
        .key(format!("{key}-scenario"))
        .w_full()
        .max_w(760.0)
        .gap(14.0)
        .child(
            row()
                .center_y()
                .w_full()
                .gap(16.0)
                .child(col().grow(1.0).w(0.0).min_w(0.0).child(ui::header(
                    "If that happened",
                    note,
                    t,
                )))
                .child(toggle(slot, then, t, &key)),
        )
        .child(
            col()
                .gap(2.0)
                .child(ui::caption(
                    if then { "What would be left" } else { "Today" },
                    t,
                ))
                .child(figure(
                    s,
                    t,
                    &key,
                    total,
                    then.then_some((change, outcome.percent(), tone)),
                )),
        )
        .child(bar(&outcome, then, &key, t))
        .child(
            col()
                .key(format!("{key}-classes"))
                .w_full()
                .gap(6.0)
                .children(
                    outcome
                        .lines
                        .iter()
                        .filter(|line| line.before >= 0.005 || line.after >= 0.005)
                        .map(|line| legend(line, then, t)),
                ),
        )
        .children((!outcome.unknown.is_empty()).then(|| {
            ui::caption(
                format!(
                    "Nothing in the portfolio is called {}.",
                    outcome.unknown.join(", ")
                ),
                t,
            )
        }))
        .child(details(s, t, &outcome, slot, open))
}

/// The total, rolling to the value it is showing, with the difference beside it where there is
/// room and under it on a phone.
fn figure(
    s: &State,
    t: &Theme,
    key: &str,
    total: f64,
    change: Option<(f64, Option<f64>, Color)>,
) -> El {
    let amount = RollingText::new(format!("{key}-total"), s.money(total))
        .font(crate::theme::FIGURE)
        .color(t.kit.c.text_base)
        .value(total)
        .build(&t.kit)
        .key(format!("{key}-total"));
    let difference = change.map(|(change, percent, tone)| {
        row()
            .center_y()
            .gap(8.0)
            .transition(rune::Transition::opacity())
            .child(
                text(s.signed_money(change))
                    .font(t.kit.ty.ui_medium)
                    .color(tone),
            )
            .children(percent.map(|percent| {
                text(format!("{percent:+.1}%"))
                    .font(t.kit.ty.ui_medium)
                    .color(tone)
            }))
    });
    if s.compact {
        col()
            .gap(4.0)
            .child(amount)
            .children(difference.map(|difference| difference.pb(0.0)))
    } else {
        row()
            .center_y()
            .items_end()
            .gap(14.0)
            .child(amount)
            .children(difference.map(|difference| difference.pb(4.0)))
    }
}

/// Whether the block is showing what the shock would leave: it starts on today, plays the change
/// after a beat, and stays wherever the owner puts it. Under reduced motion it starts settled on
/// "Then", with the toggle there to look back.
pub fn showing_then(s: &State, slot: &Slot, reduced: bool) -> bool {
    if let Some(what_if) = s.ask.what_if_at(slot.turn, slot.at) {
        if what_if.now {
            return false;
        }
        if reduced {
            return true;
        }
    }
    match slot.age {
        Some(age) => age >= ask::PLAY_HOLD,
        None => true,
    }
}

/// How a shock reads in words: "Crypto falls 50%".
fn name(shock: &Shock) -> String {
    if shock.percent <= -100.0 {
        format!("{} goes to nothing", shock.class)
    } else if shock.percent < 0.0 {
        format!("{} falls {:.0}%", shock.class, -shock.percent)
    } else if shock.percent > 0.0 {
        format!("{} rises {:.0}%", shock.class, shock.percent)
    } else {
        format!("{} stands still", shock.class)
    }
}

/// Now / Then: two stops, focusable, and the arrow keys move between them.
fn toggle(slot: &Slot, then: bool, t: &Theme, key: &str) -> El {
    let (turn, at) = (slot.turn, slot.at);
    let selected = usize::from(then);
    let font = rune::TextStyle {
        line_height: 13.0,
        ..t.kit.ty.ui
    };
    let segment = |label: &str, index: usize| {
        let on = index == selected;
        row()
            .key(format!("{key}-{}", label.to_lowercase()))
            .role("radio")
            .label(label.to_string())
            .position_in_set(index + 1, 2)
            .focus_selected(on)
            .focusable(true)
            .focus_ring(2.0, 1.0, t.kit.c.border_focus)
            .h(26.0)
            .px(12.0)
            .center()
            .radius(6.0)
            .grid_cell(index)
            .on_press(move |s: &mut State| s.what_if_show(turn, at, index == 0))
            .child(text(label.to_string()).font(font).color(if on {
                t.kit.c.text_base
            } else {
                t.kit.c.text_muted
            }))
    };
    row()
        .key(format!("{key}-toggle"))
        .role("radiogroup")
        .label("Show")
        .focus_group()
        .h(26.0)
        .center_y()
        .radius(6.0)
        .fill(t.kit.c.layer_01)
        .box_shadow(rune::Shadow {
            spread: 0.5,
            color: t.kit.c.border_base,
            ..Default::default()
        })
        .equal_columns(2)
        .child(
            el().key("indicator")
                .h(26.0)
                .radius(6.0)
                .fill(t.kit.c.bg_base)
                .box_shadow(rune::Shadow {
                    spread: 0.5,
                    color: t.kit.c.border_strong,
                    ..Default::default()
                })
                .grid_cell(selected)
                .a11y_hidden(),
        )
        .child(segment("Now", 0))
        .child(segment("Then", 1))
        .on_key(move |s: &mut State, event, _| match event.key {
            Key::Left => {
                s.what_if_show(turn, at, true);
                true
            }
            Key::Right => {
                s.what_if_show(turn, at, false);
                true
            }
            _ => false,
        })
}

/// The stacked bar, reshaping from today's shares to the shocked ones.
fn bar(outcome: &Outcome, then: bool, key: &str, t: &Theme) -> El {
    row()
        .key(format!("{key}-bar"))
        .center_y()
        .w_full()
        .h(8.0)
        .gap(2.0)
        .radius(3.0)
        .clip_rounded(3.0)
        .fill(t.kit.c.layer_03)
        .children(
            outcome
                .lines
                .iter()
                .enumerate()
                .filter(|(_, line)| line.before >= 0.005 || line.after >= 0.005)
                .map(|(index, line)| {
                    let share = if then {
                        line.after_share
                    } else {
                        line.before_share
                    };
                    el().key(format!("{key}-slice-{index}"))
                        .h_full()
                        .fill(t.class_color(&line.name))
                        .w_frac((share / 100.0).clamp(0.0, 1.0) as f32)
                        .motion(RESHAPE)
                }),
        )
}

/// One class under the bar: its share now, its share then, and a word if it is outside its band.
fn legend(line: &Line, then: bool, t: &Theme) -> El {
    row()
        .center_y()
        .w_full()
        .gap(8.0)
        .child(ui::dot(t.class_color(&line.name)))
        .child(
            text(line.name.clone())
                .font(t.kit.ty.ui)
                .color(t.kit.c.text_base)
                .grow(1.0)
                .min_w(0.0)
                .truncate(),
        )
        .child(ui::number(
            format!("{:.1}%", line.before_share),
            if then {
                t.kit.c.text_muted
            } else {
                t.kit.c.text_base
            },
        ))
        .child(ui::caption("→", t))
        .child(ui::number(
            format!("{:.1}%", line.after_share),
            if then {
                t.kit.c.text_base
            } else {
                t.kit.c.text_muted
            },
        ))
        .child(row().w(MARK).justify_end().children(mark(line, t)))
}

/// Where a class stands after the shock, when that is worth a word: the warning colour outside
/// its policy band, the gain colour when the shock brings it back inside.
fn mark(line: &Line, t: &Theme) -> Option<El> {
    let word = |word: &str, color: Color| {
        Some(
            text(word.to_string())
                .font(t.kit.ty.text_xsmall)
                .color(color),
        )
    };
    if line.returns_to_band() {
        return word("inside", t.up);
    }
    match (line.inside_after, line.target) {
        (Some(false), Some(target)) if line.after_share > target => word("over", t.warn),
        (Some(false), _) => word("under", t.warn),
        _ => None,
    }
}

/// The table behind "Details": every class in money, now and then. Closed by default.
fn details(s: &State, t: &Theme, outcome: &Outcome, slot: &Slot, open: bool) -> El {
    let key = slot.key.clone();
    let rows = outcome
        .lines
        .iter()
        .filter(|line| line.before >= 0.005 || line.after >= 0.005)
        .map(|line| {
            vec![
                row()
                    .center_y()
                    .gap(8.0)
                    .child(ui::dot(t.class_color(&line.name)))
                    .child(
                        text(line.name.clone())
                            .font(t.kit.ty.ui)
                            .color(t.kit.c.text_base)
                            .truncate(),
                    ),
                ui::number(s.money(line.before), t.kit.c.text_muted),
                ui::number(s.money(line.after), t.kit.c.text_base),
                ui::number(format!("{:.1}%", line.after_share), t.kit.c.text_muted),
            ]
        })
        .collect();
    let (turn, at) = (slot.turn, slot.at);
    let trigger = row()
        .key(format!("{key}-details"))
        .role("button")
        .label("Details")
        .expanded(open)
        .focusable(true)
        .center_y()
        .gap(6.0)
        .py(4.0)
        .on_press(move |s: &mut State| s.ask.what_if_details(turn, at))
        .child(
            text(if open { "Hide details" } else { "Details" })
                .font(t.kit.ty.text_xsmall)
                .color(t.kit.c.text_muted),
        )
        .child(
            icon(&icons::CHEVRON_RIGHT, 12.0)
                .rotate(if open { 90.0 } else { 0.0 })
                .rotate_motion(rune::tok::motion::SETTLE)
                .color(t.kit.c.text_faint),
        );
    col()
        .key(format!("{key}-details-block"))
        .w_full()
        .items_start()
        .child(trigger)
        .children(open.then(|| {
            col()
                .key(format!("{key}-details-body"))
                .w_full()
                .pt(6.0)
                .child(ui::table(&["Class", "Now", "Then", "Share"], rows, t))
        }))
}

/// A rebalance plan, compactly: the trades, and the fraction they were worked out at, draggable
/// right here.
pub fn plan_block(
    s: &State,
    t: &Theme,
    fraction: Option<f64>,
    amount: Option<f64>,
    slot: &Slot,
) -> El {
    let classes = plan::load_classes(s);
    let key = slot.key.clone();
    let dragged = s
        .ask
        .plan_at(slot.turn, slot.at)
        .and_then(|play| play.fraction);
    let fraction = dragged.or(fraction);
    let (plan, title, detail) = match (fraction, amount) {
        (Some(fraction), _) => {
            let plan = rebalance::blend(&classes, fraction);
            let title = if fraction >= 0.999 {
                "All the way to your targets".to_string()
            } else if fraction <= 0.001 {
                "Where the portfolio is now".to_string()
            } else {
                format!(
                    "{}% of the way to your targets",
                    (fraction * 100.0).round() as i64
                )
            };
            (plan, title, "The total stays where it is.".to_string())
        }
        (_, Some(amount)) if amount >= 0.0 => (
            rebalance::place(&classes, amount).unwrap_or_else(|_| rebalance::today(&classes)),
            "Where new money goes".to_string(),
            format!("Buying only, {} in all.", s.money(amount)),
        ),
        (_, Some(amount)) => (
            rebalance::place(&classes, amount).unwrap_or_else(|_| rebalance::today(&classes)),
            "Where a withdrawal comes from".to_string(),
            format!("Selling only, {} in all.", s.money(-amount)),
        ),
        _ => (rebalance::today(&classes), String::new(), String::new()),
    };
    let trades: Vec<El> = plan
        .trades
        .iter()
        .map(|trade| trade_row(s, t, trade))
        .collect();
    col()
        .key(format!("{key}-plan"))
        .w_full()
        .max_w(760.0)
        .gap(10.0)
        .child(ui::header(title, detail, t))
        .children(fraction.map(|fraction| slider(s, t, fraction, slot, &key)))
        .children(trades)
        .children(plan.trades.is_empty().then(|| {
            ui::caption(
                "Nothing to trade: every class is already where this plan wants it.",
                t,
            )
        }))
        .child(
            row().items_start().child(
                Button::new("Open in Rebalance")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .icon(&icons::SCALE)
                    .on_press(move |s: &mut State| open_in_rebalance(s, fraction, amount))
                    .build(&t.kit)
                    .key(format!("{key}-open-plan")),
            ),
        )
}

/// One class's trade: what it does, and how much.
fn trade_row(s: &State, t: &Theme, trade: &Trade) -> El {
    row()
        .center_y()
        .w_full()
        .gap(10.0)
        .child(ui::dot(t.class_color(&trade.asset_class)))
        .child(
            text(trade.asset_class.clone())
                .font(t.kit.ty.ui)
                .color(t.kit.c.text_base)
                .grow(1.0)
                .truncate(),
        )
        .child(ui::caption(trade.action.label(), t))
        .child(ui::number(s.money(trade.amount), t.kit.c.text_base))
}

/// The Rebalance page, showing the plan the answer does, at the fraction the owner dragged it to.
fn open_in_rebalance(s: &mut State, fraction: Option<f64>, amount: Option<f64>) {
    match (fraction, amount) {
        (Some(fraction), _) => plan::set_blend(s, fraction),
        (None, Some(amount)) => plan::set_amount(s, amount),
        (None, None) => {}
    }
    s.navigate(Page::Rebalance);
}

/// The Rebalance page's "how far" slider, in small: drag it, or use the arrow keys, and the trades
/// above it are worked out again.
fn slider(s: &State, t: &Theme, fraction: f64, slot: &Slot, key: &str) -> El {
    let (turn, at) = (slot.turn, slot.at);
    let width = s.ask.plan_at(turn, at).map_or(0.0, |play| play.width);
    let percent = (fraction * 100.0).round();
    let travel = (width - THUMB).max(1.0);
    let thumb_at = fraction as f32 * travel;
    let track = el()
        .key(format!("{key}-rail"))
        .w_full()
        .h(RAIL)
        .role("slider")
        .label("How far to rebalance")
        .description(
            "Left is the portfolio as it is, right is every target met. Return goes to the \
             nearest stop.",
        )
        .a11y_value(format!("{percent:.0} percent of the way"))
        .range_value(percent, 0.0, 100.0)
        .range_step(1.0)
        .focusable(true)
        .focus_ring(2.0, -2.0, t.info)
        .cursor(Cursor::Pointer)
        .on_layout(move |s: &mut State, rect: [f32; 4]| {
            s.ask.plan_measure(turn, at, rect[2]);
        })
        .on_drag(
            move |s: &mut State, drag: &mut rune::Drag, _: &mut rune::Tx| {
                let width = f64::from(
                    s.ask
                        .plan_at(turn, at)
                        .map_or(0.0, |play| play.width)
                        .max(1.0),
                );
                let fraction = snap((f64::from(drag.x) / width).clamp(0.0, 1.0));
                match drag.phase {
                    Phase::Down => s.ask.plan_drag(turn, at, true),
                    Phase::Up => s.ask.plan_drag(turn, at, false),
                    Phase::Move => {}
                }
                s.ask.plan_set(turn, at, fraction);
            },
        )
        .on_step(move |s: &mut State, by: i32, _| {
            if by == 0 {
                return;
            }
            let current = s
                .ask
                .plan_at(turn, at)
                .and_then(|play| play.fraction)
                .unwrap_or(0.0);
            let points = if by.abs() >= 10 { 5.0 } else { 1.0 };
            s.ask
                .plan_set(turn, at, current + by.signum() as f64 * points / 100.0);
        })
        .on_key(move |s: &mut State, event, _| {
            if event.key != Key::Enter {
                return false;
            }
            let current = s
                .ask
                .plan_at(turn, at)
                .and_then(|play| play.fraction)
                .unwrap_or(0.0);
            let nearest = [0.0, 0.5, 1.0]
                .into_iter()
                .min_by(|a, b| (current - a).abs().total_cmp(&(current - b).abs()))
                .unwrap_or(0.0);
            s.ask.plan_set(turn, at, nearest);
            true
        })
        .child(
            el().absolute(0.0, (RAIL - 4.0) / 2.0)
                .w_full()
                .h(4.0)
                .radius(2.0)
                .fill(t.kit.c.layer_03),
        )
        .child(
            el().absolute(0.0, (RAIL - 4.0) / 2.0)
                .w(thumb_at + THUMB / 2.0)
                .h(4.0)
                .radius(2.0)
                .fill(t.accent)
                .motion(LEAD),
        )
        .child(
            el().absolute(thumb_at, (RAIL - THUMB) / 2.0)
                .size(THUMB, THUMB)
                .radius(THUMB / 2.0)
                .fill(t.kit.c.text_base)
                .border(2.0, t.card)
                .motion(LEAD),
        );
    col()
        .key(format!("{key}-slider"))
        .w_full()
        .gap(2.0)
        .child(track)
        .child(
            row()
                .w_full()
                .justify_between()
                .child(ui::caption("As it is", t))
                .child(
                    RollingText::new(format!("{key}-percent"), format!("{percent:.0}%"))
                        .font(t.kit.ty.text_xsmall)
                        .color(t.kit.c.text_base)
                        .build(&t.kit),
                )
                .child(ui::caption("On target", t)),
        )
}

/// The slider's three stops: as it is, halfway, on target.
fn snap(fraction: f64) -> f64 {
    const DETENT: f64 = 0.02;
    if fraction < DETENT {
        0.0
    } else if fraction > 1.0 - DETENT {
        1.0
    } else if (fraction - 0.5).abs() < DETENT {
        0.5
    } else {
        fraction
    }
}
