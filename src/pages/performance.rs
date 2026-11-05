//! Performance: value over time from stored snapshots, and today's movers.

use gpui_kit::{
    App, Context, Div, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window,
    component::{
        Sizable as _,
        button::{Button, ButtonVariants as _},
    },
    div, px,
};
use portfolio_rs::state::TopMoverDto;

use crate::{
    backend::{History, Period},
    format,
    model::{AppModel, money, signed_money},
    pages::overview::history_chart,
    ui::{self, Tone},
};

pub struct Performance {
    model: Entity<AppModel>,
    period: Period,
    history: History,
    error: Option<String>,
    revision: u64,
}

impl Performance {
    pub fn new(model: Entity<AppModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |page, model, cx| {
            let revision = model.read(cx).revision;
            if revision != page.revision {
                page.revision = revision;
                page.load(cx);
            }
            cx.notify();
        })
        .detach();
        let revision = model.read(cx).revision;
        let page = Self {
            model,
            period: Period::default(),
            history: History::default(),
            error: None,
            revision,
        };
        page.load(cx);
        page
    }

    fn load(&self, cx: &mut Context<Self>) {
        let period = self.period;
        ui::run_backend(
            cx,
            move |backend| async move { backend.history(period).await },
            |page: &mut Self, result, _| match result {
                Ok(history) => {
                    page.history = history;
                    page.error = None;
                }
                Err(error) => page.error = Some(error),
            },
        );
    }

    fn set_period(&mut self, period: Period, cx: &mut Context<Self>) {
        self.period = period;
        self.load(cx);
        cx.notify();
    }
}

fn movers_card(
    title: &'static str,
    empty: &'static str,
    movers: &[TopMoverDto],
    currency: &str,
    cx: &App,
) -> Div {
    ui::card(cx)
        .gap_3()
        .child(ui::section_title(title))
        .children(movers.is_empty().then(|| ui::muted(empty, cx).text_sm()))
        .children(movers.iter().map(|mover| {
            ui::row()
                .justify_between()
                .gap_3()
                .text_sm()
                .child(
                    div()
                        .truncate()
                        .child(SharedString::from(mover.name.clone())),
                )
                .child(
                    ui::row()
                        .flex_shrink_0()
                        .gap_3()
                        .child(ui::muted(signed_money(mover.pnl, currency, cx), cx))
                        .child(ui::badge(
                            format::percent(mover.percent),
                            Tone::of_change(mover.percent),
                            cx,
                        )),
                )
        }))
}

impl Render for Performance {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let currency = model.currency();
        let Some(performance) = model.performance.clone() else {
            return div().into_any_element();
        };

        let selector = ui::row().gap_1().children(Period::ALL.map(|period| {
            let button = Button::new(period.label())
                .small()
                .label(period.label())
                .on_click(cx.listener(move |page, _, _, cx| page.set_period(period, cx)));
            if period == self.period {
                button.secondary()
            } else {
                button.ghost()
            }
        }));

        let optional = |value: Option<f64>, cx: &App| {
            value.map_or_else(
                || "-".to_string(),
                |value| signed_money(value, &currency, cx),
            )
        };
        let tone = |value: Option<f64>| value.map(Tone::of_change);
        let history = &self.history;
        let period_stats = ui::grid(4, 12.)
            .child(ui::stat(
                "Change",
                optional(history.change, cx),
                tone(history.change),
                cx,
            ))
            .child(ui::stat(
                "Time-weighted return",
                history
                    .twr_percent
                    .map_or_else(|| "-".to_string(), format::percent),
                tone(history.twr_percent),
                cx,
            ))
            .child(ui::stat(
                "Net contributions",
                optional(history.net_contributions, cx),
                None,
                cx,
            ))
            .child(ui::stat(
                "Market effect",
                optional(history.market_effect, cx),
                tone(history.market_effect),
                cx,
            ));

        let totals = ui::grid(ui::grid_columns(window, 4, 2), 12.)
            .child(ui::stat_card(
                "Invested",
                money(performance.invested, &currency, cx),
                None,
                cx,
            ))
            .child(ui::stat_card(
                "Unrealized PnL",
                signed_money(performance.unrealized_pnl, &currency, cx),
                Some(Tone::of_change(performance.unrealized_pnl)),
                cx,
            ))
            .child(ui::stat_card(
                "Total return",
                format::percent(performance.pnl_percent),
                Some(Tone::of_change(performance.pnl_percent)),
                cx,
            ))
            .child(ui::stat_card(
                "Today",
                format!(
                    "{} ({})",
                    signed_money(performance.day_pnl, &currency, cx),
                    format::percent(performance.day_percent),
                ),
                Some(Tone::of_change(performance.day_pnl)),
                cx,
            ));

        let storage = history
            .path
            .as_ref()
            .map(|path| ui::caption(format!("History is stored in {path}"), cx));

        ui::stack()
            .gap_4()
            .children(
                self.error
                    .as_ref()
                    .map(|error| ui::danger_banner(error.clone(), cx)),
            )
            .child(
                ui::card(cx)
                    .gap_4()
                    .child(
                        ui::row()
                            .justify_between()
                            .child(ui::section_title("Value over time"))
                            .child(selector),
                    )
                    .child(period_stats)
                    .child(div().h(px(260.)).child(history_chart(
                        "performance-history",
                        &history.points,
                        &currency,
                        cx,
                    )))
                    .children(storage),
            )
            .child(totals)
            .child(
                ui::grid(2, 16.)
                    .child(movers_card(
                        "Top gainers today",
                        "No gainers today.",
                        &performance.top_gainers,
                        &currency,
                        cx,
                    ))
                    .child(movers_card(
                        "Top losers today",
                        "No losers today.",
                        &performance.top_losers,
                        &currency,
                        cx,
                    )),
            )
            .into_any_element()
    }
}
