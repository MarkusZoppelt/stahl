//! Every made-up broker file in `tests/intake/`, read and compared with the demo portfolio.
//! The files imitate real shapes with invented names and numbers.

use std::{fs, path::Path};

use chrono::NaiveDate;
use serde_json::Value;
use stahl::{
    demo,
    intake::{
        self, Outcome,
        apply::{self, Choice},
        reconcile::{Bought, Change, Holding, Ledger, Match, reconcile},
        rules::{Aside, Kind, Reading, Shape},
    },
};

fn bytes(name: &str) -> Vec<u8> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/intake")
        .join(name);
    fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn read(name: &str) -> (Reading, intake::text::Sheet) {
    match intake::understand(&bytes(name)) {
        Outcome::Read { reading, sheet, .. } => (reading, sheet),
        other => panic!("{name} should read: {other:?}"),
    }
}

fn unreadable(name: &str) -> String {
    match intake::understand(&bytes(name)) {
        Outcome::Unreadable(why) => why,
        other => panic!("{name} should be unreadable: {other:?}"),
    }
}

/// The demo's quote currencies, as the app knows them from its quotes.
fn currency(ticker: &str) -> Option<String> {
    match ticker {
        "BTC-USD" => Some("USD".to_string()),
        "SPY" | "TLT" | "GLD" => Some("EUR".to_string()),
        _ => None,
    }
}

/// Positions JSON as reconciling sees it.
fn holdings(json: &str) -> Vec<Holding> {
    let positions: Vec<Value> = serde_json::from_str(json).unwrap();
    positions
        .iter()
        .map(|position| {
            let purchases: Vec<Bought> = position["Purchases"]
                .as_array()
                .map(|list| {
                    list.iter()
                        .map(|purchase| Bought {
                            date: purchase["Date"].as_str().and_then(|date| {
                                NaiveDate::parse_from_str(date, "%Y-%m-%d")
                                    .or_else(|_| NaiveDate::parse_from_str(date, "%Y/%m/%d"))
                                    .ok()
                            }),
                            quantity: purchase["Quantity"].as_f64().unwrap(),
                            price: purchase["Price"].as_f64(),
                        })
                        .collect()
                })
                .unwrap_or_default();
            let quantity = if purchases.is_empty() {
                position["Amount"].as_f64().unwrap()
            } else {
                purchases.iter().map(|purchase| purchase.quantity).sum()
            };
            let ticker = position["Ticker"].as_str().map(String::from);
            Holding {
                id: position["Id"].as_str().unwrap_or("pos_new").to_string(),
                name: position["Name"].as_str().map(String::from),
                currency: ticker.as_deref().and_then(currency),
                ticker,
                class: position["AssetClass"].as_str().unwrap().to_string(),
                quantity,
                purchases,
            }
        })
        .collect()
}

/// Give every position an id, as `portfolio_rs` does on load.
fn with_ids(json: &str) -> String {
    let mut positions: Vec<Value> = serde_json::from_str(json).unwrap();
    for (index, position) in positions.iter_mut().enumerate() {
        if position.get("Id").is_none() {
            position["Id"] = Value::String(format!("pos_new{index}"));
        }
    }
    serde_json::to_string(&positions).unwrap()
}

fn ledger(name: &str) -> Ledger {
    let (reading, _) = read(name);
    reconcile(&reading, &holdings(demo::POSITIONS))
}

/// Accept everything that can be, the way the approval list's "accept all" and individual
/// accepts would, and write it.
fn accept_ready(json: &str, ledger: &Ledger) -> String {
    let ops: Vec<_> = ledger
        .proposals
        .iter()
        .filter(|proposal| proposal.ready)
        .filter_map(|proposal| apply::op(proposal, &Choice::of(proposal)))
        .collect();
    with_ids(&apply::apply(json, &ops).unwrap())
}

#[test]
fn a_german_transactions_export_is_read_and_reconciled() {
    let (reading, sheet) = read("de-transactions.csv");
    assert_eq!(sheet.delimiter, ';');
    assert_eq!(reading.shape, Shape::Transactions);
    assert_eq!(reading.entries.len(), 3);
    let spy = &reading.entries[0];
    assert_eq!(spy.kind, Some(Kind::Buy));
    assert_eq!(spy.quantity.value, 3.0);
    let price = spy.price.as_ref().unwrap();
    assert_eq!(price.value, 512.4);
    assert_eq!(price.text, "512,40");
    // Where it came from, exactly: the line and the bytes of it.
    assert_eq!(
        &sheet.lines[price.at.line][price.at.start..price.at.end],
        "512,40"
    );
    assert_eq!(spy.fees.as_ref().unwrap().value, 1.0);
    assert_eq!(
        spy.date.as_ref().unwrap().date,
        NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()
    );
    assert_eq!(spy.currency.as_ref().unwrap().text, "EUR");
    assert_eq!(
        reading.entries[2].quantity.value, -5.0,
        "a sale is negative"
    );
    assert!(
        reading
            .aside
            .iter()
            .any(|aside| matches!(&aside.why, Aside::NotATrade(word) if word == "Ausschüttung")),
        "the payout is not a trade: {:?}",
        reading.aside
    );
    assert!(reading.aside.iter().any(|aside| aside.why == Aside::Total));

    let ledger = reconcile(&reading, &holdings(demo::POSITIONS));
    assert_eq!(ledger.proposals.len(), 3, "{:#?}", ledger.proposals);
    let Change::Trades {
        holding,
        trades,
        opening,
    } = &ledger.proposals[0].change
    else {
        panic!("a purchase on SPY");
    };
    assert_eq!(holding.id, "pos_spy");
    assert_eq!(holding.by, Match::Ticker);
    assert!(!opening);
    assert_eq!(trades.len(), 1);
    assert!(ledger.proposals[0].in_all);
    assert!(
        matches!(&ledger.proposals[1].change, Change::Add { holding, .. } if holding.ticker.as_ref().unwrap().text == "NGSC")
    );
    assert!(
        matches!(&ledger.proposals[2].change, Change::Trades { holding, .. } if holding.id == "pos_gld")
    );
}

#[test]
fn dropping_the_same_file_twice_proposes_nothing_the_second_time() {
    for name in [
        "de-transactions.csv",
        "us-transactions.csv",
        "unrelated.csv",
    ] {
        let first = ledger(name);
        assert!(!first.proposals.is_empty(), "{name}");
        let after = accept_ready(&with_ids(demo::POSITIONS), &first);
        let (reading, _) = read(name);
        let second = reconcile(&reading, &holdings(&after));
        let still: Vec<_> = second
            .proposals
            .iter()
            .filter(|proposal| proposal.ready)
            .collect();
        assert!(still.is_empty(), "{name}: {still:#?}");
        assert!(second.agreed > 0, "{name}");
    }
}

#[test]
fn a_us_export_reads_month_first_dates_and_leaves_out_a_price_in_the_wrong_currency() {
    let (reading, sheet) = read("us-transactions.csv");
    assert_eq!(sheet.delimiter, ',');
    assert_eq!(reading.entries.len(), 3);
    assert_eq!(
        reading.entries[0].date.as_ref().unwrap().date,
        NaiveDate::from_ymd_opt(2026, 11, 25).unwrap()
    );
    assert_eq!(
        reading.entries[2].date.as_ref().unwrap().date,
        NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
        "the column's other dates say it is month first"
    );
    assert_eq!(reading.entries[2].price.as_ref().unwrap().value, 61_000.0);
    assert_eq!(reading.entries[0].currency.as_ref().unwrap().text, "USD");

    let ledger = reconcile(&reading, &holdings(demo::POSITIONS));
    let tlt = ledger
        .proposals
        .iter()
        .find_map(|proposal| match &proposal.change {
            Change::Trades {
                holding, trades, ..
            } if holding.id == "pos_tlt" => Some((proposal, trades)),
            _ => None,
        })
        .expect("the TLT sale");
    assert!(
        tlt.1[0].price_left_out,
        "dollars against a euro quote are left out"
    );
    assert_eq!(tlt.1[0].recorded_price(), None);
    assert!(tlt.0.doubts.iter().any(|doubt| doubt.contains("left out")));
    let btc = ledger
        .proposals
        .iter()
        .find_map(|proposal| match &proposal.change {
            Change::Trades {
                holding, trades, ..
            } if holding.id == "pos_btc" => Some(trades),
            _ => None,
        })
        .expect("the bitcoin purchase");
    assert_eq!(
        btc[0].recorded_price(),
        Some(61_000.0),
        "dollars against a dollar quote are kept"
    );
}

#[test]
fn a_holdings_list_reports_differences_and_what_it_leaves_out() {
    let ledger = ledger("uk-holdings.csv");
    let mut quantity = 0;
    let mut missing = Vec::new();
    let mut added = 0;
    for proposal in &ledger.proposals {
        match &proposal.change {
            Change::Quantity {
                holding,
                held,
                settable,
            } => {
                quantity += 1;
                assert_eq!(holding.id, "pos_spy");
                assert_eq!(held.value, 83.0);
                assert!(
                    !settable,
                    "the record keeps purchases, so the file can't settle it"
                );
                assert!(!proposal.ready);
            }
            Change::Missing { holding } => missing.push(holding.id.clone()),
            Change::Add { held, .. } => {
                added += 1;
                assert_eq!(held.as_ref().unwrap().value, 250.0);
            }
            Change::Trades { .. } => panic!("a holdings list has no trades"),
        }
    }
    assert_eq!(quantity, 1);
    assert_eq!(added, 1);
    missing.sort();
    assert_eq!(
        missing,
        ["pos_btc", "pos_gld"],
        "cash isn't a broker's to report"
    );
    assert_eq!(ledger.agreed, 1, "TLT agrees");
}

#[test]
fn a_depot_overview_without_tickers_is_matched_by_name_with_the_doubt_said() {
    let (reading, _) = read("de-depot.csv");
    assert_eq!(reading.shape, Shape::Holdings);
    assert_eq!(reading.as_of, NaiveDate::from_ymd_opt(2026, 11, 12));
    let ledger = reconcile(&reading, &holdings(demo::POSITIONS));
    assert_eq!(
        ledger.agreed, 2,
        "the S&P fund and gold agree, by name: {:#?}",
        ledger.proposals
    );
    let newcomer = ledger
        .proposals
        .iter()
        .find(|proposal| matches!(proposal.change, Change::Add { .. }))
        .expect("the bonds are new: the names don't match");
    assert!(!newcomer.ready, "no ticker, so it can't be priced");
    assert!(newcomer.doubts.iter().any(|doubt| doubt.contains("ticker")));
}

#[test]
fn a_name_match_is_shown_as_one_and_never_taken_by_accept_all() {
    let ledger = ledger("name-only.csv");
    assert_eq!(ledger.proposals.len(), 2);
    for proposal in &ledger.proposals {
        let Change::Trades { holding, .. } = &proposal.change else {
            panic!("both match by name: {proposal:#?}");
        };
        assert!(matches!(holding.by, Match::Name { .. }));
        assert!(!proposal.in_all);
        assert!(proposal.doubts[0].starts_with("Matched by name only"));
    }
}

#[test]
fn a_utf16_tab_separated_list_that_agrees_proposes_nothing() {
    let (reading, sheet) = read("tab-holdings.tsv");
    assert_eq!(sheet.delimiter, '\t');
    let ledger = reconcile(&reading, &holdings(demo::POSITIONS));
    assert!(ledger.proposals.is_empty(), "{:#?}", ledger.proposals);
    assert_eq!(ledger.agreed, 4);
}

#[test]
fn duplicates_and_partial_overlaps() {
    let ledger = ledger("duplicates.csv");
    assert_eq!(ledger.agreed, 1, "the 2024 purchase is on record");
    assert_eq!(ledger.proposals.len(), 1);
    let proposal = &ledger.proposals[0];
    let Change::Trades { trades, .. } = &proposal.change else {
        panic!("purchases on SPY");
    };
    assert_eq!(trades.len(), 2, "both copies are shown");
    assert!(!proposal.in_all, "a repeat is for the owner to decide");
    assert!(proposal.doubts.iter().any(|doubt| doubt.contains("twice")));
}

#[test]
fn a_sale_larger_than_the_holding_cannot_be_recorded() {
    let ledger = ledger("oversell.csv");
    assert_eq!(ledger.proposals.len(), 1);
    assert!(!ledger.proposals[0].ready);
}

#[test]
fn a_file_unrelated_to_the_portfolio_says_so_and_removes_nothing() {
    let ledger = ledger("unrelated.csv");
    assert_eq!(ledger.notes.len(), 1);
    assert!(ledger.notes[0].starts_with("Nothing in this file matches"));
    assert_eq!(ledger.proposals.len(), 2);
    assert!(
        ledger
            .proposals
            .iter()
            .all(|proposal| matches!(proposal.change, Change::Add { .. })),
        "nothing on record is reported missing from somebody else's account"
    );
}

#[test]
fn rows_that_cannot_be_read_with_certainty_are_set_aside_with_a_reason() {
    let (reading, _) = read("odd-rows.csv");
    assert_eq!(reading.entries.len(), 1);
    let why: Vec<String> = reading
        .aside
        .iter()
        .map(|aside| match &aside.why {
            Aside::Unsure(why) => why.clone(),
            other => panic!("{other:?}"),
        })
        .collect();
    assert!(why[0].contains("Umtausch"), "{why:?}");
    assert!(why[1].contains("doesn't name a security"), "{why:?}");

    let (reading, _) = read("ambiguous-dates.csv");
    assert!(reading.entries.is_empty());
    assert!(reading.aside.iter().all(
        |aside| matches!(&aside.why, Aside::Unsure(why) if why.contains("day-first or month-first"))
    ));
}

#[test]
fn a_bare_table_asks_which_column_is_which() {
    match intake::understand(&bytes("bare.csv")) {
        Outcome::Unmapped { missing, .. } => assert_eq!(missing, ["which column is the quantity"]),
        other => panic!("{other:?}"),
    }
}

#[test]
fn files_that_cannot_be_read_say_why() {
    assert!(unreadable("statement.pdf").starts_with("This is a PDF"));
    assert!(unreadable("workbook.xlsx").contains("workbook"));
    assert_eq!(unreadable("empty.csv"), "The file is empty.");
    assert!(unreadable("not-a-table.txt").starts_with("There is no table"));
    assert!(unreadable("bank-account.csv").starts_with("This looks like an account statement"));
}
