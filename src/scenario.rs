//! A what-if: one or more asset classes move by a shock, and the app works out what that leaves.
//!
//! The model may name the shocks ("crypto -50%"), never the consequences. Everything here is
//! arithmetic over the same [`Class`] list the Rebalance page plans with, so a scenario and a plan
//! never disagree about what the portfolio holds.

use serde::{Deserialize, Serialize};

use crate::rebalance::{self, Class};

/// One class moving by `percent`, as in "crypto -50%" for a halving.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Shock {
    pub class: String,
    pub percent: f64,
}

impl Shock {
    pub fn new(class: impl Into<String>, percent: f64) -> Shock {
        Shock {
            class: class.into(),
            percent,
        }
    }

    /// What a class worth `value` is worth after this shock.
    fn apply(&self, value: f64) -> f64 {
        (value * (1.0 + self.percent / 100.0)).max(0.0)
    }
}

/// One asset class, before and after.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub name: String,
    pub before: f64,
    pub after: f64,
    pub before_share: f64,
    pub after_share: f64,
    pub target: Option<f64>,
    /// Inside its policy band before and after, where it has one.
    pub inside_before: Option<bool>,
    pub inside_after: Option<bool>,
}

impl Line {
    /// It was inside its band and the shock pushes it out.
    pub fn leaves_band(&self) -> bool {
        self.inside_before == Some(true) && self.inside_after == Some(false)
    }

    /// It was outside its band and the shock brings it back in.
    pub fn returns_to_band(&self) -> bool {
        self.inside_before == Some(false) && self.inside_after == Some(true)
    }

    pub fn moved(&self) -> bool {
        (self.after - self.before).abs() >= 0.005
    }
}

/// What the portfolio looks like after the shocks.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub total_before: f64,
    pub total_after: f64,
    pub lines: Vec<Line>,
    /// Shocks that named no class the portfolio has. Nothing is invented for them.
    pub unknown: Vec<String>,
}

impl Outcome {
    pub fn change(&self) -> f64 {
        self.total_after - self.total_before
    }

    /// The change as a share of what the portfolio was worth, in percent.
    pub fn percent(&self) -> Option<f64> {
        (self.total_before.abs() >= 0.005).then(|| self.change() / self.total_before * 100.0)
    }

    /// The classes the shock pushes outside their policy band.
    pub fn leaving(&self) -> Vec<&Line> {
        self.lines
            .iter()
            .filter(|line| line.leaves_band())
            .collect()
    }

    /// The classes it brings back inside.
    pub fn returning(&self) -> Vec<&Line> {
        self.lines
            .iter()
            .filter(|line| line.returns_to_band())
            .collect()
    }
}

/// The shock a question asks for, if it asks for one: "what if crypto halves?", "if stocks fall
/// 20%", "what if gold doubles". `None` when the question names no class the portfolio has, or no
/// move. The shock is the question's; everything that follows from it is the app's.
pub fn asked<'a>(question: &str, classes: impl IntoIterator<Item = &'a str>) -> Option<Vec<Shock>> {
    let lower = question.to_lowercase();
    let class = classes
        .into_iter()
        .find(|name| name.len() >= 3 && lower.contains(&name.to_lowercase()))?;
    Some(vec![Shock::new(class, percent_in(&lower)?)])
}

/// How far a question says a class moves, in percent: "halves" is -50, "falls 20%" is -20,
/// "rises 5%" is +5.
fn percent_in(question: &str) -> Option<f64> {
    if question.contains("halve") || question.contains("half") {
        return Some(-50.0);
    }
    if question.contains("double") {
        return Some(100.0);
    }
    let words: Vec<&str> = question
        .split(|character: char| !character.is_alphanumeric() && character != '.')
        .filter(|word| !word.is_empty())
        .collect();
    let at = words.iter().position(|word| {
        word.chars()
            .next()
            .is_some_and(|first| first.is_ascii_digit())
    })?;
    let number = words[at].parse::<f64>().ok()?;
    if !number.is_finite() || number <= 0.0 {
        return None;
    }
    let up = [
        "rise", "rises", "rising", "gain", "gains", "grow", "grows", "up",
    ];
    let before = words[..at].join(" ");
    let sign = if up
        .iter()
        .any(|word| before.split(' ').any(|each| each == *word))
    {
        1.0
    } else {
        -1.0
    };
    Some(sign * number.min(100.0))
}

fn share(value: f64, total: f64) -> f64 {
    if total.abs() < 0.005 {
        0.0
    } else {
        value.max(0.0) / total * 100.0
    }
}

/// Apply `shocks` to `classes`. A class named twice is moved by both, in order.
pub fn project(classes: &[Class], shocks: &[Shock]) -> Outcome {
    let total_before: f64 = classes.iter().map(|class| class.held_value()).sum();
    let after: Vec<f64> = classes
        .iter()
        .map(|class| {
            shocks
                .iter()
                .filter(|shock| shock.class.eq_ignore_ascii_case(&class.name))
                .fold(class.held_value(), |value, shock| shock.apply(value))
        })
        .collect();
    let total_after: f64 = after.iter().sum();
    let lines = classes
        .iter()
        .enumerate()
        .map(|(index, class)| {
            let before = class.held_value();
            let now = after[index];
            Line {
                name: class.name.clone(),
                before,
                after: now,
                before_share: share(before, total_before),
                after_share: share(now, total_after),
                target: class.target,
                inside_before: rebalance::inside_band(class, before, total_before),
                inside_after: rebalance::inside_band(class, now, total_after),
            }
        })
        .collect();
    let unknown = shocks
        .iter()
        .filter(|shock| {
            !classes
                .iter()
                .any(|class| class.name.eq_ignore_ascii_case(&shock.class))
        })
        .map(|shock| shock.class.clone())
        .collect();
    Outcome {
        total_before,
        total_after,
        lines,
        unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stocks 40 (target 50 ± 5), Bonds 5 (15 ± 5), Crypto 25 (10 ± 3), Cash 30 (20 ± 5).
    fn book() -> Vec<Class> {
        vec![
            Class::new("Stocks", 40.0).target(50.0).tolerance(5.0),
            Class::new("Bonds", 5.0).target(15.0).tolerance(5.0),
            Class::new("Crypto", 25.0).target(10.0).tolerance(3.0),
            Class::new("Cash", 30.0).target(20.0).tolerance(5.0).cash(),
        ]
    }

    fn close(left: f64, right: f64) {
        assert!((left - right).abs() < 1e-6, "{left} != {right}");
    }

    #[test]
    fn a_question_that_names_a_class_and_a_move_is_a_what_if() {
        let classes = ["Stocks", "Bonds", "Gold", "Crypto", "Cash"];
        assert_eq!(
            asked("What if crypto halves?", classes),
            Some(vec![Shock::new("Crypto", -50.0)])
        );
        assert_eq!(
            asked("What if stocks fall 20%?", classes),
            Some(vec![Shock::new("Stocks", -20.0)])
        );
        assert_eq!(
            asked("If gold rises 10%, what happens?", classes),
            Some(vec![Shock::new("Gold", 10.0)])
        );
        assert_eq!(
            asked("What if bonds double?", classes),
            Some(vec![Shock::new("Bonds", 100.0)])
        );
        // A question about no class the portfolio has is not a shock.
        assert_eq!(asked("What if watches halve?", classes), None);
        // Neither is a question with no move in it.
        assert_eq!(asked("What if crypto is a good idea?", classes), None);
        assert_eq!(asked("How is crypto doing?", classes), None);
    }

    #[test]
    fn a_move_of_more_than_all_of_it_is_all_of_it() {
        let classes = ["Crypto"];
        assert_eq!(
            asked("What if crypto falls 400%?", classes),
            Some(vec![Shock::new("Crypto", -100.0)])
        );
    }

    #[test]
    fn a_halved_class_takes_its_half_out_of_the_total() {
        let outcome = project(&book(), &[Shock::new("Crypto", -50.0)]);
        close(outcome.total_before, 100.0);
        close(outcome.total_after, 87.5);
        close(outcome.change(), -12.5);
        close(outcome.percent().unwrap(), -12.5);
        let crypto = &outcome.lines[2];
        close(crypto.after, 12.5);
        close(crypto.before_share, 25.0);
        close(crypto.after_share, 14.285_714);
    }

    #[test]
    fn a_shock_names_a_class_however_it_is_written() {
        let outcome = project(&book(), &[Shock::new("crypto", -50.0)]);
        close(outcome.total_after, 87.5);
        assert!(outcome.unknown.is_empty());
    }

    /// Everything inside its band, so a shock can be seen to push one out.
    fn steady() -> Vec<Class> {
        vec![
            Class::new("Stocks", 48.0).target(50.0).tolerance(5.0),
            Class::new("Bonds", 14.0).target(15.0).tolerance(5.0),
            Class::new("Crypto", 11.0).target(10.0).tolerance(3.0),
            Class::new("Cash", 22.0).target(20.0).tolerance(5.0).cash(),
        ]
    }

    #[test]
    fn a_class_leaves_its_band_only_when_the_shock_pushes_it_out() {
        // Crypto is far outside its band already, so a fall doesn't "leave" it.
        let outcome = project(&book(), &[Shock::new("Crypto", -50.0)]);
        assert!(!outcome.lines[2].leaves_band());
        assert!(
            outcome.leaving().is_empty(),
            "every class was outside already"
        );
        // Stocks at 48% against 50 ± 5 is inside; a 30% fall takes it to 33.6 of 80.6: outside.
        let outcome = project(&steady(), &[Shock::new("Stocks", -30.0)]);
        assert!(outcome.lines[0].leaves_band());
        assert_eq!(outcome.leaving()[0].name, "Stocks");
        // Cash is not shocked, but the total falls under it, so it drifts up and out.
        let outcome = project(&steady(), &[Shock::new("Stocks", -30.0)]);
        assert!(outcome.lines[3].leaves_band());
    }

    #[test]
    fn a_class_without_a_target_never_leaves_a_band() {
        let mut classes = book();
        classes.push(Class::new("Art", 10.0));
        let outcome = project(&classes, &[Shock::new("Art", -80.0)]);
        assert_eq!(outcome.lines[4].inside_after, None);
        assert!(outcome.leaving().iter().all(|line| line.name != "Art"));
    }

    #[test]
    fn an_unknown_class_is_reported_and_changes_nothing() {
        let outcome = project(&book(), &[Shock::new("Watches", -50.0)]);
        close(outcome.total_after, outcome.total_before);
        assert_eq!(outcome.unknown, vec!["Watches".to_string()]);
    }

    #[test]
    fn two_shocks_apply_in_order_and_nothing_goes_negative() {
        let outcome = project(
            &book(),
            &[Shock::new("Crypto", -50.0), Shock::new("Crypto", -50.0)],
        );
        close(outcome.lines[2].after, 6.25);
        let gone = project(&book(), &[Shock::new("Bonds", -400.0)]);
        close(gone.lines[1].after, 0.0);
        assert!(gone.lines.iter().all(|line| line.after >= 0.0));
    }

    #[test]
    fn an_empty_portfolio_has_no_shares_to_divide() {
        let outcome = project(&[], &[Shock::new("Stocks", -20.0)]);
        close(outcome.total_before, 0.0);
        close(outcome.total_after, 0.0);
        assert_eq!(outcome.percent(), None);
        assert_eq!(outcome.unknown, vec!["Stocks".to_string()]);
    }
}
