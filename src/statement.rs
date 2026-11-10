//! What changed since the owner last looked, in a sentence or two.
//!
//! Everything here is pure: the sentences are composed from data, the same data always gives the
//! same words, and when there is nothing worth saying it says less. The backend gathers the facts
//! ([`Change`], from the stored daily valuations) and remembers the days the owner looked
//! ([`Seen`]); the Overview shows what [`compose`] makes of them.

use chrono::{Datelike as _, NaiveDate};
use portfolio_rs::state::PositionDto;

use crate::format;

/// The days the owner looked at a portfolio: the latest, and the one before it. Only dates, so an
/// encrypted portfolio's record says nothing about its values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Seen {
    pub last: NaiveDate,
    pub before: Option<NaiveDate>,
}

impl Seen {
    /// The record after a look on `today`. Looking twice in a day keeps the day before it.
    pub fn after_visit(previous: Option<Seen>, today: NaiveDate) -> Seen {
        match previous {
            Some(seen) if seen.last >= today => seen,
            Some(seen) => Seen {
                last: today,
                before: Some(seen.last),
            },
            None => Seen {
                last: today,
                before: None,
            },
        }
    }

    /// When the owner last looked before `today`, given the record as it stood before this look.
    pub fn since(previous: Option<Seen>, today: NaiveDate) -> Since {
        match previous {
            None => Since::First,
            Some(seen) if seen.last < today => Since::Day(seen.last),
            Some(Seen {
                before: Some(day), ..
            }) if day < today => Since::Day(day),
            Some(_) => Since::EarlierToday,
        }
    }
}

/// What the Statement measures from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Since {
    /// Nobody has looked at this portfolio in Stahl before.
    First,
    /// The only earlier look was today.
    EarlierToday,
    /// The last look on an earlier day.
    Day(NaiveDate),
}

/// A holding's share of a move.
#[derive(Clone, Debug, PartialEq)]
pub struct Contribution {
    pub name: String,
    pub amount: f64,
}

/// The portfolio between a stored valuation and now, from `portfolio_rs`'s daily history.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    /// The date of the valuation compared with.
    pub from: NaiveDate,
    pub start: f64,
    pub end: f64,
    /// Money put in, or taken out when negative. The rest of the change is the market.
    pub added: f64,
    /// The holding whose price moved the total most, in the direction the market went.
    pub leader: Option<Contribution>,
    /// The highest stored valuation before today, and the first day of the history.
    pub previous_high: Option<f64>,
    pub history_from: Option<NaiveDate>,
}

impl Change {
    pub fn total(&self) -> f64 {
        self.end - self.start
    }

    pub fn market(&self) -> f64 {
        self.total() - self.added
    }
}

/// Today's move, from the previous close.
#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    pub amount: f64,
    pub percent: f64,
    pub leader: Option<Contribution>,
}

/// Everything the Statement may speak about.
#[derive(Clone, Debug)]
pub struct Facts<'a> {
    pub today: NaiveDate,
    pub since: Since,
    pub currency: &'a str,
    /// Privacy mode: no amounts, only directions, percentages and names.
    pub hidden: bool,
    /// Every holding has a price, so today's total is the whole portfolio.
    pub complete: bool,
    /// An encrypted portfolio keeps no history.
    pub encrypted: bool,
    /// Measured from the valuation on the day in `since` (or, the first time, from the oldest).
    pub change: Option<Change>,
    pub today_move: Option<Move>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Up,
    Down,
    Plain,
}

impl Tone {
    fn of(value: f64) -> Tone {
        if value >= 0.005 {
            Tone::Up
        } else if value <= -0.005 {
            Tone::Down
        } else {
            Tone::Plain
        }
    }
}

/// One supporting figure under the sentences.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    pub label: String,
    pub value: String,
    pub tone: Tone,
    /// A line under the value, such as the amount beside a holding's name.
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Statement {
    pub text: String,
    pub figures: Vec<Figure>,
}

/// Under this, money in or out is rounding, not a deposit.
const NO_MONEY: f64 = 0.5;
/// A move smaller than this share of the portfolio has "barely moved".
const BARELY: f64 = 0.0005;
/// A history shorter than this makes "the highest value recorded" hollow.
const RECORD_AFTER_DAYS: i64 = 30;

/// The day as a sentence names it, seen from `today`: "yesterday", "Tuesday", "October 14",
/// "October 14, 2025".
pub fn day_name(day: NaiveDate, today: NaiveDate) -> String {
    let days = (today - day).num_days();
    if days == 1 {
        "yesterday".to_string()
    } else if (2..=6).contains(&days) {
        day.format("%A").to_string()
    } else if day.year() == today.year() {
        day.format("%B %-d").to_string()
    } else {
        day.format("%B %-d, %Y").to_string()
    }
}

/// What `positions` moved today, with the holding that moved the total most.
pub fn today_move(day_change: f64, day_change_percent: f64, positions: &[PositionDto]) -> Move {
    let leader = positions
        .iter()
        .filter_map(|position| {
            let percent = position.day_change_percent?;
            // value = previous × (1 + p), so the move is value × p / (1 + p).
            let amount = position.value * percent / (100.0 + percent);
            let name = position.name.clone().or_else(|| position.ticker.clone())?;
            amount.is_finite().then_some(Contribution { name, amount })
        })
        .filter(|contribution| contribution.amount * day_change > 0.0)
        .max_by(|a, b| a.amount.abs().total_cmp(&b.amount.abs()));
    Move {
        amount: day_change,
        percent: day_change_percent,
        leader,
    }
}

struct Words<'a> {
    facts: &'a Facts<'a>,
}

impl Words<'_> {
    /// An amount in prose, unsigned: the words carry the direction, and cents only where they
    /// matter. `None` in privacy mode.
    fn money(&self, value: f64) -> Option<String> {
        if self.facts.hidden {
            return None;
        }
        let value = value.abs();
        if value < 100.0 {
            return Some(format::currency(value, self.facts.currency));
        }
        let whole = format::currency(value.round(), self.facts.currency);
        Some(whole.strip_suffix(".00").unwrap_or(&whole).to_string())
    }

    fn signed(&self, value: f64) -> String {
        if self.facts.hidden {
            format::HIDDEN.to_string()
        } else {
            format::signed_currency(value, self.facts.currency)
        }
    }

    /// "€1,240.00 (1.2%)", or "1.2%" with the amounts hidden.
    fn amount_and_share(&self, value: f64, of: f64) -> String {
        let share = percent_of(value, of);
        match self.money(value) {
            Some(money) => match share {
                Some(share) => format!("{money} ({share})"),
                None => money,
            },
            None => share.unwrap_or_else(|| "some".to_string()),
        }
    }
}

/// "1.2%", unsigned, or `None` when there's no base to measure against.
fn percent_of(value: f64, of: f64) -> Option<String> {
    (of.abs() >= 0.005).then(|| format!("{:.1}%", (value / of * 100.0).abs()))
}

/// ", nearly all of it from Bitcoin", when one holding made most of a move.
fn leader_clause(leader: Option<&Contribution>, total: f64) -> String {
    let Some(leader) = leader else {
        return String::new();
    };
    if total.abs() < 0.005 || leader.amount * total <= 0.0 {
        return String::new();
    }
    let share = leader.amount / total;
    if share >= 0.8 {
        format!(", nearly all of it from {}", leader.name)
    } else if share >= 0.5 {
        format!(", most of it from {}", leader.name)
    } else if share >= 0.3 {
        format!(", led by {}", leader.name)
    } else {
        String::new()
    }
}

fn up_or_down(value: f64) -> &'static str {
    if value >= 0.0 { "up" } else { "down" }
}

/// A sentence about a move with no money in or out: "you are up €280 (0.3%), most of it
/// from Gold", after an opening such as "Since Tuesday,".
fn plain_move(
    words: &Words,
    opening: &str,
    amount: f64,
    base: f64,
    leader: Option<&Contribution>,
) -> String {
    format!(
        "{opening} you are {} {}{}.",
        up_or_down(amount),
        words.amount_and_share(amount, base),
        leader_clause(leader, amount),
    )
}

/// "Since Tuesday," or, for a day with a year, "Since October 14, 2025,".
fn since_opening(day: &str) -> String {
    format!("Since {day},")
}

/// The sentence about today's move, after an opening such as "Today,".
fn today_sentence(words: &Words, opening: &str, change: &Move) -> Option<String> {
    if change.amount.abs() < 0.005 {
        return None;
    }
    let share = if change.percent.abs() >= 0.005 {
        format!("{:.1}%", change.percent.abs())
    } else {
        String::new()
    };
    let amount = match (words.money(change.amount), share.is_empty()) {
        (Some(money), false) => format!("{money} ({share})"),
        (Some(money), true) => money,
        (None, false) => share,
        (None, true) => return None,
    };
    Some(format!(
        "{opening} you are {} {amount}{}.",
        up_or_down(change.amount),
        leader_clause(change.leader.as_ref(), change.amount),
    ))
}

/// A record the change set, in one plain sentence, or nothing.
fn record(words: &Words, change: &Change, today: NaiveDate) -> Option<String> {
    let high = change.previous_high?;
    let long_enough = change
        .history_from
        .is_some_and(|from| (today - from).num_days() >= RECORD_AFTER_DAYS);
    if change.total() <= 0.0 || change.end <= high || !long_enough {
        return None;
    }
    if let Some(round) = round_crossed(high, change.end)
        && let Some(money) = words.money(round)
    {
        return Some(format!("It is past {money} for the first time."));
    }
    Some("That is the highest value Stahl has recorded.".to_string())
}

/// The largest round number in `(below, at]`: a multiple of half the total's order of magnitude,
/// like 50,000 or 100,000 for a six-figure total.
fn round_crossed(below: f64, at: f64) -> Option<f64> {
    if at < 1000.0 || below >= at {
        return None;
    }
    let step = 10f64.powf(at.log10().floor()) / 2.0;
    let round = (at / step).floor() * step;
    (round > below).then_some(round)
}

/// The figures under the sentences: the change, how much of it was the market and how much was
/// money, and the holding that moved it most.
fn figures(words: &Words, change: &Change, day: &str) -> Vec<Figure> {
    let mut figures = vec![Figure {
        label: format!("Change since {day}"),
        value: words.signed(change.total()),
        tone: Tone::of(change.total()),
        note: percent_of(change.total(), change.start).map(|share| {
            let sign = if change.total() < 0.0 { "-" } else { "+" };
            format!("{sign}{share}")
        }),
    }];
    if change.added.abs() < NO_MONEY {
        figures.push(Figure {
            label: "Money in or out".to_string(),
            value: "None".to_string(),
            tone: Tone::Plain,
            note: Some("All of it was the market".to_string()),
        });
    } else {
        figures.push(Figure {
            label: "From the market".to_string(),
            value: words.signed(change.market()),
            tone: Tone::of(change.market()),
            note: percent_of(change.market(), change.start).map(|share| {
                let sign = if change.market() < 0.0 { "-" } else { "+" };
                format!("{sign}{share}")
            }),
        });
        figures.push(Figure {
            label: if change.added > 0.0 {
                "Money added"
            } else {
                "Money taken out"
            }
            .to_string(),
            value: if words.facts.hidden {
                format::HIDDEN.to_string()
            } else {
                format::currency(change.added.abs(), words.facts.currency)
            },
            tone: Tone::Plain,
            note: None,
        });
    }
    if let Some(leader) = &change.leader
        && leader.amount.abs() >= 0.005
    {
        figures.push(Figure {
            label: "Moved it most".to_string(),
            value: leader.name.clone(),
            tone: Tone::Plain,
            note: Some(words.signed(leader.amount)),
        });
    }
    figures
}

/// The sentence about a measured change, after `opening` ("Since Tuesday,").
fn change_sentence(words: &Words, opening: &str, change: &Change) -> String {
    let market = change.market();
    let barely = market.abs() < (change.start.abs() * BARELY).max(0.005);
    if change.added.abs() < NO_MONEY {
        if barely {
            // "Since Tuesday," becomes "since Tuesday".
            let day = opening.trim_end_matches(',').replacen("Since", "since", 1);
            return if market.abs() < 0.005 {
                format!("Nothing has changed {day}.")
            } else {
                format!("Your portfolio has barely moved {day}.")
            };
        }
        return plain_move(words, opening, market, change.start, change.leader.as_ref());
    }
    let money = match words.money(change.added) {
        Some(money) if change.added > 0.0 => format!("you added {money}"),
        Some(money) => format!("you took out {money}"),
        None if change.added > 0.0 => "you added money".to_string(),
        None => "you took money out".to_string(),
    };
    let market_words = if barely {
        "the market barely moved".to_string()
    } else {
        format!(
            "the market {} {}",
            if market > 0.0 { "added" } else { "took" },
            words.amount_and_share(market, change.start)
        )
    };
    format!("{opening} {money}, and {market_words}.")
}

/// The Statement, or `None` when there is nothing worth saying.
pub fn compose(facts: &Facts) -> Option<Statement> {
    let words = Words { facts };
    let today = facts.today_move.as_ref();
    let only = |text: String| Statement {
        text,
        figures: Vec::new(),
    };

    if !facts.complete {
        return Some(only(match facts.since {
            Since::Day(day) => format!(
                "Some prices are missing, so what changed since {} isn't known yet.",
                day_name(day, facts.today)
            ),
            _ => "Some prices are missing, so today's total leaves out part of the portfolio."
                .to_string(),
        }));
    }

    match facts.since {
        Since::Day(day) => {
            let name = day_name(day, facts.today);
            match &facts.change {
                Some(change) => {
                    // The valuation compared with may be older than the last look, when the
                    // app wasn't able to record one that day.
                    let measured = if change.from < day {
                        day_name(change.from, facts.today)
                    } else {
                        name
                    };
                    let mut text = change_sentence(&words, &since_opening(&measured), change);
                    if let Some(record) = record(&words, change, facts.today) {
                        text = format!("{text} {record}");
                    }
                    Some(Statement {
                        figures: figures(&words, change, &measured),
                        text,
                    })
                }
                None => {
                    let today = today.and_then(|change| today_sentence(&words, "Today,", change));
                    let why = facts.encrypted.then(|| {
                        format!(
                            "An encrypted portfolio keeps no history, so there is nothing from \
                             {name} to compare with."
                        )
                    });
                    let text = [today, why]
                        .into_iter()
                        .flatten()
                        .collect::<Vec<_>>()
                        .join(" ");
                    (!text.is_empty()).then(|| only(text))
                }
            }
        }
        Since::EarlierToday => {
            let moved =
                today.and_then(|change| today_sentence(&words, "Since yesterday's close,", change));
            Some(only(match moved {
                Some(moved) => format!("You looked earlier today. {moved}"),
                None => "You looked earlier today, and prices are where they closed.".to_string(),
            }))
        }
        Since::First => match &facts.change {
            Some(change) if change.from < facts.today => {
                let begins = format!(
                    "Your history goes back to {}.",
                    day_name(change.from, facts.today)
                );
                let text = format!(
                    "{begins} {}",
                    change_sentence(&words, "Since then,", change)
                );
                Some(Statement {
                    figures: figures(&words, change, &day_name(change.from, facts.today)),
                    text,
                })
            }
            _ if facts.encrypted => today
                .and_then(|change| today_sentence(&words, "Today,", change))
                .map(only),
            _ => Some(only(
                "Stahl keeps a valuation for each day you open it. Next time, this will say \
                 what changed."
                    .to_string(),
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
    }

    /// Tuesday, November 10, 2026.
    fn today() -> NaiveDate {
        day("2026-11-10")
    }

    fn base() -> Facts<'static> {
        Facts {
            today: today(),
            since: Since::Day(day("2026-11-05")),
            currency: "EUR",
            hidden: false,
            complete: true,
            encrypted: false,
            change: None,
            today_move: None,
        }
    }

    fn change(start: f64, end: f64, added: f64) -> Change {
        Change {
            from: day("2026-11-05"),
            start,
            end,
            added,
            leader: None,
            previous_high: None,
            history_from: None,
        }
    }

    fn leader(name: &str, amount: f64) -> Option<Contribution> {
        Some(Contribution {
            name: name.to_string(),
            amount,
        })
    }

    fn say(facts: &Facts) -> String {
        compose(facts).expect("something to say").text
    }

    #[test]
    fn days_are_named_as_people_say_them() {
        assert_eq!(day_name(day("2026-11-09"), today()), "yesterday");
        assert_eq!(day_name(day("2026-11-05"), today()), "Thursday");
        assert_eq!(day_name(day("2026-11-04"), today()), "Wednesday");
        assert_eq!(day_name(day("2026-11-03"), today()), "November 3");
        assert_eq!(day_name(day("2025-12-24"), today()), "December 24, 2025");
    }

    #[test]
    fn a_look_remembers_the_day_before_it() {
        let first = Seen::after_visit(None, day("2026-11-05"));
        assert_eq!(Seen::since(None, day("2026-11-05")), Since::First);
        assert_eq!(first.before, None);

        // Later the same day: still nothing earlier to measure from.
        let again = Seen::after_visit(Some(first), day("2026-11-05"));
        assert_eq!(again, first);
        assert_eq!(
            Seen::since(Some(first), day("2026-11-05")),
            Since::EarlierToday
        );

        // A new day measures from the last one, and keeps doing so on a second look that day.
        assert_eq!(
            Seen::since(Some(first), today()),
            Since::Day(day("2026-11-05"))
        );
        let next = Seen::after_visit(Some(first), today());
        assert_eq!(next.before, Some(day("2026-11-05")));
        assert_eq!(
            Seen::since(Some(next), today()),
            Since::Day(day("2026-11-05"))
        );
    }

    #[test]
    fn the_first_visit_without_history_says_what_will_happen() {
        let facts = Facts {
            since: Since::First,
            ..base()
        };
        let statement = compose(&facts).unwrap();
        assert_eq!(
            statement.text,
            "Stahl keeps a valuation for each day you open it. Next time, this will say what \
             changed."
        );
        assert!(statement.figures.is_empty());
    }

    #[test]
    fn the_first_visit_with_history_reads_from_its_start() {
        let facts = Facts {
            since: Since::First,
            change: Some(Change {
                from: day("2026-08-12"),
                ..change(100_000.0, 103_100.0, 0.0)
            }),
            ..base()
        };
        assert_eq!(
            say(&facts),
            "Your history goes back to August 12. Since then, you are up €3,100 (3.1%)."
        );
    }

    #[test]
    fn a_second_look_the_same_day_speaks_of_today() {
        let facts = Facts {
            since: Since::EarlierToday,
            today_move: Some(Move {
                amount: 280.0,
                percent: 0.26,
                leader: leader("Gold", 150.0),
            }),
            ..base()
        };
        assert_eq!(
            say(&facts),
            "You looked earlier today. Since yesterday's close, you are up €280 (0.3%), most \
             of it from Gold."
        );

        let still = Facts {
            today_move: None,
            ..facts
        };
        assert_eq!(
            say(&still),
            "You looked earlier today, and prices are where they closed."
        );
    }

    #[test]
    fn no_change_says_so_briefly() {
        let facts = Facts {
            change: Some(change(100_000.0, 100_000.0, 0.0)),
            ..base()
        };
        assert_eq!(say(&facts), "Nothing has changed since Thursday.");

        let barely = Facts {
            change: Some(change(100_000.0, 100_004.0, 0.0)),
            ..base()
        };
        assert_eq!(
            say(&barely),
            "Your portfolio has barely moved since Thursday."
        );
    }

    #[test]
    fn a_loss_names_what_caused_it() {
        let facts = Facts {
            change: Some(Change {
                leader: leader("S&P 500", -900.0),
                ..change(108_640.0, 107_400.0, 0.0)
            }),
            ..base()
        };
        let statement = compose(&facts).unwrap();
        assert_eq!(
            statement.text,
            "Since Thursday, you are down €1,240 (1.1%), most of it from S&P 500."
        );
        assert_eq!(statement.figures[0].label, "Change since Thursday");
        assert_eq!(statement.figures[0].value, "-€1,240.00");
        assert_eq!(statement.figures[0].tone, Tone::Down);
        assert_eq!(statement.figures[0].note.as_deref(), Some("-1.1%"));
        assert_eq!(statement.figures[1].value, "None", "no money moved");
        assert_eq!(
            statement.figures[1].note.as_deref(),
            Some("All of it was the market")
        );
        assert_eq!(
            statement.figures.len(),
            3,
            "the change, the money, and the leader"
        );
        assert_eq!(statement.figures[2].value, "S&P 500");
        assert_eq!(statement.figures[2].note.as_deref(), Some("-€900.00"));
    }

    #[test]
    fn a_gain_names_the_holding_only_when_it_made_most_of_it() {
        let gain = |amount| Facts {
            change: Some(Change {
                leader: leader("Bitcoin", amount),
                ..change(100_000.0, 101_000.0, 0.0)
            }),
            ..base()
        };
        assert!(say(&gain(950.0)).ends_with("(1.0%), nearly all of it from Bitcoin."));
        assert!(say(&gain(400.0)).ends_with("(1.0%), led by Bitcoin."));
        assert_eq!(
            say(&gain(100.0)),
            "Since Thursday, you are up €1,000 (1.0%)."
        );
        // A holding that moved against the total didn't make the gain.
        assert_eq!(
            say(&gain(-600.0)),
            "Since Thursday, you are up €1,000 (1.0%)."
        );
    }

    #[test]
    fn a_gain_from_deposits_is_not_called_a_return() {
        let facts = Facts {
            change: Some(change(100_000.0, 105_400.0, 5_000.0)),
            ..base()
        };
        let statement = compose(&facts).unwrap();
        assert_eq!(
            statement.text,
            "Since Thursday, you added €5,000, and the market added €400 (0.4%)."
        );
        let labels: Vec<&str> = statement.figures.iter().map(|f| f.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Change since Thursday", "From the market", "Money added"]
        );
        assert_eq!(statement.figures[1].value, "+€400.00");
        assert_eq!(statement.figures[2].value, "€5,000.00");

        let lost = Facts {
            change: Some(change(100_000.0, 104_700.0, 5_000.0)),
            ..base()
        };
        assert_eq!(
            say(&lost),
            "Since Thursday, you added €5,000, and the market took €300 (0.3%)."
        );

        let withdrew = Facts {
            change: Some(change(100_000.0, 98_000.0, -2_000.0)),
            ..base()
        };
        assert_eq!(
            say(&withdrew),
            "Since Thursday, you took out €2,000, and the market barely moved."
        );
    }

    #[test]
    fn without_history_it_speaks_of_today_or_not_at_all() {
        let facts = Facts {
            today_move: Some(Move {
                amount: -120.0,
                percent: -0.11,
                leader: None,
            }),
            ..base()
        };
        assert_eq!(say(&facts), "Today, you are down €120 (0.1%).");
        assert_eq!(
            compose(&Facts {
                today_move: None,
                ..facts
            }),
            None
        );
    }

    #[test]
    fn an_encrypted_portfolio_explains_why_it_cannot_compare() {
        let facts = Facts {
            encrypted: true,
            ..base()
        };
        assert_eq!(
            say(&facts),
            "An encrypted portfolio keeps no history, so there is nothing from Thursday to \
             compare with."
        );
        let first = Facts {
            since: Since::First,
            ..facts
        };
        assert_eq!(
            compose(&first),
            None,
            "and makes no promise about next time"
        );
    }

    #[test]
    fn missing_prices_never_read_as_a_loss() {
        let facts = Facts {
            complete: false,
            change: Some(change(100_000.0, 60_000.0, 0.0)),
            ..base()
        };
        let statement = compose(&facts).unwrap();
        assert_eq!(
            statement.text,
            "Some prices are missing, so what changed since Thursday isn't known yet."
        );
        assert!(statement.figures.is_empty());
    }

    #[test]
    fn privacy_keeps_amounts_out_of_the_words() {
        let facts = Facts {
            hidden: true,
            change: Some(Change {
                leader: leader("Bitcoin", 900.0),
                previous_high: Some(99_000.0),
                history_from: Some(day("2026-01-01")),
                ..change(99_000.0, 100_000.0, 0.0)
            }),
            ..base()
        };
        let statement = compose(&facts).unwrap();
        assert_eq!(
            statement.text,
            "Since Thursday, you are up 1.0%, nearly all of it from Bitcoin. That is the highest \
             value Stahl has recorded."
        );
        assert!(!statement.text.contains('€'));
        assert!(
            statement
                .figures
                .iter()
                .all(|figure| !figure.value.contains('€')
                    && !figure.note.as_deref().unwrap_or("").contains('€'))
        );

        let deposit = Facts {
            hidden: true,
            change: Some(change(100_000.0, 105_400.0, 5_000.0)),
            ..base()
        };
        assert_eq!(
            say(&deposit),
            "Since Thursday, you added money, and the market added 0.4%."
        );
    }

    #[test]
    fn records_are_one_plain_sentence() {
        let record = |previous_high, history_from| Facts {
            change: Some(Change {
                previous_high: Some(previous_high),
                history_from: Some(day(history_from)),
                ..change(98_000.0, 101_000.0, 0.0)
            }),
            ..base()
        };
        assert_eq!(
            say(&record(99_500.0, "2026-01-01")),
            "Since Thursday, you are up €3,000 (3.1%). It is past €100,000 for the first time."
        );
        assert_eq!(
            say(&record(100_500.0, "2026-01-01")),
            "Since Thursday, you are up €3,000 (3.1%). That is the highest value Stahl has \
             recorded."
        );
        assert_eq!(
            say(&record(102_000.0, "2026-01-01")),
            "Since Thursday, you are up €3,000 (3.1%)."
        );
        // A week of history makes every value a record. That says nothing.
        assert_eq!(
            say(&record(99_500.0, "2026-11-03")),
            "Since Thursday, you are up €3,000 (3.1%)."
        );
    }

    #[test]
    fn round_numbers_are_round() {
        assert_eq!(round_crossed(99_000.0, 107_400.0), Some(100_000.0));
        assert_eq!(round_crossed(148_000.0, 151_000.0), Some(150_000.0));
        assert_eq!(round_crossed(101_000.0, 107_400.0), None);
        assert_eq!(round_crossed(9_900.0, 10_100.0), Some(10_000.0));
        assert_eq!(
            round_crossed(400.0, 600.0),
            None,
            "too small to celebrate quietly"
        );
    }

    #[test]
    fn an_older_valuation_is_named_for_its_own_day() {
        let facts = Facts {
            change: Some(Change {
                from: day("2026-11-03"),
                ..change(100_000.0, 101_000.0, 0.0)
            }),
            ..base()
        };
        assert!(say(&facts).starts_with("Since November 3, you are up"));
    }

    #[test]
    fn todays_leader_is_the_holding_that_moved_the_total_most() {
        let position = |name: &str, value: f64, percent: f64| PositionDto {
            id: 0,
            position_id: None,
            name: Some(name.to_string()),
            ticker: None,
            asset_class: "Stocks".to_string(),
            amount: 1.0,
            price: value,
            value,
            pnl: None,
            pnl_percent: None,
            day_change_percent: Some(percent),
            average_cost: None,
            invested: None,
            purchases: Vec::new(),
        };
        let positions = [
            position("Small", 1_000.0, 10.0),
            position("Large", 50_000.0, 1.0),
            position("Falling", 20_000.0, -5.0),
        ];
        let moved = today_move(300.0, 0.4, &positions);
        assert_eq!(moved.leader.unwrap().name, "Large");
        let fell = today_move(-300.0, -0.4, &positions);
        assert_eq!(fell.leader.unwrap().name, "Falling");
    }
}
