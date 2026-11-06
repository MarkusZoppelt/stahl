//! Tokyo Night, dark and light: a rune-kit `Kit` for the components, and the few
//! colours Stahl's own views paint with.

use rune::{Color, TextStyle, theme::hex};
use rune_kit::{Kit, PaletteSeed};

/// The numerals on the hero, with tabular figures so a rolling digit doesn't shift its neighbours.
pub const HERO: TextStyle = TextStyle::new(44.0, 700.0)
    .lh(50.0)
    .tracking(-0.8)
    .features("'tnum' 1");
pub const FIGURE: TextStyle = TextStyle::new(19.0, 600.0).lh(24.0).features("'tnum' 1");
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
            ("#1a1b26", "#c0caf5", "#7aa2f7")
        } else {
            ("#f1f1f5", "#262b45", "#34548a")
        };
        let accent = color(accent);
        let mut kit = Kit::from_palette(PaletteSeed {
            bg: color(bg),
            fg: color(fg),
            accent,
        });
        kit.radius.sm = 6.0;
        let (card, sunken, up, down, warn, info) = if dark {
            (
                "#1f2233", "#16171f", "#73daca", "#f7768e", "#e0af68", "#7dcfff",
            )
        } else {
            (
                "#fbfbfd", "#e6e6ec", "#2b7a6f", "#b5405a", "#8f5e15", "#0f4b6e",
            )
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
