//! Allocation: the split today, and how it compares with the policy's targets.

use gpui_kit::{
    Context, Entity, FontWeight, Hsla, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, component::chart::PieChart, div, px,
};
use portfolio_rs::review::Review;

use crate::{
    format,
    model::{AppModel, money},
    theme::{ActivePalette as _, chart_color},
    ui::{self, Tone},
};

pub struct Allocation {
    model: Entity<AppModel>,
    review: Option<Review>,
}

#[derive(Clone)]
struct Slice {
    label: SharedString,
    percent: f32,
    color: Hsla,
}

impl Allocation {
    pub fn new(model: Entity<AppModel>, cx: &mut Context<Self>) -> Self {
        cx.observe(&model, |_, _, cx| cx.notify()).detach();
        ui::run_backend(
            cx,
            |backend| async move { backend.review().await },
            // No policy just means there are no targets to compare with.
            |page: &mut Self, result, _| page.review = result.ok(),
        );
        Self {
            model,
            review: None,
        }
    }
}

impl Render for Allocation {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let model = self.model.read(cx);
        let currency = model.currency();
        let total = model
            .summary
            .as_ref()
            .map_or(0.0, |summary| summary.total_value);
        let items = model.allocation.clone();
        let slices: Vec<Slice> = items
            .iter()
            .enumerate()
            .map(|(index, item)| Slice {
                label: item.asset_class.clone().into(),
                percent: item.percent as f32,
                color: chart_color(index).into(),
            })
            .collect();

        let donut = div().absolute().inset_0().child(
            PieChart::new(slices)
                .value(|slice: &Slice| slice.percent)
                .color(|slice: &Slice| slice.color)
                .tooltip_name(|slice: &Slice| slice.label.clone())
                .tooltip_value(|_, value, _| SharedString::from(format!("{value:.2}%")))
                .inner_radius(88.)
                .outer_radius(132.)
                .pad_angle(0.03),
        );
        let center = div()
            .absolute()
            .inset_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .child(ui::caption("Total", cx))
            .child(
                div()
                    .text_lg()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(palette.fg_strong)
                    .child(money(total, &currency, cx)),
            );

        let breakdown: Vec<_> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let target = self.review.as_ref().and_then(|review| {
                    review
                        .allocations
                        .iter()
                        .find(|target| target.asset_class == item.asset_class)
                });
                let drift = target.map(|target| {
                    let tone = if target.within_tolerance {
                        Tone::Success
                    } else {
                        Tone::Danger
                    };
                    ui::badge(
                        format!(
                            "target {:.0}% · {}",
                            target.target_percent,
                            format::percent(target.drift_percent)
                        ),
                        tone,
                        cx,
                    )
                });
                div()
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
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
                                            .font_weight(FontWeight::MEDIUM)
                                            .child(SharedString::from(item.asset_class.clone())),
                                    )
                                    .children(drift),
                            )
                            .child(
                                ui::row()
                                    .flex_shrink_0()
                                    .gap_3()
                                    .child(
                                        div()
                                            .text_color(palette.fg_muted)
                                            .child(money(item.value, &currency, cx)),
                                    )
                                    .child(format!("{:.1}%", item.percent)),
                            ),
                    )
                    .child(ui::share_bar(
                        item.percent,
                        target.map(|target| target.target_percent),
                        chart_color(index),
                        cx,
                    ))
            })
            .collect();
        let empty = breakdown
            .is_empty()
            .then(|| ui::muted("No allocation data.", cx).text_sm());

        ui::grid(ui::grid_columns(window, 2, 1), 16.)
            .child(ui::card(cx).child(div().relative().h(px(300.)).child(donut).child(center)))
            .child(
                ui::card(cx)
                    .gap_4()
                    .child(ui::card_header(
                        "Breakdown",
                        if self.review.is_some() {
                            "Compared with your policy targets"
                        } else {
                            "Share of total value"
                        },
                        cx,
                    ))
                    .children(empty)
                    .children(breakdown),
            )
    }
}
