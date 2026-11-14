//! Matters for the owner's signature: what the bank has prepared, and what happened to the last
//! decision of each kind.
//!
//! A private bank does not hand its client a list of problems. It prepares what needs deciding,
//! with the figures, and brings it for signature. [`prepare`] turns the same review and findings
//! the Overview already reads into two lists: matters that can be signed, declined or put off,
//! and the data fixes that need doing but not signing. It is pure; [`State::prepared`] feeds it.
//!
//! Signing records a decision in the workspace's decisions folder. It never places an order:
//! Stahl has no way to trade, and every matter says so.

use std::{
    collections::BTreeMap,
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
};

use chrono::{Datelike as _, Duration, Local, NaiveDate};
use portfolio_rs::{
    policy::Policy,
    review::{AllocationReview as ReviewAllocation, Review},
    services::atomic_write,
    state::AllocationItemDto,
};
use serde::{Deserialize, Serialize};

use crate::{
    agenda::{self, Action as AgendaAction, Item as AgendaItem, Weight},
    backend::{Backend, BackendResult},
    format,
    rebalance::{self, Class, Plan},
    state::{Page, State},
};

/// How long the Sign control is held before the decision is recorded.
pub const HOLD: f64 = 1.0;
/// How long the seal stays on the card after it is stamped, before the card files itself away.
pub const BREATH: f64 = 1.15;
/// How long "Hold to sign" stays after a press that let go too soon.
const HINT_FOR: f64 = 2.5;
/// The part of the way to the policy's targets the default plan goes.
pub const HALFWAY: f64 = 0.5;
/// The file beside the history that remembers what was signed, declined and put off.
const LEDGER_FILE: &str = "matters.json";

// --- What a matter is ---

/// What kind of decision a matter asks for. Data fixes are not decisions and are not signed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// Move every class part of the way to its target.
    Rebalance,
    /// Put cash above its band to work, buying only.
    PlaceCash,
    /// Sell enough to bring cash back to the policy's minimum.
    TopUpCash,
}

impl Kind {
    /// Matters of one group answer the same question, so a signed one settles the others.
    pub fn group(self) -> Option<&'static str> {
        match self {
            Kind::Rebalance | Kind::PlaceCash | Kind::TopUpCash => Some("allocation"),
        }
    }

    /// The words a past decision of this kind is remembered by.
    fn summary(self) -> &'static str {
        match self {
            Kind::Rebalance => "a halfway rebalance",
            Kind::PlaceCash => "putting the excess cash to work",
            Kind::TopUpCash => "topping up cash",
        }
    }

    /// The small word over a matter, so the list can be read at a glance.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Rebalance => "Rebalance",
            Kind::PlaceCash => "Cash",
            Kind::TopUpCash => "Cash floor",
        }
    }
}

/// One trade the plan would make.
#[derive(Clone, Debug, PartialEq)]
pub struct TradeLine {
    pub class: String,
    /// Buy, rather than sell.
    pub buy: bool,
    pub amount: f64,
}

/// One asset class before and after the plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub class: String,
    pub before: f64,
    pub after: f64,
    pub target: Option<f64>,
}

/// The concrete thing a matter proposes, with its figures.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Proposal {
    /// One sentence naming what would happen.
    pub summary: String,
    pub trades: Vec<TradeLine>,
    pub rows: Vec<Row>,
    /// What the plan is worth before any money in or out.
    pub total: f64,
    /// Money in (positive) or out (negative). Always zero for these plans.
    pub external: f64,
    /// A note under the trades: buying only, selling only, and so on.
    pub note: String,
}

impl Proposal {
    /// What the plan does, in money, for a record.
    pub fn traded(&self) -> (f64, f64) {
        let bought = self
            .trades
            .iter()
            .filter(|trade| trade.buy)
            .map(|trade| trade.amount)
            .sum();
        let sold = self
            .trades
            .iter()
            .filter(|trade| !trade.buy)
            .map(|trade| trade.amount)
            .sum();
        (bought, sold)
    }

    fn money_note(&self, money: impl Fn(f64) -> String) -> String {
        let (bought, sold) = self.traded();
        match (bought >= 0.01, sold >= 0.01) {
            (true, true) => format!(
                "Buy {} and sell {}. No money in or out.",
                money(bought),
                money(sold)
            ),
            (true, false) => format!("Buy {} only. No money in or out.", money(bought)),
            (false, true) => format!("Sell {} only. No money in or out.", money(sold)),
            (false, false) => "No trades.".to_string(),
        }
    }
}

/// A prepared decision, or a data fix, ready for the Overview.
#[derive(Clone, Debug, PartialEq)]
pub struct Matter {
    /// Stable across renders and across days, for keys and for the ledger.
    pub id: String,
    pub kind: Kind,
    pub weight: Weight,
    pub title: String,
    /// Two or three sentences in the bank's voice.
    pub reasoning: String,
    /// What happened to the last decision of this kind, when it can be computed honestly.
    pub prior: Option<String>,
    pub proposal: Proposal,
    /// What signing records, in one line.
    pub records: String,
    /// The page that shows the plan behind the proposal.
    pub adjust: Option<AgendaAction>,
    /// The numbers this matter is about, for telling a material change from a quiet one.
    pub figures: BTreeMap<String, f64>,
    /// How far each figure has to move before the matter is worth raising again.
    pub steps: BTreeMap<String, f64>,
    /// The classes outside their tolerance when the matter was prepared, sorted.
    pub out_of_band: Vec<String>,
    pub policy: String,
}

/// Everything the Overview's prepared section shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Prepared {
    /// Decisions to sign, most important first.
    pub matters: Vec<Matter>,
    /// Data fixes, most important first.
    pub todo: Vec<AgendaItem>,
}

impl Prepared {
    /// The matters to show as cards, then the alternatives, each a line. A group answers one
    /// question, so it gets one card: the `shown` matter if it is prepared, else the first.
    pub fn cards(&self, shown: Option<&str>) -> (Vec<&Matter>, Vec<&Matter>) {
        let mut cards: Vec<&Matter> = Vec::new();
        let mut alternatives = Vec::new();
        for matter in &self.matters {
            let Some(group) = matter.kind.group() else {
                cards.push(matter);
                continue;
            };
            let members = || {
                self.matters
                    .iter()
                    .filter(move |other| other.kind.group() == Some(group))
            };
            let lead = members()
                .find(|other| Some(other.id.as_str()) == shown)
                .or_else(|| members().next());
            if lead.is_some_and(|lead| lead.id == matter.id) {
                cards.push(matter);
            } else {
                alternatives.push(matter);
            }
        }
        (cards, alternatives)
    }
}

/// Everything [`prepare`] may read.
pub struct Inputs<'a> {
    pub agenda: agenda::Inputs<'a>,
    pub allocation: &'a [AllocationItemDto],
    /// The policy, for its minimum cash and its name.
    pub policy: Option<&'a Policy>,
    /// Whether amounts are hidden, as on the screen they were read from.
    pub privacy: bool,
    pub currency: String,
}

impl Inputs<'_> {
    /// Money as the screen shows it: hidden in privacy mode.
    fn money(&self, value: f64) -> String {
        if self.privacy {
            format::HIDDEN.to_string()
        } else {
            format::currency(value, &self.currency)
        }
    }
}

fn points(value: f64) -> String {
    format!("{:.1}", value.abs())
}

fn over_under(value: f64) -> &'static str {
    if value > 0.0 { "over" } else { "under" }
}

/// The classes the plan moves, from the review and the allocation on screen.
fn classes(review: &Review, allocation: &[AllocationItemDto]) -> Vec<Class> {
    let mut out = Vec::new();
    for item in &review.allocations {
        let value = allocation
            .iter()
            .find(|current| current.asset_class.eq_ignore_ascii_case(&item.asset_class))
            .map_or(item.value, |current| current.value);
        out.push(class(
            &item.asset_class,
            value,
            Some(item.target_percent),
            item.tolerance_percent,
        ));
    }
    for item in allocation {
        if out
            .iter()
            .any(|class| class.name.eq_ignore_ascii_case(&item.asset_class))
        {
            continue;
        }
        out.push(class(&item.asset_class, item.value, None, None));
    }
    // Cash pays for the rest, so it reads last, whatever order the policy lists it in.
    out.sort_by_key(|class| class.cash);
    out
}

fn class(name: &str, value: f64, target: Option<f64>, tolerance: Option<f64>) -> Class {
    let mut class = Class::new(name, value);
    if let Some(target) = target {
        class = class.target(target);
    }
    if let Some(tolerance) = tolerance {
        class = class.tolerance(tolerance);
    }
    if name.eq_ignore_ascii_case("cash") {
        class = class.cash();
    }
    class
}

/// The classes outside their tolerance, sorted, for the ledger.
fn out_of_band(review: &Review) -> Vec<String> {
    let mut names: Vec<String> = review
        .allocations
        .iter()
        .filter(|class| !class.within_tolerance)
        .map(|class| class.asset_class.clone())
        .collect();
    names.sort();
    names
}

/// The figures a matter is judged by: each class's drift, and anything else it is about.
fn figures(classes: &[Class], review: &Review) -> (BTreeMap<String, f64>, BTreeMap<String, f64>) {
    let mut figures = BTreeMap::new();
    let mut steps = BTreeMap::new();
    for class in classes {
        let Some(allocation) = review
            .allocations
            .iter()
            .find(|allocation| allocation.asset_class.eq_ignore_ascii_case(&class.name))
        else {
            continue;
        };
        figures.insert(format!("class:{}", class.name), allocation.drift_percent);
        steps.insert(
            format!("class:{}", class.name),
            allocation
                .tolerance_percent
                .unwrap_or(rebalance::DEFAULT_TOLERANCE)
                .max(0.5),
        );
    }
    (figures, steps)
}

fn row(classes: &[Class], plan: &Plan) -> Vec<Row> {
    let total = plan.total.max(1e-9);
    classes
        .iter()
        .zip(&plan.values)
        .map(|(class, value)| Row {
            class: class.name.clone(),
            before: class.held_value() / total * 100.0,
            after: value / total * 100.0,
            target: class.target,
        })
        .collect()
}

fn trades(plan: &Plan) -> Vec<TradeLine> {
    plan.trades
        .iter()
        .map(|trade| TradeLine {
            class: trade.asset_class.clone(),
            buy: trade.action == rebalance::Action::Increase,
            amount: trade.amount,
        })
        .collect()
}

/// What a past decision of this kind is called in a sentence.
fn past(entry: &Entry) -> String {
    match entry.status {
        Status::Signed => format!("You signed {}", entry.summary),
        Status::Declined => format!("You declined {}", entry.summary),
        Status::Postponed => format!("You put off {}", entry.summary),
    }
}

/// The widest drift a matter's figures carry, ignoring the figures that are not class drifts.
fn widest(figures: &BTreeMap<String, f64>) -> Option<f64> {
    figures
        .iter()
        .filter(|(key, _)| key.starts_with("class:"))
        .map(|(_, value)| value.abs())
        .reduce(f64::max)
}

/// What the last decision of this kind left, in the present tense of the numbers on screen.
fn prior_line(entry: &Entry, matter: &Matter) -> Option<String> {
    let then = widest(&entry.figures)?;
    let now = widest(&matter.figures)?;
    let date = date_words(entry.at_date()?);
    Some(format!(
        "{} on {}. Since then the widest drift went from {} to {} points.",
        past(entry),
        date,
        points(then),
        points(now)
    ))
}

/// "12 November 2026", for a card that has just been signed.
pub fn long_date(date: NaiveDate) -> String {
    format!("{} {} {}", date.day(), month_name(date), date.year())
}

/// "12 NOV", as a seal carries it.
pub fn seal_date(date: NaiveDate) -> String {
    let month: String = month_name(date)
        .chars()
        .take(3)
        .collect::<String>()
        .to_uppercase();
    format!("{} {month}", date.day())
}

fn month_name(date: NaiveDate) -> String {
    const MONTHS: [&str; 12] = [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ];
    MONTHS
        .get(date.month0() as usize)
        .copied()
        .unwrap_or_default()
        .to_string()
}

/// "3 October", or "3 October 2025" for another year.
fn date_words(date: NaiveDate) -> String {
    let month = month_name(date);
    if date.year() == Local::now().date_naive().year() {
        format!("{} {month}", date.day())
    } else {
        format!("{} {month} {}", date.day(), date.year())
    }
}

// --- The matters the data supports ---

fn rebalance_matter(
    review: &Review,
    classes: &[Class],
    money: &dyn Fn(f64) -> String,
) -> Option<Matter> {
    let outside: Vec<_> = review
        .allocations
        .iter()
        .filter(|class| !class.within_tolerance)
        .collect();
    if outside.is_empty() {
        return None;
    }
    let plan = rebalance::blend(classes, HALFWAY);
    if plan.trades.is_empty() {
        return None;
    }
    // Furthest past its tolerance first, so the sentence names what matters most.
    let excess = |class: &&ReviewAllocation| {
        class.drift_percent.abs() / class.tolerance_percent.unwrap_or(5.0).max(0.1)
    };
    let mut sorted = outside.clone();
    sorted.sort_by(|a, b| excess(b).total_cmp(&excess(a)));
    let lead = sorted[0];
    let sentence = if sorted.len() == 1 {
        format!(
            "{} is {} points {} its target, more than the {} you allow.",
            lead.asset_class,
            points(lead.drift_percent),
            over_under(lead.drift_percent),
            points(lead.tolerance_percent.unwrap_or(5.0))
        )
    } else {
        let over = sorted
            .iter()
            .find(|class| class.drift_percent > 0.0)
            .map(|class| {
                format!(
                    "{} is {} points over",
                    class.asset_class,
                    points(class.drift_percent)
                )
            });
        let under = sorted
            .iter()
            .find(|class| class.drift_percent < 0.0)
            .map(|class| {
                format!(
                    "{} is {} under",
                    class.asset_class,
                    points(class.drift_percent)
                )
            });
        let pair = match (over, under) {
            (Some(over), Some(under)) => format!(" {over} and {under}."),
            (Some(over), None) => format!(" {over}."),
            (None, Some(under)) => format!(" {under}."),
            (None, None) => String::new(),
        };
        format!(
            "{} of {} asset classes are outside their tolerance.{pair}",
            sorted.len(),
            review.allocations.len()
        )
    };
    let cash = review
        .allocations
        .iter()
        .find(|class| class.asset_class.eq_ignore_ascii_case("Cash"));
    let cash_line = match cash {
        Some(cash) if !cash.within_tolerance => ", and would bring cash back inside its band too",
        _ => "",
    };
    let reasoning = format!(
        "{sentence} A move halfway to your targets takes the edge off without going all the way{cash_line}."
    );
    let mut proposal = Proposal {
        summary: format!(
            "Move every class halfway to its target, {}.",
            proposal_money(&plan, money)
        ),
        trades: trades(&plan),
        rows: row(classes, &plan),
        total: plan.total,
        external: plan.external,
        note: String::new(),
    };
    proposal.note = proposal.money_note(money);
    let (figures, steps) = figures(classes, review);
    Some(Matter {
        id: "rebalance".to_string(),
        kind: Kind::Rebalance,
        weight: if sorted.iter().any(|class| excess(class) > 2.0) {
            Weight::Critical
        } else {
            Weight::Warning
        },
        title: "Rebalance halfway to your targets".to_string(),
        reasoning,
        prior: None,
        proposal,
        records: "Signing records it in Documents, with the plan and these figures. Stahl never \
                  trades."
            .to_string(),
        adjust: Some(AgendaAction::Open(Page::Rebalance)),
        figures,
        steps,
        out_of_band: out_of_band(review),
        policy: review.policy_name.clone(),
    })
}

/// What the plan does in money, as one clause.
fn proposal_money(plan: &Plan, money: &dyn Fn(f64) -> String) -> String {
    let bought: f64 = plan
        .trades
        .iter()
        .filter(|trade| trade.action == rebalance::Action::Increase)
        .map(|trade| trade.amount)
        .sum();
    let sold: f64 = plan
        .trades
        .iter()
        .filter(|trade| trade.action == rebalance::Action::Reduce)
        .map(|trade| trade.amount)
        .sum();
    match (bought >= 0.01, sold >= 0.01) {
        (true, true) => format!("buying {} and selling {}", money(bought), money(sold)),
        (true, false) => format!("buying {}", money(bought)),
        (false, true) => format!("selling {}", money(sold)),
        (false, false) => "with no trades".to_string(),
    }
}

fn place_cash_matter(
    review: &Review,
    classes: &[Class],
    money: &dyn Fn(f64) -> String,
) -> Option<Matter> {
    let cash = review
        .allocations
        .iter()
        .find(|class| class.asset_class.eq_ignore_ascii_case("Cash"))?;
    if cash.within_tolerance || cash.drift_percent <= 0.0 {
        return None;
    }
    let index = classes.iter().position(|class| class.cash)?;
    let held = classes[index].held_value();
    let target_value = cash.target_percent / 100.0 * review.portfolio_value;
    let surplus = held - target_value;
    if surplus < 1.0 {
        return None;
    }
    // Spend the surplus: lower cash by it, then place it as new money, buying only.
    let mut spending = classes.to_vec();
    spending[index].value = (held - surplus).max(0.0);
    let placed = rebalance::place(&spending, surplus).ok()?;
    if placed.values.get(index).copied().unwrap_or(0.0) > spending[index].held_value() + 0.01 {
        // Nothing can take the money without selling or overshooting: there is no honest plan.
        return None;
    }
    // The trades are measured against the real book, so cash shows as sold.
    let plan = rebalance::from_values(classes, placed.values, 0.0);
    if plan.trades.is_empty() {
        return None;
    }
    let reasoning = format!(
        "Cash is {} points over its target: {} against {}. This is the smaller move. It buys \
         only, out of that cash, and leaves the rest of the allocation where it is. The rebalance \
         would bring cash back inside its band as well, by moving every class.",
        points(cash.drift_percent),
        money(held),
        money(target_value)
    );
    let mut proposal = Proposal {
        summary: format!(
            "Put {} to work, buying only: {}.",
            money(surplus),
            proposal_money(&plan, money)
        ),
        trades: trades(&plan),
        rows: row(classes, &plan),
        total: plan.total,
        external: 0.0,
        note: format!(
            "Buying only, out of the cash above the band. Cash goes from {:.1}% to {:.1}%.",
            held / review.portfolio_value * 100.0,
            plan.values.get(index).copied().unwrap_or(0.0) / plan.total * 100.0
        ),
    };
    proposal.note = format!(
        "{} Buying only, out of the cash above the band.",
        proposal.money_note(money)
    );
    let (mut figures, mut steps) = figures(classes, review);
    figures.insert(
        "surplus".to_string(),
        surplus / review.portfolio_value * 100.0,
    );
    steps.insert(
        "surplus".to_string(),
        cash.tolerance_percent
            .unwrap_or(rebalance::DEFAULT_TOLERANCE)
            .max(0.5),
    );
    Some(Matter {
        id: "cash-place".to_string(),
        kind: Kind::PlaceCash,
        weight: Weight::Warning,
        title: "Put the excess cash to work".to_string(),
        reasoning,
        prior: None,
        proposal,
        records: "Signing records where it would go, in Documents. Stahl never trades.".to_string(),
        adjust: Some(AgendaAction::Open(Page::Rebalance)),
        figures,
        steps,
        out_of_band: out_of_band(review),
        policy: review.policy_name.clone(),
    })
}

fn top_up_matter(
    review: &Review,
    classes: &[Class],
    policy: &Policy,
    money: &dyn Fn(f64) -> String,
) -> Option<Matter> {
    let minimum = policy.constraints.minimum_cash_amount?;
    let index = classes.iter().position(|class| class.cash)?;
    let held = classes[index].held_value();
    if held >= minimum - 1.0 {
        return None;
    }
    let start: Vec<f64> = classes.iter().map(Class::held_value).collect();
    let values = rebalance::set_class(classes, &start, index, minimum);
    let plan = rebalance::from_values(classes, values, 0.0);
    if plan.trades.is_empty() {
        return None;
    }
    let reasoning = format!(
        "You hold {} in cash, and your policy asks for at least {}. Selling across the other \
         classes would restore the buffer without changing the total. Nothing urgent, but the \
         floor is there for a reason.",
        money(held),
        money(minimum)
    );
    let mut proposal = Proposal {
        summary: format!(
            "Raise cash to {}, {}.",
            money(minimum),
            proposal_money(&plan, money)
        ),
        trades: trades(&plan),
        rows: row(classes, &plan),
        total: plan.total,
        external: 0.0,
        note: String::new(),
    };
    proposal.note = proposal.money_note(money);
    let (mut figures, mut steps) = figures(classes, review);
    let shortfall = minimum - held;
    figures.insert(
        "shortfall".to_string(),
        shortfall / review.portfolio_value.max(1e-9) * 100.0,
    );
    // A share of the minimum is material: a small rounding is not worth raising again.
    steps.insert(
        "shortfall".to_string(),
        (shortfall / minimum * 100.0).clamp(0.5, 5.0),
    );
    Some(Matter {
        id: "cash-top-up".to_string(),
        kind: Kind::TopUpCash,
        weight: Weight::Critical,
        title: "Top up cash to your policy's minimum".to_string(),
        reasoning,
        prior: None,
        proposal,
        records: "Signing records the sales that would restore it, in Documents. Stahl never \
                  trades."
            .to_string(),
        adjust: Some(AgendaAction::Open(Page::Rebalance)),
        figures,
        steps,
        out_of_band: out_of_band(review),
        policy: review.policy_name.clone(),
    })
}

/// Everything the Overview prepares: what to sign, and what to fix.
pub fn prepare(inputs: &Inputs) -> Prepared {
    let items = agenda::build(&inputs.agenda);
    let Some(review) = inputs.agenda.review else {
        return Prepared {
            matters: Vec::new(),
            todo: items,
        };
    };
    let classes = classes(review, inputs.allocation);
    let mut prepared = Prepared::default();
    for item in items {
        match item.id.as_str() {
            "drift" => {
                if let Some(matter) =
                    rebalance_matter(review, &classes, &|value| inputs.money(value))
                {
                    prepared.matters.push(matter);
                }
            }
            "cash" => {
                match inputs.policy.and_then(|policy| {
                    top_up_matter(review, &classes, policy, &|value| inputs.money(value))
                }) {
                    Some(matter) => prepared.matters.push(matter),
                    // Without the policy's minimum in hand, the finding stays a to-do.
                    None => prepared.todo.push(item),
                }
            }
            _ => prepared.todo.push(item),
        }
    }
    // Cash above its band is not an agenda item, but it is a decision worth preparing.
    if let Some(matter) = place_cash_matter(review, &classes, &|value| inputs.money(value)) {
        prepared.matters.push(matter);
    }
    // A floor breach comes before drift, and drift before the smaller cash move. Within a kind
    // the agenda's own order stands.
    prepared.matters.sort_by_key(|matter| match matter.kind {
        Kind::TopUpCash => 0,
        Kind::Rebalance => 1,
        Kind::PlaceCash => 2,
    });
    prepared
}

// --- What was decided ---

/// Signed, declined or put off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Signed,
    Declined,
    Postponed,
}

impl Status {
    pub fn label(self) -> &'static str {
        match self {
            Status::Signed => "Signed",
            Status::Declined => "Declined",
            Status::Postponed => "Postponed",
        }
    }
}

/// One decision, kept beside the history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub kind: Kind,
    #[serde(default)]
    pub group: Option<String>,
    pub status: Status,
    /// `YYYY-MM-DD`.
    pub at: String,
    #[serde(default)]
    pub reason: Option<String>,
    /// For a postponed matter, the day it comes back on its own.
    #[serde(default)]
    pub until: Option<String>,
    /// What the past decision is called in a sentence.
    #[serde(default)]
    pub summary: String,
    pub policy: String,
    /// Each class's drift, and anything else the matter was about, when it was decided.
    #[serde(default)]
    pub figures: BTreeMap<String, f64>,
    #[serde(default)]
    pub out_of_band: Vec<String>,
}

impl Entry {
    fn at_date(&self) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(&self.at, "%Y-%m-%d").ok()
    }

    fn until_date(&self) -> Option<NaiveDate> {
        self.until
            .as_deref()
            .and_then(|until| NaiveDate::parse_from_str(until, "%Y-%m-%d").ok())
    }
}

/// The decisions kept beside the history, newest last.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ledger {
    /// The file's shape. One, for now; a later reader can tell what it is looking at.
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Ledger {
    fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().rev().find(|entry| entry.id == id)
    }

    /// Keep one entry per matter id: the latest decision replaces the one before it.
    pub fn record(&mut self, entry: Entry) {
        self.version = self.version.max(1);
        self.entries.retain(|each| each.id != entry.id);
        self.entries.push(entry);
    }

    /// Whether a matter is already decided, and should stay off the list.
    pub fn hidden(&self, matter: &Matter, today: NaiveDate) -> bool {
        if let Some(entry) = self.entry(&matter.id) {
            match entry.status {
                Status::Postponed => {
                    let until = entry.until_date();
                    if until.is_some_and(|until| today < until) && !material(entry, matter) {
                        return true;
                    }
                }
                Status::Signed | Status::Declined => {
                    if !material(entry, matter) {
                        return true;
                    }
                }
            }
        }
        // A signed allocation decision settles the alternatives: the owner chose one plan, and
        // the bank does not ask the same question twice. A material change brings them back.
        if let Some(group) = matter.kind.group() {
            let signed = self.entries.iter().rev().find(|entry| {
                entry.status == Status::Signed && entry.group.as_deref() == Some(group)
            });
            if let Some(signed) = signed
                && settled(signed, matter)
            {
                return true;
            }
        }
        false
    }

    /// The sentence about the last decision of this same kind, when one can be computed. A
    /// decision in the same group but of another kind (a signed rebalance, say, seen from the
    /// cash question) is not the same decision, and is left out.
    pub fn prior(&self, matter: &Matter) -> Option<String> {
        prior_line(self.entry(&matter.id)?, matter)
    }
}

/// Whether the numbers have moved enough to raise a decided matter again.
///
/// A matter comes back when its policy changes, when the set of classes outside their tolerance
/// changes, or when any figure it was judged by moves by more than the step recorded for it: a
/// class's own tolerance for a drift, and a share of the minimum for a cash shortfall (between
/// half a point and five points of the portfolio).
pub fn material(entry: &Entry, matter: &Matter) -> bool {
    if entry.policy != matter.policy {
        return true;
    }
    if entry.out_of_band != matter.out_of_band {
        return true;
    }
    if entry.figures.len() != matter.figures.len() {
        return true;
    }
    entry.figures.iter().any(|(key, then)| {
        let Some(now) = matter.figures.get(key) else {
            return true;
        };
        let step = matter.steps.get(key).copied().unwrap_or(5.0);
        (now - then).abs() > step
    })
}

/// Whether a signed decision in the same group still speaks for this matter: the policy and the
/// classes outside their tolerance are the same, and every class drift has stayed within its
/// step. The figures a matter of another kind adds (a surplus, a shortfall) don't count.
fn settled(entry: &Entry, matter: &Matter) -> bool {
    if entry.policy != matter.policy || entry.out_of_band != matter.out_of_band {
        return false;
    }
    let classes = |figures: &BTreeMap<String, f64>| {
        figures
            .keys()
            .filter(|key| key.starts_with("class:"))
            .count()
    };
    if classes(&entry.figures) != classes(&matter.figures) {
        return false;
    }
    entry
        .figures
        .iter()
        .filter(|(key, _)| key.starts_with("class:"))
        .all(|(key, then)| {
            matter.figures.get(key).is_some_and(|now| {
                let step = matter.steps.get(key).copied().unwrap_or(5.0);
                (now - then).abs() <= step
            })
        })
}

/// The newest shape of the file this Stahl knows how to read and write.
const LEDGER_VERSION: u32 = 1;

fn read_ledger(text: &str) -> Result<Ledger, String> {
    let ledger: Ledger = serde_json::from_str(text).map_err(|error| error.to_string())?;
    // A later Stahl wrote this. Reading it as version one could drop what it added, and the
    // next decision would write that loss back.
    if ledger.version > LEDGER_VERSION {
        return Err(format!(
            "it is version {}, written by a newer Stahl than this one",
            ledger.version
        ));
    }
    Ok(ledger)
}

/// What the ledger file held, when it was read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LedgerRead {
    pub ledger: Ledger,
    /// A sentence, when the file exists but can't be read. It is left alone, and no decision is
    /// written over it.
    pub problem: Option<String>,
}

fn unreadable(path: &Path, why: &str) -> String {
    format!(
        "The decision ledger ({}) can't be read: {why}. Stahl has left it as it is and will not \
         write over it. Fix or move the file to keep deciding here.",
        path.display()
    )
}

/// Read a workspace's ledger. A missing file is an empty ledger; one that can't be read is
/// reported, never replaced.
pub fn load_ledger(workspace: &Path) -> LedgerRead {
    let path = matters_dir(workspace).join(LEDGER_FILE);
    let mut path = path;
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        // Before the ledger moved, it sat beside the history, inside the workspace.
        Err(error) if error.kind() == ErrorKind::NotFound => {
            let old = workspace.join("portfolio/history/main").join(LEDGER_FILE);
            match fs::read_to_string(&old) {
                Ok(text) => {
                    path = old;
                    text
                }
                Err(_) => return LedgerRead::default(),
            }
        }
        Err(error) => {
            return LedgerRead {
                ledger: Ledger::default(),
                problem: Some(unreadable(&path, &error.to_string())),
            };
        }
    };
    match read_ledger(&text) {
        Ok(ledger) => LedgerRead {
            ledger,
            problem: None,
        },
        Err(why) => LedgerRead {
            ledger: Ledger::default(),
            problem: Some(unreadable(&path, &why)),
        },
    }
}

fn write_ledger(ledger: &Ledger) -> Result<String, String> {
    serde_json::to_string_pretty(ledger)
        .map(|text| text + "\n")
        .map_err(|error| format!("couldn't encode the decision ledger: {error}"))
}

/// The day a postponed matter comes back.
fn tomorrow(today: NaiveDate) -> NaiveDate {
    today + Duration::days(1)
}

// --- Where matters and records live ---

/// The directory a workspace keeps its decision ledger in: its own, whatever the environment
/// says about history, so two workspaces never share a decision.
pub fn matters_dir(workspace: &Path) -> PathBuf {
    workspace.join("portfolio")
}

/// Where a workspace keeps its decision records.
pub fn decisions_dir(workspace: &Path) -> PathBuf {
    workspace.join("portfolio/decisions")
}

/// A decision record, as the Documents page lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct Decision {
    pub path: PathBuf,
    pub title: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    pub status: String,
    pub body: String,
}

impl Decision {
    /// The file's name, for a key.
    pub fn key(&self) -> String {
        self.path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
    }
}

/// Read one decision record. `None` for a file that isn't one.
pub fn read_decision(path: &Path) -> Option<Decision> {
    let body = fs::read_to_string(path).ok()?;
    let title = body
        .lines()
        .find_map(|line| line.strip_prefix("# Decision:"))
        .map(|title| title.trim().to_string())
        .or_else(|| {
            body.lines()
                .find_map(|line| line.strip_prefix("# "))
                .map(|title| title.trim().to_string())
        })?;
    let field = |name: &str| {
        body.lines()
            .find_map(|line| line.strip_prefix(name))
            .map(|value| value.trim().to_string())
    };
    let date = field("Date:").unwrap_or_default();
    let status = field("Status:").unwrap_or_default();
    Some(Decision {
        path: path.to_path_buf(),
        title,
        date,
        status,
        body,
    })
}

/// Every decision record in a workspace, newest first.
pub fn decisions(workspace: &Path) -> Vec<Decision> {
    let Ok(entries) = fs::read_dir(decisions_dir(workspace)) else {
        return Vec::new();
    };
    let mut out: Vec<Decision> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .filter_map(|path| read_decision(&path))
        .collect();
    out.sort_by(|a, b| b.date.cmp(&a.date).then_with(|| a.title.cmp(&b.title)));
    out
}

/// The slug `portfolio_rs` gives a decision record's file name.
fn slug(title: &str) -> String {
    title
        .to_lowercase()
        .replace(' ', "-")
        .replace(|c: char| !c.is_alphanumeric() && c != '-', "")
}

/// A free path for a record, never overwriting one.
fn record_path(dir: &Path, title: &str, date: NaiveDate) -> PathBuf {
    let base = format!("{}-{}", date.format("%Y-%m-%d"), slug(title));
    let mut path = dir.join(format!("{base}.md"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{base}-{n}.md"));
        n += 1;
    }
    path
}

/// The record a signed or declined matter leaves, in the same shape as the app's own drafts.
pub fn record_markdown(
    matter: &Matter,
    status: Status,
    date: NaiveDate,
    currency: &str,
    reason: Option<&str>,
) -> String {
    let money = |value: f64| format::currency(value, currency);
    let mut out = String::new();
    out.push_str(&format!("# Decision: {}\n\n", matter.title));
    out.push_str(&format!("Date: {}\n", date.format("%Y-%m-%d")));
    out.push_str(&format!("Status: {}\n\n", status.label()));
    out.push_str("## Context\n\n");
    out.push_str(&format!(
        "Portfolio value: {} {currency}\n\n",
        money(matter.proposal.total)
    ));
    out.push_str(&format!("Against \"{}\".\n\n", matter.policy));
    if !matter.out_of_band.is_empty() {
        out.push_str(&format!(
            "Outside their tolerance: {}.\n\n",
            matter.out_of_band.join(", ")
        ));
    }
    out.push_str("## Decision\n\n");
    match status {
        Status::Signed => {
            out.push_str(&format!("Signed. {}\n\n", matter.proposal.summary));
        }
        Status::Declined => {
            out.push_str("Declined. ");
            if let Some(reason) = reason.filter(|reason| !reason.trim().is_empty()) {
                out.push_str(reason.trim());
                out.push_str("\n\n");
            } else {
                out.push_str("No reason given.\n\n");
            }
            out.push_str(&format!(
                "The proposal was: {}\n\n",
                matter.proposal.summary
            ));
        }
        Status::Postponed => out.push_str("Postponed.\n\n"),
    }
    if !matter.proposal.trades.is_empty() {
        out.push_str("| Asset class | Action | Amount | After |\n");
        out.push_str("| --- | --- | --- | --- |\n");
        for trade in &matter.proposal.trades {
            let after = matter
                .proposal
                .rows
                .iter()
                .find(|row| row.class == trade.class)
                .map_or(0.0, |row| row.after);
            out.push_str(&format!(
                "| {} | {} | {} {currency} | {:.1}% |\n",
                trade.class,
                if trade.buy { "Buy" } else { "Sell" },
                money(trade.amount),
                after
            ));
        }
        out.push('\n');
    }
    if !matter.proposal.rows.is_empty() {
        out.push_str("Allocation before and after: ");
        out.push_str(
            &matter
                .proposal
                .rows
                .iter()
                .map(|row| format!("{} {:.1}% → {:.1}%", row.class, row.before, row.after))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str(".\n\n");
    }
    out.push_str("Stahl never trades. This record is a decision on a plan, not an order.\n\n");
    out.push_str("## Rationale\n\n");
    out.push_str(&matter.reasoning);
    out.push_str("\n\n");
    out.push_str("## Risks\n\n");
    out.push_str(
        "Prices move, so the figures here are the ones on the day of signing. A later review may \
         propose another plan.\n\n",
    );
    out.push_str("## Review Date\n\n");
    out.push_str("At the next review, or when the numbers move materially.\n");
    out
}

// --- The backend's side ---

/// Held by `decide` from the read of the ledger to its write.
static DECIDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

impl Backend {
    /// The workspace's directory, when one is open.
    async fn matters_workspace(&self) -> Option<PathBuf> {
        let open = self.open_document().await;
        // An encrypted portfolio's values must not land on disk in plaintext (README, History),
        // and it has no workspace to keep a record in.
        if open
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with(".gpg"))
        {
            return None;
        }
        open.workspace_dir.map(PathBuf::from)
    }

    /// The matters the owner has already decided, and the policy the plans are measured against.
    pub async fn matters(&self) -> (LedgerRead, Option<Policy>) {
        let policy = self.policy().await;
        let Some(workspace) = self.matters_workspace().await else {
            return (LedgerRead::default(), policy);
        };
        (load_ledger(&workspace), policy)
    }

    /// Decide a matter: read the ledger, change it, write the record and then the ledger. A
    /// failure leaves the files as they were, so "nothing was written" is true when it is said.
    /// Returns the ledger as it now stands on disk.
    pub async fn decide(
        &self,
        workspace: &Path,
        entry: Entry,
        record: Option<RecordRequest>,
    ) -> BackendResult<Ledger> {
        // One decision at a time, from reading the ledger to writing it: two made close together
        // would otherwise both read the same file, and the later write would drop the earlier.
        let _one_at_a_time = DECIDING.lock().await;
        let read = load_ledger(workspace);
        if let Some(problem) = read.problem {
            return Err(format!("Nothing was written. {problem}"));
        }
        let mut ledger = read.ledger;
        ledger.record(entry);
        let text =
            write_ledger(&ledger).map_err(|error| format!("Nothing was written: {error}"))?;
        let written = match &record {
            Some(request) => Some(
                self.record_matter(
                    workspace,
                    &request.matter,
                    request.status,
                    request.reason.clone(),
                    request.date,
                    &request.currency,
                )
                .await?,
            ),
            None => None,
        };
        let dir = matters_dir(workspace);
        let saved = fs::create_dir_all(&dir)
            .map_err(|error| format!("couldn't create {}: {error}", dir.display()))
            .and_then(|()| {
                atomic_write::write_atomic_private(&dir.join(LEDGER_FILE), text.as_bytes())
                    .map_err(|error| format!("couldn't save the decision ledger: {error}"))
            });
        if let Err(error) = saved {
            // The record without its ledger entry would be listed as decided when it isn't.
            if let Some(path) = written {
                let _ = fs::remove_file(path);
            }
            return Err(format!("Nothing was written: {error}"));
        }
        Ok(ledger)
    }

    /// Write a signed or declined matter as a decision record, and return its path.
    async fn record_matter(
        &self,
        workspace: &Path,
        matter: &Matter,
        status: Status,
        reason: Option<String>,
        date: NaiveDate,
        currency: &str,
    ) -> BackendResult<String> {
        let dir = decisions_dir(workspace);
        fs::create_dir_all(&dir)
            .map_err(|error| format!("couldn't create {}: {error}", dir.display()))?;
        let path = record_path(&dir, &matter.title, date);
        let markdown = record_markdown(matter, status, date, currency, reason.as_deref());
        atomic_write::write_new_private(&path, markdown.as_bytes())
            .map_err(|error| format!("couldn't write the record: {error}"))?;
        Ok(path.to_string_lossy().into_owned())
    }

    /// Every decision record in the workspace, newest first.
    pub async fn decisions(&self) -> Vec<Decision> {
        match self.matters_workspace().await {
            Some(workspace) => decisions(&workspace),
            None => Vec::new(),
        }
    }
}

/// The record a decision leaves beside its ledger entry.
pub struct RecordRequest {
    pub matter: Matter,
    pub status: Status,
    pub reason: Option<String>,
    pub date: NaiveDate,
    pub currency: String,
}

// --- The page's state ---

/// A press on a Sign control, before it is either completed or let go.
#[derive(Clone, Debug, PartialEq)]
pub struct Holding {
    pub id: String,
    /// On the animation clock.
    pub started: f64,
    /// Whether this hold came from the keyboard, for the hint.
    pub key: bool,
}

/// A matter that was just signed, shown stamped for a breath before it files itself away.
#[derive(Clone, Debug, PartialEq)]
pub struct Stamp {
    pub matter: Matter,
    /// Where the card stood in the list, so it leaves from the same place.
    pub index: usize,
    pub started: f64,
    pub date: NaiveDate,
}

/// Why a matter is being declined, while the owner writes it.
#[derive(Clone, Debug)]
pub struct Declining {
    pub id: String,
    pub reason: rune::TextEdit,
}

/// Everything the matters section and the Documents page keep.
#[derive(Default)]
pub struct MattersPage {
    pub ledger: Ledger,
    /// The policy the plans are measured against, loaded with the ledger.
    pub policy: Option<Policy>,
    pub decisions: Vec<Decision>,
    pub decisions_loaded: bool,
    pub hold: Option<Holding>,
    pub stamp: Option<Stamp>,
    /// A press that let go too soon, and when: the card says "Hold to sign".
    pub hint: Option<(String, f64)>,
    pub expanded: Option<String>,
    /// The matter of a group the owner chose to look at instead of the first.
    pub shown: Option<String>,
    pub declining: Option<Declining>,
    /// The matter the reduced-motion confirmation is about.
    pub confirm_sign: Option<String>,
    /// The record opened on the Documents page.
    pub open_decision: Option<String>,
    pub error: Option<String>,
    /// Why the ledger on disk can't be read, while it can't. Nothing is decided meanwhile.
    pub ledger_problem: Option<String>,
}

impl MattersPage {
    /// A portfolio was opened or closed: nothing from the last one carries over.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// A decision wasn't written: put back the entry the matter had before, or none.
    fn restore(&mut self, id: &str, before: &Ledger) {
        self.ledger.entries.retain(|entry| entry.id != id);
        if let Some(previous) = before.entries.iter().find(|entry| entry.id == id) {
            self.ledger.entries.push(previous.clone());
        }
    }

    pub fn progress(&self, now: f64) -> Option<f32> {
        let hold = self.hold.as_ref()?;
        Some((((now - hold.started) / HOLD).clamp(0.0, 1.0)) as f32)
    }

    /// Whether the matter is being held right now.
    pub fn holding(&self, id: &str) -> bool {
        self.hold.as_ref().is_some_and(|hold| hold.id == id)
    }
}

impl State {
    fn matter_inputs(&self) -> Inputs<'_> {
        Inputs {
            agenda: agenda::Inputs {
                review: self.review.data.as_ref(),
                has_policy: self.has_policy,
                is_workspace: self.is_workspace(),
                findings: &self.findings,
                positions: &self.positions,
                cash: self
                    .summary
                    .as_ref()
                    .map_or_else(String::new, |summary| self.money(summary.cash_value)),
            },
            allocation: &self.allocation,
            policy: self.matters.policy.as_ref(),
            privacy: self.privacy,
            currency: self.currency(),
        }
    }

    /// Why nothing can be signed right now, in a sentence, or `None`. A record says what was true
    /// on the day it was signed, so it is never built from a past day's figures or from a review
    /// that doesn't describe the allocation on screen.
    pub fn matters_withheld(&self) -> Option<&'static str> {
        if self.in_past() {
            Some(
                "You are looking at a day in the past. Decisions are signed on today's figures, \
                 so return to today to see what needs your signature.",
            )
        } else if self.review.loading && self.review.data.is_some() {
            Some(
                "The review is being redone on the latest figures. Decisions will return in a moment.",
            )
        } else {
            None
        }
    }

    /// What the bank has prepared: matters to sign, and data fixes, with past decisions noted.
    pub fn prepared(&self) -> Prepared {
        if self.positions.is_empty() {
            return Prepared::default();
        }
        let mut prepared = prepare(&self.matter_inputs());
        if self.matters_withheld().is_some() {
            prepared.matters.clear();
            return prepared;
        }
        let today = Local::now().date_naive();
        for matter in &mut prepared.matters {
            matter.prior = self.matters.ledger.prior(matter);
        }
        prepared
            .matters
            .retain(|matter| !self.matters.ledger.hidden(matter, today));
        prepared
    }

    /// The matter as the record keeps it: the same one, with its money written out. `None` when
    /// it can't be built that way, because a record is never written masked.
    fn unmasked(&self, matter: &Matter) -> Option<Matter> {
        if !self.privacy {
            return Some(matter.clone());
        }
        let mut inputs = self.matter_inputs();
        inputs.privacy = false;
        prepare(&inputs)
            .matters
            .into_iter()
            .find(|open| open.id == matter.id)
    }

    /// Whether a decision may be written now; if not, say why, on the page.
    fn can_decide(&mut self) -> bool {
        match self.matters.ledger_problem.clone() {
            Some(problem) => {
                self.matters.error = Some(format!("Nothing was written. {problem}"));
                self.matters.hold = None;
                self.matters.declining = None;
                false
            }
            None => true,
        }
    }

    /// The matter as it goes in a record, or the page says it can't be written.
    fn matter_for_record(&mut self, matter: &Matter) -> Option<Matter> {
        let unmasked = self.unmasked(matter);
        if unmasked.is_none() {
            self.matters.error = Some(
                "These figures can't be written in full just now, and a record is never written \
                 with its amounts hidden. Nothing was recorded; try again in a moment."
                    .to_string(),
            );
            self.matters.hold = None;
            self.matters.declining = None;
        }
        unmasked
    }

    /// Where a decision may be written: a plaintext workspace, or nothing. An encrypted
    /// portfolio's figures must not land on disk in plaintext (README, History).
    fn matters_workspace(&self) -> Result<PathBuf, String> {
        if self
            .open
            .file_path
            .as_deref()
            .is_some_and(|path| path.ends_with(".gpg"))
        {
            return Err(
                "This portfolio is encrypted, so a record would put its figures on disk in                  plaintext."
                    .to_string(),
            );
        }
        self.open
            .workspace_dir
            .clone()
            .map(PathBuf::from)
            .ok_or_else(|| "Records need a workspace. This portfolio is a single file.".to_string())
    }

    /// Whether `workspace` is still the one open: a decision written for another one has nothing
    /// to say about this one.
    fn still_open(&self, workspace: &Path) -> bool {
        self.matters_workspace().is_ok_and(|open| open == workspace)
    }

    /// Load the decisions and the policy the plans are measured against.
    pub fn load_matters(&mut self) {
        if !self.is_workspace() {
            self.matters.ledger = Ledger::default();
            self.matters.ledger_problem = None;
            self.matters.policy = None;
            self.matters.decisions.clear();
            self.matters.decisions_loaded = false;
            return;
        }
        self.run(
            |backend| async move { Ok(backend.matters().await) },
            |state, result: BackendResult<(LedgerRead, Option<Policy>)>| {
                if let Ok((read, policy)) = result {
                    state.matters.ledger = read.ledger;
                    state.matters.ledger_problem = read.problem;
                    state.matters.policy = policy;
                }
            },
        );
    }

    /// Load the decision records the Documents page lists.
    pub fn load_decisions(&mut self) {
        self.run(
            |backend| async move { Ok(backend.decisions().await) },
            |state, result: BackendResult<Vec<Decision>>| match result {
                Ok(decisions) => {
                    state.matters.decisions = decisions;
                    state.matters.decisions_loaded = true;
                    state.matters.error = None;
                }
                Err(error) => state.matters.error = Some(error),
            },
        );
    }

    /// Start holding the Sign control. `key` is the keyboard's path to the same ceremony.
    pub fn begin_hold(&mut self, id: &str, key: bool) {
        if !self.matters.hold.as_ref().is_some_and(|hold| hold.id == id) {
            self.matters.hold = Some(Holding {
                id: id.to_string(),
                started: self.now,
                key,
            });
            self.matters.hint = None;
        }
    }

    /// Let go. Before the hold is complete, nothing is signed and the card springs back. A hold
    /// that has lasted its time signs here, on the release, and nowhere else: the clock alone
    /// never completes one, so a release that never arrives signs nothing.
    pub fn release_hold(&mut self) {
        let Some(hold) = self.matters.hold.take() else {
            return;
        };
        if self.now - hold.started < HOLD {
            self.matters.hint = Some((hold.id, self.now));
        } else {
            self.sign_matter(&hold.id);
        }
    }

    /// The touch turned into something else (a scroll, or it left the control): the hold stops
    /// at once, nothing is signed, and there is no hint, because nothing was let go too soon.
    pub fn cancel_hold(&mut self) {
        self.matters.hold = None;
    }

    /// Whether something floats over the page and has the owner's attention.
    fn covered(&self) -> bool {
        self.palette.is_some()
            || self.form.is_some()
            || self.confirm.is_some()
            || self.new_workspace.is_some()
            || self.intake.is_some()
            || self.consent.is_some()
            || self.guide
            || self.record_plate.is_some()
    }

    /// The hold was completed: record the decision and stamp the card.
    pub fn sign_matter(&mut self, id: &str) {
        let prepared = self.prepared();
        let Some(card) = prepared
            .matters
            .iter()
            .find(|matter| matter.id == id)
            .cloned()
        else {
            self.matters.hold = None;
            return;
        };
        if !self.can_decide() {
            return;
        }
        // The card may hide amounts; the record sits beside a table of real ones, so it is
        // written from the same matter prepared without the masks.
        let Some(matter) = self.matter_for_record(&card) else {
            return;
        };
        let workspace = match self.matters_workspace() {
            Ok(workspace) => workspace,
            Err(error) => {
                self.matters.error = Some(error);
                self.matters.hold = None;
                return;
            }
        };
        let today = Local::now().date_naive();
        let index = prepared
            .matters
            .iter()
            .position(|matter| matter.id == id)
            .unwrap_or(0);
        let currency = self.currency();
        self.matters.hold = None;
        self.matters.stamp = Some(Stamp {
            matter: card,
            index,
            started: self.now,
            date: today,
        });
        let entry = entry(&matter, Status::Signed, today, None);
        let before = self.matters.ledger.clone();
        self.matters.ledger.record(entry.clone());
        let request = RecordRequest {
            matter,
            status: Status::Signed,
            reason: None,
            date: today,
            currency,
        };
        let id = id.to_string();
        let opened = workspace.clone();
        self.run(
            move |backend| async move { backend.decide(&workspace, entry, Some(request)).await },
            move |state, result: BackendResult<Ledger>| match result {
                // Another workspace was opened meanwhile; its own load reads its own file.
                _ if !state.still_open(&opened) => {}
                Ok(ledger) => {
                    state.matters.error = None;
                    state.matters.ledger = ledger;
                    state.matters.decisions_loaded = false;
                    state.show_toast("Recorded. The plan is in your documents.");
                }
                Err(error) => {
                    // Nothing was written, so nothing was decided: put the matter back.
                    state.matters.error = Some(error);
                    state.matters.stamp = None;
                    state.matters.restore(&id, &before);
                }
            },
        );
    }

    /// Decline a matter, with an optional one-line reason, and write the record.
    pub fn decline_matter(&mut self, id: &str, reason: String) {
        let prepared = self.prepared();
        let Some(card) = prepared
            .matters
            .iter()
            .find(|matter| matter.id == id)
            .cloned()
        else {
            return;
        };
        if !self.can_decide() {
            return;
        }
        let Some(matter) = self.matter_for_record(&card) else {
            return;
        };
        let today = Local::now().date_naive();
        let reason = reason.trim().to_string();
        let reason = (!reason.is_empty()).then_some(reason);
        self.matters.declining = None;
        self.matters.hint = None;
        let entry = entry(&matter, Status::Declined, today, reason.clone());
        let before = self.matters.ledger.clone();
        self.matters.ledger.record(entry.clone());
        let Ok(workspace) = self.matters_workspace() else {
            self.show_toast(
                "Declined for this session only. An encrypted portfolio keeps no record, so it \
                 will return next time.",
            );
            return;
        };
        self.show_toast("Declined. We won't bring it again unless the numbers move.");
        let request = RecordRequest {
            matter,
            status: Status::Declined,
            reason,
            date: today,
            currency: self.currency(),
        };
        let id = id.to_string();
        let opened = workspace.clone();
        self.run(
            move |backend| async move { backend.decide(&workspace, entry, Some(request)).await },
            move |state, result: BackendResult<Ledger>| match result {
                // Another workspace was opened meanwhile; its own load reads its own file.
                _ if !state.still_open(&opened) => {}
                Ok(ledger) => {
                    state.matters.error = None;
                    state.matters.ledger = ledger;
                    state.matters.decisions_loaded = false;
                }
                Err(error) => {
                    state.matters.error = Some(error);
                    state.matters.restore(&id, &before);
                }
            },
        );
    }

    /// Hide a matter until tomorrow, or until the numbers move materially.
    pub fn postpone_matter(&mut self, id: &str) {
        let prepared = self.prepared();
        let Some(matter) = prepared
            .matters
            .iter()
            .find(|matter| matter.id == id)
            .cloned()
        else {
            return;
        };
        if !self.can_decide() {
            return;
        }
        let today = Local::now().date_naive();
        let mut entry = entry(&matter, Status::Postponed, today, None);
        entry.until = Some(tomorrow(today).format("%Y-%m-%d").to_string());
        let before = self.matters.ledger.clone();
        self.matters.ledger.record(entry.clone());
        let Ok(workspace) = self.matters_workspace() else {
            self.show_toast(
                "Set aside for this session only. An encrypted portfolio keeps no record, so it \
                 will return next time.",
            );
            return;
        };
        self.show_toast("Set aside until tomorrow");
        let id = id.to_string();
        let opened = workspace.clone();
        self.run(
            move |backend| async move { backend.decide(&workspace, entry, None).await },
            move |state, result: BackendResult<Ledger>| match result {
                _ if !state.still_open(&opened) => {}
                Ok(ledger) => {
                    state.matters.error = None;
                    state.matters.ledger = ledger;
                }
                Err(error) => {
                    state.matters.error = Some(error);
                    state.matters.restore(&id, &before);
                }
            },
        );
    }

    /// The card that is filing itself away has gone: it leaves the list.
    pub fn end_stamp(&mut self) {
        self.matters.stamp = None;
    }

    /// The reduced-motion confirmation, answered.
    pub fn confirm_sign(&mut self) {
        if let Some(id) = self.matters.confirm_sign.take() {
            self.sign_matter(&id);
        }
    }

    /// Runs every frame: completes a hold at its deadline, and clears what has been shown long
    /// enough. True while something on screen is still moving.
    pub fn matters_tick(&mut self) -> bool {
        let mut changed = false;
        if self.matters.hold.is_some() {
            if self.covered() {
                // A sheet, the palette or a dialog took the owner's attention: no hold goes on.
                self.matters.hold = None;
            }
            // A frame for the ring to fill, or to show that it let go. The clock never signs.
            changed = true;
        }
        if let Some(stamp) = &self.matters.stamp {
            if self.now - stamp.started >= BREATH {
                self.matters.stamp = None;
                changed = true;
            } else {
                changed = true;
            }
        }
        if let Some((_, at)) = &self.matters.hint
            && self.now - at >= HINT_FOR
        {
            self.matters.hint = None;
            changed = true;
        }
        changed
    }
}

/// The ledger entry a decision leaves.
fn entry(matter: &Matter, status: Status, today: NaiveDate, reason: Option<String>) -> Entry {
    Entry {
        id: matter.id.clone(),
        kind: matter.kind,
        group: matter.kind.group().map(str::to_string),
        status,
        at: today.format("%Y-%m-%d").to_string(),
        reason,
        until: None,
        summary: matter.kind.summary().to_string(),
        policy: matter.policy.clone(),
        // A figure that isn't a number would be written as `null`, which the file can't read back.
        figures: matter
            .figures
            .iter()
            .filter(|(_, value)| value.is_finite())
            .map(|(key, value)| (key.clone(), *value))
            .collect(),
        out_of_band: matter.out_of_band.clone(),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt as _;
    use std::{collections::BTreeMap, env, process, time::SystemTime};

    use portfolio_rs::{
        policy::{AllocationTarget, Constraints, RiskProfile},
        review::{AllocationReview, ConstraintCheck, Review},
        state::AllocationItemDto,
    };

    use super::*;

    fn class(name: &str, target: f64, tolerance: f64, actual: f64) -> AllocationReview {
        let drift = actual - target;
        AllocationReview {
            asset_class: name.to_string(),
            target_percent: target,
            tolerance_percent: Some(tolerance),
            actual_percent: actual,
            drift_percent: drift,
            within_tolerance: drift.abs() <= tolerance,
            value: 0.0,
        }
    }

    fn review(allocations: Vec<AllocationReview>, checks: Vec<ConstraintCheck>) -> Review {
        Review {
            portfolio_value: 100_000.0,
            currency: "EUR".to_string(),
            policy_name: "Balanced Growth".to_string(),
            findings: Vec::new(),
            allocations,
            constraint_checks: checks,
            data_quality: Vec::new(),
            suggested_actions: Vec::new(),
        }
    }

    /// The demo's book, at 100,000 so the figures read easily.
    fn demo_review() -> Review {
        let mut review = review(
            vec![
                class("Stocks", 50.0, 5.0, 37.0),
                class("Bonds", 15.0, 5.0, 5.0),
                class("Gold", 5.0, 2.0, 4.7),
                class("Crypto", 10.0, 3.0, 25.0),
                class("Cash", 20.0, 5.0, 28.0),
            ],
            vec![ConstraintCheck {
                name: "Minimum Cash Amount".to_string(),
                passed: true,
                message: "Cash buffer: 28000.00 EUR (minimum: 20000.00 EUR)".to_string(),
            }],
        );
        for allocation in &mut review.allocations {
            allocation.value = allocation.actual_percent / 100.0 * review.portfolio_value;
        }
        review
    }

    fn policy(minimum: Option<f64>) -> Policy {
        Policy {
            version: "1.0".to_string(),
            name: "Balanced Growth".to_string(),
            base_currency: "EUR".to_string(),
            time_horizon_years: 15,
            risk_profile: RiskProfile::Moderate,
            constraints: Constraints {
                minimum_cash_months: None,
                minimum_cash_amount: minimum,
                single_position_limit_percent: None,
                asset_class_limit_percent: None,
            },
            allocations: Vec::<AllocationTarget>::new(),
        }
    }

    fn inputs<'a>(
        review: Option<&'a Review>,
        policy: Option<&'a Policy>,
        allocation: &'a [AllocationItemDto],
        has_policy: bool,
    ) -> Inputs<'a> {
        Inputs {
            agenda: agenda::Inputs {
                review,
                has_policy,
                is_workspace: true,
                findings: &[],
                positions: &[],
                cash: "€28,000.00".to_string(),
            },
            allocation,
            policy,
            privacy: false,
            currency: "EUR".to_string(),
        }
    }

    #[test]
    fn without_a_policy_there_is_nothing_to_sign() {
        let allocation = [];
        let prepared = prepare(&inputs(None, None, &allocation, false));
        assert!(prepared.matters.is_empty());
        assert_eq!(prepared.todo.len(), 1, "the policy is still a to-do");
        assert_eq!(prepared.todo[0].action, AgendaAction::Open(Page::Policy));
    }

    #[test]
    fn a_portfolio_inside_its_bands_has_no_matters() {
        let review = review(
            vec![
                class("Stocks", 50.0, 5.0, 52.0),
                class("Cash", 50.0, 5.0, 48.0),
            ],
            Vec::new(),
        );
        let policy = policy(Some(10_000.0));
        let allocation = [];
        let prepared = prepare(&inputs(Some(&review), Some(&policy), &allocation, true));
        assert!(prepared.matters.is_empty());
        assert!(prepared.todo.is_empty());
    }

    #[test]
    fn one_group_gets_one_card_and_the_owner_picks_it() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let allocation = [];
        let prepared = prepare(&inputs(Some(&review), Some(&policy), &allocation, true));
        let ids = |matters: Vec<&Matter>| -> Vec<String> {
            matters
                .into_iter()
                .map(|matter| matter.id.clone())
                .collect()
        };
        let (cards, others) = prepared.cards(None);
        assert_eq!(ids(cards), ["rebalance"]);
        assert_eq!(ids(others), ["cash-place"]);
        let (cards, others) = prepared.cards(Some("cash-place"));
        assert_eq!(ids(cards), ["cash-place"]);
        assert_eq!(ids(others), ["rebalance"]);
        assert_eq!(prepared.cards(Some("gone")), prepared.cards(None));
    }

    #[test]
    fn the_demo_prepares_a_rebalance_and_the_cash_alternative() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let allocation = [];
        let prepared = prepare(&inputs(Some(&review), Some(&policy), &allocation, true));
        let kinds: Vec<Kind> = prepared.matters.iter().map(|matter| matter.kind).collect();
        assert_eq!(kinds, [Kind::Rebalance, Kind::PlaceCash]);
        let rebalance = &prepared.matters[0];
        assert_eq!(rebalance.title, "Rebalance halfway to your targets");
        assert!(rebalance.reasoning.contains("4 of 5 asset classes"));
        assert!(rebalance.reasoning.contains("Crypto is 15.0 points over"));
        // Halfway: Stocks 37 -> 43.5, Crypto 25 -> 17.5.
        let stocks = rebalance
            .proposal
            .rows
            .iter()
            .find(|row| row.class == "Stocks")
            .unwrap();
        assert!((stocks.before - 37.1).abs() < 0.2, "{}", stocks.before);
        assert!((stocks.after - 43.6).abs() < 0.2, "{}", stocks.after);
        assert!(
            rebalance
                .proposal
                .trades
                .iter()
                .any(|trade| trade.class == "Stocks" && trade.buy)
        );
        assert!(
            rebalance
                .proposal
                .trades
                .iter()
                .any(|trade| trade.class == "Crypto" && !trade.buy)
        );
        assert!(rebalance.records.contains("Stahl never trades"));

        // The cash alternative spends the 8,000 above the 20% target, buying only.
        let cash = &prepared.matters[1];
        assert_eq!(cash.kind, Kind::PlaceCash);
        assert!(cash.reasoning.contains("8.0 points over"));
        let cash_trade = cash
            .proposal
            .trades
            .iter()
            .find(|trade| trade.class == "Cash")
            .unwrap();
        assert!(!cash_trade.buy);
        assert!((cash_trade.amount - 8_000.0).abs() < 1.0);
        assert!(
            cash.proposal
                .trades
                .iter()
                .filter(|trade| trade.class != "Cash")
                .all(|trade| trade.buy)
        );
    }

    #[test]
    fn several_matters_arrive_together_most_important_first() {
        let mut review = demo_review();
        review.constraint_checks[0].passed = false;
        let policy = policy(Some(20_000.0));
        let allocation = [];
        let prepared = prepare(&inputs(Some(&review), Some(&policy), &allocation, true));
        assert!(prepared.matters.len() >= 2);
        // Cash is 28,000, above the 20,000 floor, so no top-up: the checks disagree with the
        // numbers, and the numbers win.
        assert!(
            prepared
                .matters
                .iter()
                .all(|matter| matter.kind != Kind::TopUpCash)
        );
    }

    #[test]
    fn cash_below_the_minimum_is_topped_up_by_selling() {
        let mut review = review(
            vec![
                class("Stocks", 50.0, 5.0, 52.0),
                class("Bonds", 30.0, 5.0, 33.0),
                class("Cash", 20.0, 5.0, 15.0),
            ],
            vec![ConstraintCheck {
                name: "Minimum Cash Amount".to_string(),
                passed: false,
                message: "Cash buffer: 15000.00 EUR (minimum: 20000.00 EUR)".to_string(),
            }],
        );
        for allocation in &mut review.allocations {
            allocation.value = allocation.actual_percent / 100.0 * review.portfolio_value;
        }
        let policy = policy(Some(20_000.0));
        let allocation = [];
        let prepared = prepare(&inputs(Some(&review), Some(&policy), &allocation, true));
        let top_up = prepared
            .matters
            .iter()
            .find(|matter| matter.kind == Kind::TopUpCash)
            .expect("cash is under the floor");
        assert_eq!(top_up.title, "Top up cash to your policy's minimum");
        let cash = top_up
            .proposal
            .trades
            .iter()
            .find(|trade| trade.class == "Cash")
            .unwrap();
        assert!(cash.buy);
        assert!((cash.amount - 5_000.0).abs() < 1.0);
        assert!(
            top_up
                .proposal
                .trades
                .iter()
                .filter(|trade| trade.class != "Cash")
                .all(|trade| !trade.buy)
        );
        // Cash ends on the floor.
        let row = top_up
            .proposal
            .rows
            .iter()
            .find(|row| row.class == "Cash")
            .unwrap();
        assert!((row.after - 20.0).abs() < 0.01);
    }

    #[test]
    fn privacy_hides_every_amount_in_the_words() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let allocation = [];
        let inputs = Inputs {
            privacy: true,
            ..inputs(Some(&review), Some(&policy), &allocation, true)
        };
        let prepared = prepare(&inputs);
        for matter in &prepared.matters {
            assert!(
                !matter.reasoning.contains('€') && !matter.proposal.summary.contains('€'),
                "{} shows an amount",
                matter.title
            );
            assert!(matter.proposal.summary.contains(format::HIDDEN));
        }
    }

    fn signed_entry(matter: &Matter, status: Status, days_ago: i64) -> Entry {
        let today = Local::now().date_naive();
        let at = today - Duration::days(days_ago);
        entry(matter, status, at, None)
    }

    #[test]
    fn a_declined_matter_stays_hidden_until_the_drift_moves_a_tolerance() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let allocation = [];
        let inputs = inputs(Some(&review), Some(&policy), &allocation, true);
        let prepared = prepare(&inputs);
        let rebalance = &prepared.matters[0];
        let mut ledger = Ledger::default();
        ledger.record(signed_entry(rebalance, Status::Declined, 3));
        let today = Local::now().date_naive();
        assert!(ledger.hidden(rebalance, today), "declined, nothing moved");

        // Crypto drifts 3 more points: within its 3.0 tolerance, so still hidden.
        let mut small = demo_review();
        small.allocations[3] = class("Crypto", 10.0, 3.0, 27.9);
        small.allocations[4] = class("Cash", 20.0, 5.0, 25.1);
        let small = prepare(&inputs_with(Some(&small), Some(&policy)));
        assert!(
            ledger.hidden(&small.matters[0], today),
            "three points is inside the class's tolerance"
        );

        // Four more: the class's tolerance is passed, so the matter comes back.
        let mut large = demo_review();
        large.allocations[3] = class("Crypto", 10.0, 3.0, 29.1);
        large.allocations[4] = class("Cash", 20.0, 5.0, 23.9);
        let large = prepare(&inputs_with(Some(&large), Some(&policy)));
        assert!(
            !ledger.hidden(&large.matters[0], today),
            "a material change brings it back"
        );
    }

    #[test]
    fn a_postponed_matter_returns_tomorrow_or_on_a_material_change() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let prepared = prepare(&inputs_with(Some(&review), Some(&policy)));
        let matter = &prepared.matters[0];
        let today = Local::now().date_naive();
        let mut ledger = Ledger::default();
        let mut postponed = entry(matter, Status::Postponed, today, None);
        postponed.until = Some(tomorrow(today).format("%Y-%m-%d").to_string());
        ledger.record(postponed);
        assert!(ledger.hidden(matter, today));
        assert!(!ledger.hidden(matter, tomorrow(today)));
    }

    #[test]
    fn a_signed_allocation_settles_its_alternatives_until_the_numbers_move() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let prepared = prepare(&inputs_with(Some(&review), Some(&policy)));
        let rebalance = &prepared.matters[0];
        let cash = &prepared.matters[1];
        let today = Local::now().date_naive();
        let mut ledger = Ledger::default();
        ledger.record(signed_entry(rebalance, Status::Signed, 1));
        assert!(ledger.hidden(rebalance, today));
        assert!(
            ledger.hidden(cash, today),
            "one signed plan settles the other allocation matter"
        );
        // Declining one does not settle the other.
        let mut declined = Ledger::default();
        declined.record(signed_entry(rebalance, Status::Declined, 1));
        assert!(declined.hidden(rebalance, today));
        assert!(!declined.hidden(cash, today));
    }

    #[test]
    fn a_past_decision_is_named_with_the_drift_it_measured() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let prepared = prepare(&inputs_with(Some(&review), Some(&policy)));
        let matter = &prepared.matters[0];
        let mut ledger = Ledger::default();
        let mut entry = signed_entry(matter, Status::Signed, 40);
        entry.figures.insert("class:Crypto".to_string(), 18.6);
        ledger.record(entry);
        let line = ledger.prior(matter).expect("a past decision is named");
        assert!(
            line.starts_with("You signed a halfway rebalance on "),
            "{line}"
        );
        assert!(line.contains("went from 18.6 to 15.0 points"), "{line}");
    }

    #[test]
    fn a_record_has_the_shape_the_documents_page_reads() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let prepared = prepare(&inputs_with(Some(&review), Some(&policy)));
        let matter = &prepared.matters[0];
        let today = Local::now().date_naive();
        let text = record_markdown(matter, Status::Signed, today, "EUR", None);
        assert!(text.starts_with("# Decision: Rebalance halfway to your targets\n"));
        assert!(text.contains(&format!("Date: {}", today.format("%Y-%m-%d"))));
        assert!(text.contains("Status: Signed"));
        for section in [
            "## Context",
            "## Decision",
            "## Rationale",
            "## Risks",
            "## Review Date",
        ] {
            assert!(text.contains(section), "{section}");
        }
        assert!(text.contains("| Crypto | Sell | €"), "{text}");
        assert!(text.contains("| Stocks | Buy | €"));
        assert!(text.contains("Stahl never trades."));
        let declined = record_markdown(
            matter,
            Status::Declined,
            today,
            "EUR",
            Some("Not this month."),
        );
        assert!(declined.contains("Status: Declined"));
        assert!(declined.contains("Not this month."));
        assert!(declined.contains("The proposal was:"));
    }

    #[test]
    fn a_ledger_round_trips_and_keeps_one_entry_per_matter() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let prepared = prepare(&inputs_with(Some(&review), Some(&policy)));
        let matter = &prepared.matters[0];
        let today = Local::now().date_naive();
        let mut ledger = Ledger::default();
        ledger.record(entry(
            matter,
            Status::Declined,
            today,
            Some("no".to_string()),
        ));
        ledger.record(entry(matter, Status::Signed, today, None));
        assert_eq!(ledger.entries.len(), 1);
        assert_eq!(ledger.entries[0].status, Status::Signed);
        let text = write_ledger(&ledger).unwrap();
        assert!(text.contains("\"id\": \"rebalance\""));
        assert_eq!(read_ledger(&text), Ok(ledger));
        assert!(read_ledger("not json").is_err());
    }

    #[test]
    fn a_record_is_read_back_with_its_title_date_and_status() {
        let dir = env::temp_dir().join(format!(
            "stahl-matters-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        let records = dir.join("portfolio/decisions");
        fs::create_dir_all(&records).unwrap();
        fs::write(
            records.join("2026-10-03-halfway.md"),
            "# Decision: Rebalance halfway\n\nDate: 2026-10-03\nStatus: Signed\n\n## Context\n",
        )
        .unwrap();
        fs::write(records.join("notes.txt"), "not a record").unwrap();
        let read = decisions(&dir);
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].title, "Rebalance halfway");
        assert_eq!(read[0].date, "2026-10-03");
        assert_eq!(read[0].status, "Signed");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_record_path_never_overwrites_one() {
        let dir = env::temp_dir().join(format!(
            "stahl-matters-paths-{}-{}",
            process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos())
        ));
        fs::create_dir_all(&dir).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 10, 3).unwrap();
        let first = record_path(&dir, "Rebalance Stocks", today);
        assert_eq!(first.file_name().unwrap(), "2026-10-03-rebalance-stocks.md");
        fs::write(&first, "taken").unwrap();
        let second = record_path(&dir, "Rebalance Stocks", today);
        assert_eq!(
            second.file_name().unwrap(),
            "2026-10-03-rebalance-stocks-2.md"
        );
        fs::remove_dir_all(&dir).ok();
    }

    /// [`inputs`] with no allocation list, for tests that only vary the review.
    fn inputs_with<'a>(review: Option<&'a Review>, policy: Option<&'a Policy>) -> Inputs<'a> {
        Inputs {
            agenda: agenda::Inputs {
                review,
                has_policy: policy.is_some(),
                is_workspace: true,
                findings: &[],
                positions: &[],
                cash: "€28,000.00".to_string(),
            },
            allocation: &[],
            policy,
            privacy: false,
            currency: "EUR".to_string(),
        }
    }

    #[test]
    fn a_matter_says_what_signing_records() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let prepared = prepare(&inputs_with(Some(&review), Some(&policy)));
        for matter in &prepared.matters {
            assert!(matter.records.contains("Signing records"));
            assert!(matter.records.contains("Stahl never trades"));
            assert!(matter.adjust.is_some());
        }
    }

    #[test]
    fn figures_and_steps_line_up() {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let prepared = prepare(&inputs_with(Some(&review), Some(&policy)));
        for matter in &prepared.matters {
            for key in matter.figures.keys() {
                assert!(matter.steps.contains_key(key), "{key} has no step");
            }
        }
        let mut entry = Entry {
            id: "x".to_string(),
            kind: Kind::Rebalance,
            group: Some("allocation".to_string()),
            status: Status::Declined,
            at: "2026-10-03".to_string(),
            reason: None,
            until: None,
            summary: "a halfway rebalance".to_string(),
            policy: "Balanced Growth".to_string(),
            figures: BTreeMap::new(),
            out_of_band: Vec::new(),
        };
        let matter = &prepared.matters[0];
        entry.figures = matter.figures.clone();
        entry.out_of_band = matter.out_of_band.clone();
        assert!(!material(&entry, matter));
        entry.out_of_band.clear();
        assert!(
            material(&entry, matter),
            "a new class out of band is material"
        );
    }

    // --- The ledger on disk ---

    fn sample() -> (Matter, Entry) {
        let review = demo_review();
        let policy = policy(Some(20_000.0));
        let prepared = prepare(&inputs_with(Some(&review), Some(&policy)));
        let matter = prepared.matters[0].clone();
        let today = Local::now().date_naive();
        let entry = entry(&matter, Status::Signed, today, None);
        (matter, entry)
    }

    fn request(matter: &Matter) -> RecordRequest {
        RecordRequest {
            matter: matter.clone(),
            status: Status::Signed,
            reason: None,
            date: Local::now().date_naive(),
            currency: "EUR".to_string(),
        }
    }

    fn backend(dir: &Path, runtime: &tokio::runtime::Runtime) -> Backend {
        crate::demo::backend(dir, Some(dir), runtime.handle().clone())
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("stahl-ledger-{name}-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("portfolio")).unwrap();
        dir
    }

    #[test]
    fn an_unreadable_ledger_is_kept_reported_and_never_written_over() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (matter, entry) = sample();
        let mut kinds = Vec::new();
        for (name, text) in [
            ("hand-edit", "{ \"entries\": [ oops".to_string()),
            (
                "new-kind",
                r#"{"version":2,"entries":[{"id":"x","kind":"time_travel","status":"signed","at":"2026-01-01","policy":"P"}]}"#.to_string(),
            ),
            (
                "null-figure",
                r#"{"version":1,"entries":[{"id":"x","kind":"rebalance","status":"signed","at":"2026-01-01","policy":"P","figures":{"class:Stocks":null}}]}"#.to_string(),
            ),
        ] {
            let dir = scratch(name);
            let path = dir.join("portfolio/matters.json");
            fs::write(&path, &text).unwrap();
            let read = load_ledger(&dir);
            let problem = read.problem.expect("the page is told");
            assert!(problem.contains("can't be read"), "{problem}");
            assert!(read.ledger.entries.is_empty());
            let backend = backend(&dir, &runtime);
            let result = runtime.block_on(backend.decide(
                &dir,
                entry.clone(),
                Some(request(&matter)),
            ));
            let error = result.expect_err("a decision is refused");
            assert!(error.starts_with("Nothing was written"), "{error}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text, "{name}: kept");
            assert!(decisions(&dir).is_empty(), "{name}: no record either");
            kinds.push(name);
        }
        assert_eq!(kinds.len(), 3);
    }

    #[test]
    fn a_figure_that_is_not_a_number_is_never_written() {
        let (mut matter, _) = sample();
        matter.figures.insert("class:Odd".to_string(), f64::NAN);
        let entry = entry(&matter, Status::Signed, Local::now().date_naive(), None);
        assert!(!entry.figures.contains_key("class:Odd"));
        assert!(
            read_ledger(
                &write_ledger(&Ledger {
                    version: 1,
                    entries: vec![entry]
                })
                .unwrap()
            )
            .is_ok()
        );
    }

    /// Restores `PORTFOLIO_RS_DATA_DIR` when dropped, and keeps two tests that set it apart.
    struct DataDir {
        before: Option<std::ffi::OsString>,
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl DataDir {
        /// Point the variable at `dir`, which is a scratch directory, so a test that reads it
        /// meanwhile finds somewhere harmless.
        fn set(dir: &Path) -> Self {
            static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let guard = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
            let before = env::var_os("PORTFOLIO_RS_DATA_DIR");
            // SAFETY: held under a lock, undone on drop, and the value is a scratch directory.
            unsafe { env::set_var("PORTFOLIO_RS_DATA_DIR", dir) };
            Self {
                before,
                _guard: guard,
            }
        }
    }

    impl Drop for DataDir {
        fn drop(&mut self) {
            // SAFETY: as in `set`.
            unsafe {
                match &self.before {
                    Some(value) => env::set_var("PORTFOLIO_RS_DATA_DIR", value),
                    None => env::remove_var("PORTFOLIO_RS_DATA_DIR"),
                }
            }
        }
    }

    #[test]
    fn two_workspaces_keep_separate_ledgers_whatever_the_environment_says() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (matter, entry) = sample();
        let first = scratch("first");
        let second = scratch("second");
        // Both workspaces share the one place the environment names for history.
        let shared = scratch("shared");
        let _data_dir = DataDir::set(&shared);
        assert!(matters_dir(&first).starts_with(&first));
        assert_ne!(matters_dir(&first), matters_dir(&second));
        let backend = backend(&first, &runtime);
        runtime
            .block_on(backend.decide(&first, entry, Some(request(&matter))))
            .unwrap();
        assert_eq!(load_ledger(&first).ledger.entries.len(), 1);
        assert!(load_ledger(&second).ledger.entries.is_empty());
        assert!(decisions(&second).is_empty());
        assert!(
            !shared.join(LEDGER_FILE).exists() && !shared.join("history/main").exists(),
            "nothing went to the environment's directory"
        );
    }

    #[test]
    fn decisions_made_close_together_both_end_up_in_the_file() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (matter, entry) = sample();
        for round in 0..40 {
            let dir = scratch(&format!("serial-{round}"));
            let backend = backend(&dir, &runtime);
            let mut other = entry.clone();
            other.id = "other".to_string();
            let (a, b) = runtime.block_on(async {
                let first = {
                    let (backend, dir, entry) = (backend.clone(), dir.clone(), entry.clone());
                    let request = request(&matter);
                    backend
                        .clone()
                        .run(async move { backend.decide(&dir, entry, Some(request)).await })
                };
                let second = {
                    let (backend, dir) = (backend.clone(), dir.clone());
                    let other = other.clone();
                    backend
                        .clone()
                        .run(async move { backend.decide(&dir, other, None).await })
                };
                (first.await, second.await)
            });
            a.unwrap();
            b.unwrap();
            let ids: Vec<_> = load_ledger(&dir)
                .ledger
                .entries
                .into_iter()
                .map(|entry| entry.id)
                .collect();
            assert_eq!(ids.len(), 2, "round {round}: {ids:?}");
            assert!(ids.contains(&entry.id) && ids.contains(&"other".to_string()));
        }
    }

    #[test]
    fn a_ledger_from_a_newer_version_is_kept_and_never_lowered() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (matter, entry) = sample();
        let dir = scratch("newer");
        let path = dir.join("portfolio/matters.json");
        let text = r#"{"version":2,"entries":[{"id":"x","kind":"rebalance","status":"signed","at":"2026-01-01","policy":"P"}]}"#;
        fs::write(&path, text).unwrap();
        let read = load_ledger(&dir);
        let problem = read.problem.expect("a newer ledger is reported");
        assert!(problem.contains("version 2"), "{problem}");
        let error = runtime
            .block_on(backend(&dir, &runtime).decide(&dir, entry, Some(request(&matter))))
            .expect_err("refused");
        assert!(error.starts_with("Nothing was written"), "{error}");
        assert_eq!(fs::read_to_string(&path).unwrap(), text, "byte for byte");
        assert!(decisions(&dir).is_empty());
        let mut ledger = Ledger {
            version: 2,
            entries: Vec::new(),
        };
        ledger.record(sample().1);
        assert_eq!(ledger.version, 2, "recording never lowers a version");
    }

    #[test]
    fn an_unreadable_old_ledger_is_named_by_its_old_path() {
        let dir = scratch("old-path");
        let old = dir.join("portfolio/history/main");
        fs::create_dir_all(&old).unwrap();
        fs::write(old.join(LEDGER_FILE), "{ oops").unwrap();
        let problem = load_ledger(&dir).problem.expect("reported");
        assert!(
            problem.contains(&old.join(LEDGER_FILE).display().to_string()),
            "{problem}"
        );
        assert!(
            !problem.contains(
                &dir.join("portfolio")
                    .join(LEDGER_FILE)
                    .display()
                    .to_string()
            ),
            "{problem}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_write_leaves_no_record_and_the_old_ledger() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (matter, entry) = sample();
        let dir = scratch("failed");
        let backend = backend(&dir, &runtime);
        // An earlier decision, written properly.
        let mut earlier = entry.clone();
        earlier.id = "earlier".to_string();
        runtime
            .block_on(backend.decide(&dir, earlier, None))
            .unwrap();
        let before = fs::read_to_string(dir.join("portfolio/matters.json")).unwrap();
        // The records can be written, the ledger can't: the directory refuses new files.
        fs::create_dir_all(decisions_dir(&dir)).unwrap();
        let portfolio = dir.join("portfolio");
        fs::set_permissions(&portfolio, fs::Permissions::from_mode(0o555)).unwrap();
        let result = runtime.block_on(backend.decide(&dir, entry, Some(request(&matter))));
        fs::set_permissions(&portfolio, fs::Permissions::from_mode(0o755)).unwrap();
        let error = result.expect_err("the ledger could not be written");
        assert!(error.starts_with("Nothing was written"), "{error}");
        assert_eq!(
            fs::read_to_string(dir.join("portfolio/matters.json")).unwrap(),
            before
        );
        assert!(decisions(&dir).is_empty(), "the orphaned record is gone");
    }

    #[test]
    fn a_decision_that_fails_gives_the_matter_its_old_entry_back() {
        let (matter, entry) = sample();
        let mut page = MattersPage::default();
        let mut old = entry.clone();
        old.status = Status::Postponed;
        let before = Ledger {
            version: 1,
            entries: vec![old.clone()],
        };
        page.ledger = before.clone();
        page.ledger.record(entry);
        page.restore(&matter.id, &before);
        assert_eq!(page.ledger.entries, vec![old]);
        page.restore("other", &Ledger::default());
        assert_eq!(page.ledger.entries.len(), 1);
    }
}
