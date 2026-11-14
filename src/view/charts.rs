//! Charts that draw themselves in, and follow the pointer.

use std::f32::consts::{FRAC_PI_2, TAU};

use chrono::NaiveDate;
use rune::{
    Align, Color, Cx, Drag, El, Key, KeyEvent, Phase, Shape, Transition, Tx, canvas, col, el, row,
    text,
};
use rune_kit::RollingText;

use crate::{
    backend::HistoryValue,
    format,
    state::State,
    theme::{FIGURE, NUMBER, Theme},
    view::ui,
};

/// `2026-03-15` as `Mar 15`.
fn short_date(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_or_else(
        |_| date.to_string(),
        |date| date.format("%b %-d").to_string(),
    )
}

/// A holding's line over the period the page states. No line, and no placeholder, without two
/// points. The stroke draws in with the value chart and holds still under reduced motion.
pub fn sparkline(values: &[f64], width: f32, height: f32, color: Color, progress: f32) -> El {
    if values.len() < 2 || width < 4.0 || height < 4.0 {
        return el();
    }
    let inset = 1.0;
    let points: Vec<[f32; 2]> = plot(values, width - inset * 2.0, height - inset * 2.0)
        .into_iter()
        .map(|[x, y]| [x + inset, y + inset])
        .collect();
    let line = Shape::new().stroke(points, 1.25, color).drawn(progress);
    canvas(line).size(width, height).a11y_hidden()
}

/// Gain or loss over the series, or steel when it is flat. The shape is the data; the colour
/// says which way it went.
pub fn spark_color(values: &[f64], t: &Theme) -> Color {
    match (values.first(), values.last()) {
        (Some(first), Some(last)) if last > first => t.up,
        (Some(first), Some(last)) if last < first => t.down,
        _ => t.kit.c.text_muted,
    }
}

/// Where each value lands in a chart `width` by `height`.
fn plot(values: &[f64], width: f32, height: f32) -> Vec<[f32; 2]> {
    let (low, high) = values
        .iter()
        .fold((f64::MAX, f64::MIN), |(low, high), value| {
            (low.min(*value), high.max(*value))
        });
    // Zoom to the range: from zero, any real portfolio reads as a flat line.
    let margin = ((high - low) * 0.15).max(high.abs() * 0.001).max(0.01);
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

/// What an arbitrary span of the value chart honestly supports: the change in value, that change
/// as a percentage of the start, and how many days it covers. Time-weighted return and
/// contributions need each day's positions and the ledger, which this chart does not hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measured {
    pub start: usize,
    pub end: usize,
    pub days: i64,
    pub change: f64,
    pub percent: Option<f64>,
}

pub fn measure_span(dates: &[String], values: &[f64], a: usize, b: usize) -> Option<Measured> {
    if dates.len() != values.len() || values.len() < 2 {
        return None;
    }
    let last = values.len() - 1;
    let start = a.min(b).min(last);
    let end = a.max(b).min(last);
    if start == end {
        return None;
    }
    let from = NaiveDate::parse_from_str(&dates[start], "%Y-%m-%d").ok()?;
    let to = NaiveDate::parse_from_str(&dates[end], "%Y-%m-%d").ok()?;
    let days = (to - from).num_days();
    if days < 0 {
        return None;
    }
    let change = values[end] - values[start];
    let percent =
        (values[start] > 0.0 && values[start].is_finite()).then(|| change / values[start] * 100.0);
    Some(Measured {
        start,
        end,
        days,
        change,
        percent,
    })
}

const READOUT_W: f32 = 168.0;

/// The chart's box and its gain and loss colours, so a drag doesn't take eight arguments.
#[derive(Clone, Copy)]
struct Frame {
    width: f32,
    height: f32,
    up: Color,
    down: Color,
}

/// Publish the band and the readout. Live, so a drag can call it without rebuilding the view.
fn publish_span(
    s: &State,
    measured: &Measured,
    dates: &[String],
    plotted: &[[f32; 2]],
    frame: Frame,
) {
    let left = plotted[measured.start][0].min(plotted[measured.end][0]);
    let right = plotted[measured.start][0].max(plotted[measured.end][0]);
    s.measure
        .band
        .set([left, 0.0, (right - left).max(1.0), frame.height]);
    s.measure.shown.set(1.0);
    let pin = (left + (right - left) / 2.0 - READOUT_W / 2.0)
        .clamp(0.0, (frame.width - READOUT_W).max(0.0));
    s.measure.pin.set([pin, 0.0]);
    s.measure.ink.set(if measured.change >= 0.0 {
        frame.up
    } else {
        frame.down
    });
    s.measure.dates.set(format!(
        "{} – {}",
        short_date(&dates[measured.start]),
        short_date(&dates[measured.end])
    ));
    s.measure.change.set(if s.privacy {
        format::HIDDEN.to_string()
    } else {
        s.signed_money(measured.change)
    });
    let percent = measured
        .percent
        .map(format::percent)
        .unwrap_or_else(|| "-".to_string());
    let days = if measured.days == 1 {
        "1 day".to_string()
    } else {
        format!("{} days", measured.days)
    };
    s.measure.detail.set(format!("{percent} · {days}"));
}

/// One move of a measuring drag. `live_only` on the move, so the view is not rebuilt; the
/// press and the release commit, because they change what the view reads.
fn drag_span(
    s: &mut State,
    drag: &mut Drag,
    dates: &[String],
    values: &[f64],
    frame: Frame,
    tx: &mut Tx,
) {
    let plotted = plot(values, frame.width, frame.height);
    let Some(index) = nearest(&plotted, drag.x) else {
        return;
    };
    if drag.phase == Phase::Down {
        s.measure.dragging = true;
        s.chart_hover = None;
        s.chart_point = None;
        drag.data = index as f32;
    }
    let start = drag.data as usize;
    if drag.phase == Phase::Up {
        s.measure.dragging = false;
        if !drag.cancelled() && index != start {
            s.measure.span = Some((start.min(index), start.max(index)));
        }
        // The release commits: the view reads the kept span. Moves below do not.
        return;
    }
    if let Some(measured) = measure_span(dates, values, start, index) {
        publish_span(s, &measured, dates, &plotted, frame);
    }
    // Down changed the hover, which the view reads, so it has to commit. A move does not.
    if drag.phase == Phase::Move {
        tx.live_only();
    }
}

/// The nearest point to a pointer at `x`.
fn nearest(points: &[[f32; 2]], x: f32) -> Option<usize> {
    points
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| (a[0] - x).abs().total_cmp(&(b[0] - x).abs()))
        .map(|(index, _)| index)
}

/// A value-over-time chart, or a note until there are two days to compare.
pub fn value_chart(
    s: &State,
    points: &[HistoryValue],
    t: &Theme,
    cx: &Cx<State>,
    key: &str,
    height: f32,
) -> El {
    let k = &t.kit;
    if points.len() < 2 {
        return col()
            .key(key)
            .w_full()
            .h(height)
            .center()
            .gap(4.0)
            .child(ui::muted("Your history starts today", t))
            .px(16.0)
            .child(ui::caption(
                "Stahl records one valuation a day while you use it. Come back tomorrow for your first line.",
                t,
            )
            .wrap()
            .align(Align::Center));
    }
    let chart_height = height - 22.0;
    let values: Vec<f64> = points.iter().map(|point| point.value).collect();
    // Where each day sits across the chart, so the pointer knows which it's nearest.
    let days: Vec<[f32; 2]> = cx
        .layout_size(key)
        .map(|[width, _]| plot(&values, width, chart_height))
        .unwrap_or_default();
    let holder = el()
        .key(key)
        .role("img")
        .label("Portfolio value over time. Drag across it to measure a span.")
        .focusable(true)
        .w_full()
        .h(chart_height)
        .on_pointer_move_local(move |s: &mut State, event, tx: &mut Tx| {
            // A measuring drag owns the pointer. Hover would rebuild the view every move.
            if s.measure.dragging {
                tx.live_only();
                return;
            }
            s.chart_hover = Some(event.x);
            s.chart_point = nearest(&days, event.x);
        })
        .on_hover_change(|s: &mut State, over, _| {
            if !over && !s.measure.dragging {
                s.chart_hover = None;
                s.chart_point = None;
            }
        });
    let dates = |count: usize| -> Vec<El> {
        let step = (points.len() - 1) as f32 / (count - 1) as f32;
        (0..count)
            .map(|index| {
                let point = &points[(index as f32 * step).round() as usize];
                ui::caption(short_date(&point.date), t)
            })
            .collect()
    };
    let axis = row()
        .center_y()
        .w_full()
        .justify_between()
        .pt(6.0)
        .children(dates(5));
    let Some([width, _]) = cx.layout_size(key) else {
        return col().w_full().child(holder).child(axis);
    };

    let plotted = plot(&values, width, chart_height);
    let progress = s.draw_progress(cx.is_reduced_motion());
    let trend = t.change(values[values.len() - 1] - values[0]);

    let hovered = s.chart_hover.and_then(|x| nearest(&plotted, x));
    let mut grid = Shape::new();
    for line in 1..4 {
        let y = chart_height * line as f32 / 4.0;
        grid = grid.stroke(vec![[0.0, y], [width, y]], 1.0, k.c.text_base.alpha(0.05));
    }
    let mut area = Shape::new();
    for band in area_bands(&plotted, chart_height) {
        area = area.fill(band.points, trend.alpha(band.alpha));
    }
    let glow = Shape::new()
        .stroke(plotted.clone(), 4.0, trend.alpha(0.22))
        .drawn(progress);
    let mut line = Shape::new()
        .stroke(plotted.clone(), 1.5, trend)
        .drawn(progress);
    if let Some(index) = hovered {
        let [x, y] = plotted[index];
        line = line
            .stroke(vec![[x, 0.0], [x, chart_height]], 1.0, k.c.border_strong)
            .fill(circle([x, y], 4.5), trend)
            .fill(circle([x, y], 2.0), t.card);
    }
    let [last_x, last_y] = plotted[plotted.len() - 1];
    // The live end: a soft glow in the gain or loss colour, breathing as the live dot does.
    // While the line draws in, the same light sits at its head and runs along it, once.
    let live = {
        let head = along(&plotted, progress);
        if progress < 1.0 {
            ui::glow_orb(trend, 26.0, 0.7).absolute(head[0] - 13.0, head[1] - 13.0)
        } else {
            ui::glow_orb(trend, 34.0, 0.42)
                .absolute(head[0] - 17.0, head[1] - 17.0)
                .cycle(ui::BREATHE)
        }
    };
    // Where the line has got to: today, once it has finished drawing.
    let now = (progress >= 1.0).then(|| {
        el().key(format!("{key}-now"))
            .absolute(last_x - 8.0, last_y - 8.0)
            .size(16.0, 16.0)
            .center()
            .a11y_hidden()
            .transition(Transition::scale(0.2).combined(Transition::opacity()))
            .child(el().size(6.0, 6.0).radius(3.0).fill(trend))
    });
    // A kept span, or one being dragged, replaces the hover readout.
    let readout = hovered
        .filter(|_| s.measure.span.is_none() && !s.measure.dragging)
        .map(|index| {
            let point = &points[index];
            let [x, _] = plotted[index];
            let value = if s.privacy {
                format::HIDDEN.to_string()
            } else {
                s.money(point.value)
            };
            ui::glass(s, t)
                .key("chart-readout")
                .pin(
                    Some((x + 10.0).min(width - 124.0).max(0.0)),
                    Some(4.0),
                    None,
                    None,
                )
                .w(120.0)
                .px(10.0)
                .py(6.0)
                .radius(8.0)
                .child(ui::caption(short_date(&point.date), t))
                .child(text(value).font(NUMBER).color(k.c.text_base))
        });
    let point_dates: Vec<String> = points.iter().map(|point| point.date.clone()).collect();
    let frame = Frame {
        width,
        height: chart_height,
        up: t.up,
        down: t.down,
    };
    if !s.measure.dragging
        && let Some((start, end)) = s.measure.span
    {
        if let Some(measured) = measure_span(&point_dates, &values, start, end) {
            publish_span(s, &measured, &point_dates, &plotted, frame);
        } else {
            s.measure.shown.set(0.0);
        }
    }
    let drag_dates = point_dates.clone();
    let drag_values = values.clone();
    // A finger drag on the chart itself would steal scrolling, pull-to-refresh and the edge
    // swipe. On a phone the rail below takes the drag; the chart stays a vertical pan.
    let holder = if s.compact {
        holder
    } else {
        let keys_dates = point_dates.clone();
        let keys_values = values.clone();
        holder
            .on_drag(move |s: &mut State, drag: &mut Drag, tx: &mut Tx| {
                drag_span(s, drag, &drag_dates, &drag_values, frame, tx);
            })
            .on_key(move |s: &mut State, event: &KeyEvent, _: &mut Tx| {
                if event.mods.ctrl || event.mods.alt || event.mods.cmd {
                    return false;
                }
                if event.key == Key::Escape {
                    s.measure.clear();
                    return true;
                }
                let step = match event.key {
                    Key::Left => -1,
                    Key::Right => 1,
                    _ => return false,
                };
                let last = keys_values.len().saturating_sub(1);
                let current = s.chart_point.unwrap_or(last);
                let next = (current as i32 + step).clamp(0, last as i32) as usize;
                if event.mods.shift {
                    let anchor = s.measure.span.map(|(start, _)| start).unwrap_or(current);
                    s.measure.span = Some((anchor.min(next), anchor.max(next)));
                    if let Some(measured) = measure_span(&keys_dates, &keys_values, anchor, next) {
                        let plotted = plot(&keys_values, width, chart_height);
                        publish_span(s, &measured, &keys_dates, &plotted, frame);
                    }
                } else {
                    s.chart_point = Some(next);
                    s.chart_hover = plot(&keys_values, width, chart_height)
                        .get(next)
                        .map(|point| point[0]);
                }
                true
            })
    };
    let rail = s.compact.then(|| {
        let rail_dates = point_dates.clone();
        let rail_values = values.clone();
        col()
            .key("chart-rail")
            .role("slider")
            .label("Drag to measure a span")
            .w_full()
            .h(44.0)
            .gap(6.0)
            .justify_center()
            .on_drag(move |s: &mut State, drag: &mut Drag, tx: &mut Tx| {
                drag_span(s, drag, &rail_dates, &rail_values, frame, tx);
            })
            .child(ui::caption("Drag to measure", t))
            .child(
                el().w_full()
                    .h(4.0)
                    .radius(2.0)
                    .fill(k.c.border_strong)
                    .a11y_hidden(),
            )
    });
    let band = el()
        .key("chart-band")
        .absolute(0.0, 0.0)
        .size(1.0, chart_height)
        .live_rect(&s.measure.band)
        .live_opacity(&s.measure.shown)
        .fill(trend.alpha(0.16))
        .border(1.0, trend.alpha(0.7))
        .a11y_hidden();
    let measure = ui::glass(s, t)
        .key("chart-measure")
        .absolute(0.0, 8.0)
        .live_offset(&s.measure.pin)
        .live_opacity(&s.measure.shown)
        .w(READOUT_W)
        .px(10.0)
        .py(8.0)
        .radius(8.0)
        .gap(2.0)
        .child(
            text("Nov 00 – Nov 00")
                .live_text(&s.measure.dates)
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted),
        )
        .child(
            text("+€000,000.00")
                .live_text(&s.measure.change)
                .font(NUMBER)
                .live_text_color(&s.measure.ink),
        )
        .child(
            text("+00.00% · 000 days")
                .live_text(&s.measure.detail)
                .font(k.ty.text_xsmall)
                .color(k.c.text_muted),
        );
    col()
        .w_full()
        .child(
            holder
                .child(canvas(grid).absolute(0.0, 0.0).size(width, chart_height))
                .child(
                    canvas(area)
                        .absolute(0.0, 0.0)
                        .size(width, chart_height)
                        // Melts the seams between the bands.
                        .blur(2.0)
                        .opacity(progress),
                )
                .child(
                    canvas(glow)
                        .absolute(0.0, 0.0)
                        .size(width, chart_height)
                        .blur(5.0),
                )
                .child(canvas(line).absolute(0.0, 0.0).size(width, chart_height))
                .child(band)
                .children(now)
                .child(live)
                .children(readout)
                .child(measure),
        )
        .child(axis)
        .children(rail)
}

struct AreaBand {
    points: Vec<[f32; 2]>,
    alpha: f32,
}

/// The area under a line, cut into horizontal bands that fade towards the bottom. A real gradient
/// that follows the line, which also shows whatever is behind the card through it.
fn area_bands(line: &[[f32; 2]], height: f32) -> Vec<AreaBand> {
    const BANDS: usize = 28;
    const TOP_ALPHA: f32 = 0.16;
    let top = line.iter().fold(height, |top, point| top.min(point[1]));
    let step = (height - top) / BANDS as f32;
    (0..BANDS)
        .map(|band| {
            let upper = top + band as f32 * step;
            let lower = upper + step;
            let mut points: Vec<[f32; 2]> = line
                .iter()
                .map(|point| [point[0], point[1].clamp(upper, lower)])
                .collect();
            if let (Some(first), Some(last)) = (line.first(), line.last()) {
                points.push([last[0], lower]);
                points.push([first[0], lower]);
            }
            let fade = 1.0 - band as f32 / BANDS as f32;
            AreaBand {
                points,
                alpha: TOP_ALPHA * fade * fade,
            }
        })
        .collect()
}

/// The point `t` of the way along a polyline, from 0 at its start to 1 at its end.
fn along(points: &[[f32; 2]], t: f32) -> [f32; 2] {
    let Some(last) = points.len().checked_sub(1) else {
        return [0.0, 0.0];
    };
    let at = t.clamp(0.0, 1.0) * last as f32;
    let index = (at.floor() as usize).min(last);
    let part = at - index as f32;
    let [ax, ay] = points[index];
    let [bx, by] = points[(index + 1).min(last)];
    [ax + (bx - ax) * part, ay + (by - ay) * part]
}

fn circle(center: [f32; 2], radius: f32) -> Vec<[f32; 2]> {
    (0..24)
        .map(|index| {
            let angle = index as f32 / 24.0 * TAU;
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ]
        })
        .collect()
}

/// A ring segment from `start` to `end`, in radians.
fn ring_segment(center: f32, inner: f32, outer: f32, start: f32, end: f32) -> Vec<[f32; 2]> {
    let steps = (((end - start) / TAU * 96.0).ceil() as usize).max(2);
    let at =
        |radius: f32, angle: f32| [center + radius * angle.cos(), center + radius * angle.sin()];
    let forward = (0..=steps).map(|i| at(outer, start + (end - start) * i as f32 / steps as f32));
    let back = (0..=steps)
        .rev()
        .map(|i| at(inner, start + (end - start) * i as f32 / steps as f32));
    forward.chain(back).collect()
}

/// Which slice a point lies on, if any. Slices start at the top and run clockwise.
fn slice_at(shares: &[f64], x: f32, y: f32, center: f32, inner: f32, outer: f32) -> Option<usize> {
    let (dx, dy) = (x - center, y - center);
    let distance = (dx * dx + dy * dy).sqrt();
    if distance < inner || distance > outer {
        return None;
    }
    let turned = (dy.atan2(dx) + FRAC_PI_2).rem_euclid(TAU) / TAU * 100.0;
    let mut sum = 0.0;
    for (index, share) in shares.iter().enumerate() {
        sum += *share as f32;
        if turned <= sum {
            return Some(index);
        }
    }
    None
}

const DONUT: f32 = 288.0;
const INNER: f32 = 92.0;
const OUTER: f32 = 124.0;
const TARGETS: f32 = 137.0;

/// The allocation ring. The hovered slice grows, and the centre reads it out.
pub fn donut(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    let progress = s.draw_progress(cx.is_reduced_motion());
    let center = DONUT / 2.0;
    let gap = 0.025;
    let mut shape = Shape::new();
    let mut glow = Shape::new();
    let mut angle = -FRAC_PI_2;
    for (index, item) in s.allocation.iter().enumerate() {
        let sweep = (item.percent as f32 / 100.0) * TAU * progress;
        if sweep > gap * 2.0 {
            let grown = if s.donut_hover == Some(index) {
                8.0
            } else {
                0.0
            };
            let segment = ring_segment(
                center,
                INNER,
                OUTER + grown,
                angle + gap,
                angle + sweep - gap,
            );
            let color = t.class_color(&item.asset_class);
            glow = glow.fill(segment.clone(), color.alpha(0.5));
            shape = shape.fill(segment, color);
        }
        angle += sweep;
    }
    // A thin outer ring shows the shares your policy asks for, against the ones you have.
    let targets = s.review.data.as_ref().filter(|_| !s.allocation.is_empty());
    if let Some(review) = targets {
        let mut angle = -FRAC_PI_2;
        for allocation in &review.allocations {
            let sweep = (allocation.target_percent as f32 / 100.0) * TAU * progress;
            if sweep > gap * 2.0 {
                shape = shape.fill(
                    ring_segment(
                        center,
                        TARGETS,
                        TARGETS + 3.0,
                        angle + gap,
                        angle + sweep - gap,
                    ),
                    t.class_color(&allocation.asset_class).alpha(0.75),
                );
            }
            angle += sweep;
        }
    }
    let hovered = s.donut_hover.and_then(|index| s.allocation.get(index));
    let (label, value, share) = match hovered {
        Some(item) => (
            item.asset_class.clone(),
            s.money(item.value),
            format!("{:.1}%", item.percent),
        ),
        None => (
            "Total".to_string(),
            s.money(
                s.summary
                    .as_ref()
                    .map_or(0.0, |summary| summary.total_value),
            ),
            String::new(),
        ),
    };
    col()
        .key("donut")
        .role("img")
        .label("Allocation by asset class")
        .size(DONUT, DONUT)
        .self_center()
        .on_pointer_move_local(|s: &mut State, event, _| {
            let shares: Vec<f64> = s.allocation.iter().map(|item| item.percent).collect();
            s.donut_hover = slice_at(&shares, event.x, event.y, DONUT / 2.0, INNER, OUTER + 8.0);
        })
        .on_hover_change(|s: &mut State, over, _| {
            if !over {
                s.donut_hover = None;
            }
        })
        .child(
            canvas(glow)
                .absolute(0.0, 0.0)
                .size(DONUT, DONUT)
                .blur(12.0)
                .opacity(0.5),
        )
        .child(canvas(shape).absolute(0.0, 0.0).size(DONUT, DONUT))
        .child(
            col()
                .absolute(0.0, 0.0)
                .size(DONUT, DONUT)
                .center()
                .gap(2.0)
                .child(ui::caption(label, t))
                .child(
                    RollingText::new("donut-value", value)
                        .font(FIGURE)
                        .color(k.c.text_base)
                        .build(k),
                )
                .children((!share.is_empty()).then(|| ui::caption(share, t))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plotting_zooms_to_the_range_and_spans_the_width() {
        let points = plot(&[100_000.0, 110_000.0], 200.0, 100.0);
        assert_eq!(points[0][0], 0.0);
        assert_eq!(points[1][0], 200.0);
        // A 10% rise climbs most of the chart, instead of looking flat.
        assert!(points[0][1] - points[1][1] > 70.0);
    }

    #[test]
    fn a_flat_history_still_plots_finite_points() {
        for point in plot(&[0.0, 0.0, 0.0], 100.0, 50.0) {
            assert!(point[1].is_finite());
        }
    }

    #[test]
    fn area_bands_stay_under_the_line_and_fade_downwards() {
        let line = [[0.0, 80.0], [50.0, 20.0], [100.0, 50.0]];
        let bands = area_bands(&line, 100.0);
        assert!(bands.windows(2).all(|pair| pair[0].alpha > pair[1].alpha));
        for band in &bands {
            // Where the line is below this band, the band collapses onto its own lower edge.
            let lower = band.points[band.points.len() - 1][1];
            for (point, source) in band.points.iter().zip(&line) {
                assert!(
                    point[1] >= source[1] || point[1] == lower,
                    "{point:?} rises above {source:?}"
                );
            }
            assert!(band.points.iter().all(|point| point[1] <= 100.0));
        }
    }

    #[test]
    fn a_span_is_the_change_in_value_over_its_days() {
        let dates = ["2026-03-01", "2026-03-02", "2026-03-10"]
            .map(str::to_string)
            .to_vec();
        let values = [100.0, 110.0, 80.0];
        let span = measure_span(&dates, &values, 2, 0).unwrap();
        assert_eq!((span.start, span.end, span.days), (0, 2, 9));
        assert_eq!(span.change, -20.0);
        assert_eq!(span.percent, Some(-20.0));
        assert!(
            measure_span(&dates, &values, 1, 1).is_none(),
            "a point is not a span"
        );
        let flat = measure_span(&dates[..2], &[0.0, 10.0], 0, 1).unwrap();
        assert!(flat.percent.is_none(), "no percentage from a zero start");
    }

    #[test]
    fn nearest_point_follows_the_pointer() {
        let points = [[0.0, 0.0], [50.0, 0.0], [100.0, 0.0]];
        assert_eq!(nearest(&points, 60.0), Some(1));
        assert_eq!(nearest(&points, 99.0), Some(2));
        assert_eq!(nearest(&[], 1.0), None);
    }

    #[test]
    fn slices_run_clockwise_from_the_top() {
        let shares = [25.0, 25.0, 50.0];
        let (center, inner, outer) = (100.0, 40.0, 80.0);
        // A quarter turn right of the top is the end of the first quarter, so just past 3 o'clock
        // is the second slice; the left half is the third.
        assert_eq!(
            slice_at(&shares, 160.0, 90.0, center, inner, outer),
            Some(0)
        );
        assert_eq!(
            slice_at(&shares, 160.0, 110.0, center, inner, outer),
            Some(1)
        );
        assert_eq!(
            slice_at(&shares, 40.0, 100.0, center, inner, outer),
            Some(2)
        );
    }

    #[test]
    fn the_hole_and_the_outside_are_no_slice() {
        let shares = [100.0];
        assert_eq!(slice_at(&shares, 100.0, 100.0, 100.0, 40.0, 80.0), None);
        assert_eq!(slice_at(&shares, 199.0, 100.0, 100.0, 40.0, 80.0), None);
    }
}
