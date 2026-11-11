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
use portfolio_rs::{
    AppConfig, FixtureProvider, services::quotes::AnyProvider, state::AppState, workspace,
};
use semver::Version;
use sha2::{Digest, Sha256};
use tokio::runtime::Handle;

use crate::{
    backend::Backend,
    update::{self, Asset, Installation, Pending, Release, ReleaseSource, Updater},
};

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

/// The demo's model: canned answers, handed over at once, so the demo, the screenshots and the
/// tests are deterministic and never touch the network. The page stages their arrival.
pub struct ScriptedModel;

impl crate::ask::Model for ScriptedModel {
    fn ask(&self, request: crate::ask::Request) -> crate::ask::Reply {
        crate::ask::Reply::Ready(scripted(&request.question))
    }
}

/// The answer to `question`, as JSON Lines. Questions are matched on the words a person would
/// use; anything else gets the demo's own version of "I can't answer that".
pub fn scripted(question: &str) -> String {
    let question = question.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| question.contains(word));
    if has(&["half", "halve", "halves", "what if", "fall"]) && has(&["crypto", "bitcoin"]) {
        return SCRIPT_HALVES.to_string();
    }
    if has(&["2,000", "2000", "new money", "invest", "where should"]) {
        return SCRIPT_NEW_MONEY.to_string();
    }
    if has(&["allocation", "off", "drift", "target"]) {
        return SCRIPT_ALLOCATION.to_string();
    }
    if has(&["bitcoin", "btc"]) {
        return SCRIPT_BITCOIN.to_string();
    }
    if has(&["changed", "month", "since", "last look", "week"]) {
        return SCRIPT_CHANGE.to_string();
    }
    SCRIPT_ELSE.to_string()
}

/// A month of small moves, and the one class worth a look.
const SCRIPT_CHANGE: &str = concat!(
    r#"{"block":"say","text":"Since {since.date}, the total has moved by {since.change}. {since.leader} did most of the work, which will surprise nobody."}"#,
    "\n",
    r#"{"block":"figures","ids":["total","since.change","today.change","cash.value"]}"#,
    "\n",
    r#"{"block":"chart","period":"month"}"#,
    "\n",
    r#"{"block":"say","text":"The rest is quiet. Crypto is {class.crypto.share} of the portfolio against a target of {class.crypto.target}, and that is the one number worth your attention."}"#,
    "\n",
    r#"{"block":"go","steps":[{"label":"Why is crypto over its target?","ask":"Why is crypto over its target?"},{"label":"Open Allocation","page":"allocation"}]}"#,
    "\n",
);

/// A shock, and what it would leave.
const SCRIPT_HALVES: &str = concat!(
    r#"{"block":"say","text":"A halving is the kind of day worth planning for. Here is what it would leave, and what a move toward your targets would ask of you."}"#,
    "\n",
    r#"{"block":"scenario","shocks":[{"class":"Crypto","percent":-50}]}"#,
    "\n",
    r#"{"block":"say","text":"Nothing needs signing today. The plan is there for when it does."}"#,
    "\n",
    r#"{"block":"go","steps":[{"label":"Open Rebalance","page":"rebalance"},{"label":"What is my crypto target?","ask":"What is my crypto target?"}]}"#,
    "\n",
);

/// New money, placed by buying only.
const SCRIPT_NEW_MONEY: &str = concat!(
    r#"{"block":"say","text":"Two thousand will not undo the drift, and it does not have to. It goes where it does the most good, buying only."}"#,
    "\n",
    r#"{"block":"figures","ids":["total","cash.value"]}"#,
    "\n",
    r#"{"block":"plan","amount":2000}"#,
    "\n",
    r#"{"block":"go","steps":[{"label":"Why is my allocation off?","ask":"Why is my allocation off?"}]}"#,
    "\n",
);

/// Why the shares moved away from the policy.
const SCRIPT_ALLOCATION: &str = concat!(
    r#"{"block":"say","text":"Because prices moved and you did not. Crypto grew to {class.crypto.share} of the portfolio against a target of {class.crypto.target}, and stocks fell to {class.stocks.share} against {class.stocks.target}."}"#,
    "\n",
    r#"{"block":"allocation"}"#,
    "\n",
    r#"{"block":"say","text":"Nothing urgent. A move part of the way back would take the edge off it before the month is out."}"#,
    "\n",
    r#"{"block":"plan","fraction":0.5}"#,
    "\n",
    r#"{"block":"go","steps":[{"label":"Where should new money go?","ask":"Where should new money go?"}]}"#,
    "\n",
);

/// One holding, in its own figures.
const SCRIPT_BITCOIN: &str = concat!(
    r#"{"block":"say","text":"Bitcoin is {holding.pos_btc.value}, or {class.crypto.share} of the portfolio. Today it moved {holding.pos_btc.today}."}"#,
    "\n",
    r#"{"block":"figures","ids":["holding.pos_btc.value","holding.pos_btc.today","class.crypto.share","class.crypto.drift"]}"#,
    "\n",
    r#"{"block":"holdings","class":"Crypto","sort":"value"}"#,
    "\n",
    r#"{"block":"go","steps":[{"label":"What if crypto halves?","ask":"What if crypto halves?"},{"label":"Why is crypto over its target?","ask":"Why is crypto over its target?"}]}"#,
    "\n",
);

/// What the demo says when the question is outside what it was written for.
const SCRIPT_ELSE: &str = concat!(
    r#"{"block":"say","text":"That one needs a model. This portfolio can answer what changed, where new money goes, and what a shock would leave. Add a model in Settings; a local one keeps everything on this computer."}"#,
    "\n",
    r#"{"block":"figures","ids":["total","today.change"]}"#,
    "\n",
    r#"{"block":"go","steps":[{"label":"What changed this month?","ask":"What changed this month?"},{"label":"Open Settings","page":"settings"}]}"#,
    "\n",
);

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
    // Each holding's own drift per day, swing and phase, so their lines differ. Every one lands
    // on today's price.
    let paths = [
        (0.0030, 0.05, 1.3),
        (0.0012, 0.02, 0.0),
        (-0.0006, 0.015, 2.4),
        (0.0016, 0.012, 4.1),
    ];
    let today = Local::now().date_naive();
    let lines: Vec<String> = (1..=90)
        .rev()
        .map(|days_ago: i64| {
            let date = today - Duration::days(days_ago);
            let mut positions = vec![
                r#"{"positionId":"pos_cash","kind":"cash","assetClass":"Cash","quantity":30000.0,"valueBase":30000.0}"#
                    .to_string(),
            ];
            let mut total = 30000.0;
            for ((id, class, quantity, price, currency, fx), (drift, swing, phase)) in
                holdings.into_iter().zip(paths)
            {
                let wave = 1.0 - days_ago as f64 * drift
                    + ((days_ago as f64 / 8.0 + phase).sin() - phase.sin()) * swing;
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

    // The owner last looked five days ago, so the Overview has something to tell them.
    let day = |days_ago| (today - Duration::days(days_ago)).format("%Y-%m-%d");
    fs::write(
        history.join("seen.json"),
        format!("{{\"last\":\"{}\",\"before\":\"{}\"}}\n", day(5), day(12)),
    )
    .expect("demo workspace is writable");
}

/// A workspace as a new user has it: the files `portfolio_rs` sets up, and no positions.
pub fn seed_empty_workspace(dir: &Path) {
    fs::create_dir_all(dir).expect("demo workspace is writable");
    workspace::init_workspace(&dir.to_string_lossy(), false).expect("demo workspace is writable");
    fs::write(dir.join("positions.json"), "[]\n").expect("demo workspace is writable");
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
