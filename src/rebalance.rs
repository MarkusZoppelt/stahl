//! What-if allocation, with no trades executed.
//!
//! A plan is a split of a fixed total across asset classes. Dragging a boundary
//! moves money between the two classes it separates. New money is placed by
//! buying only, a withdrawal by selling only, each as close to the policy
//! targets as that constraint allows.

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

/// The result of dragging one boundary, including a magnet hold if it snapped.
#[derive(Clone, Debug, PartialEq)]
pub struct Drag {
    pub values: Vec<f64>,
    /// The left class's value while a magnet is holding the boundary, so the caller can
    /// keep holding until the pointer leaves.
    pub held: Option<f64>,
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

/// No class goes below this. Cash, in particular, cannot go below zero.
pub fn floor_of(class: &Class) -> f64 {
    let _ = class.cash;
    0.0
}

fn floors(classes: &[Class]) -> Vec<f64> {
    classes.iter().map(floor_of).collect()
}

/// Move the boundary between class `index` and the next so the left class becomes `want`,
/// clamped so neither side goes below its floor. Every other class stays. The total stays.
pub fn move_boundary(values: &[f64], floors: &[f64], index: usize, want: f64) -> Vec<f64> {
    let mut out: Vec<f64> = values
        .iter()
        .copied()
        .map(finite)
        .map(|value| value.max(0.0))
        .collect();
    let Some(right) = index.checked_add(1).filter(|right| *right < out.len()) else {
        return out;
    };
    let pair = out[index] + out[right];
    let floor_l = floors.get(index).copied().unwrap_or(0.0).max(0.0);
    let floor_r = floors.get(right).copied().unwrap_or(0.0).max(0.0);
    if floor_l + floor_r > pair {
        return out;
    }
    let left = finite(want).clamp(floor_l, pair - floor_r);
    out[index] = left;
    out[right] = pair - left;
    out
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

/// Left-class values at which either adjacent class would sit on its target, if that
/// point is reachable without taking the other below its floor.
pub fn snap_points(classes: &[Class], values: &[f64], index: usize, total: f64) -> Vec<f64> {
    let Some(right) = index
        .checked_add(1)
        .filter(|right| *right < classes.len() && *right < values.len())
    else {
        return Vec::new();
    };
    if total <= 0.0 {
        return Vec::new();
    }
    let pair = finite(values[index]).max(0.0) + finite(values[right]).max(0.0);
    let floor_l = floor_of(&classes[index]);
    let floor_r = floor_of(&classes[right]);
    let mut points = Vec::new();
    if let Some(target) = classes[index].target {
        let at = target / 100.0 * total;
        if (floor_l..=pair - floor_r).contains(&at) {
            points.push(at);
        }
    }
    if let Some(target) = classes[right].target {
        let at = pair - target / 100.0 * total;
        if (floor_l..=pair - floor_r).contains(&at) {
            points.push(at);
        }
    }
    points
}

/// The nearer snap point, when `want` is within `engage_share` of one.
pub fn nearest_snap(want: f64, points: &[f64], total: f64, engage_share: f64) -> Option<f64> {
    if total <= 0.0 || engage_share < 0.0 {
        return None;
    }
    let engage = engage_share * total;
    points
        .iter()
        .copied()
        .filter(|point| (want - point).abs() <= engage)
        .min_by(|a, b| {
            (want - a)
                .abs()
                .partial_cmp(&(want - b).abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

/// Drag the boundary at `index` toward `want` for the left class.
///
/// `held` is the snap point from the previous move. It keeps holding until `want` is
/// further than `release_share` away, so the boundary doesn't flicker at the edge.
pub fn drag(
    classes: &[Class],
    values: &[f64],
    index: usize,
    want: f64,
    engage_share: f64,
    held: Option<f64>,
    release_share: f64,
) -> Drag {
    let total = values
        .iter()
        .copied()
        .map(finite)
        .map(|value| value.max(0.0))
        .sum();
    let floors = floors(classes);
    let clamped = move_boundary(values, &floors, index, want);
    let want = clamped.get(index).copied().unwrap_or(want);
    let release = release_share.max(engage_share) * total;
    if let Some(held) = held
        && total > 0.0
        && (want - held).abs() <= release
    {
        return Drag {
            values: move_boundary(values, &floors, index, held),
            held: Some(held),
        };
    }
    let points = snap_points(classes, values, index, total);
    if let Some(point) = nearest_snap(want, &points, total, engage_share) {
        return Drag {
            values: move_boundary(values, &floors, index, point),
            held: Some(point),
        };
    }
    Drag {
        values: clamped,
        held: None,
    }
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

/// Every class with a target goes to that target. A class with none is sold.
/// Targets that don't sum to 100 are scaled so the result still sums to the portfolio.
pub fn full(classes: &[Class]) -> Plan {
    let total = total_of(classes);
    let target_sum: f64 = classes.iter().filter_map(|class| class.target).sum();
    let values = if total <= 0.0 || target_sum <= 0.0 {
        classes.iter().map(Class::held).collect()
    } else {
        classes
            .iter()
            .map(|class| class.target.unwrap_or(0.0) / target_sum * total)
            .collect()
    };
    plan(classes, values, 0.0)
}

/// Classes outside their band move halfway to the target. The rest stay.
/// The result is scaled so it still sums to the portfolio.
pub fn conservative(classes: &[Class]) -> Plan {
    let total = total_of(classes);
    if total <= 0.0 {
        return plan(classes, classes.iter().map(|_| 0.0).collect(), 0.0);
    }
    let mut values: Vec<f64> = classes
        .iter()
        .map(|class| {
            let actual = class.held() / total * 100.0;
            let Some(target) = class.target else {
                return class.held();
            };
            let drift = actual - target;
            let next = if drift.abs() > class.band().unwrap_or(DEFAULT_TOLERANCE) {
                actual - drift / 2.0
            } else {
                actual
            };
            next / 100.0 * total
        })
        .collect();
    let sum: f64 = values.iter().sum();
    if sum > 0.0 {
        let scale = total / sum;
        for value in &mut values {
            *value *= scale;
        }
    }
    plan(classes, values, 0.0)
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
    fn a_drag_moves_only_the_two_classes_and_keeps_the_total() {
        let classes = book();
        let start: Vec<f64> = classes.iter().map(Class::held).collect();
        // Crypto and Cash share 50. Dragging their boundary can't touch the others.
        let moved = move_boundary(&start, &floors(&classes), 3, 40.0);
        values_approx(&moved, &[40.0, 5.0, 5.0, 40.0, 10.0]);
        approx(moved.iter().sum(), 100.0);
    }

    #[test]
    fn a_drag_cannot_make_a_class_or_cash_negative() {
        let classes = book();
        let start: Vec<f64> = classes.iter().map(Class::held).collect();
        let drained = move_boundary(&start, &floors(&classes), 3, 1_000.0);
        assert!(drained.iter().all(|value| *value >= 0.0));
        approx(drained[3] + drained[4], 50.0);
        approx(drained[4], 0.0);
        let emptied = move_boundary(&start, &floors(&classes), 3, -40.0);
        approx(emptied[3], 0.0);
        approx(emptied[4], 50.0);
        assert!(classes[4].cash);
    }

    #[test]
    fn a_boundary_snaps_to_the_nearer_reachable_target_and_holds() {
        let classes = drifted();
        let start = [80.0, 20.0];
        // Stocks' target is 50, Bonds' is 50, so both snap points are 50.
        let near = drag(&classes, &start, 0, 52.0, 0.03, None, 0.05);
        values_approx(&near.values, &[50.0, 50.0]);
        assert_eq!(near.held, Some(50.0));
        // Still holding, a little past the engage distance.
        let held = drag(&classes, &start, 0, 54.0, 0.03, near.held, 0.05);
        assert_eq!(held.held, Some(50.0));
        // Far enough, and it lets go.
        let released = drag(&classes, &start, 0, 70.0, 0.03, near.held, 0.05);
        values_approx(&released.values, &[70.0, 30.0]);
        assert_eq!(released.held, None);
    }

    #[test]
    fn an_unreachable_target_is_not_a_snap_point() {
        // Bonds holds 5, so Stocks cannot reach its target of 50 by moving this boundary.
        let classes = book();
        let start: Vec<f64> = classes.iter().map(Class::held).collect();
        let points = snap_points(&classes, &start, 0, 100.0);
        assert!(points.iter().all(|point| (*point - 50.0).abs() > 1.0));
        // Bonds can reach its own target of 15: Stocks would be 40 + 5 - 15 = 30.
        assert!(points.iter().any(|point| (*point - 30.0).abs() < 1e-6));
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
    fn a_full_rebalance_lands_on_target_and_keeps_the_total() {
        let plan = full(&book());
        values_approx(&plan.values, &[50.0, 15.0, 5.0, 10.0, 20.0]);
        approx(plan.total, 100.0);
        approx(plan.external, 0.0);
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
        approx(bought, sold);
    }

    #[test]
    fn a_full_rebalance_sells_a_class_with_no_target() {
        let mut classes = drifted();
        classes.push(Class::new("Art", 20.0));
        let plan = full(&classes);
        values_approx(&plan.values, &[60.0, 60.0, 0.0]);
        assert!(
            plan.trades
                .iter()
                .any(|trade| { trade.asset_class == "Art" && trade.action == Action::Reduce })
        );
    }

    #[test]
    fn a_conservative_plan_moves_only_what_is_outside_its_band() {
        let plan = conservative(&book());
        // Gold is inside ±2 of 5, so it barely moves (only the scaling that keeps the total).
        assert!((plan.values[2] - 5.0).abs() < 0.2);
        // Stocks was 10 points under, so it moves toward 50 but not all the way.
        assert!(plan.values[0] > 40.0 && plan.values[0] < 50.0);
        // Crypto was 15 points over.
        assert!(plan.values[3] < 25.0 && plan.values[3] > 10.0);
        approx(plan.total, 100.0);
        approx(plan.external, 0.0);
    }

    #[test]
    fn dust_is_not_a_trade_and_zero_is_today() {
        let mut classes = drifted();
        classes[0].value = 50.004;
        classes[1].value = 49.996;
        let plan = full(&classes);
        assert!(plan.trades.is_empty());
        let same = place(&drifted(), 0.0).unwrap();
        assert!(same.trades.is_empty());
        approx(same.external, 0.0);
    }

    #[test]
    fn the_markdown_names_the_plan_and_says_it_never_trades() {
        let plan = full(&drifted());
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
