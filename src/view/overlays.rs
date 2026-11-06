//! What floats above the window: the command palette, dialogs, toasts, and the parse failure.

use rune::{
    Color, El, Icon, Key, ScrollSpec, TextEdit, Transition, Tx, col, el, icon, row, text,
    tok::motion::LEAD,
};
use rune_kit::{Button, ButtonVariant, Dialog, Keybind, TextInput, Toast, ToastRegion};

use crate::{
    icons,
    state::{Command, Confirm, Page, State},
    theme::Theme,
    view::{page_icon, portfolio::matches_search, ui},
};

pub fn build(s: &State, t: &Theme, layer: &mut Vec<El>) {
    if s.palette.is_some() {
        palette(s, t, layer);
    }
    if s.form.is_some() {
        form(s, t, layer);
    }
    if let Some(confirm) = &s.confirm {
        confirmation(confirm, t, layer);
    }
    if let Some((message, _)) = &s.toast {
        layer.push(
            ToastRegion::new()
                .toast(
                    Toast::new("toast", message.clone())
                        .icon(&t.kit, &icons::CHECK_CIRCLE)
                        .build(&t.kit),
                )
                .build(&t.kit),
        );
    }
}

/// Shown instead of an empty portfolio when the file does not parse.
pub fn parse_failure(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let Some(problem) = &s.open.parse_error else {
        return col();
    };
    col().w_full().h_full().center().padding(24.0).child(
        ui::card(t)
            .w_full()
            .max_w(640.0)
            .child(
                row()
                    .center_y()
                    .gap(8.0)
                    .center_y()
                    .child(icon(&icons::TRIANGLE_ALERT, 20.0).color(t.warn))
                    .child(ui::title("This portfolio can't be read", t)),
            )
            .child(ui::muted(
                "Nothing has been changed. Fix the file in your editor, then reload.",
                t,
            ))
            .child(
                col()
                    .w_full()
                    .gap(4.0)
                    .padding(12.0)
                    .radius(8.0)
                    .fill(t.warn.alpha(0.12))
                    .child(
                        text(format!("Line {}, column {}", problem.line, problem.column))
                            .font(k.ty.ui_medium)
                            .color(t.warn),
                    )
                    .child(
                        text(problem.reason.clone())
                            .font(k.ty.ui)
                            .color(k.c.text_base)
                            .wrap()
                            .selectable(true),
                    )
                    .child(ui::caption(problem.file.clone(), t).truncate_middle()),
            )
            .child(
                row()
                    .center_y()
                    .gap(8.0)
                    .child(
                        Button::new("Reload")
                            .variant(ButtonVariant::Submit)
                            .icon(&icons::REFRESH)
                            .on_press(|s: &mut State| s.reload())
                            .build(k)
                            .key("reload-file"),
                    )
                    .child(
                        Button::new("Open Another File…")
                            .on_press_tx(|s: &mut State, tx: &mut Tx| s.ask_for_file(tx))
                            .build(k)
                            .key("open-other"),
                    ),
            ),
    )
}

pub struct Entry {
    pub label: String,
    pub icon: &'static Icon,
    pub keys: Vec<String>,
    pub command: Command,
    pub group: &'static str,
}

fn page_entry(page: Page) -> Entry {
    Entry {
        label: page.label().to_string(),
        icon: page_icon(page),
        keys: page
            .shortcut()
            .map(|key| ui::chord_keys(false, &key.to_string()))
            .unwrap_or_default(),
        command: Command::Go(page),
        group: "Go to",
    }
}

/// Every command, then what's in the portfolio, filtered by every word of the query.
pub fn entries(s: &State) -> Vec<Entry> {
    let action = |label: &str, icon: &'static Icon, keys: Option<(bool, &str)>, command| Entry {
        label: label.to_string(),
        icon,
        keys: keys.map_or_else(Vec::new, |(shift, key)| ui::chord_keys(shift, key)),
        command,
        group: "Actions",
    };
    let mut all: Vec<Entry> = Page::PORTFOLIO
        .into_iter()
        .chain(Page::INSIGHTS)
        .chain([Page::Settings])
        .map(page_entry)
        .collect();
    if s.is_workspace() {
        all.extend(Page::PLAN.map(page_entry));
    }
    all.extend([
        action(
            "Refresh Prices",
            &icons::REFRESH,
            Some((false, "R")),
            Command::Refresh,
        ),
        action(
            "Add Position",
            &icons::PLUS,
            Some((false, "N")),
            Command::AddPosition,
        ),
        action("Save", &icons::SAVE, Some((false, "S")), Command::Save),
        action(
            "Open Portfolio File…",
            &icons::FILE_BRACES,
            Some((false, "O")),
            Command::OpenFile,
        ),
        action(
            "Open Workspace…",
            &icons::FOLDER_OPEN,
            Some((true, "O")),
            Command::OpenWorkspace,
        ),
        action(
            "Hide or Show Amounts",
            &icons::EYE_OFF,
            Some((true, "H")),
            Command::Privacy,
        ),
        action(
            "Toggle Light/Dark Theme",
            &icons::MOON,
            None,
            Command::Theme,
        ),
        action(
            "Toggle Sidebar",
            &icons::LAYOUT_DASHBOARD,
            Some((true, "B")),
            Command::Sidebar,
        ),
        action(
            "Copy Briefing as Markdown",
            &icons::COPY,
            None,
            Command::CopyBriefing,
        ),
    ]);
    let query = s
        .palette
        .as_ref()
        .map_or(String::new(), |palette| palette.query.text.to_lowercase());
    let mut found: Vec<Entry> = all
        .into_iter()
        .filter(|entry| {
            let label = entry.label.to_lowercase();
            query.split_whitespace().all(|word| label.contains(word))
        })
        .collect();
    // Holdings only when asked for: they'd bury the commands otherwise.
    if !query.trim().is_empty() {
        found.extend(
            s.positions
                .iter()
                .filter(|position| matches_search(position, &query))
                .map(|position| Entry {
                    label: position
                        .name
                        .clone()
                        .or_else(|| position.ticker.clone())
                        .unwrap_or_else(|| "Unknown".to_string()),
                    icon: &icons::WALLET,
                    keys: Vec::new(),
                    command: Command::Reveal(position.id),
                    group: "Holdings",
                }),
        );
    }
    found
}

fn palette(s: &State, t: &Theme, layer: &mut Vec<El>) {
    let k = &t.kit;
    let Some(palette) = &s.palette else {
        return;
    };
    let found = entries(s);
    let selected = palette.selected.min(found.len().saturating_sub(1));
    let input = TextInput::new(&palette.query)
        .placeholder("Search pages, actions and holdings…")
        .leading_icon(&icons::SEARCH)
        .plain()
        .width(None)
        .on_edit(|s: &mut State| &mut s.palette.as_mut().expect("palette is open").query)
        .on_change(|s: &mut State, _| {
            if let Some(palette) = &mut s.palette {
                palette.selected = 0;
            }
        })
        .on_key(|s: &mut State, event, tx: &mut Tx| {
            let count = entries(s).len();
            let Some(palette) = &mut s.palette else {
                return false;
            };
            match event.key {
                Key::Down => palette.selected = (palette.selected + 1).min(count.saturating_sub(1)),
                Key::Up => palette.selected = palette.selected.saturating_sub(1),
                Key::Enter => {
                    let index = palette.selected;
                    if let Some(entry) = entries(s).into_iter().nth(index) {
                        s.run_command(entry.command, tx);
                    }
                }
                _ => return false,
            }
            true
        })
        .build(k)
        .key("palette-input");
    let mut last_group = "";
    let mut rows = Vec::new();
    for (index, entry) in found.into_iter().enumerate() {
        if entry.group != last_group {
            last_group = entry.group;
            rows.push(ui::caption(entry.group, t).px(10.0).pt(8.0).pb(4.0));
        }
        let command = entry.command.clone();
        rows.push(
            row()
                .center_y()
                .key(format!("palette-{index}"))
                .role("option")
                .label(entry.label.clone())
                .w_full()
                .h(34.0)
                .gap(10.0)
                .px(10.0)
                .radius(6.0)
                .center_y()
                .reveal_selected(index == selected)
                .on_press_tx(move |s: &mut State, tx: &mut Tx| s.run_command(command.clone(), tx))
                // One highlight, which slides from row to row.
                .children((index == selected).then(|| {
                    el().key("palette-highlight")
                        .matched("palette-highlight")
                        .absolute(0.0, 0.0)
                        .w_full()
                        .h_full()
                        .radius(7.0)
                        .fill(k.c.text_base.alpha(0.1))
                        .motion(LEAD)
                }))
                .child(icon(entry.icon, 15.0).color(k.c.icon_muted))
                .child(
                    text(entry.label)
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .grow(1.0)
                        .truncate(),
                )
                .children((!entry.keys.is_empty()).then(|| Keybind::new(&entry.keys).build(k))),
        );
    }
    let panel = col()
        .key("palette-panel")
        .role("dialog")
        .label("Command Palette")
        .w_full()
        .max_w(560.0)
        .h(440.0)
        .radius(14.0)
        .fill(k.c.layer_01.alpha(0.62))
        .backdrop_blur(30.0)
        .continuous_corners()
        .border_gradient(1.0, k.c.text_base.alpha(0.22), k.c.text_base.alpha(0.06))
        .shadow(22.0, 60.0, Color::BLACK.alpha(0.45))
        .transition(
            Transition::scale(0.96)
                .combined(Transition::blur(4.0))
                .combined(Transition::opacity()),
        )
        .modal(true, |s: &mut State| s.palette = None)
        .child(col().w_full().padding(6.0).child(input))
        .child(el().w_full().h(1.0).fill(k.c.border_muted))
        .child(
            col()
                .key("palette-list")
                .w_full()
                .min_h(0.0)
                .grow(1.0)
                .padding(6.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .children(rows),
        );
    layer.push(
        el().key("palette-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(k.c.scrim.alpha(0.3))
            .a11y_hidden(),
    );
    layer.push(
        col()
            .key("palette-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .pt(84.0)
            .px(16.0)
            .items_center()
            .child(panel),
    );
}

fn labelled(
    input: TextInput,
    key: &'static str,
    t: &Theme,
    access: fn(&mut State) -> &mut TextEdit,
) -> El {
    input
        .width(None)
        .on_edit(access)
        .on_submit(|s: &mut State, _| s.submit_form())
        .build(&t.kit)
        .key(key)
}

fn form(s: &State, t: &Theme, layer: &mut Vec<El>) {
    let k = &t.kit;
    let Some(form) = &s.form else {
        return;
    };
    let is_new = form.existing.is_none();
    let currency = s.currency();
    macro_rules! access {
        ($field:ident) => {
            |s: &mut State| &mut s.form.as_mut().expect("form is open").$field
        };
    }
    let fields = col()
        .w_full()
        .gap(14.0)
        .px(16.0)
        .pb(16.0)
        .child(
            row()
                .w_full()
                .gap(12.0)
                .items_start()
                .child(ui::field(
                    "Name",
                    labelled(
                        TextInput::new(&form.name)
                            .label("Name")
                            .placeholder("Apple"),
                        "form-name",
                        t,
                        access!(name),
                    ),
                    t,
                ))
                .child(ui::field(
                    "Ticker",
                    labelled(
                        TextInput::new(&form.ticker)
                            .label("Ticker")
                            .placeholder("AAPL"),
                        "form-ticker",
                        t,
                        access!(ticker),
                    ),
                    t,
                )),
        )
        .child(
            row()
                .w_full()
                .gap(12.0)
                .items_start()
                .child(ui::field(
                    "Asset class",
                    labelled(
                        TextInput::new(&form.asset_class)
                            .label("Asset class")
                            .placeholder("Stock"),
                        "form-class",
                        t,
                        access!(asset_class),
                    ),
                    t,
                ))
                .child(ui::field(
                    "Amount",
                    labelled(
                        TextInput::new(&form.amount)
                            .label("Amount")
                            .placeholder("0")
                            .invalid(form.error.is_some()),
                        "form-amount",
                        t,
                        access!(amount),
                    ),
                    t,
                )),
        )
        .children(is_new.then(|| {
            col()
                .w_full()
                .gap(10.0)
                .padding(12.0)
                .radius(8.0)
                .border(1.0, k.c.border_muted)
                .child(ui::caption(
                    "Initial purchase (optional, sets cost basis)",
                    t,
                ))
                .child(
                    row()
                        .w_full()
                        .gap(12.0)
                        .items_start()
                        .child(ui::field(
                            "Quantity",
                            labelled(
                                TextInput::new(&form.quantity)
                                    .label("Quantity")
                                    .placeholder("0"),
                                "form-quantity",
                                t,
                                access!(quantity),
                            ),
                            t,
                        ))
                        .child(ui::field(
                            format!("Price ({currency})"),
                            labelled(
                                TextInput::new(&form.price)
                                    .label(format!("Price ({currency})"))
                                    .placeholder("0.00"),
                                "form-price",
                                t,
                                access!(price),
                            ),
                            t,
                        )),
                )
        }))
        .children(form.error.map(|error| ui::danger(error, t)))
        .child(
            row()
                .center_y()
                .w_full()
                .justify_end()
                .gap(8.0)
                .child(
                    Button::new("Cancel")
                        .on_press(|s: &mut State| s.form = None)
                        .build(k)
                        .key("form-cancel"),
                )
                .child(
                    Button::new(if is_new { "Add Position" } else { "Save" })
                        .variant(ButtonVariant::Submit)
                        .on_press(|s: &mut State| s.submit_form())
                        .build(k)
                        .key("form-submit"),
                ),
        );
    Dialog::new(
        "position",
        if is_new {
            "Add Position"
        } else {
            "Edit Position"
        },
    )
    .open(true)
    .fit()
    .width(520.0)
    .on_close(|s: &mut State| s.form = None)
    .child(fields)
    .build(k, layer);
}

fn confirmation(confirm: &Confirm, t: &Theme, layer: &mut Vec<El>) {
    let k = &t.kit;
    let (title, description, buttons): (String, &str, El) = match confirm {
        Confirm::Delete { id, name } => {
            let id = *id;
            (
                format!("Delete {name}?"),
                "This reaches the file on disk when you save.",
                row()
                    .center_y()
                    .gap(8.0)
                    .child(
                        Button::new("Cancel")
                            .on_press(|s: &mut State| s.confirm = None)
                            .build(k)
                            .key("confirm-cancel"),
                    )
                    .child(
                        Button::new("Delete")
                            .variant(ButtonVariant::Danger)
                            .on_press(move |s: &mut State| {
                                s.confirm = None;
                                s.delete_position(id);
                            })
                            .build(k)
                            .key("confirm-delete"),
                    ),
            )
        }
        Confirm::Quit => (
            "Save changes to your portfolio before quitting?".to_string(),
            "Your position changes are lost if you don't save them.",
            row()
                .center_y()
                .gap(8.0)
                .child(
                    Button::new("Don't Save")
                        .variant(ButtonVariant::Danger)
                        .on_press_tx(|_: &mut State, tx: &mut Tx| tx.quit())
                        .build(k)
                        .key("quit-discard"),
                )
                .child(
                    Button::new("Cancel")
                        .on_press(|s: &mut State| s.confirm = None)
                        .build(k)
                        .key("confirm-cancel"),
                )
                .child(
                    Button::new("Save")
                        .variant(ButtonVariant::Submit)
                        .on_press(|s: &mut State| s.save_and_quit())
                        .build(k)
                        .key("quit-save"),
                ),
        ),
    };
    Dialog::new("confirm", title)
        .open(true)
        .fit()
        .width(440.0)
        .description(description)
        .on_close(|s: &mut State| s.confirm = None)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_end()
                .px(16.0)
                .pb(16.0)
                .child(buttons),
        )
        .build(k, layer);
}
