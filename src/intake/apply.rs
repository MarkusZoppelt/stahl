//! Approved changes, written into the positions as `portfolio_rs` keeps them.
//!
//! The edits are made to the positions' own JSON, so everything about a position that a change
//! doesn't touch stays exactly as it was. Undo puts the JSON from before back.

use chrono::NaiveDate;
use serde_json::{Map, Number, Value, json};

use super::reconcile::{Change, Proposal};

/// A purchase (or, with a negative quantity, a sale) to record.
#[derive(Clone, Debug, PartialEq)]
pub struct Purchase {
    pub date: Option<NaiveDate>,
    pub quantity: f64,
    pub price: Option<f64>,
    pub fees: Option<f64>,
}

/// One edit to the positions.
#[derive(Clone, Debug, PartialEq)]
pub enum Op {
    /// Add purchases to the position with this id. With `opening`, a position with no purchases
    /// first gets its amount as an undated purchase, so what it held is kept.
    Append {
        id: String,
        purchases: Vec<Purchase>,
        opening: bool,
    },
    /// Replace the amount of a position that keeps no purchases.
    SetAmount { id: String, amount: f64 },
    /// A new position.
    Add {
        name: Option<String>,
        ticker: String,
        class: String,
        amount: f64,
        purchases: Vec<Purchase>,
    },
}

/// What the owner decided about one proposal, beyond accepting it.
#[derive(Clone, Debug, PartialEq)]
pub struct Choice {
    /// Which of its trades to record.
    pub keep: Vec<bool>,
    /// For a new holding: its ticker, name and asset class as the owner left them.
    pub ticker: String,
    pub name: String,
    pub class: String,
}

impl Choice {
    /// What a proposal starts with: every trade kept, and the file's own words for a newcomer.
    pub fn of(proposal: &Proposal) -> Choice {
        let (count, ticker, name, class) = match &proposal.change {
            Change::Add {
                holding, trades, ..
            } => (
                trades.len(),
                holding.ticker.as_ref().map(|text| text.text.clone()),
                holding.name.as_ref().map(|text| text.text.clone()),
                holding.class.as_ref().map(|text| text.text.clone()),
            ),
            Change::Trades { trades, .. } => (trades.len(), None, None, None),
            _ => (0, None, None, None),
        };
        Choice {
            keep: vec![true; count],
            ticker: ticker.unwrap_or_default(),
            name: name.unwrap_or_default(),
            // As the policy templates spell it, like the form's default.
            class: class.unwrap_or_else(|| "Stocks".to_string()),
        }
    }
}

/// The edit a proposal makes under the owner's choice. `None` when it makes none: a report, a
/// quantity the file can't settle, or every trade unticked.
pub fn op(proposal: &Proposal, choice: &Choice) -> Option<Op> {
    let purchases = |trades: &[super::reconcile::Trade]| -> Vec<Purchase> {
        trades
            .iter()
            .zip(&choice.keep)
            .filter(|(_, keep)| **keep)
            .map(|(trade, _)| Purchase {
                date: trade.date.as_ref().map(|date| date.date),
                quantity: trade.quantity.value,
                price: trade.recorded_price(),
                fees: trade.fees.as_ref().map(|fees| fees.value),
            })
            .collect()
    };
    match &proposal.change {
        Change::Trades {
            holding,
            trades,
            opening,
        } => {
            let purchases = purchases(trades);
            (!purchases.is_empty()).then(|| Op::Append {
                id: holding.id.clone(),
                purchases,
                opening: *opening,
            })
        }
        Change::Quantity {
            holding,
            held,
            settable: true,
        } => Some(Op::SetAmount {
            id: holding.id.clone(),
            amount: held.value,
        }),
        Change::Add { trades, held, .. } => {
            let ticker = choice.ticker.trim();
            if ticker.is_empty() {
                return None;
            }
            let purchases = purchases(trades);
            let amount = match held {
                Some(held) => held.value,
                None if purchases.is_empty() => return None,
                // `portfolio_rs` reads the amount from the purchases; this only mirrors them.
                None => purchases.iter().map(|purchase| purchase.quantity).sum(),
            };
            let name = choice.name.trim();
            Some(Op::Add {
                name: (!name.is_empty()).then(|| name.to_string()),
                ticker: ticker.to_string(),
                class: if choice.class.trim().is_empty() {
                    "Stocks".to_string()
                } else {
                    choice.class.trim().to_string()
                },
                amount,
                purchases,
            })
        }
        Change::Quantity { .. } | Change::Missing { .. } => None,
    }
}

/// The largest figure written. `portfolio_rs` reads its numbers as decimals, which stop near
/// 7.9e28; nothing a broker lists comes near this.
const MOST: f64 = 1e12;

fn number(value: f64) -> Result<Value, String> {
    if value.abs() > MOST {
        return Err(format!("{value} is too large to record."));
    }
    Number::from_f64(value)
        .map(Value::Number)
        .ok_or_else(|| format!("{value} can't be written as a number"))
}

fn purchase_json(purchase: &Purchase) -> Result<Value, String> {
    let mut map = Map::new();
    if let Some(date) = purchase.date {
        map.insert(
            "Date".into(),
            Value::String(date.format("%Y-%m-%d").to_string()),
        );
    }
    map.insert("Quantity".into(), number(purchase.quantity)?);
    if let Some(price) = purchase.price {
        map.insert("Price".into(), number(price)?);
    }
    if let Some(fees) = purchase.fees {
        map.insert("Fees".into(), number(fees)?);
    }
    Ok(Value::Object(map))
}

/// A date as `portfolio_rs` stores it: `2024-01-15` or `2024/01/15`.
fn stored_date(value: &Value) -> Option<NaiveDate> {
    let text = value.as_str()?.trim();
    NaiveDate::parse_from_str(text, "%Y-%m-%d")
        .or_else(|_| NaiveDate::parse_from_str(text, "%Y/%m/%d"))
        .or_else(|_| NaiveDate::parse_from_str(text, "%d-%m-%Y"))
        .ok()
}

/// Whether a stored purchase is this one already.
fn holds(stored: &Value, purchase: &Purchase) -> bool {
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);
    let date = stored.get("Date").and_then(stored_date);
    let quantity = stored.get("Quantity").and_then(Value::as_f64);
    let price = stored.get("Price").and_then(Value::as_f64);
    date == purchase.date
        && quantity.is_some_and(|quantity| close(quantity, purchase.quantity))
        && match (price, purchase.price) {
            (Some(a), Some(b)) => close(a, b),
            _ => true,
        }
}

/// Apply `ops` to the positions JSON. A purchase that is already there is not added twice, so
/// a change applied after the record moved on still can't double anything.
pub fn apply(json: &str, ops: &[Op]) -> Result<String, String> {
    let mut positions: Vec<Value> = serde_json::from_str(json)
        .map_err(|error| format!("The positions can't be read: {error}"))?;
    for op in ops {
        match op {
            Op::Append {
                id,
                purchases,
                opening,
            } => {
                let position = find(&mut positions, id)?;
                let object = position
                    .as_object_mut()
                    .ok_or_else(|| "A position isn't an object".to_string())?;
                let empty = object
                    .get("Purchases")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty);
                let mut list: Vec<Value> = object
                    .get("Purchases")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                if *opening && empty {
                    let amount = object.get("Amount").cloned().unwrap_or(json!(0));
                    if amount.as_f64().is_some_and(|amount| amount != 0.0) {
                        list.push(json!({ "Quantity": amount }));
                    }
                }
                // Each stored purchase answers for one identical new one, as in reconciling.
                let mut unused: Vec<bool> = vec![true; list.len()];
                for purchase in purchases {
                    let found = list
                        .iter()
                        .zip(unused.iter_mut())
                        .find(|(stored, free)| **free && holds(stored, purchase));
                    match found {
                        Some((_, free)) => *free = false,
                        None => list.push(purchase_json(purchase)?),
                    }
                }
                object.insert("Purchases".into(), Value::Array(list));
            }
            Op::SetAmount { id, amount } => {
                let position = find(&mut positions, id)?;
                let object = position
                    .as_object_mut()
                    .ok_or_else(|| "A position isn't an object".to_string())?;
                let has_purchases = object
                    .get("Purchases")
                    .and_then(Value::as_array)
                    .is_some_and(|list| !list.is_empty());
                if has_purchases {
                    return Err(
                        "That holding's purchases changed, so its quantity wasn't replaced."
                            .to_string(),
                    );
                }
                object.insert("Amount".into(), number(*amount)?);
            }
            Op::Add {
                name,
                ticker,
                class,
                amount,
                purchases,
            } => {
                let on_record = positions.iter().any(|position| {
                    position
                        .get("Ticker")
                        .and_then(Value::as_str)
                        .is_some_and(|own| own.trim().eq_ignore_ascii_case(ticker.trim()))
                });
                if on_record {
                    return Err(format!(
                        "{ticker} is already on record, so it wasn't added a second time."
                    ));
                }
                let mut map = Map::new();
                if let Some(name) = name {
                    map.insert("Name".into(), Value::String(name.clone()));
                }
                map.insert("AssetClass".into(), Value::String(class.clone()));
                map.insert("Ticker".into(), Value::String(ticker.clone()));
                map.insert("Amount".into(), number(*amount)?);
                if !purchases.is_empty() {
                    map.insert(
                        "Purchases".into(),
                        Value::Array(
                            purchases
                                .iter()
                                .map(purchase_json)
                                .collect::<Result<_, _>>()?,
                        ),
                    );
                }
                positions.push(Value::Object(map));
            }
        }
    }
    serde_json::to_string_pretty(&positions).map_err(|error| error.to_string())
}

fn find<'a>(positions: &'a mut [Value], id: &str) -> Result<&'a mut Value, String> {
    positions
        .iter_mut()
        .find(|position| position.get("Id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| "A holding this change is about is no longer on record.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSITIONS: &str = r#"[
      {"Id": "pos_spy", "Name": "S&P 500", "AssetClass": "Stocks", "Ticker": "SPY", "Amount": 80.0,
       "Purchases": [{"Date": "2024/01/10", "Quantity": 80.0, "Price": 410.0}]},
      {"Id": "pos_vwce", "Name": "All-World", "AssetClass": "Stocks", "Ticker": "VWCE.DE", "Amount": 12.5,
       "Currency": "EUR"}
    ]"#;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    fn read(json: &str) -> Vec<Value> {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn a_purchase_is_added_once_and_everything_else_is_kept() {
        let buy = Purchase {
            date: Some(day(2026, 11, 10)),
            quantity: 3.0,
            price: Some(512.4),
            fees: Some(1.0),
        };
        let op = Op::Append {
            id: "pos_spy".into(),
            purchases: vec![buy.clone()],
            opening: false,
        };
        let once = apply(POSITIONS, std::slice::from_ref(&op)).unwrap();
        let twice = apply(&once, &[op]).unwrap();
        assert_eq!(
            read(&once),
            read(&twice),
            "the same purchase is not added twice"
        );
        let positions = read(&once);
        let purchases = positions[0]["Purchases"].as_array().unwrap();
        assert_eq!(purchases.len(), 2);
        assert_eq!(
            purchases[0]["Date"], "2024/01/10",
            "the old one is untouched"
        );
        assert_eq!(purchases[1]["Date"], "2026-11-10");
        assert_eq!(purchases[1]["Price"], 512.4);
        assert_eq!(purchases[1]["Fees"], 1.0);
        assert_eq!(
            positions[1]["Currency"], "EUR",
            "an unrelated field survives"
        );
    }

    #[test]
    fn a_holding_without_purchases_keeps_what_it_held() {
        let op = Op::Append {
            id: "pos_vwce".into(),
            purchases: vec![Purchase {
                date: Some(day(2026, 11, 3)),
                quantity: 2.0,
                price: None,
                fees: None,
            }],
            opening: true,
        };
        let positions = read(&apply(POSITIONS, &[op]).unwrap());
        let purchases = positions[1]["Purchases"].as_array().unwrap();
        assert_eq!(purchases.len(), 2);
        assert_eq!(purchases[0], json!({"Quantity": 12.5}));
        assert_eq!(purchases[1], json!({"Date": "2026-11-03", "Quantity": 2.0}));
    }

    #[test]
    fn an_amount_is_replaced_only_where_there_are_no_purchases() {
        let set = |id: &str| Op::SetAmount {
            id: id.into(),
            amount: 14.0,
        };
        let positions = read(&apply(POSITIONS, &[set("pos_vwce")]).unwrap());
        assert_eq!(positions[1]["Amount"], 14.0);
        assert!(apply(POSITIONS, &[set("pos_spy")]).is_err());
        assert!(apply(POSITIONS, &[set("pos_gone")]).is_err());
    }

    #[test]
    fn a_new_holding_is_added_at_the_end() {
        let op = Op::Add {
            name: Some("Nordic Timber".into()),
            ticker: "NTMB.ST".into(),
            class: "Stocks".into(),
            amount: 40.0,
            purchases: Vec::new(),
        };
        let positions = read(&apply(POSITIONS, &[op]).unwrap());
        assert_eq!(positions.len(), 3);
        assert_eq!(positions[2]["Ticker"], "NTMB.ST");
        assert_eq!(positions[2]["Amount"], 40.0);
        assert!(positions[2].get("Purchases").is_none());
    }
}
