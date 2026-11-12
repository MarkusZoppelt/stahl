//! Every figure Ask is allowed to quote, each with a stable id and a typed value.
//!
//! The model never writes a number about the portfolio. It writes `{class.stocks.share}`, and the
//! app shows the figure this table holds under that id. So the table is the whole vocabulary of
//! what an answer may say about the money, built only from what the app already computes: the
//! snapshot, the review, the statement's [`Change`], and the positions.
//!
//! [`build`] is pure: the same portfolio always gives the same table, in the same order.

use std::collections::HashMap;

use chrono::NaiveDate;
use portfolio_rs::{
    review::Review,
    state::{AllocationItemDto, PortfolioSummaryDto, PositionDto},
};

use crate::{format, statement::Change};

/// A figure, as a type rather than a string: the view formats it the way the rest of the app does,
/// so a chip is never a different kind of number from the one beside it.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// An amount, unsigned.
    Money(f64),
    /// An amount that can be negative, with its sign.
    SignedMoney(f64),
    /// A share or a return, in percent, unsigned.
    Percent(f64),
    /// A change, in percent, with its sign.
    SignedPercent(f64),
    /// Percentage points away from a target, unsigned: "12.8 points".
    Points(f64),
    /// A day, named as the Statement names it ("yesterday", "Tuesday", "October 14"), so an
    /// answer and the Overview never disagree about what to call the same date.
    Day {
        date: NaiveDate,
        today: NaiveDate,
    },
    Count(usize),
    /// A name: a policy, a holding, a class.
    Name(String),
}

impl Value {
    /// The figure as the app writes it. Privacy mode hides amounts and keeps shares, percentages
    /// and names, exactly as the Statement does.
    pub fn render(&self, hidden: bool, currency: &str) -> String {
        match self {
            Value::Money(value) => {
                if hidden {
                    format::HIDDEN.to_string()
                } else {
                    format::currency(*value, currency)
                }
            }
            Value::SignedMoney(value) => {
                if hidden {
                    format::HIDDEN.to_string()
                } else {
                    format::signed_currency(*value, currency)
                }
            }
            Value::Percent(value) => format!("{:.1}%", value.abs()),
            Value::SignedPercent(value) => format!(
                "{}{:.1}%",
                if *value >= 0.0 { "+" } else { "-" },
                value.abs()
            ),
            Value::Points(value) => format!("{:.1} points", value.abs()),
            Value::Day { date, today } => crate::statement::day_name(*date, *today),
            Value::Count(count) => count.to_string(),
            Value::Name(name) => name.clone(),
        }
    }

    /// Whether the figure is an amount, which privacy mode hides. Used to hide a whole block.
    pub fn is_amount(&self) -> bool {
        matches!(self, Value::Money(_) | Value::SignedMoney(_))
    }
}

/// One figure, with the words around it that an answer would use.
#[derive(Clone, Debug, PartialEq)]
pub struct Fact {
    pub id: String,
    /// What the figure is, for a chip's tooltip and for the model's instructions.
    pub label: String,
    /// The same, short enough to sit over a figure: "Total", "Crypto share".
    pub short: String,
    pub value: Value,
    /// The asset class it belongs to, so a chip can wear its colour.
    pub class: Option<String>,
}

/// The whole table, in a stable order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Table {
    pub facts: Vec<Fact>,
}

impl Table {
    pub fn get(&self, id: &str) -> Option<&Fact> {
        self.facts.iter().find(|fact| fact.id == id)
    }

    pub fn insert(&mut self, fact: Fact) {
        self.facts.push(fact);
    }

    /// The table as the model is shown it: one line per id, its label and its current value.
    pub fn describe(&self, hidden: bool, currency: &str) -> String {
        self.facts
            .iter()
            .map(|fact| {
                format!(
                    "{} = {} ({})",
                    fact.id,
                    fact.value.render(hidden, currency),
                    fact.label
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// What the table is built from. Everything is borrowed from the app's own state, so nothing here
/// can drift from what the pages show.
pub struct Input<'a> {
    pub summary: Option<&'a PortfolioSummaryDto>,
    pub allocation: &'a [AllocationItemDto],
    pub positions: &'a [PositionDto],
    pub review: Option<&'a Review>,
    pub change: Option<&'a Change>,
    pub policy_name: Option<&'a str>,
    pub currency: &'a str,
    /// The day the app is showing, so a date reads as the Statement names it.
    pub today: NaiveDate,
    /// Privacy mode only matters for the words in [`Table::describe`].
    pub hidden: bool,
}

/// A class name as an id: lower case, and anything that isn't a letter or a digit is a dash.
pub fn slug(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for character in name.trim().chars() {
        if character.is_alphanumeric() {
            out.extend(character.to_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    out.trim_end_matches('-').to_string()
}

/// A holding's stable id: the one it was made with, or its number.
pub fn holding_id(position: &PositionDto) -> String {
    position
        .position_id
        .clone()
        .unwrap_or_else(|| format!("p{}", position.id))
}

fn share_of(value: f64, total: f64) -> Option<f64> {
    (total.abs() >= 0.005).then(|| value / total * 100.0)
}

fn fact(id: String, label: String, short: String, value: Value) -> Fact {
    Fact {
        id,
        label,
        short,
        value,
        class: None,
    }
}

/// Every figure the app can vouch for, in a fixed order.
pub fn build(input: &Input) -> Table {
    let mut table = Table::default();
    let mut push = |id: &str, label: String, short: &str, value: Value| {
        table.insert(fact(id.to_string(), label, short.to_string(), value));
    };

    if let Some(summary) = input.summary {
        let total = summary.total_value;
        push(
            "total",
            "The portfolio's total value".to_string(),
            "Total",
            Value::Money(total),
        );
        push(
            "today.change",
            "What the portfolio moved today".to_string(),
            "Today",
            Value::SignedMoney(summary.day_change),
        );
        push(
            "today.percent",
            "Today's move, in percent".to_string(),
            "Today",
            Value::SignedPercent(summary.day_change_percent),
        );
        push(
            "unrealized.pnl",
            "Unrealized gain or loss".to_string(),
            "Unrealized",
            Value::SignedMoney(summary.total_pnl),
        );
        push(
            "unrealized.percent",
            "Unrealized gain or loss, in percent".to_string(),
            "Unrealized",
            Value::SignedPercent(summary.pnl_percent),
        );
        push(
            "cash.value",
            "What is held in cash".to_string(),
            "Cash",
            Value::Money(summary.cash_value),
        );
        push(
            "securities.value",
            "What is held in securities".to_string(),
            "Securities",
            Value::Money(summary.securities_value),
        );
        if let Some(share) = share_of(summary.cash_value, total) {
            push(
                "cash.share",
                "Cash's share of the portfolio".to_string(),
                "Cash share",
                Value::Percent(share),
            );
        }
        if let Some(share) = share_of(summary.securities_value, total) {
            push(
                "securities.share",
                "Securities' share of the portfolio".to_string(),
                "Securities share",
                Value::Percent(share),
            );
        }
    }

    if let Some(change) = input.change {
        push(
            "since.change",
            "What the portfolio changed by since the last look".to_string(),
            "Since you last looked",
            Value::SignedMoney(change.total()),
        );
        if let Some(share) = share_of(change.total(), change.start) {
            push(
                "since.percent",
                "The change since the last look, in percent".to_string(),
                "Since you last looked",
                Value::SignedPercent(share),
            );
        }
        push(
            "since.market",
            "How much of that change was the market".to_string(),
            "From the market",
            Value::SignedMoney(change.market()),
        );
        push(
            "since.added",
            "Money added since the last look, or taken out when negative".to_string(),
            "Money in or out",
            Value::SignedMoney(change.added),
        );
        push(
            "since.date",
            "The day the change is measured from".to_string(),
            "Since",
            Value::Day {
                date: change.from,
                today: input.today,
            },
        );
        if let Some(leader) = change.leader.as_ref() {
            push(
                "since.leader",
                "The holding that moved the portfolio most since the last look".to_string(),
                "Moved it most",
                Value::Name(leader.name.clone()),
            );
            push(
                "since.leader.amount",
                "How much that holding moved it".to_string(),
                "From it",
                Value::SignedMoney(leader.amount),
            );
        }
    }

    push(
        "count.positions",
        "How many holdings the portfolio has".to_string(),
        "Holdings",
        Value::Count(input.positions.len()),
    );
    push(
        "count.classes",
        "How many asset classes it has".to_string(),
        "Classes",
        Value::Count(input.allocation.len()),
    );
    if let Some(name) = input.policy_name.filter(|name| !name.trim().is_empty()) {
        push(
            "policy.name",
            "The name of the policy".to_string(),
            "Policy",
            Value::Name(name.to_string()),
        );
    }

    let total = input.summary.map_or(0.0, |summary| summary.total_value);
    for item in input.allocation {
        let class = &item.asset_class;
        let id = slug(class);
        let review = input.review.and_then(|review| {
            review
                .allocations
                .iter()
                .find(|each| each.asset_class.eq_ignore_ascii_case(class))
        });
        let mut add = |suffix: &str, label: String, short: &str, value: Value| {
            table.insert(Fact {
                id: format!("class.{id}.{suffix}"),
                label,
                short: short.to_string(),
                value,
                class: Some(class.clone()),
            });
        };
        add(
            "value",
            format!("What {class} is worth"),
            class,
            Value::Money(item.value),
        );
        add(
            "share",
            format!("{class}'s share of the portfolio"),
            &format!("{class} share"),
            Value::Percent(item.percent),
        );
        if let Some(review) = review {
            add(
                "target",
                format!("{class}'s policy target share"),
                &format!("{class} target"),
                Value::Percent(review.target_percent),
            );
            add(
                "drift",
                format!("How far {class} is from its target"),
                &format!("{class} drift"),
                Value::Points(review.drift_percent),
            );
            if let Some(tolerance) = review.tolerance_percent {
                add(
                    "tolerance",
                    format!("How far {class} may drift before it needs attention"),
                    &format!("{class} band"),
                    Value::Points(tolerance),
                );
            }
        }
        if let Some(today) = crate::view::answers::change_of(
            input
                .positions
                .iter()
                .filter(|position| position.asset_class.eq_ignore_ascii_case(class)),
        ) {
            add(
                "today",
                format!("How {class} moved today, in percent"),
                &format!("{class} today"),
                Value::SignedPercent(today),
            );
        }
    }

    for position in input.positions {
        let id = holding_id(position);
        let name = position
            .name
            .clone()
            .or_else(|| position.ticker.clone())
            .unwrap_or_else(|| "A holding".to_string());
        let mut add = |suffix: &str, label: String, short: &str, value: Value| {
            table.insert(Fact {
                id: format!("holding.{id}.{suffix}"),
                label,
                short: short.to_string(),
                value,
                class: Some(position.asset_class.clone()),
            });
        };
        add(
            "name",
            format!("The name of the holding {name}"),
            &name,
            Value::Name(name.clone()),
        );
        add(
            "value",
            format!("What {name} is worth"),
            &name,
            Value::Money(position.value),
        );
        if let Some(share) = share_of(position.value, total) {
            add(
                "share",
                format!("{name}'s share of the portfolio"),
                &format!("{name} share"),
                Value::Percent(share),
            );
        }
        if let Some(today) = position.day_change_percent {
            add(
                "today",
                format!("How {name} moved today, in percent"),
                &format!("{name} today"),
                Value::SignedPercent(today),
            );
        }
        if let Some(pnl) = position.pnl {
            add(
                "pnl",
                format!("{name}'s unrealized gain or loss"),
                &format!("{name} gain"),
                Value::SignedMoney(pnl),
            );
        }
    }

    table
}

/// The table for a state: what [`build`] makes of everything the app is showing.
pub fn of(s: &crate::state::State) -> Table {
    let currency = s.currency();
    let policy_name = s
        .review
        .data
        .as_ref()
        .map(|review| review.policy_name.as_str());
    build(&Input {
        summary: s.summary.as_ref(),
        allocation: &s.allocation,
        positions: &s.positions,
        review: s.review.data.as_ref(),
        change: s.change.as_ref(),
        policy_name,
        currency: &currency,
        today: chrono::Local::now().date_naive(),
        hidden: s.privacy,
    })
}

/// Every id the model may use, as the prompt lists them.
pub fn vocabulary(table: &Table) -> Vec<String> {
    table.facts.iter().map(|fact| fact.id.clone()).collect()
}

/// The ids grouped by kind, for a test that wants to know what exists without reading every one.
pub fn by_kind(table: &Table) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for fact in &table.facts {
        let kind = fact.id.split('.').next().unwrap_or("").to_string();
        *counts.entry(kind).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use portfolio_rs::{review::AllocationReview, state::AllocationItemDto};

    use super::*;
    use crate::{demo, format, state::State};

    fn position(class: &str, id: &str, value: f64, change: Option<f64>) -> PositionDto {
        PositionDto {
            id: 0,
            position_id: Some(id.to_string()),
            name: Some(format!("Holding {id}")),
            ticker: None,
            asset_class: class.to_string(),
            amount: 1.0,
            price: value,
            value,
            pnl: Some(10.0),
            pnl_percent: None,
            day_change_percent: change,
            average_cost: None,
            invested: None,
            purchases: Vec::new(),
        }
    }

    fn summary(total: f64) -> PortfolioSummaryDto {
        PortfolioSummaryDto {
            total_value: total,
            total_invested: total,
            total_pnl: 50.0,
            pnl_percent: 2.5,
            day_change: 10.0,
            day_change_percent: 0.5,
            cash_value: 40.0,
            securities_value: total - 40.0,
            allocation: HashMap::new(),
            positions: Vec::new(),
            has_stale: false,
            complete: true,
            data_status: "complete".to_string(),
        }
    }

    fn review() -> Review {
        Review {
            portfolio_value: 200.0,
            currency: "EUR".to_string(),
            policy_name: "Balanced Growth".to_string(),
            findings: Vec::new(),
            allocations: vec![AllocationReview {
                asset_class: "Stocks".to_string(),
                target_percent: 50.0,
                tolerance_percent: Some(5.0),
                actual_percent: 55.0,
                drift_percent: 5.0,
                within_tolerance: true,
                value: 110.0,
            }],
            constraint_checks: Vec::new(),
            data_quality: Vec::new(),
            suggested_actions: Vec::new(),
        }
    }

    fn table() -> Table {
        let summary = summary(200.0);
        let allocation = vec![AllocationItemDto {
            asset_class: "Stocks".to_string(),
            percent: 55.0,
            value: 110.0,
        }];
        let positions = vec![position("Stocks", "pos_spy", 110.0, Some(-1.5))];
        let review = review();
        build(&Input {
            summary: Some(&summary),
            allocation: &allocation,
            positions: &positions,
            review: Some(&review),
            change: Some(&Change {
                from: NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
                start: 190.0,
                end: 200.0,
                added: 0.0,
                leader: None,
                previous_high: None,
                history_from: None,
            }),
            policy_name: Some("Balanced Growth"),
            currency: "EUR",
            today: NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
            hidden: false,
        })
    }

    #[test]
    fn every_figure_the_answer_may_quote_has_a_typed_value() {
        let table = table();
        assert_eq!(table.get("total").unwrap().value, Value::Money(200.0));
        assert_eq!(
            table.get("today.change").unwrap().value,
            Value::SignedMoney(10.0)
        );
        assert_eq!(
            table.get("class.stocks.share").unwrap().value,
            Value::Percent(55.0)
        );
        assert_eq!(
            table.get("class.stocks.target").unwrap().value,
            Value::Percent(50.0)
        );
        assert_eq!(
            table.get("class.stocks.drift").unwrap().value,
            Value::Points(5.0)
        );
        assert_eq!(
            table.get("class.stocks.today").unwrap().value,
            Value::SignedPercent(
                crate::view::answers::change_of([&position(
                    "Stocks",
                    "pos_spy",
                    110.0,
                    Some(-1.5)
                )])
                .unwrap()
            )
        );
        assert_eq!(
            table.get("holding.pos_spy.value").unwrap().value,
            Value::Money(110.0)
        );
        let percent = |id: &str| match table.get(id).unwrap().value {
            Value::Percent(percent) => percent,
            ref other => panic!("{id} is {other:?}, not a share"),
        };
        assert!((percent("holding.pos_spy.share") - 55.0).abs() < 1e-9);
        assert_eq!(
            table.get("since.change").unwrap().value,
            Value::SignedMoney(10.0)
        );
        assert_eq!(
            table.get("since.date").unwrap().value,
            Value::Day {
                date: NaiveDate::from_ymd_opt(2026, 11, 5).unwrap(),
                today: NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
            }
        );
        assert_eq!(
            table.get("policy.name").unwrap().value,
            Value::Name("Balanced Growth".to_string())
        );
        assert_eq!(table.get("count.positions").unwrap().value, Value::Count(1));
        assert_eq!(table.get("cash.share").unwrap().value, Value::Percent(20.0));
    }

    #[test]
    fn figures_render_as_the_rest_of_the_app_writes_them() {
        let table = table();
        let render = |id: &str| table.get(id).unwrap().value.render(false, "EUR");
        assert_eq!(render("total"), "€200.00");
        assert_eq!(render("today.change"), "+€10.00");
        assert_eq!(render("today.percent"), "+0.5%");
        assert_eq!(render("class.stocks.drift"), "5.0 points");
        assert_eq!(render("class.stocks.today"), "-1.5%");
        assert_eq!(
            render("since.date"),
            "Thursday",
            "named as the Statement names it, not as a bare date"
        );
        assert_eq!(render("policy.name"), "Balanced Growth");
        assert_eq!(render("count.positions"), "1");
        // Privacy hides amounts and keeps shares, percentages and names.
        assert_eq!(
            table.get("total").unwrap().value.render(true, "EUR"),
            format::HIDDEN
        );
        assert_eq!(
            table
                .get("class.stocks.share")
                .unwrap()
                .value
                .render(true, "EUR"),
            "55.0%"
        );
        assert_eq!(
            table
                .get("class.stocks.today")
                .unwrap()
                .value
                .render(true, "EUR"),
            "-1.5%"
        );
    }

    #[test]
    fn a_portfolio_without_a_policy_or_history_has_fewer_figures_and_no_invented_ones() {
        let summary = summary(100.0);
        let allocation = vec![AllocationItemDto {
            asset_class: "Cash".to_string(),
            percent: 100.0,
            value: 100.0,
        }];
        let table = build(&Input {
            summary: Some(&summary),
            allocation: &allocation,
            positions: &[],
            review: None,
            change: None,
            policy_name: None,
            currency: "EUR",
            today: NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
            hidden: false,
        });
        assert!(table.get("policy.name").is_none());
        assert!(table.get("since.change").is_none());
        assert!(table.get("class.cash.target").is_none());
        assert!(table.get("class.cash.share").is_some());
        assert!(table.get("class.cash.today").is_none(), "cash has no day");
    }

    #[test]
    fn a_class_name_becomes_a_stable_id() {
        assert_eq!(slug("Stocks"), "stocks");
        assert_eq!(slug("Real Estate"), "real-estate");
        assert_eq!(slug("  Gold  "), "gold");
        assert_eq!(slug("Cash & Equivalents"), "cash-equivalents");
        assert_eq!(slug(""), "");
    }

    #[test]
    fn the_description_lists_every_id_once() {
        let table = table();
        let text = table.describe(false, "EUR");
        let mut ids: Vec<&str> = text
            .lines()
            .filter_map(|line| line.split(" = ").next())
            .collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), count);
        assert!(text.contains("class.stocks.share = 55.0%"));
    }

    #[test]
    fn a_real_demo_state_builds_the_whole_table() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("portfolio");
        demo::seed_workspace(&workspace);
        let mut state = State::new(
            demo::backend(dir.path(), Some(&workspace), runtime.handle().clone()),
            None,
        );
        state.startup();
        state.settle();
        let table = of(&state);
        assert!(table.get("total").is_some());
        assert!(table.get("class.stocks.share").is_some());
        assert!(table.get("holding.pos_btc.value").is_some());
        assert!(
            table.get("class.stocks.target").is_some(),
            "the policy gives each class a target"
        );
        assert_eq!(
            by_kind(&table).get("holding"),
            Some(&(state.positions.len() * 4 + 3)),
            "each holding has a name and a value; a priced one has a share and a day too"
        );
    }
}
