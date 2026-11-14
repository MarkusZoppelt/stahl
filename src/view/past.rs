//! Turning time back, on every page: the strip in the toolbar that holds the recorded line, the
//! band that says which day the app is showing, and what a page about today says in the past.
//!
//! The strip's playhead is a live value: a drag moves it without rebuilding the view, and only a
//! new day (when the whole app has something new to show) commits.

use chrono::{Datelike, Local};
use rune::{
    Cx, Drag, El, Key, KeyEvent, Phase, Shape, Spring, Tx, canvas, col, el, icon, row, text,
};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant};

use crate::{
    icons,
    state::State,
    theme::{SECTION, Theme},
    time::{self, STRIP},
    view::ui,
};

/// The strip's height: the line, and a row of month ticks under it.
const STRIP_H: f32 = 26.0;

/// The playhead's weight: it trails the hand a little and settles without bouncing about.
pub const HEAD: Spring = Spring::new(0.32, 0.06);

/// The recorded totals, then today's.
pub fn line(s: &State) -> Vec<f64> {
    s.time
        .days
        .iter()
        .map(|day| day.total)
        .chain(s.today_total())
        .collect()
}

/// Where each value lands in a box `width` by `height`, zoomed to the range.
pub fn plot(values: &[f64], width: f32, height: f32) -> Vec<[f32; 2]> {
    let (low, high) = values
        .iter()
        .fold((f64::MAX, f64::MIN), |(low, high), value| {
            (low.min(*value), high.max(*value))
        });
    let margin = ((high - low) * 0.1).max(high.abs() * 0.001).max(0.01);
    let (low, high) = (low - margin, high + margin);
    let last = values.len().saturating_sub(1).max(1) as f32;
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            [
                index as f32 / last * width,
                height - ((value - low) / (high - low)) as f32 * height,
            ]
        })
        .collect()
}

/// A disc as a polygon, for a canvas.
pub fn disc(center: [f32; 2], radius: f32) -> Vec<[f32; 2]> {
    (0..20)
        .map(|step| {
            let angle = step as f32 / 20.0 * std::f32::consts::TAU;
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]
        })
        .collect()
}

/// The recorded day nearest `x` along a strip `width` wide, with `None` for today.
fn day_at(s: &State, x: f32, width: f32) -> Option<usize> {
    let count = s.time.days.len();
    let index = ((x / width).clamp(0.0, 1.0) * count as f32).round() as usize;
    (index < count).then_some(index)
}

/// One move of a hand on the strip: the playhead follows it, and the app moves only when it
/// reaches another day.
fn drag_strip(s: &mut State, drag: &mut Drag, tx: &mut Tx) {
    let x = drag.x.clamp(0.0, STRIP);
    match drag.phase {
        Phase::Down => s.time.dragging = true,
        Phase::Up => {
            s.time.dragging = false;
            // Let go, it settles on the day it reached.
            s.place_head();
            return;
        }
        Phase::Move => {}
    }
    let day = day_at(s, x, STRIP);
    if day != s.time.at {
        s.travel(day);
    } else if drag.phase == Phase::Move {
        tx.live_only();
    }
    s.time.head.set([x, 0.0]);
}

/// The keyboard's way along the strip: a recorded day with an arrow, a week with Shift, the
/// first day and today with Home and End.
pub fn time_key(s: &mut State, event: &KeyEvent, _tx: &mut Tx) -> bool {
    if event.mods.cmd || event.mods.ctrl || event.mods.alt {
        return false;
    }
    let week = if event.mods.shift { 7 } else { 1 };
    match event.key {
        Key::Left => s.step_time(-week),
        Key::Right => s.step_time(week),
        Key::Home if !s.time.days.is_empty() => s.travel(Some(0)),
        Key::End => s.travel(None),
        _ => return false,
    }
    true
}

/// What the strip and the band call the day shown.
pub fn day_label(s: &State) -> String {
    match s.past_day() {
        Some(day) => time::long(day.date, Local::now().date_naive()),
        None => "Today".to_string(),
    }
}

/// The strip in the toolbar: the recorded line, with today at its right end and a playhead to
/// pull back along it. Nothing without two recorded days.
pub fn strip(s: &State, t: &Theme, cx: &Cx<State>) -> Option<El> {
    if s.time.days.len() < 2 || s.positions.is_empty() {
        return None;
    }
    let k = &t.kit;
    let values = line(s);
    let plotted = plot(&values, STRIP, STRIP_H - 8.0);
    let head = s.time.at.unwrap_or(s.time.days.len());
    let past = s.in_past();
    let ink = if past { t.info } else { k.c.text_muted };
    // The line up to the day shown is in ink; after it, what has not happened yet, as seen from
    // there, is faint.
    let (before, after) = plotted.split_at((head + 1).min(plotted.len()));
    let mut shape = Shape::new();
    if before.len() >= 2 {
        shape = shape.stroke(before.to_vec(), 1.25, ink);
    }
    if !after.is_empty() {
        let mut rest = vec![before[before.len() - 1]];
        rest.extend_from_slice(after);
        shape = shape.stroke(rest, 1.0, k.c.text_faint.alpha(0.6));
    }
    // A tick where each month begins.
    for (index, day) in s.time.days.iter().enumerate() {
        if day.date.day() == 1 {
            let x = plotted[index][0];
            shape = shape.stroke(
                vec![[x, STRIP_H - 4.0], [x, STRIP_H - 1.0]],
                1.0,
                k.c.text_faint,
            );
        }
    }
    let today = plotted[plotted.len() - 1];
    shape = shape.fill(disc(today, 2.25), if past { k.c.text_faint } else { ink });
    let at = plotted[head.min(plotted.len() - 1)];
    let binding = if cx.is_reduced_motion() {
        (&s.time.head).into()
    } else {
        s.time.head.smooth(HEAD)
    };
    // The playhead: a hairline through the day, and a bead on the line.
    let playhead = el()
        .key("time-head")
        .absolute(-4.0, 0.0)
        .size(9.0, STRIP_H)
        .live_offset(binding)
        .a11y_hidden()
        .child(
            el().absolute(4.0, 0.0)
                .size(1.0, STRIP_H - 2.0)
                .fill(if past { t.info } else { k.c.border_strong }),
        )
        .child(
            el().absolute(1.5, at[1] - 3.0)
                .size(6.0, 6.0)
                .radius(3.0)
                .fill(if past { t.info } else { k.c.text_base })
                .border(1.5, k.c.bg_base),
        );
    let label = day_label(s);
    Some(
        row()
            .center_y()
            .gap(10.0)
            .child(
                el().key("time-strip")
                    .role("slider")
                    .label("Time")
                    .a11y_value(label.clone())
                    .focusable(true)
                    .size(STRIP, STRIP_H)
                    .on_drag(drag_strip)
                    .on_key(time_key)
                    .help("Drag back to any recorded day. Arrow keys move a day, Shift a week.")
                    .child(canvas(shape).absolute(0.0, 0.0).size(STRIP, STRIP_H))
                    .child(playhead),
            )
            .child(
                text(if past {
                    s.past_day()
                        .map(|day| time::short(day.date))
                        .unwrap_or_default()
                } else {
                    "Today".to_string()
                })
                .key("time-strip-date")
                .font(k.ty.text_xsmall_medium)
                .color(if past { t.info } else { k.c.text_muted }),
            ),
    )
}

/// The band over every page while the app shows the past: which day, a step either way, and the
/// one way back. Pinned above the page, so it never scrolls away.
pub fn band(s: &State, t: &Theme) -> Option<El> {
    let day = s.past_day()?;
    let k = &t.kit;
    let narrow = s.compact;
    let first = s.time.at == Some(0);
    let step = |glyph: &'static rune::Icon, label: &str, key: &str, by: i64, off: bool| {
        IconButton::new(glyph)
            .variant(IconButtonVariant::Ghost)
            .label(label.to_string())
            .help(label.to_string())
            .disabled(off)
            .on_press(move |s: &mut State| s.step_time(by))
            .build(k)
            .key(key.to_string())
    };
    let date = time::long(day.date, Local::now().date_naive());
    let words = col()
        .gap(1.0)
        .min_w(0.0)
        .shrink()
        .child(
            text(if narrow {
                "As recorded".to_string()
            } else {
                "As recorded. Holdings and prices as they stood that day".to_string()
            })
            .font(k.ty.text_xsmall)
            .color(t.info)
            .truncate(),
        )
        .child(
            text(date)
                .key("time-band-date")
                .font(SECTION)
                .color(k.c.text_base)
                .truncate(),
        );
    Some(
        col()
            .key("time-band")
            .role("status")
            .w_full()
            .transition(ui::notice_in())
            .child(
                row()
                    .center_y()
                    .w_full()
                    .px(if narrow { 16.0 } else { 32.0 })
                    .py(8.0)
                    .gap(if narrow { 6.0 } else { 12.0 })
                    .child(icon(&icons::HISTORY, 18.0).color(t.info))
                    .child(words)
                    .child(el().grow(1.0))
                    .child(step(
                        &icons::CHEVRON_LEFT,
                        "A recorded day back",
                        "time-back",
                        -1,
                        first,
                    ))
                    .child(step(
                        &icons::CHEVRON_RIGHT,
                        "A recorded day forward",
                        "time-forward",
                        1,
                        false,
                    ))
                    .child(
                        Button::new(if narrow { "Today" } else { "Return to today" })
                            .variant(ButtonVariant::Outline)
                            .control_size(ControlSize::Small)
                            .shortcut(if narrow { "" } else { "Esc" })
                            .on_press(|s: &mut State| {
                                s.return_to_today();
                            })
                            .build(k)
                            .key("time-today"),
                    ),
            )
            // The line under it is the colour of information: this is the record, not now.
            .child(el().w_full().h(1.0).fill(t.info.alpha(0.55))),
    )
}

/// What a page about today shows in the past, instead of figures that would read as current.
pub fn now_only(s: &State, t: &Theme) -> Option<El> {
    if !s.in_past() {
        return None;
    }
    let words = time::now_only(s.page)?;
    let k = &t.kit;
    Some(
        ui::ledger(t)
            .key("time-now-only")
            .w_full()
            .max_w(640.0)
            .child(ui::title(format!("{} is about today", s.page.label()), t))
            .child(ui::muted(words, t).wrap())
            .child(
                row().center_y().mt(4.0).child(
                    Button::new("Return to today")
                        .variant(ButtonVariant::Submit)
                        .on_press(|s: &mut State| {
                            s.return_to_today();
                        })
                        .build(k)
                        .key("time-now-only-today"),
                ),
            ),
    )
}
