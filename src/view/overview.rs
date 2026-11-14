//! The Overview's own parts: what changed since the owner last looked, and what to look at.

use rune::{Cx, El, col, row, text};

use crate::{
    state::State,
    statement::{Figure, Tone},
    theme::{FACT, LEDE, LEDE_COMPACT, Theme},
    view::ui,
};

/// A supporting figure: quieter than the hero's, so the sentence and the total lead.
fn figure(figure: &Figure, t: &Theme, stack_note: bool) -> El {
    let k = &t.kit;
    let color = match figure.tone {
        Tone::Up => t.up,
        Tone::Down => t.down,
        Tone::Plain => k.c.text_base,
    };
    let value = text(figure.value.clone())
        .font(FACT)
        .color(color)
        .selectable(true);
    // A number sits beside the value. A sentence goes under it unless the page is wide: beside
    // "None" in a column it truncates to nothing.
    let note = figure.note.clone();
    let words = stack_note
        && note
            .as_ref()
            .is_some_and(|note| note.chars().any(|c| c.is_ascii_alphabetic()));
    let body = if words {
        col()
            .gap(2.0)
            .min_w(0.0)
            .child(value)
            .children(note.map(|note| ui::caption(note, t).wrap()))
    } else {
        row()
            .center_y()
            .gap(8.0)
            .min_w(0.0)
            .child(value)
            .children(note.map(|note| ui::caption(note, t).truncate().shrink()))
    };
    col()
        .gap(4.0)
        .min_w(0.0)
        .child(ui::caption(figure.label.clone(), t))
        .child(body)
}

/// The figures behind the statement's sentences, or `None` when it has none.
pub fn statement_figures(s: &State, t: &Theme, cx: &Cx<State>) -> Option<El> {
    let statement = s.statement()?;
    if statement.figures.is_empty() {
        return None;
    }
    let room = ui::room(s, cx);
    let figures = statement
        .figures
        .iter()
        .map(|each| figure(each, t, room != ui::Room::Wide));
    Some(match room {
        // Under the sentence, not stretched across the page, so they read as its evidence.
        ui::Room::Wide => row()
            .key("statement-figures")
            .items_start()
            .gap(32.0)
            .max_w(760.0)
            .children(figures),
        _ => ui::columns(room, [3, 3, 2], 16.0)
            .key("statement-figures")
            .children(figures),
    })
}

/// One or two sentences on what changed since the last look. The figures behind them follow
/// them, except on a phone, where the page puts them after what there is to do.
pub fn statement(s: &State, t: &Theme, cx: &Cx<State>) -> Option<El> {
    let statement = s.statement()?;
    let narrow = ui::room(s, cx) == ui::Room::Narrow;
    let figures = (!narrow).then(|| statement_figures(s, t, cx)).flatten();
    Some(
        col()
            .key("statement")
            .w_full()
            .gap(if narrow { 12.0 } else { 16.0 })
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

/// What the bank has prepared for signature, and what needs doing but not signing.
pub fn agenda(s: &State, t: &Theme, narrow: bool, reduced: bool) -> El {
    super::matters::section(s, t, narrow, reduced)
}
