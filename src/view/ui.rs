//! Layout pieces shared by the pages.

use std::{
    f32::consts::{FRAC_PI_2, PI, TAU},
    sync::Arc,
};

use rune::{
    Color, Cx, Cycle, El, Image, ImageCache, LoopSpec, Shape, Transition, canvas, col, el, icon,
    row, text, theme::hex, tok::motion::LEAD,
};
use rune_kit::{Button, ButtonVariant};

use crate::{
    icons,
    state::{Loadable, State},
    theme::{FIGURE, NUMBER, SECTION, Theme},
};

thread_local! {
    /// The door, decoded and mipmapped the first time a view shows it. Views all run on the UI
    /// thread, and the cache's worker is idle after that one image.
    static DOOR: (ImageCache, Image) = {
        let cache = ImageCache::new(Arc::new(|| {}));
        let image = cache.load(Arc::<[u8]>::from(&include_bytes!("../../assets/door.png")[..]));
        // Once, and only a few milliseconds: the welcome screen shouldn't open on an empty frame.
        let _ = image.wait();
        (cache, image)
    };
}

/// The icon's rings, flat. What the door stands in for if it can't be shown.
pub fn rings(size: f32) -> El {
    const RINGS: [(f32, &str, &str); 5] = [
        (254.0, "#26333B", "#11212C"),
        (204.0, "#3C5260", "#1C3547"),
        (156.0, "#4B6475", "#204154"),
        (100.0, "#A5B2BA", "#90A1AA"),
        (48.0, "#D2D8DC", "#C7D0D4"),
    ];
    let half = |radius: f32, from: f32, to: f32| -> Vec<[f32; 2]> {
        (0..=48)
            .map(|i| {
                let angle = from + (to - from) * i as f32 / 48.0;
                [256.0 + radius * angle.cos(), 256.0 + radius * angle.sin()]
            })
            .collect()
    };
    let mut shape = Shape::new();
    for (radius, left, right) in RINGS {
        let (left, right) = (
            hex(left).unwrap_or(Color::WHITE),
            hex(right).unwrap_or(Color::WHITE),
        );
        shape = shape
            .fill(half(radius, FRAC_PI_2, FRAC_PI_2 + PI), left)
            .fill(half(radius, -FRAC_PI_2, FRAC_PI_2), right);
    }
    canvas(shape.view_box(512.0, 512.0))
        .size(size, size)
        .a11y_hidden()
}

/// The Stahl mark: the icon's rings as a steel door, rendered by the landing page's shader.
/// Mipmapped, so it holds from the sidebar's 28 points to the welcome screen's 168.
pub fn door(size: f32) -> El {
    DOOR.with(|(_, image)| {
        if image.is_ready() && image.error().is_none() {
            el().size(size, size).pixels(image.pixels()).a11y_hidden()
        } else {
            rings(size)
        }
    })
}

/// A box, for what has to hold together: a table, a form.
pub fn card(t: &Theme) -> El {
    col()
        .fill(t.card)
        .radius(12.0)
        .continuous_corners()
        .border(1.0, t.kit.c.border_muted)
        .padding(20.0)
        .gap(12.0)
        .min_w(0.0)
}

/// Something that floats over the page: the command palette, a chart's readout, a menu.
/// The only surfaces with a shadow and a blur behind them.
pub fn floating(t: &Theme) -> El {
    col()
        .fill(t.card.alpha(0.86))
        .backdrop_blur(24.0)
        .radius(12.0)
        .continuous_corners()
        .border(1.0, t.kit.c.border_base)
        .shadow(
            18.0,
            48.0,
            Color::BLACK.alpha(if t.dark { 0.45 } else { 0.16 }),
        )
}

/// A section with a rule over it and no box, like a line in a ledger.
pub fn ledger(t: &Theme) -> El {
    col()
        .pt(16.0)
        .gap(12.0)
        .min_w(0.0)
        .child(rule(t).absolute(0.0, 0.0))
}

/// A hairline across its parent.
pub fn rule(t: &Theme) -> El {
    el().w_full().h(1.0).fill(t.kit.c.border_base).a11y_hidden()
}

/// A card that sharpens into place after the ones before it.
pub fn entering(card: El, index: usize) -> El {
    card.enter_delay(index as f32 * 0.05).transition(
        Transition::offset(0.0, 12.0)
            .combined(Transition::blur(5.0))
            .combined(Transition::opacity()),
    )
}

/// One soft light from above, on the wall behind a page.
/// `radius` follows whatever it fills: the rounded main pane, or a whole window.
pub fn wash(t: &Theme, radius: f32) -> El {
    let light = if t.dark {
        Color::WHITE.alpha(0.055)
    } else {
        Color::WHITE.alpha(0.42)
    };
    el().key("wash")
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(radius)
        .a11y_hidden()
        .radial_gradient(
            [0.5, 0.0],
            [900.0, 520.0],
            [(0.0, light), (1.0, light.alpha(0.0))],
        )
}

/// Something alive: fades gently in and out, and holds still under reduced motion.
pub const BREATHE: Cycle = Cycle {
    period: 2.6,
    phase: 0.0,
    stops: &[(0.0, 0.25), (0.5, 1.0), (1.0, 0.25)],
    still: 1.0,
    fps: 30.0,
};

/// The mark, with a light orbiting it. For the moments when the app has the whole window.
pub fn orbiting_mark(size: f32, t: &Theme) -> El {
    let frame = size * 1.5;
    let center = frame / 2.0;
    let orbit = |radius: f32, from: f32, to: f32| -> Vec<[f32; 2]> {
        (0..=40)
            .map(|i| {
                let angle = from + (to - from) * i as f32 / 40.0;
                [center + radius * angle.cos(), center + radius * angle.sin()]
            })
            .collect()
    };
    let radius = center - 2.0;
    el().size(frame, frame)
        .center()
        .a11y_hidden()
        .child(
            el().absolute(0.0, 0.0)
                .size(frame, frame)
                .radial_gradient(
                    [0.5, 0.5],
                    center,
                    [(0.0, t.accent.alpha(0.12)), (1.0, t.accent.alpha(0.0))],
                )
                .cycle(BREATHE),
        )
        .child(
            canvas(
                Shape::new()
                    .stroke(orbit(radius, 0.0, TAU), 1.0, t.kit.c.text_base.alpha(0.07))
                    .stroke(orbit(radius, 0.0, 1.2), 2.0, t.accent)
                    .stroke(orbit(radius, 3.1, 3.5), 2.0, t.accent.alpha(0.5)),
            )
            .absolute(0.0, 0.0)
            .size(frame, frame)
            .looping(LoopSpec::rotation(7.0)),
        )
        .child(door(size))
}

/// A ring filled to `fraction`, with a figure in the middle.
pub fn gauge(fraction: f32, figure: impl Into<String>, color: Color, t: &Theme) -> El {
    const SIZE: f32 = 64.0;
    let center = SIZE / 2.0;
    let arc = |to: f32| -> Vec<[f32; 2]> {
        (0..=64)
            .map(|i| {
                let angle = -FRAC_PI_2 + to * TAU * i as f32 / 64.0;
                [center + 27.0 * angle.cos(), center + 27.0 * angle.sin()]
            })
            .collect()
    };
    el().size(SIZE, SIZE)
        .center()
        .role("img")
        .child(
            canvas(Shape::new().stroke(arc(1.0), 6.0, t.kit.c.layer_03).stroke(
                arc(fraction.clamp(0.0, 1.0)),
                6.0,
                color,
            ))
            .absolute(0.0, 0.0)
            .size(SIZE, SIZE),
        )
        .child(text(figure.into()).font(NUMBER).color(t.kit.c.text_base))
}

pub fn title(value: impl Into<String>, t: &Theme) -> El {
    text(value.into()).font(SECTION).color(t.kit.c.text_base)
}

pub fn muted(value: impl Into<String>, t: &Theme) -> El {
    text(value.into())
        .font(t.kit.ty.ui)
        .color(t.kit.c.text_muted)
}

pub fn caption(value: impl Into<String>, t: &Theme) -> El {
    text(value.into())
        .font(t.kit.ty.text_xsmall)
        .color(t.kit.c.text_muted)
}

pub fn number(value: impl Into<String>, color: Color) -> El {
    text(value.into()).font(NUMBER).color(color)
}

pub fn header(heading: impl Into<String>, detail: impl Into<String>, t: &Theme) -> El {
    col()
        .gap(2.0)
        .child(title(heading, t))
        .child(caption(detail, t).wrap())
}

/// A labelled figure.
pub fn stat(
    label: impl Into<String>,
    value: impl Into<String>,
    color: Option<Color>,
    t: &Theme,
) -> El {
    col().gap(4.0).min_w(0.0).child(caption(label, t)).child(
        text(value.into())
            .font(FIGURE)
            .color(color.unwrap_or(t.kit.c.text_base))
            .selectable(true),
    )
}

pub fn stat_card(
    label: impl Into<String>,
    value: impl Into<String>,
    color: Option<Color>,
    t: &Theme,
) -> El {
    card(t).padding(16.0).child(stat(label, value, color, t))
}

/// Equal columns that fit the window.
pub fn columns(wide: bool, many: usize, few: usize, gap: f32) -> El {
    row()
        .center_y()
        .w_full()
        .gap(gap)
        .items_stretch()
        .equal_columns(if wide { many } else { few })
}

/// Whether the content area has room for wide layouts.
pub fn is_wide(cx: &Cx<State>) -> bool {
    cx.layout_size("main")
        .is_none_or(|[width, _]| width >= 860.0)
}

pub fn badge(value: impl Into<String>, color: Color) -> El {
    row()
        .center_y()
        .px(8.0)
        .py(2.0)
        .radius(10.0)
        .fill(color.alpha(0.14))
        .child(text(value.into()).font(NUMBER.lh(16.0)).color(color))
}

pub fn dot(color: Color) -> El {
    el().size(7.0, 7.0).radius(3.5).fill(color)
}

pub fn danger(message: impl Into<String>, t: &Theme) -> El {
    row()
        .center_y()
        .w_full()
        .gap(10.0)
        .px(14.0)
        .py(10.0)
        .radius(8.0)
        .fill(t.down.alpha(0.12))
        .border(1.0, t.down.alpha(0.6))
        .role("alert")
        .transition(Transition::offset(0.0, -6.0).combined(Transition::opacity()))
        .child(icon(&icons::TRIANGLE_ALERT, 15.0).color(t.down))
        .child(
            text(message.into())
                .font(t.kit.ty.ui)
                .color(t.down)
                .wrap()
                .shrink(),
        )
}

pub fn notice(message: impl Into<String>, t: &Theme) -> El {
    row()
        .center_y()
        .w_full()
        .px(14.0)
        .py(10.0)
        .radius(8.0)
        .fill(t.kit.c.layer_02)
        .child(
            text(message.into())
                .font(t.kit.ty.ui)
                .color(t.kit.c.text_base)
                .wrap(),
        )
}

pub fn finding(color: Color, heading: impl Into<String>, detail: Option<String>, t: &Theme) -> El {
    row()
        .center_y()
        .w_full()
        .items_start()
        .gap(12.0)
        .py(10.0)
        .child(rule(t).absolute(0.0, 0.0))
        .child(dot(color).mt(5.0))
        .child(
            col()
                .shrink()
                .gap(2.0)
                .child(
                    text(heading.into())
                        .font(t.kit.ty.ui_medium)
                        .color(t.kit.c.text_base)
                        .wrap(),
                )
                .children(detail.map(|detail| caption(detail, t).wrap())),
        )
}

/// A bar showing a share of the whole, with an optional target tick. Its width springs.
pub fn share_bar(key: String, percent: f64, target: Option<f64>, color: Color, t: &Theme) -> El {
    let fraction = |value: f64| (value / 100.0).clamp(0.0, 1.0) as f32;
    el().key(key)
        .w_full()
        .h(4.0)
        .radius(2.0)
        .fill(t.kit.c.layer_03)
        .child(
            el().h_full()
                .radius(2.0)
                .fill(color)
                .w_frac(fraction(percent))
                .motion(LEAD),
        )
        .children(target.map(|target| {
            row()
                .center_y()
                .pin(Some(0.0), Some(-4.0), Some(0.0), None)
                .h(12.0)
                .child(el().h_full().w_frac(fraction(target)))
                .child(el().size(2.0, 12.0).radius(1.0).fill(t.kit.c.text_base))
        }))
}

/// One share per slice, side by side.
pub fn stacked_bar(shares: &[(f64, Color)], t: &Theme) -> El {
    row()
        .center_y()
        .w_full()
        .h(8.0)
        .gap(2.0)
        .radius(4.0)
        .clip_rounded(4.0)
        .fill(t.kit.c.layer_03)
        .children(
            shares
                .iter()
                .filter(|(percent, _)| *percent > 0.0)
                .map(|(percent, color)| {
                    el().h_full()
                        .fill(*color)
                        .w_frac((*percent / 100.0).clamp(0.0, 1.0) as f32)
                        .motion(LEAD)
                }),
        )
}

pub fn bullets(items: &[String], color: Color, t: &Theme) -> El {
    col().gap(4.0).children(items.iter().map(|item| {
        row()
            .center_y()
            .items_start()
            .gap(8.0)
            .child(text("•").font(t.kit.ty.ui).color(color))
            .child(
                text(item.clone())
                    .font(t.kit.ty.ui)
                    .color(color)
                    .wrap()
                    .shrink(),
            )
    }))
}

/// The error banner, or a spinner until there is data to show.
pub fn status<T>(state: &Loadable<T>, label: &'static str, t: &Theme) -> Option<El> {
    match &state.error {
        Some(error) => Some(danger(error.clone(), t)),
        None if state.loading && state.data.is_none() => Some(
            card(t)
                .role("status")
                .label(label)
                .gap(14.0)
                .children([0.45, 0.9, 0.7, 0.8].map(|width| {
                    el().w_frac(width)
                        .h(12.0)
                        .radius(6.0)
                        .fill(t.kit.c.layer_03)
                        .cycle(BREATHE)
                })),
        ),
        None => None,
    }
}

pub fn copy_button(key: &'static str, markdown: String, t: &Theme) -> El {
    Button::new("Copy as Markdown")
        .variant(ButtonVariant::Ghost)
        .icon(&icons::COPY)
        .help("For pasting into notes or an AI assistant")
        .on_press_tx(move |s: &mut State, tx| s.copy(markdown.clone(), tx))
        .build(&t.kit)
        .key(key)
}

pub fn refresh_button(key: &'static str, load: fn(&mut State), t: &Theme) -> El {
    Button::new("Refresh")
        .variant(ButtonVariant::Ghost)
        .icon(&icons::REFRESH)
        .on_press(load)
        .build(&t.kit)
        .key(key)
}

/// A shortcut's keys as this platform writes them. Rune maps its command chords to Ctrl off macOS.
pub fn chord_keys(shift: bool, key: &str) -> Vec<String> {
    let (command, shifted) = if cfg!(target_os = "macos") {
        ("⌘", "⇧")
    } else {
        ("Ctrl", "Shift")
    };
    let mut keys = Vec::new();
    if shift {
        keys.push(shifted.to_string());
    }
    keys.push(command.to_string());
    keys.push(key.to_string());
    keys
}

/// [`chord_keys`] as one label, for a tooltip.
pub fn chord(shift: bool, key: &str) -> String {
    let separator = if cfg!(target_os = "macos") { "" } else { "+" };
    chord_keys(shift, key).join(separator)
}

/// A visible label over a field, sharing a row evenly with its neighbours.
pub fn field(label: impl Into<String>, input: El, t: &Theme) -> El {
    col()
        .grow(1.0)
        .w(0.0)
        .min_w(0.0)
        .gap(6.0)
        .child(caption(label, t))
        .child(input)
}

/// A simple table: a header row and rows of cells, the first column left-aligned.
pub fn table(headers: &[&str], rows: Vec<Vec<El>>, t: &Theme) -> El {
    let cell = |index: usize, content: El| {
        let cell = row().center_y().grow(1.0).min_w(0.0).w(0.0);
        if index == 0 {
            cell.child(content)
        } else {
            cell.justify_end().child(content)
        }
    };
    col()
        .w_full()
        .child(
            row().center_y().w_full().gap(12.0).pb(8.0).children(
                headers
                    .iter()
                    .enumerate()
                    .map(|(index, heading)| cell(index, caption(*heading, t))),
            ),
        )
        .children(rows.into_iter().flat_map(|cells| {
            [
                el().w_full().h(1.0).fill(t.kit.c.border_muted),
                row()
                    .center_y()
                    .w_full()
                    .gap(12.0)
                    .py(8.0)
                    .center_y()
                    .children(
                        cells
                            .into_iter()
                            .enumerate()
                            .map(|(index, content)| cell(index, content)),
                    ),
            ]
        }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_door_decodes() {
        DOOR.with(|(_, image)| assert_eq!(image.wait(), Ok([512, 512])));
    }
}
