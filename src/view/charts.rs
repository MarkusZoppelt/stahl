//! Charts that draw themselves in, and follow the pointer.

use std::f32::consts::{FRAC_PI_2, TAU};

use chrono::NaiveDate;
use rune::{Cx, El, Shape, Transition, canvas, col, el, row, text};
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
    key: &'static str,
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
            .child(ui::caption(
                "Stahl records one valuation a day while you use it. Come back tomorrow for your first line.",
                t,
            ));
    }
    let chart_height = height - 22.0;
    let holder = el()
        .key(key)
        .role("img")
        .label("Portfolio value over time")
        .w_full()
        .h(chart_height)
        .on_pointer_move_local(|s: &mut State, event, _| s.chart_hover = Some(event.x))
        .on_hover_change(|s: &mut State, over, _| {
            if !over {
                s.chart_hover = None;
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

    let values: Vec<f64> = points.iter().map(|point| point.value).collect();
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
    // Where the line has got to: today, once it has finished drawing.
    let now = (progress >= 1.0).then(|| {
        el().key(format!("{key}-now"))
            .absolute(last_x - 8.0, last_y - 8.0)
            .size(16.0, 16.0)
            .center()
            .a11y_hidden()
            .transition(Transition::scale(0.2).combined(Transition::opacity()))
            .child(
                el().absolute(0.0, 0.0)
                    .size(16.0, 16.0)
                    .radius(8.0)
                    .fill(trend.alpha(0.2))
                    .cycle(ui::BREATHE),
            )
            .child(el().size(6.0, 6.0).radius(3.0).fill(trend))
    });
    let readout = hovered.map(|index| {
        let point = &points[index];
        let [x, _] = plotted[index];
        let value = if s.privacy {
            format::HIDDEN.to_string()
        } else {
            s.money(point.value)
        };
        ui::floating(t)
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
                .children(now)
                .children(readout),
        )
        .child(axis)
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
