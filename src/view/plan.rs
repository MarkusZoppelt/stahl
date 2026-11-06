//! The workspace pages: Review, Rebalance, Policy and Documents.

use portfolio_rs::{review::Severity, simulate::RebalanceScenario};
use rune::{Color, Cx, DragItem, El, ScrollSpec, col, icon, row, text, text_input};
use rune_kit::{Button, ButtonVariant, Markdown, TextInput};

use crate::{format, icons, state::State, theme::Theme, view::ui};

fn tone(severity: &Severity, t: &Theme) -> Color {
    match severity {
        Severity::Critical => t.down,
        Severity::Warning => t.warn,
        Severity::Info => t.info,
    }
}

fn pass_icon(passed: bool, t: &Theme) -> El {
    icon(
        if passed {
            &icons::CHECK_CIRCLE
        } else {
            &icons::X_CIRCLE
        },
        16.0,
    )
    .color(if passed { t.up } else { t.down })
}

pub fn review(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let mut page = col()
        .w_full()
        .gap(16.0)
        .children(ui::status(&s.review, "Reviewing…", t));
    let Some(review) = &s.review.data else {
        return page;
    };
    let within = review
        .allocations
        .iter()
        .filter(|allocation| allocation.within_tolerance)
        .count();
    let classes = review.allocations.len();
    let fit = if classes == 0 {
        1.0
    } else {
        within as f32 / classes as f32
    };
    let fit_color = if within == classes {
        t.up
    } else if fit >= 0.5 {
        t.warn
    } else {
        t.down
    };
    page = page.child(
        row()
            .center_y()
            .w_full()
            .justify_between()
            .gap(16.0)
            .child(
                row()
                    .center_y()
                    .gap(16.0)
                    .child(ui::gauge(fit, format!("{within}/{classes}"), fit_color, t))
                    .child(
                        col()
                            .gap(2.0)
                            .child(ui::stat(
                                format!("Against “{}”", review.policy_name),
                                s.money(review.portfolio_value),
                                None,
                                t,
                            ))
                            .child(ui::caption(
                                format!("{within} of {classes} asset classes within tolerance"),
                                t,
                            )),
                    ),
            )
            .child(
                row()
                    .center_y()
                    .gap(4.0)
                    .child(ui::copy_button("copy-review", review.to_markdown(), t))
                    .child(ui::refresh_button("refresh-review", State::load_review, t)),
            ),
    );
    if !review.findings.is_empty() {
        page = page.child(ui::entering(
            ui::card(t)
                .gap(8.0)
                .child(ui::title("Findings", t))
                .children(review.findings.iter().map(|finding| {
                    ui::finding(
                        tone(&finding.severity, t),
                        finding.message.clone(),
                        finding.detail.clone(),
                        t,
                    )
                })),
            0,
        ));
    }
    page = page.child(ui::entering(
        ui::card(t)
            .child(ui::title("Allocation vs target", t))
            .child(ui::table(
                &["Asset class", "Target", "Actual", "Drift", "Status"],
                review
                    .allocations
                    .iter()
                    .map(|allocation| {
                        vec![
                            text(allocation.asset_class.clone())
                                .font(k.ty.ui)
                                .color(k.c.text_base),
                            ui::number(
                                format!("{:.1}%", allocation.target_percent),
                                k.c.text_muted,
                            ),
                            ui::number(format!("{:.1}%", allocation.actual_percent), k.c.text_base),
                            ui::number(format::percent(allocation.drift_percent), k.c.text_base),
                            pass_icon(allocation.within_tolerance, t),
                        ]
                    })
                    .collect(),
                t,
            )),
        1,
    ));
    if !review.constraint_checks.is_empty() {
        page = page.child(ui::entering(
            ui::card(t)
                .gap(8.0)
                .child(ui::title("Constraints", t))
                .children(review.constraint_checks.iter().map(|check| {
                    row()
                        .center_y()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(pass_icon(check.passed, t))
                        .child(
                            text(format!("{}: {}", check.name, check.message))
                                .font(k.ty.ui)
                                .color(k.c.text_base)
                                .wrap()
                                .shrink(),
                        )
                })),
            2,
        ));
    }
    if !review.data_quality.is_empty() {
        page = page.child(
            ui::card(t)
                .gap(8.0)
                .child(ui::title("Data quality", t))
                .children(review.data_quality.iter().map(|issue| {
                    ui::finding(
                        tone(&issue.severity, t),
                        format!("{}: {}", issue.position, issue.issue),
                        None,
                        t,
                    )
                })),
        );
    }
    if !review.suggested_actions.is_empty() {
        page = page.child(
            ui::card(t)
                .child(ui::title("Suggested actions", t))
                .child(ui::bullets(&review.suggested_actions, k.c.text_base, t)),
        );
    }
    page
}

fn scenario_card(s: &State, scenario: &RebalanceScenario, t: &Theme) -> El {
    let k = &t.kit;
    let shares: Vec<_> = scenario
        .new_allocation
        .iter()
        .map(|item| (item.percent, t.class_color(&item.asset_class)))
        .collect();
    // In the same order as afterwards, so a class that moves stays in its place.
    let today: Vec<_> = scenario
        .new_allocation
        .iter()
        .map(|item| {
            let percent = s
                .allocation
                .iter()
                .find(|current| current.asset_class == item.asset_class)
                .map_or(0.0, |current| current.percent);
            (percent, t.class_color(&item.asset_class))
        })
        .collect();
    let trades = if scenario.trades.is_empty() {
        text("No trades needed.").font(k.ty.ui).color(t.up)
    } else {
        ui::table(
            &["Asset class", "Action", "Amount", "Share"],
            scenario
                .trades
                .iter()
                .map(|trade| {
                    let color = if trade.action == "Reduce" {
                        t.down
                    } else {
                        t.up
                    };
                    vec![
                        text(trade.asset_class.clone())
                            .font(k.ty.ui)
                            .color(k.c.text_base),
                        ui::badge(trade.action.clone(), color),
                        ui::number(s.money(trade.amount), k.c.text_base),
                        ui::number(
                            format!("{:.1}%", trade.percent_of_portfolio),
                            k.c.text_muted,
                        ),
                    ]
                })
                .collect(),
            t,
        )
    };
    ui::card(t)
        .child(ui::header(
            scenario.name.clone(),
            scenario.description.clone(),
            t,
        ))
        .child(trades)
        // portfolio_rs doesn't project every scenario, so there may be nothing to draw.
        .children((!scenario.new_allocation.is_empty()).then(|| {
            col()
                .w_full()
                .gap(8.0)
                .child(ui::caption("Allocation today", t))
                .child(ui::stacked_bar(&today, t))
                .child(ui::caption("Allocation afterwards", t).mt(4.0))
                .child(ui::stacked_bar(&shares, t))
                .child(row().center_y().flex_wrap().gaps(4.0, 12.0).children(
                    scenario.new_allocation.iter().map(|item| {
                        row()
                            .center_y()
                            .gap(4.0)
                            .center_y()
                            .child(ui::dot(t.class_color(&item.asset_class)))
                            .child(ui::caption(
                                format!("{} {:.1}%", item.asset_class, item.percent),
                                t,
                            ))
                    }),
                ))
        }))
        .child(ui::caption(
            format!("Cash impact: {}", s.signed_money(scenario.cash_impact)),
            t,
        ))
}

pub fn rebalance(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let page = col()
        .w_full()
        .gap(16.0)
        .children(ui::status(&s.simulation, "Simulating…", t));
    let Some(simulation) = &s.simulation.data else {
        return page;
    };
    page.child(
        row()
            .center_y()
            .w_full()
            .justify_between()
            .gap(16.0)
            .child(ui::stat(
                format!("Toward “{}”", simulation.policy_name),
                s.money(simulation.portfolio_value),
                None,
                t,
            ))
            .child(
                row()
                    .center_y()
                    .gap(4.0)
                    .child(ui::copy_button(
                        "copy-simulation",
                        simulation.to_markdown(),
                        t,
                    ))
                    .child(ui::refresh_button(
                        "refresh-simulation",
                        State::load_simulation,
                        t,
                    )),
            ),
    )
    .child(
        ui::columns(ui::is_wide(cx), 2, 1, 16.0).children(
            simulation
                .scenarios
                .iter()
                .enumerate()
                .map(|(index, scenario)| ui::entering(scenario_card(s, scenario, t), index)),
        ),
    )
}

const STRATEGIES: [(&str, &str); 4] = [
    ("balanced-growth", "Balanced Growth"),
    ("capital-preservation", "Capital Preservation"),
    ("aggressive-growth", "Aggressive Growth"),
    ("custom", "Custom"),
];

pub fn policy(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    let page = &s.policy;
    let templates = page.policy.is_none().then(|| {
        ui::card(t)
            .child(ui::muted("Start from a strategy template:", t))
            .child(
                row()
                    .center_y()
                    .flex_wrap()
                    .gaps(8.0, 8.0)
                    .children(STRATEGIES.map(|(id, label)| {
                        Button::new(label)
                            .on_press(move |s: &mut State| s.use_strategy(id))
                            .build(k)
                            .key(format!("strategy-{id}"))
                    })),
            )
    });
    let editor = ui::card(t)
        .grow(2.0)
        .w(0.0)
        .child(
            col()
                .key("policy-frame")
                .w_full()
                .h(420.0)
                .padding(12.0)
                .radius(8.0)
                .fill(t.sunken)
                .border(1.0, k.c.border_muted)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(
                    text_input(&page.editor, k.ty.mono.lh(20.0))
                        .key("policy-editor")
                        .label("Policy")
                        .w_full()
                        .color(k.c.text_base)
                        .placeholder("# policy.toml", k.c.text_faint)
                        .caret(k.c.text_base, k.selection())
                        .on_edit(|s: &mut State| &mut s.policy.editor),
                ),
        )
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(
                    Button::new("Validate")
                        .disabled(page.loading)
                        .on_press(|s: &mut State| s.validate_policy())
                        .build(k)
                        .key("policy-validate"),
                )
                .child(
                    Button::new("Save to Workspace")
                        .variant(ButtonVariant::Submit)
                        .icon(&icons::SAVE)
                        .disabled(page.loading)
                        .on_press(|s: &mut State| s.save_policy())
                        .build(k)
                        .key("policy-save"),
                )
                .child(
                    Button::new("Reload from Workspace")
                        .variant(ButtonVariant::Ghost)
                        .icon(&icons::FILE_CHECK)
                        .disabled(page.loading)
                        .on_press(|s: &mut State| s.reload_policy())
                        .build(k)
                        .key("policy-load"),
                ),
        );
    let summary = page.policy.as_ref().map(|policy| {
        let constraints = &policy.constraints;
        let limits = [
            constraints
                .minimum_cash_months
                .map(|months| format!("Keep {months} months of cash")),
            constraints
                .minimum_cash_amount
                .map(|amount| format!("Keep at least {}", s.money(amount))),
            constraints
                .single_position_limit_percent
                .map(|limit| format!("No position above {limit}%")),
            constraints
                .asset_class_limit_percent
                .map(|limit| format!("No asset class above {limit}%")),
        ];
        ui::card(t)
            .grow(1.0)
            .w(0.0)
            .child(ui::header(
                policy.name.clone(),
                format!(
                    "{} · {} years · {}",
                    policy.risk_profile, policy.time_horizon_years, policy.base_currency
                ),
                t,
            ))
            .children(policy.allocations.iter().map(|allocation| {
                row()
                    .center_y()
                    .w_full()
                    .justify_between()
                    .gap(12.0)
                    .child(
                        text(allocation.asset_class.clone())
                            .font(k.ty.ui_medium)
                            .color(k.c.text_base),
                    )
                    .child(ui::muted(
                        match allocation.tolerance_percent {
                            Some(tolerance) => {
                                format!("{}% ±{tolerance}%", allocation.target_percent)
                            }
                            None => format!("{}%", allocation.target_percent),
                        },
                        t,
                    ))
            }))
            .children(
                limits
                    .into_iter()
                    .flatten()
                    .map(|limit| ui::caption(limit, t)),
            )
    });
    let body = if ui::is_wide(cx) {
        row().center_y()
    } else {
        col()
    };
    col()
        .w_full()
        .gap(16.0)
        .children(
            page.error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t)),
        )
        .children(
            page.message
                .as_ref()
                .map(|message| ui::notice(message.clone(), t)),
        )
        .children(templates)
        .child(
            body.w_full()
                .gap(16.0)
                .items_start()
                .child(editor)
                .children(summary),
        )
}

pub fn documents(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let k = &t.kit;
    let page = &s.documents;
    let weekly = ui::card(t)
        .child(ui::header(
            "Weekly report",
            "Portfolio status and policy alignment, as Markdown.",
            t,
        ))
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(
                    Button::new("Preview")
                        .disabled(page.loading)
                        .on_press(|s: &mut State| s.generate_report(true))
                        .build(k)
                        .key("report-preview"),
                )
                .child(
                    Button::new("Generate & Save")
                        .variant(ButtonVariant::Submit)
                        .icon(&icons::SAVE)
                        .disabled(page.loading)
                        .on_press(|s: &mut State| s.generate_report(false))
                        .build(k)
                        .key("report-save"),
                ),
        );
    let decision = ui::card(t)
        .child(ui::header(
            "Decision record",
            "A structured record of what you decided and why.",
            t,
        ))
        .child(
            TextInput::new(&page.title)
                .placeholder("Rebalance Stocks")
                .width(None)
                .on_edit(|s: &mut State| &mut s.documents.title)
                .build(k)
                .key("decision-title"),
        )
        .child(
            row()
                .center_y()
                .gap(8.0)
                .child(
                    Button::new("Preview")
                        .disabled(page.loading)
                        .on_press(|s: &mut State| s.draft_decision(true))
                        .build(k)
                        .key("decision-preview"),
                )
                .child(
                    Button::new("Draft & Save")
                        .variant(ButtonVariant::Submit)
                        .icon(&icons::SAVE)
                        .disabled(page.loading)
                        .on_press(|s: &mut State| s.draft_decision(false))
                        .build(k)
                        .key("decision-save"),
                ),
        );
    let generated = page.generated.as_ref().map(|document| {
        let content = document.content.clone();
        // A preview has no file yet, so only a saved document can leave the app.
        let saved = (!document.dry_run).then(|| document.file_path.clone());
        ui::entering(
            ui::card(t)
                .key("document-preview")
                .child(
                    row()
                        .center_y()
                        .w_full()
                        .justify_between()
                        .gap(12.0)
                        .child(
                            ui::caption(document.file_path.clone(), t)
                                .truncate_middle()
                                .shrink()
                                .key("document-file")
                                .drag_out(move |_: &State| {
                                    saved
                                        .as_ref()
                                        .map(|path| DragItem::new().file(path.as_str()))
                                }),
                        )
                        .child(ui::copy_button("copy-document", content.clone(), t)),
                )
                .child(
                    col()
                        .w_full()
                        .padding(16.0)
                        .radius(8.0)
                        .fill(t.sunken)
                        .child(Markdown::new(content).selectable(true).build(k)),
                ),
            0,
        )
    });
    col()
        .w_full()
        .gap(16.0)
        .child(
            ui::columns(ui::is_wide(cx), 2, 1, 16.0)
                .child(weekly)
                .child(decision),
        )
        .children(
            page.error
                .as_ref()
                .map(|error| ui::danger(error.clone(), t)),
        )
        .children(
            page.message
                .as_ref()
                .map(|message| ui::notice(message.clone(), t)),
        )
        .children(generated)
}
