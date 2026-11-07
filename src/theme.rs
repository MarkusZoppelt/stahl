//! Steel and paper: a dark wall and a light letterhead, as on the landing page. A rune-kit `Kit` for
//! the components, and the few colours Stahl's own views paint with.

use rune::{Color, Family, TextStyle, theme::hex};
use rune_kit::{Kit, PaletteFocus, PaletteSeed};

/// The fonts the app draws with: Inter for reading, Newsreader for the few things worth announcing.
pub const FONTS: &[&[u8]] = &[
    include_bytes!("../assets/fonts/Inter.ttf"),
    include_bytes!("../assets/fonts/Newsreader.ttf"),
];

const SERIF: Family = Family::Named("Newsreader");

/// The numerals on the hero, with tabular figures so a rolling digit doesn't shift its neighbours.
pub const HERO: TextStyle = TextStyle::new(60.0, 340.0)
    .family(SERIF)
    .lh(66.0)
    .tracking(-1.2)
    .features("'tnum' 1");
/// A page's name, at the top of it.
pub const PAGE_TITLE: TextStyle = TextStyle::new(32.0, 340.0)
    .family(SERIF)
    .lh(38.0)
    .tracking(-0.5);
/// A holding's name, in the details pane.
pub const HOLDING_TITLE: TextStyle = TextStyle::new(24.0, 360.0)
    .family(SERIF)
    .lh(30.0)
    .tracking(-0.3);
/// The welcome screen's one sentence.
pub const STATEMENT: TextStyle = TextStyle::new(46.0, 340.0)
    .family(SERIF)
    .lh(52.0)
    .tracking(-0.9);
/// A section's name, on a page.
pub const SECTION: TextStyle = TextStyle::new(18.0, 400.0)
    .family(SERIF)
    .lh(24.0)
    .tracking(-0.1);
pub const WORDMARK: TextStyle = TextStyle::new(16.0, 420.0).family(SERIF).lh(20.0);
pub const FIGURE: TextStyle = TextStyle::new(24.0, 360.0)
    .family(SERIF)
    .lh(30.0)
    .tracking(-0.3)
    .features("'tnum' 1");
pub const NUMBER: TextStyle = TextStyle::new(13.0, 500.0).features("'tnum' 1");

/// Colours for allocation charts, cycled by slice.
const CHART: [&str; 8] = [
    "#7aa2f7", "#73daca", "#bb9af7", "#e0af68", "#7dcfff", "#f7768e", "#9ece6a", "#ff9e64",
];

/// The common classes get a colour each. Anything else lands on a stable one by name.
fn class_slot(asset_class: &str) -> usize {
    let name = asset_class.trim().to_lowercase();
    match name.as_str() {
        "stock" | "stocks" | "equity" | "equities" => 0,
        "cash" => 1,
        "crypto" => 2,
        "bond" | "bonds" => 3,
        "gold" => 4,
        "commodity" | "commodities" => 5,
        "real estate" => 6,
        _ => name.bytes().map(usize::from).sum::<usize>() % CHART.len(),
    }
}

pub struct Theme {
    pub kit: Kit,
    pub dark: bool,
    pub card: Color,
    pub sunken: Color,
    pub up: Color,
    pub down: Color,
    pub warn: Color,
    pub info: Color,
    pub accent: Color,
}

fn color(value: &str) -> Color {
    hex(value).unwrap_or(Color::WHITE)
}

impl Theme {
    pub fn new(dark: bool) -> Theme {
        let (bg, fg, accent) = if dark {
            ("#0a0f13", "#e9eef1", "#c7d0d4")
        } else {
            ("#e9e6dd", "#1b2329", "#26333b")
        };
        let accent = color(accent);
        let (up, down, warn, info) = if dark {
            ("#73daca", "#f7768e", "#e0af68", "#7dcfff")
        } else {
            ("#1f7a6c", "#b5405a", "#8f5e15", "#0f4b6e")
        };
        let mut kit = Kit::from_palette_focus(
            PaletteSeed {
                bg: color(bg),
                fg: color(fg),
                accent,
            },
            PaletteFocus::Custom(color(up)),
        );
        kit.radius.sm = 6.0;
        let (card, sunken) = if dark {
            ("#121a20", "#0d1317")
        } else {
            ("#f1eee6", "#dedbd1")
        };
        Theme {
            kit,
            dark,
            card: color(card),
            sunken: color(sunken),
            up: color(up),
            down: color(down),
            warn: color(warn),
            info: color(info),
            accent,
        }
    }

    /// An asset class keeps its colour on every page, whatever else is on that page.
    pub fn class_color(&self, asset_class: &str) -> Color {
        color(CHART[class_slot(asset_class)])
    }

    /// Green for gains and flat, red for losses.
    pub fn change(&self, value: f64) -> Color {
        if value >= 0.0 { self.up } else { self.down }
    }

    /// As [`Theme::change`], but a figure we don't have is neither good nor bad.
    pub fn optional_change(&self, value: Option<f64>) -> Color {
        value.map_or(self.kit.c.text_muted, |value| self.change(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_colours_stay_readable_on_both_walls() {
        let contrast = |a: Color, b: Color| {
            let (a, b) = (a.luminance(), b.luminance());
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        };
        for dark in [true, false] {
            let t = Theme::new(dark);
            for ink in [t.up, t.down, t.warn, t.info, t.kit.c.text_muted] {
                assert!(contrast(ink, t.kit.c.bg_base) >= 3.0, "dark: {dark}");
                assert!(contrast(ink, t.card) >= 3.0, "dark: {dark}");
            }
        }
    }

    #[test]
    fn common_classes_never_share_a_colour() {
        let slots = ["Stocks", "Cash", "Crypto", "Bonds", "Gold"].map(class_slot);
        for (index, slot) in slots.iter().enumerate() {
            assert!(!slots[index + 1..].contains(slot), "{slots:?}");
        }
    }

    #[test]
    fn a_class_keeps_its_colour_however_it_is_written() {
        assert_eq!(class_slot("Stock"), class_slot("stocks"));
        assert_eq!(class_slot("Art"), class_slot(" art "));
        assert!(class_slot("Anything at all") < CHART.len());
    }
}
