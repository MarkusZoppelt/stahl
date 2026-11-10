//! The Overview's own parts: what changed since the owner last looked.

use rune::{Cx, El, col, row, text};

use crate::{
    state::State,
    statement::{Figure, Tone},
    theme::{LEDE, LEDE_COMPACT, Theme},
    view::ui,
};

fn figure(figure: &Figure, t: &Theme) -> El {
    let color = match figure.tone {
        Tone::Up => Some(t.up),
        Tone::Down => Some(t.down),
        Tone::Plain => None,
    };
    ui::stat(figure.label.clone(), figure.value.clone(), color, t)
        .children(figure.note.clone().map(|note| ui::caption(note, t)))
}

/// One or two sentences on what changed since the last look, and the figures behind them.
pub fn statement(s: &State, t: &Theme, cx: &Cx<State>) -> Option<El> {
    let statement = s.statement()?;
    let room = ui::room(s, cx);
    let narrow = room == ui::Room::Narrow;
    let figures = (!statement.figures.is_empty()).then(|| {
        let figures = statement.figures.iter().map(|each| figure(each, t));
        match room {
            ui::Room::Wide => row().gap(40.0).children(figures),
            _ => ui::columns(room, [4, 4, 2], 16.0).children(figures),
        }
    });
    Some(
        col()
            .key("statement")
            .w_full()
            .gap(18.0)
            .child(
                text(statement.text)
                    .key("statement-text")
                    .font(if narrow { LEDE_COMPACT } else { LEDE })
                    .color(t.kit.c.text_base)
                    .wrap()
                    .selectable(true)
                    .max_w(760.0),
            )
            .children(figures),
    )
}
