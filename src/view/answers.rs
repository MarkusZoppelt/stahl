//! What the command palette can answer before the owner goes anywhere: a holding's or an asset
//! class's value, share and today's change, and the portfolio's own figures. Plain matching over
//! the data the app has, nothing inferred. Amounts go through `State::money`, so privacy mode
//! hides them here as everywhere.

use portfolio_rs::state::PositionDto;

use crate::state::{Command, Page, State};

/// A figure shown in a palette row.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    /// The amount, as the app writes amounts (hidden in privacy mode).
    pub value: String,
    /// Colour the amount as a gain or a loss, by this sign.
    pub signed: Option<f64>,
    /// Its share of the total, in percent.
    pub share: Option<f64>,
    /// Today's change, in percent, where every price it depends on has one.
    pub today: Option<f64>,
}

/// A figure the palette offers as a row of its own.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    pub label: String,
    pub figure: Figure,
    /// Where Enter goes: where the figure comes from.
    pub command: Command,
    /// Set when this is an asset class, so the row wears that colour.
    pub class: Option<String>,
}

/// A word as it is typed: lower case, and without a possessive, so "today's" is "today".
fn word(text: &str) -> String {
    text.to_lowercase().replace("'s", "").replace('\'', "")
}

/// Whether every word of `query` begins a word of `label` or of `words`. Three letters at least,
/// so a query on its way to a command doesn't fill the list with figures.
fn asks_for(query: &str, label: &str, words: &[&str]) -> bool {
    let query = query.trim();
    if query.chars().count() < 3 {
        return false;
    }
    let mut candidates: Vec<String> = words.iter().copied().map(word).collect();
    candidates.extend(label.split_whitespace().map(word));
    query.split_whitespace().all(|part| {
        let part = word(part);
        !part.is_empty() && candidates.iter().any(|each| each.starts_with(&part))
    })
}

fn share_of(s: &State, value: f64) -> Option<f64> {
    s.summary
        .as_ref()
        .filter(|summary| summary.total_value.abs() > f64::EPSILON)
        .map(|summary| value / summary.total_value * 100.0)
}

/// The portfolio's own figures that `query` asks for. These are the numbers on Overview, and
/// Enter goes there. Cash is left out when the portfolio has a cash class: that row answers it.
pub fn figures(s: &State, query: &str) -> Vec<Answer> {
    let Some(summary) = &s.summary else {
        return Vec::new();
    };
    let has_cash_class = s
        .allocation
        .iter()
        .any(|item| item.asset_class.eq_ignore_ascii_case("cash"));
    let mut found = Vec::new();
    let mut offer = |words: &[&str], label: &str, figure: Figure| {
        if asks_for(query, label, words) {
            found.push(Answer {
                label: label.to_string(),
                figure,
                command: Command::Go(Page::Overview),
                class: None,
            });
        }
    };
    offer(
        &["total", "value", "worth", "portfolio", "net"],
        "Total value",
        Figure {
            value: s.money(summary.total_value),
            signed: None,
            share: None,
            today: Some(summary.day_change_percent),
        },
    );
    offer(
        &["today", "day", "daily", "change"],
        "Today's change",
        Figure {
            value: s.signed_money(summary.day_change),
            signed: Some(summary.day_change),
            share: None,
            today: Some(summary.day_change_percent),
        },
    );
    // The same words as the Overview stat. "Return" is not one of them: the performance page's
    // return is a different number, and this must not answer with the wrong one.
    offer(
        &["unrealized", "gain", "loss", "pnl", "profit"],
        "Unrealized PnL",
        Figure {
            value: s.signed_money(summary.total_pnl),
            signed: Some(summary.total_pnl),
            share: None,
            today: None,
        },
    );
    if !has_cash_class {
        offer(
            &["cash"],
            "Cash",
            Figure {
                value: s.money(summary.cash_value),
                signed: None,
                share: share_of(s, summary.cash_value),
                today: None,
            },
        );
    }
    offer(
        &["securities", "invested"],
        "Securities",
        Figure {
            value: s.money(summary.securities_value),
            signed: None,
            share: share_of(s, summary.securities_value),
            today: None,
        },
    );
    found
}

/// Today's change of a set of positions, in percent of what they were worth yesterday. A position
/// without a ticker (cash) doesn't move. One with a ticker but no change leaves the set unknown,
/// rather than a number invented from the rest. A set with nothing priced has no today's change.
fn change_of<'a>(positions: impl IntoIterator<Item = &'a PositionDto>) -> Option<f64> {
    let mut now = 0.0;
    let mut before = 0.0;
    let mut priced = false;
    for position in positions {
        let change = match (position.day_change_percent, &position.ticker) {
            (Some(change), _) => {
                priced = true;
                change
            }
            (None, None) => 0.0,
            (None, Some(_)) => return None,
        };
        now += position.value;
        before += position.value / (1.0 + change / 100.0);
    }
    if !priced || before.abs() <= f64::EPSILON {
        return None;
    }
    Some((now / before - 1.0) * 100.0)
}

/// The asset classes `query` names, with their value, share and today's change.
pub fn classes(s: &State, query: &str) -> Vec<Answer> {
    let query = query.trim().to_lowercase();
    if query.chars().count() < 2 {
        return Vec::new();
    }
    s.allocation
        .iter()
        .filter(|item| {
            let class = item.asset_class.to_lowercase();
            query.split_whitespace().all(|word| class.contains(word))
        })
        .map(|item| Answer {
            label: item.asset_class.clone(),
            figure: Figure {
                value: s.money(item.value),
                signed: None,
                share: Some(item.percent),
                today: change_of(
                    s.positions
                        .iter()
                        .filter(|position| position.asset_class == item.asset_class),
                ),
            },
            command: Command::Go(Page::Allocation),
            class: Some(item.asset_class.clone()),
        })
        .collect()
}

/// A holding's value, share and today's change.
pub fn holding(s: &State, position: &PositionDto) -> Figure {
    Figure {
        value: s.money(position.value),
        signed: None,
        share: share_of(s, position.value),
        today: position.day_change_percent,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use portfolio_rs::state::{AllocationItemDto, PortfolioSummaryDto};
    use tokio::runtime::Runtime;

    use super::*;
    use crate::{demo, format, state::State};

    fn position(class: &str, ticker: Option<&str>, value: f64, change: Option<f64>) -> PositionDto {
        PositionDto {
            id: 0,
            position_id: None,
            name: None,
            ticker: ticker.map(str::to_string),
            asset_class: class.to_string(),
            amount: 1.0,
            price: value,
            value,
            pnl: None,
            pnl_percent: None,
            day_change_percent: change,
            average_cost: None,
            invested: None,
            purchases: Vec::new(),
        }
    }

    fn summary(total: f64, day: f64, pnl: f64, cash: f64) -> PortfolioSummaryDto {
        PortfolioSummaryDto {
            total_value: total,
            total_invested: total - pnl,
            total_pnl: pnl,
            pnl_percent: 0.0,
            day_change: day,
            day_change_percent: if total == 0.0 {
                0.0
            } else {
                day / total * 100.0
            },
            cash_value: cash,
            securities_value: total - cash,
            allocation: HashMap::new(),
            positions: Vec::new(),
            has_stale: false,
            complete: true,
            data_status: "complete".to_string(),
        }
    }

    fn state() -> (Runtime, tempfile::TempDir, State) {
        let runtime = Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let state = State::new(
            demo::backend(dir.path(), None, runtime.handle().clone()),
            None,
        );
        (runtime, dir, state)
    }

    #[test]
    fn a_query_asks_for_a_figure_by_the_start_of_its_words() {
        assert!(asks_for("tot", "Total value", &["total"]));
        assert!(asks_for("Total value", "Total value", &["total", "value"]));
        assert!(asks_for(
            "today's change",
            "Today's change",
            &["today", "change"]
        ));
        assert!(
            !asks_for("to", "Total value", &["total"]),
            "too short to mean it"
        );
        assert!(!asks_for("toggle", "Total value", &["total", "today"]));
        assert!(!asks_for(
            "total bitcoin",
            "Total value",
            &["total", "value"]
        ));
        assert!(
            !asks_for("return", "Unrealized PnL", &["pnl", "gain"]),
            "a return is a different number"
        );
    }

    #[test]
    fn today_s_change_counts_cash_as_still_and_an_unknown_price_as_unknown() {
        // 110 now after +10%, and 100 of cash: 210 against 200.
        let both = [
            position("Stocks", Some("SPY"), 110.0, Some(10.0)),
            position("Cash", None, 100.0, None),
        ];
        assert!((change_of(&both).unwrap() - 5.0).abs() < 1e-9);
        let unknown = [
            position("Stocks", Some("SPY"), 110.0, Some(10.0)),
            position("Bonds", Some("TLT"), 50.0, None),
        ];
        assert_eq!(change_of(&unknown), None);
        assert_eq!(change_of(&[]), None);
        assert_eq!(
            change_of(&[position("Cash", None, 100.0, None)]),
            None,
            "cash has no price to have moved"
        );
    }

    #[test]
    fn figures_answer_what_was_asked_and_hide_amounts() {
        let (_runtime, _dir, mut state) = state();
        state.summary = Some(summary(200.0, 10.0, 50.0, 40.0));

        assert!(figures(&state, "to").is_empty());
        assert!(figures(&state, "toggle").is_empty());
        assert!(figures(&state, "return").is_empty());

        let total = figures(&state, "total");
        assert_eq!(total.len(), 1);
        assert_eq!(total[0].label, "Total value");
        assert_eq!(total[0].figure.value, "€200.00");
        assert_eq!(total[0].figure.today, Some(5.0));
        assert_eq!(total[0].command, Command::Go(Page::Overview));

        let today = figures(&state, "today's");
        assert_eq!(today[0].label, "Today's change");
        assert_eq!(today[0].figure.value, "+€10.00");
        assert_eq!(today[0].figure.signed, Some(10.0));

        let gain = figures(&state, "pnl");
        assert_eq!(gain[0].label, "Unrealized PnL");
        assert_eq!(gain[0].figure.value, "+€50.00");

        let cash = figures(&state, "cash");
        assert_eq!(cash[0].label, "Cash");
        assert_eq!(cash[0].figure.share, Some(20.0));

        state.allocation.push(AllocationItemDto {
            asset_class: "Cash".to_string(),
            percent: 20.0,
            value: 40.0,
        });
        assert!(
            figures(&state, "cash").is_empty(),
            "the cash class answers it, once"
        );

        state.privacy = true;
        assert_eq!(figures(&state, "total")[0].figure.value, format::HIDDEN);
        assert_eq!(figures(&state, "today")[0].figure.value, format::HIDDEN);
    }

    #[test]
    fn a_class_answers_with_its_share_and_only_a_known_day() {
        let (_runtime, _dir, mut state) = state();
        state.summary = Some(summary(200.0, 0.0, 0.0, 0.0));
        state.allocation.push(AllocationItemDto {
            asset_class: "Stocks".to_string(),
            percent: 55.0,
            value: 110.0,
        });
        state
            .positions
            .push(position("Stocks", Some("SPY"), 110.0, Some(10.0)));

        assert!(classes(&state, "s").is_empty(), "one letter is not a class");
        let stocks = classes(&state, "sto");
        assert_eq!(stocks.len(), 1);
        assert_eq!(stocks[0].label, "Stocks");
        assert_eq!(stocks[0].figure.value, "€110.00");
        assert_eq!(stocks[0].figure.share, Some(55.0));
        assert!((stocks[0].figure.today.unwrap() - 10.0).abs() < 1e-9);
        assert_eq!(stocks[0].command, Command::Go(Page::Allocation));
        assert_eq!(stocks[0].class.as_deref(), Some("Stocks"));

        state.positions[0].day_change_percent = None;
        assert_eq!(
            classes(&state, "stocks")[0].figure.today,
            None,
            "a missing price is not zero"
        );

        state.privacy = true;
        assert_eq!(classes(&state, "stocks")[0].figure.value, format::HIDDEN);
    }

    #[test]
    fn a_holding_carries_its_value_share_and_day() {
        let (_runtime, _dir, mut state) = state();
        state.summary = Some(summary(200.0, 0.0, 0.0, 0.0));
        let spy = position("Stocks", Some("SPY"), 50.0, Some(-1.5));
        let figure = holding(&state, &spy);
        assert_eq!(figure.value, "€50.00");
        assert_eq!(figure.share, Some(25.0));
        assert_eq!(figure.today, Some(-1.5));

        state.privacy = true;
        assert_eq!(holding(&state, &spy).value, format::HIDDEN);
    }
}
