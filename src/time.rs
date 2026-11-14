//! Turning time back: the portfolio as it stood on any day Stahl recorded.
//!
//! The history file keeps, for every recorded day, each holding's quantity, price and value. From
//! that alone the app can show that day's total, allocation and holdings, and say what happened.
//! Nothing here is interpolated: a day without a valuation is not a day the owner can visit, and
//! what a snapshot did not record (cost, profit and loss) is left unknown rather than guessed.
//!
//! While the owner is in the past, the app's own figures (`summary`, `positions`, `allocation`)
//! are that day's, and today's wait in [`Present`] until they come back. Every page that reads
//! those figures follows without knowing it; the pages that only make sense for now say so.

use chrono::{Datelike, Local, NaiveDate};
use portfolio_rs::{
    mcp::snapshots::{self, Snapshot as HistoryPoint},
    position::parse_purchase_date,
    state::{AllocationItemDto, PortfolioSummaryDto, PositionDto},
};
use rune::{Live, LiveText};

use crate::{
    format,
    state::{Page, State},
    statement::{Figure, Statement, Tone},
};

/// How wide the toolbar's time strip is, in points: its playhead's live offset is measured in it.
pub const STRIP: f32 = 240.0;

/// One holding on a recorded day, as the snapshot kept it.
#[derive(Clone, Debug, PartialEq)]
pub struct Held {
    pub id: String,
    pub class: String,
    pub cash: bool,
    pub quantity: f64,
    /// In the quote currency, when the snapshot recorded one.
    pub price: Option<f64>,
    /// In the base currency.
    pub value: f64,
}

/// A recorded day: what the portfolio was worth, and what it held.
#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub total: f64,
    pub holdings: Vec<Held>,
}

/// Recorded days from stored valuations, oldest first, one per date. A valuation with any holding
/// unvalued is not a day: its total would read as a fall.
pub fn days_from(points: &[HistoryPoint]) -> Vec<Day> {
    let mut days: Vec<Day> = points
        .iter()
        .filter(|point| point.complete && point.total_base.is_finite())
        .filter_map(|point| {
            let date = snapshots::strict_date(&point.date)?;
            let holdings = point
                .positions
                .iter()
                .map(|position| {
                    let value = position.value_base.filter(|value| value.is_finite())?;
                    Some(Held {
                        id: position.position_id.clone(),
                        class: position.asset_class.clone(),
                        cash: position.kind == "cash",
                        quantity: position.quantity,
                        price: position.price.filter(|price| price.is_finite()),
                        value,
                    })
                })
                .collect::<Option<Vec<Held>>>()?;
            Some(Day {
                date,
                total: point.total_base,
                holdings,
            })
        })
        .collect();
    days.sort_by_key(|day| day.date);
    days.dedup_by_key(|day| day.date);
    days
}

/// Today's figures, kept aside while the app shows a day in the past.
#[derive(Clone, Debug)]
pub struct Present {
    pub summary: Option<PortfolioSummaryDto>,
    pub positions: Vec<PositionDto>,
    pub allocation: Vec<AllocationItemDto>,
}

/// Where the owner is in time, and the controls that move them, in live values so a drag moves the
/// playhead without rebuilding the view.
pub struct Time {
    /// Every recorded day before today, oldest first.
    pub days: Vec<Day>,
    /// The day shown, as an index into `days`. `None` is today.
    pub at: Option<usize>,
    /// Today's figures while the app shows the past.
    pub present: Option<Present>,
    /// A drag on a strip, in progress: where the playhead is held.
    pub dragging: bool,
    /// The strip's playhead, from 0 (the first recorded day) to 1 (today), as drawn.
    pub head: Live<[f32; 2]>,
    /// The date under the strip's playhead while it is dragged.
    pub reading: LiveText,
    /// The door's turn not yet spent on a whole day, in radians.
    pub dial_rest: f32,
}

impl Default for Time {
    fn default() -> Self {
        Time {
            days: Vec::new(),
            at: None,
            present: None,
            dragging: false,
            head: Live::new([0.0, 0.0]),
            reading: LiveText::new(""),
            dial_rest: 0.0,
        }
    }
}

/// How the app names the move it shows beside each figure: today's, a past day's, or the move
/// since an earlier valuation when the day before was not recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Today,
    /// A recorded day whose previous recorded day was the calendar day before.
    ThatDay,
    /// A recorded day whose previous valuation is older.
    Since(NaiveDate),
    /// The first recorded day: there is nothing before it to move from.
    First,
}

impl When {
    /// The word after a figure: "today", "that day", "since Oct 1".
    pub fn word(self) -> String {
        match self {
            When::Today => "today".to_string(),
            When::ThatDay => "that day".to_string(),
            When::Since(date) => format!("since {}", short(date)),
            When::First => "on the first day".to_string(),
        }
    }

    /// The share of the move a holding made: "Of today", "Of that day", "Of the move".
    pub fn of(self) -> String {
        match self {
            When::Today => "Of today".to_string(),
            When::ThatDay => "Of that day".to_string(),
            _ => "Of the move".to_string(),
        }
    }

    /// The same as a label: "Today", "That day", "Since Oct 1".
    pub fn title(self) -> String {
        let word = self.word();
        let mut chars = word.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().collect::<String>() + chars.as_str()
        })
    }
}

/// `Oct 3`.
pub fn short(date: NaiveDate) -> String {
    date.format("%b %-d").to_string()
}

/// `Thursday, October 3`, with the year when it is not this one.
pub fn long(date: NaiveDate, today: NaiveDate) -> String {
    if date.year() == today.year() {
        date.format("%A, %B %-d").to_string()
    } else {
        date.format("%A, %B %-d, %Y").to_string()
    }
}

/// The move from `before` to `now` in a holding's value per unit, in the base currency, so a
/// change of exchange rate counts as it did for the owner. Cash does not move; a holding whose
/// quantity was zero on either day has no move to speak of.
fn unit_move(now: &Held, before: Option<&Held>) -> Option<f64> {
    if now.cash {
        return None;
    }
    let before = before?;
    if now.quantity <= 0.0 || before.quantity <= 0.0 || before.value <= 0.0 {
        return None;
    }
    let unit_now = now.value / now.quantity;
    let unit_before = before.value / before.quantity;
    let percent = (unit_now / unit_before - 1.0) * 100.0;
    percent.is_finite().then_some(percent)
}

/// A holding's move on the day in money, from its value and its percentage move.
fn amount_of(value: f64, percent: Option<f64>) -> Option<f64> {
    let percent = percent?;
    if percent <= -100.0 {
        return None;
    }
    let factor = 1.0 + percent / 100.0;
    Some(value * (percent / 100.0) / factor)
}

/// The holdings on `day`, named as they are today. A holding sold since keeps its class; nothing
/// it did not record (cost, profit) is guessed.
pub fn positions_on(day: &Day, before: Option<&Day>, today: &[PositionDto]) -> Vec<PositionDto> {
    let mut sold = 0;
    let mut positions: Vec<PositionDto> = day
        .holdings
        .iter()
        .map(|held| {
            let previous =
                before.and_then(|before| before.holdings.iter().find(|h| h.id == held.id));
            let now = today
                .iter()
                .find(|position| position.position_id.as_deref() == Some(held.id.as_str()));
            let (id, name, ticker, purchases) = match now {
                Some(position) => (
                    position.id,
                    position.name.clone(),
                    position.ticker.clone(),
                    position
                        .purchases
                        .iter()
                        .filter(|purchase| {
                            purchase
                                .date
                                .as_deref()
                                .and_then(parse_purchase_date)
                                .is_none_or(|date| date.date_naive() <= day.date)
                        })
                        .cloned()
                        .collect(),
                ),
                None => {
                    sold += 1;
                    (
                        today.len() + sold,
                        Some(format!("{} holding, since sold", held.class)),
                        None,
                        Vec::new(),
                    )
                }
            };
            PositionDto {
                id,
                position_id: Some(held.id.clone()),
                name,
                ticker,
                asset_class: held.class.clone(),
                amount: held.quantity,
                price: held.price.unwrap_or(if held.quantity > 0.0 {
                    held.value / held.quantity
                } else {
                    0.0
                }),
                value: held.value,
                pnl: None,
                pnl_percent: None,
                day_change_percent: unit_move(held, previous),
                average_cost: None,
                invested: None,
                purchases,
            }
        })
        .collect();
    positions.sort_by_key(|position| position.id);
    positions
}

/// What each class held on the day, largest first.
pub fn allocation_on(day: &Day) -> Vec<AllocationItemDto> {
    let mut classes: Vec<AllocationItemDto> = Vec::new();
    for held in &day.holdings {
        match classes
            .iter_mut()
            .find(|item| item.asset_class == held.class)
        {
            Some(item) => item.value += held.value,
            None => classes.push(AllocationItemDto {
                asset_class: held.class.clone(),
                percent: 0.0,
                value: held.value,
            }),
        }
    }
    let total: f64 = classes.iter().map(|item| item.value).sum();
    for item in &mut classes {
        item.percent = if total > 0.0 {
            item.value / total * 100.0
        } else {
            0.0
        };
    }
    classes.sort_by(|a, b| b.value.total_cmp(&a.value));
    classes
}

/// The day's summary. Profit and cost were not recorded, so they are zero here and the pages
/// that show them say so instead of reading them.
pub fn summary_on(day: &Day, positions: &[PositionDto]) -> PortfolioSummaryDto {
    let day_change: f64 = positions
        .iter()
        .filter_map(|position| amount_of(position.value, position.day_change_percent))
        .sum();
    let previous = day.total - day_change;
    let cash_value: f64 = day
        .holdings
        .iter()
        .filter(|held| held.cash)
        .map(|held| held.value)
        .sum();
    PortfolioSummaryDto {
        total_value: day.total,
        total_invested: 0.0,
        total_pnl: 0.0,
        pnl_percent: 0.0,
        day_change,
        day_change_percent: if previous > 0.0 {
            day_change / previous * 100.0
        } else {
            0.0
        },
        cash_value,
        securities_value: day.total - cash_value,
        allocation: allocation_on(day)
            .into_iter()
            .map(|item| (item.asset_class, item.percent))
            .collect(),
        positions: positions.to_vec(),
        has_stale: false,
        complete: true,
        data_status: "complete".to_string(),
    }
}

/// How a past day's moves are named: against the calendar day before, an older valuation, or
/// nothing at all on the first.
pub fn when_on(days: &[Day], index: usize) -> When {
    match index.checked_sub(1).and_then(|before| days.get(before)) {
        None => When::First,
        Some(before) if (days[index].date - before.date).num_days() == 1 => When::ThatDay,
        Some(before) => When::Since(before.date),
    }
}

/// What the Overview says about a day in the past: what was held, how the day went, where it sits
/// among the recorded days, and how far today is from it. Plain on a bad day.
pub struct PastFacts<'a> {
    pub days: &'a [Day],
    pub index: usize,
    pub positions: &'a [PositionDto],
    pub today_total: Option<f64>,
    pub today: NaiveDate,
    pub currency: &'a str,
    pub hidden: bool,
}

/// Under this share of the portfolio, a move is "barely" one.
const BARELY: f64 = 0.0005;

pub fn statement_on(facts: &PastFacts) -> Statement {
    let day = &facts.days[facts.index];
    let total = day.total;
    let when = when_on(facts.days, facts.index);
    let money = |value: f64| {
        let value = value.abs();
        if value < 100.0 {
            format::currency(value, facts.currency)
        } else {
            format::currency(value.round(), facts.currency)
        }
        .trim_end_matches(".00")
        .to_string()
    };
    let mut sentences = Vec::new();
    let mut figures = Vec::new();
    let date = long(day.date, facts.today);

    sentences.push(if facts.hidden {
        format!("This is {date}, as Stahl recorded it.")
    } else {
        format!("On {date}, you held {}.", money(total))
    });

    // The day's own move, and what carried it.
    let moved: f64 = facts
        .positions
        .iter()
        .filter_map(|position| amount_of(position.value, position.day_change_percent))
        .sum();
    let leader = facts
        .positions
        .iter()
        .filter_map(|position| {
            let amount = amount_of(position.value, position.day_change_percent)?;
            (amount * moved > 0.0).then_some((position, amount))
        })
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()));
    let previous = total - moved;
    let percent = if previous > 0.0 {
        moved / previous * 100.0
    } else {
        0.0
    };
    if when != When::First {
        let span = match when {
            When::Since(date) => format!("Since the valuation before, on {}, it", short(date)),
            _ => "That day it".to_string(),
        };
        let size = if facts.hidden {
            format!("{:.1}%", percent.abs())
        } else {
            format!("{} ({:.1}%)", money(moved), percent.abs())
        };
        let carried = leader.as_ref().map(|(position, amount)| {
            let name = position
                .name
                .clone()
                .or_else(|| position.ticker.clone())
                .unwrap_or_else(|| "one holding".to_string());
            if amount.abs() >= moved.abs() * 0.9 {
                format!(", nearly all of it {name}")
            } else if amount.abs() >= moved.abs() * 0.5 {
                format!(", most of it {name}")
            } else {
                format!(", led by {name}")
            }
        });
        sentences.push(if previous <= 0.0 || (moved / previous).abs() < BARELY {
            format!("{span} barely moved.")
        } else if moved > 0.0 {
            format!("{span} rose {size}{}.", carried.unwrap_or_default())
        } else {
            format!("{span} fell {size}{}.", carried.unwrap_or_default())
        });
        figures.push(Figure {
            label: when.title(),
            value: if facts.hidden {
                format::HIDDEN.to_string()
            } else {
                format::signed_currency(moved, facts.currency)
            },
            tone: tone(moved),
            note: Some(format::percent(percent)),
        });
    }

    // Where the day sits among those recorded.
    let lowest = facts.days.iter().all(|other| other.total >= total);
    let highest_then = facts.days[..facts.index]
        .iter()
        .all(|other| other.total < total)
        && facts.index >= 7;
    if facts.days.len() >= 7 && lowest {
        sentences.push("It is the lowest value Stahl has recorded.".to_string());
    } else if highest_then {
        sentences.push("It was the highest value Stahl had recorded until then.".to_string());
    }

    // And how far today is from it.
    if let Some(now) = facts.today_total {
        let since = now - total;
        let percent = if total > 0.0 {
            since / total * 100.0
        } else {
            0.0
        };
        sentences.push(if total <= 0.0 || (since / total).abs() < BARELY {
            "Today it stands about where it was.".to_string()
        } else if facts.hidden {
            format!(
                "Today it stands {:.1}% {}.",
                percent.abs(),
                if since > 0.0 { "higher" } else { "lower" }
            )
        } else {
            format!(
                "Today it stands {} {}.",
                money(since),
                if since > 0.0 { "higher" } else { "lower" }
            )
        });
        figures.push(Figure {
            label: "Since then".to_string(),
            value: if facts.hidden {
                format::HIDDEN.to_string()
            } else {
                format::signed_currency(since, facts.currency)
            },
            tone: tone(since),
            note: Some(format::percent(percent)),
        });
    }
    if let Some((position, amount)) = leader.filter(|_| when != When::First) {
        figures.push(Figure {
            label: "Moved it most".to_string(),
            value: position
                .name
                .clone()
                .or_else(|| position.ticker.clone())
                .unwrap_or_default(),
            tone: Tone::Plain,
            note: Some(if facts.hidden {
                format::HIDDEN.to_string()
            } else {
                format::signed_currency(amount, facts.currency)
            }),
        });
    }
    Statement {
        text: sentences.join(" "),
        figures,
    }
}

fn tone(value: f64) -> Tone {
    if value >= 0.005 {
        Tone::Up
    } else if value <= -0.005 {
        Tone::Down
    } else {
        Tone::Plain
    }
}

/// Pages that speak about now, and what each says in the past instead of showing stale figures.
pub fn now_only(page: Page) -> Option<&'static str> {
    match page {
        Page::Ask => Some(
            "Counsel answers about today. Return to today to ask, or keep looking around this day.",
        ),
        Page::Review => Some(
            "Review measures today's portfolio against your policy. A past day has nothing left \
             to decide.",
        ),
        Page::Rebalance => Some(
            "A rebalance plan is made from today's prices and today's holdings. It would be \
             nothing but history on a past day.",
        ),
        Page::Documents => Some("Documents are written from today's portfolio."),
        Page::Briefing => Some("The briefing is about today's portfolio."),
        Page::Performance => Some(
            "Performance measures periods that end today. Turn back a day at a time to see what \
             each recorded day held.",
        ),
        _ => None,
    }
}

impl State {
    /// Whether the app is showing a day in the past.
    pub fn in_past(&self) -> bool {
        self.time.at.is_some()
    }

    /// The day shown, when it is in the past.
    pub fn past_day(&self) -> Option<&Day> {
        self.time.at.and_then(|at| self.time.days.get(at))
    }

    /// How moves on screen are named.
    pub fn when(&self) -> When {
        match self.time.at {
            Some(at) => when_on(&self.time.days, at),
            None => When::Today,
        }
    }

    /// Today's total, wherever the app is in time.
    pub fn today_total(&self) -> Option<f64> {
        match &self.time.present {
            Some(present) => present.summary.as_ref().map(|summary| summary.total_value),
            None => self.summary.as_ref().map(|summary| summary.total_value),
        }
    }

    /// Today's positions, wherever the app is in time.
    pub fn today_positions(&self) -> &[PositionDto] {
        match &self.time.present {
            Some(present) => &present.positions,
            None => &self.positions,
        }
    }

    /// Read the recorded days again, and stay on the same date if it is still there.
    pub fn load_days(&mut self) {
        if !self.has_portfolio() {
            return;
        }
        self.run(
            |backend| async move { backend.days().await },
            |state, result| {
                let Ok(days) = result else {
                    return;
                };
                let date = state.past_day().map(|day| day.date);
                state.time.days = days;
                match date.and_then(|date| state.time.days.iter().position(|d| d.date == date)) {
                    Some(index) => state.travel(Some(index)),
                    None if date.is_some() => state.travel(None),
                    None => state.place_head(),
                }
            },
        );
    }

    /// Show the portfolio as it stood on recorded day `index`, or today with `None`. The whole app
    /// follows: its figures are swapped for that day's, and today's are kept until it comes back.
    pub fn travel(&mut self, index: Option<usize>) {
        let index = index.filter(|index| *index < self.time.days.len());
        if index.is_none() {
            if let Some(present) = self.time.present.take() {
                self.summary = present.summary;
                self.positions = present.positions;
                self.allocation = present.allocation;
            }
            self.time.at = None;
            self.place_head();
            return;
        }
        let index = index.expect("checked above");
        if self.time.present.is_none() {
            self.time.present = Some(Present {
                summary: self.summary.clone(),
                positions: self.positions.clone(),
                allocation: self.allocation.clone(),
            });
            // Nothing is edited in the past.
            self.form = None;
            self.confirm = None;
        }
        let today = self
            .time
            .present
            .as_ref()
            .map(|present| present.positions.clone())
            .unwrap_or_default();
        let day = &self.time.days[index];
        let before = index.checked_sub(1).map(|before| &self.time.days[before]);
        let positions = positions_on(day, before, &today);
        self.summary = Some(summary_on(day, &positions));
        self.allocation = allocation_on(day);
        self.positions = positions;
        if self
            .selected
            .is_some_and(|id| !self.positions.iter().any(|position| position.id == id))
        {
            self.selected = None;
        }
        self.time.at = Some(index);
        self.place_head();
    }

    /// Move `steps` recorded days through time: negative is back. Stops at the first recorded
    /// day, and at today going forward.
    pub fn step_time(&mut self, steps: i64) {
        let count = self.time.days.len() as i64;
        if count == 0 || steps == 0 {
            return;
        }
        let now = self.time.at.map_or(count, |at| at as i64);
        let next = (now + steps).clamp(0, count);
        self.travel((next < count).then_some(next as usize));
    }

    /// Back to today, from wherever the app is in time. Returns whether it was elsewhere.
    pub fn return_to_today(&mut self) -> bool {
        if !self.in_past() {
            return false;
        }
        self.travel(None);
        true
    }

    /// Called first when today's figures arrive: they belong in the stash while the app is in the
    /// past. Returns the day to go back to once they are in place.
    pub fn leave_past_for_news(&mut self) -> Option<usize> {
        let at = self.time.at;
        if at.is_some() {
            self.travel(None);
        }
        at
    }

    /// The strip's playhead to where the app is in time, from 0 at the first recorded day to 1 at
    /// today.
    pub fn place_head(&self) {
        self.time.head.set([self.head_fraction() * STRIP, 0.0]);
    }

    /// Where the app is in time, along the strip.
    pub fn head_fraction(&self) -> f32 {
        let count = self.time.days.len();
        match self.time.at {
            Some(at) if count > 0 => at as f32 / count as f32,
            _ => 1.0,
        }
    }

    /// The statement in the past: `None` today, so the usual one speaks.
    pub fn past_statement(&self) -> Option<Statement> {
        let index = self.time.at?;
        let currency = self.currency();
        Some(statement_on(&PastFacts {
            days: &self.time.days,
            index,
            positions: &self.positions,
            today_total: self.today_total(),
            today: Local::now().date_naive(),
            currency: &currency,
            hidden: self.privacy,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(id: &str, class: &str, quantity: f64, price: Option<f64>, value: f64) -> Held {
        Held {
            id: id.to_string(),
            class: class.to_string(),
            cash: class == "Cash",
            quantity,
            price,
            value,
        }
    }

    fn day(date: &str, holdings: Vec<Held>) -> Day {
        Day {
            date: NaiveDate::parse_from_str(date, "%Y-%m-%d").unwrap(),
            total: holdings.iter().map(|held| held.value).sum(),
            holdings,
        }
    }

    fn position(id: usize, position_id: &str, name: &str) -> PositionDto {
        PositionDto {
            id,
            position_id: Some(position_id.to_string()),
            name: Some(name.to_string()),
            ticker: None,
            asset_class: "Stocks".to_string(),
            amount: 1.0,
            price: 1.0,
            value: 1.0,
            pnl: Some(1.0),
            pnl_percent: Some(1.0),
            day_change_percent: Some(1.0),
            average_cost: Some(1.0),
            invested: Some(1.0),
            purchases: Vec::new(),
        }
    }

    fn two_days() -> Vec<Day> {
        vec![
            day(
                "2026-10-02",
                vec![
                    held("a", "Stocks", 10.0, Some(100.0), 1000.0),
                    held("c", "Cash", 500.0, None, 500.0),
                ],
            ),
            day(
                "2026-10-03",
                vec![
                    held("a", "Stocks", 10.0, Some(90.0), 900.0),
                    held("c", "Cash", 500.0, None, 500.0),
                    held("gone", "Gold", 1.0, Some(100.0), 100.0),
                ],
            ),
        ]
    }

    #[test]
    fn a_past_day_shows_what_was_recorded_and_nothing_else() {
        let days = two_days();
        let today = vec![position(0, "c", "Savings"), position(1, "a", "Index")];
        let positions = positions_on(&days[1], Some(&days[0]), &today);
        assert_eq!(positions.len(), 3);
        let index = &positions[1];
        assert_eq!(index.name.as_deref(), Some("Index"));
        assert_eq!(index.id, 1, "today's id, so the details and quotes line up");
        assert_eq!(index.value, 900.0);
        assert_eq!(index.price, 90.0);
        assert!((index.day_change_percent.unwrap() + 10.0).abs() < 1e-9);
        // What the snapshot did not keep is unknown, not today's.
        assert_eq!(index.pnl, None);
        assert_eq!(index.invested, None);
        // Cash does not move, and a holding sold since keeps its class.
        assert_eq!(positions[0].day_change_percent, None);
        assert_eq!(
            positions[2].name.as_deref(),
            Some("Gold holding, since sold")
        );
        assert_eq!(positions[2].day_change_percent, None, "nothing before it");
    }

    #[test]
    fn a_past_summary_adds_up_and_moves_by_the_market_only() {
        let days = two_days();
        let positions = positions_on(&days[1], Some(&days[0]), &[]);
        let summary = summary_on(&days[1], &positions);
        assert_eq!(summary.total_value, 1500.0);
        assert_eq!(summary.cash_value, 500.0);
        assert_eq!(summary.securities_value, 1000.0);
        // The stock fell 100; the gold arriving is not a market move.
        assert!((summary.day_change + 100.0).abs() < 1e-9);
        let allocation = allocation_on(&days[1]);
        assert_eq!(allocation[0].asset_class, "Stocks");
        let shares: f64 = allocation.iter().map(|item| item.percent).sum();
        assert!((shares - 100.0).abs() < 1e-9);
    }

    #[test]
    fn a_gap_in_the_record_is_named_not_filled() {
        let mut days = two_days();
        assert_eq!(when_on(&days, 0), When::First);
        assert_eq!(when_on(&days, 1), When::ThatDay);
        days[1].date = NaiveDate::from_ymd_opt(2026, 10, 6).unwrap();
        assert_eq!(
            when_on(&days, 1),
            When::Since(NaiveDate::from_ymd_opt(2026, 10, 2).unwrap())
        );
        assert_eq!(when_on(&days, 1).title(), "Since Oct 2");
    }

    #[test]
    fn the_past_speaks_plainly_and_hides_amounts_when_asked() {
        let days = two_days();
        let today_positions = vec![position(0, "c", "Savings"), position(1, "a", "Index")];
        let positions = positions_on(&days[1], Some(&days[0]), &today_positions);
        let facts = PastFacts {
            days: &days,
            index: 1,
            positions: &positions,
            today_total: Some(1650.0),
            today: NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(),
            currency: "EUR",
            hidden: false,
        };
        let statement = statement_on(&facts);
        assert!(
            statement
                .text
                .starts_with("On Saturday, October 3, you held €1,500."),
            "{}",
            statement.text
        );
        assert!(
            statement.text.contains("That day it fell €100"),
            "{}",
            statement.text
        );
        assert!(
            statement.text.contains("nearly all of it Index"),
            "{}",
            statement.text
        );
        assert!(
            statement.text.contains("Today it stands €150 higher."),
            "{}",
            statement.text
        );
        assert!(!statement.text.contains('!'));
        let hidden = statement_on(&PastFacts {
            hidden: true,
            ..facts
        });
        assert!(!hidden.text.contains('€'), "{}", hidden.text);
        assert!(
            hidden
                .figures
                .iter()
                .all(|figure| !figure.value.contains('€'))
        );
    }

    #[test]
    fn incomplete_valuations_are_not_days() {
        let line = |date: &str, complete: bool| -> HistoryPoint {
            serde_json::from_str(&format!(
                r#"{{"date":"{date}","baseCurrency":"EUR","positions":[{{"positionId":"a","kind":"market","assetClass":"Stocks","quantity":1.0,"price":5.0,"valueBase":5.0}}],"cashBase":0.0,"totalBase":5.0,"complete":{complete}}}"#
            ))
            .unwrap()
        };
        let days = days_from(&[
            line("2026-10-03", true),
            line("2026-10-02", true),
            line("2026-10-04", false),
            line("2026-10-03", true),
        ]);
        let dates: Vec<String> = days.iter().map(|day| day.date.to_string()).collect();
        assert_eq!(dates, ["2026-10-02", "2026-10-03"]);
    }
}
