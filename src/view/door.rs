//! The vault door as a living mark: still at rest, its tumblers turn once when prices land, it
//! carries a single mark when something needs attention, and a portfolio opens through it, with
//! the landing page's sequence played over the page. Nothing loops while the app is idle.
//!
//! A window with a compositor draws the door with the shader (`crate::fx`); the picture
//! (`ui::door`) and its turned dial stand in for tests, which never draw, and for iOS.

use std::f32::consts::{PI, TAU};

use rune::{Drag, El, Key, KeyEvent, Phase, Spring, Transition, Tx, el};

use crate::{agenda::Weight, state::State, theme::Theme};

use super::ui;

/// The dial's outer edge, as a fraction of the door's radius: the fourth ring of the icon, with the
/// graduations (`EDGE[4]` in `site/door.js`).
const DIAL: f32 = 0.7969;

/// One notch of the dial: the step an arrow key turns it by.
pub const NOTCH: f32 = TAU / 24.0;

/// A heavy dial: it takes its time and settles with a small give, as steel would.
pub const DIAL_TURN: Spring = Spring::new(1.1, 0.12);

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

/// A hand on the dial: it follows the pointer's angle around the door's centre, the short way
/// round, and stays where it is left. The compositor gives it the weight.
fn turn_dial(s: &mut State, drag: &mut Drag, _tx: &mut Tx) {
    let centre = [drag.w / 2.0, drag.h / 2.0];
    let angle = (drag.y - centre[1]).atan2(drag.x - centre[0]);
    match drag.phase {
        Phase::Down => s.dial_drag = Some(angle),
        Phase::Move => {
            if let Some(last) = s.dial_drag {
                let mut delta = angle - last;
                if delta > PI {
                    delta -= TAU;
                } else if delta < -PI {
                    delta += TAU;
                }
                s.dial_hand += delta;
            }
            s.dial_drag = Some(angle);
        }
        Phase::Up => s.dial_drag = None,
    }
}

/// The keyboard's way to turn the dial: one notch per arrow.
fn turn_key(s: &mut State, event: &KeyEvent, _tx: &mut Tx) -> bool {
    match event.key {
        Key::Left => s.dial_hand -= NOTCH,
        Key::Right => s.dial_hand += NOTCH,
        _ => return false,
    }
    true
}

/// The magic of the living door at `size`: either the shader's slot, or the picture with its
/// turned dial for a window without a compositor (tests, which never draw, and iOS).
/// `place` keeps the dial's key apart where two doors are on screen.
pub fn vault(s: &State, t: &Theme, size: f32, place: &str) -> El {
    let count = if s.has_portfolio() { attention(s) } else { 0 };
    let mut door = el()
        .key(format!("door-{place}"))
        .size(size, size)
        // Round, so the focus ring follows the door and not a box around it.
        .radius(size / 2.0)
        .child(body(s, size, place));
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
    // The door can be turned by hand, for no reason but that it is a door. It is a control of
    // its own, so the press never becomes a window drag or a page press. On a phone the top
    // bar's door already turns at a tap, and that stays its one gesture.
    if place != "phone" {
        door = door
            .focusable(true)
            .on_drag(turn_dial)
            .on_key(turn_key)
            .help("Drag to turn the dial, or press the left and right arrow keys");
    }
    door.role("img")
        .label(format!("Stahl Capital. {}", describe(count)))
}

/// What the door is drawn with: the shader, or the picture and its turned dial.
fn body(s: &State, size: f32, place: &str) -> El {
    let dragging = s.dial_drag.is_some();
    if s.shaders {
        // The welcome screen's door is the large door; the mark has its own light and its own
        // tumblers.
        if place == "welcome" {
            crate::fx::door(size, dial_turns(s), s.door_busy, s.dial_hand, dragging)
        } else {
            crate::fx::mark(size, dial_turns(s), s.door_busy, s.dial_hand, dragging)
        }
    } else {
        let dial = size * DIAL;
        let inset = (size - dial) / 2.0;
        el().size(size, size).child(ui::door(size)).child(
            // The dial: the same picture, clipped to the dial's disc, so at rest it is
            // invisible.
            el().key(format!("door-dial-{place}"))
                .absolute(inset, inset)
                .size(dial, dial)
                .radius(dial / 2.0)
                .clip_rounded(dial / 2.0)
                .a11y_hidden()
                .rotate(dial_angle(dial_turns(s)) + s.dial_hand)
                .rotate_motion(DIAL_TURN)
                .child(ui::door(size).absolute(-inset, -inset)),
        )
    }
}

/// The large door on the welcome screen. It says where it is, so the opening can start from
/// there when a portfolio opens.
pub fn welcome(s: &State, t: &Theme, size: f32) -> El {
    // It leaves at once: the opening's door takes its place in the same frame.
    vault(s, t, size, "welcome")
        .transition(Transition::none())
        .on_layout(|s: &mut State, rect| {
            if s.welcome_door != Some(rect) {
                s.welcome_door = Some(rect);
            }
        })
}

/// The mark's place: in the sidebar's header, or the phone's top bar.
pub fn mark(s: &State, t: &Theme, size: f32, place: &str) -> El {
    vault(s, t, size, place)
        .transition(Transition::none())
        .on_layout(|s: &mut State, rect| {
            // A refresh's pulse of light leaves from here.
            if s.door_mark != Some(rect) {
                s.door_mark = Some(rect);
            }
        })
}

/// The door opening onto a portfolio: one full-window slot over the page, which the compositor
/// paints with the landing page's sequence. Nothing covers the page for long: the halves part, and
/// the page is there, laid out, underneath. Under reduced motion there is no sequence at all.
pub fn opening(s: &State) -> Option<El> {
    // Without a compositor there is no door to open: tests never draw, and iOS keeps the picture.
    if !s.shaders {
        return None;
    }
    let opening = s.opening?;
    Some(crate::fx::opening(
        opening.started,
        opening.center,
        opening.radius,
    ))
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
