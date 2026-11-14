//! The sheet a broker's file opens over Holdings: what it holds, in one sentence, and a row for
//! each change it asks for, with the file's own line under it.

use rune::{Cx, El, ScrollSpec, col, el, icon, row, text};
use rune_kit::{Button, ButtonVariant};

use crate::{
    format, icons,
    intake::{
        self,
        reconcile::{Change, Ledger, Proposal, Trade},
        rules::{self, Aside, SetAside},
        text::Sheet,
    },
    state::{Decision, IntakeReview, State},
    theme::Theme,
    view::ui,
};

pub fn sheet(s: &State, t: &Theme, cx: &Cx<State>, layer: &mut Vec<El>) {
    let Some(review) = &s.intake else {
        return;
    };
    let k = &t.kit;
    let compact = s.compact;
    let name = review.file.clone();
    let mut body = col()
        .w_full()
        .gap(8.0)
        .child(ui::caption(name, t).key("intake-file").truncate_middle());
    match (&review.problem, &review.ledger) {
        (Some(problem), _) => {
            body = body.child(
                text(problem.clone())
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .wrap()
                    .key("intake-problem"),
            );
        }
        (None, Some(ledger)) => {
            body = body.child(
                text(sentence(ledger))
                    .font(k.ty.ui_medium)
                    .color(k.c.text_base)
                    .wrap()
                    .key("intake-sentence"),
            );
            if ledger.agreed > 0 {
                body = body.child(ui::caption(already(ledger.agreed), t).key("intake-agreed"));
            }
            for note in &ledger.notes {
                body = body.child(ui::caption(note.clone(), t).wrap());
            }
            if let Some(said) = set_aside(&review.aside) {
                body = body.child(
                    text(said)
                        .font(k.ty.ui)
                        .color(t.warn)
                        .wrap()
                        .key("intake-aside"),
                );
            }
        }
        (None, None) => {}
    }

    let mut panel = ui::floating(t)
        .key("intake-sheet")
        .role("dialog")
        .label("From your broker")
        .w_full()
        .max_w(if compact { f32::MAX } else { 720.0 })
        .transition(ui::float_in())
        .child(col().w_full().padding(20.0).child(body));

    let listed = review.ledger.is_some() && !review.aside.is_empty();
    let shown = review.ledger.as_ref().map_or(0, |ledger| {
        ledger
            .proposals
            .iter()
            .enumerate()
            .filter(|(index, _)| review.decided.get(*index) != Some(&Decision::Left))
            .count()
    });
    // What the window leaves the sheet, so a long list scrolls inside it rather than off the
    // bottom of the page.
    let height = cx.layout_size("window").map_or(860.0, |[_, height]| height);
    let room = (height - if compact { 180.0 } else { 260.0 }).max(160.0);
    if let Some(rows) = review
        .ledger
        .as_ref()
        .filter(|_| shown > 0 || listed)
        .map(|ledger| rows(s, review, ledger, t))
    {
        panel = panel.child(ui::rule(t)).child(
            col()
                .w_full()
                .min_h(0.0)
                .max_h(room)
                .padding(8.0)
                .scroll(ScrollSpec {
                    stick_bottom: false,
                })
                .child(rows),
        );
    }

    let take_all = review.ledger.as_ref().is_some_and(|ledger| {
        ledger
            .proposals
            .iter()
            .enumerate()
            .any(|(index, proposal)| {
                proposal.in_all && review.decided.get(index) == Some(&Decision::Pending)
            })
    });
    let close = Button::new("Close")
        .variant(ButtonVariant::Submit)
        .on_press(|s: &mut State| s.intake = None)
        .build(k)
        .key("intake-cancel");
    let foot = if shown == 0 {
        row().center_y().w_full().justify_end().child(close)
    } else {
        row()
            .center_y()
            .w_full()
            .justify_between()
            .gap(12.0)
            .children(
                (!compact)
                    .then(|| ui::caption("Nothing reaches your file until you save.", t).wrap()),
            )
            .child(
                row()
                    .center_y()
                    .gap(8.0)
                    .child(
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .on_press(|s: &mut State| s.intake = None)
                            .build(k)
                            .key("intake-dismiss"),
                    )
                    .child(
                        Button::new("Accept all")
                            .variant(ButtonVariant::Submit)
                            .disabled(!take_all || review.busy)
                            .help("Takes every change that needs no second look")
                            .on_press(|s: &mut State| s.accept_all_intake())
                            .build(k)
                            .key("intake-accept-all"),
                    ),
            )
    };
    panel = panel
        .child(ui::rule(t))
        .child(col().w_full().padding(16.0).child(foot));

    layer.push(
        el().key("intake-scrim")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .fill(k.c.scrim.alpha(0.3))
            .a11y_hidden(),
    );
    layer.push(
        col()
            .key("intake-layer")
            .absolute(0.0, 0.0)
            .w_full()
            .h_full()
            .center()
            .padding(if compact { 12.0 } else { 24.0 })
            .child(panel),
    );
}

/// A hairline between rows, and one row per change the owner hasn't left.
fn rows(s: &State, review: &IntakeReview, ledger: &Ledger, t: &Theme) -> El {
    let mut list = col().w_full().gap(0.0);
    let mut shown = 0;
    for (index, proposal) in ledger.proposals.iter().enumerate() {
        let decision = review
            .decided
            .get(index)
            .copied()
            .unwrap_or(Decision::Pending);
        if decision == Decision::Left {
            continue;
        }
        if shown > 0 {
            list = list.child(ui::rule(t));
        }
        list = list.child(one_row(s, review, index, proposal, decision, t));
        shown += 1;
    }
    if !review.aside.is_empty() {
        if shown > 0 {
            list = list.child(ui::rule(t));
        }
        list = list.child(
            col()
                .key("intake-aside-list")
                .w_full()
                .gap(8.0)
                .padding(12.0)
                .child(ui::caption("Set aside, and not recorded", t))
                .children(
                    review
                        .aside
                        .iter()
                        .take(ASIDE_SHOWN)
                        .enumerate()
                        .map(|(index, aside)| aside_row(s, review, index, aside, t)),
                )
                .children((review.aside.len() > ASIDE_SHOWN).then(|| {
                    ui::caption(
                        format!("and {} more lines", review.aside.len() - ASIDE_SHOWN),
                        t,
                    )
                })),
        );
    }
    list
}

/// How many set-aside lines the sheet lists before it says how many more there are.
const ASIDE_SHOWN: usize = 12;

fn aside_row(s: &State, review: &IntakeReview, index: usize, aside: &SetAside, t: &Theme) -> El {
    let k = &t.kit;
    let line = review
        .sheet
        .as_ref()
        .and_then(|sheet| sheet.lines.get(aside.line))
        .filter(|_| !s.privacy)
        .map_or_else(
            || format!("line {}", aside.line + 1),
            |text| format!("line {}: {}", aside.line + 1, text.trim()),
        );
    col()
        .key(format!("intake-aside-{index}"))
        .w_full()
        .gap(3.0)
        .child(
            text(reason(&aside.why, s.privacy))
                .font(k.ty.text_xsmall)
                .color(k.c.text_base)
                .wrap(),
        )
        .child(
            text(line)
                .font(k.ty.mono.lh(14.0))
                .color(k.c.text_faint)
                .truncate_middle(),
        )
}

/// Why a line was set aside, in words. With amounts hidden, what quotes the file's cells is
/// not repeated.
pub fn reason(why: &Aside, privacy: bool) -> String {
    match why {
        Aside::NotATrade(word) if rules::changes_a_count(word) => format!(
            "“{word}” changes a count without a purchase or a sale. Make it by hand in the \
             holding."
        ),
        Aside::NotATrade(word) => format!("“{word}” is not a purchase or a sale."),
        Aside::Cancelled(word, _) => {
            format!("“{word}” takes a trade back, so it is not read as a purchase or a sale.")
        }
        Aside::Total => "A total, not a trade.".to_string(),
        Aside::Unsure(_) if privacy => "This line couldn't be read with certainty.".to_string(),
        Aside::Unsure(why) => why.clone(),
    }
}

/// The lines left out, counted in one sentence. `None` when nothing was.
pub fn set_aside(aside: &[SetAside]) -> Option<String> {
    if aside.is_empty() {
        return None;
    }
    let by_hand = aside
        .iter()
        .filter(|each| matches!(&each.why, Aside::NotATrade(word) if rules::changes_a_count(word)))
        .count();
    let mut said = if aside.len() == 1 {
        "One line was set aside, and nothing was taken from it.".to_string()
    } else {
        format!(
            "{} lines were set aside, and nothing was taken from them.",
            capitalise(&number_word(aside.len()))
        )
    };
    if by_hand > 0 {
        said.push_str(if by_hand == 1 {
            " A transfer or a split changes a count, and is yours to make by hand."
        } else {
            " Transfers and splits change a count, and are yours to make by hand."
        });
    }
    Some(said)
}

fn one_row(
    s: &State,
    review: &IntakeReview,
    index: usize,
    proposal: &Proposal,
    decision: Decision,
    t: &Theme,
) -> El {
    let k = &t.kit;
    let (headline, detail) = what(proposal, s.privacy);
    let mut words = col()
        .min_w(0.0)
        .gap(3.0)
        .child(
            text(headline)
                .font(k.ty.ui_medium)
                .color(k.c.text_base)
                .wrap(),
        )
        .child(ui::number(detail, k.c.text_muted));
    if let Some(sheet) = &review.sheet {
        for line in sources(proposal, sheet, s.privacy) {
            words = words.child(
                text(line)
                    .font(k.ty.mono.lh(14.0))
                    .color(k.c.text_faint)
                    .truncate_middle(),
            );
        }
    }
    for doubt in &proposal.doubts {
        words = words.child(
            row()
                .center_y()
                .items_start()
                .gap(6.0)
                .child(ui::dot(t.warn).mt(5.0))
                .child(
                    text(doubt.clone())
                        .font(k.ty.text_xsmall)
                        .color(t.warn)
                        .wrap()
                        .shrink(),
                ),
        );
    }
    let controls = if decision == Decision::Accepted {
        row()
            .center_y()
            .gap(6.0)
            .child(icon(&icons::CHECK_CIRCLE, 14.0).color(t.up))
            .child(text("Recorded").font(k.ty.text_xsmall_medium).color(t.up))
    } else {
        row()
            .center_y()
            .gap(6.0)
            .child(
                Button::new("Leave")
                    .variant(ButtonVariant::Ghost)
                    .disabled(review.busy)
                    .on_press(move |s: &mut State| s.leave_intake(index))
                    .build(k)
                    .key(format!("intake-leave-{index}")),
            )
            .child(
                Button::new("Accept")
                    .disabled(!proposal.ready || review.busy)
                    .help(if proposal.ready {
                        "Record this change"
                    } else {
                        "This one can't be recorded as it stands"
                    })
                    .on_press(move |s: &mut State| s.accept_intake(index))
                    .build(k)
                    .key(format!("intake-accept-{index}")),
            )
    };
    let content = if s.compact {
        col()
            .w_full()
            .gap(10.0)
            .child(words)
            .child(row().center_y().w_full().justify_end().child(controls))
    } else {
        row()
            .center_y()
            .w_full()
            .items_start()
            .gap(16.0)
            .child(words.grow(1.0).w(0.0))
            .child(controls)
    };
    col()
        .key(format!("intake-row-{index}"))
        .w_full()
        .gap(0.0)
        .padding(12.0)
        .child(content)
}

/// What one change does, and the figure it moves: the holding's count before and after.
fn what(proposal: &Proposal, privacy: bool) -> (String, String) {
    let count = |value: f64| {
        if privacy {
            format::HIDDEN.to_string()
        } else {
            format::amount(value)
        }
    };
    match &proposal.change {
        Change::Trades {
            holding, trades, ..
        } => {
            let title = &holding.title;
            let sold = trades
                .iter()
                .filter(|trade| trade.quantity.value < 0.0)
                .count();
            let bought = trades.len() - sold;
            let headline = match (bought, sold) {
                (1, 0) => format!("A purchase of {title}"),
                (0, 1) => format!("A sale of {title}"),
                (bought, 0) => capitalised(&things(bought, "purchase", "purchases"), title),
                (0, sold) => capitalised(&things(sold, "sale", "sales"), title),
                _ => format!("{} in {title}", things(trades.len(), "trade", "trades")),
            };
            let after =
                holding.before + trades.iter().map(|trade| trade.quantity.value).sum::<f64>();
            (
                headline,
                format!("{} → {} shares", count(holding.before), count(after)),
            )
        }
        Change::Quantity { holding, held, .. } => (
            format!("A different count for {}", holding.title),
            format!("{} → {} shares", count(holding.before), count(held.value)),
        ),
        Change::Add {
            holding,
            trades,
            held,
            ..
        } => {
            let after = held.as_ref().map_or_else(
                || trades.iter().map(|trade| trade.quantity.value).sum::<f64>(),
                |held| held.value,
            );
            (
                format!("A new holding: {}", holding.title()),
                format!("{} → {} shares", count(0.0), count(after)),
            )
        }
        Change::Missing { holding } => (
            format!("{} is not in this file", holding.title),
            format!("{} shares on record", count(holding.before)),
        ),
    }
}

/// The lines of the file a change came from, as the file itself writes them. With amounts
/// hidden, only where they are: the line's text is the amounts.
fn sources(proposal: &Proposal, sheet: &Sheet, privacy: bool) -> Vec<String> {
    let lines = proposal.change.lines();
    let mut shown: Vec<String> = lines
        .iter()
        .take(3)
        .filter_map(|line| {
            let text = sheet.lines.get(*line)?;
            Some(if privacy {
                format!("line {}", line + 1)
            } else {
                format!("line {}: {}", line + 1, text.trim())
            })
        })
        .collect();
    if lines.len() > shown.len() {
        shown.push(format!("and {} more lines", lines.len() - shown.len()));
    }
    shown
}

/// The file's contents as one sentence, in the bank's voice.
pub fn sentence(ledger: &Ledger) -> String {
    let mut purchases = 0;
    let mut sales = 0;
    let mut added = 0;
    let mut counts = 0;
    let mut missing = 0;
    let trades = |list: &[Trade], purchases: &mut usize, sales: &mut usize| {
        for trade in list {
            if trade.quantity.value < 0.0 {
                *sales += 1;
            } else {
                *purchases += 1;
            }
        }
    };
    for proposal in &ledger.proposals {
        match &proposal.change {
            Change::Trades { trades: list, .. } => trades(list, &mut purchases, &mut sales),
            Change::Add { trades: list, .. } => {
                added += 1;
                trades(list, &mut purchases, &mut sales);
            }
            Change::Quantity { .. } => counts += 1,
            Change::Missing { .. } => missing += 1,
        }
    }
    let mut parts = Vec::new();
    if purchases > 0 {
        parts.push(things(purchases, "purchase", "purchases"));
    }
    if sales > 0 {
        parts.push(things(sales, "sale", "sales"));
    }
    if added > 0 {
        parts.push(things(
            added,
            "holding we have not seen before",
            "holdings we have not seen before",
        ));
    }
    if counts > 0 {
        parts.push(things(
            counts,
            "holding whose count the file gives differently",
            "holdings whose counts the file gives differently",
        ));
    }
    if missing > 0 {
        parts.push(things(
            missing,
            "holding the file leaves out",
            "holdings the file leaves out",
        ));
    }
    if parts.is_empty() {
        return if ledger.agreed > 0 {
            "Nothing here is new: every line is already on your record.".to_string()
        } else {
            "There is nothing in this file we can read as a trade or a holding.".to_string()
        };
    }
    let borrowed: Vec<&str> = parts.iter().map(String::as_str).collect();
    let spoken = intake::spoken(&borrowed);
    format!("{}.", capitalise(&spoken))
}

/// What already agrees, said in passing.
fn already(rows: usize) -> String {
    if rows == 1 {
        "One line already matches your record.".to_string()
    } else {
        format!("{rows} lines already match your record.")
    }
}

const NUMBERS: [&str; 13] = [
    "no", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten", "eleven",
    "twelve",
];

fn number_word(count: usize) -> String {
    NUMBERS
        .get(count)
        .map_or_else(|| count.to_string(), |word| (*word).to_string())
}

fn things(count: usize, one: &str, many: &str) -> String {
    format!(
        "{} {}",
        number_word(count),
        if count == 1 { one } else { many }
    )
}

fn capitalised(what: &str, title: &str) -> String {
    format!("{} of {title}", capitalise(what))
}

/// The first letter in its upper case; the rest is left alone.
fn capitalise(words: &str) -> String {
    let mut characters = words.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().chain(characters).collect(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::*;
    use crate::intake::{
        reconcile::{Change, Match, Matched, Newcomer, Trade},
        rules::{Dated, Figure, Text},
        text::Spot,
    };

    fn figure(value: f64) -> Figure {
        Figure {
            value,
            text: format!("{value}"),
            at: Spot {
                line: 1,
                start: 0,
                end: 1,
            },
        }
    }

    fn trade(line: usize, quantity: f64) -> Trade {
        Trade {
            line,
            date: Some(Dated {
                date: NaiveDate::from_ymd_opt(2026, 11, 10).unwrap(),
                text: "2026-11-10".into(),
                at: Spot {
                    line,
                    start: 0,
                    end: 10,
                },
            }),
            quantity: figure(quantity),
            price: None,
            fees: None,
            currency: None,
            price_left_out: false,
        }
    }

    fn matched(before: f64) -> Matched {
        Matched {
            id: "pos_spy".into(),
            title: "S&P 500".into(),
            ticker: Some("SPY".into()),
            by: Match::Ticker,
            before,
        }
    }

    fn proposal(change: Change) -> Proposal {
        Proposal {
            change,
            doubts: Vec::new(),
            ready: true,
            in_all: true,
        }
    }

    fn ledger(proposals: Vec<Proposal>, agreed: usize) -> Ledger {
        Ledger {
            proposals,
            agreed,
            notes: Vec::new(),
        }
    }

    #[test]
    fn the_sentence_counts_what_the_file_holds() {
        let three_purchases_and_a_new_holding = ledger(
            vec![
                proposal(Change::Trades {
                    holding: matched(80.0),
                    trades: vec![trade(1, 3.0)],
                    opening: false,
                }),
                proposal(Change::Add {
                    holding: Newcomer {
                        name: Some(Text {
                            text: "Nordic Timber".into(),
                            at: Spot {
                                line: 3,
                                start: 0,
                                end: 1,
                            },
                        }),
                        ticker: None,
                        isin: None,
                        class: None,
                    },
                    trades: vec![trade(3, 40.0), trade(4, 2.0)],
                    held: None,
                }),
            ],
            0,
        );
        assert_eq!(
            sentence(&three_purchases_and_a_new_holding),
            "Three purchases and one holding we have not seen before."
        );
    }

    #[test]
    fn a_file_already_on_record_says_so() {
        assert_eq!(
            sentence(&ledger(Vec::new(), 3)),
            "Nothing here is new: every line is already on your record."
        );
        assert_eq!(
            sentence(&ledger(Vec::new(), 0)),
            "There is nothing in this file we can read as a trade or a holding."
        );
    }

    #[test]
    fn a_change_reads_as_what_it_does_to_the_count() {
        let (headline, detail) = what(
            &proposal(Change::Trades {
                holding: matched(80.0),
                trades: vec![trade(1, 3.0), trade(2, 1.0)],
                opening: false,
            }),
            false,
        );
        assert_eq!(headline, "Two purchases of S&P 500");
        assert_eq!(detail, "80.00 → 84.00 shares");

        let (_, hidden) = what(
            &proposal(Change::Trades {
                holding: matched(80.0),
                trades: vec![trade(1, 3.0), trade(2, 1.0)],
                opening: false,
            }),
            true,
        );
        assert_eq!(
            hidden,
            format!("{} → {} shares", format::HIDDEN, format::HIDDEN)
        );
    }
}
