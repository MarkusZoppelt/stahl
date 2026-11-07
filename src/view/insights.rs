//! Briefing, Data Health and Settings.

use portfolio_rs::{
    config::ThemeMode, doctor::HealthStatus, mcp::dto::Severity, validate::ValidationSeverity,
};
use rune::{Cx, El, TextEdit, col, icon, row, text};
use rune_kit::{Button, ButtonVariant, SegmentedControl, TextInput};

use crate::{
    format, icons,
    state::State,
    theme::Theme,
    update,
    view::{ui, updates},
};

pub fn briefing(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    if s.positions.is_empty() {
        return ui::empty_state(
            "briefing-empty",
            &icons::CLIPBOARD_LIST,
            "Nothing to summarize yet",
            "A briefing is one page on your whole portfolio, to read yourself or paste into an AI \
             assistant. Add a position to get one.",
            ui::add_position_button("briefing-empty-add", t),
            t,
        );
    }
    let mut page =
        col()
            .w_full()
            .gap(16.0)
            .children(ui::status(&s.briefing, "Preparing briefing…", t));
    let Some(briefing) = &s.briefing.data else {
        return page;
    };
    let share =
        |value: Option<f64>| value.map_or_else(|| "-".to_string(), |value| format!("{value:.2}%"));
    page = page
        .child(
            row().center_y().w_full().justify_end().gap(16.0).child(
                row()
                    .center_y()
                    .gap(4.0)
                    .child(ui::copy_button(
                        "copy-briefing",
                        briefing.markdown.clone(),
                        t,
                    ))
                    .child(ui::refresh_button(
                        "refresh-briefing",
                        State::load_briefing,
                        t,
                    )),
            ),
        )
        .child(
            ui::columns(ui::room(s, cx), [4, 2, 2], 12.0)
                .child(ui::stat_card(
                    "Total value",
                    s.money(briefing.summary.total_value),
                    None,
                    t,
                ))
                .child(ui::stat_card(
                    "Securities",
                    s.money(briefing.summary.securities_value),
                    None,
                    t,
                ))
                .child(ui::stat_card(
                    "Cash",
                    s.money(briefing.summary.cash_value),
                    None,
                    t,
                ))
                .child(ui::stat_card(
                    "Positions",
                    briefing.summary.position_count.to_string(),
                    None,
                    t,
                )),
        );
    if let (Some(minimum), Some(surplus)) =
        (briefing.cash_buffer.minimum, briefing.cash_buffer.surplus)
    {
        page = page.child(ui::entering(
            ui::card(t).child(ui::title("Cash buffer", t)).child(
                ui::columns(ui::room(s, cx), [3, 3, 1], 12.0)
                    .child(ui::stat(
                        "Cash",
                        s.money(briefing.summary.cash_value),
                        None,
                        t,
                    ))
                    .child(ui::stat("Policy minimum", s.money(minimum), None, t))
                    .child(ui::stat(
                        if surplus >= 0.0 {
                            "Surplus"
                        } else {
                            "Shortfall"
                        },
                        s.signed_money(surplus),
                        Some(t.change(surplus)),
                        t,
                    )),
            ),
            0,
        ));
    }
    if !briefing.risk_flags.is_empty() {
        // Neutral: portfolio_rs also reports "no flags" as a flag.
        page = page.child(ui::entering(
            ui::card(t)
                .child(ui::title("Risk flags", t))
                .child(ui::bullets(&briefing.risk_flags, k.c.text_base, t)),
            1,
        ));
    }
    page = page
        .child(ui::entering(
            ui::card(t)
                .child(ui::title("Allocation", t))
                .child(ui::table(
                    &["Asset class", "Value", "Weight", "Positions"],
                    briefing
                        .allocation
                        .iter()
                        .map(|allocation| {
                            vec![
                                text(allocation.asset_class.clone())
                                    .font(k.ty.ui)
                                    .color(k.c.text_base),
                                ui::number(s.optional_money(allocation.value), k.c.text_base),
                                ui::number(share(allocation.percent), k.c.text_muted),
                                ui::number(allocation.position_count.to_string(), k.c.text_muted),
                            ]
                        })
                        .collect(),
                    t,
                )),
            2,
        ))
        .child(ui::entering(
            ui::card(t)
                .child(ui::title("Positions", t))
                .child(ui::table(
                    &["Name", "Class", "Value", "Weight", "Today"],
                    briefing
                        .positions
                        .iter()
                        .map(|position| {
                            let name = if position.is_stale {
                                format!("{} (stale price)", position.name)
                            } else {
                                position.name.clone()
                            };
                            vec![
                                text(name).font(k.ty.ui).color(k.c.text_base).truncate(),
                                ui::muted(position.asset_class.clone(), t),
                                ui::number(s.optional_money(position.value), k.c.text_base),
                                ui::number(share(position.weight_percent), k.c.text_muted),
                                ui::number(
                                    format::optional_percent(position.daily_return_percent),
                                    t.optional_change(position.daily_return_percent),
                                ),
                            ]
                        })
                        .collect(),
                    t,
                )),
            3,
        ));
    if !briefing.data_quality_flags.is_empty() {
        page = page.child(
            ui::card(t)
                .child(ui::title("Data quality", t))
                .child(ui::bullets(&briefing.data_quality_flags, k.c.text_muted, t)),
        );
    }
    page
}

pub fn health(s: &State, t: &Theme) -> El {
    let validation = s.validation.data.as_ref().map(|report| {
        let (glyph, color, heading) = if report.valid {
            (&icons::CHECK_CIRCLE, t.up, "Portfolio file is valid")
        } else {
            (&icons::X_CIRCLE, t.down, "Portfolio file has errors")
        };
        ui::entering(
            ui::card(t)
                .child(
                    row()
                        .center_y()
                        .gap(12.0)
                        .center_y()
                        .child(icon(glyph, 22.0).color(color))
                        .child(ui::header(
                            heading,
                            format!(
                                "{} positions · {} errors · {} warnings",
                                report.position_count, report.error_count, report.warning_count
                            ),
                            t,
                        )),
                )
                .children(report.issues.iter().map(|issue| {
                    let color = match issue.severity {
                        ValidationSeverity::Error => t.down,
                        ValidationSeverity::Warning => t.warn,
                    };
                    let heading = match &issue.position {
                        Some(position) => format!("{position}: {}", issue.message),
                        None => issue.message.clone(),
                    };
                    ui::finding(color, heading, None, t)
                })),
            0,
        )
    });
    let quality = (!s.findings.is_empty()).then(|| {
        ui::entering(
            ui::card(t)
                .gap(8.0)
                .child(ui::header(
                    "Data quality",
                    "What could make a figure wrong or incomplete.",
                    t,
                ))
                .children(s.findings.iter().map(|finding| {
                    let color = match finding.severity {
                        Severity::Critical => t.down,
                        Severity::Warning => t.warn,
                        Severity::Info => t.info,
                    };
                    ui::finding(color, finding.message.clone(), None, t)
                })),
            1,
        )
    });
    let doctor = s.doctor.data.as_ref().map(|health| {
        ui::entering(
            ui::card(t)
                .gap(8.0)
                .child(ui::header("Workspace", health.dir.clone(), t))
                .children(health.checks.iter().map(|check| {
                    let color = match check.status {
                        HealthStatus::Ok => t.up,
                        HealthStatus::Warning => t.warn,
                        HealthStatus::Error => t.down,
                    };
                    ui::finding(color, check.name.clone(), Some(check.message.clone()), t)
                })),
            2,
        )
    });
    col()
        .w_full()
        .gap(16.0)
        .child(
            row()
                .center_y()
                .w_full()
                .justify_end()
                .child(ui::refresh_button("refresh-health", State::load_health, t)),
        )
        .children(
            s.validation
                .error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t)),
        )
        .children(
            s.doctor
                .error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t)),
        )
        .children(validation)
        .children(quality)
        .children(doctor)
}

fn field(
    label: &'static str,
    input: TextInput,
    key: &'static str,
    t: &Theme,
    access: fn(&mut State) -> &mut TextEdit,
) -> El {
    col()
        .w_full()
        .max_w(440.0)
        .gap(6.0)
        .child(ui::caption(label, t))
        .child(
            input
                .label(label)
                .width(None)
                .on_edit(access)
                .build(&t.kit)
                .key(key),
        )
}

pub fn settings(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let form = &s.settings;
    let theme = SegmentedControl::new(&["Dark", "Light"], Some(usize::from(!s.is_dark())))
        .label("Theme")
        .width(Some(180.0))
        .on_pick(|s: &mut State, index, _| {
            s.set_theme(if index == 0 {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            });
        })
        .build(k)
        .key("theme-choice");
    let key_help = if s.has_api_key {
        "A key is stored. Leave this blank to keep it."
    } else {
        "Stored in the config file below, readable only by you."
    };
    let config_path = s.backend.config_path().unwrap_or_else(|error| error);
    col()
        .w_full()
        .gap(16.0)
        .children(
            form.error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t)),
        )
        .children(
            form.message
                .as_ref()
                .map(|message| ui::notice(message.clone(), t)),
        )
        .child(
            ui::card(t)
                .gap(16.0)
                .child(ui::title("General", t))
                .child(field(
                    "Base currency",
                    TextInput::new(&form.currency).placeholder("EUR"),
                    "settings-currency",
                    t,
                    |s| &mut s.settings.currency,
                ))
                .child(col().gap(6.0).child(ui::caption("Theme", t)).child(theme)),
        )
        .child(
            ui::card(t)
                .gap(16.0)
                .child(ui::header(
                    "AI provider",
                    "An Anthropic or OpenAI-compatible endpoint, saved in the config Stahl shares \
                     with portfolio_rs. Stahl itself doesn't use it yet.",
                    t,
                ))
                .child(field(
                    "Provider URL",
                    TextInput::new(&form.provider_url).placeholder("https://api.anthropic.com/v1"),
                    "settings-url",
                    t,
                    |s| &mut s.settings.provider_url,
                ))
                .child(
                    col()
                        .gap(4.0)
                        .child(field(
                            "API key",
                            TextInput::new(&form.api_key).secure().placeholder("sk-..."),
                            "settings-key",
                            t,
                            |s| &mut s.settings.api_key,
                        ))
                        .child(ui::caption(key_help, t)),
                )
                .child(field(
                    "Model",
                    TextInput::new(&form.model).placeholder("claude-sonnet-5-5"),
                    "settings-model",
                    t,
                    |s| &mut s.settings.model,
                )),
        )
        .child(
            row().center_y().child(
                Button::new("Save Settings")
                    .variant(ButtonVariant::Submit)
                    .icon(&icons::SAVE)
                    .on_press(|s: &mut State| s.save_settings())
                    .build(k)
                    .key("settings-save"),
            ),
        )
        .child(
            ui::card(t)
                .child(ui::header(
                    "Workspace tools",
                    "A workspace adds a policy, reviews, decisions and reports. An agent skill \
                     is a file that tells an AI agent how to use portfolio_rs on your data.",
                    t,
                ))
                .child(
                    row()
                        .center_y()
                        .gap(8.0)
                        .child(
                            Button::new("New Workspace…")
                                .icon(&icons::FOLDER_OPEN)
                                .on_press(|s: &mut State| s.open_new_workspace())
                                .build(k)
                                .key("create-workspace"),
                        )
                        .child(
                            Button::new("Export Agent Skill…")
                                .icon(&icons::SHIELD)
                                .on_press_tx(|s: &mut State, tx| s.export_skill(tx))
                                .build(k)
                                .key("export-skill"),
                        ),
                ),
        )
        .children(update::SELF_UPDATES.then(|| updates::settings_card(s, t)))
        .child(
            ui::card(t)
                .child(ui::title("Data", t))
                .child(
                    ui::caption(format!("Config file: {config_path}"), t)
                        .wrap()
                        .selectable(true),
                )
                .child(
                    row().center_y().child(
                        Button::new("Forget This Portfolio")
                            .icon(&icons::TRASH)
                            .help("Back to the welcome screen. Your files are not touched.")
                            .on_press(|s: &mut State| s.reset())
                            .build(k)
                            .key("clear-path"),
                    ),
                ),
        )
}
