//! Policy review: drift, constraint checks, data quality, suggested actions.

use gpui_kit::{
    Context, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window,
    assets::IconName, div,
};
use portfolio_rs::review::{Review, Severity};

use crate::{
    app::copy_text,
    format,
    model::{AppModel, money},
    theme::ActivePalette as _,
    ui::{self, Loadable, Tone, left, right},
};

pub struct ReviewPage {
    state: Loadable<Review>,
}

fn tone(severity: &Severity) -> Tone {
    match severity {
        Severity::Critical => Tone::Danger,
        Severity::Warning => Tone::Warning,
        Severity::Info => Tone::Info,
    }
}

impl ReviewPage {
    pub fn new(model: Entity<AppModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        let mut page = Self {
            state: Loadable::default(),
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.state.loading = true;
        cx.notify();
        ui::run_backend(
            cx,
            |backend| async move { backend.review().await },
            |page: &mut Self, result, _| page.state.finish(result),
        );
    }
}

impl Render for ReviewPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let mut page = ui::stack()
            .gap_4()
            .children(self.state.status("Reviewing…", cx));
        let refresh = ui::refresh_button("refresh-review", self.state.loading)
            .on_click(cx.listener(|page, _, _, cx| page.load(cx)));
        let Some(review) = &self.state.data else {
            return page;
        };
        let markdown = review.to_markdown();
        page = page.child(
            ui::row()
                .justify_between()
                .gap_4()
                .child(ui::stat(
                    format!("Against “{}”", review.policy_name),
                    money(review.portfolio_value, &review.currency, cx),
                    None,
                    cx,
                ))
                .child(
                    ui::row()
                        .gap_1()
                        .child(
                            ui::copy_markdown_button("copy-review")
                                .on_click(move |_, _, cx| copy_text(markdown.clone(), cx)),
                        )
                        .child(refresh),
                ),
        );
        if !review.findings.is_empty() {
            page = page.child(
                ui::card(cx)
                    .gap_2()
                    .child(ui::section_title("Findings").mb_1())
                    .children(review.findings.iter().map(|finding| {
                        ui::finding_row(
                            tone(&finding.severity),
                            finding.message.clone(),
                            finding.detail.clone().map(SharedString::from),
                            cx,
                        )
                    })),
            );
        }
        page = page.child(
            ui::card(cx)
                .child(ui::section_title("Allocation vs target").mb_4())
                .child(ui::table(
                    &[
                        left("Asset class"),
                        right("Target"),
                        right("Actual"),
                        right("Drift"),
                        right("Status"),
                    ],
                    review
                        .allocations
                        .iter()
                        .map(|allocation| {
                            let (icon, color) = if allocation.within_tolerance {
                                (IconName::CircleCheck, palette.success)
                            } else {
                                (IconName::CircleX, palette.danger)
                            };
                            vec![
                                ui::text_cell(allocation.asset_class.clone()),
                                ui::text_cell(format!("{:.1}%", allocation.target_percent)),
                                ui::text_cell(format!("{:.1}%", allocation.actual_percent)),
                                ui::text_cell(format::percent(allocation.drift_percent)),
                                ui::icon(icon, 16., color).into_any_element(),
                            ]
                        })
                        .collect(),
                    cx,
                )),
        );
        if !review.constraint_checks.is_empty() {
            page = page.child(
                ui::card(cx)
                    .gap_2()
                    .child(ui::section_title("Constraints").mb_1())
                    .children(review.constraint_checks.iter().map(|check| {
                        let (icon, color) = if check.passed {
                            (IconName::CircleCheck, palette.success)
                        } else {
                            (IconName::CircleX, palette.danger)
                        };
                        ui::row()
                            .gap_2()
                            .text_sm()
                            .child(ui::icon(icon, 16., color))
                            .child(
                                div()
                                    .flex_1()
                                    .child(format!("{}: {}", check.name, check.message)),
                            )
                    })),
            );
        }
        if !review.data_quality.is_empty() {
            page = page.child(
                ui::card(cx)
                    .gap_2()
                    .child(ui::section_title("Data quality").mb_1())
                    .children(review.data_quality.iter().map(|issue| {
                        ui::finding_row(
                            tone(&issue.severity),
                            format!("{}: {}", issue.position, issue.issue),
                            None,
                            cx,
                        )
                    })),
            );
        }
        if !review.suggested_actions.is_empty() {
            page = page.child(
                ui::card(cx)
                    .child(ui::section_title("Suggested actions").mb_3())
                    .child(ui::bullet_list(&review.suggested_actions, palette.fg)),
            );
        }
        page
    }
}
