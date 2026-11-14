//! Where the portfolio could go: a seeded fan of monthly paths, and the assumptions behind it.
//!
//! This is a model, not a forecast. It says what a range of ordinary outcomes looks like on
//! assumptions the owner can read and change, never what will happen. Every figure it returns
//! comes from the same inputs in the same order, so the same page always draws the same fan.

use std::f64::consts::TAU;

/// How many paths the fan is drawn from. Enough for a stable tenth and ninetieth percentile,
/// few enough to rebuild on a slider step without a pause.
pub const PATHS: usize = 300;

/// The seed every run starts from. Paths get their own streams from it, so a path never
/// depends on the ones before it.
const SEED: u64 = 0x5EED_57A1_1E00;

/// One asset class's yearly assumption.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Assumption {
    /// The expected yearly return, as a fraction: `0.05` is five percent.
    pub annual_return: f64,
    /// How far a year typically strays from that return, as a fraction.
    pub volatility: f64,
}

impl Assumption {
    /// The monthly drift and standard deviation a yearly assumption implies.
    fn monthly(self) -> (f64, f64) {
        let growth = (1.0 + self.annual_return).max(0.0).powf(1.0 / 12.0);
        (growth - 1.0, self.volatility.max(0.0) / 12.0f64.sqrt())
    }
}

const fn assumed(annual_return: f64, volatility: f64) -> Assumption {
    Assumption {
        annual_return,
        volatility,
    }
}

/// The bank's conservative long-run assumptions, one row per class: an average year, and how
/// far an ordinary year strays from it. These are starting points, not forecasts, and the
/// policy's `[outlook]` table can replace any of them.
pub const DEFAULTS: &[(&str, Assumption)] = &[
    ("Stocks", assumed(0.05, 0.16)),
    ("Bonds", assumed(0.03, 0.06)),
    ("Gold", assumed(0.02, 0.15)),
    ("Crypto", assumed(0.04, 0.60)),
    ("Cash", assumed(0.02, 0.005)),
];

/// A class the table doesn't name: modest growth and ordinary risk.
pub const OTHER: Assumption = assumed(0.03, 0.10);

/// What a class was assumed to do, and whether that came from the owner's policy.
#[derive(Clone, Debug, PartialEq)]
pub struct Used {
    pub asset_class: String,
    pub assumption: Assumption,
    pub from_policy: bool,
}

/// The owner's overrides, read from `[outlook.assumptions.<class>]` in the policy.
///
/// ```toml
/// [outlook.assumptions.Stocks]
/// return = 0.06
/// volatility = 0.17
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Assumptions {
    overrides: Vec<(String, Override)>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Override {
    annual_return: Option<f64>,
    volatility: Option<f64>,
}

impl Assumptions {
    /// The policy's `[outlook]` table, if it has one. A table or a key that can't be read is
    /// left alone: the bank's assumption stands rather than the page refusing to draw.
    pub fn from_policy_toml(text: &str) -> Self {
        let Ok(policy) = toml::from_str::<toml::Value>(text) else {
            return Self::default();
        };
        let Some(table) = policy
            .get("outlook")
            .and_then(|outlook| outlook.get("assumptions"))
            .and_then(toml::Value::as_table)
        else {
            return Self::default();
        };
        let mut overrides = Vec::new();
        for (name, entry) in table {
            let Some(entry) = entry.as_table() else {
                continue;
            };
            let annual_return =
                number(entry.get("return")).filter(|value| value.is_finite() && *value > -1.0);
            let volatility =
                number(entry.get("volatility")).filter(|value| value.is_finite() && *value >= 0.0);
            if annual_return.is_none() && volatility.is_none() {
                continue;
            }
            overrides.push((
                name.clone(),
                Override {
                    annual_return,
                    volatility,
                },
            ));
        }
        overrides.sort_by(|(a, _), (b, _)| a.cmp(b));
        Self { overrides }
    }

    /// What to assume for `asset_class`, and whether the owner set it.
    pub fn for_class(&self, asset_class: &str) -> (Assumption, bool) {
        let base = DEFAULTS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(asset_class))
            .map_or(OTHER, |(_, assumption)| *assumption);
        let Some((_, over)) = self
            .overrides
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(asset_class))
        else {
            return (base, false);
        };
        (
            Assumption {
                annual_return: over.annual_return.unwrap_or(base.annual_return),
                volatility: over.volatility.unwrap_or(base.volatility),
            },
            true,
        )
    }

    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }
}

/// A number from the policy, whether it was written as `0.06` or as `6`.
fn number(value: Option<&toml::Value>) -> Option<f64> {
    let value = value?;
    value
        .as_float()
        .or_else(|| value.as_integer().map(|value| value as f64))
}

/// A class falling by a share of itself in one year, on top of the ordinary path.
#[derive(Clone, Debug, PartialEq)]
pub struct Shock {
    pub asset_class: String,
    /// How far it falls, in percent: `30.0` is a fall of thirty percent.
    pub fall_percent: f64,
    /// The year it happens in, counting the year we are in as 1.
    pub year: u32,
}

/// Everything a fan is drawn from.
#[derive(Clone, Debug, PartialEq)]
pub struct Inputs {
    /// What each asset class is worth today.
    pub today: Vec<(String, f64)>,
    /// What is added every month, spread over the classes as they are today.
    pub monthly: f64,
    pub years: u32,
    pub goal: Option<f64>,
    pub shock: Option<Shock>,
    pub assumptions: Assumptions,
}

impl Inputs {
    /// The horizon in months, kept to the range the page offers.
    pub fn months(&self) -> usize {
        self.years.clamp(1, 30) as usize * 12
    }

    fn weights(&self) -> Vec<f64> {
        let total: f64 = self.today.iter().map(|(_, value)| value.max(0.0)).sum();
        if total > 0.0 {
            self.today
                .iter()
                .map(|(_, value)| value.max(0.0) / total)
                .collect()
        } else if self.today.is_empty() {
            Vec::new()
        } else {
            vec![1.0 / self.today.len() as f64; self.today.len()]
        }
    }
}

/// The tenth, median and ninetieth percentile of the paths at one month.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub low: f64,
    pub median: f64,
    pub high: f64,
}

/// A simulated fan. `months[0]` is today.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outlook {
    pub months: Vec<Band>,
    /// The share of paths at or above the goal at the horizon, from 0 to 1.
    pub reached: Option<f64>,
    /// The assumptions used, in the order the classes were given.
    pub used: Vec<Used>,
}

impl Outlook {
    pub fn horizon(&self) -> Option<Band> {
        self.months.last().copied()
    }
}

/// Draw the fan.
pub fn simulate(inputs: &Inputs) -> Outlook {
    let months = inputs.months();
    let weights = inputs.weights();
    let monthly = inputs.monthly.max(0.0);
    let used: Vec<Used> = inputs
        .today
        .iter()
        .map(|(asset_class, _)| {
            let (assumption, from_policy) = inputs.assumptions.for_class(asset_class);
            Used {
                asset_class: asset_class.clone(),
                assumption,
                from_policy,
            }
        })
        .collect();
    let factors: Vec<(f64, f64)> = used.iter().map(|used| used.assumption.monthly()).collect();
    let shock = inputs.shock.as_ref();
    let shock_month = shock.map(|shock| (shock.year.max(1) as usize - 1) * 12);
    let fall = shock.map(|shock| (shock.fall_percent / 100.0).clamp(0.0, 1.0));
    let shock_at = shock.and_then(|shock| {
        inputs
            .today
            .iter()
            .position(|(name, _)| name.eq_ignore_ascii_case(&shock.asset_class))
    });

    let mut columns: Vec<Vec<f64>> = (0..=months).map(|_| Vec::with_capacity(PATHS)).collect();
    let mut finals = Vec::with_capacity(PATHS);
    for path in 0..PATHS {
        let mut rng = stream(path);
        let mut values: Vec<f64> = inputs
            .today
            .iter()
            .map(|(_, value)| value.max(0.0))
            .collect();
        // A fall in the first year is already behind us today.
        if shock_month == Some(0)
            && let (Some(at), Some(fall)) = (shock_at, fall)
        {
            values[at] *= 1.0 - fall;
        }
        columns[0].push(values.iter().sum());
        for (month, column) in columns.iter_mut().enumerate().skip(1) {
            for index in 0..values.len() {
                let (drift, sigma) = factors[index];
                let stray = normal(&mut rng) * sigma;
                values[index] = (values[index] * (1.0 + drift + stray).max(0.0))
                    + monthly * weights.get(index).copied().unwrap_or(0.0);
            }
            if shock_month == Some(month)
                && let (Some(at), Some(fall)) = (shock_at, fall)
            {
                values[at] *= 1.0 - fall;
            }
            column.push(values.iter().sum());
        }
        finals.push(columns[months].last().copied().unwrap_or(0.0));
    }

    let bands: Vec<Band> = columns
        .iter_mut()
        .map(|column| {
            column.sort_by(f64::total_cmp);
            Band {
                low: percentile(column, 0.1),
                median: percentile(column, 0.5),
                high: percentile(column, 0.9),
            }
        })
        .collect();
    let reached = inputs.goal.map(|goal| {
        if finals.is_empty() {
            return 0.0;
        }
        finals.iter().filter(|value| **value >= goal).count() as f64 / finals.len() as f64
    });
    Outlook {
        months: bands,
        reached,
        used,
    }
}

/// The value at `p` of the way through a sorted column, interpolating between its neighbours.
fn percentile(sorted: &[f64], p: f64) -> f64 {
    match sorted.len() {
        0 => 0.0,
        1 => sorted[0],
        len => {
            let at = p.clamp(0.0, 1.0) * (len - 1) as f64;
            let low = at.floor() as usize;
            let high = at.ceil() as usize;
            let part = at - low as f64;
            sorted[low] + (sorted[high] - sorted[low]) * part
        }
    }
}

/// Each path's own stream, so the fan is the same however many paths are drawn.
fn stream(path: usize) -> u64 {
    splitmix(SEED ^ (path as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

fn splitmix(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// A standard normal from the path's stream, by Box–Muller.
fn normal(rng: &mut u64) -> f64 {
    let uniform = |rng: &mut u64| {
        *rng = splitmix(*rng);
        ((*rng >> 11) as f64 / (1u64 << 53) as f64).max(f64::MIN_POSITIVE)
    };
    let (first, second) = (uniform(rng), uniform(rng));
    (-2.0 * first.ln()).sqrt() * (TAU * second).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(today: &[(&str, f64)], monthly: f64, years: u32) -> Inputs {
        Inputs {
            today: today
                .iter()
                .map(|(name, value)| ((*name).to_string(), *value))
                .collect(),
            monthly,
            years,
            goal: None,
            shock: None,
            assumptions: Assumptions::default(),
        }
    }

    fn steady(mut inputs: Inputs, annual_return: f64) -> Inputs {
        inputs.assumptions = Assumptions::from_policy_toml(&format!(
            "[outlook.assumptions.Stocks]\nreturn = {annual_return}\nvolatility = 0.0\n\
             [outlook.assumptions.Crypto]\nreturn = {annual_return}\nvolatility = 0.0\n"
        ));
        inputs
    }

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() < 0.01, "{left} != {right}");
    }

    #[test]
    fn a_steady_return_compounds_to_the_same_figure_on_every_path() {
        let outlook = simulate(&steady(inputs(&[("Stocks", 1_000.0)], 0.0, 1), 0.12));
        assert_eq!(outlook.months.len(), 13);
        let last = outlook.horizon().unwrap();
        close(last.low, 1_120.0);
        close(last.median, 1_120.0);
        close(last.high, 1_120.0);
    }

    #[test]
    fn with_no_return_only_the_contributions_grow_it() {
        let outlook = simulate(&steady(inputs(&[("Stocks", 0.0)], 100.0, 1), 0.0));
        for (month, band) in outlook.months.iter().enumerate() {
            close(band.median, month as f64 * 100.0);
        }
        let last = outlook.horizon().unwrap();
        close(last.low, 1_200.0);
        close(last.high, 1_200.0);
    }

    #[test]
    fn a_shock_falls_at_the_start_of_its_year_and_stays_fallen() {
        let mut shock = inputs(&[("Crypto", 1_000.0)], 0.0, 2);
        shock.shock = Some(Shock {
            asset_class: "Crypto".to_string(),
            fall_percent: 50.0,
            year: 2,
        });
        let outlook = simulate(&steady(shock, 0.0));
        close(outlook.months[11].median, 1_000.0);
        close(outlook.months[12].median, 500.0);
        close(outlook.horizon().unwrap().median, 500.0);
    }

    #[test]
    fn the_same_inputs_draw_the_same_fan() {
        let first = simulate(&inputs(&[("Stocks", 1_000.0), ("Bonds", 400.0)], 50.0, 3));
        let second = simulate(&inputs(&[("Stocks", 1_000.0), ("Bonds", 400.0)], 50.0, 3));
        assert_eq!(first, second);
    }

    #[test]
    fn every_month_brackets_its_median_and_starts_where_the_portfolio_is() {
        let outlook = simulate(&inputs(&[("Stocks", 1_000.0), ("Bonds", 500.0)], 100.0, 5));
        close(outlook.months[0].median, 1_500.0);
        assert_eq!(outlook.months[0].low, outlook.months[0].high);
        for band in &outlook.months {
            assert!(band.low <= band.median && band.median <= band.high);
        }
        assert!(outlook.horizon().unwrap().high > outlook.months[0].high);
    }

    #[test]
    fn a_goal_is_the_share_of_paths_that_reach_it() {
        let mut outlook = steady(inputs(&[("Stocks", 0.0)], 100.0, 1), 0.0);
        outlook.goal = Some(1_200.0);
        let reached = simulate(&outlook).reached.unwrap();
        close(reached, 1.0);
        outlook.goal = Some(1_201.0);
        close(simulate(&outlook).reached.unwrap(), 0.0);
        outlook.goal = None;
        assert_eq!(simulate(&outlook).reached, None);
    }

    #[test]
    fn the_policy_replaces_only_the_numbers_it_names() {
        let assumptions = Assumptions::from_policy_toml(
            "[outlook.assumptions.Stocks]\nreturn = 0.06\n\n\
             [outlook.assumptions.Bonds]\nvolatility = 0.02\n",
        );
        let (stocks, from_policy) = assumptions.for_class("Stocks");
        assert!(from_policy);
        close(stocks.annual_return, 0.06);
        close(stocks.volatility, 0.16);
        let (bonds, _) = assumptions.for_class("Bonds");
        close(bonds.annual_return, 0.03);
        close(bonds.volatility, 0.02);
        let (gold, from_policy) = assumptions.for_class("Gold");
        assert!(!from_policy);
        close(gold.annual_return, 0.02);
        let (art, _) = assumptions.for_class("Art");
        close(art.annual_return, OTHER.annual_return);
    }

    #[test]
    fn a_policy_without_an_outlook_table_changes_nothing() {
        let plain = Assumptions::from_policy_toml("version = \"1.0\"\nname = \"Balanced\"\n");
        assert!(plain.is_empty());
        assert_eq!(
            plain.for_class("Stocks"),
            (DEFAULTS[0].1, false),
            "the bank's assumption stands"
        );
        assert!(Assumptions::from_policy_toml("not toml at all").is_empty());
    }
}
