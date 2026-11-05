//! Settings: currency, appearance, AI provider, workspace tools.

use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window,
    assets::IconName,
    component::{
        button::{Button, ButtonVariants as _},
        input::{Input, InputState},
    },
    div, px,
};
use portfolio_rs::{config::ThemeMode, state::AppConfigDto};

use crate::{
    backend::Backend,
    model::{AppModel, pick_directory},
    ui,
};

pub struct Settings {
    model: Entity<AppModel>,
    currency: Entity<InputState>,
    provider_url: Entity<InputState>,
    api_key: Entity<InputState>,
    llm_model: Entity<InputState>,
    has_api_key: bool,
    config_path: String,
    message: Option<String>,
    error: Option<String>,
}

fn field(
    label: &'static str,
    input: &Entity<InputState>,
    help: Option<SharedString>,
    cx: &App,
) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .max_w(px(440.))
        .child(div().text_sm().child(label))
        .child(Input::new(input))
        .children(help.map(|help| ui::caption(help, cx)))
}

impl Settings {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        let currency = cx.new(|cx| InputState::new(window, cx).placeholder("EUR"));
        let provider_url =
            cx.new(|cx| InputState::new(window, cx).placeholder("https://api.anthropic.com/v1"));
        let api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("sk-...")
                .masked(true)
        });
        let llm_model = cx.new(|cx| InputState::new(window, cx).placeholder("claude-sonnet-5-5"));
        let config_path = cx
            .global::<Backend>()
            .config_path()
            .unwrap_or_else(|error| error);
        let mut page = Self {
            model,
            currency,
            provider_url,
            api_key,
            llm_model,
            has_api_key: false,
            config_path,
            message: None,
            error: None,
        };
        page.load(window, cx);
        page
    }

    fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        ui::run_backend_in(
            window,
            cx,
            |backend| async move { Ok(backend.config().await) },
            |page: &mut Self, result: Result<AppConfigDto, String>, window, cx| {
                let Ok(config) = result else {
                    return;
                };
                page.currency.update(cx, |state, cx| {
                    state.set_value(config.currency, window, cx);
                });
                page.provider_url.update(cx, |state, cx| {
                    state.set_value(config.llm_provider_url.unwrap_or_default(), window, cx);
                });
                page.llm_model.update(cx, |state, cx| {
                    state.set_value(config.llm_model.unwrap_or_default(), window, cx);
                });
                page.has_api_key = config.has_llm_api_key;
            },
        );
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let read = |state: &Entity<InputState>, cx: &App| state.read(cx).value().trim().to_string();
        let optional = |text: String| (!text.is_empty()).then_some(text);
        let currency = read(&self.currency, cx).to_uppercase();
        if currency.is_empty() {
            self.error = Some("Currency can't be empty.".to_string());
            self.message = None;
            cx.notify();
            return;
        }
        let provider_url = optional(read(&self.provider_url, cx));
        let api_key = optional(read(&self.api_key, cx));
        let llm_model = optional(read(&self.llm_model, cx));

        self.model
            .update(cx, |model, cx| model.set_currency(currency, cx));
        ui::run_backend_in(
            window,
            cx,
            |backend| async move {
                backend
                    .set_llm_config(provider_url, api_key, llm_model)
                    .await
            },
            |page: &mut Self, result, window, cx| match result {
                Ok(()) => {
                    page.message = Some("Settings saved".to_string());
                    page.error = None;
                    page.api_key
                        .update(cx, |state, cx| state.set_value("", window, cx));
                    page.model.update(cx, |model, cx| model.reload_config(cx));
                    page.load(window, cx);
                }
                Err(error) => {
                    page.message = None;
                    page.error = Some(error);
                }
            },
        );
    }

    fn create_workspace(&mut self, cx: &mut Context<Self>) {
        let dir = pick_directory(cx);
        cx.spawn(async move |page, cx| {
            let Some(dir) = dir.await else {
                return;
            };
            let _ = page.update(cx, |page, cx| {
                match cx.global::<Backend>().init_workspace(&dir) {
                    Ok(()) => {
                        page.error = None;
                        page.message = Some(format!("Workspace created at {dir}"));
                        page.model
                            .update(cx, |model, cx| model.load_workspace(dir, cx));
                    }
                    Err(error) => {
                        page.message = None;
                        page.error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn export_skill(&mut self, cx: &mut Context<Self>) {
        let dir = pick_directory(cx);
        cx.spawn(async move |page, cx| {
            let Some(dir) = dir.await else {
                return;
            };
            let _ = page.update(cx, |page, cx| {
                match cx.global::<Backend>().export_skill(&dir) {
                    Ok(()) => {
                        page.error = None;
                        page.message = Some(format!("Skill exported to {dir}"));
                    }
                    Err(error) => {
                        page.message = None;
                        page.error = Some(error);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn theme_button(
        &self,
        id: &'static str,
        mode: ThemeMode,
        label: &'static str,
        icon: IconName,
        cx: &mut Context<Self>,
    ) -> Button {
        let selected = self.model.read(cx).theme_mode() == mode;
        let button = Button::new(id)
            .icon(icon)
            .label(label)
            .on_click(cx.listener(move |page, _, _, cx| {
                page.model.update(cx, |model, cx| model.set_theme(mode, cx));
            }));
        if selected {
            button.primary()
        } else {
            button.secondary()
        }
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = self.theme_button("theme-dark", ThemeMode::Dark, "Dark", IconName::Moon, cx);
        let light = self.theme_button("theme-light", ThemeMode::Light, "Light", IconName::Sun, cx);
        let key_help: SharedString = if self.has_api_key {
            "A key is stored. Leave this blank to keep it.".into()
        } else {
            "Stored in the config file below, readable only by you.".into()
        };

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
            .child(
                ui::card(cx)
                    .gap_4()
                    .child(ui::section_title("General"))
                    .child(field("Base currency", &self.currency, None, cx))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_sm().child("Theme"))
                            .child(ui::row().gap_2().child(dark).child(light)),
                    ),
            )
            .child(
                ui::card(cx)
                    .gap_4()
                    .child(ui::card_header(
                        "AI provider",
                        "An Anthropic or OpenAI-compatible endpoint for portfolio_rs.",
                        cx,
                    ))
                    .child(field("Provider URL", &self.provider_url, None, cx))
                    .child(field("API key", &self.api_key, Some(key_help), cx))
                    .child(field("Model", &self.llm_model, None, cx)),
            )
            .child(
                ui::row().child(
                    Button::new("settings-save")
                        .primary()
                        .icon(IconName::Save)
                        .label("Save Settings")
                        .on_click(cx.listener(|page, _, window, cx| page.save(window, cx))),
                ),
            )
            .child(
                ui::card(cx)
                    .gap_3()
                    .child(ui::card_header(
                        "Workspace tools",
                        "A workspace adds a policy, reviews, decisions, and reports.",
                        cx,
                    ))
                    .child(
                        ui::row()
                            .gap_2()
                            .child(
                                Button::new("create-workspace")
                                    .secondary()
                                    .icon(IconName::FolderOpen)
                                    .label("Create Workspace…")
                                    .on_click(
                                        cx.listener(|page, _, _, cx| page.create_workspace(cx)),
                                    ),
                            )
                            .child(
                                Button::new("export-skill")
                                    .secondary()
                                    .icon(IconName::Shield)
                                    .label("Export Agent Skill…")
                                    .on_click(cx.listener(|page, _, _, cx| page.export_skill(cx))),
                            ),
                    ),
            )
            .child(
                ui::card(cx)
                    .gap_3()
                    .child(ui::section_title("Data"))
                    .child(ui::caption(
                        format!("Config file: {}", self.config_path),
                        cx,
                    ))
                    .child(
                        ui::row().child(
                            Button::new("clear-path")
                                .secondary()
                                .icon(IconName::Trash)
                                .label("Forget This Portfolio")
                                .tooltip("Back to the welcome screen. Your files are not touched.")
                                .on_click(cx.listener(|page, _, _, cx| {
                                    page.model.update(cx, |model, cx| model.reset(cx));
                                })),
                        ),
                    ),
            )
    }
}
