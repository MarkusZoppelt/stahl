//! Generate weekly reports and decision records into the workspace.

use gpui_kit::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window,
    assets::IconName,
    component::{
        Disableable as _, Sizable as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputState, Textarea, TextareaState},
    },
    px,
};
use portfolio_rs::state::GeneratedDocument;

use crate::{app::copy_text, backend::BackendResult, model::AppModel, ui};

pub struct Documents {
    model: Entity<AppModel>,
    title: Entity<InputState>,
    preview: Entity<TextareaState>,
    generated: Option<GeneratedDocument>,
    loading: bool,
    message: Option<String>,
    error: Option<String>,
}

impl Documents {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let title = cx.new(|cx| InputState::new(window, cx).placeholder("Rebalance Stocks"));
        let preview = cx.new(|cx| TextareaState::new(window, cx));
        Self {
            model,
            title,
            preview,
            generated: None,
            loading: false,
            message: None,
            error: None,
        }
    }

    fn finish(
        &mut self,
        result: BackendResult<GeneratedDocument>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.loading = false;
        match result {
            Ok(document) => {
                self.message = Some(if document.dry_run {
                    "Preview only. Nothing was written.".to_string()
                } else {
                    format!("Saved to {}", document.file_path)
                });
                self.error = None;
                let content = document.content.clone();
                self.preview
                    .update(cx, |preview, cx| preview.set_value(content, window, cx));
                self.generated = Some(document);
            }
            Err(error) => {
                self.message = None;
                self.error = Some(error);
            }
        }
    }

    fn report(&mut self, dry_run: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.model.read(cx).open.workspace_dir.clone() else {
            return;
        };
        self.loading = true;
        ui::run_backend_in(
            window,
            cx,
            move |backend| async move { backend.generate_report(dir, dry_run).await },
            |page: &mut Self, result, window, cx| page.finish(result, window, cx),
        );
    }

    fn decision(&mut self, dry_run: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.model.read(cx).open.workspace_dir.clone() else {
            return;
        };
        let title = self.title.read(cx).value().trim().to_string();
        let title = (!title.is_empty()).then_some(title);
        self.loading = true;
        ui::run_backend_in(
            window,
            cx,
            move |backend| async move { backend.draft_decision(title, dir, dry_run).await },
            |page: &mut Self, result, window, cx| page.finish(result, window, cx),
        );
    }
}

impl Render for Documents {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let loading = self.loading;
        let weekly = ui::card(cx)
            .gap_3()
            .child(ui::card_header(
                "Weekly report",
                "Portfolio status and policy alignment, as Markdown.",
                cx,
            ))
            .child(
                ui::row()
                    .gap_2()
                    .child(
                        Button::new("report-preview")
                            .secondary()
                            .label("Preview")
                            .disabled(loading)
                            .on_click(
                                cx.listener(|page, _, window, cx| page.report(true, window, cx)),
                            ),
                    )
                    .child(
                        Button::new("report-save")
                            .primary()
                            .icon(IconName::Save)
                            .label("Generate & Save")
                            .disabled(loading)
                            .on_click(
                                cx.listener(|page, _, window, cx| page.report(false, window, cx)),
                            ),
                    ),
            );
        let decision = ui::card(cx)
            .gap_3()
            .child(ui::card_header(
                "Decision record",
                "A structured record of what you decided and why.",
                cx,
            ))
            .child(Input::new(&self.title))
            .child(
                ui::row()
                    .gap_2()
                    .child(
                        Button::new("decision-preview")
                            .secondary()
                            .label("Preview")
                            .disabled(loading)
                            .on_click(
                                cx.listener(|page, _, window, cx| page.decision(true, window, cx)),
                            ),
                    )
                    .child(
                        Button::new("decision-save")
                            .primary()
                            .icon(IconName::Save)
                            .label("Draft & Save")
                            .disabled(loading)
                            .on_click(
                                cx.listener(|page, _, window, cx| page.decision(false, window, cx)),
                            ),
                    ),
            );
        let generated = self.generated.as_ref().map(|document| {
            let content = document.content.clone();
            ui::card(cx)
                .gap_3()
                .child(
                    ui::row()
                        .justify_between()
                        .gap_3()
                        .child(ui::caption(document.file_path.clone(), cx).truncate())
                        .child(
                            Button::new("copy-document")
                                .ghost()
                                .small()
                                .icon(IconName::Copy)
                                .label("Copy")
                                .on_click(move |_, _, cx| copy_text(content.clone(), cx)),
                        ),
                )
                .child(Textarea::new(&self.preview).readonly(true).h(px(400.)))
        });
        ui::stack()
            .gap_4()
            .child(
                ui::grid(ui::grid_columns(window, 2, 1), 16.)
                    .child(weekly)
                    .child(decision),
            )
            .children(
                self.error
                    .as_ref()
                    .map(|error| ui::danger_banner(error.clone(), cx)),
            )
            .children(
                self.message
                    .as_ref()
                    .map(|message| ui::message_banner(message.clone(), cx)),
            )
            .children(generated)
    }
}
