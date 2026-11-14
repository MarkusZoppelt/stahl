//! What a file's rows mean for the positions on record: the changes to propose, each with its
//! doubts, and what already agrees.
//!
//! Pure: a reading and the holdings in, proposals out. Nothing is ever removed: a holding the
//! file doesn't mention is reported, and the owner decides.

use std::collections::HashMap;

use chrono::NaiveDate;

use super::{
    rules::{Dated, Entry, Figure, Reading, Shape, Text},
    text::fold,
};

/// A purchase on record, as reconciling needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Bought {
    pub date: Option<NaiveDate>,
    pub quantity: f64,
    pub price: Option<f64>,
}

/// A position on record, as reconciling needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Holding {
    /// The stable `pos_` id, which edits are made by.
    pub id: String,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub class: String,
    /// What is held: the purchases' total, or the amount when there are none.
    pub quantity: f64,
    pub purchases: Vec<Bought>,
    /// The currency its price is quoted in, when known.
    pub currency: Option<String>,
}

impl Holding {
    pub fn title(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.ticker.clone())
            .unwrap_or_else(|| "A holding".to_string())
    }

    /// A holding without a ticker is cash or something valued by hand: no broker reports it.
    fn traded(&self) -> bool {
        self.ticker.is_some()
    }
}

/// How a row was matched to a holding.
#[derive(Clone, Debug, PartialEq)]
pub enum Match {
    Isin,
    Ticker,
    /// The file's ticker without the record's exchange suffix: `VWCE` and `VWCE.DE`.
    TickerBase,
    /// By name only, so the owner should look: the file's name and the score.
    Name {
        file: String,
        score: f64,
    },
}

impl Match {
    pub fn certain(&self) -> bool {
        matches!(self, Match::Isin | Match::Ticker)
    }
}

/// A holding on record that a change is about.
#[derive(Clone, Debug, PartialEq)]
pub struct Matched {
    pub id: String,
    pub title: String,
    pub ticker: Option<String>,
    pub by: Match,
    /// What is held before the change.
    pub before: f64,
}

/// A holding the file has and the record doesn't.
#[derive(Clone, Debug, PartialEq)]
pub struct Newcomer {
    pub name: Option<Text>,
    pub ticker: Option<Text>,
    pub isin: Option<Text>,
    pub class: Option<Text>,
}

impl Newcomer {
    pub fn title(&self) -> String {
        self.name
            .as_ref()
            .or(self.ticker.as_ref())
            .or(self.isin.as_ref())
            .map_or_else(|| "A new holding".to_string(), |text| text.text.clone())
    }
}

/// One purchase or sale to add, every figure copied from its row.
#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    pub line: usize,
    pub date: Option<Dated>,
    /// Negative for a sale.
    pub quantity: Figure,
    pub price: Option<Figure>,
    pub fees: Option<Figure>,
    pub currency: Option<Text>,
    /// The file's price is in another currency than the holding is quoted in, so it is left
    /// out rather than converted.
    pub price_left_out: bool,
}

impl Trade {
    fn of(entry: &Entry) -> Trade {
        Trade {
            line: entry.line,
            date: entry.date.clone(),
            quantity: entry.quantity.clone(),
            price: entry.price.clone(),
            fees: entry.fees.clone(),
            currency: entry.currency.clone(),
            price_left_out: false,
        }
    }

    /// The price that would be recorded.
    pub fn recorded_price(&self) -> Option<f64> {
        self.price
            .as_ref()
            .filter(|_| !self.price_left_out)
            .map(|price| price.value)
    }
}

/// What a proposal would change.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    /// A new holding, with its trades or with what the file says is held.
    Add {
        holding: Newcomer,
        trades: Vec<Trade>,
        held: Option<Figure>,
    },
    /// Purchases or sales added to a holding on record. `opening` is set when the holding has
    /// no purchases yet: what it holds becomes its first, undated entry, so nothing is lost.
    Trades {
        holding: Matched,
        trades: Vec<Trade>,
        opening: bool,
    },
    /// The file holds a different quantity. `settable` when the record has no purchases, so
    /// the file's figure can simply replace its amount.
    Quantity {
        holding: Matched,
        held: Figure,
        settable: bool,
    },
    /// On record, and not in this holdings list. Reported only.
    Missing { holding: Matched },
}

impl Change {
    /// The lines of the file this change comes from.
    pub fn lines(&self) -> Vec<usize> {
        match self {
            Change::Add { trades, held, .. } => trades
                .iter()
                .map(|trade| trade.line)
                .chain(held.iter().map(|held| held.at.line))
                .collect(),
            Change::Trades { trades, .. } => trades.iter().map(|trade| trade.line).collect(),
            Change::Quantity { held, .. } => vec![held.at.line],
            Change::Missing { .. } => Vec::new(),
        }
    }
}

/// A change to show the owner, and what to be careful about.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub change: Change,
    /// What the owner should know before accepting, in plain words.
    pub doubts: Vec<String>,
    /// It can be recorded as it stands. A new holding without a ticker, say, can't.
    pub ready: bool,
    /// "Accept all" takes it. Only what is certain enough to accept unseen in a list the owner
    /// is looking at.
    pub in_all: bool,
}

/// Everything a file meant for the record.
#[derive(Clone, Debug, PartialEq)]
pub struct Ledger {
    pub proposals: Vec<Proposal>,
    /// Rows already on record, so nothing is proposed for them.
    pub agreed: usize,
    /// Something to say about the file as a whole.
    pub notes: Vec<String>,
}

/// Words in security names that say nothing about which security it is.
const FILLER: [&str; 30] = [
    "inc",
    "plc",
    "etf",
    "ucits",
    "acc",
    "dist",
    "the",
    "fund",
    "class",
    "shares",
    "share",
    "sa",
    "ag",
    "corp",
    "corporation",
    "ltd",
    "co",
    "se",
    "nv",
    "trust",
    "usd",
    "eur",
    "a",
    "c",
    "d",
    "registered",
    "namens",
    "aktie",
    "aktien",
    "inhaber",
];

/// A name's distinctive words, each without a plural `s`, so "Long Bonds" and "Long Bond
/// Fund" are the same two words.
fn words(name: &str) -> Vec<String> {
    fold(name)
        .split(' ')
        .filter(|word| !word.is_empty() && !FILLER.contains(word))
        .map(|word| match word.strip_suffix('s') {
            Some(stem) if stem.len() >= 3 && !stem.ends_with('s') => stem.to_string(),
            _ => word.to_string(),
        })
        .collect()
}

/// How alike two names are, 0 to 1: shared words over all words, after the filler is gone.
pub fn likeness(a: &str, b: &str) -> f64 {
    let a = words(a);
    let b = words(b);
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.iter().filter(|word| b.contains(word)).count();
    // One name wholly inside the other, with something distinctive in it, is a strong match.
    let smaller = a.len().min(b.len());
    if shared == smaller {
        // A single distinctive word ("Gold" for "Gold Bullion Shares") is enough to show, but
        // not enough to take unseen: it is still only a match by name.
        if smaller >= 2 {
            return 0.9_f64.max(2.0 * shared as f64 / (a.len() + b.len()) as f64);
        }
        if a.iter().chain(&b).any(|word| word.len() >= 4) {
            return 0.8;
        }
    }
    2.0 * shared as f64 / (a.len() + b.len()) as f64
}

/// The least likeness that counts as a match by name.
const NAME_MATCH: f64 = 0.75;

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Which holding an entry is about, and how that was decided. `Err` when two holdings match
/// equally well, with their titles.
fn find(entry: &Entry, holdings: &[Holding]) -> Result<Option<(usize, Match)>, Vec<String>> {
    if let Some(isin) = &entry.isin {
        let found = holdings.iter().position(|holding| {
            holding
                .ticker
                .as_deref()
                .is_some_and(|ticker| same(ticker, &isin.text))
                || holding
                    .name
                    .as_deref()
                    .is_some_and(|name| name.to_uppercase().contains(&isin.text.to_uppercase()))
        });
        if let Some(at) = found {
            return Ok(Some((at, Match::Isin)));
        }
    }
    if let Some(ticker) = &entry.ticker {
        if let Some(at) = holdings.iter().position(|holding| {
            holding
                .ticker
                .as_deref()
                .is_some_and(|own| same(own, &ticker.text))
        }) {
            return Ok(Some((at, Match::Ticker)));
        }
        let base = |text: &str| text.split('.').next().unwrap_or(text).trim().to_uppercase();
        let bases: Vec<usize> = holdings
            .iter()
            .enumerate()
            .filter(|(_, holding)| {
                holding
                    .ticker
                    .as_deref()
                    .is_some_and(|own| base(own) == base(&ticker.text))
            })
            .map(|(at, _)| at)
            .collect();
        if let [at] = bases.as_slice() {
            return Ok(Some((*at, Match::TickerBase)));
        }
    }
    let Some(name) = &entry.name else {
        return Ok(None);
    };
    let mut scored: Vec<(usize, f64)> = holdings
        .iter()
        .enumerate()
        .filter(|(_, holding)| {
            // A row whose ticker names something else is not this holding, whatever its name.
            match (&entry.ticker, &holding.ticker) {
                (Some(theirs), Some(ours)) => same(&theirs.text, ours),
                _ => true,
            }
        })
        .filter_map(|(at, holding)| {
            let score = holding
                .name
                .as_deref()
                .map_or(0.0, |own| likeness(&name.text, own));
            (score >= NAME_MATCH).then_some((at, score))
        })
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    match scored.as_slice() {
        [] => Ok(None),
        [(at, score)] => Ok(Some((
            *at,
            Match::Name {
                file: name.text.clone(),
                score: *score,
            },
        ))),
        [(at, score), (next, other), ..] => {
            if (score - other).abs() < 0.05 {
                Err(vec![holdings[*at].title(), holdings[*next].title()])
            } else {
                Ok(Some((
                    *at,
                    Match::Name {
                        file: name.text.clone(),
                        score: *score,
                    },
                )))
            }
        }
    }
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// Whether a purchase on record is this trade.
fn recorded(bought: &Bought, trade: &Trade) -> bool {
    bought.date == trade.date.as_ref().map(|date| date.date)
        && close(bought.quantity, trade.quantity.value)
        && match (bought.price, trade.recorded_price()) {
            (Some(a), Some(b)) => close(a, b),
            _ => true,
        }
}

fn trade_key(trade: &Trade) -> (Option<NaiveDate>, String, Option<String>) {
    (
        trade.date.as_ref().map(|date| date.date),
        format!("{:.9}", trade.quantity.value),
        trade.recorded_price().map(|price| format!("{price:.9}")),
    )
}

/// Who an entry belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Owner {
    Existing(usize),
    New(String),
}

fn new_key(entry: &Entry) -> String {
    entry
        .isin
        .as_ref()
        .map(|isin| format!("isin:{}", isin.text.to_uppercase()))
        .or_else(|| {
            entry
                .ticker
                .as_ref()
                .map(|ticker| format!("ticker:{}", ticker.text.to_uppercase()))
        })
        .unwrap_or_else(|| format!("name:{}", fold(&entry.title())))
}

/// Compare a reading with the holdings on record.
pub fn reconcile(reading: &Reading, holdings: &[Holding]) -> Ledger {
    let mut groups: Vec<(Owner, Vec<&Entry>, Option<Match>)> = Vec::new();
    let mut place: HashMap<Owner, usize> = HashMap::new();
    let mut notes = Vec::new();
    let mut ambiguous: Vec<(usize, Vec<String>)> = Vec::new();
    for entry in &reading.entries {
        let (owner, by) = match find(entry, holdings) {
            Ok(Some((at, by))) => (Owner::Existing(at), Some(by)),
            Ok(None) => (Owner::New(new_key(entry)), None),
            Err(titles) => {
                ambiguous.push((entry.line, titles));
                (Owner::New(new_key(entry)), None)
            }
        };
        match place.get(&owner) {
            Some(at) => groups[*at].1.push(entry),
            None => {
                place.insert(owner.clone(), groups.len());
                groups.push((owner, vec![entry], by));
            }
        }
    }

    let mut proposals = Vec::new();
    let mut agreed = 0;
    let mut matched_any = false;
    for (owner, entries, by) in &groups {
        match owner {
            Owner::Existing(at) => {
                matched_any = true;
                let holding = &holdings[*at];
                let by = by.clone().expect("an existing holding was matched somehow");
                let matched = Matched {
                    id: holding.id.clone(),
                    title: holding.title(),
                    ticker: holding.ticker.clone(),
                    by: by.clone(),
                    before: holding.quantity,
                };
                let mut doubts = Vec::new();
                match &by {
                    Match::Name { file, .. } => doubts.push(format!(
                        "Matched by name only: “{file}” in the file, “{}” on record.",
                        holding.title()
                    )),
                    Match::TickerBase => doubts.push(format!(
                        "Matched by ticker without its exchange: “{}” in the file, “{}” on record.",
                        entries[0]
                            .ticker
                            .as_ref()
                            .map_or("", |ticker| ticker.text.as_str()),
                        holding.ticker.as_deref().unwrap_or("")
                    )),
                    Match::Isin | Match::Ticker => {}
                }
                match reading.shape {
                    Shape::Transactions => {
                        let (proposal, already) = trades_for(holding, matched, entries, doubts);
                        agreed += already;
                        proposals.extend(proposal);
                    }
                    Shape::Holdings => {
                        if entries.len() > 1 {
                            doubts.push(format!(
                                "The file lists this holding on {} lines; it may be held in more than one account.",
                                entries.len()
                            ));
                            for entry in entries {
                                proposals.push(Proposal {
                                    change: Change::Quantity {
                                        holding: matched.clone(),
                                        held: entry.quantity.clone(),
                                        settable: false,
                                    },
                                    doubts: doubts.clone(),
                                    ready: false,
                                    in_all: false,
                                });
                            }
                            continue;
                        }
                        let held = &entries[0].quantity;
                        if close(held.value, holding.quantity) {
                            agreed += 1;
                            continue;
                        }
                        let settable = holding.purchases.is_empty();
                        if !settable {
                            doubts.push(
                                "Your record keeps this holding's purchases, and the file doesn't say \
                                 which trades made the difference. A transactions export would."
                                    .to_string(),
                            );
                        }
                        let certain = by.certain();
                        proposals.push(Proposal {
                            change: Change::Quantity {
                                holding: matched,
                                held: held.clone(),
                                settable,
                            },
                            doubts,
                            ready: settable,
                            in_all: settable && certain,
                        });
                    }
                }
            }
            Owner::New(_) => {
                let first = entries[0];
                let newcomer = Newcomer {
                    name: entries.iter().find_map(|entry| entry.name.clone()),
                    ticker: entries.iter().find_map(|entry| entry.ticker.clone()),
                    isin: entries.iter().find_map(|entry| entry.isin.clone()),
                    class: entries.iter().find_map(|entry| entry.class.clone()),
                };
                let mut doubts = Vec::new();
                if let Some((_, titles)) = ambiguous.iter().find(|(line, _)| *line == first.line) {
                    doubts.push(format!(
                        "This could be {} or {}, so it isn't matched to either.",
                        titles[0], titles[1]
                    ));
                }
                if newcomer.ticker.is_none() {
                    doubts.push(
                        "The file gives no ticker, and prices are looked up by ticker, so this can't be recorded from here. Add it by hand in Holdings."
                            .to_string(),
                    );
                }
                if newcomer.class.is_none() {
                    doubts.push(
                        "The file gives no asset class, so this would be filed under Stocks."
                            .to_string(),
                    );
                }
                let ready_ticker = newcomer.ticker.is_some();
                match reading.shape {
                    Shape::Transactions => {
                        let mut trades: Vec<Trade> =
                            entries.iter().map(|entry| Trade::of(entry)).collect();
                        let repeated = repeats(&trades);
                        if repeated > 0 {
                            doubts.push(repeated_doubt(repeated));
                        }
                        trades.sort_by_key(|trade| trade.date.as_ref().map(|date| date.date));
                        let held: f64 = trades.iter().map(|trade| trade.quantity.value).sum();
                        let mut ready = ready_ticker;
                        if held < -1e-9 {
                            doubts.push(
                                "The file shows more sold than bought, and nothing is on record to sell from."
                                    .to_string(),
                            );
                            ready = false;
                        }
                        proposals.push(Proposal {
                            change: Change::Add {
                                holding: newcomer,
                                trades,
                                held: None,
                            },
                            in_all: ready && doubts.is_empty(),
                            doubts,
                            ready,
                        });
                    }
                    Shape::Holdings => {
                        if entries.len() > 1 {
                            doubts.push(format!(
                                "The file lists this holding on {} lines; each is shown separately.",
                                entries.len()
                            ));
                        }
                        for entry in entries {
                            proposals.push(Proposal {
                                change: Change::Add {
                                    holding: newcomer.clone(),
                                    trades: Vec::new(),
                                    held: Some(entry.quantity.clone()),
                                },
                                in_all: ready_ticker && entries.len() == 1 && doubts.is_empty(),
                                doubts: doubts.clone(),
                                ready: ready_ticker,
                            });
                        }
                    }
                }
            }
        }
    }

    // A holdings list says what is held: a traded holding it leaves out is worth a word. Only
    // when the file is about this portfolio at all.
    if reading.shape == Shape::Holdings && matched_any {
        let mentioned: Vec<usize> = groups
            .iter()
            .filter_map(|(owner, ..)| match owner {
                Owner::Existing(at) => Some(*at),
                Owner::New(_) => None,
            })
            .collect();
        for (at, holding) in holdings.iter().enumerate() {
            if mentioned.contains(&at) || !holding.traded() {
                continue;
            }
            proposals.push(Proposal {
                change: Change::Missing {
                    holding: Matched {
                        id: holding.id.clone(),
                        title: holding.title(),
                        ticker: holding.ticker.clone(),
                        by: Match::Ticker,
                        before: holding.quantity,
                    },
                },
                doubts: Vec::new(),
                ready: false,
                in_all: false,
            });
        }
    }
    if !matched_any && !reading.entries.is_empty() && holdings.iter().any(Holding::traded) {
        notes.push(
            "Nothing in this file matches a holding on record. If it belongs to another account, \
             each holding can still be added."
                .to_string(),
        );
    }
    if reading.guessed {
        let said = match reading.shape {
            Shape::Transactions => "a list of trades",
            Shape::Holdings => "a list of what is held",
        };
        for proposal in &mut proposals {
            if matches!(proposal.change, Change::Missing { .. }) {
                continue;
            }
            proposal.doubts.push(format!(
                "Nothing in the file says whether it is a list of trades or of what is held. We \
                 read it as {said}; check that before recording."
            ));
            proposal.in_all = false;
        }
    }
    Ledger {
        proposals,
        agreed,
        notes,
    }
}

/// How many of the trades repeat one before them exactly.
fn repeats(trades: &[Trade]) -> usize {
    let mut seen: HashMap<(Option<NaiveDate>, String, Option<String>), usize> = HashMap::new();
    let mut repeated = 0;
    for trade in trades {
        let count = seen.entry(trade_key(trade)).or_insert(0);
        if *count > 0 {
            repeated += 1;
        }
        *count += 1;
    }
    repeated
}

fn repeated_doubt(count: usize) -> String {
    if count == 1 {
        "One trade appears twice in the file, on the same day at the same price. If it is the same trade, leave this change and make it by hand.".to_string()
    } else {
        format!(
            "{count} trades repeat others in the file, on the same day at the same price. If some are the same trade, leave this change and make it by hand."
        )
    }
}

/// The trades for a holding on record that the record doesn't have yet, and how many it does.
fn trades_for(
    holding: &Holding,
    matched: Matched,
    entries: &[&Entry],
    mut doubts: Vec<String>,
) -> (Option<Proposal>, usize) {
    let mut trades: Vec<Trade> = entries.iter().map(|entry| Trade::of(entry)).collect();
    // A price in another currency than the holding's quote would be wrong in its cost, so it is
    // left out rather than converted.
    let mut left_out = 0;
    if let Some(own) = &holding.currency {
        for trade in &mut trades {
            if trade
                .currency
                .as_ref()
                .is_some_and(|currency| !same(&currency.text, own))
                && trade.price.is_some()
            {
                trade.price_left_out = true;
                left_out += 1;
            }
        }
    }
    // Each purchase on record accounts for one identical trade in the file, no more.
    let mut unused: Vec<&Bought> = holding.purchases.iter().collect();
    let mut fresh = Vec::new();
    let mut already = 0;
    for trade in trades {
        match unused.iter().position(|bought| recorded(bought, &trade)) {
            Some(at) => {
                unused.remove(at);
                already += 1;
            }
            None => fresh.push(trade),
        }
    }
    if fresh.is_empty() {
        return (None, already);
    }
    if left_out > 0 {
        let file = fresh
            .iter()
            .find_map(|trade| {
                trade
                    .currency
                    .as_ref()
                    .map(|currency| currency.text.clone())
            })
            .unwrap_or_default();
        doubts.push(format!(
            "The file prices this in {file}, and the holding is quoted in {}. The price is left out rather than converted.",
            holding.currency.as_deref().unwrap_or("")
        ));
    }
    let repeated = repeats(&fresh);
    if repeated > 0 {
        doubts.push(repeated_doubt(repeated));
    }
    let opening = holding.purchases.is_empty() && holding.quantity.abs() > 1e-12;
    let after = holding.quantity + fresh.iter().map(|trade| trade.quantity.value).sum::<f64>();
    let mut ready = true;
    if after < -1e-9 {
        doubts.push("This sells more than is on record.".to_string());
        ready = false;
    }
    fresh.sort_by_key(|trade| trade.date.as_ref().map(|date| date.date));
    if opening {
        doubts.push(
            "Your record holds an amount for this and no purchases. If that amount already \
             includes these trades, recording them would count them twice."
                .to_string(),
        );
    }
    let certain = matched.by.certain();
    let in_all = ready && certain && repeated == 0 && !opening;
    (
        Some(Proposal {
            change: Change::Trades {
                holding: matched,
                trades: fresh,
                opening,
            },
            doubts,
            ready,
            in_all,
        }),
        already,
    )
}
