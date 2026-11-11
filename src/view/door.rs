//! The vault door as a living mark: still at rest, its dial turns once when prices land, it
//! carries a single mark when something needs attention, and when a portfolio opens from the
//! welcome screen it travels to the mark's place and shrinks to it, the dial turning once as it
//! lands. Nothing covers the page.
//!
//! It is built from the shader-lit picture of the door (`ui::door`) and Rune's own transforms:
//! the dial is the same picture clipped to the dial's disc and turned on a spring, so at rest it
//! draws exactly the icon, and nothing animates while the app is idle. The welcome door and the
//! mark share one matched id, so the flight is the same element moving between the two places.

use std::f32::consts::TAU;

use rune::{El, Spring, Transition, el};

use crate::{agenda::Weight, state::State, theme::Theme};

use super::ui;

/// The dial's outer edge, as a fraction of the door's radius: the fourth ring of the icon, with the
/// graduations (`EDGE[4]` in `site/door.js`).
const DIAL: f32 = 0.7969;

/// A heavy dial: it takes its time and settles with a small give, as steel would.
pub const DIAL_TURN: Spring = Spring::new(1.1, 0.12);

/// The flight from the welcome door to the mark: long enough to read as one object travelling,
/// settling without a bounce, as something heavy would.
pub const DOOR_LAND: Spring = Spring::new(0.9, 0.0);

/// How many things need the owner's attention: what the Overview's agenda lists, apart from its
/// notices. The door and the agenda read the same list, so they never disagree.
pub fn attention(s: &State) -> usize {
    s.agenda()
        .iter()
        .filter(|item| item.weight != Weight::Notice)
        .count()
}

/// Whether one of the things needing attention is a failure rather than a warning.
fn attention_is_critical(s: &State) -> bool {
    s.agenda()
        .iter()
        .any(|item| matches!(item.weight, Weight::Total | Weight::Critical))
}

/// The angle the dial rests at: one full turn per price refresh and per portfolio opened, so
/// every rest looks the same and the spring between them is the turn.
pub fn dial_angle(turns: u64) -> f32 {
    // Wrapped, so a long session doesn't lose precision; the spring only ever sees one turn.
    (turns % 1024) as f32 * TAU
}

/// Turns the dial has to show: each refresh, and one when a portfolio's door first appears.
pub fn dial_turns(s: &State) -> u64 {
    s.price_updates + s.door_arrivals
}

/// What the door says about the portfolio, for assistive technology and the tooltip.
pub fn describe(count: usize) -> String {
    match count {
        0 => "Nothing needs attention".to_string(),
        1 => "1 thing needs attention".to_string(),
        n => format!("{n} things need attention"),
    }
}

/// The living door at `size`. `place` keeps the dial's key apart where two doors are on screen.
pub fn vault(s: &State, t: &Theme, size: f32, place: &str) -> El {
    let count = if s.has_portfolio() { attention(s) } else { 0 };
    let dial = size * DIAL;
    let inset = (size - dial) / 2.0;
    let mut door = el()
        .key(format!("door-{place}"))
        .size(size, size)
        .child(ui::door(size))
        .child(
            // The dial: the same picture, clipped to the dial's disc, so at rest it is invisible.
            el().key(format!("door-dial-{place}"))
                .absolute(inset, inset)
                .size(dial, dial)
                .radius(dial / 2.0)
                .clip_rounded(dial / 2.0)
                .a11y_hidden()
                .rotate(dial_angle(dial_turns(s)))
                .rotate_motion(DIAL_TURN)
                .child(ui::door(size).absolute(-inset, -inset)),
        );
    if count > 0 {
        // One mark on the dial at twelve o'clock, where a combination is read.
        let color = if attention_is_critical(s) {
            t.down
        } else {
            t.warn
        };
        let mark = (size * 0.13).max(5.0);
        let ring = size * 0.5 * (DIAL - 0.09);
        door = door.child(
            el().key(format!("door-attention-{place}"))
                .absolute(size / 2.0 - mark / 2.0, size / 2.0 - ring - mark / 2.0)
                .size(mark, mark)
                .radius(mark / 2.0)
                .fill(color)
                .border((mark * 0.18).max(1.0), t.kit.c.bg_base.alpha(0.7))
                .a11y_hidden()
                .transition(ui::appear()),
        );
    }
    door.role("img")
        .label(format!("Stahl Capital. {}", describe(count)))
}

/// The large door on the welcome screen. It says where it is, so that opening a portfolio can fly
/// it from here to the mark.
pub fn welcome(s: &State, t: &Theme, size: f32) -> El {
    // It leaves at once: the flying door takes its place in the same frame.
    vault(s, t, size, "welcome")
        .transition(Transition::none())
        .on_layout(|s: &mut State, rect| {
            if s.welcome_door != Some(rect) {
                s.welcome_door = Some(rect);
            }
        })
}

/// The mark's place: in the sidebar's header, or the phone's top bar. It holds the mark, except
/// while the welcome door is flying in to take its place.
pub fn mark(s: &State, t: &Theme, size: f32, place: &str) -> El {
    el().key(format!("door-slot-{place}"))
        .size(size, size)
        .on_layout(|s: &mut State, rect| {
            if s.mark_door != Some(rect) {
                s.mark_door = Some(rect);
            }
        })
        .children(
            s.door_flight
                .is_none()
                .then(|| vault(s, t, size, place).transition(Transition::none())),
        )
}

/// The welcome door on its way to the mark, over everything: it starts where the welcome door
/// was, at its size, and springs to the mark's place and size, its dial turning as it lands. It
/// is the large door drawn scaled down, so it stays sharp all the way. When it has landed and the
/// dial has settled, the mark takes its place, at the same angle, and nothing jumps. Nothing
/// covers the page. Under reduced motion it lands at once.
pub fn flight(s: &State, t: &Theme) -> Option<El> {
    s.door_flight?;
    let from = s.welcome_door?;
    let size = from[2];
    // Drawn first exactly where the welcome door was; then, released, it springs to the mark.
    let (at, scale) = match s.mark_door.filter(|_| s.door_released) {
        Some(to) => (
            [
                to[0] + to[2] / 2.0 - size / 2.0,
                to[1] + to[3] / 2.0 - size / 2.0,
            ],
            to[2] / size,
        ),
        None => ([from[0], from[1]], 1.0),
    };
    let door = vault(s, t, size, "flying")
        .absolute(at[0], at[1])
        .scale(scale)
        .motion(DOOR_LAND)
        .scale_motion(DOOR_LAND)
        .transition(Transition::none());
    Some(
        el().key("door-flight-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .transition(Transition::none())
            .a11y_hidden()
            .child(door),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dial_rests_where_it_started() {
        for turns in [0, 1, 7, 1023, 1024, 5000] {
            let angle = dial_angle(turns);
            assert!((angle / TAU - (angle / TAU).round()).abs() < 1e-4);
        }
        assert!(dial_angle(1) > dial_angle(0));
    }

    #[test]
    fn the_door_says_how_many_things_need_attention() {
        assert_eq!(describe(0), "Nothing needs attention");
        assert_eq!(describe(1), "1 thing needs attention");
        assert_eq!(describe(3), "3 things need attention");
    }
}
