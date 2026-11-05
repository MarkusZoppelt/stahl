//! The investment policy: a TOML editor with a readable summary beside it.

use gpui_kit::{
    AppContext as _, Context, Entity, FontWeight, IntoElement, ParentElement as _, Render,
    Styled as _, Window,
    assets::IconName,
    component::{
        Disableable as _,
        button::{Button, ButtonVariants as _},
        input::{Editor, EditorState},
    },
    div, px,
};
use portfolio_rs::policy::Policy;

use crate::{backend::Backend, model::AppModel, ui};

const STRATEGIES: [(&str, &str); 4] = [
    ("balanced-growth", "Balanced Growth"),
    ("capital-preservation", "Capital Preservation"),
    ("aggressive-growth", "Aggressive Growth"),
    ("custom", "Custom"),
];

pub struct PolicyPage {
    model: Entity<AppModel>,
    policy: Option<Policy>,
    editor: Entity<EditorState>,
    loading: bool,
    message: Option<String>,
    error: Option<String>,
}

impl PolicyPage {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            EditorState::new(window, cx)
                .line_number(true)
                .placeholder("# policy.toml")
        });
        let mut page = Self {
            model,
            policy: None,
            editor,
            loading: false,
            message: None,
            error: None,
        };
        page.refresh(window, cx);
        page
    }

    fn show(&mut self, policy: Policy, window: &mut Window, cx: &mut Context<Self>) {
        match Backend::policy_to_toml(&policy) {
            Ok(toml) => self
                .editor
                .update(cx, |editor, cx| editor.set_value(toml, window, cx)),
            Err(error) => self.error = Some(error),
        }
        self.policy = Some(policy);
    }

    fn refresh(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        ui::run_backend_in(
            window,
            cx,
            |backend| async move { Ok(backend.policy().await) },
            |page: &mut Self, result: Result<Option<Policy>, String>, window, cx| {
                page.loading = false;
                if let Ok(Some(policy)) = result {
                    page.show(policy, window, cx);
                }
            },
        );
    }

    fn load_from_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.loading = true;
        self.clear_status();
        ui::run_backend_in(
            window,
            cx,
            |backend| async move { backend.load_policy().await },
            |page: &mut Self, result, window, cx| {
                page.loading = false;
                match result {
                    Ok(policy) => {
                        page.show(policy, window, cx);
                        page.message = Some("Policy loaded".to_string());
                    }
                    Err(error) => page.error = Some(error),
                }
            },
        );
    }

    fn clear_status(&mut self) {
        self.message = None;
        self.error = None;
    }

    fn finish(&mut self, result: Result<Policy, String>, success: &str) {
        self.loading = false;
        match result {
            Ok(policy) => {
                self.policy = Some(policy);
                self.message = Some(success.to_string());
            }
            Err(error) => self.error = Some(error),
        }
    }

    fn validate(&mut self, cx: &mut Context<Self>) {
        let toml = self.editor.read(cx).value().to_string();
        self.loading = true;
        self.clear_status();
        ui::run_backend(
            cx,
            |backend| async move { backend.set_policy_from_toml(&toml).await },
            |page: &mut Self, result, _| page.finish(result, "Policy is valid"),
        );
    }

    fn save(&mut self, cx: &mut Context<Self>) {
        let toml = self.editor.read(cx).value().to_string();
        self.loading = true;
        self.clear_status();
        ui::run_backend(
            cx,
            |backend| async move { backend.save_policy(toml).await },
            |page: &mut Self, result, _| page.finish(result, "Policy saved"),
        );
    }
}

impl Render for PolicyPage {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let has_workspace = self.model.read(cx).is_workspace();
        let loading = self.loading;
        let (wide, full) = (
            ui::grid_columns(window, 2, 1),
            ui::grid_columns(window, 3, 1),
        );

        let templates = self.policy.is_none().then(|| {
            ui::card(cx)
                .gap_3()
                .child(ui::muted("Start from a strategy template:", cx).text_sm())
                .child(
                    ui::row()
                        .flex_wrap()
                        .gap_2()
                        .children(STRATEGIES.map(|(id, label)| {
                            Button::new(id)
                                .secondary()
                                .label(label)
                                .on_click(cx.listener(move |page, _, window, cx| {
                                    match Backend::policy_from_strategy(id) {
                                        Ok(policy) => page.show(policy, window, cx),
                                        Err(error) => page.error = Some(error),
                                    }
                                    cx.notify();
                                }))
                        })),
                )
        });

        let summary = self.policy.as_ref().map(|policy| {
            let constraints = &policy.constraints;
            let limits = [
                constraints
                    .minimum_cash_months
                    .map(|months| format!("Keep {months} months of cash")),
                constraints
                    .single_position_limit_percent
                    .map(|limit| format!("No position above {limit}%")),
                constraints
                    .asset_class_limit_percent
                    .map(|limit| format!("No asset class above {limit}%")),
            ];
            ui::card(cx)
                .gap_3()
                .child(ui::card_header(
                    policy.name.clone(),
                    format!(
                        "{} · {} years · {}",
                        policy.risk_profile, policy.time_horizon_years, policy.base_currency
                    ),
                    cx,
                ))
                .children(policy.allocations.iter().map(|allocation| {
                    ui::row()
                        .justify_between()
                        .gap_3()
                        .text_sm()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(allocation.asset_class.clone()),
                        )
                        .child(ui::muted(
                            match allocation.tolerance_percent {
                                Some(tolerance) => {
                                    format!("{}% ±{tolerance}%", allocation.target_percent)
                                }
                                None => format!("{}%", allocation.target_percent),
                            },
                            cx,
                        ))
                }))
                .children(
                    limits
                        .into_iter()
                        .flatten()
                        .map(|limit| ui::caption(limit, cx)),
                )
        });

        let editor_span = if summary.is_some() { wide } else { full };
        let editor = ui::card(cx)
            .col_span(editor_span)
            .gap_3()
            .child(Editor::new(&self.editor).h(px(420.)))
            .child(
                ui::row()
                    .gap_2()
                    .child(
                        Button::new("policy-validate")
                            .secondary()
                            .label("Validate")
                            .disabled(loading)
                            .on_click(cx.listener(|page, _, _, cx| page.validate(cx))),
                    )
                    .child(
                        Button::new("policy-save")
                            .primary()
                            .icon(IconName::Save)
                            .label("Save to Workspace")
                            .disabled(loading || !has_workspace)
                            .on_click(cx.listener(|page, _, _, cx| page.save(cx))),
                    )
                    .child(
                        Button::new("policy-load")
                            .ghost()
                            .icon(IconName::FileCheck)
                            .label("Reload from Workspace")
                            .disabled(loading || !has_workspace)
                            .on_click(cx.listener(|page, _, window, cx| {
                                page.load_from_workspace(window, cx);
                            })),
                    ),
            );

        ui::stack()
            .gap_4()
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
            .children(templates)
            .child(ui::grid(full, 16.).child(editor).children(summary))
    }
}
