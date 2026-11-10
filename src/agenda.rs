//! What deserves a look, in order, each with the one action that deals with it.
//!
//! Built from what the backend already knows: the policy review (the same one the Review page
//! shows, so the two pages never disagree) and the data-quality findings. [`build`] is pure;
//! [`State::agenda`] feeds it.

use portfolio_rs::{
    mcp::dto::{Finding, Severity},
    review::{AllocationReview, Review},
    state::PositionDto,
};

use crate::state::{Page, State};

/// How much an item matters. Items sort by this, most important first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Weight {
    /// The total itself is wrong until this is dealt with.
    Total,
    /// Far outside the policy: more than twice a class's tolerance, or cash under its minimum.
    Critical,
    /// Outside the policy.
    Warning,
    /// Worth knowing, not urgent.
    Notice,
}

/// The one thing an item's button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open(Page),
    /// Show a holding, by its index.
    Holding(usize),
    Refresh,
}

impl Action {
    pub fn label(self) -> &'static str {
        match self {
            Action::Open(Page::Rebalance) => "Rebalance",
            Action::Open(Page::Policy) => "Open Policy",
            Action::Open(Page::Health) => "Open Data Health",
            Action::Open(Page::Review) => "Open Review",
            Action::Open(_) => "Open",
            Action::Holding(_) => "Show Holding",
            Action::Refresh => "Refresh Prices",
        }
    }

    pub fn perform(self, s: &mut State) {
        match self {
            Action::Open(page) => s.navigate(page),
            Action::Holding(id) => s.reveal_position(id),
            Action::Refresh => s.refresh_prices(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// Stable across renders, for keys.
    pub id: String,
    pub weight: Weight,
    pub title: String,
    pub detail: Option<String>,
    pub action: Action,
}

/// The agenda when it's empty, or while it can't be known yet.
#[derive(Clone, Debug, PartialEq)]
pub enum Calm {
    /// The review against the policy hasn't landed yet: no claims either way.
    Checking,
    /// Nothing needs a look. The words say what was checked, and what's closest to needing one.
    Clear(String),
}

pub struct Inputs<'a> {
    /// `None` without a policy, or while it loads.
    pub review: Option<&'a Review>,
    pub has_policy: bool,
    pub is_workspace: bool,
    pub findings: &'a [Finding],
    pub positions: &'a [PositionDto],
    /// What cash is worth, already formatted (hidden in privacy mode).
    pub cash: String,
}

fn points(value: f64) -> String {
    format!("{:.1}", value.abs())
}

/// The holding a finding is about, by its stable id.
fn holding<'a>(finding: &Finding, positions: &'a [PositionDto]) -> Option<&'a PositionDto> {
    let id = finding.position_id.as_deref()?;
    positions
        .iter()
        .find(|position| position.position_id.as_deref() == Some(id))
}

fn name(position: &PositionDto) -> String {
    position
        .name
        .clone()
        .or_else(|| position.ticker.clone())
        .unwrap_or_else(|| "A holding".to_string())
}

fn drift(review: &Review, items: &mut Vec<Item>) {
    let mut out: Vec<_> = review
        .allocations
        .iter()
        .filter(|class| !class.within_tolerance)
        .collect();
    if out.is_empty() {
        return;
    }
    // Furthest past its tolerance first.
    let excess = |class: &&AllocationReview| {
        class.drift_percent.abs() / class.tolerance_percent.unwrap_or(5.0).max(0.1)
    };
    out.sort_by(|a, b| excess(b).total_cmp(&excess(a)));
    let critical = out.iter().any(|class| excess(class) > 2.0);
    let weight = if critical {
        Weight::Critical
    } else {
        Weight::Warning
    };
    let over_under = |class: &AllocationReview| {
        if class.drift_percent > 0.0 {
            "over"
        } else {
            "under"
        }
    };
    let (title, detail) = match out.as_slice() {
        [class] => (
            format!(
                "{} is {} points {} its target",
                class.asset_class,
                points(class.drift_percent),
                over_under(class)
            ),
            format!(
                "{:.1}% against a target of {:.1}%, with {:.1} points of tolerance.",
                class.actual_percent,
                class.target_percent,
                class.tolerance_percent.unwrap_or(5.0)
            ),
        ),
        classes => (
            format!(
                "{} of {} asset classes are outside their tolerance",
                classes.len(),
                review.allocations.len()
            ),
            classes
                .iter()
                .enumerate()
                .map(|(index, class)| {
                    // "Crypto 15.1 points over, Stocks 12.8 under".
                    format!(
                        "{} {}{} {}",
                        class.asset_class,
                        points(class.drift_percent),
                        if index == 0 { " points" } else { "" },
                        over_under(class)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
                + ".",
        ),
    };
    items.push(Item {
        id: "drift".to_string(),
        weight,
        title,
        detail: Some(detail),
        action: Action::Open(Page::Rebalance),
    });
}

fn constraints(review: &Review, cash: &str, items: &mut Vec<Item>) {
    for check in review
        .constraint_checks
        .iter()
        .filter(|check| !check.passed)
    {
        let item = match check.name.as_str() {
            "Minimum Cash Amount" => Item {
                id: "cash".to_string(),
                weight: Weight::Critical,
                title: "Cash is under your policy's minimum".to_string(),
                detail: Some(format!("You hold {cash} in cash.")),
                action: Action::Open(Page::Rebalance),
            },
            // These say what is over its limit in percentages, which privacy mode shows anyway.
            "Single Position Limit" | "Asset Class Limit" => Item {
                id: format!("limit-{}", check.message),
                weight: Weight::Warning,
                title: if check.name == "Single Position Limit" {
                    "A holding is over your limit for one position"
                } else {
                    "An asset class is over your limit for one class"
                }
                .to_string(),
                detail: Some(format!("{}.", check.message.trim_end_matches('.'))),
                action: Action::Open(Page::Review),
            },
            _ => Item {
                id: format!("check-{}", check.name),
                weight: Weight::Warning,
                title: check.name.clone(),
                detail: Some(check.message.clone()),
                action: Action::Open(Page::Review),
            },
        };
        items.push(item);
    }
    for finding in &review.findings {
        if finding.category == "Unknown Asset Class" {
            items.push(Item {
                id: format!("class-{}", finding.message),
                weight: Weight::Notice,
                title: "An asset class isn't in your policy".to_string(),
                detail: Some(format!(
                    "{}. Give it a target, or move its holdings to a class that has one.",
                    finding.message.trim_end_matches('.')
                )),
                action: Action::Open(Page::Policy),
            });
        }
    }
}

fn data(findings: &[Finding], positions: &[PositionDto], items: &mut Vec<Item>) {
    for finding in findings {
        if matches!(finding.severity, Severity::Info) {
            continue;
        }
        let position = holding(finding, positions);
        let id = format!(
            "{}-{}",
            finding.code,
            finding.position_id.as_deref().unwrap_or("")
        );
        let item = match (finding.code.as_str(), position) {
            ("stale_prices", _) => Item {
                id,
                weight: Weight::Notice,
                title: "Some prices are out of date".to_string(),
                detail: Some("Values use the last prices Stahl could get.".to_string()),
                action: Action::Refresh,
            },
            ("no_live_price", Some(position)) => Item {
                id,
                weight: Weight::Total,
                title: format!("{} has no price", name(position)),
                detail: Some("It's left out of the total until it has one.".to_string()),
                action: Action::Holding(position.id),
            },
            ("fx_unavailable", Some(position)) => Item {
                id,
                weight: Weight::Total,
                title: format!("{} has no exchange rate", name(position)),
                detail: Some("It's left out of the total until it has one.".to_string()),
                action: Action::Holding(position.id),
            },
            ("missing_cost_basis", Some(position)) => Item {
                id,
                weight: Weight::Notice,
                title: format!("{} has no purchase price", name(position)),
                detail: Some("Without one, its gain can't be worked out.".to_string()),
                action: Action::Holding(position.id),
            },
            _ => Item {
                id,
                weight: Weight::Notice,
                title: finding.message.clone(),
                detail: None,
                action: Action::Open(Page::Health),
            },
        };
        items.push(item);
    }
}

/// Everything that deserves a look, most important first.
pub fn build(inputs: &Inputs) -> Vec<Item> {
    let mut items = Vec::new();
    data(inputs.findings, inputs.positions, &mut items);
    if let Some(review) = inputs.review {
        drift(review, &mut items);
        constraints(review, &inputs.cash, &mut items);
    }
    if inputs.is_workspace && !inputs.has_policy {
        items.push(Item {
            id: "policy".to_string(),
            weight: Weight::Notice,
            title: "There's no policy to measure against".to_string(),
            detail: Some(
                "Say how you want your money split, and this list will say when it drifts."
                    .to_string(),
            ),
            action: Action::Open(Page::Policy),
        });
    }
    // Stable, so items of one weight keep the order they were found in.
    items.sort_by_key(|item| item.weight);
    items
}

/// What to say when [`build`] found nothing.
pub fn calm(inputs: &Inputs) -> Calm {
    if !inputs.has_policy {
        return Calm::Clear(if inputs.is_workspace {
            "Every price is current.".to_string()
        } else {
            "Every price is current. In a workspace with a policy, this would also watch how your \
             money is split."
                .to_string()
        });
    }
    let Some(review) = inputs.review else {
        return Calm::Checking;
    };
    // The class nearest its tolerance is the one to watch.
    let closest = review
        .allocations
        .iter()
        .map(|class| {
            let tolerance = class.tolerance_percent.unwrap_or(5.0).max(0.1);
            (class, class.drift_percent.abs() / tolerance)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1));
    let watch = match closest {
        Some((class, used)) if used >= 0.5 => format!(
            " Closest to the edge: {}, {} points {} its target, with {:.1} allowed.",
            class.asset_class,
            points(class.drift_percent),
            if class.drift_percent > 0.0 {
                "over"
            } else {
                "under"
            },
            class.tolerance_percent.unwrap_or(5.0)
        ),
        _ => String::new(),
    };
    Calm::Clear(format!(
        "Every asset class is within its tolerance, and every price is current.{watch}"
    ))
}

impl State {
    fn agenda_inputs(&self) -> Inputs<'_> {
        Inputs {
            // A review still loading, or one that failed, makes no claim.
            review: self.review.data.as_ref(),
            has_policy: self.has_policy,
            is_workspace: self.is_workspace(),
            findings: &self.findings,
            positions: &self.positions,
            cash: self
                .summary
                .as_ref()
                .map_or_else(String::new, |summary| self.money(summary.cash_value)),
        }
    }

    /// What deserves a look on the Overview, most important first.
    pub fn agenda(&self) -> Vec<Item> {
        if self.positions.is_empty() {
            return Vec::new();
        }
        build(&self.agenda_inputs())
    }

    /// How many things the Overview's agenda asks the owner to look at. Zero when there's nothing,
    /// and while there's no portfolio.
    pub fn agenda_len(&self) -> usize {
        self.agenda().len()
    }

    /// What the agenda says when it has nothing in it.
    pub fn agenda_calm(&self) -> Calm {
        calm(&self.agenda_inputs())
    }
}

#[cfg(test)]
mod tests {
    use portfolio_rs::{
        mcp::dto::Category,
        review::{ConstraintCheck, Finding as ReviewFinding, Severity as ReviewSeverity},
    };

    use super::*;

    fn class(name: &str, target: f64, tolerance: f64, actual: f64) -> AllocationReview {
        let drift = actual - target;
        AllocationReview {
            asset_class: name.to_string(),
            target_percent: target,
            tolerance_percent: Some(tolerance),
            actual_percent: actual,
            drift_percent: drift,
            within_tolerance: drift.abs() <= tolerance,
            value: 0.0,
        }
    }

    fn review(allocations: Vec<AllocationReview>, checks: Vec<ConstraintCheck>) -> Review {
        Review {
            portfolio_value: 100_000.0,
            currency: "EUR".to_string(),
            policy_name: "Balanced Growth".to_string(),
            findings: Vec::new(),
            allocations,
            constraint_checks: checks,
            data_quality: Vec::new(),
            suggested_actions: Vec::new(),
        }
    }

    /// The demo's: four of five classes outside their tolerance.
    fn demo_review() -> Review {
        review(
            vec![
                class("Stocks", 50.0, 5.0, 37.2),
                class("Bonds", 15.0, 5.0, 5.0),
                class("Gold", 5.0, 2.0, 4.7),
                class("Crypto", 10.0, 3.0, 25.1),
                class("Cash", 20.0, 5.0, 27.9),
            ],
            vec![ConstraintCheck {
                name: "Minimum Cash Amount".to_string(),
                passed: true,
                message: "Cash buffer: 30000.00 EUR (minimum: 20000.00 EUR)".to_string(),
            }],
        )
    }

    fn position(index: usize, id: &str, name: &str) -> PositionDto {
        PositionDto {
            id: index,
            position_id: Some(id.to_string()),
            name: Some(name.to_string()),
            ticker: None,
            asset_class: "Stocks".to_string(),
            amount: 1.0,
            price: 1.0,
            value: 1.0,
            pnl: None,
            pnl_percent: None,
            day_change_percent: None,
            average_cost: None,
            invested: None,
            purchases: Vec::new(),
        }
    }

    fn inputs<'a>(
        review: Option<&'a Review>,
        findings: &'a [Finding],
        positions: &'a [PositionDto],
    ) -> Inputs<'a> {
        Inputs {
            review,
            has_policy: true,
            is_workspace: true,
            findings,
            positions,
            cash: "€30,000.00".to_string(),
        }
    }

    #[test]
    fn drift_the_review_reports_is_on_the_agenda() {
        let review = demo_review();
        let items = build(&inputs(Some(&review), &[], &[]));
        assert_eq!(items.len(), 1, "one item for all the drift, not four");
        let drift = &items[0];
        assert_eq!(
            drift.title,
            "4 of 5 asset classes are outside their tolerance"
        );
        assert_eq!(
            drift.detail.as_deref(),
            Some("Crypto 15.1 points over, Stocks 12.8 under, Bonds 10.0 under, Cash 7.9 over.")
        );
        assert_eq!(drift.weight, Weight::Critical);
        assert_eq!(drift.action, Action::Open(Page::Rebalance));
        assert_eq!(drift.action.label(), "Rebalance");
    }

    #[test]
    fn one_class_out_is_named() {
        let review = review(
            vec![
                class("Stocks", 50.0, 5.0, 56.0),
                class("Bonds", 50.0, 5.0, 46.0),
            ],
            Vec::new(),
        );
        let items = build(&inputs(Some(&review), &[], &[]));
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "Stocks is 6.0 points over its target");
        assert_eq!(
            items[0].detail.as_deref(),
            Some("56.0% against a target of 50.0%, with 5.0 points of tolerance.")
        );
        assert_eq!(items[0].weight, Weight::Warning);
    }

    #[test]
    fn the_most_important_comes_first() {
        let mut review = review(
            vec![class("Stocks", 50.0, 5.0, 56.0)],
            vec![ConstraintCheck {
                name: "Minimum Cash Amount".to_string(),
                passed: false,
                message: "Cash buffer: 1000.00 EUR (minimum: 20000.00 EUR)".to_string(),
            }],
        );
        review.findings.push(ReviewFinding {
            severity: ReviewSeverity::Warning,
            category: "Unknown Asset Class".to_string(),
            message: "Asset class 'Art' (3.0%) is not defined in policy".to_string(),
            detail: None,
        });
        let findings = [
            Finding::new(
                "stale_prices",
                Severity::Warning,
                Category::DataQuality,
                "1 position(s) have prices older than the stale threshold".to_string(),
            ),
            Finding::new(
                "no_live_price",
                Severity::Warning,
                Category::DataQuality,
                "No live price for XYZ; excluded from totals.".to_string(),
            )
            .position(Some("pos_xyz")),
            Finding::new(
                "missing_ticker",
                Severity::Info,
                Category::DataQuality,
                "Too minor to list".to_string(),
            ),
        ];
        let positions = [
            position(0, "pos_abc", "Other"),
            position(3, "pos_xyz", "Xyz Corp"),
        ];
        let items = build(&inputs(Some(&review), &findings, &positions));
        let titles: Vec<&str> = items.iter().map(|item| item.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Xyz Corp has no price",
                "Cash is under your policy's minimum",
                "Stocks is 6.0 points over its target",
                "Some prices are out of date",
                "An asset class isn't in your policy",
            ]
        );
        assert_eq!(items[0].action, Action::Holding(3));
        assert_eq!(
            items[1].detail.as_deref(),
            Some("You hold €30,000.00 in cash.")
        );
        assert_eq!(items[3].action, Action::Refresh);
        assert_eq!(items[4].action, Action::Open(Page::Policy));
    }

    #[test]
    fn a_workspace_without_a_policy_is_asked_for_one() {
        let inputs = Inputs {
            has_policy: false,
            ..inputs(None, &[], &[])
        };
        let items = build(&inputs);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].action, Action::Open(Page::Policy));
    }

    #[test]
    fn an_empty_agenda_is_never_claimed_before_the_review_lands() {
        assert_eq!(calm(&inputs(None, &[], &[])), Calm::Checking);
        assert!(build(&inputs(None, &[], &[])).is_empty());
    }

    #[test]
    fn an_empty_agenda_says_what_was_checked_and_what_to_watch() {
        let quiet = review(
            vec![
                class("Stocks", 50.0, 5.0, 53.5),
                class("Bonds", 50.0, 5.0, 47.0),
            ],
            Vec::new(),
        );
        assert!(build(&inputs(Some(&quiet), &[], &[])).is_empty());
        assert_eq!(
            calm(&inputs(Some(&quiet), &[], &[])),
            Calm::Clear(
                "Every asset class is within its tolerance, and every price is current. Closest \
                 to the edge: Stocks, 3.5 points over its target, with 5.0 allowed."
                    .to_string()
            )
        );
        let centred = review(vec![class("Stocks", 50.0, 5.0, 50.5)], Vec::new());
        assert_eq!(
            calm(&inputs(Some(&centred), &[], &[])),
            Calm::Clear(
                "Every asset class is within its tolerance, and every price is current."
                    .to_string()
            )
        );
        let file = Inputs {
            has_policy: false,
            is_workspace: false,
            ..inputs(None, &[], &[])
        };
        assert!(build(&file).is_empty());
        assert!(matches!(calm(&file), Calm::Clear(text) if text.starts_with("Every price")));
    }
}
