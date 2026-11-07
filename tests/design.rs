//! Keeps the design language in one place. Colours, type, shadows and gradients belong to
//! `src/theme.rs` and the primitives in `src/view/ui.rs`; the pages compose those. See DESIGN.md.

use std::{fs, path::Path};

/// What a page must not reach for. Each has a token or a primitive instead.
const OFF_LIMITS: &[(&str, &str)] = &[
    ("TextStyle::new", "a type style from theme.rs"),
    ("Family::", "a type style from theme.rs"),
    ("hex(", "a colour from Theme"),
    ("Color::BLACK", "a colour from Theme"),
    ("Color::WHITE", "a colour from Theme"),
    (".shadow(", "ui::floating"),
    ("gradient(", "ui::wash, or nothing"),
    ("backdrop_blur(", "ui::floating"),
    ("include_bytes!", "an asset loaded in ui.rs or theme.rs"),
];

/// Corner radii on the pages: hairline bars, small dots, controls, panels.
const RADII: &[f32] = &[0.0, 1.0, 2.0, 3.0, 6.0, 8.0, 12.0];

fn pages() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/view");
    let mut files: Vec<_> = fs::read_dir(&dir)
        .expect("src/view exists")
        .map(|entry| entry.expect("readable entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .filter(|path| path.file_name().is_some_and(|name| name != "ui.rs"))
        .map(|path| {
            let source = fs::read_to_string(&path).expect("readable source");
            (path.display().to_string(), source)
        })
        .collect();
    files.sort();
    assert!(files.len() >= 5, "found the pages in {}", dir.display());
    files
}

#[test]
fn pages_use_the_theme_for_colour_type_and_depth() {
    let mut problems = Vec::new();
    for (path, source) in pages() {
        for (number, line) in source.lines().enumerate() {
            for (pattern, instead) in OFF_LIMITS {
                if line.contains(pattern) {
                    problems.push(format!("{path}:{}: `{pattern}`; use {instead}", number + 1));
                }
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}

#[test]
fn pages_round_corners_from_the_scale() {
    let mut problems = Vec::new();
    for (path, source) in pages() {
        for (number, line) in source.lines().enumerate() {
            let mut rest = line;
            while let Some(at) = rest.find(".radius(") {
                rest = &rest[at + ".radius(".len()..];
                let argument = rest.split(')').next().unwrap_or("");
                // A computed radius (a pill's half height, say) is its own business.
                if let Ok(radius) = argument.trim().parse::<f32>()
                    && !RADII.contains(&radius)
                {
                    problems.push(format!(
                        "{path}:{}: radius {radius}; use one of {RADII:?}",
                        number + 1
                    ));
                }
            }
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
