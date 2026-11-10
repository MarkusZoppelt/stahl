//! A squarified treemap of the holdings. Area is value. The layout is a pure function.

use rune::{Color, El, Key, KeyEvent, Tx, col, el, text, tok::motion::LEAD};

use crate::{
    format,
    state::{Page, State},
    theme::{NUMBER, Theme},
    view::{portfolio, ui},
};

/// A tile's rectangle, origin at the top left of the map.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn area(self) -> f32 {
        (self.w * self.h).max(0.0)
    }

    #[cfg(test)]
    fn contains(self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// Lay `weights` into `width` by `height`. Tiles never overlap, they fill the rectangle, and a
/// holding too small to read is given at least `min_side` rather than a sliver beside a huge one.
///
/// A non-positive weight is treated as nothing, and still gets a tile when it would otherwise
/// vanish. The result is in the same order as `weights`.
pub fn layout(weights: &[f64], width: f32, height: f32, min_side: f32) -> Vec<Rect> {
    let n = weights.len();
    let empty = Rect {
        x: 0.0,
        y: 0.0,
        w: 0.0,
        h: 0.0,
    };
    let mut out = vec![empty; n];
    if n == 0 || width <= 0.0 || height <= 0.0 {
        return out;
    }
    let area = width * height;
    let min_side = min_side.clamp(0.0, (area / n as f32).sqrt());
    let min_area = min_side * min_side;
    let total: f64 = weights.iter().map(|weight| weight.max(0.0)).sum();
    let shares: Vec<f64> = if total > 0.0 {
        weights
            .iter()
            .map(|weight| weight.max(0.0) / total)
            .collect()
    } else {
        vec![1.0 / n as f64; n]
    };
    let tiny: Vec<usize> = shares
        .iter()
        .enumerate()
        .filter(|(_, share)| **share * f64::from(area) < f64::from(min_area))
        .map(|(index, _)| index)
        .collect();
    let bounds = Rect {
        x: 0.0,
        y: 0.0,
        w: width,
        h: height,
    };
    if tiny.is_empty() || tiny.len() == n || min_side <= 0.0 {
        squarify(&shares, bounds, &mut out);
        return out;
    }
    let per_row = (width / min_side).floor().max(1.0) as usize;
    let rows = tiny.len().div_ceil(per_row);
    let strip = (rows as f32 * min_side).min(height * 0.5);
    if strip >= height - min_side {
        squarify(&shares, bounds, &mut out);
        return out;
    }
    let large: Vec<(usize, f64)> = shares
        .iter()
        .enumerate()
        .filter(|(index, _)| !tiny.contains(index))
        .map(|(index, share)| (index, *share))
        .collect();
    squarify_indexed(
        &large,
        Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height - strip,
        },
        &mut out,
    );
    let cell_h = strip / rows as f32;
    for (place, index) in tiny.iter().enumerate() {
        let row = place / per_row;
        let col = place % per_row;
        let count = if row + 1 == rows {
            tiny.len() - row * per_row
        } else {
            per_row
        };
        let cell_w = width / count as f32;
        out[*index] = Rect {
            x: col as f32 * cell_w,
            y: height - strip + row as f32 * cell_h,
            w: cell_w,
            h: cell_h,
        };
    }
    out
}

/// The tile in `direction` from `from`, or `None` at the edge. `direction` is a unit step:
/// left is `(-1, 0)`.
pub fn neighbor(tiles: &[(usize, Rect)], from: usize, direction: [f32; 2]) -> Option<usize> {
    let origin = tiles.iter().find(|(id, _)| *id == from)?.1;
    let (ox, oy) = (origin.x + origin.w / 2.0, origin.y + origin.h / 2.0);
    tiles
        .iter()
        .filter(|(id, _)| *id != from)
        .filter_map(|(id, rect)| {
            let (vx, vy) = (rect.x + rect.w / 2.0 - ox, rect.y + rect.h / 2.0 - oy);
            let along = vx * direction[0] + vy * direction[1];
            if along <= 1.0 {
                return None;
            }
            let cross = (vx * direction[1] - vy * direction[0]).abs();
            Some((along + cross * 1.6, *id))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, id)| id)
}

fn squarify(shares: &[f64], rect: Rect, out: &mut [Rect]) {
    let indexed: Vec<(usize, f64)> = shares.iter().copied().enumerate().collect();
    squarify_indexed(&indexed, rect, out);
}

fn squarify_indexed(items: &[(usize, f64)], rect: Rect, out: &mut [Rect]) {
    if items.is_empty() || rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let mut ordered = items.to_vec();
    ordered.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    let total: f64 = ordered.iter().map(|(_, share)| *share).sum();
    if total <= 0.0 {
        let share = 1.0 / ordered.len() as f64;
        for item in &mut ordered {
            item.1 = share;
        }
    }
    place(&ordered, rect, out);
}

/// Worst aspect ratio of laying `row` along a side of length `short`.
fn worst(row: &[(usize, f64)], short: f32, total: f64, area: f32) -> f32 {
    let row_sum: f64 = row.iter().map(|(_, share)| share.max(0.0)).sum();
    if row_sum <= 0.0 || short <= 0.0 || total <= 0.0 || area <= 0.0 {
        return f32::MAX;
    }
    let thickness = (row_sum / total) as f32 * area / short;
    if thickness <= 0.0 {
        return f32::MAX;
    }
    row.iter()
        .map(|(_, share)| {
            let length = (share.max(0.0) / row_sum) as f32 * short;
            let aspect = length / thickness;
            aspect.max(1.0 / aspect.max(1e-6))
        })
        .fold(1.0, f32::max)
}

fn place(items: &[(usize, f64)], rect: Rect, out: &mut [Rect]) {
    if items.is_empty() || rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    if items.len() == 1 {
        out[items[0].0] = rect;
        return;
    }
    let total: f64 = items.iter().map(|(_, share)| share.max(0.0)).sum();
    let short = rect.w.min(rect.h);
    let mut row_end = 1;
    let mut best = worst(&items[..1], short, total, rect.area());
    while row_end < items.len() {
        let next = worst(&items[..=row_end], short, total, rect.area());
        if next > best + 1e-3 {
            break;
        }
        best = next;
        row_end += 1;
    }
    let row_share: f64 = items[..row_end]
        .iter()
        .map(|(_, share)| share.max(0.0))
        .sum();
    let fraction = if total > 0.0 {
        (row_share / total) as f32
    } else {
        1.0
    };
    // Along the shorter side: a strip across the top of a tall rectangle, down the left of a wide one.
    if rect.h >= rect.w {
        let thickness = (fraction * rect.h).clamp(0.0, rect.h);
        lay_row(
            &items[..row_end],
            Rect {
                x: rect.x,
                y: rect.y,
                w: rect.w,
                h: thickness,
            },
            true,
            out,
        );
        place(
            &items[row_end..],
            Rect {
                x: rect.x,
                y: rect.y + thickness,
                w: rect.w,
                h: (rect.h - thickness).max(0.0),
            },
            out,
        );
    } else {
        let thickness = (fraction * rect.w).clamp(0.0, rect.w);
        lay_row(
            &items[..row_end],
            Rect {
                x: rect.x,
                y: rect.y,
                w: thickness,
                h: rect.h,
            },
            false,
            out,
        );
        place(
            &items[row_end..],
            Rect {
                x: rect.x + thickness,
                y: rect.y,
                w: (rect.w - thickness).max(0.0),
                h: rect.h,
            },
            out,
        );
    }
}

/// `across` lays the row left to right; otherwise top to bottom. The last tile takes the remainder,
/// so the row fills its strip exactly.
fn lay_row(row: &[(usize, f64)], rect: Rect, across: bool, out: &mut [Rect]) {
    let sum: f64 = row.iter().map(|(_, share)| share.max(0.0)).sum();
    let mut cursor = 0.0;
    for (place, (index, share)) in row.iter().enumerate() {
        let fraction = if sum > 0.0 {
            share.max(0.0) / sum
        } else {
            1.0 / row.len() as f64
        };
        let last = place + 1 == row.len();
        if across {
            let w = if last {
                rect.w - cursor
            } else {
                fraction as f32 * rect.w
            };
            out[*index] = Rect {
                x: rect.x + cursor,
                y: rect.y,
                w,
                h: rect.h,
            };
            cursor += w;
        } else {
            let h = if last {
                rect.h - cursor
            } else {
                fraction as f32 * rect.h
            };
            out[*index] = Rect {
                x: rect.x,
                y: rect.y + cursor,
                w: rect.w,
                h,
            };
            cursor += h;
        }
    }
}

fn direction(key: &Key) -> Option<[f32; 2]> {
    match key {
        Key::Left => Some([-1.0, 0.0]),
        Key::Right => Some([1.0, 0.0]),
        Key::Up => Some([0.0, -1.0]),
        Key::Down => Some([0.0, 1.0]),
        _ => None,
    }
}

/// A wash of today's move over the card, never a painted block. Quiet days stay close to steel;
/// a large move reads, and only in the gain or loss colour.
fn wash(change: Option<f64>, up: Color, down: Color) -> Option<Color> {
    let change = change.filter(|change| change.is_finite() && *change != 0.0)?;
    let strength = ((change.abs() / 2.5) as f32).clamp(0.0, 1.0);
    Some((if change > 0.0 { up } else { down }).alpha(0.08 + 0.16 * strength))
}

/// The map. Area is value, the tint is today's move, and a hairline keeps the asset class.
pub fn view(s: &State, t: &Theme, width: f32, height: f32) -> El {
    let rows = portfolio::holding_rows(s);
    if rows.is_empty() {
        return ui::muted("No holdings match.", t).padding(12.0);
    }
    let previous = s.summary.as_ref().map_or(0.0, |summary| {
        portfolio::previous_total(summary.total_value, summary.day_change)
    });
    let weights: Vec<f64> = rows
        .iter()
        .map(|position| position.value.max(0.0))
        .collect();
    let min_side = if s.compact { 64.0 } else { 76.0 };
    let rects = layout(&weights, width, height, min_side);
    let placed: Vec<(usize, Rect)> = rows
        .iter()
        .zip(&rects)
        .map(|(position, rect)| (position.id, *rect))
        .collect();
    let k = &t.kit;
    el().key("holdings-map")
        .role("grid")
        .label("Holdings by value")
        .w_full()
        .h(height)
        .children(rows.iter().zip(rects).map(|(position, rect)| {
            let id = position.id;
            let selected = s.selected == Some(id);
            let name = position
                .name
                .clone()
                .or_else(|| position.ticker.clone())
                .unwrap_or_else(|| "Unknown".to_string());
            // A narrow tile keeps the ticker, which still fits, rather than half a name.
            let shown = if rect.w < 120.0 {
                position.ticker.clone().unwrap_or(name.clone())
            } else {
                name.clone()
            };
            let change = position.day_change_percent;
            let points = portfolio::day_points(portfolio::day_amount(position), previous);
            let share = points.map_or_else(|| "-".to_string(), format::percent);
            let show_value = rect.w >= 140.0 && rect.h >= 88.0;
            let show_share = rect.w >= 72.0 && rect.h >= 56.0;
            let neighbors = placed.clone();
            let gap = 3.0;
            let (x, y, w, h) = (
                rect.x + gap,
                rect.y + gap,
                (rect.w - gap * 2.0).max(1.0),
                (rect.h - gap * 2.0).max(1.0),
            );
            let label = if points.is_some() {
                format!("{name}, {share} of today's move")
            } else {
                name.clone()
            };
            el().key(format!("tile-{id}"))
                .matched(format!("position-{id}"))
                .role("gridcell")
                .label(label)
                .focusable(true)
                .focus_selected(selected)
                .absolute(x, y)
                .size(w, h)
                .radius(8.0)
                .clip_rounded(8.0)
                .fill(t.card)
                .border(
                    1.0,
                    if selected {
                        k.c.text_base
                    } else {
                        k.c.border_muted
                    },
                )
                .hover(k.c.hover)
                .motion(LEAD)
                .press_scale(0.985)
                .on_press(move |s: &mut State| s.select_position(id))
                .on_key(move |s: &mut State, event: &KeyEvent, tx: &mut Tx| {
                    if event.mods.shift || event.mods.ctrl || event.mods.alt || event.mods.cmd {
                        return false;
                    }
                    if event.key == Key::Enter {
                        s.open_form(Some(id));
                        return true;
                    }
                    let Some(step) = direction(&event.key) else {
                        return false;
                    };
                    let Some(next) = neighbor(&neighbors, id, step) else {
                        return true;
                    };
                    if s.selected != Some(next) {
                        s.select_position(next);
                    }
                    tx.focus(format!("tile-{next}"), true);
                    tx.scroll_into_view(format!("tile-{next}"));
                    true
                })
                .children(
                    wash(change, t.up, t.down)
                        .map(|wash| el().absolute(0.0, 0.0).size(w, h).fill(wash).a11y_hidden()),
                )
                .child(
                    el().absolute(0.0, 0.0)
                        .w(3.0)
                        .h_full()
                        .fill(t.class_color(&position.asset_class))
                        .a11y_hidden(),
                )
                .child(
                    col()
                        .absolute(0.0, 0.0)
                        .size(w, h)
                        .px(12.0)
                        .pt(10.0)
                        .pl(14.0)
                        .gap(2.0)
                        .child(
                            text(shown)
                                .font(k.ty.ui_medium)
                                .color(k.c.text_base)
                                .truncate(),
                        )
                        .children(show_value.then(|| {
                            text(s.money(position.value))
                                .font(NUMBER)
                                .color(k.c.text_muted)
                                .truncate()
                        }))
                        .children(show_share.then(|| {
                            text(share)
                                .font(NUMBER)
                                .color(t.optional_change(points))
                                .truncate()
                        })),
                )
        }))
}

/// How tall the map should be so a short table's empty page becomes the picture.
pub fn height(s: &State, main: Option<[f32; 2]>) -> f32 {
    let fallback = if s.compact { 520.0 } else { 560.0 };
    let Some([_, main]) = main else {
        return fallback;
    };
    // Heading, the sentence, the toolbar row, and the page's own padding.
    // The heading, the sentence and the toolbar sit above the map, inside the same scroller.
    let chrome = if s.compact { 340.0 } else { 248.0 };
    (main - chrome).max(if s.compact { 300.0 } else { 420.0 })
}

/// Arrow keys while nothing is focused: land on the selected tile, or the largest.
pub fn focus_tile(s: &mut State, tx: &mut Tx) -> bool {
    if s.page != Page::Holdings || !s.holdings_map {
        return false;
    }
    let id = s.selected.or_else(|| {
        portfolio::holding_rows(s)
            .into_iter()
            .max_by(|a, b| a.value.total_cmp(&b.value))
            .map(|position| position.id)
    });
    let Some(id) = id else {
        return false;
    };
    if s.selected != Some(id) {
        s.select_position(id);
    }
    tx.focus(format!("tile-{id}"), true);
    tx.scroll_into_view(format!("tile-{id}"));
    true
}

#[cfg(test)]
mod tests {
    use rune::Color;

    use super::*;

    fn covers(rects: &[Rect], width: f32, height: f32) {
        assert!(rects.iter().all(|rect| rect.w >= 0.0 && rect.h >= 0.0));
        let sum: f32 = rects.iter().map(|rect| rect.area()).sum();
        assert!(
            (sum - width * height).abs() < 1.0,
            "filled {sum}, wanted {}",
            width * height
        );
        let step = 4.0;
        let mut y = step / 2.0;
        while y < height {
            let mut x = step / 2.0;
            while x < width {
                let hits = rects.iter().filter(|rect| rect.contains(x, y)).count();
                assert_eq!(hits, 1, "({x}, {y}) is in {hits} tiles");
                x += step;
            }
            y += step;
        }
    }

    #[test]
    fn two_equal_holdings_split_a_wide_map_into_squares() {
        let tiles = layout(&[1.0, 1.0], 200.0, 100.0, 0.0);
        assert_eq!(
            tiles[0],
            Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0
            }
        );
        assert_eq!(
            tiles[1],
            Rect {
                x: 100.0,
                y: 0.0,
                w: 100.0,
                h: 100.0
            }
        );
        covers(&tiles, 200.0, 100.0);
    }

    #[test]
    fn area_follows_value_and_nothing_overlaps() {
        let weights = [6.0, 6.0, 4.0, 3.0, 2.0, 1.0];
        let tiles = layout(&weights, 240.0, 160.0, 0.0);
        covers(&tiles, 240.0, 160.0);
        let total: f64 = weights.iter().sum();
        for (weight, tile) in weights.iter().zip(&tiles) {
            let share = tile.area() / (240.0 * 160.0);
            assert!(
                (f64::from(share) - weight / total).abs() < 0.02,
                "{tile:?} for {weight}"
            );
        }
    }

    #[test]
    fn a_tiny_holding_stays_legible_beside_a_huge_one() {
        let tiles = layout(&[99.0, 0.4, 0.6], 400.0, 300.0, 48.0);
        covers(&tiles, 400.0, 300.0);
        for tile in &tiles[1..] {
            assert!(tile.w >= 48.0 - 0.5 && tile.h >= 48.0 - 0.5, "{tile:?}");
        }
        assert!(tiles[0].area() > tiles[1].area() + tiles[2].area());
    }

    #[test]
    fn an_empty_or_flat_map_does_not_panic() {
        assert!(layout(&[], 100.0, 100.0, 20.0).is_empty());
        assert!(layout(&[1.0], 0.0, 100.0, 20.0)[0].area() == 0.0);
        let flat = layout(&[0.0, 0.0, 0.0], 90.0, 60.0, 0.0);
        covers(&flat, 90.0, 60.0);
        assert!(flat.iter().all(|tile| (tile.area() - 1800.0).abs() < 1.0));
    }

    #[test]
    fn arrows_move_toward_the_tile_in_that_direction() {
        let tiles = [
            (
                1,
                Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 50.0,
                },
            ),
            (
                2,
                Rect {
                    x: 100.0,
                    y: 0.0,
                    w: 50.0,
                    h: 50.0,
                },
            ),
            (
                3,
                Rect {
                    x: 0.0,
                    y: 50.0,
                    w: 150.0,
                    h: 50.0,
                },
            ),
        ];
        assert_eq!(neighbor(&tiles, 1, [1.0, 0.0]), Some(2));
        assert_eq!(neighbor(&tiles, 1, [0.0, 1.0]), Some(3));
        assert_eq!(neighbor(&tiles, 1, [-1.0, 0.0]), None);
        assert_eq!(neighbor(&tiles, 2, [-1.0, 0.0]), Some(1));
    }

    #[test]
    fn wash_uses_only_gain_and_loss_and_grows_with_the_move() {
        let up = Color {
            r: 0.0,
            g: 1.0,
            b: 0.5,
            a: 1.0,
        };
        let down = Color {
            r: 1.0,
            g: 0.0,
            b: 0.2,
            a: 1.0,
        };
        assert_eq!(wash(None, up, down), None);
        assert_eq!(wash(Some(0.0), up, down), None);
        let quiet = wash(Some(0.4), up, down).unwrap();
        let loud = wash(Some(3.0), up, down).unwrap();
        let loss = wash(Some(-2.0), up, down).unwrap();
        assert!(loud.a > quiet.a, "a bigger gain tints more");
        assert!(loud.g > 0.5 && loss.r > 0.5);
        assert!(loud.a < 0.3, "a wash, not a painted tile");
    }
}
