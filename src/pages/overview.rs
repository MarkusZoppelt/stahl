//! The first screen: what it is worth, how it got here, what needs attention.

use chrono::NaiveDate;
use gpui_kit::{
    App, Context, Div, Entity, FontWeight, Hsla, IntoElement, ParentElement as _, Render,
    SharedString, Styled as _, Window, assets::IconName, component::chart::AreaChart, div,
    linear_color_stop, linear_gradient, px,
};
use portfolio_rs::{mcp::dto::Severity, state::PositionDto};

use crate::{
    backend::{History, HistoryValue, Period},
    format,
    model::{AppModel, Privacy, money, signed_money},
    theme::{ActivePalette as _, chart_color},
    ui::{self, Tone},
};

pub struct Overview {
    model: Entity<AppModel>,
    history: Option<History>,
    revision: u64,
}

impl Overview {
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
            history: None,
            revision,
        };
        page.load(cx);
        page
    }

    fn load(&self, cx: &mut Context<Self>) {
        ui::run_backend(
            cx,
            |backend| async move { backend.history(Period::Month).await },
            |page: &mut Self, result, _| {
                if let Ok(history) = result {
                    page.history = Some(history);
                }
            },
        );
    }
}

/// `2026-03-15` as `Mar 15`.
fn short_date(date: &str) -> String {
    NaiveDate::parse_from_str(date, "%Y-%m-%d").map_or_else(
        |_| date.to_string(),
        |date| date.format("%b %-d").to_string(),
    )
}

/// A value-over-time chart, or a note until there are two days to compare.
pub fn history_chart(id: &'static str, points: &[HistoryValue], currency: &str, cx: &App) -> Div {
    if points.len() < 2 {
        return div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_1()
            .h_full()
            .child(ui::muted("Your history starts today", cx).text_sm())
            .child(ui::caption(
                "Stahl records one valuation a day while you use it. \
                 Come back tomorrow for your first line.",
                cx,
            ));
    }
    let hidden = cx.global::<Privacy>().0;
    let accent = Hsla::from(cx.palette().accent);
    let currency = currency.to_string();
    // Zoom to the range: from zero, any real portfolio reads as a flat line.
    let (low, high) = points
        .iter()
        .fold((f64::MAX, f64::MIN), |(low, high), point| {
            (low.min(point.value), high.max(point.value))
        });
    let margin = ((high - low) * 0.15).max(high.abs() * 0.001);
    let chart = AreaChart::new(points.to_vec())
        .id(id)
        .x(|point: &HistoryValue| SharedString::from(short_date(&point.date)))
        .y(|point: &HistoryValue| point.value)
        .stroke(accent)
        .fill(linear_gradient(
            180.,
            linear_color_stop(accent.opacity(0.3), 0.),
            linear_color_stop(accent.opacity(0.0), 1.),
        ))
        .linear()
        .grid(false)
        .y_axis(false)
        .y_domain(low - margin, high + margin)
        .x_axis(true)
        .tick_margin((points.len() / 6).max(1))
        .tooltip_value(move |_, _, value| {
            SharedString::from(if hidden {
                format::HIDDEN.to_string()
            } else {
                format::currency(value, &currency)
            })
        });
    div().size_full().child(chart)
}

fn position_name(position: &PositionDto) -> String {
    position
        .name
        .clone()
        .or_else(|| position.ticker.clone())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn hero(page: &Overview, currency: &str, cx: &App) -> Div {
    let palette = cx.palette();
    let model = page.model.read(cx);
    let Some(summary) = &model.summary else {
        return div();
    };
    let day = Tone::of_change(summary.day_change_percent);
    let pnl = Tone::of_change(summary.total_pnl);
    let points = page
        .history
        .as_ref()
        .map_or_else(Vec::new, |history| history.points.clone());
    ui::card(cx)
        .p_0()
        .overflow_hidden()
        .child(
            div()
                .flex()
                .justify_between()
                .gap_8()
                .p_6()
                .pb_2()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(ui::caption("Total value", cx))
                        .child(
                            div()
                                .text_size(px(40.))
                                .line_height(px(46.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(palette.fg_strong)
                                .child(money(summary.total_value, currency, cx)),
                        )
                        .child(
                            ui::row()
                                .gap_2()
                                .text_sm()
                                .child(
                                    div()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(day.color(cx))
                                        .child(format!(
                                            "{} ({})",
                                            signed_money(summary.day_change, currency, cx),
                                            format::percent(summary.day_change_percent),
                                        )),
                                )
                                .child(ui::muted("today", cx)),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap_8()
                        .child(ui::stat(
                            "Unrealized PnL",
                            signed_money(summary.total_pnl, currency, cx),
                            Some(pnl),
                            cx,
                        ))
                        .child(ui::stat(
                            "Securities",
                            money(summary.securities_value, currency, cx),
                            None,
                            cx,
                        ))
                        .child(ui::stat(
                            "Cash",
                            money(summary.cash_value, currency, cx),
                            None,
                            cx,
                        )),
                ),
        )
        .child(div().h(px(200.)).px_2().pb_2().child(history_chart(
            "overview-history",
            &points,
            currency,
            cx,
        )))
}

fn allocation_card(model: &AppModel, currency: &str, cx: &App) -> Div {
    let palette = cx.palette();
    let shares: Vec<_> = model
        .allocation
        .iter()
        .enumerate()
        .map(|(index, item)| (item.percent, chart_color(index)))
        .collect();
    ui::card(cx)
        .gap_3()
        .child(ui::section_title("Allocation"))
        .child(ui::stacked_bar(&shares, cx))
        .child(div().flex().flex_col().gap_2().mt_1().children(
            model.allocation.iter().enumerate().map(|(index, item)| {
                ui::row()
                    .justify_between()
                    .gap_3()
                    .text_sm()
                    .child(
                        ui::row()
                            .gap_2()
                            .min_w_0()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .size_2()
                                    .rounded_full()
                                    .bg(chart_color(index)),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .child(SharedString::from(item.asset_class.clone())),
                            ),
                    )
                    .child(
                        ui::row()
                            .flex_shrink_0()
                            .gap_3()
                            .child(
                                div()
                                    .text_color(palette.fg_muted)
                                    .child(money(item.value, currency, cx)),
                            )
                            .child(
                                div()
                                    .w(px(52.))
                                    .flex()
                                    .justify_end()
                                    .child(format!("{:.1}%", item.percent)),
                            ),
                    )
            }),
        ))
}

fn movers_card(model: &AppModel, cx: &App) -> Div {
    let mut movers: Vec<&PositionDto> = model
        .positions
        .iter()
        .filter(|position| {
            position
                .day_change_percent
                .is_some_and(|change| change != 0.0)
        })
        .collect();
    movers.sort_by(|a, b| {
        let size = |position: &PositionDto| position.day_change_percent.unwrap_or(0.0).abs();
        size(b).total_cmp(&size(a))
    });
    let empty = movers
        .is_empty()
        .then(|| ui::muted("No price movements today.", cx).text_sm());
    ui::card(cx)
        .gap_3()
        .child(ui::section_title("Top movers today"))
        .children(empty)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(movers.into_iter().take(5).map(|position| {
                    let change = position.day_change_percent.unwrap_or(0.0);
                    ui::row()
                        .justify_between()
                        .gap_3()
                        .text_sm()
                        .child(div().truncate().child(position_name(position)))
                        .child(ui::badge(
                            format::percent(change),
                            Tone::of_change(change),
                            cx,
                        ))
                })),
        )
}

fn attention_card(model: &AppModel, cx: &App) -> Div {
    let palette = cx.palette();
    let mut findings: Vec<_> = model
        .findings
        .iter()
        .filter(|finding| !matches!(finding.severity, Severity::Info))
        .collect();
    findings.sort_by_key(|finding| !matches!(finding.severity, Severity::Critical));
    let all_clear = findings.is_empty().then(|| {
        ui::row()
            .gap_2()
            .text_sm()
            .text_color(palette.fg_muted)
            .child(ui::icon(IconName::CircleCheck, 16., palette.success))
            .child("Nothing right now. Your data looks good.")
    });
    ui::card(cx)
        .gap_3()
        .child(ui::section_title("Needs attention"))
        .children(all_clear)
        .child(
            div()
                .flex()
                .flex_col()
                .gap_2()
                .children(findings.into_iter().take(4).map(|finding| {
                    let tone = match finding.severity {
                        Severity::Critical => Tone::Danger,
                        _ => Tone::Warning,
                    };
                    ui::finding_row(tone, finding.message.clone(), None, cx)
                })),
        )
}

impl Render for Overview {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let model = self.model.read(cx);
        let currency = model.currency();
        ui::stack().gap_4().child(hero(self, &currency, cx)).child(
            ui::grid(ui::grid_columns(window, 3, 2), 16.)
                .child(allocation_card(model, &currency, cx))
                .child(movers_card(model, cx))
                .child(attention_card(model, cx)),
        )
    }
}
