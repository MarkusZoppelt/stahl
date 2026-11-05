//! Rebalance simulation: what-if scenarios. It never trades.

use gpui_kit::{
    App, Context, Div, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div,
};
use portfolio_rs::simulate::{RebalanceScenario, RebalanceSimulation};

use crate::{
    app::copy_text,
    model::{AppModel, money, signed_money},
    theme::{ActivePalette as _, chart_color},
    ui::{self, Loadable, Tone, left, right},
};

pub struct Rebalance {
    state: Loadable<RebalanceSimulation>,
}

impl Rebalance {
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
            |backend| async move { backend.simulation().await },
            |page: &mut Self, result, _| page.state.finish(result),
        );
    }
}

fn trades(scenario: &RebalanceScenario, currency: &str, cx: &App) -> Div {
    if scenario.trades.is_empty() {
        return div()
            .text_sm()
            .text_color(cx.palette().success)
            .child("No trades needed.");
    }
    ui::table(
        &[
            left("Asset class"),
            left("Action"),
            right("Amount"),
            right("Share"),
        ],
        scenario
            .trades
            .iter()
            .map(|trade| {
                let tone = if trade.action == "Reduce" {
                    Tone::Danger
                } else {
                    Tone::Success
                };
                vec![
                    ui::text_cell(trade.asset_class.clone()),
                    ui::badge(trade.action.clone(), tone, cx).into_any_element(),
                    ui::text_cell(money(trade.amount, currency, cx)),
                    ui::text_cell(format!("{:.1}%", trade.percent_of_portfolio)),
                ]
            })
            .collect(),
        cx,
    )
}

fn scenario_card(scenario: &RebalanceScenario, currency: &str, cx: &App) -> Div {
    let shares: Vec<_> = scenario
        .new_allocation
        .iter()
        .enumerate()
        .map(|(index, item)| (item.percent, chart_color(index)))
        .collect();
    ui::card(cx)
        .gap_3()
        .child(ui::card_header(
            scenario.name.clone(),
            scenario.description.clone(),
            cx,
        ))
        .child(trades(scenario, currency, cx))
        .children((!scenario.new_allocation.is_empty()).then(|| {
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(ui::caption("Allocation afterwards", cx))
                .child(ui::stacked_bar(&shares, cx))
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_x_3()
                        .gap_y_1()
                        .text_xs()
                        .text_color(cx.palette().fg_muted)
                        .children(scenario.new_allocation.iter().enumerate().map(
                            |(index, item)| {
                                ui::row()
                                    .gap_1()
                                    .child(div().size_2().rounded_full().bg(chart_color(index)))
                                    .child(format!("{} {:.1}%", item.asset_class, item.percent))
                            },
                        )),
                )
        }))
        .child(ui::caption(
            format!(
                "Cash impact: {}",
                signed_money(scenario.cash_impact, currency, cx)
            ),
            cx,
        ))
}

impl Render for Rebalance {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let columns = ui::grid_columns(window, 2, 1);
        let page = ui::stack()
            .gap_4()
            .children(self.state.status("Simulating…", cx));
        let refresh = ui::refresh_button("refresh-simulation", self.state.loading)
            .on_click(cx.listener(|page, _, _, cx| page.load(cx)));
        let Some(simulation) = &self.state.data else {
            return page;
        };
        let markdown = simulation.to_markdown();
        let currency = &simulation.currency;
        page.child(
            ui::row()
                .justify_between()
                .gap_4()
                .child(ui::stat(
                    format!("Toward “{}”", simulation.policy_name),
                    money(simulation.portfolio_value, currency, cx),
                    None,
                    cx,
                ))
                .child(
                    ui::row()
                        .gap_1()
                        .child(
                            ui::copy_markdown_button("copy-simulation")
                                .on_click(move |_, _, cx| copy_text(markdown.clone(), cx)),
                        )
                        .child(refresh),
                ),
        )
        .child(
            ui::grid(columns, 16.).children(
                simulation
                    .scenarios
                    .iter()
                    .map(|scenario| scenario_card(scenario, currency, cx)),
            ),
        )
    }
}
