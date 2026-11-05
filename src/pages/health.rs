//! Whether the numbers can be trusted: file checks, findings, workspace health.

use gpui_kit::{
    Context, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _, Window,
    assets::IconName, div,
};
use portfolio_rs::{
    doctor::{HealthStatus, WorkspaceHealth},
    mcp::dto::Severity,
    validate::{ValidationReport, ValidationSeverity},
};

use crate::{
    model::AppModel,
    theme::ActivePalette as _,
    ui::{self, Loadable, Tone},
};

pub struct Health {
    model: Entity<AppModel>,
    validation: Loadable<ValidationReport>,
    doctor: Loadable<WorkspaceHealth>,
}

impl Health {
    pub fn new(model: Entity<AppModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        let mut page = Self {
            model,
            validation: Loadable::default(),
            doctor: Loadable::default(),
        };
        page.load(cx);
        page
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.validation.loading = true;
        ui::run_backend(
            cx,
            |backend| async move { backend.validate().await },
            |page: &mut Self, result, _| page.validation.finish(result),
        );
        if let Some(dir) = self.model.read(cx).open.workspace_dir.clone() {
            self.doctor.loading = true;
            ui::run_backend(
                cx,
                move |backend| async move { backend.doctor(dir).await },
                |page: &mut Self, result, _| page.doctor.finish(result),
            );
        }
        cx.notify();
    }
}

impl Render for Health {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let findings = self.model.read(cx).findings.clone();
        let is_loading = self.validation.loading || self.doctor.loading;

        let validation = self.validation.data.as_ref().map(|report| {
            let (icon, color, title) = if report.valid {
                (
                    IconName::CircleCheck,
                    palette.success,
                    "Portfolio file is valid",
                )
            } else {
                (
                    IconName::CircleX,
                    palette.danger,
                    "Portfolio file has errors",
                )
            };
            ui::card(cx)
                .gap_3()
                .child(
                    ui::row().gap_3().child(ui::icon(icon, 22., color)).child(
                        div().child(ui::section_title(title)).child(
                            ui::muted(
                                format!(
                                    "{} positions · {} errors · {} warnings",
                                    report.position_count, report.error_count, report.warning_count
                                ),
                                cx,
                            )
                            .text_sm(),
                        ),
                    ),
                )
                .children(report.issues.iter().map(|issue| {
                    let tone = match issue.severity {
                        ValidationSeverity::Error => Tone::Danger,
                        ValidationSeverity::Warning => Tone::Warning,
                    };
                    let text = match &issue.position {
                        Some(position) => format!("{position}: {}", issue.message),
                        None => issue.message.clone(),
                    };
                    ui::finding_row(tone, text, None, cx)
                }))
        });

        let quality = (!findings.is_empty()).then(|| {
            ui::card(cx)
                .gap_2()
                .child(ui::card_header(
                    "Data quality",
                    "What could make a figure wrong or incomplete.",
                    cx,
                ))
                .children(findings.iter().map(|finding| {
                    let tone = match finding.severity {
                        Severity::Critical => Tone::Danger,
                        Severity::Warning => Tone::Warning,
                        Severity::Info => Tone::Info,
                    };
                    ui::finding_row(tone, finding.message.clone(), None, cx)
                }))
        });

        let doctor = self.doctor.data.as_ref().map(|health| {
            ui::card(cx)
                .gap_2()
                .child(ui::card_header("Workspace", health.dir.clone(), cx))
                .children(health.checks.iter().map(|check| {
                    let tone = match check.status {
                        HealthStatus::Ok => Tone::Success,
                        HealthStatus::Warning => Tone::Warning,
                        HealthStatus::Error => Tone::Danger,
                    };
                    ui::finding_row(
                        tone,
                        check.name.clone(),
                        Some(SharedString::from(check.message.clone())),
                        cx,
                    )
                }))
        });

        ui::stack()
            .gap_4()
            .child(
                ui::row().justify_end().child(
                    ui::refresh_button("refresh-health", is_loading)
                        .on_click(cx.listener(|page, _, _, cx| page.load(cx))),
                ),
            )
            .children(
                self.validation
                    .error
                    .as_ref()
                    .map(|error| ui::danger_banner(error.clone(), cx)),
            )
            .children(
                self.doctor
                    .error
                    .as_ref()
                    .map(|error| ui::danger_banner(error.clone(), cx)),
            )
            .children(validation)
            .children(quality)
            .children(doctor)
    }
}
