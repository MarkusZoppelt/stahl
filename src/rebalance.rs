//! What-if allocation, with no trades executed.
//!
//! A plan is a split of a fixed total across asset classes. A blend moves every class a
//! fraction of the way to its target; setting one class by hand leaves the difference on
//! cash. New money is placed by buying only, a withdrawal by selling only, each as close to
//! the policy targets as that constraint allows. Every one of them keeps the total.

/// Half-width used when a target has no tolerance of its own, as the simulator does.
pub const DEFAULT_TOLERANCE: f64 = 5.0;

/// A trade smaller than this is dust, not a trade.
const DUST: f64 = 0.01;

/// One asset class the plan can move.
#[derive(Clone, Debug, PartialEq)]
pub struct Class {
    pub name: String,
    pub value: f64,
    /// Target share, in percent. `None` when the policy doesn't name this class.
    pub target: Option<f64>,
    /// Half-width of the tolerance band, in percentage points.
    pub tolerance: Option<f64>,
    pub cash: bool,
}

impl Class {
    pub fn new(name: impl Into<String>, value: f64) -> Self {
        Self {
            name: name.into(),
            value: finite(value),
            target: None,
            tolerance: None,
            cash: false,
        }
    }

    pub fn target(mut self, percent: f64) -> Self {
        self.target = finite_opt(percent);
        self
    }

    pub fn tolerance(mut self, percent: f64) -> Self {
        self.tolerance = finite_opt(percent).map(|percent| percent.max(0.0));
        self
    }

    pub fn cash(mut self) -> Self {
        self.cash = true;
        self
    }

    fn held(&self) -> f64 {
        finite(self.value).max(0.0)
    }

    fn band(&self) -> Option<f64> {
        self.target
            .map(|_| self.tolerance.unwrap_or(DEFAULT_TOLERANCE))
    }
}

/// Buy or sell. A plan never does both to one class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Increase,
    Reduce,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Self::Increase => "Increase",
            Self::Reduce => "Reduce",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trade {
    pub asset_class: String,
    pub action: Action,
    pub amount: f64,
    /// The trade as a share of the plan's total, in percentage points.
    pub percent: f64,
}

/// A split of `total` and the trades that would get there from where the classes are.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub values: Vec<f64>,
    pub total: f64,
    /// Money added, or withdrawn when negative. Zero when the plan only moves money around.
    pub external: f64,
    pub trades: Vec<Trade>,
}

/// Why new money, or a withdrawal, can't be placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaceError {
    /// No classes at all.
    Empty,
    /// Nothing has a target, so buying or selling can't get closer to one.
    NoTargets,
    /// A withdrawal larger than the portfolio.
    MoreThanPortfolio,
    /// Funding the withdrawal would mean selling a class the policy doesn't name.
    WouldSellUntargeted,
}

impl PlaceError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Empty => "There's nothing to put this in.",
            Self::NoTargets => "No class has a target, so there's nowhere this should go.",
            Self::MoreThanPortfolio => "That's more than the portfolio.",
            Self::WouldSellUntargeted => "That would mean selling a class with no target.",
        }
    }
}

fn finite(value: f64) -> f64 {
    if value.is_finite() { value } else { 0.0 }
}

fn finite_opt(value: f64) -> Option<f64> {
    value.is_finite().then_some(value)
}

fn total_of(classes: &[Class]) -> f64 {
    classes.iter().map(Class::held).sum()
}

/// Whether `value` is inside the class's tolerance band, at `total`.
/// `None` when the class has no target, or there is no portfolio to take a share of.
pub fn inside_band(class: &Class, value: f64, total: f64) -> Option<bool> {
    let (target, band) = (class.target?, class.band()?);
    if total <= 0.0 {
        return None;
    }
    let share = finite(value).max(0.0) / total * 100.0;
    // The edge is inside. A fraction of a point covers the rounding of share * 100.
    Some((share - target).abs() <= band + 1e-6)
}

/// Where each class sits when every target is met, in money. The total doesn't change: the
/// classes with a target share out what the classes without one leave. `None` of them all
/// leaves the money where it is.
pub fn at_targets(classes: &[Class]) -> Vec<f64> {
    let total = total_of(classes);
    let held: f64 = classes
        .iter()
        .filter(|class| class.target.is_none())
        .map(Class::held)
        .sum();
    let named: f64 = classes.iter().filter_map(|class| class.target).sum();
    let budget = (total - held).max(0.0);
    classes
        .iter()
        .map(|class| match class.target {
            Some(target) if named > 0.0 => target.max(0.0) / named * budget,
            _ => class.held(),
        })
        .collect()
}

/// A fraction of the way from what each class holds to its target. `0` is the portfolio as
/// it is, `1` is every target met. A class with no target stays where it is. The total never
/// changes.
pub fn blend(classes: &[Class], fraction: f64) -> Plan {
    let fraction = finite(fraction).clamp(0.0, 1.0);
    let to = at_targets(classes);
    let values = classes
        .iter()
        .enumerate()
        .map(|(index, class)| {
            let held = class.held();
            match class.target {
                Some(_) => held + (to[index] - held) * fraction,
                None => held,
            }
        })
        .collect();
    plan(classes, values, 0.0)
}

/// Set one class to `want`, with cash taking up the difference, and the total unchanged.
///
/// A class can't go below zero and neither can cash, so it rises only as far as the cash
/// there is. Without a cash class, or when cash is the class being set, the others give up or
/// take up the difference in proportion to what they hold.
pub fn set_class(classes: &[Class], values: &[f64], index: usize, want: f64) -> Vec<f64> {
    let mut out: Vec<f64> = values
        .iter()
        .copied()
        .map(|value| finite(value).max(0.0))
        .collect();
    if out.len() != classes.len() || index >= out.len() {
        return out;
    }
    let total: f64 = out.iter().sum();
    let held = out[index];
    let want = finite(want).clamp(0.0, total);
    let cash = classes
        .iter()
        .position(|class| class.cash)
        .filter(|cash| *cash != index);
    match cash {
        Some(cash) => {
            // The class can only rise as far as the cash that can pay for it.
            let want = want.min(held + out[cash]);
            out[cash] += held - want;
            out[index] = want;
        }
        None => {
            let others = total - held;
            if others <= 0.0 || (want - held).abs() < 1e-12 {
                return out;
            }
            let keep = ((total - want) / others).max(0.0);
            for (at, value) in out.iter_mut().enumerate() {
                if at != index {
                    *value *= keep;
                }
            }
            out[index] = want;
        }
    }
    out
}

fn trades(classes: &[Class], values: &[f64], total: f64) -> Vec<Trade> {
    classes
        .iter()
        .enumerate()
        .filter_map(|(index, class)| {
            let next = values.get(index).copied().unwrap_or(0.0);
            let delta = next - class.held();
            if delta.abs() < DUST {
                return None;
            }
            Some(Trade {
                asset_class: class.name.clone(),
                action: if delta > 0.0 {
                    Action::Increase
                } else {
                    Action::Reduce
                },
                amount: delta.abs(),
                percent: if total > 0.0 {
                    delta.abs() / total * 100.0
                } else {
                    0.0
                },
            })
        })
        .collect()
}

/// A plan with these class values. Lengths that don't match are ignored.
pub fn from_values(classes: &[Class], values: Vec<f64>, external: f64) -> Plan {
    if values.len() != classes.len() {
        return today(classes);
    }
    plan(classes, values, external)
}

fn plan(classes: &[Class], values: Vec<f64>, external: f64) -> Plan {
    let total = values.iter().sum();
    let trades = trades(classes, &values, total);
    Plan {
        values,
        total,
        external,
        trades,
    }
}

/// The portfolio as it is: no trades.
pub fn today(classes: &[Class]) -> Plan {
    plan(classes, classes.iter().map(Class::held).collect(), 0.0)
}

/// Split `amount` of new money by buying only, or withdraw it by selling only.
///
/// Either way, only classes with a target are traded, and the result is the closest
/// the new total can get to those targets without a trade in the forbidden direction.
pub fn place(classes: &[Class], amount: f64) -> Result<Plan, PlaceError> {
    if classes.is_empty() {
        return Err(PlaceError::Empty);
    }
    let amount = finite(amount);
    if amount.abs() < DUST {
        return Ok(today(classes));
    }
    if !classes.iter().any(|class| class.target.is_some()) {
        return Err(PlaceError::NoTargets);
    }
    let total = total_of(classes);
    let new_total = total + amount;
    // A cent past the portfolio is already more than there is.
    if amount < 0.0 && -amount - total > 1e-6 {
        return Err(PlaceError::MoreThanPortfolio);
    }
    let untargeted: f64 = classes
        .iter()
        .filter(|class| class.target.is_none())
        .map(Class::held)
        .sum();
    if amount < 0.0 && untargeted - new_total > 1e-6 {
        return Err(PlaceError::WouldSellUntargeted);
    }
    let budget = new_total - untargeted;
    let ideals: Vec<f64> = classes
        .iter()
        .map(|class| class.target.unwrap_or(0.0) / 100.0 * new_total)
        .collect();
    let values = if amount > 0.0 {
        solve(classes, &ideals, budget, true)
    } else {
        solve(classes, &ideals, budget, false)
    };
    Ok(plan(classes, values, amount))
}

/// `final = max(held, ideal + μ)` when buying, `min(held, max(floor, ideal + μ))` when selling.
/// `μ` is chosen so the targeted classes sum to `budget`.
fn solve(classes: &[Class], ideals: &[f64], budget: f64, buy: bool) -> Vec<f64> {
    let at = |mu: f64| -> f64 {
        classes
            .iter()
            .enumerate()
            .filter(|(_, class)| class.target.is_some())
            .map(|(index, class)| projected(class.held(), ideals[index], mu, buy))
            .sum()
    };
    let mut lo = -budget.abs() - total_of(classes) - 1.0;
    let mut hi = budget.abs() + total_of(classes) + 1.0;
    for _ in 0..64 {
        let mid = (lo + hi) / 2.0;
        if at(mid) < budget {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let mu = (lo + hi) / 2.0;
    classes
        .iter()
        .enumerate()
        .map(|(index, class)| {
            if class.target.is_none() {
                class.held()
            } else {
                projected(class.held(), ideals[index], mu, buy)
            }
        })
        .collect()
}

fn projected(held: f64, ideal: f64, mu: f64, buy: bool) -> f64 {
    if buy {
        held.max(ideal + mu)
    } else {
        held.min((ideal + mu).max(0.0))
    }
}

/// The plan as Markdown, for notes. `money` formats one amount.
pub fn to_markdown(
    plan: &Plan,
    classes: &[Class],
    heading: &str,
    detail: &str,
    policy: &str,
    currency: &str,
    money: impl Fn(f64) -> String,
) -> String {
    let mut out = String::new();
    out.push_str("# Rebalance\n\n");
    if !policy.is_empty() {
        out.push_str(&format!("Toward \"{policy}\"\n\n"));
    }
    out.push_str(&format!(
        "Portfolio value: {} {currency}\n\n",
        money(plan.total - plan.external)
    ));
    out.push_str(&format!("## {heading}\n\n"));
    if !detail.is_empty() {
        out.push_str(detail);
        out.push_str("\n\n");
    }
    if plan.trades.is_empty() {
        out.push_str("No trades.\n\n");
    } else {
        out.push_str("| Asset class | Action | Amount | Share |\n");
        out.push_str("| --- | --- | --- | --- |\n");
        for trade in &plan.trades {
            out.push_str(&format!(
                "| {} | {} | {} {currency} | {:.1}% |\n",
                trade.asset_class,
                trade.action.label(),
                money(trade.amount),
                trade.percent
            ));
        }
        out.push('\n');
    }
    if plan.external >= DUST {
        out.push_str(&format!(
            "New money: {} {currency}. Buying only, selling nothing.\n\n",
            money(plan.external)
        ));
    } else if plan.external <= -DUST {
        out.push_str(&format!(
            "Withdrawal: {} {currency}. Selling only, buying nothing.\n\n",
            money(-plan.external)
        ));
    } else {
        out.push_str("No money in or out.\n\n");
    }
    if !classes.is_empty() && plan.total > 0.0 {
        out.push_str("Afterwards:");
        for (class, value) in classes.iter().zip(&plan.values) {
            out.push_str(&format!(
                " {} {:.1}%",
                class.name,
                value / plan.total * 100.0
            ));
        }
        out.push_str(".\n\n");
    }
    out.push_str("> Stahl never trades. This is a plan, not an order.\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-6, "{left} != {right}");
    }

    fn values_approx(values: &[f64], want: &[f64]) {
        assert_eq!(values.len(), want.len());
        for (value, want) in values.iter().zip(want) {
            approx(*value, *want);
        }
    }

    /// Stocks 80, Bonds 20, targets 50/50. Total 100.
    fn drifted() -> Vec<Class> {
        vec![
            Class::new("Stocks", 80.0).target(50.0).tolerance(5.0),
            Class::new("Bonds", 20.0).target(50.0).tolerance(5.0),
        ]
    }

    /// The demo's shape, scaled to 100 so the figures are easy to read.
    /// Stocks 40 (target 50 ± 5), Bonds 5 (15 ± 5), Gold 5 (5 ± 2),
    /// Crypto 25 (10 ± 3), Cash 25 (20 ± 5).
    fn book() -> Vec<Class> {
        vec![
            Class::new("Stocks", 40.0).target(50.0).tolerance(5.0),
            Class::new("Bonds", 5.0).target(15.0).tolerance(5.0),
            Class::new("Gold", 5.0).target(5.0).tolerance(2.0),
            Class::new("Crypto", 25.0).target(10.0).tolerance(3.0),
            Class::new("Cash", 25.0).target(20.0).tolerance(5.0).cash(),
        ]
    }

    #[test]
    fn a_blend_moves_every_class_the_same_share_of_the_way_and_keeps_the_total() {
        let classes = book();
        let plan = blend(&classes, 0.5);
        // Stocks 40 -> 50 and Crypto 25 -> 10, halfway; Gold is already on target.
        values_approx(&plan.values, &[45.0, 10.0, 5.0, 17.5, 22.5]);
        approx(plan.total, 100.0);
        values_approx(&blend(&classes, 0.0).values, &[40.0, 5.0, 5.0, 25.0, 25.0]);
        values_approx(&blend(&classes, 1.0).values, &[50.0, 15.0, 5.0, 10.0, 20.0]);
        // Out of range is the nearest end, not a different plan.
        values_approx(&blend(&classes, -3.0).values, &[40.0, 5.0, 5.0, 25.0, 25.0]);
        values_approx(&blend(&classes, 9.0).values, &[50.0, 15.0, 5.0, 10.0, 20.0]);
    }

    #[test]
    fn a_blend_leaves_a_class_without_a_target_where_it_is() {
        let mut classes = drifted();
        classes.push(Class::new("Art", 20.0));
        let plan = blend(&classes, 1.0);
        approx(plan.values[2], 20.0);
        approx(plan.total, 120.0);
        // The two that have a target share what Art leaves: 100 of the 120.
        values_approx(&plan.values[..2], &[50.0, 50.0]);
        assert!(
            plan.trades
                .iter()
                .all(|trade| trade.asset_class != "Art" || trade.amount < 1e-9)
        );
    }

    #[test]
    fn a_portfolio_already_on_its_targets_has_nothing_to_trade() {
        let mut classes = book();
        for (class, value) in classes.iter_mut().zip([50.0, 15.0, 5.0, 10.0, 20.0]) {
            class.value = value;
        }
        assert!(blend(&classes, 1.0).trades.is_empty());
        assert_eq!(blend(&[Class::new("Art", 10.0)], 0.5).values, vec![10.0]);
    }

    #[test]
    fn setting_a_class_takes_the_difference_from_cash_and_keeps_the_total() {
        let classes = book();
        let start: Vec<f64> = classes.iter().map(Class::held).collect();
        // Stocks 40 -> 50: the 10 comes out of Cash's 25.
        let moved = set_class(&classes, &start, 0, 50.0);
        values_approx(&moved, &[50.0, 5.0, 5.0, 25.0, 15.0]);
        approx(moved.iter().sum(), 100.0);
        // And the other way: cash takes the 5 that Crypto gives up.
        let back = set_class(&classes, &start, 3, 20.0);
        values_approx(&back, &[40.0, 5.0, 5.0, 20.0, 30.0]);
    }

    #[test]
    fn setting_a_class_stops_where_cash_runs_out_and_never_goes_negative() {
        let classes = book();
        let start: Vec<f64> = classes.iter().map(Class::held).collect();
        // 25 of cash is all the Crypto there can be on top of the 25 it holds.
        values_approx(
            &set_class(&classes, &start, 3, 80.0),
            &[40.0, 5.0, 5.0, 50.0, 0.0],
        );
        assert!(set_class(&classes, &start, 0, 1_000.0)[4] >= 0.0);
        let emptied = set_class(&classes, &start, 3, -5.0);
        approx(emptied[3], 0.0);
        approx(emptied[4], 50.0);
        assert!(emptied.iter().all(|value| *value >= 0.0));
    }

    #[test]
    fn setting_a_class_with_no_cash_class_moves_the_others_in_proportion() {
        let classes = vec![
            Class::new("Stocks", 60.0).target(50.0),
            Class::new("Bonds", 20.0).target(30.0),
            Class::new("Gold", 20.0).target(20.0),
        ];
        let start: Vec<f64> = classes.iter().map(Class::held).collect();
        let moved = set_class(&classes, &start, 0, 40.0);
        // The 20 Stocks gives up is shared by Bonds and Gold as 60:40 of what they hold.
        values_approx(&moved, &[40.0, 30.0, 30.0]);
        approx(moved.iter().sum(), 100.0);
        // Setting cash itself is the same as having no cash: the others take it up.
        let book = book();
        let start: Vec<f64> = book.iter().map(Class::held).collect();
        let cash = set_class(&book, &start, 4, 10.0);
        approx(cash.iter().sum(), 100.0);
        assert!((cash[4] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn setting_a_class_ignores_a_mismatched_book() {
        let classes = book();
        let short = vec![1.0, 2.0];
        assert_eq!(set_class(&classes, &short, 0, 5.0), short);
        assert_eq!(
            set_class(&classes, &[1.0; 5], 9, 5.0),
            vec![1.0; 5],
            "an index past the end changes nothing"
        );
    }

    #[test]
    fn the_targets_are_where_a_full_rebalance_lands() {
        let classes = book();
        values_approx(&at_targets(&classes), &[50.0, 15.0, 5.0, 10.0, 20.0]);
        let plan = blend(&classes, 1.0);
        values_approx(&plan.values, &at_targets(&classes));
        // A class with no target keeps what it holds, and the rest share what is left.
        let mut with_art = drifted();
        with_art.push(Class::new("Art", 20.0));
        values_approx(&at_targets(&with_art), &[50.0, 50.0, 20.0]);
    }

    #[test]
    fn new_money_never_sells_and_a_withdrawal_never_buys_at_any_amount() {
        let classes = book();
        for step in 1..=60 {
            let amount = f64::from(step) * 7.5;
            let plan = place(&classes, amount).unwrap();
            approx(plan.values.iter().sum(), 100.0 + amount);
            for (class, value) in classes.iter().zip(&plan.values) {
                assert!(
                    *value >= class.value - 1e-9,
                    "{} sold at {amount}",
                    class.name
                );
            }
            let out = place(&classes, -amount.min(99.0)).unwrap();
            approx(out.values.iter().sum(), 100.0 - amount.min(99.0));
            for (class, value) in classes.iter().zip(&out.values) {
                assert!(*value <= class.value + 1e-9 && *value >= 0.0);
            }
        }
    }

    #[test]
    fn inside_a_band_includes_the_edge_and_ignores_a_class_with_no_target() {
        let class = Class::new("Gold", 7.0).target(5.0).tolerance(2.0);
        assert_eq!(inside_band(&class, 7.0, 100.0), Some(true));
        assert_eq!(inside_band(&class, 7.1, 100.0), Some(false));
        let loose = Class::new("Art", 10.0);
        assert_eq!(inside_band(&loose, 10.0, 100.0), None);
        assert_eq!(inside_band(&class, 5.0, 0.0), None);
    }

    #[test]
    fn a_small_amount_goes_where_it_most_reduces_the_gap() {
        // Gold is only a little under its target, and already closer than Stocks and Bonds
        // can be brought, so the money goes to those two and Gold is left alone.
        let plan = place(&book(), 2.0).unwrap();
        values_approx(&plan.values, &[41.35, 5.65, 5.0, 25.0, 25.0]);
    }

    #[test]
    fn an_amount_too_small_to_fix_the_drift_buys_the_gap_and_sells_nothing() {
        let plan = place(&drifted(), 10.0).unwrap();
        values_approx(&plan.values, &[80.0, 30.0]);
        assert!(
            plan.trades
                .iter()
                .all(|trade| trade.action == Action::Increase)
        );
        assert!(plan.trades.iter().any(|trade| trade.asset_class == "Bonds"));
        assert!(
            !plan
                .trades
                .iter()
                .any(|trade| trade.asset_class == "Stocks")
        );
        approx(plan.total, 110.0);
        approx(plan.external, 10.0);
    }

    #[test]
    fn an_amount_large_enough_lands_exactly_on_target() {
        // Stocks at 80 needs a new total of at least 160 before 50% covers it.
        let short = place(&drifted(), 59.0).unwrap();
        assert!(short.values[0] > short.total * 0.5 + 0.1);
        let exact = place(&drifted(), 60.0).unwrap();
        values_approx(&exact.values, &[80.0, 80.0]);
        let larger = place(&drifted(), 100.0).unwrap();
        values_approx(&larger.values, &[100.0, 100.0]);
    }

    #[test]
    fn a_class_with_no_target_is_left_alone() {
        let mut classes = drifted();
        classes.insert(1, Class::new("Art", 20.0));
        // Targets are 50/50 of the new total, but Art's 20 can't be sold.
        let plan = place(&classes, 40.0).unwrap();
        approx(plan.values[1], 20.0);
        assert!(!plan.trades.iter().any(|trade| trade.asset_class == "Art"));
        assert!(plan.values.iter().sum::<f64>() - plan.total < 1e-6);
        let withdrawn = place(&classes, -10.0).unwrap();
        approx(withdrawn.values[1], 20.0);
        assert!(
            withdrawn
                .trades
                .iter()
                .all(|trade| trade.action == Action::Reduce)
        );
    }

    #[test]
    fn an_empty_portfolio_lands_on_the_targets() {
        let classes = vec![
            Class::new("Stocks", 0.0).target(60.0),
            Class::new("Bonds", 0.0).target(40.0),
        ];
        let plan = place(&classes, 1_000.0).unwrap();
        values_approx(&plan.values, &[600.0, 400.0]);
        assert!(
            plan.trades
                .iter()
                .all(|trade| trade.action == Action::Increase)
        );
        assert!(place(&[], 1_000.0).is_err());
        assert_eq!(place(&classes, -1.0), Err(PlaceError::MoreThanPortfolio));
    }

    #[test]
    fn a_withdrawal_is_the_mirror_and_a_larger_one_is_refused() {
        let plan = place(&drifted(), -20.0).unwrap();
        values_approx(&plan.values, &[60.0, 20.0]);
        assert!(
            plan.trades
                .iter()
                .all(|trade| trade.action == Action::Reduce)
        );
        approx(plan.external, -20.0);
        assert_eq!(
            place(&drifted(), -100.01),
            Err(PlaceError::MoreThanPortfolio)
        );
        let mut with_art = drifted();
        with_art.push(Class::new("Art", 50.0));
        // 120 is less than the portfolio, but more than the classes that have a target.
        assert_eq!(
            place(&with_art, -120.0),
            Err(PlaceError::WouldSellUntargeted)
        );
        assert_eq!(
            place(&[Class::new("Art", 40.0)], 10.0),
            Err(PlaceError::NoTargets)
        );
    }

    #[test]
    fn a_full_blend_buys_and_sells_the_same_amount() {
        let plan = blend(&book(), 1.0);
        let bought: f64 = plan
            .trades
            .iter()
            .filter(|trade| trade.action == Action::Increase)
            .map(|trade| trade.amount)
            .sum();
        let sold: f64 = plan
            .trades
            .iter()
            .filter(|trade| trade.action == Action::Reduce)
            .map(|trade| trade.amount)
            .sum();
        approx(bought, 20.0);
        approx(sold, 20.0);
    }

    #[test]
    fn dust_is_not_a_trade_and_zero_is_today() {
        let mut classes = drifted();
        classes[0].value = 50.004;
        classes[1].value = 49.996;
        let plan = blend(&classes, 1.0);
        assert!(plan.trades.is_empty());
        let same = place(&drifted(), 0.0).unwrap();
        assert!(same.trades.is_empty());
        approx(same.external, 0.0);
    }

    #[test]
    fn the_markdown_names_the_plan_and_says_it_never_trades() {
        let plan = blend(&drifted(), 1.0);
        let text = to_markdown(
            &plan,
            &drifted(),
            "Full rebalance to targets",
            "Rebalance all asset classes to policy targets.",
            "Balanced Growth",
            "EUR",
            |value| format!("{value:.2}"),
        );
        assert!(text.contains("Toward \"Balanced Growth\""));
        assert!(text.contains("| Bonds | Increase |"));
        assert!(text.contains("| Stocks | Reduce |"));
        assert!(text.contains("No money in or out."));
        assert!(text.contains("Stahl never trades."));
        let invested = place(&drifted(), 60.0).unwrap();
        let text = to_markdown(&invested, &drifted(), "New money", "", "", "EUR", |value| {
            format!("{value:.2}")
        });
        assert!(text.contains("Buying only, selling nothing."));
    }
}
