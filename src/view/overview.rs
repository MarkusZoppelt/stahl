//! The Overview's own parts: what changed since the owner last looked, and what to look at.

use rune::{Cx, El, col, icon, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize};

use crate::{
    agenda::{Calm, Item, Weight},
    icons,
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

fn agenda_item(index: usize, item: &Item, t: &Theme) -> El {
    let k = &t.kit;
    let color = match item.weight {
        Weight::Total | Weight::Critical => t.down,
        Weight::Warning => t.warn,
        Weight::Notice => t.info,
    };
    let action = item.action;
    row()
        .key(format!("agenda-{}", item.id))
        .w_full()
        .items_start()
        .gap(12.0)
        .py(12.0)
        // The ledger's own rule heads the first.
        .children((index > 0).then(|| ui::rule(t).absolute(0.0, 0.0)))
        .child(ui::dot(color).mt(6.0))
        .child(
            col()
                .grow(1.0)
                .w(0.0)
                .min_w(0.0)
                .gap(2.0)
                .child(
                    text(item.title.clone())
                        .font(k.ty.ui_medium)
                        .color(k.c.text_base)
                        .wrap(),
                )
                .children(
                    item.detail
                        .clone()
                        .map(|detail| ui::caption(detail, t).wrap()),
                ),
        )
        .child(
            Button::new(action.label())
                .variant(ButtonVariant::Outline)
                .control_size(ControlSize::Small)
                .on_press(move |s: &mut State| action.perform(s))
                .build(k)
                .key(format!("agenda-action-{}", item.id)),
        )
}

/// What deserves a look, most important first, each with the one button that deals with it.
pub fn agenda(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let items = s.agenda();
    let ledger = ui::ledger(t)
        .key("agenda")
        .child(ui::title("What to look at", t));
    if !items.is_empty() {
        return ledger.child(
            col().w_full().children(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| agenda_item(index, item, t)),
            ),
        );
    }
    let calm = match s.agenda_calm() {
        Calm::Checking => ui::muted("Checking against your policy…", t),
        Calm::Clear(words) => row()
            .key("agenda-clear")
            .w_full()
            .items_start()
            .gap(8.0)
            .child(icon(&icons::CHECK_CIRCLE, 16.0).color(t.up).mt(2.0))
            .child(
                text(words)
                    .font(k.ty.ui)
                    .color(k.c.text_muted)
                    .wrap()
                    .shrink(),
            ),
    };
    ledger.child(calm)
}
