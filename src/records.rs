//! The records room: what has truly happened to this portfolio, as plates on a wall.
//!
//! Everything here is pure. [`find`] reads a portfolio's stored valuations, its holdings and its
//! policy, and returns the records they honestly support, newest first. The same history always
//! gives the same list, and a record keeps its id for as long as it stands, so the app can tell
//! which plates the owner has already been shown.
//!
//! A record is only made where the data really carries it. A single day's move is measured only
//! between two consecutive valuations with no money in or out; a month only between two
//! consecutive month ends, likewise. Where that isn't true, the record is left out rather than
//! guessed at.

use chrono::{Datelike as _, Months, NaiveDate};
use portfolio_rs::policy::AllocationTarget;

use crate::format;

/// A move smaller than this is a cent, not a move.
const CENT: f64 = 0.005;
/// Money in or out below this is rounding, not a flow.
const NO_MONEY: f64 = 0.5;
/// A run shorter than this is not worth a plate.
const SHORTEST_RUN: i64 = 2;
/// A high that was beaten the next day says nothing about the portfolio. One that stood for
/// this long, or still stands, is a record worth engraving.
const STANDING_DAYS: i64 = 30;
/// The most years of history a plate can celebrate. Fifty is a guess at "longer than anyone".
const LONGEST: i32 = 50;

/// One stored valuation of the whole portfolio.
#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub total: f64,
    /// What each asset class was worth that day.
    pub classes: Vec<(String, f64)>,
    /// Money in or out since the valuation before it. Zero when nothing moved.
    pub flow: f64,
}

impl Day {
    /// The valuation on record, with no money in or out.
    fn quiet(&self) -> bool {
        self.flow.abs() < NO_MONEY
    }
}

/// One purchase, as the ledger has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Purchase {
    pub date: NaiveDate,
    pub quantity: f64,
    /// The price paid per unit, in the holding's own currency, when the ledger knows it.
    pub price: Option<f64>,
}

/// A holding today, with what its own milestones need.
#[derive(Clone, Debug, PartialEq)]
pub struct Holding {
    pub id: String,
    pub name: String,
    /// In the holding's own currency, when known.
    pub currency: Option<String>,
    pub purchases: Vec<Purchase>,
    /// Its price on each stored day, oldest first, where the snapshot recorded one.
    pub prices: Vec<(NaiveDate, f64)>,
    /// The average price paid per unit today.
    pub average_cost: Option<f64>,
}

/// Everything a record may be made from.
#[derive(Clone, Debug, Default)]
pub struct Facts<'a> {
    /// Oldest first. Days out of order or repeated are put right before anything is read.
    pub days: &'a [Day],
    pub holdings: &'a [Holding],
    /// The policy's classes. Without one, no tolerance record can be made.
    pub allocations: &'a [AllocationTarget],
    pub currency: &'a str,
}

/// An amount of money, and the currency it is in. `None` is the portfolio's own.
#[derive(Clone, Debug, PartialEq)]
pub struct Money {
    pub amount: f64,
    pub currency: Option<String>,
}

impl Money {
    fn here(amount: f64) -> Money {
        Money {
            amount,
            currency: None,
        }
    }
}

/// The figure a plate carries, under its title.
#[derive(Clone, Debug, PartialEq)]
pub enum Figure {
    /// A figure of the portfolio's own: a total, a change.
    Money(Money),
    /// Something that isn't money: a run of days, a multiple, a number of units.
    Text(String),
}

/// What kind of record this is, and the figures its words need.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// The first valuation Stahl kept.
    Begins {
        total: f64,
    },
    /// A value above every one before it, and the high it replaced.
    High {
        previous: f64,
        previous_date: NaiveDate,
    },
    /// A round number crossed for the first time, and the total that closed above it.
    Round {
        crossed: f64,
        closed: f64,
    },
    /// A year since the first valuation.
    Anniversary {
        years: i32,
        then: f64,
        now: f64,
    },
    BestDay {
        change: f64,
    },
    WorstDay {
        change: f64,
        recovered: Option<NaiveDate>,
    },
    BestMonth {
        change: f64,
        since: NaiveDate,
    },
    /// The longest stretch with every class inside its policy band.
    InTolerance {
        days: i64,
        from: NaiveDate,
    },
    /// A holding's first purchase.
    FirstBought {
        holding: String,
        quantity: f64,
        price: Option<f64>,
        currency: Option<String>,
    },
    /// A holding worth at least twice what was paid for it.
    Doubled {
        holding: String,
        multiple: f64,
    },
    /// The deepest fall from a high, and the day it was recovered.
    Drawdown {
        fall: f64,
        recovered: Option<NaiveDate>,
    },
}

impl Kind {
    /// The small word over a plate's title, so a wall can be read at a glance.
    pub fn caption(&self) -> &'static str {
        match self {
            Kind::Begins { .. } => "The start",
            Kind::High { .. } => "A high",
            Kind::Round { .. } => "A milestone",
            Kind::Anniversary { .. } => "An anniversary",
            Kind::BestDay { .. } | Kind::WorstDay { .. } => "A day",
            Kind::BestMonth { .. } => "A month",
            Kind::InTolerance { .. } => "The policy",
            Kind::FirstBought { .. } | Kind::Doubled { .. } => "A holding",
            Kind::Drawdown { .. } => "A fall",
        }
    }

    /// How plates dated the same day stand among themselves, good news above bad.
    fn rank(&self) -> u8 {
        match self {
            Kind::High { .. } => 0,
            Kind::Round { .. } => 1,
            Kind::Anniversary { .. } => 2,
            Kind::Doubled { .. } => 3,
            Kind::BestDay { .. } => 4,
            Kind::BestMonth { .. } => 5,
            Kind::InTolerance { .. } => 6,
            Kind::WorstDay { .. } => 7,
            Kind::Drawdown { .. } => 8,
            Kind::FirstBought { .. } => 9,
            Kind::Begins { .. } => 10,
        }
    }
}

/// One plate on the wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// Stable for as long as the record stands, and free of amounts, so it can be written down.
    pub id: String,
    pub kind: Kind,
    pub date: NaiveDate,
    pub figure: Option<Figure>,
    /// Shown beside the figure when the record is about a change.
    pub percent: Option<f64>,
}

impl Record {
    /// The plate's headline: the serif line under the date.
    pub fn title(&self) -> String {
        match &self.kind {
            Kind::Begins { .. } => "The first valuation".to_string(),
            Kind::High { .. } => "A new high".to_string(),
            Kind::Round { .. } => "A round number".to_string(),
            Kind::Anniversary { years, .. } => format!("{} on record", years_in_words(*years)),
            Kind::BestDay { .. } => "The best day".to_string(),
            Kind::WorstDay { .. } => "The worst day".to_string(),
            Kind::BestMonth { .. } => "The best month".to_string(),
            Kind::InTolerance { .. } => "In line with the policy".to_string(),
            Kind::FirstBought { holding, .. } => holding.clone(),
            Kind::Doubled { holding, .. } => holding.clone(),
            Kind::Drawdown { .. } => "The deepest fall".to_string(),
        }
    }

    /// The sign the figure carries, where it has one: gains above, losses below, facts neither.
    pub fn tone(&self) -> Option<f64> {
        match &self.kind {
            Kind::BestDay { change, .. } => Some(*change),
            Kind::WorstDay { change, .. } => Some(*change),
            Kind::Drawdown { fall, .. } => Some(*fall),
            Kind::BestMonth { change, .. } => Some(*change),
            _ => None,
        }
    }

    /// The figure as a plate shows it: money, or a count. `None` in privacy mode for money.
    pub fn figure_text(&self, hidden: bool, portfolio: &str) -> Option<String> {
        let figure = self.figure.as_ref()?;
        Some(match figure {
            Figure::Money(money) => {
                let code = money.currency.as_deref().unwrap_or(portfolio);
                if hidden {
                    format::HIDDEN.to_string()
                } else {
                    whole(money.amount, code)
                }
            }
            Figure::Text(text) => text.clone(),
        })
    }

    /// One sentence in the bank's voice. A loss is told plainly and with respect; a good record
    /// may smile once.
    pub fn sentence(&self, hidden: bool, portfolio: &str) -> String {
        let words = Words { hidden, portfolio };
        match &self.kind {
            Kind::Begins { .. } => {
                "The first valuation Stahl kept. The wall grows from here.".to_string()
            }
            Kind::High {
                previous,
                previous_date,
            } => {
                let day = words.day(*previous_date, self.date);
                match words.money(*previous) {
                    Some(previous) => {
                        format!("The highest value on record, past the {previous} of {day}.")
                    }
                    None => format!("The highest value on record, past the one of {day}."),
                }
            }
            Kind::Round { crossed, closed } => {
                match (words.money(*crossed), words.money(*closed)) {
                    (Some(crossed), Some(closed)) => format!(
                        "The portfolio passed {crossed} for the first time, closing at {closed}."
                    ),
                    _ => "The portfolio passed a round number for the first time.".to_string(),
                }
            }
            Kind::Anniversary { years, then, now } => {
                let since = format!("{} since the first valuation.", years_in_words(*years));
                match (words.money(*then), words.money(*now)) {
                    (Some(then), Some(now)) => {
                        format!("{since} The portfolio has gone from {then} to {now}.")
                    }
                    _ => since,
                }
            }
            Kind::BestDay { .. } => "A good day, and the best of them so far.".to_string(),
            Kind::WorstDay { recovered, .. } => match recovered {
                Some(day) => format!(
                    "The largest single-day fall on record, recovered by {}.",
                    words.day(*day, self.date)
                ),
                None => "The largest single-day fall on record.".to_string(),
            },
            Kind::BestMonth { change, since, .. } => {
                let month = month_name(*since, self.date);
                if *change >= 0.0 {
                    match words.money(*change) {
                        Some(change) => {
                            format!(
                                "{} closed {change} higher than {month}.",
                                self.date.format("%B")
                            )
                        }
                        None => format!(
                            "{} was the best month on record, ahead of {month}.",
                            self.date.format("%B")
                        ),
                    }
                } else {
                    match words.money(*change) {
                        Some(change) => {
                            format!(
                                "{} closed {change} lower than {month}.",
                                self.date.format("%B")
                            )
                        }
                        None => format!(
                            "{} was the best month on record, though it closed lower than {month}.",
                            self.date.format("%B")
                        ),
                    }
                }
            }
            Kind::InTolerance { days, from } => format!(
                "From {} to {}, every class stayed inside its band: {days} days. Nothing needed \
                 your signature.",
                words.day(*from, self.date),
                words.day(self.date, self.date),
            ),
            Kind::FirstBought {
                price, currency, ..
            } => {
                let day = self.date.format("%-d %B %Y");
                let code = currency.as_deref().unwrap_or(portfolio);
                match price.filter(|_| !hidden) {
                    Some(price) => format!(
                        "First bought on {day}, at {} each.",
                        format::currency(price, code)
                    ),
                    None => format!("First bought on {day}."),
                }
            }
            Kind::Doubled { .. } => {
                "More than twice what you paid for it, measured against the average purchase \
                 price."
                    .to_string()
            }
            Kind::Drawdown {
                fall, recovered, ..
            } => {
                let low = words.day(self.date, self.date);
                match (words.money(*fall), recovered) {
                    (Some(fall), Some(day)) => format!(
                        "The low point so far: down {fall} from the high, on {low}. Recovered on \
                         {}.",
                        words.day(*day, self.date)
                    ),
                    (Some(fall), None) => format!(
                        "The low point so far: down {fall} from the high, on {low}. Not yet \
                         recovered."
                    ),
                    (None, Some(day)) => format!(
                        "The deepest fall on record, on {low}. Recovered on {}.",
                        words.day(*day, self.date)
                    ),
                    (None, None) => {
                        format!("The deepest fall on record, on {low}. Not yet recovered.")
                    }
                }
            }
        }
    }
}

struct Words<'a> {
    hidden: bool,
    portfolio: &'a str,
}

impl Words<'_> {
    /// An amount in prose, unsigned: the words carry the direction. `None` when hidden.
    fn money(&self, value: f64) -> Option<String> {
        (!self.hidden).then(|| whole(value.abs(), self.portfolio))
    }

    /// A day as prose: "14 March", or "14 March 2025" when it is another year than the plate's.
    fn day(&self, day: NaiveDate, seen_from: NaiveDate) -> String {
        day_name(day, seen_from)
    }
}

/// "14 March", or "14 March 2025" when it is not the same year as the day the plate is about.
pub fn day_name(day: NaiveDate, seen_from: NaiveDate) -> String {
    if day.year() == seen_from.year() {
        day.format("%-d %B").to_string()
    } else {
        day.format("%-d %B %Y").to_string()
    }
}

/// The month of `since`, named against the plate's own date.
fn month_name(since: NaiveDate, seen_from: NaiveDate) -> String {
    if since.year() == seen_from.year() {
        since.format("%B").to_string()
    } else {
        since.format("%B %Y").to_string()
    }
}

/// An amount without cents once it is large enough that they don't matter.
fn whole(value: f64, currency: &str) -> String {
    let value = value.abs();
    if value < 100.0 {
        return format::currency(value, currency);
    }
    let rounded = format::currency(value.round(), currency);
    rounded.strip_suffix(".00").unwrap_or(&rounded).to_string()
}

/// A count of units, without the zeros a float carries.
fn units(value: f64) -> String {
    let text = format!("{value:.6}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text.is_empty() {
        "0".to_string()
    } else {
        text.to_string()
    }
}

/// "One", "Two", … "Ten", then the digits.
fn years_in_words(years: i32) -> String {
    const NAMES: [&str; 10] = [
        "One", "Two", "Three", "Four", "Five", "Six", "Seven", "Eight", "Nine", "Ten",
    ];
    if years == 1 {
        return "One year".to_string();
    }
    match usize::try_from(years)
        .ok()
        .and_then(|index| NAMES.get(index - 1))
    {
        Some(name) => format!("{name} years"),
        None => format!("{years} years"),
    }
}

/// The records this history honestly supports, newest first.
pub fn find(facts: &Facts) -> Vec<Record> {
    let days = ordered(facts.days);
    if days.is_empty() {
        return Vec::new();
    }
    let mut records = Vec::new();
    begins(&days, &mut records);
    highs(&days, &mut records);
    rounds(&days, &mut records);
    anniversaries(&days, &mut records);
    days_moved(&days, &mut records);
    best_month(&days, &mut records);
    tolerance_run(&days, facts.allocations, &mut records);
    first_bought(facts.holdings, &mut records);
    doublings(&days, facts.holdings, &mut records);
    drawdown(&days, &mut records);
    records.sort_by(|a, b| {
        b.date
            .cmp(&a.date)
            .then_with(|| a.kind.rank().cmp(&b.kind.rank()))
            .then_with(|| b.id.cmp(&a.id))
    });
    records.dedup_by(|a, b| a.id == b.id);
    records
}

/// The days oldest first, one per date.
fn ordered(days: &[Day]) -> Vec<&Day> {
    let mut ordered: Vec<&Day> = days.iter().collect();
    ordered.sort_by_key(|day| day.date);
    ordered.dedup_by_key(|day| day.date);
    ordered
}

fn date_key(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn begins(days: &[&Day], records: &mut Vec<Record>) {
    let first = days[0];
    records.push(Record {
        id: format!("{}:begins", date_key(first.date)),
        kind: Kind::Begins { total: first.total },
        date: first.date,
        figure: Some(Figure::Money(Money::here(first.total))),
        percent: None,
    });
}

fn highs(days: &[&Day], records: &mut Vec<Record>) {
    let mut high = days[0].total;
    let mut high_date = days[0].date;
    for (index, day) in days.iter().enumerate().skip(1) {
        if day.total <= high + CENT {
            continue;
        }
        // A high that a later day beat within the month was a step on the way up, not a record.
        let stands = days[index + 1..]
            .iter()
            .find(|later| later.total > day.total + CENT)
            .is_none_or(|beaten| (beaten.date - day.date).num_days() >= STANDING_DAYS);
        if stands {
            records.push(Record {
                id: format!("{}:high", date_key(day.date)),
                kind: Kind::High {
                    previous: high,
                    previous_date: high_date,
                },
                date: day.date,
                figure: Some(Figure::Money(Money::here(day.total))),
                percent: None,
            });
        }
        high = day.total;
        high_date = day.date;
    }
}

/// The round numbers a portfolio notices, in the currency it keeps its books in: 1,000, 2,500,
/// 5,000, 10,000, 25,000, and so on. Below a thousand there is nothing to celebrate quietly.
fn ladder(up_to: f64) -> Vec<f64> {
    let mut values = Vec::new();
    for power in 3..=15 {
        for step in [1.0, 2.5, 5.0] {
            let value = step * 10f64.powi(power);
            if value > up_to + CENT {
                return values;
            }
            values.push(value);
        }
    }
    values
}

fn rounds(days: &[&Day], records: &mut Vec<Record>) {
    let high = days.iter().map(|day| day.total).fold(f64::MIN, f64::max);
    for round in ladder(high) {
        // A portfolio that opens above a round number never crossed it on record.
        let Some(day) = days.iter().find(|day| day.total >= round - CENT) else {
            continue;
        };
        if day.date == days[0].date {
            continue;
        }
        records.push(Record {
            id: format!("{}:round:{:012.0}", date_key(day.date), round),
            kind: Kind::Round {
                crossed: round,
                closed: day.total,
            },
            date: day.date,
            figure: Some(Figure::Money(Money::here(round))),
            percent: None,
        });
    }
}

fn anniversaries(days: &[&Day], records: &mut Vec<Record>) {
    let start = days[0];
    let last = days[days.len() - 1];
    for years in 1..=LONGEST {
        let Some(anniversary) = start
            .date
            .checked_add_months(Months::new(12 * years as u32))
        else {
            break;
        };
        if anniversary > last.date {
            break;
        }
        // The first valuation on or after the day, which is the day the wall has.
        let Some(day) = days.iter().find(|day| day.date >= anniversary) else {
            break;
        };
        records.push(Record {
            id: format!("{}:years:{}", date_key(day.date), years),
            kind: Kind::Anniversary {
                years,
                then: start.total,
                now: day.total,
            },
            date: day.date,
            figure: Some(Figure::Money(Money::here(day.total))),
            percent: None,
        });
    }
}

fn days_moved(days: &[&Day], records: &mut Vec<Record>) {
    let mut best: Option<(f64, Option<f64>, NaiveDate, f64)> = None;
    let mut worst: Option<(f64, Option<f64>, NaiveDate, f64)> = None;
    for pair in days.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        if (after.date - before.date).num_days() != 1 || !after.quiet() {
            continue;
        }
        let change = after.total - before.total;
        let percent = (before.total > 0.0).then(|| change / before.total * 100.0);
        if best.is_none_or(|(seen, ..)| change > seen) {
            best = Some((change, percent, after.date, before.total));
        }
        if worst.is_none_or(|(seen, ..)| change < seen) {
            worst = Some((change, percent, after.date, before.total));
        }
    }
    if let Some((change, percent, date, _)) = best.filter(|(change, ..)| *change >= CENT) {
        records.push(Record {
            id: format!("{}:best-day", date_key(date)),
            kind: Kind::BestDay { change },
            date,
            figure: Some(Figure::Money(Money::here(change))),
            percent,
        });
    }
    if let Some((change, percent, date, before)) = worst.filter(|(change, ..)| *change <= -CENT) {
        // The day it was recovered: the first later valuation back at the level it fell from.
        let recovered = days
            .iter()
            .filter(|day| day.date > date && day.quiet())
            .find(|day| day.total >= before - CENT)
            .map(|day| day.date);
        records.push(Record {
            id: format!("{}:worst-day", date_key(date)),
            kind: Kind::WorstDay { change, recovered },
            date,
            figure: Some(Figure::Money(Money::here(change))),
            percent,
        });
    }
}

/// The last valuation in each month, oldest first.
fn month_ends<'a>(days: &'a [&'a Day]) -> Vec<&'a Day> {
    let mut ends: Vec<&Day> = Vec::new();
    for day in days {
        match ends.last_mut() {
            Some(last)
                if (last.date.year(), last.date.month()) == (day.date.year(), day.date.month()) =>
            {
                *last = day;
            }
            _ => ends.push(day),
        }
    }
    ends
}

fn best_month(days: &[&Day], records: &mut Vec<Record>) {
    let ends = month_ends(days);
    let mut best: Option<(f64, Option<f64>, &Day, NaiveDate)> = None;
    for pair in ends.windows(2) {
        let (before, after) = (pair[0], pair[1]);
        if !adjacent_months(before.date, after.date) {
            continue;
        }
        // Any money in or out during the month makes its move partly a deposit.
        let quiet = days
            .iter()
            .filter(|day| day.date > before.date && day.date <= after.date)
            .all(|day| day.quiet());
        if !quiet {
            continue;
        }
        let change = after.total - before.total;
        let percent = (before.total > 0.0).then(|| change / before.total * 100.0);
        if best.is_none_or(|(seen, ..)| change > seen) {
            best = Some((change, percent, after, before.date));
        }
    }
    if let Some((change, percent, after, since)) = best.filter(|(change, ..)| change.abs() >= CENT)
    {
        records.push(Record {
            id: format!("{}:best-month", date_key(after.date)),
            kind: Kind::BestMonth { change, since },
            date: after.date,
            figure: Some(Figure::Money(Money::here(change))),
            percent,
        });
    }
}

fn adjacent_months(before: NaiveDate, after: NaiveDate) -> bool {
    before
        .checked_add_months(Months::new(1))
        .is_some_and(|next| (next.year(), next.month()) == (after.year(), after.month()))
}

fn tolerance_run(days: &[&Day], allocations: &[AllocationTarget], records: &mut Vec<Record>) {
    let bands: Vec<(&str, f64, f64)> = allocations
        .iter()
        .filter_map(|target| {
            let tolerance = target.tolerance_percent?;
            Some((
                target.asset_class.as_str(),
                target.target_percent,
                tolerance,
            ))
        })
        .collect();
    if bands.is_empty() {
        return;
    }
    let inside = |day: &Day| {
        bands.iter().all(|(class, target, tolerance)| {
            let value: f64 = day
                .classes
                .iter()
                .filter(|(name, _)| name == class)
                .map(|(_, value)| value)
                .sum();
            let share = if day.total.abs() > CENT {
                value / day.total * 100.0
            } else {
                0.0
            };
            (share - target).abs() <= *tolerance
        })
    };
    let mut best: Option<(NaiveDate, NaiveDate, i64)> = None;
    let mut run: Option<NaiveDate> = None;
    let mut previous: Option<NaiveDate> = None;
    for day in days {
        let consecutive = previous.is_some_and(|previous| (day.date - previous).num_days() == 1);
        if inside(day) {
            let from = match run {
                Some(from) if consecutive => from,
                _ => day.date,
            };
            run = Some(from);
            let length = (day.date - from).num_days() + 1;
            if best.is_none_or(|(_, _, seen)| length > seen) {
                best = Some((from, day.date, length));
            }
        } else {
            run = None;
        }
        previous = Some(day.date);
    }
    if let Some((from, to, length)) = best.filter(|(_, _, length)| *length >= SHORTEST_RUN) {
        records.push(Record {
            id: "policy-run".to_string(),
            kind: Kind::InTolerance { days: length, from },
            date: to,
            figure: Some(Figure::Text(format!("{length} days"))),
            percent: None,
        });
    }
}

fn first_bought(holdings: &[Holding], records: &mut Vec<Record>) {
    for holding in holdings {
        let Some(first) = holding
            .purchases
            .iter()
            .min_by_key(|purchase| purchase.date)
        else {
            continue;
        };
        records.push(Record {
            id: format!("{}:first-bought:{}", date_key(first.date), holding.id),
            kind: Kind::FirstBought {
                holding: holding.name.clone(),
                quantity: first.quantity,
                price: first.price,
                currency: holding.currency.clone(),
            },
            date: first.date,
            figure: Some(Figure::Text(units(first.quantity))),
            percent: None,
        });
    }
}

fn doublings(days: &[&Day], holdings: &[Holding], records: &mut Vec<Record>) {
    let start = days[0].date;
    for holding in holdings {
        let Some(cost) = holding.average_cost.filter(|cost| *cost > CENT) else {
            continue;
        };
        // The average price only means the same thing across the whole history when every
        // purchase predates it; otherwise a later buy would move the bar under past prices.
        if holding
            .purchases
            .iter()
            .any(|purchase| purchase.date >= start)
        {
            continue;
        }
        let mut was_below = false;
        for (date, price) in &holding.prices {
            let multiple = price / cost;
            if multiple >= 2.0 {
                if was_below {
                    records.push(Record {
                        id: format!("{}:doubled:{}", date_key(*date), holding.id),
                        kind: Kind::Doubled {
                            holding: holding.name.clone(),
                            multiple,
                        },
                        date: *date,
                        figure: Some(Figure::Text(format!("{multiple:.1}×"))),
                        percent: None,
                    });
                }
                break;
            }
            was_below = true;
        }
    }
}

fn drawdown(days: &[&Day], records: &mut Vec<Record>) {
    let mut peak = days[0].total;
    let mut peak_date = days[0].date;
    let mut deepest: Option<(f64, NaiveDate, NaiveDate)> = None;
    for day in &days[1..] {
        if day.total > peak {
            peak = day.total;
            peak_date = day.date;
        }
        let fall = day.total - peak;
        if deepest.is_none_or(|(seen, ..)| fall < seen) {
            deepest = Some((fall, day.date, peak_date));
        }
    }
    let Some((fall, trough, peak_date)) = deepest.filter(|(fall, ..)| *fall <= -CENT) else {
        return;
    };
    // A deposit during the fall or the climb back would make both figures someone else's money.
    let recovered = days
        .iter()
        .filter(|day| day.date > trough)
        .find(|day| day.total >= peak - CENT)
        .map(|day| day.date);
    let end = recovered.unwrap_or(trough);
    let quiet = days
        .iter()
        .filter(|day| day.date > peak_date && day.date <= end)
        .all(|day| day.quiet());
    if !quiet {
        return;
    }
    records.push(Record {
        id: format!("{}:drawdown", date_key(trough)),
        kind: Kind::Drawdown { fall, recovered },
        date: trough,
        figure: Some(Figure::Money(Money::here(fall))),
        percent: (peak > 0.0).then(|| fall / peak * 100.0),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    /// A day with no money in or out, split evenly between two classes.
    fn quiet(text: &str, total: f64) -> Day {
        Day {
            date: day(text),
            total,
            classes: vec![
                ("Stocks".to_string(), total * 0.6),
                ("Cash".to_string(), total * 0.4),
            ],
            flow: 0.0,
        }
    }

    fn facts(days: &[Day]) -> Facts<'_> {
        Facts {
            days,
            holdings: &[],
            allocations: &[],
            currency: "EUR",
        }
    }

    fn find(days: &[Day]) -> Vec<Record> {
        super::find(&facts(days))
    }

    fn ids(records: &[Record]) -> Vec<&str> {
        records.iter().map(|record| record.id.as_str()).collect()
    }

    fn of_kind<'a>(records: &'a [Record], wanted: &str) -> Vec<&'a Record> {
        records
            .iter()
            .filter(|record| record.kind.caption() == wanted)
            .collect()
    }

    #[test]
    fn an_empty_history_has_no_records() {
        assert!(find(&[]).is_empty());
    }

    #[test]
    fn one_valuation_is_the_start_and_nothing_else() {
        let records = find(&[quiet("2026-03-01", 100_000.0)]);
        assert_eq!(ids(&records), ["2026-03-01:begins"]);
        assert_eq!(records[0].title(), "The first valuation");
        assert_eq!(
            records[0].sentence(false, "EUR"),
            "The first valuation Stahl kept. The wall grows from here."
        );
        assert_eq!(
            records[0].figure_text(false, "EUR").as_deref(),
            Some("€100,000")
        );
    }

    #[test]
    fn a_new_high_remembers_the_one_it_replaced() {
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 99_000.0),
            quiet("2026-03-03", 101_000.0),
            quiet("2026-03-04", 100_500.0),
            quiet("2026-03-05", 102_000.0),
            quiet("2026-03-30", 102_500.0),
        ];
        let records = find(&days);
        let highs = of_kind(&records, "A high");
        assert_eq!(
            highs.len(),
            1,
            "a high beaten the next day is a step, not a record: {:?}",
            ids(&records)
        );
        assert_eq!(highs[0].date, day("2026-03-30"));
        assert_eq!(
            highs[0].sentence(false, "EUR"),
            "The highest value on record, past the €102,000 of 5 March."
        );
        assert_eq!(
            highs[0].sentence(true, "EUR"),
            "The highest value on record, past the one of 5 March."
        );
    }

    #[test]
    fn round_numbers_are_the_ones_a_bank_marks() {
        assert_eq!(
            ladder(120_000.0),
            [
                1_000.0, 2_500.0, 5_000.0, 10_000.0, 25_000.0, 50_000.0, 100_000.0
            ]
        );
        let days = [
            quiet("2026-03-01", 9_000.0),
            quiet("2026-03-02", 26_000.0),
            quiet("2026-03-03", 51_000.0),
            quiet("2026-03-04", 49_000.0),
            quiet("2026-03-05", 52_000.0),
        ];
        let records = find(&days);
        let rounds: Vec<&Record> = of_kind(&records, "A milestone");
        assert_eq!(rounds.len(), 3, "{:?}", ids(&records));
        let crossed: Vec<f64> = rounds
            .iter()
            .map(|record| match record.kind {
                Kind::Round { crossed, .. } => crossed,
                _ => panic!("not a round"),
            })
            .collect();
        assert_eq!(crossed, [50_000.0, 25_000.0, 10_000.0], "newest first");
        // Crossing back down and up again does not cross it a second time.
        assert_eq!(
            rounds[0].sentence(false, "EUR"),
            "The portfolio passed €50,000 for the first time, closing at €51,000."
        );
    }

    #[test]
    fn a_portfolio_that_opens_above_a_round_number_did_not_cross_it() {
        let records = find(&[
            quiet("2026-03-01", 120_000.0),
            quiet("2026-03-02", 130_000.0),
        ]);
        assert!(of_kind(&records, "A milestone").is_empty());
    }

    #[test]
    fn each_year_on_record_is_its_own_plate() {
        let days = [
            quiet("2024-03-01", 100_000.0),
            quiet("2025-03-01", 110_000.0),
            quiet("2026-03-02", 130_000.0),
        ];
        let records = find(&days);
        let years = of_kind(&records, "An anniversary");
        assert_eq!(years.len(), 2, "{:?}", ids(&records));
        assert_eq!(years[0].title(), "Two years on record");
        assert_eq!(years[0].date, day("2026-03-02"));
        assert_eq!(
            years[0].sentence(false, "EUR"),
            "Two years since the first valuation. The portfolio has gone from €100,000 to €130,000."
        );
        assert_eq!(years[1].title(), "One year on record");
    }

    #[test]
    fn a_year_is_named_in_words_and_keeps_the_date_the_wall_has() {
        let days = [quiet("2024-02-29", 10_000.0), quiet("2025-03-01", 12_000.0)];
        let records = find(&days);
        let years = of_kind(&records, "An anniversary");
        assert_eq!(years.len(), 1);
        assert_eq!(years[0].date, day("2025-03-01"), "the first day on record");
    }

    #[test]
    fn the_best_and_worst_days_need_two_days_in_a_row_with_nothing_moving() {
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 103_000.0),
            quiet("2026-03-03", 101_000.0),
            // A gap: what happened between the 3rd and the 6th is not one day's move.
            quiet("2026-03-06", 110_000.0),
            quiet("2026-03-07", 104_000.0),
        ];
        let records = find(&days);
        let best = of_kind(&records, "A day")
            .into_iter()
            .find(|record| record.title() == "The best day")
            .cloned()
            .unwrap();
        let worst = of_kind(&records, "A day")
            .into_iter()
            .find(|record| record.title() == "The worst day")
            .cloned()
            .unwrap();
        assert_eq!(best.date, day("2026-03-02"), "the gap day is not counted");
        assert_eq!(best.percent, Some(3.0));
        assert_eq!(worst.date, day("2026-03-07"));
        assert_eq!(
            worst.sentence(false, "EUR"),
            "The largest single-day fall on record."
        );
    }

    #[test]
    fn a_deposit_is_not_a_best_day() {
        let mut days = vec![
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 101_000.0),
        ];
        days.push(Day {
            date: day("2026-03-03"),
            total: 151_000.0,
            classes: Vec::new(),
            flow: 50_000.0,
        });
        let records = find(&days);
        let best = of_kind(&records, "A day")
            .into_iter()
            .find(|record| record.title() == "The best day")
            .cloned();
        assert_eq!(
            best.map(|record| record.date),
            Some(day("2026-03-02")),
            "the deposit is left out"
        );
    }

    #[test]
    fn the_best_month_is_measured_between_two_month_ends() {
        let days = [
            quiet("2026-01-31", 100_000.0),
            quiet("2026-02-27", 102_000.0),
            quiet("2026-03-31", 109_000.0),
            // April is missing, so May's move is not a month's.
            quiet("2026-05-31", 120_000.0),
        ];
        let records = find(&days);
        let months = of_kind(&records, "A month");
        assert_eq!(months.len(), 1, "{:?}", ids(&records));
        assert_eq!(months[0].date, day("2026-03-31"));
        assert_eq!(
            months[0].sentence(false, "EUR"),
            "March closed €7,000 higher than February."
        );
        let percent = months[0].percent.unwrap();
        assert!((percent - 6.862_745_098).abs() < 1e-6, "{percent}");
    }

    #[test]
    fn a_month_with_money_moving_in_it_is_left_out() {
        let days = [
            quiet("2026-01-31", 100_000.0),
            quiet("2026-02-27", 102_000.0),
            Day {
                date: day("2026-03-31"),
                total: 160_000.0,
                classes: Vec::new(),
                flow: 50_000.0,
            },
        ];
        let records = find(&days);
        let months = of_kind(&records, "A month");
        assert_eq!(months.len(), 1, "{:?}", ids(&records));
        assert_eq!(
            months[0].date,
            day("2026-02-27"),
            "March's jump was money in"
        );
    }

    #[test]
    fn the_longest_run_inside_the_policy_needs_consecutive_days() {
        let target = AllocationTarget {
            asset_class: "Stocks".to_string(),
            target_percent: 60.0,
            tolerance_percent: Some(5.0),
        };
        let allocations = [target];
        // 60% stocks is inside; the 70% day is not.
        let at = |text: &str, total: f64, share: f64| Day {
            date: day(text),
            total,
            classes: vec![
                ("Stocks".to_string(), total * share),
                ("Cash".to_string(), total * (1.0 - share)),
            ],
            flow: 0.0,
        };
        let days = [
            at("2026-03-01", 100_000.0, 0.60),
            at("2026-03-02", 100_000.0, 0.62),
            at("2026-03-03", 100_000.0, 0.70),
            at("2026-03-04", 100_000.0, 0.60),
            at("2026-03-05", 100_000.0, 0.61),
            at("2026-03-06", 100_000.0, 0.59),
            // The 8th is not the 7th, so the run stops at the 6th.
            at("2026-03-08", 100_000.0, 0.60),
        ];
        let records = super::find(&Facts {
            days: &days,
            allocations: &allocations,
            currency: "EUR",
            ..Facts::default()
        });
        let run = of_kind(&records, "The policy");
        assert_eq!(run.len(), 1);
        assert_eq!(run[0].date, day("2026-03-06"));
        assert_eq!(run[0].figure_text(false, "EUR").as_deref(), Some("3 days"));
        assert_eq!(
            run[0].sentence(false, "EUR"),
            "From 4 March to 6 March, every class stayed inside its band: 3 days. Nothing needed \
             your signature."
        );
    }

    #[test]
    fn without_a_policy_there_is_no_tolerance_record() {
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 101_000.0),
        ];
        assert!(of_kind(&find(&days), "The policy").is_empty());
    }

    #[test]
    fn a_holding_remembers_its_first_purchase() {
        let holding = Holding {
            id: "pos_spy".to_string(),
            name: "S&P 500".to_string(),
            currency: Some("USD".to_string()),
            purchases: vec![
                Purchase {
                    date: day("2024-01-10"),
                    quantity: 80.0,
                    price: Some(410.0),
                },
                Purchase {
                    date: day("2025-06-01"),
                    quantity: 10.0,
                    price: Some(520.0),
                },
            ],
            prices: Vec::new(),
            average_cost: Some(420.0),
        };
        let records = super::find(&Facts {
            days: &[quiet("2026-03-01", 100_000.0)],
            holdings: &[holding],
            currency: "EUR",
            ..Facts::default()
        });
        let bought = of_kind(&records, "A holding");
        assert_eq!(bought.len(), 1);
        assert_eq!(bought[0].date, day("2024-01-10"));
        assert_eq!(bought[0].title(), "S&P 500");
        assert_eq!(
            bought[0].sentence(false, "EUR"),
            "First bought on 10 January 2024, at $410.00 each."
        );
        assert_eq!(
            bought[0].sentence(true, "EUR"),
            "First bought on 10 January 2024.",
            "the price is hidden with the other amounts"
        );
        assert_eq!(bought[0].figure_text(false, "EUR").as_deref(), Some("80"));
    }

    #[test]
    fn a_holding_doubles_on_the_day_its_price_first_passed_twice_the_cost() {
        let holding = |prices: Vec<(NaiveDate, f64)>| Holding {
            id: "pos_btc".to_string(),
            name: "Bitcoin".to_string(),
            currency: Some("USD".to_string()),
            purchases: vec![Purchase {
                date: day("2025-01-01"),
                quantity: 1.0,
                price: Some(100.0),
            }],
            prices,
            average_cost: Some(100.0),
        };
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 101_000.0),
            quiet("2026-03-03", 102_000.0),
        ];
        let doubling = holding(vec![
            (day("2026-03-01"), 150.0),
            (day("2026-03-02"), 190.0),
            (day("2026-03-03"), 210.0),
        ]);
        let records = super::find(&Facts {
            days: &days,
            holdings: &[doubling],
            currency: "EUR",
            ..Facts::default()
        });
        let doubled = of_kind(&records, "A holding")
            .into_iter()
            .find(|record| matches!(record.kind, Kind::Doubled { .. }))
            .cloned()
            .unwrap();
        assert_eq!(doubled.date, day("2026-03-03"));
        assert_eq!(doubled.figure_text(false, "EUR").as_deref(), Some("2.1×"));
        assert_eq!(
            doubled.sentence(false, "EUR"),
            "More than twice what you paid for it, measured against the average purchase price."
        );

        // Already past twice the cost on the first day it was watched: no date to tell.
        let early = holding(vec![(day("2026-03-01"), 250.0), (day("2026-03-02"), 260.0)]);
        let records = super::find(&Facts {
            days: &days,
            holdings: &[early],
            currency: "EUR",
            ..Facts::default()
        });
        assert!(
            !of_kind(&records, "A holding")
                .iter()
                .any(|record| matches!(record.kind, Kind::Doubled { .. }))
        );
    }

    #[test]
    fn the_deepest_fall_remembers_the_high_and_the_day_it_came_back() {
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 104_000.0),
            quiet("2026-03-03", 96_000.0),
            quiet("2026-03-04", 95_000.0),
            quiet("2026-03-05", 101_000.0),
            quiet("2026-03-06", 104_500.0),
        ];
        let records = find(&days);
        let fall = of_kind(&records, "A fall");
        assert_eq!(fall.len(), 1, "{:?}", ids(&records));
        assert_eq!(fall[0].date, day("2026-03-04"));
        assert_eq!(
            fall[0].sentence(false, "EUR"),
            "The low point so far: down €9,000 from the high, on 4 March. Recovered on 6 March."
        );
        assert_eq!(
            fall[0].sentence(true, "EUR"),
            "The deepest fall on record, on 4 March. Recovered on 6 March."
        );
        assert_eq!(
            fall[0].figure_text(true, "EUR").as_deref(),
            Some(format::HIDDEN)
        );
    }

    #[test]
    fn a_fall_not_yet_recovered_says_so_plainly() {
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 90_000.0),
            quiet("2026-03-03", 92_000.0),
        ];
        let records = find(&days);
        let fall = &of_kind(&records, "A fall")[0];
        assert_eq!(
            fall.sentence(false, "EUR"),
            "The low point so far: down €10,000 from the high, on 2 March. Not yet recovered."
        );
    }

    #[test]
    fn a_history_that_only_ever_falls_still_says_what_is_true() {
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 98_000.0),
            quiet("2026-03-03", 95_000.0),
            quiet("2026-03-04", 94_000.0),
        ];
        let records = find(&days);
        assert!(of_kind(&records, "A high").is_empty());
        assert!(of_kind(&records, "A milestone").is_empty());
        assert!(
            of_kind(&records, "A day")
                .iter()
                .all(|record| record.title() == "The worst day"),
            "a day that lost money is not called the best day"
        );
        let worst = of_kind(&records, "A day")
            .into_iter()
            .find(|record| record.title() == "The worst day")
            .cloned()
            .unwrap();
        assert_eq!(worst.date, day("2026-03-03"), "the steepest of the falls");
        assert_eq!(
            worst.sentence(false, "EUR"),
            "The largest single-day fall on record."
        );
        let fall = &of_kind(&records, "A fall")[0];
        assert_eq!(fall.date, day("2026-03-04"));
    }

    #[test]
    fn the_same_history_always_gives_the_same_list_and_no_record_twice() {
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 101_000.0),
            quiet("2026-03-03", 99_000.0),
            quiet("2026-03-04", 103_000.0),
            quiet("2026-03-05", 108_000.0),
        ];
        let once = find(&days);
        let twice = find(&days);
        assert_eq!(once, twice);
        let mut seen = std::collections::HashSet::new();
        for record in &once {
            assert!(seen.insert(record.id.clone()), "{} twice", record.id);
        }
        assert!(once.windows(2).all(|pair| pair[0].date >= pair[1].date));
    }

    #[test]
    fn a_history_out_of_order_reads_as_if_it_were_in_order() {
        let days = [
            quiet("2026-03-03", 101_000.0),
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 100_500.0),
        ];
        let records = find(&days);
        assert_eq!(records[0].date, day("2026-03-03"));
        assert_eq!(
            records[records.len() - 1].id,
            "2026-03-01:begins",
            "the start is still the oldest"
        );
    }

    #[test]
    fn privacy_hides_every_amount_but_keeps_dates_titles_and_percentages() {
        let days = [
            quiet("2026-03-01", 100_000.0),
            quiet("2026-03-02", 104_000.0),
            quiet("2026-03-03", 95_000.0),
            quiet("2026-03-04", 96_000.0),
            quiet("2026-03-05", 105_000.0),
        ];
        let records = find(&days);
        assert!(records.len() >= 5);
        for record in &records {
            let said = record.sentence(true, "EUR");
            assert!(!said.contains('€'), "{}: {said}", record.id);
            assert!(!said.contains('$'), "{}: {said}", record.id);
            if let Some(figure) = record.figure_text(true, "EUR") {
                assert!(
                    figure == format::HIDDEN || !figure.contains('€'),
                    "{}: {figure}",
                    record.id
                );
            }
            assert!(!record.title().contains('€'), "{}", record.id);
        }
        // Percentages and dates stay.
        let worst = of_kind(&records, "A day")
            .into_iter()
            .find(|record| record.title() == "The worst day")
            .cloned()
            .unwrap();
        assert!(worst.percent.is_some());
    }

    #[test]
    fn a_quiet_day_has_no_plate_but_a_flowing_one_has_no_move() {
        let mut days = vec![quiet("2026-03-01", 100_000.0)];
        days.push(Day {
            date: day("2026-03-02"),
            total: 120_000.0,
            classes: Vec::new(),
            flow: 20_000.0,
        });
        let records = find(&days);
        assert!(of_kind(&records, "A day").is_empty());
        assert_eq!(of_kind(&records, "A high").len(), 1, "a value is a value");
    }

    #[test]
    fn amounts_drop_cents_only_when_they_do_not_matter() {
        assert_eq!(whole(8_412.0, "EUR"), "€8,412");
        assert_eq!(whole(12.5, "EUR"), "€12.50");
        assert_eq!(units(80.0), "80");
        assert_eq!(units(0.45), "0.45");
        assert_eq!(units(0.0), "0");
        assert_eq!(years_in_words(1), "One year");
        assert_eq!(years_in_words(3), "Three years");
        assert_eq!(years_in_words(12), "12 years");
    }
}
