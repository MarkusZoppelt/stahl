//! The vault door as a living mark: still at rest, its dial turns once when prices land, it
//! carries a single mark when something needs attention, and it flies from the welcome screen
//! into the sidebar when a portfolio opens.
//!
//! It is built from the shader-lit picture of the door (`ui::door`) and Rune's own transforms:
//! the dial is the same picture clipped to the dial's disc and turned on a spring, so at rest it
//! draws exactly the icon, and nothing animates while the app is idle.

use std::f32::consts::TAU;

use portfolio_rs::mcp::dto::Severity;
use rune::{Beat, El, Sequence, Spring, SpringPair, Transition, el, row};

use crate::{state::State, theme::Theme};

use super::ui;

/// The dial's outer edge, as a fraction of the door's radius: the fourth ring of the icon, with the
/// graduations (`EDGE[4]` in `site/door.js`).
const DIAL: f32 = 0.7969;

/// A heavy dial: it takes its time and settles with a small give, as steel would.
pub const DIAL_TURN: Spring = Spring::new(1.1, 0.12);

/// How many things need the owner's attention, as the Overview's "Needs attention" list counts
/// them: warnings and critical findings, not information.
pub fn attention(s: &State) -> usize {
    s.findings
        .iter()
        .filter(|finding| !matches!(finding.severity, Severity::Info))
        .count()
}

/// Whether one of the things needing attention is a failure rather than a warning.
fn attention_is_critical(s: &State) -> bool {
    s.findings
        .iter()
        .any(|finding| matches!(finding.severity, Severity::Critical))
}

/// The angle the dial rests at: one full turn per price refresh, so every rest looks the same and
/// the spring between them is the turn.
pub fn dial_angle(turns: u64) -> f32 {
    // Wrapped, so a long session doesn't lose precision; the spring only ever sees one turn.
    (turns % 1024) as f32 * TAU
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
                .rotate(dial_angle(s.price_updates))
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

/// The door holds closed for a moment, then fades as its halves part.
const OPENING: &[Beat] = &[Beat::new(0.0), Beat::new(0.2), Beat::new(0.75).opacity(0.0)];

/// How far each half slides as the door opens, as a fraction of the door's size.
const PARTING: f32 = 0.36;

/// The door opening into a portfolio: when one opens, a large door stands over the page and its
/// two halves part along the seam, once. Keyed by the document, so it plays again only for a
/// different portfolio or after closing one, and never on moving between pages. Under reduced
/// motion it lands open at once: nothing shows.
pub fn opening(s: &State, size: f32) -> El {
    let document = s
        .open
        .workspace_dir
        .as_deref()
        .or(s.open.file_path.as_deref())
        .unwrap_or_default();
    let half = |left: bool| {
        let travel = size * PARTING * if left { -1.0 } else { 1.0 };
        el().size(size / 2.0, size)
            .clip_rounded(0.0)
            // At rest it is parted; it enters from closed, so the spring is the opening.
            .offset(travel, 0.0)
            .transition(
                Transition::offset(-travel, 0.0).springs(SpringPair::new(DOOR_PARTS, DOOR_PARTS)),
            )
            .child(ui::door(size).absolute(if left { 0.0 } else { -size / 2.0 }, 0.0))
    };
    row()
        .key(format!("door-opening-{document}"))
        .absolute(0.0, 0.0)
        .w_full()
        .h_full()
        .center()
        .a11y_hidden()
        .sequence(Sequence::new(OPENING))
        .child(half(true))
        .child(half(false))
}

/// The halves part slowly at first and glide to rest.
pub const DOOR_PARTS: Spring = Spring::new(0.75, 0.0);

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
