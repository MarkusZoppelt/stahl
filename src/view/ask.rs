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
    view::{charts, ui},
};

/// What a question looks like: a little quieter than the answer's first sentence.
const QUESTION: TextStyle = SECTION;

/// How long a word of the first sentence takes to fade in as it lands.
const REVEAL_FADE: f32 = 0.18;

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

/// The Overview's own line into Counsel: an input, and the questions this portfolio raises.
pub fn overview_line(s: &State, t: &Theme, narrow: bool) -> El {
    let k = &t.kit;
    let input = TextInput::new(&s.ask.input)
        .placeholder("What's on your mind?")
        .leading_icon(&icons::SPARKLES)
        .label("Ask counsel about your portfolio")
        .plain()
        .width(None)
        .on_edit(|s: &mut State| &mut s.ask.input)
        .on_key(|s: &mut State, event, _| {
            if event.key == Key::Enter {
                s.ask_here();
                return true;
            }
            false
        })
        .build(k)
        .key("overview-ask");
    let field = row()
        .center_y()
        .w_full()
        .max_w(if narrow { 100_000.0 } else { 460.0 })
        .h(34.0)
        .px(10.0)
        .radius(8.0)
        .border(1.0, k.c.border_muted)
        .hover(k.c.hover)
        .child(input);
    // On a phone the total stays close to the top, so only the two nearest questions show.
    let mut questions = suggestions(s);
    if narrow {
        questions.truncate(2);
    }
    let chips = questions.into_iter().enumerate().map(|(index, question)| {
        let asked = question.clone();
        ui::entering(
            Button::new(question)
                .variant(ButtonVariant::Ghost)
                .control_size(ControlSize::Small)
                .icon(&icons::SPARKLES)
                .on_press(move |s: &mut State| s.ask_about(asked.clone()))
                .build(k)
                .key(format!("overview-ask-{index}")),
            index,
        )
    });
    let chips = if narrow {
        col().w_full().items_start().gap(2.0).children(chips)
    } else {
        row().center_y().w_full().gap(4.0).children(chips)
    };
    ui::entering(
        col()
            .key("overview-counsel")
            .w_full()
            .gap(6.0)
            .child(field)
            .child(chips),
        0,
    )
}

/// What Counsel says before anything has been asked: the door, in words, and the questions this
/// portfolio makes worth asking.
fn empty(s: &State, t: &Theme) -> El {
    let k = &t.kit;
    let questions = questions(s);
    col()
        .key("ask-empty")
        .w_full()
        .max_w(680.0)
        .gap(if s.compact { 20.0 } else { 28.0 })
        .pt(if s.compact { 8.0 } else { 32.0 })
        .child(
            col()
                .gap(10.0)
                .child(
                    text("The door is open, and the light is on.")
                        .font(if s.compact { LEDE_COMPACT } else { LEDE })
                        .color(k.c.text_base)
                        .wrap(),
                )
                .child(
                    text(
                        "Ask anything about your portfolio. Every figure in the answer is one \
                         Stahl computed.",
                    )
                    .font(k.ty.ui)
                    .color(k.c.text_muted)
                    .wrap()
                    .max_w(520.0),
                ),
        )
        .children((!questions.is_empty()).then(|| {
            col()
                .key("ask-empty-suggestions")
                .w_full()
                .child(ui::rule(t))
                .children(
                    questions
                        .into_iter()
                        .enumerate()
                        .map(|(index, question)| suggestion(question, index, t)),
                )
        }))
}

/// The questions an empty page offers: what the portfolio raises, and what a what-if would show.
/// Every one is answerable from the app's own figures, with no model.
pub fn questions(s: &State) -> Vec<String> {
    let mut out = suggestions(s);
    if let Some(class) = what_if_class(s) {
        out.push(format!("What if {class} falls 20%?"));
    }
    if s.summary
        .as_ref()
        .is_some_and(|summary| summary.cash_value.abs() >= 0.005)
    {
        out.push("How much cash do I hold?".to_string());
    }
    out.dedup();
    out.truncate(6);
    out
}

/// The class worth pushing in a what-if: the one furthest from its target, or the largest when
/// the policy doesn't say.
fn what_if_class(s: &State) -> Option<String> {
    let drifted = s.review.data.as_ref().and_then(|review| {
        review
            .allocations
            .iter()
            .max_by(|a, b| a.drift_percent.abs().total_cmp(&b.drift_percent.abs()))
            .map(|class| class.asset_class.clone())
    });
    drifted.or_else(|| {
        s.allocation
            .iter()
            .max_by(|a, b| a.value.total_cmp(&b.value))
            .map(|class| class.asset_class.clone())
    })
}

/// A question worth asking, as a line in a ledger: a rule under it, and room around it.
fn suggestion(question: String, index: usize, t: &Theme) -> El {
    let k = &t.kit;
    let shown = question.clone();
    ui::entering(
        col()
            .w_full()
            .child(
                row()
                    .center_y()
                    .key(format!("ask-suggestion-{index}"))
                    .role("button")
                    .label(question.clone())
                    .focusable(true)
                    .w_full()
                    .gap(12.0)
                    .px(10.0)
                    .py(13.0)
                    .radius(6.0)
                    .hover(k.c.hover)
                    .press_scale(ui::PRESS_ROW)
                    .on_press(move |s: &mut State| s.ask_question(shown.clone()))
                    .child(icon(&icons::SPARKLES, 14.0).color(k.c.text_faint))
                    .child(
                        text(question)
                            .font(k.ty.ui)
                            .color(k.c.text_base)
                            .grow(1.0)
                            .min_w(0.0)
                            .truncate(),
                    )
                    .child(icon(&icons::CHEVRON_RIGHT, 14.0).color(k.c.text_faint)),
            )
            .child(ui::rule(t)),
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

/// Where a block sits in an answer: which turn, which block, and how long ago it landed. What
/// the page draws around it depends on all three.
#[derive(Clone)]
pub struct Slot {
    /// The block's key, which names every element it draws.
    pub key: String,
    pub turn: usize,
    pub at: usize,
    pub age: Option<f64>,
}

fn turn_view(s: &State, t: &Theme, cx: &Cx<State>, turn: &Turn, index: usize) -> El {
    let table = crate::facts::of(s);
    let mut first_say = true;
    let blocks: Vec<El> = turn
        .blocks
        .iter()
        .enumerate()
        .filter_map(|(at, block)| {
            let slot = Slot {
                key: format!("ask-{index}-{at}"),
                turn: index,
                at,
                age: turn.age(at, s.now),
            };
            let el = block_view(s, t, cx, block, &table, &mut first_say, &slot)?;
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
        answer = answer
            .child(say(text, first_say, &table, s, t, None).key(format!("ask-{index}-partial")));
    }
    if let Some(status) = status(s, t, turn, index) {
        answer = answer.child(status);
    }
    answer
}

/// What a turn says while it is waiting, stopped or failed.
fn status(s: &State, t: &Theme, turn: &Turn, index: usize) -> Option<El> {
    let k = &t.kit;
    match turn.phase {
        // The sidebar's door already turns its tumblers while a question is out, so the page
        // shows nothing but the question until the first word. On a phone the door is small,
        // so a quiet line stands in for it.
        ask::Phase::Waiting if s.compact => Some(
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
        ask::Phase::Waiting => None,
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
    slot: &Slot,
) -> Option<El> {
    let key = slot.key.clone();
    match block {
        Block::Say { text: words } => {
            // The first sentence of an answer arrives as words, not as a finished paragraph.
            let visible = s
                .ask
                .reveal
                .as_ref()
                .filter(|reveal| reveal.turn == slot.turn && reveal.block == slot.at)
                .filter(|_| !cx.is_reduced_motion())
                .map(|reveal| reveal.words);
            let el = say(words, *first_say, table, s, t, visible).key(format!("{key}-say"));
            *first_say = false;
            Some(el)
        }
        Block::Figures { ids } => figures(ids, table, s, t, &key, ui::room(s, cx), slot),
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
        Block::Plan { fraction, amount } => {
            Some(super::answer::plan_block(s, t, *fraction, *amount, slot))
        }
        Block::Scenario { shocks } => Some(super::answer::scenario(
            s,
            t,
            shocks,
            slot,
            cx.is_reduced_motion(),
        )),
        Block::Go { steps } => Some(go(steps, t, &key)),
    }
}

/// How a sentence's figures are set: the app's own figures a little heavier, a figure the model
/// wrote underlined in the warning colour.
enum Mark {
    Strong,
    Unverified,
}

/// A sentence with its figures filled in from the fact table. `visible` cuts it after that many
/// words, for the first sentence as it arrives.
fn say(
    words: &str,
    first: bool,
    table: &Table,
    s: &State,
    t: &Theme,
    visible: Option<usize>,
) -> El {
    let pieces = ask::pieces(words, table);
    let mut content = String::new();
    let mut marks: Vec<(usize, usize, Mark)> = Vec::new();
    let mut unverified = false;
    let currency = s.currency();
    for piece in pieces {
        match piece {
            Piece::Text(text) => content.push_str(&text),
            Piece::Chip(id) => {
                let start = content.len();
                let shown = table.get(&id).map_or_else(
                    || "—".to_string(),
                    |fact| ask::figure(fact, &content, s.privacy, &currency),
                );
                content.push_str(&shown);
                marks.push((start, content.len(), Mark::Strong));
            }
            Piece::Gap(_) => {
                let start = content.len();
                content.push('—');
                marks.push((start, content.len(), Mark::Unverified));
                unverified = true;
            }
            Piece::Unverified(digits) => {
                let start = content.len();
                content.push_str(&digits);
                marks.push((start, content.len(), Mark::Unverified));
                unverified = true;
            }
        }
    }
    if let Some(count) = visible {
        content.truncate(ask::upto_words(&content, count));
    }
    let shown = content.len();
    let spans: Vec<Span> = marks
        .into_iter()
        .filter(|(start, _, _)| *start < shown)
        .map(|(start, end, mark)| {
            let span = Span::new(start, end.min(shown));
            match mark {
                Mark::Strong => span.weight(560.0).color(t.kit.c.text_base),
                Mark::Unverified => span.color(t.warn).underline(t.warn),
            }
        })
        .collect();
    let mut body = rich(content, spans)
        .font(if first {
            if s.compact { LEDE_COMPACT } else { LEDE }
        } else {
            t.kit.ty.ui
        })
        .color(t.kit.c.text_base)
        .wrap()
        .selectable(true)
        .max_w(760.0);
    if first {
        // Words, not a paragraph: each one fades in as it lands.
        body = body.reveal(REVEAL_FADE);
    }
    if unverified {
        body.help(UNVERIFIED)
    } else {
        body
    }
}

fn shown(fact: &Fact, s: &State) -> String {
    fact.value.render(s.privacy, &s.currency())
}

/// Whether this is the first answer to quote `id`: a figure counts up from zero the first time,
/// and from the figure the last answer showed after that.
fn first_time(s: &State, turn: usize, id: &str) -> bool {
    !s.ask.turns.iter().take(turn).any(|earlier| {
        earlier.blocks.iter().any(|block| match block {
            Block::Figures { ids } => ids.iter().any(|each| each == id),
            _ => false,
        })
    })
}

fn figures(
    ids: &[String],
    table: &Table,
    s: &State,
    t: &Theme,
    key: &str,
    room: ui::Room,
    slot: &Slot,
) -> Option<El> {
    let facts: Vec<&Fact> = ids.iter().filter_map(|id| table.get(id)).take(4).collect();
    if facts.is_empty() {
        return None;
    }
    let currency = s.currency();
    let figure = |fact: &Fact, index: usize| {
        let value = shown(fact, s);
        // A figure that has just arrived counts up from zero; the rolling digits do the counting.
        let counting =
            slot.age.is_some_and(|age| age < ask::COUNT_HOLD) && first_time(s, slot.turn, &fact.id);
        let text = if counting {
            ask::zero(fact, s.privacy, &currency).unwrap_or_else(|| value.clone())
        } else {
            value
        };
        col()
            .gap(4.0)
            .min_w(0.0)
            .child(ui::caption(fact.short.clone(), t).wrap())
            .child(
                // Keyed by the figure, so a later answer rolls from the one before it.
                RollingText::new(format!("ask-figure-{}", fact.id), text)
                    .font(FIGURE)
                    .color(t.kit.c.text_base)
                    .build(&t.kit)
                    .key(format!("{key}-figure-{index}")),
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
            .max_w(900.0)
            .gap(4.0)
            .child(charts::value_chart(
                s,
                &history.points,
                t,
                cx,
                &format!("{key}-value"),
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
                .child(ui::caption(source_label(s), t).wrap().shrink())
                .children((s.ask_source() == Source::Builtin).then(|| {
                    Button::new("Settings")
                        .variant(ButtonVariant::Ghost)
                        .control_size(ControlSize::Small)
                        .on_press(|s: &mut State| s.navigate(Page::Settings))
                        .build(k)
                        .key("ask-settings")
                })),
        )
}

fn source_label(s: &State) -> String {
    match s.ask_source() {
        Source::Http { model, host } => Source::Http { model, host }.label(),
        _ => "Answered on this computer. Add a model in Settings to ask anything.".to_string(),
    }
}
