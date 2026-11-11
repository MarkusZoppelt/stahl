//! Ask: a question answered with the app's own interface.
//!
//! An answer is a list of [`Block`]s, one JSON object per line. The model writes the words and
//! names the figures; the app draws the figures from its [`crate::facts`] table, so a number about
//! the portfolio can only ever be one Stahl computed. Anything else the model writes with digits
//! is shown as unverified.

use std::{collections::VecDeque, mem, sync::Arc, sync::atomic::AtomicBool};

use rune::TextEdit;
use serde::{Deserialize, Serialize};

use crate::{facts::Table, scenario};
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
    pieces(text, table)
        .into_iter()
        .map(|piece| match piece {
            Piece::Text(words) => words,
            Piece::Chip(id) => table.get(&id).map_or_else(
                || "—".to_string(),
                |fact| fact.value.render(hidden, currency),
            ),
            Piece::Gap(_) => "—".to_string(),
            Piece::Unverified(digits) => digits,
        })
        .collect()
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
#[derive(Clone, Debug)]
pub struct Request {
    pub question: String,
    /// Earlier questions and the plain text of their answers, newest last.
    pub conversation: Vec<(String, String)>,
    /// The system prompt: the voice, the block vocabulary, the fact ids and the portfolio.
    pub system: String,
    /// Every figure the answer may quote, by id.
    pub facts: Table,
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
}

/// The app's own answers, for when no provider is set.
pub struct Builtin;

impl Model for Builtin {
    fn ask(&self, request: Request) -> Reply {
        Reply::Ready(builtin(&request))
    }
}

/// What the built-in model can answer: the questions the palette already recognises, and the
/// agenda's items. Everything else gets one calm line and the way to Settings.
pub fn builtin(_request: &Request) -> String {
    let say = Block::Say {
        text: "That one needs a model. This portfolio can answer what changed, where new money \
               goes, and what a shock would leave. Add a model in Settings to ask anything; a \
               local one keeps everything on this computer."
            .to_string(),
    };
    let go = Block::Go {
        steps: vec![Step {
            label: "Open Settings".to_string(),
            page: Some("settings".to_string()),
            ask: None,
        }],
    };
    [line(&say), line(&go)].join("\n")
}

/// A block as one JSON line.
pub fn line(block: &Block) -> String {
    serde_json::to_string(block).unwrap_or_default()
}

/// How long after a question the first block of a ready answer lands, and how far apart the
/// rest are, on the app's clock.
pub const FIRST_BLOCK: f64 = 0.3;
pub const NEXT_BLOCK: f64 = 0.42;

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
            Source::Builtin => "Answered on this computer".to_string(),
            Source::Scripted => "Answered on this computer".to_string(),
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
    pub phase: Phase,
    /// What went wrong, in plain words.
    pub error: Option<String>,
}

impl Turn {
    pub fn new(question: impl Into<String>) -> Turn {
        Turn {
            question: question.into(),
            blocks: Vec::new(),
            phase: Phase::Waiting,
            error: None,
        }
    }

    pub fn finished(&self) -> bool {
        matches!(self.phase, Phase::Done | Phase::Stopped | Phase::Failed)
    }
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
    /// Set to stop the request in flight.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Which answer is current: a stopped or replaced one must not land in a newer turn.
    pub generation: u64,
    /// A chart for a period the page isn't showing, loaded for a `chart` block.
    pub chart: Option<(crate::backend::Period, crate::backend::History)>,
    /// The answer being read, line by line, from a provider.
    pub lines: Lines,
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
            "You are up +€280.00."
        );
        assert_eq!(
            spoken("You are up {today.change}.", &table, true, "EUR"),
            "You are up ••••••."
        );
        assert_eq!(spoken("Gap {nope.here}.", &table, false, "EUR"), "Gap —.");
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
