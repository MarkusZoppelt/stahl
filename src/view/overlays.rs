//! What floats above the window: the command palette, dialogs, toasts, and the parse failure.

use rune::{El, Icon, Key, ScrollSpec, TextEdit, Tx, col, el, icon, row, text, tok::motion::LEAD};
use rune_kit::{
    Button, ButtonVariant, Checkbox, ControlSize, Dialog, Keybind, TextInput, Toast, ToastRegion,
};

use crate::{
    backend::{WorkspaceField, plan_workspace},
    format, icons,
    state::{Command, Confirm, FormField, Page, PendingOpen, State, file_name},
    theme::Theme,
    update,
    view::{
        answers::{self, Figure},
        guide, page_icon,
        portfolio::matches_search,
        ui, updates,
    },
};

pub fn build(s: &State, t: &Theme, layer: &mut Vec<El>) {
    if s.palette.is_some() {
        palette(s, t, layer);
    }
    if s.form.is_some() {
        form(s, t, layer);
    }
    if s.new_workspace.is_some() {
        new_workspace(s, t, layer);
    }
    if s.guide {
        guide::dialog(s, t, layer);
    }
    if s.consent.is_some() {
        consent(s, t, layer);
    }
    if let Some(confirm) = &s.confirm {
        confirmation(confirm, t, layer);
    }
    updates::build(s, t, layer);
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
                        text(problem.summary())
                            .font(k.ty.ui_medium)
                            .color(t.warn)
                            .wrap(),
                    )
                    .child(
                        text(format!("Details: {}", problem.reason))
                            .key("parse-detail")
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
    /// A figure that answers the query: a holding's value, a class's share, the total.
    pub figure: Option<Figure>,
    /// An asset class, drawn in its colour instead of a muted icon.
    pub class: Option<String>,
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
        figure: None,
        class: None,
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
        figure: None,
        class: None,
    };
    // Without a portfolio, only what can start one, or doesn't need one, has any effect.
    let mut all: Vec<Entry> = Vec::new();
    if s.has_portfolio() {
        all.extend(
            Page::PORTFOLIO
                .into_iter()
                .chain(Page::INSIGHTS)
                .chain([Page::Settings])
                .map(page_entry),
        );
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
        ]);
    }
    all.extend([
        action(
            "Guide",
            &icons::CIRCLE_HELP,
            Some((false, "/")),
            Command::Guide,
        ),
        action(
            "New Workspace…",
            &icons::FOLDER_PLUS,
            Some((true, "N")),
            Command::NewWorkspace,
        ),
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
            "Toggle Light/Dark Theme",
            &icons::MOON,
            None,
            Command::Theme,
        ),
        action(
            "Check for Updates…",
            &icons::DOWNLOAD,
            None,
            Command::CheckForUpdates,
        ),
    ]);
    if s.open.is_open() {
        all.push(action(
            if s.is_workspace() {
                "Close Workspace"
            } else {
                "Close File"
            },
            &icons::X_CIRCLE,
            Some((true, "W")),
            Command::Close,
        ));
    }
    all.extend([action(
        "Toggle Light/Dark Theme",
        &icons::MOON,
        None,
        Command::Theme,
    )]);
    if s.has_portfolio() {
        all.extend([
            action(
                "Hide or Show Amounts",
                &icons::EYE_OFF,
                Some((true, "H")),
                Command::Privacy,
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
    }
    let query = s
        .palette
        .as_ref()
        .map_or(String::new(), |palette| palette.query.text.to_lowercase());
    let typed = s
        .palette
        .as_ref()
        .map(|palette| palette.query.text.trim().to_string())
        .unwrap_or_default();
    // Typing a command's own words still runs it: only a question becomes the first row.
    let is_command = all.iter().any(|entry| {
        entry
            .label
            .to_lowercase()
            .starts_with(&typed.to_lowercase())
    });
    let mut found: Vec<Entry> = all
        .into_iter()
        .filter(|entry| update::SELF_UPDATES || !matches!(entry.command, Command::CheckForUpdates))
        .filter(|entry| {
            let label = entry.label.to_lowercase();
            query.split_whitespace().all(|word| label.contains(word))
        })
        .collect();
    // Figures the query asks for by name come first: Enter goes to where they come from.
    if s.has_portfolio() {
        let answer = |group: &'static str| {
            move |answer: answers::Answer| {
                let page = match &answer.command {
                    Command::Go(page) => *page,
                    _ => Page::Overview,
                };
                Entry {
                    label: answer.label,
                    icon: page_icon(page),
                    keys: Vec::new(),
                    command: answer.command,
                    group,
                    figure: Some(answer.figure),
                    class: answer.class,
                }
            }
        };
        let figures: Vec<Entry> = answers::figures(s, &query)
            .into_iter()
            .map(answer("Figures"))
            .collect();
        found.splice(0..0, figures);
        found.extend(
            answers::classes(s, &query)
                .into_iter()
                .map(answer("Asset classes")),
        );
    }
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
                    figure: Some(answers::holding(s, position)),
                    class: None,
                }),
        );
    }
    // A question, or words that match nothing else, become the first row: Enter asks Counsel.
    // A single word the portfolio answers with a figure keeps that figure first.
    let is_question = typed.contains(' ') || typed.ends_with('?');
    if typed.chars().count() >= 2 && !is_command && (is_question || found.is_empty()) {
        found.insert(
            0,
            Entry {
                label: format!("Ask counsel: {typed}"),
                icon: &icons::SPARKLES,
                keys: Vec::new(),
                command: Command::Ask(typed),
                group: "Counsel",
                figure: None,
                class: None,
            },
        );
    }
    found
}

/// The row's mark: a class in its own colour, anything else the muted icon of where it goes.
fn mark(entry: &Entry, t: &Theme) -> El {
    match &entry.class {
        Some(class) => icon(&icons::CHART_PIE, 15.0).color(t.class_color(class)),
        None => icon(entry.icon, 15.0).color(t.kit.c.icon_muted),
    }
}

/// A figure at the end of a palette row: the amount, its share, and today's change.
fn figure_row(figure: &Figure, compact: bool, t: &Theme) -> El {
    let k = &t.kit;
    row()
        .center_y()
        .gap(10.0)
        .child(ui::number(
            figure.value.clone(),
            figure.signed.map_or(k.c.text_base, |value| t.change(value)),
        ))
        .children(
            figure
                .share
                .filter(|_| !compact)
                .map(|share| ui::number(format!("{share:.1}%"), k.c.text_muted)),
        )
        .children(
            figure
                .today
                .map(|today| ui::badge(format::percent(today), t.change(today))),
        )
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
        let name = match &entry.figure {
            Some(figure) => format!("{}, {}", entry.label, figure.value),
            None => entry.label.clone(),
        };
        rows.push(
            row()
                .center_y()
                .key(format!("palette-{index}"))
                .role("option")
                .label(name)
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
                        .radius(6.0)
                        .fill(k.c.text_base.alpha(0.1))
                        .motion(LEAD)
                }))
                .child(mark(&entry, t))
                .child(
                    text(entry.label)
                        .font(k.ty.ui)
                        .color(k.c.text_base)
                        .grow(1.0)
                        .shrink()
                        .truncate(),
                )
                .children(
                    entry
                        .figure
                        .as_ref()
                        .map(|figure| figure_row(figure, s.compact, t)),
                )
                .children(
                    (!entry.keys.is_empty() && !s.compact)
                        .then(|| Keybind::new(&entry.keys).build(k)),
                ),
        );
    }
    let panel = ui::floating(t)
        .key("palette-panel")
        .role("dialog")
        .label("Command Palette")
        .w_full()
        .max_w(560.0)
        .h(440.0)
        .transition(ui::float_in())
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
    let invalid = |field| {
        form.error
            .as_ref()
            .is_some_and(|error| error.field == field)
    };
    // portfolio_rs reads a ticker's purchase prices in its quote currency, which isn't known
    // until the position has been priced.
    let price_label = if form.ticker.text.trim().is_empty() {
        format!("Price ({})", s.currency())
    } else {
        "Price (quote currency)".to_string()
    };
    let base = s.currency();
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
                    col()
                        .w_full()
                        .gap(4.0)
                        .child(labelled(
                            TextInput::new(&form.ticker)
                                .label("Ticker")
                                .placeholder("AAPL"),
                            "form-ticker",
                            t,
                            access!(ticker),
                        ))
                        .child(
                            ui::caption(
                                "Its Yahoo Finance symbol. Blank for cash, or anything without a market price.",
                                t,
                            )
                            .wrap(),
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
                    col()
                        .w_full()
                        .gap(4.0)
                        .child(labelled(
                            TextInput::new(&form.asset_class)
                                .label("Asset class")
                                .placeholder("Stocks"),
                            "form-class",
                            t,
                            access!(asset_class),
                        ))
                        .child(
                            ui::caption(
                                "Stocks, Bonds, Gold, Crypto, Cash… Spell it as your policy does.",
                                t,
                            )
                            .wrap(),
                        ),
                    t,
                ))
                .child(ui::field(
                    "Amount",
                    col()
                        .w_full()
                        .gap(4.0)
                        .child(labelled(
                            TextInput::new(&form.amount)
                                .label("Amount")
                                .placeholder("0")
                                .disabled(form.amount_from_purchases())
                                .invalid(invalid(FormField::Amount)),
                            "form-amount",
                            t,
                            access!(amount),
                        ))
                        .children((!form.amount_from_purchases()).then(|| {
                            ui::caption(
                                format!(
                                    "Shares or units you hold. Without a ticker, the value in {base}."
                                ),
                                t,
                            )
                            .wrap()
                        })),
                    t,
                )),
        )
        .children(form.amount_from_purchases().then(|| {
            ui::caption(
                "Amount is the total of this position's purchases. Change them in the portfolio file.",
                t,
            )
            .wrap()
            .key("form-amount-note")
        }))
        .children(is_new.then(|| {
            col()
                .w_full()
                .gap(10.0)
                .padding(12.0)
                .radius(8.0)
                .border(1.0, k.c.border_muted)
                .child(
                    ui::caption(
                        "Optional: what you paid, so Stahl can show your gain or loss.",
                        t,
                    )
                    .wrap(),
                )
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
                                    .placeholder("0")
                                    .invalid(invalid(FormField::Quantity)),
                                "form-quantity",
                                t,
                                access!(quantity),
                            ),
                            t,
                        ))
                        .child(ui::field(
                            price_label.clone(),
                            labelled(
                                TextInput::new(&form.price)
                                    .label(price_label)
                                    .placeholder("0.00")
                                    .invalid(invalid(FormField::Price)),
                                "form-price",
                                t,
                                access!(price),
                            ),
                            t,
                        )),
                )
        }))
        .children(
            form.error
                .as_ref()
                .map(|error| ui::danger(error.message.clone(), t)),
        )
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

fn new_workspace(s: &State, t: &Theme, layer: &mut Vec<El>) {
    let k = &t.kit;
    let Some(form) = &s.new_workspace else {
        return;
    };
    let wrong = |field: WorkspaceField| {
        form.problem
            .as_ref()
            .is_some_and(|problem| problem.field == field)
    };
    let submit = |s: &mut State, _: &mut Tx| s.submit_new_workspace();
    let name = TextInput::new(&form.name)
        .label("Name")
        .placeholder("My Portfolio")
        .invalid(wrong(WorkspaceField::Name))
        .width(None)
        .on_edit(|s: &mut State| &mut s.new_workspace.as_mut().expect("form is open").name)
        .on_submit(submit)
        .build(k)
        .key("workspace-name");
    let location = TextInput::new(&form.location)
        .label("Location")
        .placeholder("Folder to create it in")
        .invalid(wrong(WorkspaceField::Location))
        .width(None)
        .on_edit(|s: &mut State| &mut s.new_workspace.as_mut().expect("form is open").location)
        .on_submit(submit)
        .build(k)
        .key("workspace-location");
    // Encrypted files are left out, because a copy would be plaintext. Say so, or they'd look lost.
    let keep = s.positions_to_keep().map(|file| {
        Checkbox::new(form.keep_positions)
            .label(format!(
                "Bring my {} positions from {}",
                s.positions.len(),
                file_name(file)
            ))
            .description("Copies them. The original file stays as it is.")
            .on_toggle(|s: &mut State| {
                if let Some(form) = &mut s.new_workspace {
                    form.keep_positions = !form.keep_positions;
                }
            })
            .build(k)
            .key("workspace-keep")
    });
    let encrypted = s
        .open
        .file_path
        .as_deref()
        .is_some_and(|file| file.ends_with(".gpg"))
        && !s.is_workspace()
        && !s.positions.is_empty();
    let target = plan_workspace(&form.name.text, &form.location.text)
        .ok()
        .map(|dir| format!("Creates {}", format::place(&dir.to_string_lossy())));
    // An iPhone's paths are nothing to type: Files names the place, and Choose… picks another.
    let location = if cfg!(target_os = "ios") {
        ui::muted(format::place(&form.location.text), t)
            .key("workspace-location")
            .truncate_middle()
    } else {
        location
    };
    let fields = col()
        .w_full()
        .gap(14.0)
        .px(16.0)
        .pb(16.0)
        .child(ui::wide_field("Name", name, t))
        .child(ui::wide_field(
            "Location",
            row()
                .center_y()
                .w_full()
                .gap(8.0)
                .child(col().grow(1.0).w(0.0).min_w(0.0).child(location))
                .child(
                    Button::new("Choose…")
                        .on_press_tx(|s: &mut State, tx: &mut Tx| s.choose_workspace_location(tx))
                        .build(k)
                        .key("workspace-choose"),
                ),
            t,
        ))
        .children(keep)
        .children(encrypted.then(|| {
            ui::caption(
                "This workspace starts empty: an encrypted file can't be copied without decrypting it.",
                t,
            )
            .wrap()
        }))
        .children(target.map(|target| {
            ui::caption(target, t)
                .key("workspace-target")
                .truncate_middle()
        }))
        .children(
            form.problem
                .as_ref()
                .map(|problem| ui::danger(problem.message.clone(), t).key("workspace-problem")),
        )
        .child(
            row()
                .center_y()
                .w_full()
                .justify_between()
                .gap(8.0)
                .child(
                    Button::new("Open Existing…")
                        .variant(ButtonVariant::Ghost)
                        .on_press_tx(|s: &mut State, tx: &mut Tx| {
                            s.new_workspace = None;
                            s.ask_for_workspace(tx);
                        })
                        .build(k)
                        .key("workspace-open-instead"),
                )
                .child(
                    row()
                        .center_y()
                        .gap(8.0)
                        .child(
                            Button::new("Cancel")
                                .on_press(|s: &mut State| s.new_workspace = None)
                                .build(k)
                                .key("workspace-cancel"),
                        )
                        .child(
                            Button::new("Create Workspace")
                                .variant(ButtonVariant::Submit)
                                .disabled(form.busy)
                                .on_press(|s: &mut State| s.submit_new_workspace())
                                .build(k)
                                .key("workspace-create"),
                        ),
                ),
        );
    Dialog::new("new-workspace", "New Workspace")
        .open(true)
        .fit()
        .width(520.0)
        .description(if s.compact {
            "A folder of plain files you own."
        } else {
            "A folder for your positions, policy, reports and decisions, in plain files you own."
        })
        .on_close(|s: &mut State| s.new_workspace = None)
        .child(fields)
        .build(k, layer);
}

/// The one-time question before a question goes to a provider away from this machine.
fn consent(s: &State, t: &Theme, layer: &mut Vec<El>) {
    let k = &t.kit;
    let Some(consent) = &s.consent else {
        return;
    };
    let host = consent.host.clone();
    let explanation = format!(
        "Your question, your holdings and what they are worth, and your policy will go to {} at \
         {}. Stahl sends nothing else, and only when you ask a question.",
        consent.model, host
    );
    let shown = consent.show;
    let body = if shown {
        Some(
            el().key("consent-body")
                .w_full()
                .max_h(240.0)
                .min_h(0.0)
                .radius(8.0)
                .fill(t.sunken)
                .border(1.0, k.c.border_muted)
                .padding(12.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    text(consent.body.clone())
                        .font(k.ty.mono.lh(16.0))
                        .color(k.c.text_muted)
                        .wrap_anywhere(),
                ),
        )
    } else {
        None
    };
    let panel = ui::floating(t)
        .key("consent-panel")
        .role("dialog")
        .label("Before this question goes out")
        .w_full()
        .max_w(620.0)
        .gap(14.0)
        .padding(20.0)
        .transition(ui::float_in())
        .child(ui::title("Before this question goes out", t))
        .child(text(explanation).font(k.ty.ui).color(k.c.text_muted).wrap())
        .child(
            Button::new(if shown {
                "Hide what is sent"
            } else {
                "Show exactly what is sent"
            })
            .variant(ButtonVariant::Ghost)
            .control_size(ControlSize::Small)
            .icon(&icons::EYE)
            .on_press(|s: &mut State| s.consent_toggle())
            .build(k)
            .key("consent-show"),
        )
        .children(body)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_end()
                .gap(8.0)
                .child(
                    Button::new("Cancel")
                        .on_press(|s: &mut State| s.consent_cancel())
                        .build(k)
                        .key("consent-cancel"),
                )
                .child(
                    Button::new(format!("Send to {host}"))
                        .variant(ButtonVariant::Submit)
                        .icon(&icons::ARROW_UP)
                        .on_press(|s: &mut State| s.consent_send())
                        .build(k)
                        .key("consent-send"),
                ),
        );
    layer.push(
        el().key("consent-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(k.c.scrim.alpha(0.3))
            .a11y_hidden(),
    );
    layer.push(
        col()
            .key("consent-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .center()
            .padding(16.0)
            .child(panel),
    );
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
        Confirm::Unsaved(next) => {
            let (title, description) = match next {
                PendingOpen::Close => (
                    "Save changes before closing?",
                    "Closing goes back to the welcome screen. Your position changes are lost if you don't save them.",
                ),
                PendingOpen::CreateWorkspace => (
                    "Save changes before creating a workspace?",
                    "Creating a workspace closes this portfolio. Your position changes are lost if you don't save them.",
                ),
                PendingOpen::Reload => (
                    "Save changes before reloading?",
                    "Reloading reads the file from disk. Your position changes are lost if you don't save them.",
                ),
                PendingOpen::File
                | PendingOpen::Workspace
                | PendingOpen::Path(_)
                | PendingOpen::StartIn(_)
                | PendingOpen::Startup => (
                    "Save changes before opening another portfolio?",
                    "Your position changes are lost if you don't save them.",
                ),
            };
            (
                title.to_string(),
                description,
                row()
                    .center_y()
                    .gap(8.0)
                    .child(
                        Button::new("Don't Save")
                            .variant(ButtonVariant::Danger)
                            .on_press(|s: &mut State| s.confirm_unsaved(false))
                            .build(k)
                            .key("unsaved-discard"),
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
                            .on_press(|s: &mut State| s.confirm_unsaved(true))
                            .build(k)
                            .key("unsaved-save"),
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
