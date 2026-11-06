//! A made-up portfolio with 90 days of history, for the tests and `examples/demo.rs`.
//! Everything goes in a directory the caller owns; prices come from a fixture, never the network.

use std::{fs, path::Path};

use chrono::{Duration, Local};
use portfolio_rs::{AppConfig, FixtureProvider, services::quotes::AnyProvider, state::AppState};
use tokio::runtime::Handle;

use crate::backend::Backend;

pub const POSITIONS: &str = r#"[
  {"Id": "pos_btc", "Name": "Bitcoin", "AssetClass": "Crypto", "Ticker": "BTC-USD", "Amount": 0.45,
   "Purchases": [{"Date": "2025/05/28", "Quantity": 0.45, "Price": 52500.0}]},
  {"Id": "pos_spy", "Name": "S&P 500", "AssetClass": "Stocks", "Ticker": "SPY", "Amount": 80.0,
   "Purchases": [{"Date": "2024/01/10", "Quantity": 80.0, "Price": 410.0}]},
  {"Id": "pos_tlt", "Name": "Long Bonds", "AssetClass": "Bonds", "Ticker": "TLT", "Amount": 60.0,
   "Purchases": [{"Date": "2024/06/03", "Quantity": 60.0, "Price": 95.0}]},
  {"Id": "pos_gld", "Name": "Gold", "AssetClass": "Gold", "Ticker": "GLD", "Amount": 25.0,
   "Purchases": [{"Date": "2025/01/15", "Quantity": 25.0, "Price": 170.0}]},
  {"Id": "pos_cash", "Name": "Savings", "AssetClass": "Cash", "Amount": 30000.0}
]"#;

const POLICY: &str = r#"version = "1.0"
name = "Balanced Growth"
base_currency = "EUR"
time_horizon_years = 15
risk_profile = "moderate"

[constraints]
minimum_cash_amount = 20000.0
single_position_limit_percent = 40.0

[[allocations]]
asset_class = "Stocks"
target_percent = 50.0
tolerance_percent = 5.0

[[allocations]]
asset_class = "Bonds"
target_percent = 15.0
tolerance_percent = 5.0

[[allocations]]
asset_class = "Gold"
target_percent = 5.0
tolerance_percent = 2.0

[[allocations]]
asset_class = "Crypto"
target_percent = 10.0
tolerance_percent = 3.0

[[allocations]]
asset_class = "Cash"
target_percent = 20.0
tolerance_percent = 5.0
"#;

const QUOTES: &str = r#"{
  "latest": {
    "BTC-USD": {"price": 75000.0, "currency": "USD"},
    "EURUSD=X": {"price": 1.25, "currency": "USD"},
    "SPY": {"price": 500.0, "currency": "EUR"},
    "TLT": {"price": 90.0, "currency": "EUR"},
    "GLD": {"price": 200.0, "currency": "EUR"}
  },
  "previous_close": {"BTC-USD": 73750.0, "SPY": 503.0, "TLT": 90.5, "GLD": 196.0}
}"#;

/// Write the demo's prices where `PORTFOLIO_RS_QUOTES=fixture:<path>` can find them.
pub fn write_quotes(path: &Path) {
    fs::write(path, QUOTES).expect("demo directory is writable");
}

/// A workspace whose holdings never change: only prices move, so no money flows in.
pub fn seed_workspace(dir: &Path) {
    let history = dir.join("portfolio/history/main");
    fs::create_dir_all(&history).expect("demo workspace is writable");
    fs::write(dir.join("positions.json"), POSITIONS).expect("demo workspace is writable");
    fs::write(dir.join("portfolio/policy.toml"), POLICY).expect("demo workspace is writable");

    let holdings = [
        ("pos_btc", "Crypto", 0.45, 75000.0, "USD", 0.8),
        ("pos_spy", "Stocks", 80.0, 500.0, "EUR", 1.0),
        ("pos_tlt", "Bonds", 60.0, 90.0, "EUR", 1.0),
        ("pos_gld", "Gold", 25.0, 200.0, "EUR", 1.0),
    ];
    let today = Local::now().date_naive();
    let lines: Vec<String> = (1..=90)
        .rev()
        .map(|days_ago: i64| {
            let date = today - Duration::days(days_ago);
            let wave = 1.0 - days_ago as f64 * 0.0012 + (days_ago as f64 / 8.0).sin() * 0.02;
            let mut positions = vec![
                r#"{"positionId":"pos_cash","kind":"cash","assetClass":"Cash","quantity":30000.0,"valueBase":30000.0}"#
                    .to_string(),
            ];
            let mut total = 30000.0;
            for (id, class, quantity, price, currency, fx) in holdings {
                let price = (price * wave * 100.0_f64).round() / 100.0;
                let value = (quantity * price * fx * 100.0_f64).round() / 100.0;
                total += value;
                positions.push(format!(
                    r#"{{"positionId":"{id}","kind":"market","assetClass":"{class}","quantity":{quantity},"price":{price},"priceCurrency":"{currency}","fxRate":{fx},"valueBase":{value}}}"#
                ));
            }
            format!(
                r#"{{"date":"{date}","baseCurrency":"EUR","positions":[{}],"cashBase":30000.0,"totalBase":{total:.2},"netFlowsBase":0.0,"complete":true,"dataStatus":"complete"}}"#,
                positions.join(",")
            )
        })
        .collect();
    fs::write(history.join("snapshots.jsonl"), lines.join("\n") + "\n")
        .expect("demo workspace is writable");
}

/// A backend on fixture quotes, with its config in `dir`. With `workspace`, it opens that on
/// startup, without writing the user's config.
pub fn backend(dir: &Path, workspace: Option<&Path>, runtime: Handle) -> Backend {
    let mut config = AppConfig::default();
    if let Some(workspace) = workspace {
        config.set_workspace(workspace.to_string_lossy().into_owned());
    }
    let quotes = FixtureProvider::from_json(QUOTES).expect("demo quotes parse");
    Backend::new(
        AppState::with_quotes("EUR".to_string(), AnyProvider::Fixture(Box::new(quotes))),
        config,
        Some(dir.join("config.yml")),
        runtime,
    )
}
