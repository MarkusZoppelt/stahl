//! Plain words for what each page does and what its terms mean, and the guide that collects them.

use rune::{El, ScrollSpec, col, icon, row};
use rune_kit::{Dialog, Keybind};

use crate::{
    state::{Page, State},
    theme::Theme,
    view::{page_icon, ui},
};

pub const WORKSPACE: &str = "A folder holding your positions, policy, reports and decisions. A single positions file works too, without the Plan pages.";
pub const POSITION: &str = "One thing you own: a share, a fund, crypto or a cash balance.";
pub const TICKER: &str = "The Yahoo Finance symbol used to look up a price, such as AAPL. Leave it blank for cash or anything without a market price.";
pub const ASSET_CLASS: &str = "A group such as Stocks, Bonds or Cash. Spell it the way your policy does, or Review treats it as a different class.";
pub const QUOTES: &str = "Market prices, looked up when you open a portfolio, when you refresh, and when you come back after 15 minutes away. The dot in the toolbar says how current they are.";
pub const VALUATION_HISTORY: &str = "Your total value, recorded once a day when you open the app or refresh prices, if every position has a price. The charts draw it.";
pub const POLICY: &str = "How you want your money split, and the limits you set yourself. It's a file in your workspace.";
pub const DRIFT: &str = "How far an asset class is from its target, in percentage points.";
pub const TOLERANCE: &str = "How much drift you accept before Review flags it.";
pub const UNREALIZED_PNL: &str =
    "Profit or loss on what you still hold: its value now, minus what you paid for it.";
pub const INVESTED: &str = "What you paid, from the purchases you entered.";
pub const TOTAL_RETURN: &str = "Unrealized profit or loss as a share of what you invested.";
pub const TWR: &str =
    "Growth with your deposits and withdrawals taken out, so adding money doesn't count as a gain.";
pub const NET_CONTRIBUTIONS: &str = "Money you put in, minus money you took out, over this period.";
pub const MARKET_EFFECT: &str = "How much the value changed from prices alone, over this period.";
pub const CASH_IMPACT: &str = "How much your cash would change if you made these trades.";
pub const FRESHNESS: &str = "How current these prices are. Live: just looked up. Stale: the last price is days old. Partial: some are missing. Offline: no connection, so prices may be old or missing.";

/// What a page is for, in a sentence.
pub fn about(page: Page) -> &'static str {
    match page {
        Page::Overview => {
            "What changed since you last looked, where you stand, and what to look at."
        }
        Page::Holdings => {
            "Everything you own, as a table or a map. Select one for its cost and purchases."
        }
        Page::Allocation => {
            "How your money splits across asset classes, and how that compares with your policy."
        }
        Page::Performance => "Your value over time, separating market moves from money you added.",
        Page::Review => "How your portfolio compares with your policy, and what to do about it.",
        Page::Rebalance => {
            "How far to move toward your policy, and the trades that would get you there."
        }
        Page::Policy => {
            "How you want your money split, and your limits. Review and Rebalance measure against it."
        }
        Page::Documents => {
            "Weekly reports and records of your decisions, saved as Markdown in your workspace."
        }
        Page::Briefing => {
            "One page on your whole portfolio, to read or paste into an AI assistant."
        }
        Page::Health => "Checks on your files and data, so you know which figures to trust.",
        Page::Settings => "Your currency and theme, and where this app keeps its settings.",
    }
}

const GROUPS: [(&str, &str, &[Page]); 3] = [
    (
        "Portfolio",
        "What you own and how it has done.",
        &Page::PORTFOLIO,
    ),
    (
        "Plan",
        "What you want it to look like. These need a workspace.",
        &Page::PLAN,
    ),
    (
        "Insights",
        "Summaries and checks on your data.",
        &Page::INSIGHTS,
    ),
];

const TERMS: [(&str, &str); 12] = [
    ("Workspace", WORKSPACE),
    ("Position", POSITION),
    ("Ticker", TICKER),
    ("Asset class", ASSET_CLASS),
    ("Quotes", QUOTES),
    ("Valuation history", VALUATION_HISTORY),
    ("Policy", POLICY),
    ("Drift", DRIFT),
    ("Tolerance", TOLERANCE),
    ("Unrealized PnL", UNREALIZED_PNL),
    ("Time-weighted return", TWR),
    ("Market effect", MARKET_EFFECT),
];

const SHORTCUTS: [(&str, bool, &str); 10] = [
    ("Command palette: every page and action", false, "K"),
    ("This guide", false, "/"),
    ("New workspace", true, "N"),
    ("Open a workspace", true, "O"),
    ("Open a positions file", false, "O"),
    ("Close the portfolio", true, "W"),
    ("Add a position", false, "N"),
    ("Save", false, "S"),
    ("Refresh prices", false, "R"),
    ("Hide amounts", true, "H"),
];

fn section(heading: &str, detail: Option<&str>, rows: Vec<El>, t: &Theme) -> El {
    col()
        .w_full()
        .gap(8.0)
        .child(match detail {
            Some(detail) => ui::header(heading, detail, t),
            None => ui::title(heading, t),
        })
        .child(col().w_full().gap(8.0).children(rows))
}

/// A term or page on the left, and what it means on the right.
fn entry(left: El, meaning: &str, t: &Theme) -> El {
    row()
        .w_full()
        .gap(12.0)
        .items_start()
        .child(row().w(150.0).gap(8.0).child(left))
        .child(
            col()
                .grow(1.0)
                .w(0.0)
                .min_w(0.0)
                .child(ui::muted(meaning, t).wrap()),
        )
}

fn term(label: &str, t: &Theme) -> El {
    ui::caption(label, t).color(t.kit.c.text_base)
}

fn page_label(page: Page, t: &Theme) -> El {
    row()
        .center_y()
        .gap(8.0)
        .child(icon(page_icon(page), 15.0).color(t.kit.c.icon_muted))
        .child(term(page.label(), t))
}

pub fn dialog(s: &State, t: &Theme, layer: &mut Vec<El>) {
    let k = &t.kit;
    let start = [
        "Create a workspace, or open one you have.",
        "Add what you own, then save.",
        "Set a policy, then see where you stand in Review.",
    ];
    let current = s.has_portfolio().then(|| {
        section(
            &format!("You're on {}", s.page.label()),
            None,
            vec![ui::muted(about(s.page), t).wrap()],
            t,
        )
    });
    let body = col()
        .key("guide-body")
        .w_full()
        .h(480.0)
        .px(16.0)
        .pb(16.0)
        .scroll(ScrollSpec {
            stick_bottom: false,
        })
        .child(
            col()
                .w_full()
                .gap(20.0)
                .children(current)
                .child(section(
                    "Getting started",
                    None,
                    start
                        .iter()
                        .enumerate()
                        .map(|(index, step)| {
                            entry(term(&format!("Step {}", index + 1), t), step, t)
                        })
                        .collect(),
                    t,
                ))
                .children(GROUPS.map(|(heading, detail, pages)| {
                    section(
                        heading,
                        Some(detail),
                        pages
                            .iter()
                            .map(|page| entry(page_label(*page, t), about(*page), t))
                            .collect(),
                        t,
                    )
                }))
                .child(section(
                    "Words used here",
                    None,
                    TERMS
                        .iter()
                        .map(|(label, meaning)| entry(term(label, t), meaning, t))
                        .collect(),
                    t,
                ))
                .child(section(
                    "Shortcuts",
                    None,
                    SHORTCUTS
                        .iter()
                        .map(|(label, shift, key)| {
                            row()
                                .center_y()
                                .w_full()
                                .justify_between()
                                .gap(12.0)
                                .key(format!("guide-shortcut-{label}"))
                                .child(ui::muted(*label, t))
                                .child(Keybind::new(&ui::chord_keys(*shift, key)).build(k))
                        })
                        .collect(),
                    t,
                )),
        );
    Dialog::new("guide", "Guide")
        .open(true)
        .fit()
        .width(640.0)
        .description("What each page does and what its words mean.")
        .on_close(|s: &mut State| s.guide = false)
        .child(body)
        .build(k, layer);
}
