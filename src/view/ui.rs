//! Layout pieces shared by the pages.

use std::{
    f32::consts::{FRAC_PI_2, PI, TAU},
    sync::Arc,
};

use rune::{
    Align, Beat, Color, Cx, Cycle, El, Icon, Image, ImageCache, LoopSpec, Sequence, Shape,
    Transition, canvas, col, el, icon, row, text, theme::hex, tok::motion::LEAD,
};
use rune_kit::{Button, ButtonVariant};

use crate::{
    fx::Plate,
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

/// A floating surface made of glass when a compositor draws the window: the page behind it is
/// blurred and slightly bent by a lens, and the rim facing the room's light catches it. Text is
/// drawn on top by the caller, as on any panel. Without a compositor (tests, iOS) it is
/// [`floating`].
pub fn glass(s: &State, t: &Theme) -> El {
    if !s.shaders {
        return floating(t);
    }
    col()
        .radius(12.0)
        .continuous_corners()
        .shadow(
            18.0,
            48.0,
            Color::BLACK.alpha(if t.dark { 0.45 } else { 0.16 }),
        )
        // The lens bends and blurs the page behind the panel and lights its rim.
        .child(crate::fx::glass())
        // A tint over the glass, inset so the lit rim stays, so text keeps its contrast over any
        // page: a treemap, a chart, a photograph of a lot of colour.
        .child(
            el().pin(Some(1.5), Some(1.5), Some(1.5), Some(1.5))
                .radius(10.5)
                .continuous_corners()
                .fill(t.card.alpha(if t.dark { 0.62 } else { 0.60 }))
                .a11y_hidden(),
        )
}

/// Brushed steel filling its parent, cut to `radius`: the door's metal as a flat plate, lit from
/// the room's light, brushed straight across, with a chamfered edge. Without a compositor (tests,
/// iOS) it is the same steel, flat: the door's lit and shaded colour from top to bottom.
pub fn steel(s: &State, plate: Plate, radius: f32) -> El {
    if s.shaders {
        return crate::fx::steel(plate, radius);
    }
    flat_steel(plate, radius)
}

/// A round knob of turned steel filling its parent, like the door's hub; flat without a
/// compositor.
pub fn knob(s: &State, plate: Plate) -> El {
    if s.shaders {
        return crate::fx::knob(plate);
    }
    flat_steel(plate, 999.0)
}

/// A steel plate to build on: a column whose face is [`steel`], standing off the wall on a short
/// shadow when it is raised. The caller adds the content, inked with [`steel_ink`].
pub fn plate(s: &State, t: &Theme, plate: Plate, radius: f32) -> El {
    let col = col().radius(radius).continuous_corners().min_w(0.0);
    let col = if plate.raised {
        col.shadow(
            2.0,
            6.0,
            Color::BLACK.alpha(if t.dark { 0.55 } else { 0.22 }),
        )
    } else {
        col
    };
    col.child(steel(s, plate, radius))
}

/// The ink for words on pale steel, whatever the theme: steel is the same metal in either light.
/// The base ink, and the pale lip a cut catches under the light.
pub fn steel_ink() -> (Color, Color) {
    (
        hex("#141b20").unwrap_or(Color::BLACK),
        Color::WHITE.alpha(0.42),
    )
}

/// Words cut into steel: dark ink with the pale lip of the cut a point below it, on the side
/// away from the light, so the letters read as engraved rather than printed.
pub fn engraved(words: impl Into<String>, font: rune::TextStyle, ink: Color, lip: Color) -> El {
    let words = words.into();
    col()
        .min_w(0.0)
        .child(
            text(words.clone())
                .font(font)
                .color(lip)
                .wrap()
                .w_full()
                .absolute(0.5, 1.0)
                .a11y_hidden(),
        )
        .child(text(words).font(font).color(ink).wrap())
}

fn flat_steel(plate: Plate, radius: f32) -> El {
    let (lit, dim) = plate.flat();
    let color = |c: [f32; 3]| {
        let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        hex(&format!("#{r:02x}{g:02x}{b:02x}")).unwrap_or(Color::WHITE)
    };
    let (top, bottom) = if plate.raised { (lit, dim) } else { (dim, lit) };
    el().absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(radius)
        .a11y_hidden()
        .linear_gradient(180.0, [(0.0, color(top)), (1.0, color(bottom))])
}

/// A soft round light of `color`, `size` points across, brightest at its centre. For the head
/// of a line as it draws, and the live end it comes to rest at.
pub fn glow_orb(color: Color, size: f32, strength: f32) -> El {
    el().size(size, size).a11y_hidden().radial_gradient(
        [0.5, 0.5],
        size / 2.0,
        [
            (0.0, color.alpha(strength)),
            (0.35, color.alpha(strength * 0.42)),
            (1.0, color.alpha(0.0)),
        ],
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
    card.enter_delay(index as f32 * STAGGER)
        .transition(arrive())
}

// --- Motion ---
//
// The few ways things move, decided once (DESIGN.md, "Motion"). A page reaches for these rather
// than writing its own offsets and springs.

/// Between cards arriving one after another.
pub const STAGGER: f32 = 0.05;

/// How far a page travels as it arrives, in the direction the owner moved.
pub const PAGE_TRAVEL: f32 = 10.0;

/// Press feedback: a card or tile gives a little, a row in a list barely.
pub const PRESS_CARD: f32 = 0.98;
pub const PRESS_ROW: f32 = 0.995;

/// Content arriving: it rises a little and sharpens into place.
pub fn arrive() -> Transition {
    Transition::offset(0.0, 12.0)
        .combined(Transition::blur(5.0))
        .combined(Transition::opacity())
}

/// A small thing arriving or leaving in place: a mark, a badge, a button that wasn't there.
pub fn appear() -> Transition {
    Transition::scale(0.9).combined(Transition::opacity())
}

/// Something that floats over the page: the palette, a menu, a dialog of our own.
pub fn float_in() -> Transition {
    Transition::scale(0.96)
        .combined(Transition::blur(4.0))
        .combined(Transition::opacity())
}

/// A plate set onto the wall: it comes the last few points, out of a slight blur, and settles.
pub fn settle() -> Transition {
    Transition::offset(0.0, 6.0)
        .combined(Transition::scale(0.985))
        .combined(Transition::blur(3.0))
        .combined(Transition::opacity())
}

/// The light passing once over something new: a pale sheen falling from above that fades in and
/// out. Keyed by `key`, so it plays once per arrival and never on a redraw.
pub fn catch_light(key: impl Into<String>, t: &Theme, radius: f32) -> El {
    let sheen = Color::WHITE.alpha(if t.dark { 0.14 } else { 0.55 });
    el().key(format!("{}-light", key.into()))
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(radius)
        .a11y_hidden()
        .linear_gradient(
            180.0,
            [
                (0.0, sheen),
                (0.5, sheen.alpha(0.0)),
                (1.0, sheen.alpha(0.0)),
            ],
        )
        .sequence(Sequence::new(CATCH))
}

/// The light: in quickly, then out over a second and a half.
pub const CATCH: &[Beat] = &[
    Beat::new(0.0).opacity(0.0),
    Beat::new(0.12).opacity(1.0),
    Beat::new(0.5).opacity(0.85),
    Beat::new(1.3).opacity(0.0),
];

/// A banner or an error, dropping in from above where it sits.
pub fn notice_in() -> Transition {
    Transition::offset(0.0, -6.0).combined(Transition::opacity())
}

/// Which way the owner moved through the app's pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Travel {
    /// Further down the sidebar, or right along the phone's tabs.
    Forward,
    /// Back up, or left.
    Back,
}

/// A page arriving: it comes from the side the owner moved towards, down the sidebar or along
/// a phone's tabs, and the page it replaces just fades. `across` is the phone's horizontal tabs.
pub fn page_arrive(travel: Travel, across: bool) -> Transition {
    let distance = match travel {
        Travel::Forward => PAGE_TRAVEL,
        Travel::Back => -PAGE_TRAVEL,
    };
    let (x, y) = if across {
        (distance * 2.0, 0.0)
    } else {
        (0.0, distance)
    };
    Transition::asymmetric(
        Transition::offset(x, y)
            .combined(Transition::blur(3.0))
            .combined(Transition::opacity()),
        Transition::opacity(),
    )
}

/// A refresh moved this: a wash of the gain or loss colour that shows once and fades. Keyed by
/// the refresh, so the next one replays it; under reduced motion it lands faded, showing nothing.
pub fn flash(key: impl Into<String>, updates: u64, color: Color, radius: f32) -> El {
    el().key(format!("{}-flash-{updates}", key.into()))
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .radius(radius)
        .fill(color.alpha(0.22))
        .a11y_hidden()
        .sequence(Sequence::new(FLASH))
}

/// The flash: quickly in, then a slow fade.
pub const FLASH: &[Beat] = &[
    Beat::new(0.0).opacity(0.0),
    // The refresh's pulse of light reaches the figures first; then they wash.
    Beat::new(0.18).opacity(0.0),
    Beat::new(0.30).opacity(1.0),
    Beat::new(1.05).opacity(0.0),
];

/// How the last refresh moved an asset class, as the change in its value: positive, negative,
/// or `None` if none of its prices moved.
pub fn class_move(s: &State, asset_class: &str) -> Option<f64> {
    let mut moved = false;
    let mut change = 0.0;
    for position in s.positions.iter().filter(|p| p.asset_class == asset_class) {
        if let Some((_, fraction)) = s.price_moves.iter().find(|(id, _)| *id == position.id) {
            moved = true;
            // The value it had before, times how far its price moved.
            change += position.value / (1.0 + fraction) * fraction;
        }
    }
    (moved && change != 0.0).then_some(change)
}

/// The flash for an asset class whose prices the last refresh moved.
pub fn class_flash(s: &State, asset_class: &str, radius: f32, t: &Theme) -> Option<El> {
    class_move(s, asset_class).map(|change| {
        flash(
            format!("class-{asset_class}"),
            s.price_updates,
            t.change(change),
            radius,
        )
    })
}

/// The flash for position `id` if the last refresh moved its price, in the direction it moved.
pub fn price_flash(s: &State, id: usize, radius: f32, t: &Theme) -> Option<El> {
    s.price_moves
        .iter()
        .find(|(moved, _)| *moved == id)
        .map(|(_, change)| {
            flash(
                format!("position-{id}"),
                s.price_updates,
                t.change(*change),
                radius,
            )
        })
}

/// The room's one light on the wall behind a page. A compositor draws it live: it follows the
/// pointer, rests above the middle of the window, and takes the day's colour. Without one (tests,
/// iOS) it is [`wash`], the still light the app has always had.
pub fn light(s: &State, t: &Theme, radius: f32) -> El {
    if s.shaders {
        // A refresh's pulse crosses the same wall the light falls on.
        let pulse = match (s.pulse_started, s.door_mark) {
            (Some(started), Some([x, y, w, h])) => [started as f32, x + w / 2.0, y + h / 2.0, 1.0],
            _ => [0.0; 4],
        };
        crate::fx::room(radius, pulse)
    } else {
        wash(t, radius)
    }
}

/// One soft light from above, on the wall behind a page, for a window without a compositor.
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

/// How much room the content area has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Room {
    /// A phone: one column.
    Narrow,
    Medium,
    Wide,
}

/// The room the content area had at the last layout. Before the first, the phone layout is narrow
/// and anything else wide, so neither starts in the other's layout.
pub fn room(s: &State, cx: &Cx<State>) -> Room {
    match cx.layout_size("main") {
        Some([width, _]) if width >= 860.0 => Room::Wide,
        Some([width, _]) if width >= 560.0 => Room::Medium,
        Some(_) => Room::Narrow,
        None if s.compact => Room::Narrow,
        None => Room::Wide,
    }
}

/// Whether the content area has room for wide layouts.
pub fn is_wide(s: &State, cx: &Cx<State>) -> bool {
    room(s, cx) == Room::Wide
}

/// Equal columns that fit the content area: `[wide, medium, narrow]` of them, wrapping into rows.
pub fn columns(room: Room, counts: [usize; 3], gap: f32) -> El {
    let count = match room {
        Room::Wide => counts[0],
        Room::Medium => counts[1],
        Room::Narrow => counts[2],
    };
    row()
        .center_y()
        .w_full()
        .gap(gap)
        .items_stretch()
        .equal_columns(count)
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
        .transition(notice_in())
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

/// A shortcut's keys as this platform writes them: ⌘ on Apple's, Ctrl elsewhere, as Rune maps
/// its command chords. An iPad's keyboard has ⌘ too.
pub fn chord_keys(shift: bool, key: &str) -> Vec<String> {
    let (command, shifted) = if cfg!(any(target_os = "macos", target_os = "ios")) {
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
    let separator = if cfg!(any(target_os = "macos", target_os = "ios")) {
        ""
    } else {
        "+"
    };
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

/// What a page shows while it has nothing to show: what belongs there, and how to get it there.
pub fn empty_state(
    key: &'static str,
    glyph: &'static Icon,
    heading: &str,
    detail: &str,
    action: El,
    t: &Theme,
) -> El {
    let k = &t.kit;
    entering(
        card(t)
            .key(key)
            .w_full()
            .items_center()
            .gap(8.0)
            .py(40.0)
            .child(
                col()
                    .size(40.0, 40.0)
                    .mb(4.0)
                    .radius(10.0)
                    .continuous_corners()
                    .center()
                    .fill(k.c.layer_03)
                    .child(icon(glyph, 20.0).color(k.c.text_base)),
            )
            .child(title(heading, t))
            .child(
                col()
                    .w_full()
                    .max_w(480.0)
                    .items_center()
                    .child(muted(detail, t).wrap().align(Align::Center)),
            )
            .child(row().center_y().gap(8.0).mt(12.0).child(action)),
        0,
    )
}

/// The button that starts the Add Position form. `key` tells apart the places it appears.
pub fn add_position_button(key: &'static str, t: &Theme) -> El {
    Button::new("Add Position")
        .variant(ButtonVariant::Submit)
        .icon(&icons::PLUS)
        .shortcut(chord(false, "N"))
        .on_press(|s: &mut State| s.open_form(None))
        .build(&t.kit)
        .key(key)
}

/// A visible label over a field that has its whole row.
pub fn wide_field(label: impl Into<String>, input: El, t: &Theme) -> El {
    col()
        .w_full()
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
