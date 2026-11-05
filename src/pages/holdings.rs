//! Holdings: a sortable table with a detail pane, and position editing.

use std::cmp::Ordering;

use gpui_kit::{
    App, AppContext as _, Context, Div, Entity, FontWeight, IntoElement, ParentElement as _,
    PromptLevel, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Subscription,
    Window,
    assets::IconName,
    base::TestSupportExt as _,
    component::{
        Sizable as _, WindowExt as _,
        button::{Button, ButtonVariants as _},
        input::{Input, InputEvent, InputState},
    },
    div,
    prelude::{FluentBuilder as _, InteractiveElement as _},
    px,
};
use portfolio_rs::state::{
    CreatePositionRequest, CreatePurchaseRequest, PositionDto, UpdatePositionRequest,
};

use crate::{
    format,
    model::{AppModel, money, signed_money},
    theme::ActivePalette as _,
    ui::{self, Tone},
};

#[derive(Clone, Copy, PartialEq)]
enum SortBy {
    Name,
    Class,
    Value,
    Pnl,
    Day,
}

pub struct Holdings {
    model: Entity<AppModel>,
    search: Entity<InputState>,
    sort: SortBy,
    ascending: bool,
    selected: Option<usize>,
    _subscriptions: Vec<Subscription>,
}

fn display_name(position: &PositionDto) -> String {
    position
        .name
        .clone()
        .or_else(|| position.ticker.clone())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn compare(a: &PositionDto, b: &PositionDto, sort: SortBy) -> Ordering {
    // Missing numbers sort as lowest, so they never lead a table.
    let number =
        |x: Option<f64>, y: Option<f64>| x.unwrap_or(f64::MIN).total_cmp(&y.unwrap_or(f64::MIN));
    match sort {
        SortBy::Name => display_name(a)
            .to_lowercase()
            .cmp(&display_name(b).to_lowercase()),
        SortBy::Class => a.asset_class.cmp(&b.asset_class),
        SortBy::Value => a.value.total_cmp(&b.value),
        SortBy::Pnl => number(a.pnl_percent, b.pnl_percent),
        SortBy::Day => number(a.day_change_percent, b.day_change_percent),
    }
}

/// Whether a position matches every word of a search.
fn matches_search(position: &PositionDto, query: &str) -> bool {
    let haystack = format!(
        "{} {} {}",
        position.name.as_deref().unwrap_or(""),
        position.ticker.as_deref().unwrap_or(""),
        position.asset_class
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}

fn detail(label: &'static str, value: String, cx: &App) -> Div {
    div()
        .min_w_0()
        .child(ui::caption(label, cx))
        .child(div().text_sm().truncate().child(value))
}

impl Holdings {
    pub fn new(model: Entity<AppModel>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search holdings"));
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        Self {
            model,
            search,
            sort: SortBy::Value,
            ascending: false,
            selected: None,
            _subscriptions: subscriptions,
        }
    }

    fn set_sort(&mut self, sort: SortBy, cx: &mut Context<Self>) {
        if self.sort == sort {
            self.ascending = !self.ascending;
        } else {
            self.sort = sort;
            // Text reads A to Z; numbers read biggest first.
            self.ascending = matches!(sort, SortBy::Name | SortBy::Class);
        }
        cx.notify();
    }

    fn header_cell(
        &self,
        id: &'static str,
        label: &'static str,
        sort: SortBy,
        right: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let palette = cx.palette();
        let active = self.sort == sort;
        let arrow = match (active, self.ascending) {
            (true, true) => " ↑",
            (true, false) => " ↓",
            _ => "",
        };
        div()
            .id(id)
            .flex()
            .flex_1()
            .min_w_0()
            .when(right, |cell| cell.justify_end())
            .text_color(if active { palette.fg } else { palette.fg_muted })
            .hover(|style| style.text_color(palette.fg))
            .child(format!("{label}{arrow}"))
            .on_click(cx.listener(move |page, _, _, cx| page.set_sort(sort, cx)))
    }

    fn table(&self, rows: &[PositionDto], currency: &str, cx: &mut Context<Self>) -> Div {
        let palette = cx.palette();
        let selected = self.selected;
        let header = ui::row()
            .gap_3()
            .px_3()
            .pb_2()
            .text_xs()
            .child(self.header_cell("sort-name", "Name", SortBy::Name, false, cx))
            .child(self.header_cell("sort-class", "Class", SortBy::Class, false, cx))
            .child(self.header_cell("sort-value", "Value", SortBy::Value, true, cx))
            .child(self.header_cell("sort-pnl", "PnL", SortBy::Pnl, true, cx))
            .child(self.header_cell("sort-day", "Today", SortBy::Day, true, cx));
        let body: Vec<_> = rows
            .iter()
            .map(|position| {
                let id = position.id;
                let is_selected = selected == Some(id);
                let day = position.day_change_percent;
                let pnl = position.pnl_percent;
                ui::row()
                    .id(("holding", id))
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .text_sm()
                    .when(is_selected, |row| row.bg(palette.bg_tertiary))
                    .when(!is_selected, |row| {
                        row.hover(|style| style.bg(palette.bg_tertiary))
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(palette.fg_strong)
                                    .child(display_name(position)),
                            )
                            .children(
                                position
                                    .ticker
                                    .clone()
                                    .map(|ticker| ui::caption(ticker, cx).truncate()),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(palette.fg_muted)
                            .child(SharedString::from(position.asset_class.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .justify_end()
                            .font_weight(FontWeight::MEDIUM)
                            .child(money(position.value, currency, cx)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .justify_end()
                            .text_color(Tone::of_change(pnl.unwrap_or(0.0)).color(cx))
                            .child(format::optional_percent(pnl)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .justify_end()
                            .text_color(Tone::of_change(day.unwrap_or(0.0)).color(cx))
                            .child(format::optional_percent(day)),
                    )
                    .on_click(cx.listener(move |page, _, _, cx| {
                        page.selected = (page.selected != Some(id)).then_some(id);
                        cx.notify();
                    }))
                    .test_support()
            })
            .collect();
        ui::card(cx)
            .flex_1()
            .p_2()
            .pt_3()
            .child(header)
            .children(
                rows.is_empty()
                    .then(|| ui::muted("No holdings match.", cx).text_sm().p_3()),
            )
            .child(div().flex().flex_col().children(body))
    }

    fn inspector(&self, position: &PositionDto, currency: &str, cx: &mut Context<Self>) -> Div {
        let palette = cx.palette();
        let edit = position.clone();
        let id = position.id;
        let name: SharedString = display_name(position).into();
        let price = if position.price > 0.0 {
            money(position.price, currency, cx)
        } else {
            "-".to_string()
        };
        let optional_money = |value: Option<f64>, cx: &App| {
            value.map_or_else(|| "-".to_string(), |value| money(value, currency, cx))
        };
        let subtitle = match &position.ticker {
            Some(ticker) => format!("{} · {ticker}", position.asset_class),
            None => position.asset_class.clone(),
        };
        let pnl = position
            .pnl
            .map_or_else(|| "-".to_string(), |pnl| signed_money(pnl, currency, cx));
        ui::card(cx)
            .flex_shrink_0()
            .w(px(300.))
            .gap_4()
            .child(
                div()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(palette.fg_strong)
                            .child(name.clone()),
                    )
                    .child(ui::caption(subtitle, cx)),
            )
            .child(ui::stat(
                "Value",
                money(position.value, currency, cx),
                None,
                cx,
            ))
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_3()
                    .child(detail("Quantity", format!("{:.4}", position.amount), cx))
                    .child(detail("Price", price, cx))
                    .child(detail(
                        "Average cost",
                        optional_money(position.average_cost, cx),
                        cx,
                    ))
                    .child(detail(
                        "Invested",
                        optional_money(position.invested, cx),
                        cx,
                    ))
                    .child(detail("PnL", pnl, cx))
                    .child(detail(
                        "Today",
                        format::optional_percent(position.day_change_percent),
                        cx,
                    )),
            )
            .when(!position.purchases.is_empty(), |card| {
                card.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1p5()
                        .child(ui::caption(
                            format!("Purchases ({})", position.purchases.len()),
                            cx,
                        ))
                        .children(position.purchases.iter().map(|purchase| {
                            let price = purchase.price.map_or_else(
                                || "-".to_string(),
                                |price| money(price, currency, cx),
                            );
                            ui::row()
                                .justify_between()
                                .gap_2()
                                .text_xs()
                                .child(
                                    div().text_color(palette.fg_muted).child(
                                        purchase
                                            .date
                                            .clone()
                                            .unwrap_or_else(|| "Undated".to_string()),
                                    ),
                                )
                                .child(format!("{} @ {price}", purchase.quantity))
                        })),
                )
            })
            .child(
                ui::row()
                    .gap_2()
                    .child(
                        Button::new("edit-position")
                            .secondary()
                            .small()
                            .flex_1()
                            .icon(IconName::SquarePen)
                            .label("Edit")
                            .on_click(cx.listener(move |page, _, window, cx| {
                                page.open_form(Some(edit.clone()), window, cx);
                            })),
                    )
                    .child(
                        Button::new("delete-position")
                            .danger()
                            .small()
                            .icon(IconName::Trash)
                            .tooltip("Delete")
                            .on_click(cx.listener(move |page, _, window, cx| {
                                page.confirm_delete(id, name.clone(), window, cx);
                            })),
                    ),
            )
    }

    fn confirm_delete(
        &mut self,
        id: usize,
        name: SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let answer = window.prompt(
            PromptLevel::Warning,
            &format!("Delete {name}?"),
            Some("This reaches the file on disk when you save."),
            &["Delete", "Cancel"],
            cx,
        );
        cx.spawn(async move |page, cx| {
            if matches!(answer.await, Ok(0)) {
                let _ = page.update(cx, |page, cx| {
                    page.selected = None;
                    page.model
                        .update(cx, |model, cx| model.delete_position(id, cx));
                });
            }
        })
        .detach();
    }

    fn open_form(
        &mut self,
        existing: Option<PositionDto>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let currency = self.model.read(cx).currency();
        let title = if existing.is_some() {
            "Edit Position"
        } else {
            "Add Position"
        };
        let model = self.model.clone();
        let form = cx.new(|cx| PositionForm::new(existing, model, currency, window, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title(title).width(px(520.)).child(form.clone())
        });
    }
}

impl Render for Holdings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value();
        let (mut rows, currency, total) = {
            let model = self.model.read(cx);
            (
                model
                    .positions
                    .iter()
                    .filter(|position| matches_search(position, &query))
                    .cloned()
                    .collect::<Vec<_>>(),
                model.currency(),
                model.positions.len(),
            )
        };
        let (sort, ascending) = (self.sort, self.ascending);
        rows.sort_by(|a, b| {
            let order = compare(a, b, sort);
            if ascending { order } else { order.reverse() }
        });
        let selected = self
            .selected
            .and_then(|id| rows.iter().find(|position| position.id == id).cloned());

        let table = self.table(&rows, &currency, cx);
        let inspector = selected.map(|position| self.inspector(&position, &currency, cx));

        ui::stack()
            .gap_4()
            .child(
                ui::row()
                    .justify_between()
                    .gap_4()
                    .child(ui::muted(format!("{total} positions"), cx).text_sm())
                    .child(
                        ui::row()
                            .gap_2()
                            .child(
                                div().w(px(240.)).child(
                                    Input::new(&self.search)
                                        .id("holdings-search")
                                        .small()
                                        .cleanable(true),
                                ),
                            )
                            .child(
                                Button::new("add-position")
                                    .primary()
                                    .small()
                                    .icon(IconName::Plus)
                                    .label("Add")
                                    .on_click(cx.listener(|page, _, window, cx| {
                                        page.open_form(None, window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_4()
                    .child(table)
                    .children(inspector),
            )
    }
}

/// The add/edit form, a view of its own so errors can redraw inside the dialog.
struct PositionForm {
    existing: Option<usize>,
    model: Entity<AppModel>,
    currency: String,
    name: Entity<InputState>,
    ticker: Entity<InputState>,
    asset_class: Entity<InputState>,
    amount: Entity<InputState>,
    quantity: Entity<InputState>,
    price: Entity<InputState>,
    error: Option<&'static str>,
}

/// A blank field is `None`; anything else must be a finite number.
fn parse_number(text: &str) -> Result<Option<f64>, ()> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match text.parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(Some(number)),
        _ => Err(()),
    }
}

fn text_or_none(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn labelled(label: impl Into<SharedString>, input: &Entity<InputState>, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(ui::caption(label, cx))
        .child(Input::new(input))
}

impl PositionForm {
    fn new(
        existing: Option<PositionDto>,
        model: Entity<AppModel>,
        currency: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut field = |placeholder: &'static str, value: String| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .default_value(value)
            })
        };
        let current = existing.as_ref();
        let name = field(
            "Apple",
            current.and_then(|p| p.name.clone()).unwrap_or_default(),
        );
        let ticker = field(
            "AAPL",
            current.and_then(|p| p.ticker.clone()).unwrap_or_default(),
        );
        let asset_class = field(
            "Stock",
            current.map_or_else(|| "Stock".to_string(), |p| p.asset_class.clone()),
        );
        let amount = field(
            "0",
            current.map_or_else(|| "0".to_string(), |p| p.amount.to_string()),
        );
        let quantity = field("0", String::new());
        let price = field("0.00", String::new());
        Self {
            existing: existing.map(|position| position.id),
            model,
            currency,
            name,
            ticker,
            asset_class,
            amount,
            quantity,
            price,
            error: None,
        }
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let read = |state: &Entity<InputState>, cx: &App| state.read(cx).value().to_string();
        let name = text_or_none(&read(&self.name, cx));
        let ticker = text_or_none(&read(&self.ticker, cx));
        let asset_class = text_or_none(&read(&self.asset_class, cx));

        let Ok(Some(amount)) = parse_number(&read(&self.amount, cx)) else {
            self.error = Some("Amount must be a number.");
            cx.notify();
            return;
        };
        match self.existing {
            Some(id) => {
                let request = UpdatePositionRequest {
                    name,
                    ticker,
                    asset_class,
                    amount: Some(amount),
                    purchases: None,
                };
                self.model
                    .update(cx, |model, cx| model.update_position(id, request, cx));
            }
            None => {
                let (Ok(quantity), Ok(price)) = (
                    parse_number(&read(&self.quantity, cx)),
                    parse_number(&read(&self.price, cx)),
                ) else {
                    self.error = Some("Quantity and price must be numbers.");
                    cx.notify();
                    return;
                };
                let purchases = quantity
                    .filter(|quantity| *quantity > 0.0)
                    .map(|quantity| CreatePurchaseRequest {
                        date: None,
                        quantity,
                        price,
                        fees: None,
                    })
                    .into_iter()
                    .collect();
                let request = CreatePositionRequest {
                    name,
                    ticker,
                    asset_class: asset_class.unwrap_or_else(|| "Stock".to_string()),
                    amount,
                    purchases,
                };
                self.model
                    .update(cx, |model, cx| model.add_position(request, cx));
            }
        }
        window.close_dialog(cx);
    }
}

impl Render for PositionForm {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette();
        let is_new = self.existing.is_none();
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(
                div()
                    .grid()
                    .grid_cols(2)
                    .gap_4()
                    .child(labelled("Name", &self.name, cx))
                    .child(labelled("Ticker", &self.ticker, cx))
                    .child(labelled("Asset class", &self.asset_class, cx))
                    .child(labelled("Amount", &self.amount, cx)),
            )
            .when(is_new, |form| {
                form.child(
                    div()
                        .rounded_lg()
                        .border_1()
                        .border_color(palette.border)
                        .p_3()
                        .child(
                            ui::caption("Initial purchase (optional, sets cost basis)", cx).mb_3(),
                        )
                        .child(
                            div()
                                .grid()
                                .grid_cols(2)
                                .gap_4()
                                .child(labelled("Quantity", &self.quantity, cx))
                                .child(labelled(
                                    format!("Price ({})", self.currency),
                                    &self.price,
                                    cx,
                                )),
                        ),
                )
            })
            .children(self.error.map(|error| ui::danger_banner(error, cx)))
            .child(
                ui::row()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("form-cancel")
                            .secondary()
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("form-submit")
                            .primary()
                            .label(if is_new { "Add Position" } else { "Save" })
                            .on_click(cx.listener(|form, _, window, cx| form.submit(window, cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn position(
        name: Option<&str>,
        ticker: Option<&str>,
        value: f64,
        pnl: Option<f64>,
    ) -> PositionDto {
        PositionDto {
            id: 0,
            position_id: None,
            name: name.map(str::to_string),
            ticker: ticker.map(str::to_string),
            asset_class: "Stock".to_string(),
            amount: 1.0,
            price: value,
            value,
            pnl: None,
            pnl_percent: pnl,
            day_change_percent: None,
            average_cost: None,
            invested: None,
            purchases: Vec::new(),
        }
    }

    #[test]
    fn search_matches_every_word_across_fields() {
        let apple = position(Some("Apple Inc"), Some("AAPL"), 1.0, None);
        assert!(matches_search(&apple, ""));
        assert!(matches_search(&apple, "aapl"));
        assert!(matches_search(&apple, "apple stock"));
        assert!(!matches_search(&apple, "apple bond"));
    }

    #[test]
    fn sorting_puts_missing_numbers_lowest() {
        let known = position(Some("A"), None, 1.0, Some(-50.0));
        let unknown = position(Some("B"), None, 1.0, None);
        assert_eq!(compare(&known, &unknown, SortBy::Pnl), Ordering::Greater);
    }

    #[test]
    fn numbers_are_validated_not_coerced() {
        assert_eq!(parse_number(" 1.5 "), Ok(Some(1.5)));
        assert_eq!(parse_number(""), Ok(None));
        assert_eq!(parse_number("1,5"), Err(()));
        assert_eq!(parse_number("NaN"), Err(()));
        assert_eq!(parse_number("inf"), Err(()));
    }

    #[test]
    fn names_fall_back_to_the_ticker() {
        assert_eq!(display_name(&position(None, Some("SPY"), 1.0, None)), "SPY");
        assert_eq!(display_name(&position(None, None, 1.0, None)), "Unknown");
    }
}
