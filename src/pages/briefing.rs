//! The portfolio briefing: everything an advisor, human or AI, would ask first.

use gpui_kit::{
    App, Context, Div, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window,
};

use crate::{
    app::copy_text,
    backend::Briefing as BriefingData,
    format,
    model::{AppModel, money, signed_money},
    theme::ActivePalette as _,
    ui::{self, Loadable, Tone, left, right},
};

pub struct Briefing {
    state: Loadable<BriefingData>,
}

impl Briefing {
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
            |backend| async move { backend.briefing().await },
            |page: &mut Self, result, _| page.state.finish(result),
        );
    }
}

/// Unpriced positions have no value, so show a dash rather than a zero.
fn optional_money(value: Option<f64>, currency: &str, cx: &App) -> String {
    value.map_or_else(|| "-".to_string(), |value| money(value, currency, cx))
}

fn optional_share(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), |value| format!("{value:.2}%"))
}

/// The cash buffer, when the policy asks for one.
fn cash_buffer(briefing: &BriefingData, cx: &App) -> Option<Div> {
    let minimum = briefing.cash_buffer.minimum?;
    let surplus = briefing.cash_buffer.surplus?;
    let currency = &briefing.currency;
    Some(
        ui::card(cx)
            .gap_3()
            .child(ui::section_title("Cash buffer"))
            .child(
                ui::grid(3, 12.)
                    .child(ui::stat(
                        "Cash",
                        money(briefing.summary.cash_value, currency, cx),
                        None,
                        cx,
                    ))
                    .child(ui::stat(
                        "Policy minimum",
                        money(minimum, currency, cx),
                        None,
                        cx,
                    ))
                    .child(ui::stat(
                        if surplus >= 0.0 {
                            "Surplus"
                        } else {
                            "Shortfall"
                        },
                        signed_money(surplus, currency, cx),
                        Some(Tone::of_change(surplus)),
                        cx,
                    )),
            ),
    )
}

impl Render for Briefing {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let columns = ui::grid_columns(window, 4, 2);
        let mut page = ui::stack()
            .gap_4()
            .children(self.state.status("Preparing briefing…", cx));
        let refresh = ui::refresh_button("refresh-briefing", self.state.loading)
            .on_click(cx.listener(|page, _, _, cx| page.load(cx)));
        let Some(briefing) = &self.state.data else {
            return page;
        };
        let currency = &briefing.currency;
        let markdown = briefing.markdown.clone();
        page = page
            .child(
                ui::row()
                    .justify_between()
                    .gap_4()
                    .child(
                        ui::muted(
                            "A summary to read yourself, or to hand to an AI assistant.",
                            cx,
                        )
                        .text_sm(),
                    )
                    .child(
                        ui::row()
                            .gap_1()
                            .child(
                                ui::copy_markdown_button("copy-briefing")
                                    .on_click(move |_, _, cx| copy_text(markdown.clone(), cx)),
                            )
                            .child(refresh),
                    ),
            )
            .child(
                ui::grid(columns, 12.)
                    .child(ui::stat_card(
                        "Total value",
                        money(briefing.summary.total_value, currency, cx),
                        None,
                        cx,
                    ))
                    .child(ui::stat_card(
                        "Securities",
                        money(briefing.summary.securities_value, currency, cx),
                        None,
                        cx,
                    ))
                    .child(ui::stat_card(
                        "Cash",
                        money(briefing.summary.cash_value, currency, cx),
                        None,
                        cx,
                    ))
                    .child(ui::stat_card(
                        "Positions",
                        briefing.summary.position_count.to_string(),
                        None,
                        cx,
                    )),
            )
            .children(cash_buffer(briefing, cx));
        if !briefing.risk_flags.is_empty() {
            page = page.child(
                ui::card(cx)
                    .child(ui::section_title("Risk flags").mb_3())
                    // Neutral: portfolio_rs also reports "no flags" as a flag.
                    .child(ui::bullet_list(&briefing.risk_flags, palette.fg)),
            );
        }
        page = page
            .child(
                ui::card(cx)
                    .child(ui::section_title("Allocation").mb_4())
                    .child(ui::table(
                        &[
                            left("Asset class"),
                            right("Value"),
                            right("Weight"),
                            right("Positions"),
                        ],
                        briefing
                            .allocation
                            .iter()
                            .map(|allocation| {
                                vec![
                                    ui::text_cell(allocation.asset_class.clone()),
                                    ui::text_cell(optional_money(allocation.value, currency, cx)),
                                    ui::text_cell(optional_share(allocation.percent)),
                                    ui::text_cell(allocation.position_count.to_string()),
                                ]
                            })
                            .collect(),
                        cx,
                    )),
            )
            .child(
                ui::card(cx)
                    .child(ui::section_title("Positions").mb_4())
                    .child(ui::table(
                        &[
                            left("Name"),
                            left("Class"),
                            right("Value"),
                            right("Weight"),
                            right("Today"),
                        ],
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
                                    ui::text_cell(name),
                                    ui::text_cell(position.asset_class.clone()),
                                    ui::text_cell(optional_money(position.value, currency, cx)),
                                    ui::text_cell(optional_share(position.weight_percent)),
                                    ui::text_cell(format::optional_percent(
                                        position.daily_return_percent,
                                    )),
                                ]
                            })
                            .collect(),
                        cx,
                    )),
            );
        if !briefing.data_quality_flags.is_empty() {
            page = page.child(
                ui::card(cx)
                    .child(ui::section_title("Data quality").mb_3())
                    .child(ui::bullet_list(
                        &briefing.data_quality_flags,
                        palette.fg_muted,
                    )),
            );
        }
        page
    }
}
