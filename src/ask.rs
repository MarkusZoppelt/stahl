//! Counsel: a question answered with the app's own interface.
//!
//! An answer is a list of [`Block`]s, one JSON object per line. The model writes the words and
//! names the figures; the app draws the figures from its [`crate::facts`] table, so a number about
//! the portfolio can only ever be one Stahl computed. Anything else the model writes with digits
//! is shown as unverified.

use std::{collections::VecDeque, mem};

use rune::TextEdit;
use serde::{Deserialize, Serialize};

use crate::{facts::Table, scenario, state::Page};
/// Which order a holdings block lists them in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sort {
    /// By how much the holding moved the portfolio today.
    Today,
    Value,
    Share,
}

/// One of a `go` block's suggestions: a page to open, or a question to ask next.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub label: String,
    /// A page's name, as [`crate::state::Page::from_name`] reads it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page: Option<String>,
    /// A follow-up question, sent as if it were typed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<String>,
}

/// One piece of an answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "block", rename_all = "snake_case")]
pub enum Block {
    /// A sentence or two, with `{fact.id}` placeholders where figures belong.
    Say { text: String },
    /// Two to four figures, by fact id.
    Figures { ids: Vec<String> },
    /// The value chart for a period, optionally with a span marked.
    Chart {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        period: Option<String>,
        /// The first and last day of the span, as indexes into the chart's points.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        span: Option<[usize; 2]>,
    },
    /// The holdings, optionally one class and in an order.
    Holdings {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sort: Option<Sort>,
        /// Draw the treemap instead of rows.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        map: Option<bool>,
    },
    /// Today's shares against the policy's targets.
    Allocation,
    /// A rebalance plan: part of the way to target, or an amount of new money.
    Plan {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fraction: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        amount: Option<f64>,
    },
    /// A what-if. The model names the shocks; the app computes everything else.
    Scenario { shocks: Vec<scenario::Shock> },
    /// One to three things to do next.
    Go { steps: Vec<Step> },
}

impl Block {
    /// The words of a `say`, or empty.
    pub fn text(&self) -> &str {
        match self {
            Block::Say { text } => text,
            _ => "",
        }
    }

    /// Whether the block draws anything at all with the data at hand. A block that can't be drawn
    /// is skipped rather than shown as a hole.
    pub fn is_empty(&self, table: &Table) -> bool {
        match self {
            Block::Say { text } => text.trim().is_empty(),
            Block::Figures { ids } => {
                ids.is_empty() || ids.iter().all(|id| table.get(id).is_none())
            }
            Block::Chart { .. } | Block::Allocation | Block::Scenario { .. } => false,
            Block::Holdings { .. } => false,
            Block::Plan { fraction, amount } => fraction.is_none() && amount.is_none(),
            Block::Go { steps } => steps.is_empty(),
        }
    }
}

/// One line of the model's answer, or why it can't be read.
pub fn parse_line(line: &str) -> Result<Block, String> {
    serde_json::from_str::<Block>(line.trim()).map_err(|error| error.to_string())
}

/// Reads an answer as it streams: whole lines become blocks, and a half-arrived line waits.
#[derive(Debug, Default)]
pub struct Lines {
    buffer: String,
    /// Lines that couldn't be read, for the log.
    pub skipped: Vec<String>,
}

impl Lines {
    pub fn new() -> Lines {
        Lines::default()
    }

    /// Add what arrived. Returns the blocks it completed, in order. A malformed line is skipped
    /// and kept in [`Lines::skipped`]: one bad line never breaks an answer.
    pub fn push(&mut self, text: &str) -> Vec<Block> {
        self.buffer.push_str(text);
        let mut blocks = Vec::new();
        while let Some(at) = self.buffer.find('\n') {
            let line: String = self.buffer.drain(..=at).collect();
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            match parse_line(line) {
                Ok(block) => blocks.push(block),
                Err(error) => {
                    eprintln!("ask: skipped a line: {error}");
                    self.skipped.push(line.to_string());
                }
            }
        }
        blocks
    }

    /// The line being written, if there is one.
    pub fn partial(&self) -> &str {
        self.buffer.trim()
    }

    /// Whatever is left when the stream ends, as a last line without its newline.
    pub fn finish(&mut self) -> Vec<Block> {
        if self.buffer.trim().is_empty() {
            self.buffer.clear();
            return Vec::new();
        }
        let mut text = mem::take(&mut self.buffer);
        text.push('\n');
        self.push(&text)
    }
}

/// A piece of a `say`: words as written, a figure the app computed, or something the model made up.
#[derive(Clone, Debug, PartialEq)]
pub enum Piece {
    Text(String),
    /// A figure from the fact table, by id.
    Chip(String),
    /// A placeholder naming no figure the app has. Drawn as a gap, never guessed.
    Gap(String),
    /// Digits the model wrote itself.
    Unverified(String),
}

/// The currency marks a number may carry, which belong to the figure rather than the words.
const MARKS: [char; 5] = ['€', '$', '£', '¥', '₹'];

/// Whether `text` is a placeholder id: letters, digits, dots, dashes and underscores.
fn is_id(text: &str) -> bool {
    !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn flush(out: &mut Vec<Piece>, run: &mut String) {
    if !run.is_empty() {
        out.push(Piece::Text(mem::take(run)));
    }
}

/// A `say` split into words, figures and anything unverified.
pub fn pieces(text: &str, table: &Table) -> Vec<Piece> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut words = String::new();
    let mut at = 0;
    while at < chars.len() {
        let character = chars[at];
        if character == '{'
            && let Some(close) = chars[at + 1..].iter().position(|c| *c == '}')
        {
            let id: String = chars[at + 1..at + 1 + close].iter().collect();
            let id = id.trim();
            if is_id(id) {
                flush(&mut out, &mut words);
                if table.get(id).is_some() {
                    out.push(Piece::Chip(id.to_string()));
                } else {
                    out.push(Piece::Gap(id.to_string()));
                }
                at += close + 2;
                continue;
            }
        }
        if character.is_ascii_digit() {
            let start = at;
            let mut end = at;
            while end < chars.len() {
                let c = chars[end];
                let separated = matches!(c, ',' | '.')
                    && end + 1 < chars.len()
                    && chars[end + 1].is_ascii_digit();
                if c.is_ascii_digit() || separated {
                    end += 1;
                } else {
                    break;
                }
            }
            if end < chars.len() && chars[end] == '%' {
                end += 1;
            }
            // A sign or a currency mark just before the digits belongs to the figure, so the
            // underline covers it too.
            let mut token = String::new();
            if MARKS.iter().any(|mark| words.ends_with(*mark)) {
                token.push(words.pop().expect("just checked"));
            }
            if words.ends_with('-') || words.ends_with('+') {
                token.push(words.pop().expect("just checked"));
            }
            flush(&mut out, &mut words);
            token.extend(&chars[start..end]);
            out.push(Piece::Unverified(token));
            at = end;
            continue;
        }
        words.push(character);
        at += 1;
    }
    flush(&mut out, &mut words);
    out
}

/// The plain text of a `say`, as a screen reader and a test read it: each figure replaced by what
/// it shows, each gap by a dash.
pub fn spoken(text: &str, table: &Table, hidden: bool, currency: &str) -> String {
    let mut out = String::new();
    for piece in pieces(text, table) {
        match piece {
            Piece::Text(words) => out.push_str(&words),
            Piece::Chip(id) => {
                let shown = table.get(&id).map_or_else(
                    || "—".to_string(),
                    |fact| figure(fact, &out, hidden, currency),
                );
                out.push_str(&shown);
            }
            Piece::Gap(_) => out.push('—'),
            Piece::Unverified(digits) => out.push_str(&digits),
        }
    }
    out
}

/// Words that already say which way a figure went. The figure after one is written without its
/// sign, as the Statement writes amounts in prose: "you are up €291", never "up +€291".
const DIRECTIONS: [&str; 6] = ["up", "down", "higher", "lower", "gained", "lost"];

/// Whether the words just before a figure carry its direction.
pub fn directed(words: &str) -> bool {
    words
        .split_whitespace()
        .next_back()
        .map(|word| {
            word.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .is_some_and(|word| DIRECTIONS.contains(&word.as_str()))
}

/// A figure as a sentence writes it, given the words before it: unsigned after a word that
/// already says the direction, so a loss never reads "down -€291".
pub fn figure(fact: &crate::facts::Fact, before: &str, hidden: bool, currency: &str) -> String {
    use crate::facts::Value;
    match (&fact.value, directed(before)) {
        (Value::SignedMoney(value), true) => Value::Money(value.abs()).render(hidden, currency),
        (Value::SignedPercent(value), true) => Value::Percent(value.abs()).render(hidden, currency),
        (value, _) => value.render(hidden, currency),
    }
}

/// The ids a `say` names, for a test to check the answer only quotes what exists.
pub fn named(text: &str) -> Vec<String> {
    pieces(text, &Table::default())
        .into_iter()
        .filter_map(|piece| match piece {
            Piece::Chip(id) | Piece::Gap(id) => Some(id),
            _ => None,
        })
        .collect()
}

/// What the model is told about the block vocabulary. One place, so the prompt and the parser
/// can't drift.
pub const VOCABULARY: &str = r#"Write one JSON object per line, and nothing else. Each line is one block:
{"block":"say","text":"..."}                     a sentence or two. {fact.id} where a figure belongs.
{"block":"figures","ids":["total","today.change"]} two to four figures, by fact id.
{"block":"chart","period":"month","span":[2,14]} the value chart. period: week, month, ytd, year, all. span is optional.
{"block":"holdings","class":"Crypto","sort":"today"} the holdings; class and sort (today, value, share) are optional.
{"block":"allocation"}                            today's shares against the policy's targets.
{"block":"plan","fraction":0.5}                   half the way to target. Or {"block":"plan","amount":2000} for new money.
{"block":"scenario","shocks":[{"class":"Crypto","percent":-50}]} a what-if. Only the shocks; Stahl computes the rest.
{"block":"go","steps":[{"label":"Open Rebalance","page":"rebalance"}]} up to three next steps; page or ask.

Never write a number about the portfolio yourself. Write {class.stocks.share}, not 37.2%.
Only the fact ids listed below exist. An id you invent is shown as a gap."#;

// --- The model ---

/// What a model is given before it answers.
pub struct Request {
    pub question: String,
    /// Earlier questions and the plain text of their answers, newest last.
    pub conversation: Vec<(String, String)>,
    /// The system prompt: the voice, the block vocabulary, the fact ids and the portfolio.
    pub system: String,
    /// Every figure the answer may quote, by id.
    pub facts: Table,
    /// Resolves when the owner stops the answer, so a request in flight is dropped.
    pub cancel: Option<tokio::sync::oneshot::Receiver<()>>,
}

impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request")
            .field("question", &self.question)
            .field("conversation", &self.conversation.len())
            .field("system", &self.system.len())
            .field("facts", &self.facts.facts.len())
            .finish()
    }
}

/// One piece of an answer as a provider sends it.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// Text of the answer, as it arrives. Whole lines become blocks.
    Text(String),
    Done,
    Failed(String),
}

/// What a model has to say for itself.
pub enum Reply {
    /// The whole answer as JSON Lines, ready now. The page stages the blocks on its own clock,
    /// so a demo and a screenshot are deterministic.
    Ready(String),
    /// Pieces of the answer from somewhere else, read as they arrive.
    Pieces(tokio::sync::mpsc::UnboundedReceiver<Event>),
}

/// Answers questions. Three implementations: a provider over HTTP, the demo's canned answers,
/// and the app's own answers with no model at all.
pub trait Model: Send + Sync {
    fn ask(&self, request: Request) -> Reply;

    /// Where the answers come from, said quietly under the input.
    fn source(&self) -> Source {
        Source::Scripted
    }

    /// Whether these are the app's own answers, which it works out itself rather than asking
    /// a model.
    fn is_builtin(&self) -> bool {
        false
    }
}

/// The app's own answers, for when no provider is set.
pub struct Builtin;

impl Model for Builtin {
    fn is_builtin(&self) -> bool {
        true
    }

    /// The state answers a built-in question itself, with the figures it has; this is only the
    /// last word if something asks the model directly.
    fn ask(&self, _request: Request) -> Reply {
        Reply::Ready(
            [
                line(&Block::Say {
                    text: "That one needs a model. Add a model in Settings to ask anything; a \
                           local one keeps everything on this computer."
                        .to_string(),
                }),
                line(&Block::Go {
                    steps: vec![step_for(Page::Settings, "Open Settings")],
                }),
            ]
            .join("\n"),
        )
    }
}

/// What the app answers by itself, with no model: the figures the palette recognises, the
/// agenda's items, and what changed since the last look. Everything here is computed.
pub fn builtin(s: &crate::state::State, question: &str) -> Vec<Block> {
    let table = crate::facts::of(s);
    let keywords = keywords(question);
    let mut blocks = Vec::new();

    // A what-if the question names itself. The shock is the question's; what follows from it is
    // the app's own arithmetic, so this needs no model.
    if let Some(shocks) = crate::scenario::asked(
        question,
        crate::view::rebalance::load_classes(s)
            .iter()
            .map(|class| class.name.as_str()),
    ) {
        let list = shocks
            .iter()
            .map(|shock| {
                if shock.percent < 0.0 {
                    format!("{} falls {:.0}%", shock.class, -shock.percent)
                } else {
                    format!("{} rises {:.0}%", shock.class, shock.percent)
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        blocks.push(Block::Say {
            text: format!("If {list}, here is what it would leave."),
        });
        blocks.push(Block::Scenario { shocks });
        blocks.push(Block::Go {
            steps: vec![step_for(Page::Rebalance, "Open Rebalance")],
        });
        return blocks;
    }

    // A figure the palette already answers: the total, today, a class, a holding.
    let figure = keywords
        .iter()
        .find_map(|word| crate::view::answers::figures(s, word).into_iter().next());
    if let Some(answer) = figure
        && let Some(id) = figure_id(&answer.label)
    {
        blocks.push(Block::Say {
            text: format!("{} is {{{id}}}.", answer.label),
        });
        blocks.push(Block::Figures {
            ids: vec![id.to_string()],
        });
        blocks.push(Block::Go {
            steps: vec![step_for(Page::Overview, "Open Overview")],
        });
        return blocks;
    }
    let classes: Vec<_> = keywords
        .iter()
        .find_map(|word| {
            let found = crate::view::answers::classes(s, word);
            (!found.is_empty()).then_some(found)
        })
        .unwrap_or_default();
    if let [class] = classes.as_slice() {
        let id = crate::facts::slug(&class.label);
        let mut text = format!(
            "{} is {{class.{id}.value}}, or {{class.{id}.share}} of the portfolio.",
            class.label
        );
        let mut ids = vec![format!("class.{id}.value"), format!("class.{id}.share")];
        if table.get(&format!("class.{id}.today")).is_some() {
            text.push_str(&format!(" Today it moved {{class.{id}.today}}."));
            ids.push(format!("class.{id}.today"));
        }
        if table.get(&format!("class.{id}.target")).is_some() {
            text.push_str(&format!(" Your target is {{class.{id}.target}}."));
            ids.push(format!("class.{id}.target"));
        }
        blocks.push(Block::Say { text });
        blocks.push(Block::Figures { ids });
        blocks.push(Block::Go {
            steps: vec![step_for(Page::Allocation, "Open Allocation")],
        });
        return blocks;
    }
    if let Some(position) = keywords.iter().find_map(|word| {
        s.positions
            .iter()
            .filter(|position| crate::view::portfolio::matches_search(position, word))
            .min_by_key(|position| position.value.total_cmp(&position.value))
    }) && let Some(id) = s
        .positions
        .iter()
        .find(|each| each.id == position.id)
        .map(crate::facts::holding_id)
    {
        let name = position
            .name
            .clone()
            .or_else(|| position.ticker.clone())
            .unwrap_or_else(|| "It".to_string());
        let mut text = format!("{name} is {{holding.{id}.value}}.");
        let mut ids = vec![format!("holding.{id}.value")];
        if table.get(&format!("holding.{id}.share")).is_some() {
            text.push_str(&format!(
                " That is {{holding.{id}.share}} of the portfolio."
            ));
            ids.push(format!("holding.{id}.share"));
        }
        if table.get(&format!("holding.{id}.today")).is_some() {
            text.push_str(&format!(" Today it moved {{holding.{id}.today}}."));
            ids.push(format!("holding.{id}.today"));
        }
        blocks.push(Block::Say { text });
        blocks.push(Block::Figures { ids });
        blocks.push(Block::Go {
            steps: vec![step_for(Page::Holdings, "Open Holdings")],
        });
        return blocks;
    }

    // What changed since the last look, in the app's own figures. The words carry the direction,
    // so the figure is written without a sign, and a loss is said plainly: no joke on a bad day.
    let asks_change = keywords
        .iter()
        .any(|word| ["changed", "change", "since", "today", "moved"].contains(&word.as_str()));
    if asks_change && s.change.is_some() {
        let total = s
            .change
            .as_ref()
            .map_or(0.0, crate::statement::Change::total);
        let leader = table.get("since.leader").is_some();
        let text = if total > 0.005 {
            if leader {
                "Since {since.date} you are up {since.change}. {since.leader} did most of the work, \
                 which will surprise nobody."
                    .to_string()
            } else {
                "Since {since.date} you are up {since.change}.".to_string()
            }
        } else if total < -0.005 {
            if leader {
                "Since {since.date} you are down {since.change}. {since.leader} accounted for most \
                 of it."
                    .to_string()
            } else {
                "Since {since.date} you are down {since.change}.".to_string()
            }
        } else {
            "Since {since.date} the total is where it was.".to_string()
        };
        blocks.push(Block::Say { text });
        blocks.push(Block::Figures {
            ids: ["since.change", "since.percent", "total", "today.change"]
                .into_iter()
                .filter(|id| table.get(id).is_some())
                .map(str::to_string)
                .collect(),
        });
        blocks.push(Block::Go {
            steps: vec![step_for(Page::Overview, "Open Overview")],
        });
        return blocks;
    }

    // An item on the agenda, if the question is about one.
    if let Some(item) = s
        .agenda()
        .into_iter()
        .max_by_key(|item| mentions(&keywords, &item.title))
        .filter(|item| mentions(&keywords, &item.title) > 0)
    {
        let mut text = item.title.clone();
        if let Some(detail) = &item.detail {
            text.push(' ');
            text.push_str(detail);
        }
        blocks.push(Block::Say { text });
        blocks.push(Block::Go {
            steps: vec![Step {
                label: item.action.label().to_string(),
                page: match item.action {
                    crate::agenda::Action::Open(page) => {
                        Some(page.label().to_lowercase().replace(' ', "-"))
                    }
                    crate::agenda::Action::Holding(_) => Some("holdings".to_string()),
                    crate::agenda::Action::Refresh => Some("data-health".to_string()),
                },
                ask: None,
            }],
        });
        return blocks;
    }

    // Nothing this portfolio can work out on its own.
    blocks.push(Block::Say {
        text: "That one needs a model. Stahl can still tell you what the portfolio holds, how it \
               moved, and what needs a look. Add a model in Settings to ask anything; a local one keeps \
               everything on this computer."
            .to_string(),
    });
    blocks.push(Block::Go {
        steps: vec![step_for(Page::Settings, "Open Settings")],
    });
    blocks
}

/// The words of a question that carry meaning: no "what", "is" or "the".
fn keywords(question: &str) -> Vec<String> {
    const STOP: [&str; 30] = [
        "what", "how", "why", "when", "where", "which", "who", "is", "are", "was", "were", "the",
        "a", "an", "of", "in", "on", "at", "to", "for", "my", "me", "do", "does", "did", "should",
        "could", "would", "about", "there",
    ];
    question
        .split(|character: char| !character.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.chars().count() >= 3 && !STOP.contains(&word.as_str()))
        .collect()
}

/// How many of the question's meaningful words appear in `title`.
fn mentions(keywords: &[String], title: &str) -> usize {
    let title = title.to_lowercase();
    keywords
        .iter()
        .filter(|word| word.chars().count() >= 4 && title.contains(word.as_str()))
        .count()
}

fn step_for(page: Page, label: &str) -> Step {
    Step {
        label: label.to_string(),
        page: Some(page.label().to_lowercase().replace(' ', "-")),
        ask: None,
    }
}

/// The fact id behind a palette figure.
fn figure_id(label: &str) -> Option<&'static str> {
    Some(match label {
        "Total value" => "total",
        "Today's change" => "today.change",
        "Unrealized PnL" => "unrealized.pnl",
        "Cash" => "cash.value",
        "Securities" => "securities.value",
        _ => return None,
    })
}

/// A block as one JSON line.
pub fn line(block: &Block) -> String {
    serde_json::to_string(block).unwrap_or_default()
}

/// How long after a question the first block of a ready answer lands, and how far apart the
/// rest are, on the app's clock.
pub const FIRST_BLOCK: f64 = 0.3;
pub const NEXT_BLOCK: f64 = 0.42;

/// How long after a sentence's last word the next block lands: the answer follows its own lede
/// closely, then keeps the rhythm above.
pub const AFTER_SENTENCE: f64 = 0.18;

/// A figures block shows its figures at zero for this long, then counts them up.
pub const COUNT_HOLD: f64 = 0.12;

/// A what-if sits at today for this long, then plays the change. Long enough to read the total
/// before it moves, short enough that the answer never feels stuck.
pub const PLAY_HOLD: f64 = 0.4;

/// The byte length of the first `count` words of `text`, as a sentence revealed word by word is
/// cut. The whole string when it has fewer.
pub fn upto_words(text: &str, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let mut words = 0;
    let mut in_word = false;
    for (at, character) in text.char_indices() {
        if character.is_whitespace() {
            if in_word {
                words += 1;
                in_word = false;
                if words == count {
                    return at;
                }
            }
        } else {
            in_word = true;
        }
    }
    text.len()
}

/// A figure at zero, for the moment before it counts up. `None` for a figure that isn't a
/// number: a name or a day has nothing to count.
pub fn zero(fact: &crate::facts::Fact, hidden: bool, currency: &str) -> Option<String> {
    use crate::facts::Value;
    let value = match &fact.value {
        Value::Money(_) => Value::Money(0.0),
        Value::SignedMoney(_) => Value::SignedMoney(0.0),
        Value::Percent(_) => Value::Percent(0.0),
        Value::SignedPercent(_) => Value::SignedPercent(0.0),
        Value::Points(_) => Value::Points(0.0),
        Value::Count(_) => Value::Count(0),
        Value::Day { .. } | Value::Name(_) => return None,
    };
    Some(value.render(hidden, currency))
}

/// The `say` a half-arrived line is writing, for text that appears as it streams. `None` when
/// the line isn't a `say`, or hasn't reached its words yet.
pub fn partial_say(line: &str) -> Option<String> {
    let line = line.trim_start();
    if !line.starts_with('{') || !line.contains("\"say\"") {
        return None;
    }
    let at = line.find("\"text\"")?;
    let after = &line[at + "\"text\"".len()..];
    let after = after.trim_start().strip_prefix(':')?.trim_start();
    let body = after.strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = body.chars();
    while let Some(character) = chars.next() {
        match character {
            '\\' => match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => break,
            },
            '"' => return Some(out),
            other => out.push(other),
        }
    }
    Some(out)
}

/// The system prompt for a real model: the voice, the vocabulary, the figures, and the context
/// the app already has. Nothing here is written by the model.
pub fn prompt(s: &crate::state::State, table: &Table) -> String {
    let mut out = String::new();
    out.push_str(
        "You are Counsel, the private banker of Stahl Capital. One client, whose portfolio is \
         below. You write the way a seasoned private banker writes to a client they like and \
         respect: precise, calm, warm, brief. Dry wit is welcome about one line in five, never \
         forced, and never about a loss, a mistake, or anything the client might be worried \
         about. No exclamation marks, no emoji, no slang. Say \"you\" for the client and \"we\" \
         for the bank only when it earns its place. Never mention being a model or an assistant.\n\n",
    );
    out.push_str(
        "You answer with Stahl's own screens, not with prose. Every number about the portfolio \
         must come from the figures listed below: write the figure's id in braces, exactly as \
         shown, and Stahl will draw it. Never write a portfolio figure yourself. A number you \
         write is shown to the client as unverified, which is a small failure.\n\n",
    );
    out.push_str(VOCABULARY);
    out.push_str("\n\nThe figures you may quote:\n");
    out.push_str(&table.describe(s.privacy, &s.currency()));
    if let Some(review) = s.review.data.as_ref() {
        out.push_str("\n\nThe policy review: ");
        out.push_str(&review.policy_name);
        out.push_str(", ");
        for class in &review.allocations {
            out.push_str(&format!(
                "{} {:.1}% against {:.1}% (drift {:+.1}); ",
                class.asset_class, class.actual_percent, class.target_percent, class.drift_percent
            ));
        }
        for finding in &review.findings {
            out.push_str(&format!("finding: {}. ", finding.message));
        }
    }
    if let Some(briefing) = s.briefing.data.as_ref() {
        out.push_str("\n\nThe briefing Stahl wrote on this portfolio:\n");
        out.push_str(&briefing.markdown);
    }
    if s.privacy {
        out.push_str(
            "\n\nThe client has hidden amounts. Do not mention any, and do not ask for them.",
        );
    }
    out
}

// --- A provider ---

/// A provider that speaks the OpenAI chat completions protocol, read as it streams.
pub struct Http {
    /// The base URL as the owner wrote it.
    pub url: String,
    /// For the Authorization header, and nothing else.
    key: Option<String>,
    pub model: String,
    /// Just the host, for the line under the input and the consent question.
    pub host: String,
    runtime: tokio::runtime::Handle,
}

impl Http {
    pub fn new(
        url: impl Into<String>,
        key: Option<String>,
        model: impl Into<String>,
        runtime: tokio::runtime::Handle,
    ) -> Http {
        let url = url.into();
        let host = host_of(&url);
        Http {
            url,
            key,
            model: model.into(),
            host,
            runtime,
        }
    }

    /// The model as the interface names it.
    pub fn display_model(&self) -> String {
        if self.model.trim().is_empty() {
            "the model".to_string()
        } else {
            self.model.clone()
        }
    }

    /// Where the request goes: the base URL, or the full chat completions path if it already
    /// names one.
    pub fn endpoint(&self) -> String {
        let base = self.url.trim().trim_end_matches('/');
        if base.ends_with("/chat/completions") {
            base.to_string()
        } else {
            format!("{base}/chat/completions")
        }
    }

    /// The request body, exactly as it goes over the wire. "Show exactly what is sent" shows
    /// this, so what the owner reads is what leaves the machine.
    pub fn body(request: &Request, model: &str) -> serde_json::Value {
        let mut messages = vec![serde_json::json!({
            "role": "system",
            "content": request.system,
        })];
        for (question, answer) in &request.conversation {
            messages.push(serde_json::json!({ "role": "user", "content": question }));
            messages.push(serde_json::json!({ "role": "assistant", "content": answer }));
        }
        messages.push(serde_json::json!({
            "role": "user",
            "content": request.question,
        }));
        serde_json::json!({
            "model": model,
            "stream": true,
            "temperature": 0.2,
            "messages": messages,
        })
    }

    async fn stream(self, request: Request, out: tokio::sync::mpsc::UnboundedSender<Event>) {
        let body = Http::body(&request, &self.model);
        let mut cancel = request.cancel.map(Box::pin);
        let client = match reqwest::Client::builder().build() {
            Ok(client) => client,
            Err(error) => {
                let _ = out.send(Event::Failed(format!(
                    "Stahl couldn't set up the request: {error}"
                )));
                return;
            }
        };
        let mut call = client
            .post(self.endpoint())
            .json(&body)
            .header("Accept", "text/event-stream");
        if let Some(key) = &self.key {
            call = call.bearer_auth(key);
        }
        let sent = match cancel.as_mut() {
            Some(rx) => tokio::select! {
                _ = rx => return,
                sent = call.send() => sent,
            },
            None => call.send().await,
        };
        let mut response = match sent {
            Ok(response) => response,
            Err(error) => {
                let _ = out.send(Event::Failed(if error.is_connect() {
                    format!(
                        "Stahl couldn't reach {}. Check the address in Settings.",
                        self.host
                    )
                } else {
                    format!("The request to {} failed: {error}", self.host)
                }));
                return;
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let _ = out.send(Event::Failed(match status.as_u16() {
                401 | 403 => "The provider refused the key. Check it in Settings.".to_string(),
                404 => "The provider has no such address or model. Check Settings.".to_string(),
                429 => "The provider is busy. Try again in a moment.".to_string(),
                _ => format!("The provider answered {status}. Try again in a moment."),
            }));
            return;
        }
        let streaming = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("event-stream"));
        if !streaming {
            // A provider that ignored `stream` still answers with the whole reply at once.
            let text = response.text().await.unwrap_or_default();
            match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(json) => match json["choices"][0]["message"]["content"].as_str() {
                    Some(content) => {
                        let _ = out.send(Event::Text(content.to_string()));
                        let _ = out.send(Event::Done);
                    }
                    None => {
                        let _ = out.send(Event::Failed(
                            "The provider sent something Stahl couldn't read. Check the model in \
                             Settings."
                                .to_string(),
                        ));
                    }
                },
                Err(_) => {
                    let _ = out.send(Event::Failed(
                        "The provider sent something Stahl couldn't read. Check the model in \
                         Settings."
                            .to_string(),
                    ));
                }
            }
            return;
        }
        let mut buffer = String::new();
        loop {
            let chunk = match cancel.as_mut() {
                Some(rx) => tokio::select! {
                    _ = rx => return,
                    chunk = response.chunk() => chunk,
                },
                None => response.chunk().await,
            };
            match chunk {
                Ok(Some(bytes)) => {
                    buffer.push_str(&String::from_utf8_lossy(&bytes));
                    if buffer.contains('\r') {
                        buffer = buffer.replace("\r\n", "\n");
                    }
                    let (text, done) = take_sse(&mut buffer);
                    if !text.is_empty() {
                        let _ = out.send(Event::Text(text));
                    }
                    if done {
                        let _ = out.send(Event::Done);
                        return;
                    }
                }
                Ok(None) => {
                    // The last event may have arrived without its blank line.
                    let (text, _) = take_sse(&mut buffer);
                    if !text.is_empty() {
                        let _ = out.send(Event::Text(text));
                    }
                    let _ = out.send(Event::Done);
                    return;
                }
                Err(_) => {
                    let _ = out.send(Event::Failed(
                        "The connection closed before the answer was finished.".to_string(),
                    ));
                    return;
                }
            }
        }
    }
}

impl Model for Http {
    fn ask(&self, request: Request) -> Reply {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let this = Http {
            url: self.url.clone(),
            key: self.key.clone(),
            model: self.model.clone(),
            host: self.host.clone(),
            runtime: self.runtime.clone(),
        };
        self.runtime
            .spawn(async move { this.stream(request, tx).await });
        Reply::Pieces(rx)
    }

    fn source(&self) -> Source {
        Source::Http {
            model: self.display_model(),
            host: self.host.clone(),
        }
    }
}

/// The host a URL names, without a scheme, a user, a port or a path. An IPv6 address keeps its
/// own shape, so `[::1]` reads as `::1`.
pub fn host_of(url: &str) -> String {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split('/').next().unwrap_or(rest);
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    if let Some(after) = authority.strip_prefix('[') {
        return after.split(']').next().unwrap_or(after).to_string();
    }
    authority.split(':').next().unwrap_or(authority).to_string()
}

/// What arrived in a server-sent-events buffer: the text of its `data:` events, and whether the
/// provider said it was done. Whole events only; a half-arrived one waits in the buffer.
fn take_sse(buffer: &mut String) -> (String, bool) {
    let mut text = String::new();
    let mut done = false;
    while let Some(at) = buffer.find("\n\n") {
        let event: String = buffer.drain(..at + 2).collect();
        for line in event.lines() {
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                done = true;
                continue;
            }
            let Ok(json) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            if let Some(piece) = json["choices"][0]["delta"]["content"].as_str() {
                text.push_str(piece);
            }
        }
    }
    (text, done)
}

// --- The conversation ---

/// Where an answer is coming from, said quietly under the input.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    /// The app answered what it can work out itself.
    Builtin,
    /// The demo's canned answers.
    Scripted,
    /// A provider the owner configured. `host` is only ever the host, never the path or a key.
    Http { model: String, host: String },
}

impl Source {
    /// The one line that says where questions go.
    pub fn label(&self) -> String {
        match self {
            Source::Builtin | Source::Scripted => "Answered on this computer".to_string(),
            Source::Http { model, host } => {
                if is_local_host(host) {
                    format!("Answers from {model}, on this computer")
                } else {
                    format!("Answers from {model} at {host}")
                }
            }
        }
    }
}

/// A host that never leaves the machine.
pub fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// How far along a turn is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Sent, and nothing has come back yet.
    Waiting,
    /// Blocks are arriving.
    Streaming,
    Done,
    /// The owner stopped it.
    Stopped,
    Failed,
}

/// One question and the answer it got.
#[derive(Clone, Debug, PartialEq)]
pub struct Turn {
    pub question: String,
    pub blocks: Vec<Block>,
    /// When each block landed on the app's clock, at the same index as [`Turn::blocks`]. A block
    /// with no time here was on the page from the start (a scripted scene), and is drawn settled.
    pub landed: Vec<f64>,
    pub phase: Phase,
    /// What went wrong, in plain words.
    pub error: Option<String>,
}

impl Turn {
    pub fn new(question: impl Into<String>) -> Turn {
        Turn {
            question: question.into(),
            blocks: Vec::new(),
            landed: Vec::new(),
            phase: Phase::Waiting,
            error: None,
        }
    }

    /// Add a block that has just landed.
    pub fn land(&mut self, block: Block, now: f64) {
        self.blocks.push(block);
        self.landed.push(now);
    }

    /// When block `at` landed, if the page has a time for it.
    pub fn landed_at(&self, at: usize) -> Option<f64> {
        self.landed.get(at).copied()
    }

    /// How long ago block `at` landed, on the app's clock. `None` when it was already there.
    pub fn age(&self, at: usize, now: f64) -> Option<f64> {
        self.landed_at(at).map(|landed| (now - landed).max(0.0))
    }

    /// Whether this is the turn's first sentence, the one that arrives word by word.
    pub fn first_say(&self, block: &Block) -> bool {
        matches!(block, Block::Say { .. })
            && !self.blocks.iter().any(|b| matches!(b, Block::Say { .. }))
    }

    pub fn finished(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Stopped | Phase::Failed)
    }
}

/// The first sentence of an answer, arriving word by word rather than as a finished paragraph.
#[derive(Clone, Debug, PartialEq)]
pub struct Reveal {
    /// The turn and the block within it that the sentence belongs to.
    pub turn: usize,
    pub block: usize,
    /// How many words of it are on the page, and how many it has in all.
    pub words: usize,
    pub total: usize,
    /// When it started, on the app's clock.
    pub started: f64,
}

impl Reveal {
    /// About how long a whole sentence takes to arrive, however long it is.
    pub const FOR: f64 = 0.5;
    /// The shortest and longest a single word may take, so a very long sentence still reads as
    /// words and a very short one is not a flicker.
    const STEP: std::ops::RangeInclusive<f64> = 0.012..=0.05;

    /// How long one word of this sentence takes.
    pub fn step(&self) -> f64 {
        (Self::FOR / self.total.max(1) as f64).clamp(*Self::STEP.start(), *Self::STEP.end())
    }

    /// When the next word is due.
    pub fn next_at(&self) -> f64 {
        self.started + self.words as f64 * self.step()
    }

    /// When the last word is due.
    pub fn ends_at(&self) -> f64 {
        self.started + self.total as f64 * self.step()
    }

    pub fn done(&self) -> bool {
        self.words >= self.total
    }
}

/// A what-if block, as the page is playing it.
#[derive(Clone, Debug, PartialEq)]
pub struct WhatIf {
    pub turn: usize,
    pub block: usize,
    /// The owner is holding it on today, rather than on what the shock would leave.
    pub now: bool,
    /// The table behind "Details" is open.
    pub details: bool,
}

/// A plan block the owner is playing with.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanPlay {
    pub turn: usize,
    pub block: usize,
    /// The fraction the owner dragged the slider to, if they have.
    pub fraction: Option<f64>,
    /// The slider's width in points, measured once it is on the page.
    pub width: f32,
    /// A pointer is on the slider.
    pub dragging: bool,
}

/// The session's conversation. Nothing here is written to disk.
pub struct Conversation {
    pub turns: Vec<Turn>,
    pub input: TextEdit,
    /// Blocks on their way, with the time each is due on the app's clock.
    pub queued: VecDeque<(f64, Block)>,
    /// A question is out and its answer hasn't finished.
    pub thinking: bool,
    pub streaming: bool,
    pub source: Source,
    /// The answer being written, for a model that streams text before its line ends.
    pub partial: Option<String>,
    /// Set to stop the request in flight: dropping this drops the provider's connection.
    pub cancel: Option<tokio::sync::oneshot::Sender<()>>,
    /// Which answer is current: a stopped or replaced one must not land in a newer turn.
    pub generation: u64,
    /// A chart for a period the page isn't showing, loaded for a `chart` block.
    pub chart: Option<(crate::backend::Period, crate::backend::History)>,
    /// The answer being read, line by line, from a provider.
    pub lines: Lines,
    /// The sentence being revealed word by word, if one is.
    pub reveal: Option<Reveal>,
    /// The what-if blocks on the page, and where the owner has put them.
    pub what_if: Vec<WhatIf>,
    /// The plan blocks the owner has played with.
    pub plan: Vec<PlanPlay>,
}

impl Default for Conversation {
    fn default() -> Self {
        Self::new()
    }
}

impl Conversation {
    pub fn new() -> Conversation {
        let mut input = TextEdit::new("");
        input.focused = true;
        Conversation {
            turns: Vec::new(),
            input,
            queued: VecDeque::new(),
            thinking: false,
            streaming: false,
            source: Source::Builtin,
            partial: None,
            cancel: None,
            generation: 0,
            chart: None,
            lines: Lines::new(),
            reveal: None,
            what_if: Vec::new(),
            plan: Vec::new(),
        }
    }

    /// A what-if block, if the page has played with it.
    pub fn what_if_at(&self, turn: usize, block: usize) -> Option<&WhatIf> {
        self.what_if
            .iter()
            .find(|what_if| what_if.turn == turn && what_if.block == block)
    }

    fn what_if_mut(&mut self, turn: usize, block: usize) -> &mut WhatIf {
        if let Some(index) = self
            .what_if
            .iter()
            .position(|what_if| what_if.turn == turn && what_if.block == block)
        {
            return &mut self.what_if[index];
        }
        self.what_if.push(WhatIf {
            turn,
            block,
            now: false,
            details: false,
        });
        self.what_if.last_mut().expect("just pushed")
    }

    /// Hold a what-if on today, or let it show what the shock leaves.
    pub fn what_if_show(&mut self, turn: usize, block: usize, now: bool) {
        self.what_if_mut(turn, block).now = now;
    }

    /// Open or close the table behind a what-if's "Details".
    pub fn what_if_details(&mut self, turn: usize, block: usize) {
        let what_if = self.what_if_mut(turn, block);
        what_if.details = !what_if.details;
    }

    /// A plan block, if the owner has played with it.
    pub fn plan_at(&self, turn: usize, block: usize) -> Option<&PlanPlay> {
        self.plan
            .iter()
            .find(|play| play.turn == turn && play.block == block)
    }

    fn plan_mut(&mut self, turn: usize, block: usize) -> &mut PlanPlay {
        if let Some(index) = self
            .plan
            .iter()
            .position(|play| play.turn == turn && play.block == block)
        {
            return &mut self.plan[index];
        }
        self.plan.push(PlanPlay {
            turn,
            block,
            fraction: None,
            width: 0.0,
            dragging: false,
        });
        self.plan.last_mut().expect("just pushed")
    }

    /// The fraction the owner dragged a plan's slider to.
    pub fn plan_set(&mut self, turn: usize, block: usize, fraction: f64) {
        self.plan_mut(turn, block).fraction = Some(fraction.clamp(0.0, 1.0));
    }

    /// A pointer took the slider, or let it go.
    pub fn plan_drag(&mut self, turn: usize, block: usize, dragging: bool) {
        self.plan_mut(turn, block).dragging = dragging;
    }

    /// The slider's width, measured once it is on the page.
    pub fn plan_measure(&mut self, turn: usize, block: usize, width: f32) {
        let play = self.plan_mut(turn, block);
        if (play.width - width).abs() > 0.5 {
            play.width = width;
        }
    }

    /// The question being answered, if one is.
    pub fn current(&self) -> Option<&Turn> {
        self.turns.last().filter(|turn| !turn.finished())
    }

    pub fn current_mut(&mut self) -> Option<&mut Turn> {
        self.turns.last_mut().filter(|turn| !turn.finished())
    }

    /// Everything drawn: each question, and its answer's blocks.
    pub fn len(&self) -> usize {
        self.turns.len()
    }

    pub fn is_empty(&self) -> bool {
        self.turns.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::facts::{Fact, Value};

    fn table() -> Table {
        let mut table = Table::default();
        for (id, value) in [
            ("total", Value::Money(107_400.0)),
            ("today.change", Value::SignedMoney(280.0)),
            ("class.stocks.share", Value::Percent(37.2)),
            ("class.crypto.drift", Value::Points(15.1)),
        ] {
            table.insert(Fact {
                id: id.to_string(),
                label: id.to_string(),
                short: id.to_string(),
                value,
                class: None,
            });
        }
        table
    }

    #[test]
    fn a_line_becomes_the_block_it_names() {
        let block = parse_line(r#"{"block":"say","text":"You are up {today.change}."}"#).unwrap();
        assert_eq!(
            block,
            Block::Say {
                text: "You are up {today.change}.".to_string()
            }
        );
        assert_eq!(
            parse_line(r#"{"block":"allocation"}"#).unwrap(),
            Block::Allocation
        );
        assert_eq!(
            parse_line(r#"{"block":"figures","ids":["total"]}"#).unwrap(),
            Block::Figures {
                ids: vec!["total".to_string()]
            }
        );
        assert_eq!(
            parse_line(r#"{"block":"plan","fraction":0.5}"#).unwrap(),
            Block::Plan {
                fraction: Some(0.5),
                amount: None
            }
        );
        assert_eq!(
            parse_line(r#"{"block":"chart","period":"week"}"#).unwrap(),
            Block::Chart {
                period: Some("week".to_string()),
                span: None
            }
        );
        assert_eq!(
            parse_line(r#"{"block":"scenario","shocks":[{"class":"Crypto","percent":-50}]}"#)
                .unwrap(),
            Block::Scenario {
                shocks: vec![scenario::Shock::new("Crypto", -50.0)]
            }
        );
        assert!(parse_line("not json at all").is_err());
        assert!(
            parse_line(r#"{"block":"dance"}"#).is_err(),
            "an unknown block is not guessed at"
        );
    }

    #[test]
    fn a_stream_completes_lines_and_keeps_the_rest() {
        let mut lines = Lines::new();
        assert!(lines.push(r#"{"block":"say","text":"one"}"#).is_empty());
        let blocks = lines.push("\n{\"block\":\"say\",\"text\":\"two\"}\n{\"block\":\"nope\"}\n");
        assert_eq!(blocks.len(), 2, "both good lines, not the malformed one");
        assert_eq!(blocks[0].text(), "one", "the line that was waiting");
        assert_eq!(blocks[1].text(), "two");
        assert_eq!(lines.skipped.len(), 1);
        // The last line without a newline still counts when the stream ends.
        lines.push(r#"{"block":"say","text":"three"}"#);
        assert_eq!(lines.finish()[0].text(), "three");
        assert!(lines.finish().is_empty());
    }

    #[test]
    fn a_placeholder_becomes_the_figure_it_names() {
        let table = table();
        assert_eq!(
            pieces("You are up {today.change} today.", &table),
            vec![
                Piece::Text("You are up ".to_string()),
                Piece::Chip("today.change".to_string()),
                Piece::Text(" today.".to_string()),
            ]
        );
        assert_eq!(
            pieces("Stocks are { class.stocks.share } of it.", &table)[1],
            Piece::Chip("class.stocks.share".to_string()),
            "spaces inside the braces are fine"
        );
    }

    #[test]
    fn an_unknown_placeholder_is_a_gap_not_a_guess() {
        let table = table();
        assert_eq!(
            pieces("Your {class.bonds.share} is off.", &table),
            vec![
                Piece::Text("Your ".to_string()),
                Piece::Gap("class.bonds.share".to_string()),
                Piece::Text(" is off.".to_string()),
            ]
        );
        // Braces that don't hold an id are just words.
        assert_eq!(
            pieces("Use {curly braces} here.", &table),
            vec![Piece::Text("Use {curly braces} here.".to_string())]
        );
        assert_eq!(
            pieces("An {unclosed brace", &table),
            vec![Piece::Text("An {unclosed brace".to_string())]
        );
    }

    #[test]
    fn digits_the_model_wrote_are_marked_unverified() {
        let table = table();
        assert_eq!(
            pieces("The total is €107,400.00.", &table),
            vec![
                Piece::Text("The total is ".to_string()),
                Piece::Unverified("€107,400.00".to_string()),
                Piece::Text(".".to_string()),
            ]
        );
        assert_eq!(
            pieces("It rose 3.2% in 2 days.", &table),
            vec![
                Piece::Text("It rose ".to_string()),
                Piece::Unverified("3.2%".to_string()),
                Piece::Text(" in ".to_string()),
                Piece::Unverified("2".to_string()),
                Piece::Text(" days.".to_string()),
            ]
        );
        assert_eq!(
            pieces("Down -5% since Friday.", &table)[1],
            Piece::Unverified("-5%".to_string()),
            "the sign belongs to the figure"
        );
        assert_eq!(
            pieces("Two things, no digits.", &table),
            vec![Piece::Text("Two things, no digits.".to_string())]
        );
    }

    #[test]
    fn a_figure_the_app_computed_is_never_unverified() {
        let table = table();
        let words = "You are up {today.change} ({today.change}) today, 3.2% of {total}.";
        let pieces = pieces(words, &table);
        let chips = pieces
            .iter()
            .filter(|p| matches!(p, Piece::Chip(_)))
            .count();
        let digits = pieces
            .iter()
            .filter(|p| matches!(p, Piece::Unverified(_)))
            .count();
        assert_eq!(chips, 3);
        assert_eq!(digits, 1, "only the 3.2% the model wrote");
    }

    #[test]
    fn the_spoken_answer_shows_the_figures_and_hides_them_in_privacy_mode() {
        let table = table();
        assert_eq!(
            spoken("You are up {today.change}.", &table, false, "EUR"),
            "You are up €280.00.",
            "the words carry the direction, so the figure does not repeat it"
        );
        assert_eq!(
            spoken("It moved by {today.change}.", &table, false, "EUR"),
            "It moved by +€280.00.",
            "with no direction in the words, the figure keeps its sign"
        );
        assert_eq!(
            spoken("You are up {today.change}.", &table, true, "EUR"),
            "You are up ••••••."
        );
        assert_eq!(spoken("Gap {nope.here}.", &table, false, "EUR"), "Gap —.");
    }

    #[test]
    fn a_loss_after_the_word_down_is_not_written_with_a_second_sign() {
        let mut table = table();
        table.insert(Fact {
            id: "since.change".to_string(),
            label: "the change".to_string(),
            short: "Since".to_string(),
            value: Value::SignedMoney(-291.41),
            class: None,
        });
        assert_eq!(
            spoken(
                "Since Friday you are down {since.change}.",
                &table,
                false,
                "EUR"
            ),
            "Since Friday you are down €291.41.",
            "never \"down -€291.41\""
        );
        assert_eq!(
            spoken("You are up {since.change}.", &table, false, "EUR"),
            "You are up €291.41."
        );
    }

    #[test]
    fn a_half_written_say_shows_the_words_it_has() {
        assert_eq!(
            partial_say(r#"{"block":"say","text":"Since Novem"#),
            Some("Since Novem".to_string())
        );
        assert_eq!(
            partial_say(r#"{"block":"say","text":"Done.\nNext"}"#),
            Some("Done.\nNext".to_string())
        );
        assert_eq!(
            partial_say(r#"{"block":"figures","ids":["#),
            None,
            "only a say has words to show"
        );
        assert_eq!(partial_say("{\"block\":\"say\""), None);
    }

    #[test]
    fn a_sentence_is_cut_after_the_words_that_have_arrived() {
        let sentence = "Since Saturday you are up.";
        assert_eq!(upto_words(sentence, 0), 0);
        assert_eq!(&sentence[..upto_words(sentence, 1)], "Since");
        assert_eq!(&sentence[..upto_words(sentence, 2)], "Since Saturday");
        assert_eq!(
            upto_words(sentence, 99),
            sentence.len(),
            "more words than there are is the whole sentence"
        );
        assert_eq!(upto_words("", 3), 0);
        // A figure with a space in it counts as two words, as the page's text does.
        let figures = "Up €291.41 or 12.8 points.";
        assert_eq!(&figures[..upto_words(figures, 2)], "Up €291.41");
    }

    #[test]
    fn a_reveal_gives_a_sentence_about_half_a_second_whatever_its_length() {
        let reveal = |total| Reveal {
            turn: 0,
            block: 0,
            words: 1,
            total,
            started: 1.0,
        };
        // Ten words, half a second: 50 ms each, the slowest a word may take.
        assert!((reveal(10).step() - 0.05).abs() < 1e-9);
        assert!(reveal(20).ends_at() - 1.0 <= 0.55);
        assert!(
            reveal(200).step() >= 0.012,
            "a long sentence still reads as words"
        );
        assert!(!reveal(20).done());
        assert!(
            Reveal {
                words: 20,
                ..reveal(20)
            }
            .done()
        );
    }

    #[test]
    fn a_figure_counts_up_from_zero_only_where_there_is_a_number() {
        let table = table();
        assert_eq!(
            zero(table.get("total").unwrap(), false, "EUR").unwrap(),
            "€0.00"
        );
        assert_eq!(
            zero(table.get("today.change").unwrap(), false, "EUR").unwrap(),
            "€0.00"
        );
        assert_eq!(
            zero(table.get("class.stocks.share").unwrap(), false, "EUR").unwrap(),
            "0.0%"
        );
        let name = Fact {
            id: "policy.name".to_string(),
            label: String::new(),
            short: String::new(),
            value: Value::Name("Balanced Growth".to_string()),
            class: None,
        };
        assert_eq!(
            zero(&name, false, "EUR"),
            None,
            "a name has nothing to count"
        );
        // Privacy hides the zero as it hides any amount.
        assert_eq!(
            zero(table.get("total").unwrap(), true, "EUR").unwrap(),
            crate::format::HIDDEN
        );
    }

    #[test]
    fn the_prompt_lists_the_block_names_the_parser_reads() {
        for name in [
            "say",
            "figures",
            "chart",
            "holdings",
            "allocation",
            "plan",
            "scenario",
            "go",
        ] {
            assert!(
                VOCABULARY.contains(&format!("\"block\":\"{name}\"")),
                "{name} is missing from the vocabulary"
            );
            let line = format!("{{\"block\":\"{name}\"}}");
            // The prompt's example for each may carry fields; the bare name must still parse for
            // the ones that need nothing else.
            if matches!(name, "allocation") {
                assert!(parse_line(&line).is_ok());
            }
        }
    }
}

#[cfg(test)]
mod provider_tests {
    use std::time::Duration;

    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpListener,
        task::JoinHandle,
    };

    use super::*;

    /// A provider on this machine: it answers one request with the given body and content type,
    /// and hands back what it read.
    async fn fake_provider(
        body: &'static str,
        content_type: &'static str,
    ) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let seen = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0u8; 4096];
            // Headers, then as much of the body as the length says.
            loop {
                let read = socket.read(&mut buffer).await.unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&request);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|value| value.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });
        (format!("http://{address}/v1"), seen)
    }

    fn request() -> Request {
        Request {
            question: "What changed?".to_string(),
            conversation: Vec::new(),
            system: "You are Counsel.".to_string(),
            facts: Table::default(),
            cancel: None,
        }
    }

    fn collect(runtime: &tokio::runtime::Runtime, reply: Reply) -> Vec<Event> {
        let Reply::Pieces(mut pieces) = reply else {
            panic!("a provider streams its answer");
        };
        runtime.block_on(async {
            let mut events = Vec::new();
            while let Some(event) = tokio::time::timeout(Duration::from_secs(5), pieces.recv())
                .await
                .expect("the provider answers")
            {
                events.push(event);
            }
            events
        })
    }

    #[test]
    fn a_provider_streams_its_lines_and_gets_the_key_it_was_given() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"{\\\"block\\\":\\\"say\\\",\\\"text\\\":\\\"One.\\\"}\\n\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"content\":\"{\\\"block\\\":\\\"go\\\",\\\"steps\\\":[]}\\n\"}}]}\n\ndata: [DONE]\n\n";
        let (url, seen) = runtime.block_on(fake_provider(sse, "text/event-stream"));
        let http = Http::new(
            url,
            Some("sk-test".to_string()),
            "a-model",
            runtime.handle().clone(),
        );
        assert_eq!(http.endpoint().split('/').next_back(), Some("completions"));
        let events = collect(&runtime, http.ask(request()));
        assert_eq!(
            events,
            vec![
                Event::Text(
                    "{\"block\":\"say\",\"text\":\"One.\"}\n{\"block\":\"go\",\"steps\":[]}\n"
                        .to_string()
                ),
                Event::Done,
            ]
        );
        let seen = runtime.block_on(seen).unwrap();
        assert!(seen.contains("authorization: Bearer sk-test"), "{seen}");
        assert!(seen.contains("\"stream\":true"));
        assert!(seen.contains("What changed?"));
        assert!(seen.contains("You are Counsel."));
    }

    #[test]
    fn a_provider_that_ignores_stream_still_answers() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let json = r#"{"choices":[{"message":{"content":"{\"block\":\"say\",\"text\":\"All at once.\"}"}}]}"#;
        let (url, _) = runtime.block_on(fake_provider(json, "application/json"));
        let http = Http::new(url, None, "a-model", runtime.handle().clone());
        let events = collect(&runtime, http.ask(request()));
        assert_eq!(
            events,
            vec![
                Event::Text("{\"block\":\"say\",\"text\":\"All at once.\"}".to_string()),
                Event::Done,
            ]
        );
    }

    #[test]
    fn stopping_drops_the_request() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let listener = runtime.block_on(TcpListener::bind("127.0.0.1:0")).unwrap();
        let address = listener.local_addr().unwrap();
        // A provider that accepts and then never answers.
        let held = runtime.spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(30)).await;
        });
        let http = Http::new(
            format!("http://{address}/v1"),
            None,
            "a-model",
            runtime.handle().clone(),
        );
        let (cancel, receiver) = tokio::sync::oneshot::channel();
        let mut request = request();
        request.cancel = Some(receiver);
        let Reply::Pieces(mut pieces) = http.ask(request) else {
            panic!("a provider streams");
        };
        runtime.block_on(async {
            // Give the request time to reach the provider, then stop it.
            tokio::time::sleep(Duration::from_millis(80)).await;
            cancel.send(()).unwrap();
            let next = tokio::time::timeout(Duration::from_secs(2), pieces.recv())
                .await
                .expect("the stopped request ends");
            assert!(next.is_none(), "a stopped request says nothing more");
        });
        held.abort();
    }

    #[test]
    fn the_host_is_read_without_a_scheme_port_or_path() {
        assert_eq!(host_of("https://api.openai.com/v1"), "api.openai.com");
        assert_eq!(host_of("http://127.0.0.1:8080/v1"), "127.0.0.1");
        assert_eq!(host_of("http://[::1]:8080"), "::1");
        assert_eq!(host_of("localhost:11434/v1"), "localhost");
        assert_eq!(host_of("https://user:pass@host.example/v1"), "host.example");
        assert!(is_local_host(&host_of("http://[::1]:11434/v1")));
    }

    #[test]
    fn the_body_carries_the_conversation_and_never_a_figure() {
        let mut request = request();
        request.conversation = vec![("Earlier?".to_string(), "An answer.".to_string())];
        let body = Http::body(&request, "a-model");
        assert_eq!(body["model"], "a-model");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "Earlier?");
        assert_eq!(body["messages"][2]["content"], "An answer.");
        assert_eq!(body["messages"][3]["content"], "What changed?");
    }
}

#[cfg(test)]
mod builtin_tests {
    use super::*;
    use crate::{demo, state::State};

    fn state() -> (tokio::runtime::Runtime, tempfile::TempDir, State) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("portfolio");
        demo::seed_workspace(&workspace);
        let mut state = State::new(
            demo::backend(dir.path(), Some(&workspace), runtime.handle().clone()),
            None,
        );
        state.startup();
        state.settle();
        (runtime, dir, state)
    }

    fn words(blocks: &[Block]) -> String {
        blocks
            .iter()
            .filter_map(|block| match block {
                Block::Say { text } => Some(text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn a_figure_the_palette_knows_is_answered_with_its_own_id() {
        let (_runtime, _dir, state) = state();
        let blocks = builtin(&state, "what is the total?");
        assert!(words(&blocks).contains("{total}"));
        assert!(matches!(
            blocks.get(1),
            Some(Block::Figures { ids }) if ids == &["total".to_string()]
        ));
        let blocks = builtin(&state, "how is today going?");
        assert!(words(&blocks).contains("{today.change}"));
    }

    #[test]
    fn a_class_and_a_holding_are_answered_in_their_own_figures() {
        let (_runtime, _dir, state) = state();
        let blocks = builtin(&state, "how much is in crypto?");
        let text = words(&blocks);
        assert!(text.contains("{class.crypto.value}"), "{text}");
        assert!(text.contains("{class.crypto.share}"), "{text}");
        assert!(text.contains("{class.crypto.target}"), "{text}");

        let blocks = builtin(&state, "how is bitcoin doing?");
        let text = words(&blocks);
        assert!(text.contains("{holding.pos_btc.value}"), "{text}");
        assert!(text.contains("{holding.pos_btc.today}"), "{text}");
    }

    #[test]
    fn what_changed_is_answered_from_the_statement_s_own_figures() {
        let (_runtime, _dir, state) = state();
        let blocks = builtin(&state, "what changed since Friday?");
        let text = words(&blocks);
        assert!(text.contains("{since.date}"), "{text}");
        assert!(text.contains("{since.change}"), "{text}");
        assert!(matches!(
            blocks.get(1),
            Some(Block::Figures { ids }) if ids.iter().any(|id| id == "since.change")
        ));
    }

    #[test]
    fn an_agenda_item_answers_with_its_own_action() {
        let (_runtime, _dir, state) = state();
        let blocks = builtin(&state, "what about the tolerance?");
        let text = words(&blocks);
        assert!(text.contains("outside their tolerance"), "{text}");
        assert!(matches!(
            blocks.get(1),
            Some(Block::Go { steps }) if steps[0].label == "Rebalance"
        ));
    }

    #[test]
    fn anything_else_asks_for_a_model_and_offers_settings() {
        let (_runtime, _dir, state) = state();
        let blocks = builtin(&state, "should I buy a house?");
        let text = words(&blocks);
        assert!(text.contains("needs a model"), "{text}");
        assert!(
            text.contains("local one keeps everything on this computer"),
            "{text}"
        );
        assert!(matches!(
            blocks.get(1),
            Some(Block::Go { steps }) if steps[0].page.as_deref() == Some("settings")
        ));
    }

    #[test]
    fn every_placeholder_a_built_in_answer_writes_exists_in_the_table() {
        let (_runtime, _dir, state) = state();
        let table = crate::facts::of(&state);
        for question in [
            "what is the total?",
            "how much is in crypto?",
            "how is bitcoin doing?",
            "what changed since Friday?",
            "what about the tolerance?",
        ] {
            for id in builtin(&state, question)
                .iter()
                .filter_map(|block| match block {
                    Block::Say { text } => Some(text),
                    _ => None,
                })
                .flat_map(|text| named(text))
            {
                assert!(table.get(&id).is_some(), "{question}: {id} is not a fact");
            }
        }
    }
}
