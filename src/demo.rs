//! A made-up portfolio with 90 days of history, for the tests and `examples/demo.rs`.
//! Everything goes in a directory the caller owns; prices come from a fixture, never the network.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use chrono::{Duration, Local};
use portfolio_rs::{AppConfig, FixtureProvider, services::quotes::AnyProvider, state::AppState};
use semver::Version;
use sha2::{Digest, Sha256};
use tokio::runtime::Handle;

use crate::{
    backend::Backend,
    update::{self, Asset, Installation, Pending, Release, ReleaseSource, Updater},
};

pub const POSITIONS: &str = r#"[
  {"Id": "pos_btc", "Name": "Bitcoin", "AssetClass": "Crypto", "Ticker": "BTC-USD", "Amount": 0.45,
   "Purchases": [{"Date": "2025/05/28", "Quantity": 0.45, "Price": 42000.0}]},
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
    "BTC-USD": {"price": 60000.0, "currency": "EUR"},
    "SPY": {"price": 500.0, "currency": "EUR"},
    "TLT": {"price": 90.0, "currency": "EUR"},
    "GLD": {"price": 200.0, "currency": "EUR"}
  },
  "previous_close": {"BTC-USD": 59000.0, "SPY": 503.0, "TLT": 90.5, "GLD": 196.0}
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
        ("pos_btc", "Crypto", 0.45, 60000.0),
        ("pos_spy", "Stocks", 80.0, 500.0),
        ("pos_tlt", "Bonds", 60.0, 90.0),
        ("pos_gld", "Gold", 25.0, 200.0),
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
            for (id, class, quantity, price) in holdings {
                let price = (price * wave * 100.0_f64).round() / 100.0;
                let value = (quantity * price * 100.0_f64).round() / 100.0;
                total += value;
                positions.push(format!(
                    r#"{{"positionId":"{id}","kind":"market","assetClass":"{class}","quantity":{quantity},"price":{price},"priceCurrency":"EUR","fxRate":1.0,"valueBase":{value}}}"#
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

const RELEASE_NOTES: &str = "## What's Changed

- Holdings can be sorted by today's change
- The allocation donut reads out each class under the pointer
- Fixed stale prices after waking from sleep

**Full Changelog**: https://github.com/MarkusZoppelt/stahl/compare/v0.2.0...v0.3.0
";

/// The next minor version after this build, so these stay "newer" as the app's version moves.
pub fn next_version() -> Version {
    let current = update::current_version();
    Version::new(current.major, current.minor + 1, 0)
}

/// A line of the checksum file the release workflow writes for an asset.
pub fn checksum_line(name: &str, contents: &[u8]) -> String {
    let digest: String = Sha256::digest(contents)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{digest}  {name}\n")
}

/// A release source with no network: releases and files from memory, or nothing but failure.
pub struct FakeReleases {
    answer: Result<Vec<Release>, String>,
    files: HashMap<String, Vec<u8>>,
    downloads: AtomicUsize,
}

impl FakeReleases {
    /// One release, with these files attached.
    pub fn offering(version: Version, files: Vec<(String, Vec<u8>)>) -> FakeReleases {
        let assets = files
            .iter()
            .map(|(name, _)| Asset {
                name: name.clone(),
                url: format!("{}/download/v{version}/{name}", update::RELEASES_PAGE),
            })
            .collect();
        FakeReleases {
            answer: Ok(vec![Release {
                page: format!("{}/tag/v{version}", update::RELEASES_PAGE),
                version,
                notes: RELEASE_NOTES.to_string(),
                assets,
            }]),
            files: files.into_iter().collect(),
            downloads: AtomicUsize::new(0),
        }
    }

    /// As if offline.
    pub fn failing() -> FakeReleases {
        FakeReleases {
            answer: Err("can't reach GitHub".to_string()),
            files: HashMap::new(),
            downloads: AtomicUsize::new(0),
        }
    }

    /// How many files anyone has fetched.
    pub fn downloads(&self) -> usize {
        self.downloads.load(Ordering::SeqCst)
    }
}

impl ReleaseSource for FakeReleases {
    fn releases(&self) -> Pending<'_, Vec<Release>> {
        Box::pin(async move { self.answer.clone() })
    }

    fn download<'a>(&'a self, asset: &'a Asset, to: &'a Path) -> Pending<'a, ()> {
        Box::pin(async move {
            self.downloads.fetch_add(1, Ordering::SeqCst);
            let contents = self
                .files
                .get(&asset.name)
                .ok_or_else(|| format!("no file named {}", asset.name))?;
            fs::write(to, contents).map_err(|error| error.to_string())
        })
    }
}

/// An updater for the demo's scenes, with a release waiting that it can never fetch: scenes show
/// the screens, and nothing in them installs.
pub fn updater(installation: Installation) -> Updater {
    let names = [
        "stahl-capital-macos-arm64.dmg",
        "stahl-capital-macos-arm64.dmg.sha256",
        "stahl-capital-windows-x64.zip",
        "stahl-capital-windows-x64.zip.sha256",
        "stahl_0.0.0_amd64.deb",
        "stahl_0.0.0_amd64.deb.sha256",
    ];
    let files = names
        .into_iter()
        .map(|name| (name.to_string(), Vec::new()))
        .collect();
    Updater::new(
        Arc::new(FakeReleases::offering(next_version(), files)),
        installation,
        update::current_version(),
        PathBuf::from("/nonexistent"),
    )
}
