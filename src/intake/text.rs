//! The file as text: decoded, split into cells that remember exactly where they sit, and the
//! numbers and dates in it read the one way they can be read.
//!
//! Nothing here guesses. A number whose decimal mark could go either way, or a date that could
//! be day-first or month-first, comes back as a doubt, never as a value.

use chrono::NaiveDate;

/// The most a broker's export plausibly weighs. Anything larger is refused before it is read.
pub const MAX_BYTES: usize = 8 * 1024 * 1024;

/// Where something sits in the file: its line (from zero) and the bytes of that line it spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

/// One cell of a record: its text without quotes or padding, and where that text sits.
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub text: String,
    pub at: Spot,
}

/// One record of a delimited file, on the line it starts on.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    pub line: usize,
    pub cells: Vec<Cell>,
}

impl Record {
    /// The cell at `index`, if the record has one and it isn't blank.
    pub fn cell(&self, index: usize) -> Option<&Cell> {
        self.cells.get(index).filter(|cell| !cell.text.is_empty())
    }

    /// How many cells hold something.
    pub fn filled(&self) -> usize {
        self.cells
            .iter()
            .filter(|cell| !cell.text.is_empty())
            .count()
    }
}

/// A file as it was read: its lines as decoded, and its records under the delimiter it uses.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    pub lines: Vec<String>,
    pub delimiter: char,
    pub records: Vec<Record>,
}

/// Why a file's bytes are not text this app can read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotText {
    Empty,
    TooLarge(usize),
    Binary,
}

/// The bytes as text. UTF-8 (with or without its mark), UTF-16 with its mark, and otherwise
/// Windows-1252, which is what a German bank's "CSV for Excel" usually is. Each of those decodes
/// exactly; anything with stray control bytes is not text at all.
pub fn decode(bytes: &[u8]) -> Result<String, NotText> {
    if bytes.len() > MAX_BYTES {
        return Err(NotText::TooLarge(bytes.len()));
    }
    let text = if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8(rest.to_vec()).map_err(|_| NotText::Binary)?
    } else if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        utf16(rest, u16::from_le_bytes)?
    } else if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        utf16(rest, u16::from_be_bytes)?
    } else {
        match String::from_utf8(bytes.to_vec()) {
            Ok(text) => text,
            Err(_) => bytes.iter().map(|byte| windows_1252(*byte)).collect(),
        }
    };
    let binary = text
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t' | '\u{c}'));
    if binary {
        return Err(NotText::Binary);
    }
    if text.trim().is_empty() {
        return Err(NotText::Empty);
    }
    Ok(text)
}

fn utf16(bytes: &[u8], word: fn([u8; 2]) -> u16) -> Result<String, NotText> {
    if !bytes.len().is_multiple_of(2) {
        return Err(NotText::Binary);
    }
    let words: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| word(*pair))
        .collect();
    String::from_utf16(&words).map_err(|_| NotText::Binary)
}

/// Windows-1252: Latin-1 with printable characters where Latin-1 has C1 controls. The five
/// bytes it leaves undefined stay controls, so a file using them reads as binary.
fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9f => HIGH[(byte - 0x80) as usize],
        _ => byte as char,
    }
}

/// The lines of `text`, without their endings.
pub fn lines(text: &str) -> Vec<String> {
    text.split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect()
}

/// The delimiters exports use, in the order a tie is settled: a semicolon never appears inside
/// a German number, and a comma often does.
const DELIMITERS: [char; 4] = [';', '\t', ',', '|'];

/// Split `text` into records at `delimiter`, honouring double quotes (a doubled quote inside
/// is one quote). A quoted cell may run over a line break; it then belongs to the line it
/// started on, and its spot covers only that line.
pub fn split(lines: &[String], delimiter: char) -> Vec<Record> {
    let mut records = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let start_line = index;
        let mut cells = Vec::new();
        let mut text = String::new();
        let mut quoted = false;
        let mut was_quoted = false;
        let mut cell_start: Option<usize> = None;
        let mut cell_end = 0;
        let mut line = &lines[index];
        let mut at = 0;
        loop {
            let rest = &line[at..];
            let Some(c) = rest.chars().next() else {
                if quoted && index + 1 < lines.len() {
                    // The quote carries on onto the next line.
                    text.push('\n');
                    index += 1;
                    line = &lines[index];
                    at = 0;
                    continue;
                }
                break;
            };
            let width = c.len_utf8();
            if quoted {
                if c == '"' {
                    if rest[width..].starts_with('"') {
                        text.push('"');
                        at += 2 * width;
                        if index == start_line {
                            cell_end = at;
                        }
                        continue;
                    }
                    quoted = false;
                    at += width;
                    continue;
                }
                text.push(c);
                at += width;
                if index == start_line {
                    cell_end = at;
                }
                continue;
            }
            if c == delimiter {
                cells.push(finish(&text, cell_start, cell_end, start_line, was_quoted));
                text.clear();
                cell_start = None;
                was_quoted = false;
                at += width;
                continue;
            }
            if c == '"' && text.trim().is_empty() && !was_quoted {
                quoted = true;
                was_quoted = true;
                text.clear();
                if index == start_line {
                    cell_start = Some(at + width);
                    cell_end = at + width;
                }
                at += width;
                continue;
            }
            if !c.is_whitespace() && !was_quoted && index == start_line {
                if cell_start.is_none() {
                    cell_start = Some(at);
                }
                cell_end = at + width;
            }
            text.push(c);
            at += width;
        }
        cells.push(finish(&text, cell_start, cell_end, start_line, was_quoted));
        let blank = cells.iter().all(|cell| cell.text.is_empty());
        if !blank {
            records.push(Record {
                line: start_line,
                cells,
            });
        }
        index += 1;
    }
    records
}

fn finish(text: &str, start: Option<usize>, end: usize, line: usize, quoted: bool) -> Cell {
    let trimmed = if quoted { text } else { text.trim() };
    let start = start.unwrap_or(end);
    Cell {
        text: trimmed.trim().to_string(),
        at: Spot {
            line,
            start,
            end: end.max(start),
        },
    }
}

/// Read `text` as a delimited table: the delimiter whose records agree longest on how many
/// cells they have. `None` when no delimiter gives more than one cell to a run of lines.
pub fn sheet(text: &str) -> Option<Sheet> {
    let lines = lines(text);
    let mut best: Option<(usize, char, Vec<Record>)> = None;
    for delimiter in DELIMITERS {
        let records = split(&lines, delimiter);
        let score = steady_run(&records);
        if score > 0 && best.as_ref().is_none_or(|(top, ..)| score > *top) {
            best = Some((score, delimiter, records));
        }
    }
    let (_, delimiter, records) = best?;
    Some(Sheet {
        lines,
        delimiter,
        records,
    })
}

/// How many cells the longest run of consecutive records with the same count of two or more
/// cells holds, in total: a table's body under one delimiter.
fn steady_run(records: &[Record]) -> usize {
    let mut best = 0;
    let mut run = 0;
    let mut width = 0;
    for record in records {
        let count = record.cells.len();
        if count >= 2 && count == width {
            run += 1;
        } else {
            run = usize::from(count >= 2);
            width = count;
        }
        if run >= 2 {
            best = best.max(run * width);
        }
    }
    best
}

// --- Numbers ---

/// What a number's text says about the decimal mark it uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    /// It decides: `1.234,56`, `12,5`, `0.125`, `1,000,000`.
    Decimal(char),
    /// It could go either way: `1.000`, `12,345`.
    Either,
    /// It has no mark at all: `512`.
    None,
}

/// A number as the file writes it, taken apart: the sign, the digits and marks, and any
/// currency written beside it.
#[derive(Clone, Debug, PartialEq)]
pub struct Written {
    pub negative: bool,
    pub body: String,
    pub currency: Option<String>,
}

/// Currency symbols that may sit beside an amount, and the code each means.
const SYMBOLS: [(&str, &str); 6] = [
    ("€", "EUR"),
    ("$", "USD"),
    ("£", "GBP"),
    ("¥", "JPY"),
    ("CHF", "CHF"),
    ("Fr.", "CHF"),
];

/// Take a number's text apart: `-1.234,56 €`, `(12.50)`, `USD 10.00`, `12,00-`. `None` when
/// it isn't a number in any of the ways exports write one.
pub fn written(text: &str) -> Option<Written> {
    let mut rest = text.trim().replace(['\u{a0}', '\u{202f}'], " ");
    let mut negative = false;
    let mut currency = None;
    if rest.starts_with('(') && rest.ends_with(')') {
        negative = true;
        rest = rest[1..rest.len() - 1].trim().to_string();
    }
    // A currency before or after: a symbol, or three capital letters set off by a space.
    for (symbol, code) in SYMBOLS {
        if let Some(after) = rest.strip_prefix(symbol) {
            currency = Some(code.to_string());
            rest = after.trim().to_string();
            break;
        }
        if let Some(before) = rest.strip_suffix(symbol) {
            currency = Some(code.to_string());
            rest = before.trim().to_string();
            break;
        }
    }
    if currency.is_none() {
        if let Some((code, after)) = rest.split_once(' ')
            && is_currency_code(code)
        {
            currency = Some(code.to_string());
            rest = after.trim().to_string();
        } else if let Some((before, code)) = rest.rsplit_once(' ')
            && is_currency_code(code)
        {
            currency = Some(code.to_string());
            rest = before.trim().to_string();
        }
    }
    // A sign may come before or after the currency: `-€12` and `€-12` both occur.
    if let Some(after) = rest.strip_prefix('-').or_else(|| rest.strip_prefix('−')) {
        negative = !negative;
        rest = after.trim().to_string();
    } else if let Some(after) = rest.strip_prefix('+') {
        rest = after.trim().to_string();
    } else if let Some(before) = rest.strip_suffix('-') {
        negative = !negative;
        rest = before.trim().to_string();
    }
    if currency.is_none() {
        for (symbol, code) in SYMBOLS {
            if let Some(after) = rest.strip_prefix(symbol) {
                currency = Some(code.to_string());
                rest = after.trim().to_string();
                break;
            }
        }
    }
    let first_digit = rest
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || c == '.' || c == ',');
    let allowed = rest
        .chars()
        .all(|c| c.is_ascii_digit() || matches!(c, '.' | ',' | ' ' | '\'' | '’'));
    if rest.is_empty() || !first_digit || !allowed || !rest.contains(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(Written {
        negative,
        body: rest,
        currency,
    })
}

/// Three capital letters that name a currency. Only the ones a broker's export is likely to
/// print, so a word like "ETF" isn't taken for money.
pub fn is_currency_code(text: &str) -> bool {
    const CODES: [&str; 24] = [
        "EUR", "USD", "GBP", "CHF", "JPY", "CAD", "AUD", "NZD", "SEK", "NOK", "DKK", "PLN", "CZK",
        "HUF", "HKD", "SGD", "CNY", "INR", "ZAR", "MXN", "BRL", "TRY", "ILS", "KRW",
    ];
    CODES.contains(&text)
}

/// What the marks in a number's digits say about its decimal mark.
pub fn mark_of(body: &str) -> Mark {
    let marks: Vec<char> = body.chars().filter(|c| matches!(c, '.' | ',')).collect();
    let spaced = body.contains([' ', '\'', '’']);
    match marks.as_slice() {
        [] => Mark::None,
        [only] => {
            let after = body.rsplit(*only).next().unwrap_or("");
            let before = body.split(*only).next().unwrap_or("");
            let before_digits = before
                .trim_start_matches('0')
                .chars()
                .filter(char::is_ascii_digit)
                .count();
            if after.len() == 3 && !spaced && (1..=3).contains(&before_digits) {
                Mark::Either
            } else {
                Mark::Decimal(*only)
            }
        }
        [first, .., last] if first != last => Mark::Decimal(*last),
        // One mark, repeated: it separates thousands, so the other is the decimal.
        [first, ..] => Mark::Decimal(if *first == '.' { ',' } else { '.' }),
    }
}

/// Read `written` with `decimal` as its decimal mark. Thousands must be grouped in threes; a
/// number that breaks that is refused.
pub fn value(written: &Written, decimal: char) -> Option<f64> {
    let body = &written.body;
    let (whole, fraction) = match body.rsplit_once(decimal) {
        Some((whole, fraction)) => (whole, fraction),
        None => (body.as_str(), ""),
    };
    if !fraction.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let group: Vec<char> = whole.chars().filter(|c| !c.is_ascii_digit()).collect();
    let digits: String = if group.is_empty() {
        whole.to_string()
    } else {
        let mark = group[0];
        if mark == decimal || group.iter().any(|other| *other != mark) {
            return None;
        }
        let mut parts = whole.split(mark);
        let first = parts.next().unwrap_or("");
        let well_formed = !first.is_empty()
            && first.len() <= 3
            && parts.all(|part| part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()));
        if !well_formed {
            return None;
        }
        whole.chars().filter(char::is_ascii_digit).collect()
    };
    if digits.is_empty() && fraction.is_empty() {
        return None;
    }
    let sign = if written.negative { "-" } else { "" };
    let number: f64 = format!("{sign}0{digits}.{fraction}0").parse().ok()?;
    number.is_finite().then_some(number)
}

/// Settle a column's (or a file's) decimal mark from what its numbers say. `None` when nothing
/// decides it, and also when the numbers contradict each other.
pub fn decimal_of<'a>(bodies: impl IntoIterator<Item = &'a str>) -> Option<char> {
    let mut seen = None;
    for body in bodies {
        if let Mark::Decimal(mark) = mark_of(body) {
            match seen {
                None => seen = Some(mark),
                Some(other) if other != mark => return None,
                Some(_) => {}
            }
        }
    }
    seen
}

/// Whether some numbers contradict each other about the decimal mark.
pub fn decimal_conflict<'a>(bodies: impl IntoIterator<Item = &'a str>) -> bool {
    let mut dot = false;
    let mut comma = false;
    for body in bodies {
        match mark_of(body) {
            Mark::Decimal('.') => dot = true,
            Mark::Decimal(_) => comma = true,
            _ => {}
        }
    }
    dot && comma
}

// --- Dates ---

/// The order of a date's day and month when the text alone doesn't say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Order {
    DayFirst,
    MonthFirst,
}

/// What a date's text says, before the column decides an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DateText {
    /// Only one reading: ISO, dotted (always day first), or a month written as a word.
    Sure(NaiveDate),
    /// `a/b/year` or `a-b-year`: day-first or month-first, both possible. Holds the numbers.
    Either {
        a: u32,
        b: u32,
        year: i32,
    },
    NotADate,
}

const MONTHS: [(&str, u32); 36] = [
    ("january", 1),
    ("januar", 1),
    ("jan", 1),
    ("jän", 1),
    ("february", 2),
    ("februar", 2),
    ("feb", 2),
    ("march", 3),
    ("märz", 3),
    ("mär", 3),
    ("mar", 3),
    ("april", 4),
    ("apr", 4),
    ("may", 5),
    ("mai", 5),
    ("june", 6),
    ("juni", 6),
    ("jun", 6),
    ("july", 7),
    ("juli", 7),
    ("jul", 7),
    ("august", 8),
    ("aug", 8),
    ("september", 9),
    ("sept", 9),
    ("sep", 9),
    ("october", 10),
    ("oktober", 10),
    ("oct", 10),
    ("okt", 10),
    ("november", 11),
    ("nov", 11),
    ("december", 12),
    ("dezember", 12),
    ("dec", 12),
    ("dez", 12),
];

fn month_word(word: &str) -> Option<u32> {
    let word = word.trim_end_matches('.').to_lowercase();
    MONTHS
        .iter()
        .find(|(name, _)| *name == word)
        .map(|(_, month)| *month)
}

fn year_of(text: &str) -> Option<i32> {
    let digits = text.trim();
    if !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    match digits.len() {
        4 => digits.parse().ok(),
        // Two-digit years are this century: no broker export reaches back to the last one with
        // a two-digit year.
        2 => digits.parse::<i32>().ok().map(|year| 2000 + year),
        _ => None,
    }
}

/// Read a date's text. A time after it (`2026-11-10T09:31:00Z`, `10.11.2026 09:31`) is ignored.
pub fn date_text(text: &str) -> DateText {
    let text = text.trim();
    let text = text
        .split(['T', ' '])
        .next()
        .filter(|first| first.chars().filter(char::is_ascii_digit).count() >= 6)
        .unwrap_or(text);
    let numeric = |parts: &[&str]| -> Option<Vec<u32>> {
        parts
            .iter()
            .map(|part| part.trim().parse::<u32>().ok())
            .collect()
    };
    if text.contains(|c: char| c.is_alphabetic()) {
        return worded(text);
    }
    for separator in ['-', '/', '.'] {
        let parts: Vec<&str> = text.split(separator).collect();
        if parts.len() != 3 || parts.iter().any(|part| part.trim().is_empty()) {
            continue;
        }
        if parts[0].len() == 4 {
            let Some(numbers) = numeric(&parts) else {
                return DateText::NotADate;
            };
            return NaiveDate::from_ymd_opt(numbers[0] as i32, numbers[1], numbers[2])
                .map_or(DateText::NotADate, DateText::Sure);
        }
        let (Some(numbers), Some(year)) = (numeric(&parts[..2]), year_of(parts[2])) else {
            return DateText::NotADate;
        };
        let (a, b) = (numbers[0], numbers[1]);
        if separator == '.' {
            // Dots are always day first.
            return NaiveDate::from_ymd_opt(year, b, a).map_or(DateText::NotADate, DateText::Sure);
        }
        let day_first = NaiveDate::from_ymd_opt(year, b, a);
        let month_first = NaiveDate::from_ymd_opt(year, a, b);
        return match (day_first, month_first) {
            (Some(date), None) | (None, Some(date)) => DateText::Sure(date),
            (Some(_), Some(_)) if a == b => {
                DateText::Sure(NaiveDate::from_ymd_opt(year, a, a).expect("valid"))
            }
            (Some(_), Some(_)) => DateText::Either { a, b, year },
            (None, None) => DateText::NotADate,
        };
    }
    DateText::NotADate
}

/// `12 Nov 2026`, `12. November 2026`, `Nov 12, 2026`, `12-Nov-2026`.
fn worded(text: &str) -> DateText {
    let words: Vec<&str> = text
        .split(|c: char| c.is_whitespace() || matches!(c, ',' | '-' | '/'))
        .map(|word| word.trim_end_matches('.'))
        .filter(|word| !word.is_empty())
        .collect();
    if words.len() != 3 {
        return DateText::NotADate;
    }
    let number = |word: &str| word.parse::<u32>().ok();
    let found = if let Some(month) = month_word(words[1]) {
        number(words[0])
            .zip(year_of(words[2]))
            .map(|(day, year)| (year, month, day))
    } else if let Some(month) = month_word(words[0]) {
        number(words[1])
            .zip(year_of(words[2]))
            .map(|(day, year)| (year, month, day))
    } else {
        None
    };
    found
        .and_then(|(year, month, day)| NaiveDate::from_ymd_opt(year, month, day))
        .map_or(DateText::NotADate, DateText::Sure)
}

/// The date `text` names under `order`, when it names one.
pub fn date(text: &str, order: Option<Order>) -> Option<NaiveDate> {
    match date_text(text) {
        DateText::Sure(date) => Some(date),
        DateText::Either { a, b, year } => match order? {
            Order::DayFirst => NaiveDate::from_ymd_opt(year, b, a),
            Order::MonthFirst => NaiveDate::from_ymd_opt(year, a, b),
        },
        DateText::NotADate => None,
    }
}

/// The order a column of dates is in, when any of them decides it. `None` when none does, or
/// when they contradict each other.
pub fn order_of<'a>(texts: impl IntoIterator<Item = &'a str>) -> Option<Order> {
    let mut seen = None;
    for text in texts {
        let DateText::Sure(_) = date_text(text) else {
            continue;
        };
        // Only a slashed or dashed date that couldn't be the other way round decides.
        let parts: Vec<&str> = text.trim().split(['/', '-']).collect();
        if parts.len() != 3 || parts[0].len() == 4 {
            continue;
        }
        let (Ok(a), Ok(b)) = (
            parts[0].trim().parse::<u32>(),
            parts[1].trim().parse::<u32>(),
        ) else {
            continue;
        };
        let order = if a > 12 && b <= 12 {
            Order::DayFirst
        } else if b > 12 && a <= 12 {
            Order::MonthFirst
        } else {
            continue;
        };
        match seen {
            None => seen = Some(order),
            Some(other) if other != order => return None,
            Some(_) => {}
        }
    }
    seen
}

// --- Identifiers ---

/// An ISIN: two letters, nine letters or digits, and a check digit that agrees.
pub fn is_isin(text: &str) -> bool {
    let text = text.trim();
    let bytes = text.as_bytes();
    if bytes.len() != 12
        || !bytes[..2].iter().all(u8::is_ascii_uppercase)
        || !bytes[2..11]
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        || !bytes[11].is_ascii_digit()
    {
        return false;
    }
    // Letters become two digits (A = 10), then the Luhn check over all of it.
    let mut digits = Vec::new();
    for byte in &bytes[..11] {
        if byte.is_ascii_digit() {
            digits.push(u32::from(byte - b'0'));
        } else {
            let number = u32::from(byte - b'A') + 10;
            digits.push(number / 10);
            digits.push(number % 10);
        }
    }
    let mut sum = 0;
    for (index, digit) in digits.iter().rev().enumerate() {
        let mut digit = *digit;
        if index % 2 == 0 {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }
        sum += digit;
    }
    let check = (10 - sum % 10) % 10;
    check == u32::from(bytes[11] - b'0')
}

/// Text folded for comparing words: lower case, umlauts spelled out, punctuation as spaces.
pub fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            'é' | 'è' | 'ê' => out.push('e'),
            'à' | 'á' | 'â' => out.push('a'),
            c if c.is_alphanumeric() => out.push(c),
            '&' => out.push('&'),
            _ => out.push(' '),
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn number(text: &str, decimal: char) -> Option<f64> {
        value(&written(text)?, decimal)
    }

    #[test]
    fn text_is_decoded_exactly_or_refused() {
        assert_eq!(decode("a;b\n".as_bytes()).unwrap(), "a;b\n");
        assert_eq!(decode(b"\xef\xbb\xbfa;b").unwrap(), "a;b");
        assert_eq!(decode(b"St\xfcck;Kurs \x80").unwrap(), "Stück;Kurs €");
        let utf16: Vec<u8> = [0xff, 0xfe]
            .into_iter()
            .chain("Ä;b".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode(&utf16).unwrap(), "Ä;b");
        assert_eq!(decode(b"\x00\x01PK\x03\x04"), Err(NotText::Binary));
        assert_eq!(decode(b"  \n "), Err(NotText::Empty));
        assert_eq!(
            decode(&vec![b'a'; MAX_BYTES + 1]),
            Err(NotText::TooLarge(MAX_BYTES + 1))
        );
    }

    #[test]
    fn cells_keep_their_place_in_the_line() {
        let lines = lines("Name;\"Kurs; EUR\";  12,50 \n\"A \"\"B\"\"\";x");
        let records = split(&lines, ';');
        assert_eq!(records.len(), 2);
        let first = &records[0];
        assert_eq!(first.cells[1].text, "Kurs; EUR");
        assert_eq!(
            &lines[0][first.cells[1].at.start..first.cells[1].at.end],
            "Kurs; EUR"
        );
        assert_eq!(first.cells[2].text, "12,50");
        assert_eq!(
            &lines[0][first.cells[2].at.start..first.cells[2].at.end],
            "12,50"
        );
        assert_eq!(records[1].cells[0].text, "A \"B\"");
    }

    #[test]
    fn a_quoted_cell_may_run_over_a_line() {
        let lines = lines("a,\"one\ntwo\",3\nb,c,4");
        let records = split(&lines, ',');
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].cells[1].text, "one\ntwo");
        assert_eq!(records[0].cells[2].text, "3");
        assert_eq!(records[1].line, 2);
    }

    #[test]
    fn the_delimiter_is_the_one_the_table_agrees_on() {
        let text = "Depot 123, Export\n\nDatum;Name;Stück;Kurs\n01.02.2026;A, Inc.;1,5;12,30\n02.02.2026;B;2;3,00\n";
        let sheet = sheet(text).unwrap();
        assert_eq!(sheet.delimiter, ';');
        let tabbed = sheet_of("a\tb\tc\n1\t2\t3\n4\t5\t6");
        assert_eq!(tabbed, '\t');
        assert_eq!(sheet_of("a,b\n1,2\n3,4"), ',');
        assert!(super::sheet("just a line\nand another").is_none());
    }

    fn sheet_of(text: &str) -> char {
        sheet(text).unwrap().delimiter
    }

    #[test]
    fn numbers_come_apart_with_their_currency() {
        let read = written("-1.234,56 €").unwrap();
        assert!(read.negative);
        assert_eq!(read.body, "1.234,56");
        assert_eq!(read.currency.as_deref(), Some("EUR"));
        assert_eq!(
            written("USD 10.00").unwrap().currency.as_deref(),
            Some("USD")
        );
        assert_eq!(written("€512.40").unwrap().body, "512.40");
        assert!(written("(12.50)").unwrap().negative);
        assert!(written("12,00-").unwrap().negative);
        assert_eq!(written("ETF"), None);
        assert_eq!(written("12 ETF"), None);
        assert_eq!(written(""), None);
        assert_eq!(written("1e3"), None);
    }

    #[test]
    fn a_number_is_read_only_under_a_decided_mark() {
        assert_eq!(number("1.234,56", ','), Some(1234.56));
        assert_eq!(number("1,234.56", '.'), Some(1234.56));
        assert_eq!(number("512,40", ','), Some(512.4));
        assert_eq!(number("-0,5", ','), Some(-0.5));
        assert_eq!(number("1 000,5", ','), Some(1000.5));
        assert_eq!(number("3", ','), Some(3.0));
        assert_eq!(number("1.000", ','), Some(1000.0));
        assert_eq!(number("1.000", '.'), Some(1.0));
        // Thousands come in threes.
        assert_eq!(number("1.00,5", ','), None);
        assert_eq!(number("1,234.56", ','), None);
    }

    #[test]
    fn marks_decide_or_admit_they_cannot() {
        assert_eq!(mark_of("1.234,56"), Mark::Decimal(','));
        assert_eq!(mark_of("12,5"), Mark::Decimal(','));
        assert_eq!(mark_of("0.125"), Mark::Decimal('.'));
        assert_eq!(mark_of("1,000,000"), Mark::Decimal('.'));
        assert_eq!(mark_of("1.000"), Mark::Either);
        assert_eq!(mark_of("512"), Mark::None);
        assert_eq!(decimal_of(["1.000", "12,5", "3"]), Some(','));
        assert_eq!(decimal_of(["1.000", "3"]), None);
        assert_eq!(decimal_of(["1.5", "12,5"]), None);
        assert!(decimal_conflict(["1.5", "12,5"]));
    }

    #[test]
    fn dates_are_read_one_way_or_not_at_all() {
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(date("2026-11-10", None), Some(day(2026, 11, 10)));
        assert_eq!(date("2026/11/10", None), Some(day(2026, 11, 10)));
        assert_eq!(date("10.11.2026", None), Some(day(2026, 11, 10)));
        assert_eq!(date("10.11.26", None), Some(day(2026, 11, 10)));
        assert_eq!(date("10.11.2026 09:31", None), Some(day(2026, 11, 10)));
        assert_eq!(date("2026-11-10T09:31:00Z", None), Some(day(2026, 11, 10)));
        assert_eq!(date("12 Nov 2026", None), Some(day(2026, 11, 12)));
        assert_eq!(date("Nov 12, 2026", None), Some(day(2026, 11, 12)));
        assert_eq!(date("12. März 2026", None), Some(day(2026, 3, 12)));
        assert_eq!(date("25/11/2026", None), Some(day(2026, 11, 25)));
        assert_eq!(date("11/25/2026", None), Some(day(2026, 11, 25)));
        // Either way round is possible: no date until the column says which.
        assert_eq!(date("10/11/2026", None), None);
        assert_eq!(
            date("10/11/2026", Some(Order::DayFirst)),
            Some(day(2026, 11, 10))
        );
        assert_eq!(
            date("10/11/2026", Some(Order::MonthFirst)),
            Some(day(2026, 10, 11))
        );
        assert_eq!(date("31.02.2026", None), None);
        assert_eq!(date("Apple", None), None);
        assert_eq!(
            order_of(["10/11/2026", "25/11/2026"]),
            Some(Order::DayFirst)
        );
        assert_eq!(
            order_of(["10/11/2026", "11/25/2026"]),
            Some(Order::MonthFirst)
        );
        assert_eq!(order_of(["10/11/2026"]), None);
        assert_eq!(order_of(["25/11/2026", "11/25/2026"]), None);
    }

    #[test]
    fn isins_are_checked() {
        assert!(is_isin("US0378331005"));
        assert!(is_isin("IE00B5BMR087"));
        assert!(is_isin("DE0005140008"));
        assert!(!is_isin("US0378331006"));
        assert!(!is_isin("AAPL"));
    }

    #[test]
    fn words_are_folded_for_comparison() {
        assert_eq!(fold("Stück/Nominale"), "stueck nominale");
        assert_eq!(fold("  S&P 500 ETF  "), "s&p 500 etf");
    }
}
