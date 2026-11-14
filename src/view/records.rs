//! The records room: the wall of plates, and the moment one of them is opened to.
//!
//! A plate is a small engraved steel plaque: its date, what kind of record it is, a title in the
//! serif, the figure, and one sentence. The newest plates sit forward and the older ones recede.
//! Opening one lifts it off the wall and sets the value chart under it, with its day marked.

use chrono::NaiveDate;
use rune::{Align, Cx, El, col, color::srgb_encode, el, icon, row, text};
use rune_kit::{Button, ButtonVariant};

use crate::{
    backend::HistoryValue,
    format,
    fx::Plate,
    icons,
    records::Record,
    state::State,
    theme::{FIGURE, HOLDING_TITLE, LEDE, SECTION, Theme},
    view::{charts, ui},
};

/// Plates this close to the newest one sit forward on the wall; older ones recede.
const FRESH_DAYS: i64 = 30;
/// How many plates the arrival animation staggers before it stops waiting.
const STAGGERED: usize = 8;
/// The height of the value chart on an opened plate.
const CHART_HEIGHT: f32 = 240.0;
/// What the chart leaves under its line for the dates.
const AXIS: f32 = 22.0;
/// How wide the opened plate and its chart are allowed to be.
const MOMENT_MAX: f32 = 880.0;

pub fn page(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let open = s.record_plate.as_deref().and_then(|id| {
        s.records
            .data
            .as_ref()
            .and_then(|plates| plates.iter().find(|plate| plate.id == id))
    });
    match open {
        Some(record) => moment(s, record, t, cx),
        None => wall(s, t, cx),
    }
}

/// Whether the open portfolio's values must not be written beside it in plain text.
fn encrypted(s: &State) -> bool {
    s.open
        .file_path
        .as_deref()
        .is_some_and(|path| path.ends_with(".gpg"))
}

fn wall(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    if s.positions.is_empty() {
        return ui::empty_state(
            "records-empty",
            &icons::LANDMARK,
            "The room is empty",
            "A plate is engraved for each thing that truly happens to this portfolio. Add a \
             position, and the wall begins.",
            ui::add_position_button("records-empty-add", t),
            t,
        );
    }
    if let Some(error) = &s.records.error {
        return ui::danger(error.clone(), t);
    }
    let Some(plates) = &s.records.data else {
        return match ui::status(&s.records, "Reading the records…", t) {
            Some(status) => status,
            None => col(),
        };
    };
    if plates.is_empty() {
        return empty_room(t, encrypted(s));
    }
    let count = match plates.len() {
        1 => "One record".to_string(),
        count => format!("{count} records"),
    };
    let mut page = col().w_full().gap(16.0).child(
        row()
            .center_y()
            .w_full()
            .justify_between()
            .gap(12.0)
            .child(ui::caption(format!("{count}, newest first"), t))
            // A phone has no room for a hint beside the count, and doesn't need one.
            .children(
                (ui::room(s, cx) != ui::Room::Narrow)
                    .then(|| ui::caption("Select a plate to see its moment", t)),
            ),
    );
    let room = ui::room(s, cx);
    let newest = plates.first().map(|plate| plate.date);
    let columns = ui::columns(room, [2, 1, 1], 12.0);
    page = page.child(
        columns.children(
            plates
                .iter()
                .enumerate()
                .map(|(index, record)| plate(s, record, index, newest.unwrap_or(record.date), t)),
        ),
    );
    page
}

/// A plate on the wall: the same thing whether it is on the wall or lifted off it.
fn plate(s: &State, record: &Record, index: usize, newest: NaiveDate, t: &Theme) -> El {
    let k = &t.kit;
    let fresh = (newest - record.date).num_days() <= FRESH_DAYS;
    let arriving = s
        .record_arrival
        .as_ref()
        .is_some_and(|(id, _)| id == &record.id);
    let id = record.id.clone();
    let words = record.sentence(s.privacy, &s.currency());
    let label = format!("{}, {}", record.title(), words);
    // A plate is steel, the same metal in either theme, so it is inked the same in both. A gain
    // leans the steel teal, a fall leans it red; the words stay in the one dark ink.
    let (ink, lip) = ui::steel_ink();
    let body = col()
        .w_full()
        .min_w(0.0)
        .gap(8.0)
        .padding(16.0)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_between()
                .gap(8.0)
                .child(
                    text(record.date.format("%-d %B %Y").to_string())
                        .font(k.ty.text_xsmall)
                        .color(ink),
                )
                .child(
                    text(record.kind.caption())
                        .font(k.ty.text_xsmall)
                        .color(ink),
                ),
        )
        .child(ui::engraved(record.title(), SECTION, ink, lip))
        .children(figure(s, record, t))
        .child(text(words).font(k.ty.body).color(ink).wrap());
    let steel = steel_of(record, fresh, t).sweep(
        s.record_arrival
            .as_ref()
            .filter(|(id, _)| id == &record.id)
            .map(|(_, started)| *started),
    );
    let card = ui::plate(s, t, steel, 3.0)
        .key(format!("plate-{}", record.id))
        .role("button")
        .label(label)
        .focusable(true)
        .w_full()
        .press_scale(ui::PRESS_CARD)
        .on_press(move |s: &mut State| s.open_record(id.clone()))
        .child(body);
    if arriving {
        let card = card.transition(ui::settle());
        // Without a compositor the band can't sweep; a pale light falls over the plate instead.
        if s.shaders {
            card
        } else {
            card.child(ui::catch_light(format!("plate-{}", record.id), t, 3.0))
        }
    } else {
        ui::entering(card, index.min(STAGGERED))
    }
}

/// The steel a plate is made of: pale in either theme, leaning to the change it records, and
/// duller when it is old, so it gives back less of the light.
fn steel_of(record: &Record, fresh: bool, t: &Theme) -> Plate {
    let change = record.tone();
    let fall = change.is_some_and(|change| change < 0.0);
    // The wall is the dimmest light in the room, so in the dark the plates sit a step down. A
    // fall is darker and cooler, a slate with no red in it.
    let tone = if fresh { 0.88 } else { 0.82 } + if t.dark { -0.16 } else { 0.08 };
    let tone = if fall { tone - 0.05 } else { tone };
    let steel = Plate::new(tone).gain(if fresh { 1.0 } else { 0.45 });
    match change {
        Some(_) if fall => steel.tint([0.50, 0.57, 0.66], 0.22),
        Some(change) => {
            let c = t.change(change);
            steel.tint(
                [c.r, c.g, c.b].map(srgb_encode),
                if t.dark { 0.10 } else { 0.07 },
            )
        }
        None => steel,
    }
}

/// The figure under a plate's title, with the percentage a change carries, cut into the steel.
fn figure(s: &State, record: &Record, t: &Theme) -> Option<El> {
    let k = &t.kit;
    let figure = record.figure_text(s.privacy, &s.currency())?;
    let (ink, lip) = ui::steel_ink();
    let percent = record.percent.map(|percent| {
        text(format!("({})", format::percent(percent)))
            .font(k.ty.text_small_medium)
            .color(ink)
    });
    Some(
        row()
            .center_y()
            .w_full()
            .gap(8.0)
            .child(ui::engraved(figure, FIGURE, ink, lip))
            .children(percent),
    )
}

/// The room with nothing on its wall yet, and what will be.
fn empty_room(t: &Theme, encrypted: bool) -> El {
    let (heading, detail) = if encrypted {
        (
            "An encrypted portfolio keeps no records",
            "A plate's date and kind are written beside the history, and that file is not \
             encrypted. Nothing about this portfolio is written in plain text.",
        )
    } else {
        (
            "The room is quiet for now",
            "A plate is engraved when something real happens: a new high, a round number \
             crossed, a year on record, a fall and the day it came back. Open Stahl now and \
             then, and the wall grows from here.",
        )
    };
    col()
        .w_full()
        .items_center()
        .gap(10.0)
        .py(48.0)
        .px(16.0)
        .child(
            col()
                .size(40.0, 40.0)
                .mb(4.0)
                .radius(8.0)
                .continuous_corners()
                .center()
                .fill(t.kit.c.layer_03)
                .child(icon(&icons::LANDMARK, 20.0).color(t.kit.c.text_base)),
        )
        .child(
            text(heading)
                .font(LEDE)
                .color(t.kit.c.text_base)
                .align(Align::Center)
                .wrap(),
        )
        .child(
            col()
                .w_full()
                .max_w(480.0)
                .items_center()
                .child(ui::muted(detail, t).wrap().align(Align::Center)),
        )
}

/// One plate lifted off the wall, with the portfolio's line under it and the day marked.
fn moment(s: &State, record: &Record, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    let points = s
        .history
        .as_ref()
        .map_or(&[][..], |history| history.points.as_slice());
    let day = record.date.format("%Y-%m-%d").to_string();
    let marked = points.iter().position(|point| point.date == day);
    let words = record.sentence(s.privacy, &s.currency());
    let figure = record
        .figure_text(s.privacy, &s.currency())
        .unwrap_or_default();
    // The plate is the one on the wall, only larger: the same steel, the same ink.
    let fresh = s
        .records
        .data
        .as_ref()
        .and_then(|plates| plates.first())
        .is_some_and(|newest| (newest.date - record.date).num_days() <= FRESH_DAYS);
    let (ink, lip) = ui::steel_ink();
    let first = points.first().map(|point| point.date.clone());
    col()
        .w_full()
        .gap(20.0)
        .child(
            row()
                .center_y()
                .w_full()
                .max_w(MOMENT_MAX)
                .justify_between()
                .gap(12.0)
                .child(
                    Button::new("The wall")
                        .variant(ButtonVariant::Ghost)
                        .icon(&icons::CHEVRON_LEFT)
                        .on_press(|s: &mut State| s.close_record())
                        .build(k)
                        .key("record-back"),
                )
                .child(ui::caption(record.kind.caption(), t)),
        )
        .child(
            col()
                .w_full()
                .max_w(MOMENT_MAX)
                .gap(24.0)
                // The plate itself, larger than it is on the wall.
                .child(
                    ui::plate(s, t, steel_of(record, fresh, t), 3.0)
                        .w_full()
                        .gap(10.0)
                        .padding(24.0)
                        .child(
                            text(record.date.format("%-d %B %Y").to_string())
                                .font(k.ty.text_xsmall)
                                .color(ink),
                        )
                        .child(ui::engraved(record.title(), HOLDING_TITLE, ink, lip))
                        .child(
                            row()
                                .center_y()
                                .w_full()
                                .gap(8.0)
                                .child(ui::engraved(figure, FIGURE, ink, lip))
                                .children(record.percent.map(|percent| {
                                    text(format!("({})", format::percent(percent)))
                                        .font(k.ty.text_small_medium)
                                        .color(ink)
                                })),
                        )
                        .child(text(words).font(k.ty.body).color(ink).wrap()),
                )
                .children((marked.is_none()).then(|| {
                    ui::notice(
                        match first {
                            Some(first) => format!(
                                "This plate is older than Stahl's history, which starts on {}.",
                                long_day(&first)
                            ),
                            None => {
                                "Stahl has no valuation history for this portfolio yet.".to_string()
                            }
                        },
                        t,
                    )
                }))
                // The chart and its mark share a box, so the mark starts at the chart's top
                // edge and not at the plate's.
                .child(
                    el().w_full()
                        .min_w(0.0)
                        .child(charts::value_chart(
                            s,
                            points,
                            t,
                            cx,
                            "record-chart",
                            CHART_HEIGHT,
                        ))
                        .children(marker(s, record, points, marked, t, cx)),
                )
                .children(marked.map(|index| {
                    let then = points[index].value;
                    let now = points.last().map_or(then, |point| point.value);
                    let mut figures = row().w_full().gap(32.0).child(ui::stat(
                        "That day",
                        s.money(then),
                        None,
                        t,
                    ));
                    if index + 1 < points.len() {
                        let change = now - then;
                        figures = figures.child(ui::stat(
                            "Since then",
                            s.signed_money(change),
                            Some(t.change(change)),
                            t,
                        ));
                    }
                    figures
                })),
        )
}

/// The day marked on the value chart, drawn from the same geometry the chart plots with.
fn marker(
    s: &State,
    record: &Record,
    points: &[HistoryValue],
    marked: Option<usize>,
    t: &Theme,
    cx: &Cx<State>,
) -> Option<El> {
    let index = marked?;
    let [width, _] = cx.layout_size("record-chart")?;
    let values: Vec<f64> = points.iter().map(|point| point.value).collect();
    let [x, y] = plotted(&values, width, CHART_HEIGHT - AXIS, index)?;
    let trend = t.change(values[values.len() - 1] - values[0]);
    Some(
        el().key("record-marker")
            .absolute(x - 0.5, 0.0)
            .w(1.0)
            .h(CHART_HEIGHT - AXIS)
            .fill(t.kit.c.border_strong)
            .a11y_hidden()
            .child(
                el().key("record-marker-dot")
                    .absolute(-3.0, y - 3.5)
                    .size(7.0, 7.0)
                    .radius(3.0)
                    .fill(trend)
                    .border(1.5, t.card),
            )
            .help(format!(
                "{}: {}",
                long_day(&record.date.format("%Y-%m-%d").to_string()),
                s.money(values[index])
            )),
    )
}

/// "11 November 2026", from the date as the history stores it.
fn long_day(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_or_else(
        |_| date.to_string(),
        |date| date.format("%-d %B %Y").to_string(),
    )
}

/// Where a value lands in a chart `width` by `height`, exactly as [`charts::value_chart`] plots
/// it, so a plate can put its mark on the line. Kept in step with `charts::plot`, which is
/// private to that module; see the note in the report.
fn plotted(values: &[f64], width: f32, height: f32, index: usize) -> Option<[f32; 2]> {
    if values.len() < 2 || index >= values.len() || width < 4.0 || height < 4.0 {
        return None;
    }
    let (low, high) = values
        .iter()
        .fold((f64::MAX, f64::MIN), |(low, high), value| {
            (low.min(*value), high.max(*value))
        });
    let margin = ((high - low) * 0.15).max(high.abs() * 0.001).max(0.01);
    let (low, high) = (low - margin, high + margin);
    let last = (values.len() - 1).max(1) as f32;
    Some([
        index as f32 / last * width,
        height - ((values[index] - low) / (high - low)) as f32 * height,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RECORD_ARRIVAL_FOR;

    #[test]
    fn a_mark_sits_where_the_chart_plots_the_day() {
        let values = [100_000.0, 110_000.0];
        let [x, y] = plotted(&values, 200.0, 100.0, 0).unwrap();
        assert_eq!(x, 0.0);
        assert!(y > 80.0, "the first value is at the bottom: {y}");
        let [x, y] = plotted(&values, 200.0, 100.0, 1).unwrap();
        assert_eq!(x, 200.0);
        assert!(y < 20.0, "the last is near the top: {y}");
        assert!(plotted(&values, 200.0, 100.0, 2).is_none());
        assert!(plotted(&[1.0], 200.0, 100.0, 0).is_none());
    }

    #[test]
    fn a_day_reads_as_the_plates_write_it() {
        assert_eq!(long_day("2026-11-11"), "11 November 2026");
        assert_eq!(long_day("not a date"), "not a date");
    }

    #[test]
    fn the_light_plays_once_and_holds_still() {
        // The sequence ends on its last pose, so nothing loops for attention.
        assert_eq!(ui::CATCH[ui::CATCH.len() - 1].opacity, 0.0);
        assert!(ui::CATCH.windows(2).all(|pair| pair[1].at > pair[0].at));
        assert!(ui::CATCH[ui::CATCH.len() - 1].at < RECORD_ARRIVAL_FOR);
    }
}
