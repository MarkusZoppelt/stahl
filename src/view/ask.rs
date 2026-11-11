//! The Ask page: questions in the serif, answers made of the app's own interface.
//!
//! Every block here is built from what the pages already draw — `ui::stat`, the value chart, the
//! allocation bars, the rebalance plan — so an answer never invents a look of its own. The only
//! new look is the unverified figure: a model-written number, underlined in the warning colour.

use rune::{Cx, El, Key, Span, TextStyle, col, icon, rich, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, RollingText, TextInput};

use crate::{
    ask::{self, Block, Piece, Sort, Source, Step, Turn},
    facts::{Fact, Table},
    format, icons,
    state::{Page, State},
    theme::{FIGURE, LEDE, LEDE_COMPACT, SECTION, Theme},
    view::{charts, rebalance as plan, ui},
};

/// What a question looks like: a little quieter than the answer's first sentence.
const QUESTION: TextStyle = SECTION;

/// The one sentence that explains an unverified figure.
const UNVERIFIED: &str = "Underlined figures were written by the model. Stahl didn't compute them.";

/// The Ask page: its heading, the conversation, and the input pinned under it.
pub fn page(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    let margin = if s.compact { 16.0 } else { 32.0 };
    col().key("main").w_full().h_full().min_h(0.0).child(
        col()
            .key("page-nav-ask")
            .w_full()
            .h_full()
            .min_h(0.0)
            .max_w(1240.0)
            .px(margin)
            .pt(20.0)
            .pb(if s.compact { 12.0 } else { 20.0 })
            .gap(16.0)
            .transition(ui::page_arrive(super::travel(s), s.compact))
            .children(s.error.as_ref().map(|error| ui::danger(error.clone(), t)))
            .child(super::page_heading(s, t))
            .child(thread(s, t, cx))
            .child(composer(s, t)),
    )
}

/// The questions and their answers, newest at the bottom.
fn thread(s: &State, t: &Theme, cx: &Cx<State>) -> El {
    col()
        .key("ask-thread")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .scroll(rune::ScrollSpec { stick_bottom: true })
        .child(
            col()
                .key("ask-turns")
                .w_full()
                .max_w(900.0)
                .gap(if s.compact { 28.0 } else { 36.0 })
                .pb(8.0)
                .children(if s.ask.is_empty() {
                    vec![empty(s, t)]
                } else {
                    s.ask
                        .turns
                        .iter()
                        .enumerate()
                        .map(|(index, turn)| turn_view(s, t, cx, turn, index))
                        .collect()
                }),
        )
}

/// What Counsel says before anything has been asked.
fn empty(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let questions = suggestions(s);
    col()
        .key("ask-empty")
        .w_full()
        .gap(16.0)
        .pt(if s.compact { 8.0 } else { 24.0 })
        .child(
            text("What would you like to know?")
                .font(if s.compact { LEDE_COMPACT } else { LEDE })
                .color(k.c.text_base)
                .wrap(),
        )
        .child(
            text(
                "Ask in your own words. The answer is built from your own figures and screens, \
                 and every number in it is one Stahl computed.",
            )
            .font(k.ty.ui)
            .color(k.c.text_muted)
            .wrap()
            .max_w(560.0),
        )
        .children((!questions.is_empty()).then(|| {
            col()
                .key("ask-empty-suggestions")
                .w_full()
                .max_w(560.0)
                .gap(8.0)
                .pt(4.0)
                .children(
                    questions
                        .into_iter()
                        .enumerate()
                        .map(|(index, question)| suggestion(question, index, t)),
                )
        }))
}

/// A question worth asking, as a row you can press.
fn suggestion(question: String, index: usize, t: &Theme) -> El {
    let k = &t.kit;
    let shown = question.clone();
    ui::entering(
        row()
            .center_y()
            .key(format!("ask-suggestion-{index}"))
            .role("button")
            .label(question.clone())
            .focusable(true)
            .w_full()
            .gap(10.0)
            .px(12.0)
            .py(9.0)
            .radius(6.0)
            .border(1.0, k.c.border_muted)
            .hover(k.c.hover)
            .press_scale(ui::PRESS_ROW)
            .on_press(move |s: &mut State| s.ask_question(shown.clone()))
            .child(icon(&icons::SPARKLES, 14.0).color(k.c.icon_muted))
            .child(
                text(question)
                    .font(k.ty.ui)
                    .color(k.c.text_base)
                    .grow(1.0)
                    .truncate(),
            )
            .child(icon(&icons::CHEVRON_RIGHT, 14.0).color(k.c.text_faint)),
        index,
    )
}

/// The questions this portfolio makes worth asking, from what the app already knows. Nothing is
/// inferred: each one is about a figure or a finding that exists.
pub fn suggestions(s: &State) -> Vec<String> {
    let mut out = Vec::new();
    if !s.has_portfolio() || s.positions.is_empty() {
        return out;
    }
    // What the agenda found, as a question about the same thing.
    if let Some(item) = s.agenda().first() {
        match item.id.as_str() {
            "drift" => {
                if let Some(class) = s
                    .review
                    .data
                    .as_ref()
                    .and_then(|review| {
                        review
                            .allocations
                            .iter()
                            .filter(|class| !class.within_tolerance)
                            .max_by(|a, b| a.drift_percent.abs().total_cmp(&b.drift_percent.abs()))
                    })
                    .map(|class| class.asset_class.clone())
                {
                    out.push(format!("Why is {class} off its target?"));
                } else {
                    out.push("Why is my allocation off?".to_string());
                }
            }
            "cash" => out.push("How much cash do I hold?".to_string()),
            _ => out.push(item.title.clone()),
        }
    }
    if s.is_workspace() {
        out.push("Where should new money go?".to_string());
    }
    if s.change.is_some() {
        out.push("What changed since I last looked?".to_string());
    }
    if let Some(today) = s
        .positions
        .iter()
        .filter_map(|position| {
            let name = position.name.clone().or_else(|| position.ticker.clone())?;
            Some((name, position.day_change_percent?))
        })
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
    {
        out.push(format!("How is {} doing?", today.0));
    }
    out.truncate(3);
    out
}

fn turn_view(s: &State, t: &Theme, cx: &Cx<State>, turn: &Turn, index: usize) -> El {
    let table = crate::facts::of(s);
    let mut first_say = true;
    let blocks: Vec<El> = turn
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(at, block)| {
            let key = format!("ask-{index}-{at}");
            let el = block_view(s, t, cx, block, &table, &mut first_say, &key)?;
            Some(ui::entering(el, at))
        })
        .collect();
    let mut answer = col()
        .key(format!("ask-turn-{index}"))
        .w_full()
        .gap(16.0)
        .child(
            text(turn.question.clone())
                .key(format!("ask-question-{index}"))
                .font(QUESTION)
                .color(t.kit.c.text_muted)
                .wrap()
                .selectable(true),
        )
        .children(blocks);
    let current = index + 1 == s.ask.turns.len();
    if current
        && turn.phase == ask::Phase::Streaming
        && let Some(text) = s.ask.partial.as_deref()
        && !text.is_empty()
    {
        answer =
            answer.child(say(text, first_say, &table, s, t).key(format!("ask-{index}-partial")));
    }
    if let Some(status) = status(s, t, turn, index) {
        answer = answer.child(status);
    }
    answer
}

/// What a turn says while it is waiting, stopped or failed.
fn status(s: &State, t: &Theme, turn: &Turn, index: usize) -> Option<El> {
    let k = &t.kit;
    let _ = s;
    match turn.phase {
        ask::Phase::Waiting => Some(
            row()
                .key(format!("ask-waiting-{index}"))
                .role("status")
                .center_y()
                .gap(8.0)
                .child(row().gap(3.0).center_y().children((0..3).map(|dot| {
                    rune::el()
                        .size(4.0, 4.0)
                        .radius(2.0)
                        .fill(k.c.text_muted)
                        .cycle(ui::BREATHE)
                        .transition(rune::Transition::opacity())
                        .enter_delay(dot as f32 * 0.16)
                })))
                .child(ui::caption("Reading your portfolio", t)),
        ),
        ask::Phase::Failed => Some(
            col()
                .key(format!("ask-failed-{index}"))
                .w_full()
                .gap(10.0)
                .items_start()
                .child(ui::danger(
                    turn.error.clone().unwrap_or_else(|| {
                        "The answer didn't arrive. Nothing was lost; try again when you're ready."
                            .to_string()
                    }),
                    t,
                ))
                .child(
                    Button::new("Try again")
                        .variant(ButtonVariant::Outline)
                        .control_size(ControlSize::Small)
                        .icon(&icons::REFRESH)
                        .on_press(|s: &mut State| s.ask_retry())
                        .build(k)
                        .key(format!("ask-retry-{index}")),
                ),
        ),
        ask::Phase::Stopped => Some(
            row()
                .key(format!("ask-stopped-{index}"))
                .center_y()
                .w_full()
                .gap(10.0)
                .child(ui::caption("Stopped. Ask again whenever you like.", t))
                .child(
                    Button::new("Ask again")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut State| s.ask_retry())
                        .build(k)
                        .key(format!("ask-again-{index}")),
                ),
        ),
        ask::Phase::Streaming => None,
        ask::Phase::Done if turn.blocks.is_empty() => {
            Some(ui::caption("There was nothing to show for that.", t))
        }
        ask::Phase::Done => None,
    }
}

fn block_view(
    s: &State,
    t: &Theme,
    cx: &Cx<State>,
    block: &Block,
    table: &Table,
    first_say: &mut bool,
    key: &str,
) -> Option<El> {
    let key = key.to_string();
    match block {
        Block::Say { text: words } => {
            let el = say(words, *first_say, table, s, t).key(format!("{key}-say"));
            *first_say = false;
            Some(el)
        }
        Block::Figures { ids } => figures(ids, table, s, t, &key, ui::room(s, cx)),
        Block::Chart { period, span } => chart(s, t, cx, period.as_deref(), *span, &key),
        Block::Holdings { class, sort, map } => {
            if *map == Some(true) {
                // The treemap is the Holdings page's own; a block points there rather than
                // drawing a second one with its own hover state.
                Some(
                    col()
                        .gap(8.0)
                        .child(ui::caption("The map of your holdings is on Holdings.", t))
                        .child(open_button("Open Holdings", Page::Holdings, t, &key)),
                )
            } else {
                Some(holdings(s, t, class.as_deref(), *sort, &key))
            }
        }
        Block::Allocation => Some(allocation(s, t, &key)),
        Block::Plan { fraction, amount } => Some(plan_block(s, t, *fraction, *amount, &key)),
        Block::Scenario { shocks } => Some(scenario_block(s, t, shocks, &key)),
        Block::Go { steps } => Some(go(steps, t, &key)),
    }
}

/// A sentence with its figures filled in from the fact table.
fn say(words: &str, first: bool, table: &Table, s: &State, t: &Theme) -> El {
    let pieces = ask::pieces(words, table);
    let mut content = String::new();
    let mut spans = Vec::new();
    let mut unverified = false;
    for piece in pieces {
        match piece {
            Piece::Text(text) => content.push_str(&text),
            Piece::Chip(id) => {
                let start = content.len();
                let shown = table
                    .get(&id)
                    .map_or_else(|| "—".to_string(), |fact| shown(fact, s));
                content.push_str(&shown);
                spans.push(
                    Span::new(start, content.len())
                        .weight(560.0)
                        .color(t.kit.c.text_base),
                );
            }
            Piece::Gap(_) => {
                let start = content.len();
                content.push('—');
                spans.push(
                    Span::new(start, content.len())
                        .color(t.warn)
                        .underline(t.warn),
                );
                unverified = true;
            }
            Piece::Unverified(digits) => {
                let start = content.len();
                content.push_str(&digits);
                spans.push(
                    Span::new(start, content.len())
                        .color(t.warn)
                        .underline(t.warn),
                );
                unverified = true;
            }
        }
    }
    let body = rich(content, spans)
        .font(if first {
            if s.compact { LEDE_COMPACT } else { LEDE }
        } else {
            t.kit.ty.ui
        })
        .color(t.kit.c.text_base)
        .wrap()
        .selectable(true)
        .max_w(760.0);
    if unverified {
        body.help(UNVERIFIED)
    } else {
        body
    }
}

fn shown(fact: &Fact, s: &State) -> String {
    fact.value.render(s.privacy, &s.currency())
}

fn figures(
    ids: &[String],
    table: &Table,
    s: &State,
    t: &Theme,
    key: &str,
    room: ui::Room,
) -> Option<El> {
    let facts: Vec<&Fact> = ids.iter().filter_map(|id| table.get(id)).take(4).collect();
    if facts.is_empty() {
        return None;
    }
    let figure = |fact: &Fact, index: usize| {
        col()
            .gap(4.0)
            .min_w(0.0)
            .child(ui::caption(fact.short.clone(), t).wrap())
            .child(
                RollingText::new(format!("{key}-figure-{index}"), shown(fact, s))
                    .font(FIGURE)
                    .color(t.kit.c.text_base)
                    .build(&t.kit),
            )
    };
    Some(
        ui::columns(room, [4, 2, 2], 16.0).max_w(760.0).children(
            facts
                .into_iter()
                .enumerate()
                .map(|(index, fact)| figure(fact, index)),
        ),
    )
}

fn chart(
    s: &State,
    t: &Theme,
    cx: &Cx<State>,
    period: Option<&str>,
    span: Option<[usize; 2]>,
    key: &str,
) -> Option<El> {
    let history = s.ask_history(period)?;
    if history.points.len() < 2 {
        return None;
    }
    let _ = span;
    Some(
        col()
            .key(format!("{key}-chart"))
            .w_full()
            .gap(4.0)
            .child(charts::value_chart(
                s,
                &history.points,
                t,
                cx,
                "ask-chart",
                220.0,
            ))
            .child(ui::caption(s.ask_chart_name(period), t)),
    )
}

/// A holding's row in a `holdings` block.
fn holdings(s: &State, t: &Theme, class: Option<&str>, sort: Option<Sort>, key: &str) -> El {
    let mut positions: Vec<&portfolio_rs::state::PositionDto> = s
        .positions
        .iter()
        .filter(|position| {
            class.is_none_or(|class| position.asset_class.eq_ignore_ascii_case(class))
        })
        .collect();
    let total = s
        .summary
        .as_ref()
        .map_or(0.0, |summary| summary.total_value);
    match sort.unwrap_or(Sort::Value) {
        Sort::Today => positions.sort_by(|a, b| {
            let impact = |position: &portfolio_rs::state::PositionDto| {
                crate::view::portfolio::day_amount(position).map_or(0.0, f64::abs)
            };
            impact(b).total_cmp(&impact(a))
        }),
        Sort::Value | Sort::Share => positions.sort_by(|a, b| b.value.total_cmp(&a.value)),
    }
    positions.truncate(8);
    let rows = positions
        .iter()
        .map(|position| {
            let name = position
                .name
                .clone()
                .or_else(|| position.ticker.clone())
                .unwrap_or_else(|| "A holding".to_string());
            vec![
                row()
                    .center_y()
                    .gap(8.0)
                    .child(ui::dot(t.class_color(&position.asset_class)))
                    .child(
                        text(name)
                            .font(t.kit.ty.ui)
                            .color(t.kit.c.text_base)
                            .truncate(),
                    ),
                ui::number(s.money(position.value), t.kit.c.text_base),
                ui::number(
                    if total.abs() > 0.005 {
                        format!("{:.1}%", position.value / total * 100.0)
                    } else {
                        "-".to_string()
                    },
                    t.kit.c.text_muted,
                ),
                position.day_change_percent.map_or_else(
                    || ui::number("-", t.kit.c.text_faint),
                    |today| ui::badge(format::percent(today), t.change(today)),
                ),
            ]
        })
        .collect();
    let heading = class.map_or_else(
        || "Your holdings".to_string(),
        |class| format!("Only {class}"),
    );
    let detail = if class.is_some() {
        "The holdings in this class, by value."
    } else {
        "By value, unless the question asked for today's moves."
    };
    col()
        .key(format!("{key}-holdings"))
        .w_full()
        .gap(8.0)
        .child(ui::header(heading, detail, t))
        .child(ui::table(&["Holding", "Value", "Share", "Today"], rows, t))
        .child(
            row()
                .items_start()
                .child(open_button("Open Holdings", Page::Holdings, t, key)),
        )
}

/// Today's shares against the policy's targets, furthest off first.
fn allocation(s: &State, t: &Theme, key: &str) -> El {
    let mut classes: Vec<_> = s.allocation.iter().collect();
    let drift = |class: &&portfolio_rs::state::AllocationItemDto| {
        s.review
            .data
            .as_ref()
            .and_then(|review| {
                review
                    .allocations
                    .iter()
                    .find(|each| each.asset_class.eq_ignore_ascii_case(&class.asset_class))
            })
            .map_or(0.0, |each| each.drift_percent.abs())
    };
    classes.sort_by(|a, b| drift(b).total_cmp(&drift(a)));
    let rows = classes.iter().map(|class| {
        let target = s.review.data.as_ref().and_then(|review| {
            review
                .allocations
                .iter()
                .find(|each| each.asset_class.eq_ignore_ascii_case(&class.asset_class))
                .map(|each| each.target_percent)
        });
        col()
            .w_full()
            .gap(5.0)
            .child(
                row()
                    .center_y()
                    .w_full()
                    .gap(8.0)
                    .child(ui::dot(t.class_color(&class.asset_class)))
                    .child(
                        text(class.asset_class.clone())
                            .font(t.kit.ty.ui)
                            .color(t.kit.c.text_base)
                            .grow(1.0)
                            .truncate(),
                    )
                    .child(ui::number(
                        format!("{:.1}%", class.percent),
                        t.kit.c.text_base,
                    ))
                    .children(target.map(|target| {
                        ui::number(format!("{target:.1}% target"), t.kit.c.text_faint)
                    })),
            )
            .child(ui::share_bar(
                format!("{key}-share-{}", class.asset_class),
                class.percent,
                target,
                t.class_color(&class.asset_class),
                t,
            ))
    });
    col()
        .key(format!("{key}-allocation"))
        .w_full()
        .gap(10.0)
        .child(ui::header(
            "Today against your targets",
            "The tick on each bar is the policy's target.",
            t,
        ))
        .children(rows)
        .child(
            row()
                .items_start()
                .child(open_button("Open Allocation", Page::Allocation, t, key)),
        )
}

/// A rebalance plan, compactly, with the way to open it on the Rebalance page.
fn plan_block(s: &State, t: &Theme, fraction: Option<f64>, amount: Option<f64>, key: &str) -> El {
    let classes = plan::load_classes(s);
    let (plan, title, detail) = match (fraction, amount) {
        (Some(fraction), _) => {
            let plan = crate::rebalance::blend(&classes, fraction);
            let title = if fraction >= 0.999 {
                "All the way to your targets".to_string()
            } else {
                format!(
                    "{}% of the way to your targets",
                    (fraction * 100.0).round() as i64
                )
            };
            (plan, title, "The total stays where it is.".to_string())
        }
        (_, Some(amount)) if amount >= 0.0 => (
            crate::rebalance::place(&classes, amount)
                .unwrap_or_else(|_| crate::rebalance::today(&classes)),
            "Where new money goes".to_string(),
            format!("Buying only, {} in all.", s.money(amount)),
        ),
        (_, Some(amount)) => (
            crate::rebalance::place(&classes, amount)
                .unwrap_or_else(|_| crate::rebalance::today(&classes)),
            "Where a withdrawal comes from".to_string(),
            format!("Selling only, {} in all.", s.money(-amount)),
        ),
        _ => (
            crate::rebalance::today(&classes),
            String::new(),
            String::new(),
        ),
    };
    let trades = plan.trades.iter().map(|trade| {
        row()
            .center_y()
            .w_full()
            .gap(10.0)
            .child(ui::dot(t.class_color(&trade.asset_class)))
            .child(
                text(trade.asset_class.clone())
                    .font(t.kit.ty.ui)
                    .color(t.kit.c.text_base)
                    .grow(1.0)
                    .truncate(),
            )
            .child(ui::caption(trade.action.label(), t))
            .child(ui::number(s.money(trade.amount), t.kit.c.text_base))
    });
    let open = move |s: &mut State| {
        if let Some(amount) = amount {
            plan::set_amount(s, amount);
        } else {
            plan::set_blend(s, fraction.unwrap_or(0.0));
        }
        s.navigate(Page::Rebalance);
    };
    col()
        .key(format!("{key}-plan"))
        .w_full()
        .gap(8.0)
        .child(ui::header(title, detail, t))
        .children(trades)
        .child(
            row().items_start().child(
                Button::new("Open in Rebalance")
                    .variant(ButtonVariant::Outline)
                    .control_size(ControlSize::Small)
                    .icon(&icons::SCALE)
                    .on_press(open)
                    .build(&t.kit)
                    .key(format!("{key}-open-plan")),
            ),
        )
}

/// A what-if: the model's shocks, the app's consequences.
fn scenario_block(s: &State, t: &Theme, shocks: &[crate::scenario::Shock], key: &str) -> El {
    let classes = plan::load_classes(s);
    let outcome = crate::scenario::project(&classes, shocks);
    let name = |shock: &crate::scenario::Shock| {
        if shock.percent <= -100.0 {
            format!("{} goes to nothing", shock.class)
        } else if shock.percent < 0.0 {
            format!("{} falls {:.0}%", shock.class, -shock.percent)
        } else if shock.percent > 0.0 {
            format!("{} rises {:.0}%", shock.class, shock.percent)
        } else {
            format!("{} stands still", shock.class)
        }
    };
    let words = if shocks.is_empty() {
        "Nothing changes.".to_string()
    } else {
        let list = shocks.iter().map(name).collect::<Vec<_>>().join(", ");
        match outcome.percent() {
            Some(percent) => format!(
                "If {list}, your total goes from {} to {} ({}).",
                s.money(outcome.total_before),
                s.money(outcome.total_after),
                format::percent(percent)
            ),
            None => format!("If {list}, there is no total to change."),
        }
    };
    let rows = outcome
        .lines
        .iter()
        .filter(|line| line.before >= 0.005 || line.after >= 0.005)
        .map(|line| {
            let note = if line.leaves_band() {
                Some("leaves its band")
            } else if line.returns_to_band() {
                Some("back inside its band")
            } else {
                None
            };
            vec![
                row()
                    .center_y()
                    .gap(8.0)
                    .child(ui::dot(t.class_color(&line.name)))
                    .child(
                        text(line.name.clone())
                            .font(t.kit.ty.ui)
                            .color(t.kit.c.text_base)
                            .truncate(),
                    ),
                ui::number(format!("{:.1}%", line.before_share), t.kit.c.text_muted),
                ui::number(format!("{:.1}%", line.after_share), t.kit.c.text_base),
                note.map_or_else(
                    || ui::number("", t.kit.c.text_faint),
                    |note| ui::caption(note, t),
                ),
            ]
        })
        .collect();
    col()
        .key(format!("{key}-scenario"))
        .w_full()
        .gap(10.0)
        .child(ui::header("If that happened", words, t))
        .child(ui::table(&["Class", "Now", "Then", ""], rows, t))
        .children((!outcome.unknown.is_empty()).then(|| {
            ui::caption(
                format!(
                    "Nothing in the portfolio is called {}.",
                    outcome.unknown.join(", ")
                ),
                t,
            )
        }))
}

fn go(steps: &[Step], t: &Theme, key: &str) -> El {
    col()
        .key(format!("{key}-go"))
        .w_full()
        .gap(8.0)
        .items_start()
        .children(
            steps
                .iter()
                .take(3)
                .enumerate()
                .map(|(index, step)| step_button(step, t, format!("{key}-go-{index}"))),
        )
}

fn step_button(step: &Step, t: &Theme, key: String) -> El {
    let label = step.label.clone();
    let page = step.page.as_deref().and_then(Page::from_name);
    let question = step.ask.clone();
    let mut button = Button::new(label).variant(ButtonVariant::Outline);
    if let Some(page) = page {
        button = button
            .icon(super::page_icon(page))
            .on_press(move |s: &mut State| s.navigate(page));
    } else if let Some(question) = question {
        button = button
            .icon(&icons::SPARKLES)
            .on_press(move |s: &mut State| s.ask_question(question.clone()));
    }
    button.build(&t.kit).key(key)
}

fn open_button(label: &str, page: Page, t: &Theme, key: &str) -> El {
    Button::new(label)
        .variant(ButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .icon(&icons::EXTERNAL_LINK)
        .on_press(move |s: &mut State| s.navigate(page))
        .build(&t.kit)
        .key(format!("{key}-open"))
}

/// The input, pinned under the conversation, and where the answers come from.
fn composer(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let input = TextInput::new(&s.ask.input)
        .placeholder("What's on your mind?")
        .leading_icon(&icons::SPARKLES)
        .label("What's on your mind?")
        .width(None)
        .on_edit(|s: &mut State| &mut s.ask.input)
        .on_key(|s: &mut State, event, _| match event.key {
            Key::Enter => {
                s.ask_send();
                true
            }
            Key::Escape if s.ask_stop() => true,
            _ => false,
        })
        .build(k)
        .key("ask-input");
    let send = if s.ask.streaming {
        Button::new("Stop")
            .variant(ButtonVariant::Outline)
            .icon(&icons::X_CIRCLE)
            .shortcut("Esc")
            .on_press(|s: &mut State| {
                s.ask_stop();
            })
            .build(k)
            .key("ask-stop")
    } else {
        Button::new("Ask")
            .variant(ButtonVariant::Submit)
            .icon(&icons::ARROW_UP)
            .on_press(|s: &mut State| s.ask_send())
            .build(k)
            .key("ask-send")
    };
    col()
        .key("ask-composer")
        .w_full()
        .gap(8.0)
        .child(
            row()
                .center_y()
                .w_full()
                .gap(8.0)
                .child(col().grow(1.0).w(0.0).min_w(0.0).child(input))
                .child(send),
        )
        .child(
            row()
                .items_start()
                .w_full()
                .gap(6.0)
                .child(icon(&icons::SHIELD, 12.0).color(k.c.text_faint).mt(2.0))
                .child(ui::caption(source_label(s), t).wrap().shrink()),
        )
}

fn source_label(s: &State) -> String {
    match &s.ask.source {
        Source::Http { .. } => s.ask.source.label(),
        _ => {
            if s.has_api_key {
                // A provider is configured but this answer came from the app itself.
                "Answered on this computer".to_string()
            } else {
                "Answered on this computer. Add a model in Settings to ask anything.".to_string()
            }
        }
    }
}

/// A question's blocks as plain text, for a test to read.
pub fn plain(turn: &Turn, table: &Table, hidden: bool, currency: &str) -> String {
    turn.blocks
        .iter()
        .filter_map(|block| match block {
            Block::Say { text } => Some(ask::spoken(text, table, hidden, currency)),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}
