//! Tokyo Night, dark and light.
//!
//! `assets/stahl-theme.json` styles the component library's widgets, and
//! [`Palette`] holds the tokens Stahl's own layouts paint with.

use std::rc::Rc;

use gpui_kit::{
    App, Hsla, Rgba,
    component::{Theme, ThemeConfig, ThemeMode as ComponentMode, ThemeSet},
    rgb,
};
use portfolio_rs::config::ThemeMode;
use serde_json::from_str;

const THEMES: &str = include_str!("../assets/stahl-theme.json");

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Rgba,
    pub bg_secondary: Rgba,
    pub bg_tertiary: Rgba,
    pub fg: Rgba,
    pub fg_strong: Rgba,
    pub fg_muted: Rgba,
    pub fg_subtle: Rgba,
    pub border: Rgba,
    pub accent: Rgba,
    pub accent_fg: Rgba,
    pub success: Rgba,
    pub danger: Rgba,
    pub warning: Rgba,
    pub info: Rgba,
}

fn dark() -> Palette {
    Palette {
        bg: rgb(0x1a1b26),
        bg_secondary: rgb(0x1f2233),
        bg_tertiary: rgb(0x2a2e3f),
        fg: rgb(0xa9b1d6),
        fg_strong: rgb(0xc0caf5),
        fg_muted: rgb(0x787c99),
        fg_subtle: rgb(0x565f89),
        border: rgb(0x2a2e3f),
        accent: rgb(0x7aa2f7),
        accent_fg: rgb(0x1a1b26),
        success: rgb(0x73daca),
        danger: rgb(0xf7768e),
        warning: rgb(0xe0af68),
        info: rgb(0x7dcfff),
    }
}

fn light() -> Palette {
    Palette {
        bg: rgb(0xe9e9ee),
        bg_secondary: rgb(0xf4f4f7),
        bg_tertiary: rgb(0xdfe0e7),
        fg: rgb(0x343b58),
        fg_strong: rgb(0x1f2440),
        fg_muted: rgb(0x4e5579),
        fg_subtle: rgb(0x8b8e9c),
        border: rgb(0xd5d6db),
        accent: rgb(0x34548a),
        accent_fg: rgb(0xffffff),
        success: rgb(0x33635c),
        danger: rgb(0x8c4351),
        warning: rgb(0x8f5e15),
        info: rgb(0x0f4b6e),
    }
}

/// Colors for allocation charts, cycled by slice.
pub const CHART_COLORS: [u32; 8] = [
    0x7aa2f7, 0x73daca, 0xbb9af7, 0xe0af68, 0x7dcfff, 0xf7768e, 0x9ece6a, 0xff9e64,
];

pub fn chart_color(index: usize) -> Rgba {
    rgb(CHART_COLORS[index % CHART_COLORS.len()])
}

/// The soft fill behind a status color.
pub fn tint(color: Rgba) -> Hsla {
    Hsla::from(color).opacity(0.12)
}

pub trait ActivePalette {
    fn palette(&self) -> Palette;
}

impl ActivePalette for App {
    fn palette(&self) -> Palette {
        if Theme::global(self).is_dark() {
            dark()
        } else {
            light()
        }
    }
}

/// Install the Stahl themes. Call once, after the component library's `init`.
pub fn init(mode: ThemeMode, cx: &mut App) {
    let set: ThemeSet = from_str(THEMES).expect("bundled theme parses");
    let find = |name: &str| -> Rc<ThemeConfig> {
        let theme = set.themes.iter().find(|theme| theme.name.as_ref() == name);
        Rc::new(theme.expect("bundled theme exists").clone())
    };
    let (dark, light) = (find("Stahl Dark"), find("Stahl Light"));
    Theme::update(cx, |theme| {
        theme.dark_theme = dark;
        theme.light_theme = light;
    });
    apply(mode, cx);
}

pub fn apply(mode: ThemeMode, cx: &mut App) {
    let mode = match mode {
        ThemeMode::Dark => ComponentMode::Dark,
        ThemeMode::Light => ComponentMode::Light,
    };
    Theme::change(mode, None, cx);
}
