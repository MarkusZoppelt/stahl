//! Working out what a table says: which column is which, and what each row records.
//!
//! The rules come first and are pure. Every column's role says how it was decided (its
//! header, its values, the owner, or a model's suggestion) and whether that is certain. A row
//! the rules can't read with certainty is set aside with the reason, never guessed at.

use chrono::NaiveDate;

use super::text::{self, Order, Record, Sheet, Spot, fold};

/// What a column holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    Date,
    Kind,
    Name,
    Isin,
    Ticker,
    Wkn,
    Quantity,
    Price,
    Amount,
    Currency,
    Fees,
    Class,
}

impl Role {
    pub const ALL: [Role; 12] = [
        Role::Date,
        Role::Kind,
        Role::Name,
        Role::Isin,
        Role::Ticker,
        Role::Wkn,
        Role::Quantity,
        Role::Price,
        Role::Amount,
        Role::Currency,
        Role::Fees,
        Role::Class,
    ];

    /// What the owner picks from when they say what a column is.
    pub const CHOICES: [Role; 9] = [
        Role::Date,
        Role::Kind,
        Role::Name,
        Role::Isin,
        Role::Ticker,
        Role::Quantity,
        Role::Price,
        Role::Currency,
        Role::Fees,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Role::Date => "Date",
            Role::Kind => "Buy or sell",
            Role::Name => "Name",
            Role::Isin => "ISIN",
            Role::Ticker => "Ticker",
            Role::Wkn => "WKN",
            Role::Quantity => "Quantity",
            Role::Price => "Price",
            Role::Amount => "Total",
            Role::Currency => "Currency",
            Role::Fees => "Fees",
            Role::Class => "Asset class",
        }
    }

    /// The word a model uses for it, and that its answer is read back by.
    pub fn key(self) -> &'static str {
        match self {
            Role::Date => "date",
            Role::Kind => "kind",
            Role::Name => "name",
            Role::Isin => "isin",
            Role::Ticker => "ticker",
            Role::Wkn => "wkn",
            Role::Quantity => "quantity",
            Role::Price => "price",
            Role::Amount => "total",
            Role::Currency => "currency",
            Role::Fees => "fees",
            Role::Class => "class",
        }
    }

    pub fn from_key(key: &str) -> Option<Role> {
        Role::ALL.into_iter().find(|role| role.key() == key)
    }

    fn numeric(self) -> bool {
        matches!(
            self,
            Role::Quantity | Role::Price | Role::Amount | Role::Fees
        )
    }
}

/// Header words, folded (see [`fold`]), for each role. A header matches when it is one of these,
/// or one of these followed by a currency in brackets (`Kurs (EUR)`). The first role listed
/// for a header wins, and within a role the earlier words are the better match: a trade date
/// beats a settlement date.
const HEADERS: &[(Role, &[&str])] = &[
    (
        Role::Date,
        &[
            "trade date",
            "transaction date",
            "date",
            "execution date",
            "order date",
            "handelstag",
            "handelsdatum",
            "schlusstag",
            "ausfuehrungsdatum",
            "ausfuehrungstag",
            "datum",
            "auftragsdatum",
            "buchungstag",
            "buchungsdatum",
            "booking date",
            "settlement date",
            "value date",
            "valuta",
            "valutadatum",
            "as of",
            "stichtag",
        ],
    ),
    (
        Role::Kind,
        &[
            "type",
            "transaction type",
            "action",
            "side",
            "buy sell",
            "transaction",
            "activity",
            "order type",
            "typ",
            "art",
            "transaktion",
            "transaktionstyp",
            "transaktionsart",
            "geschaeftsart",
            "buchungsart",
            "orderart",
            "auftragsart",
            "vorgang",
        ],
    ),
    (Role::Isin, &["isin", "isin code", "security isin"]),
    (Role::Wkn, &["wkn"]),
    (
        Role::Ticker,
        &[
            "ticker",
            "symbol",
            "ticker symbol",
            "kuerzel",
            "boersenkuerzel",
        ],
    ),
    (
        Role::Name,
        &[
            "name",
            "security",
            "security name",
            "instrument",
            "instrument name",
            "description",
            "product",
            "holding",
            "investment",
            "asset",
            "wertpapier",
            "wertpapiername",
            "bezeichnung",
            "wertpapierbezeichnung",
            "produkt",
            "titel",
            "gattung",
        ],
    ),
    (
        Role::Quantity,
        &[
            "quantity",
            "qty",
            "shares",
            "units",
            "number of shares",
            "no of shares",
            "amount of shares",
            "anzahl",
            "stueck",
            "stk",
            "stueckzahl",
            "menge",
            "nominale",
            "stueck nominale",
            "bestand",
            "anteile",
        ],
    ),
    (
        Role::Price,
        &[
            "price",
            "last price",
            "close",
            "closing price",
            "share price",
            "unit price",
            "price per share",
            "price per unit",
            "execution price",
            "trade price",
            "kurs",
            "preis",
            "ausfuehrungskurs",
            "ausfuehrungspreis",
            "kaufkurs",
            "stueckpreis",
        ],
    ),
    (
        Role::Amount,
        &[
            "amount",
            "total",
            "net amount",
            "gross amount",
            "value",
            "market value",
            "current value",
            "betrag",
            "gesamtbetrag",
            "endbetrag",
            "kurswert",
            "marktwert",
            "wert",
            "summe",
        ],
    ),
    (
        Role::Currency,
        &[
            "currency",
            "ccy",
            "waehrung",
            "price currency",
            "trading currency",
            "handelswaehrung",
        ],
    ),
    (
        Role::Fees,
        &[
            "fees",
            "fee",
            "fees & comm",
            "fees & commissions",
            "commission",
            "transaction costs",
            "costs",
            "gebuehren",
            "gebuehr",
            "provision",
            "entgelt",
            "fremdkosten",
            "transaktionskosten",
        ],
    ),
    (
        Role::Class,
        &[
            "asset class",
            "asset type",
            "anlageklasse",
            "wertpapierart",
            "wertpapiertyp",
        ],
    ),
];

/// How a column's role was decided.
#[derive(Clone, Debug, PartialEq)]
pub enum Basis {
    /// Its header is a word for it.
    Header,
    /// Its values have the shape (an ISIN's check digit, dates, currency codes).
    Values,
    /// The owner said so.
    Owner,
    /// A model suggested it. Its figures are still copied from the file.
    Model,
}

/// One column of the table.
#[derive(Clone, Debug, PartialEq)]
pub struct Column {
    pub index: usize,
    pub header: String,
    pub role: Option<Role>,
    pub basis: Option<Basis>,
    /// A currency the header names: `Kurs (EUR)`.
    pub currency: Option<String>,
    /// Where the header's text sits, when there is a header.
    pub at: Option<Spot>,
}

/// Which column is which, and how to read the numbers and dates in them.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    /// The record (an index into `Sheet::records`) that names the columns, if one does.
    pub header: Option<usize>,
    pub columns: Vec<Column>,
    pub decimal: Option<char>,
    pub order: Option<Order>,
}

impl Mapping {
    pub fn column(&self, role: Role) -> Option<&Column> {
        self.columns.iter().find(|column| column.role == Some(role))
    }

    pub fn has(&self, role: Role) -> bool {
        self.column(role).is_some()
    }

    /// Give `index` the role `role`, taking it from any column that had it.
    pub fn assign(&mut self, index: usize, role: Option<Role>, basis: Basis) {
        for column in &mut self.columns {
            if role.is_some() && column.role == role && column.index != index {
                column.role = None;
                column.basis = None;
            }
        }
        if let Some(column) = self.columns.iter_mut().find(|column| column.index == index) {
            column.role = role;
            column.basis = role.map(|_| basis);
        }
    }

    /// What is still missing before rows can be read, in plain words. Empty when nothing is.
    pub fn missing(&self) -> Vec<&'static str> {
        let mut missing = Vec::new();
        if !(self.has(Role::Isin) || self.has(Role::Ticker) || self.has(Role::Name)) {
            missing.push("which column names the security");
        }
        if !self.has(Role::Quantity) {
            missing.push("which column is the quantity");
        }
        missing
    }

    /// The first record of the table's body.
    pub fn body_start(&self) -> usize {
        self.header.map_or(0, |header| header + 1)
    }
}

/// The header's own currency, in brackets or after a word: `Kurs (EUR)`, `Price in USD`,
/// `Price USD`. Returns the header without it.
fn header_currency(folded: &str, original: &str) -> (String, Option<String>) {
    let upper: Vec<&str> = original
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let code = upper
        .iter()
        .find(|word| text::is_currency_code(word))
        .map(|code| code.to_string())
        .or_else(|| original.contains('€').then(|| "EUR".to_string()))
        .or_else(|| original.contains('$').then(|| "USD".to_string()));
    let Some(code) = code else {
        return (folded.to_string(), None);
    };
    let lower = code.to_lowercase();
    let stripped: Vec<&str> = folded
        .split(' ')
        .filter(|word| *word != lower && *word != "in")
        .collect();
    (stripped.join(" "), Some(code))
}

/// The role a header names, and the rank of the word that named it (lower is better).
pub fn header_role(header: &str) -> Option<(Role, usize, Option<String>)> {
    let folded = fold(header);
    let (bare, currency) = header_currency(&folded, header);
    for (role, words) in HEADERS {
        for (rank, word) in words.iter().enumerate() {
            if bare == *word || folded == *word {
                return Some((*role, rank, currency));
            }
        }
    }
    None
}

/// Find the header and map the columns. The header is the record with the most cells that
/// name a role (at least two), searched from the top so junk above it is passed over.
pub fn map(sheet: &Sheet) -> Mapping {
    let mut best: Option<(usize, usize)> = None;
    for (at, record) in sheet.records.iter().enumerate().take(60) {
        let named = record
            .cells
            .iter()
            .filter(|cell| header_role(&cell.text).is_some())
            .count();
        if named >= 2 && best.is_none_or(|(_, top)| named > top) {
            best = Some((at, named));
        }
    }
    let header = best.map(|(at, _)| at);
    let width = match header {
        Some(at) => sheet.records[at].cells.len(),
        None => sheet
            .records
            .iter()
            .map(|record| record.cells.len())
            .max()
            .unwrap_or(0),
    };
    let mut columns: Vec<Column> = (0..width)
        .map(|index| Column {
            index,
            header: header
                .and_then(|at| sheet.records[at].cells.get(index))
                .map(|cell| cell.text.clone())
                .unwrap_or_default(),
            role: None,
            basis: None,
            currency: None,
            at: header
                .and_then(|at| sheet.records[at].cells.get(index))
                .map(|cell| cell.at),
        })
        .collect();

    // Headers first; where two claim a role, the better word wins and the other is left alone.
    let mut claimed: Vec<(Role, usize, usize)> = Vec::new();
    for column in &mut columns {
        if let Some((role, rank, currency)) = header_role(&column.header) {
            column.currency = currency;
            match claimed.iter_mut().find(|(each, ..)| *each == role) {
                Some(entry) if rank < entry.1 => *entry = (role, rank, column.index),
                Some(_) => {}
                None => claimed.push((role, rank, column.index)),
            }
        }
    }
    for (role, _, index) in claimed {
        columns[index].role = Some(role);
        columns[index].basis = Some(Basis::Header);
    }
    let mut mapping = Mapping {
        header,
        columns,
        decimal: None,
        order: None,
    };
    by_values(sheet, &mut mapping);
    settle_formats(sheet, &mut mapping);
    mapping
}

/// The body's records: everything below the header that has more than one thing in it.
pub fn body<'a>(sheet: &'a Sheet, mapping: &Mapping) -> impl Iterator<Item = &'a Record> {
    sheet
        .records
        .iter()
        .skip(mapping.body_start())
        .filter(|record| record.filled() >= 2)
}

/// Roles a column's values decide by their shape, for columns no header named: ISINs (by
/// their check digit), dates, and currency codes. Never a quantity or a price: nothing in a
/// number says which of the two it is.
fn by_values(sheet: &Sheet, mapping: &mut Mapping) {
    let rows: Vec<&Record> = body(sheet, mapping).take(200).collect();
    if rows.is_empty() {
        return;
    }
    let unnamed: Vec<usize> = mapping
        .columns
        .iter()
        .filter(|column| column.role.is_none())
        .map(|column| column.index)
        .collect();
    for index in unnamed {
        let values: Vec<&str> = rows
            .iter()
            .filter_map(|record| record.cell(index))
            .map(|cell| cell.text.as_str())
            .collect();
        if values.len() < rows.len().div_ceil(2) {
            continue;
        }
        let all = |test: &dyn Fn(&str) -> bool| values.iter().all(|value| test(value));
        let role = if !mapping.has(Role::Isin) && all(&text::is_isin) {
            Some(Role::Isin)
        } else if !mapping.has(Role::Currency) && all(&text::is_currency_code) {
            Some(Role::Currency)
        } else if !mapping.has(Role::Date)
            && all(&|value| !matches!(text::date_text(value), text::DateText::NotADate))
        {
            Some(Role::Date)
        } else {
            None
        };
        if let Some(role) = role {
            mapping.assign(index, Some(role), Basis::Values);
        }
    }
}

/// The decimal mark and the date order, from the values themselves. A column whose numbers
/// decide nothing takes the file's mark; a file that contradicts itself has none.
pub fn settle_formats(sheet: &Sheet, mapping: &mut Mapping) {
    let rows: Vec<&Record> = body(sheet, mapping).collect();
    let numeric: Vec<usize> = mapping
        .columns
        .iter()
        .filter(|column| column.role.is_some_and(Role::numeric))
        .map(|column| column.index)
        .collect();
    let bodies: Vec<String> = rows
        .iter()
        .flat_map(|record| numeric.iter().filter_map(|index| record.cell(*index)))
        .filter_map(|cell| text::written(&cell.text))
        .map(|written| written.body)
        .collect();
    // Nothing is assumed from the delimiter or the language: only the figures decide.
    mapping.decimal = text::decimal_of(bodies.iter().map(String::as_str));
    if let Some(date) = mapping.column(Role::Date).map(|column| column.index) {
        mapping.order = text::order_of(
            rows.iter()
                .filter_map(|record| record.cell(date))
                .map(|cell| cell.text.as_str()),
        );
    }
}

// --- Rows ---

/// A piece of text from the file, and where it sits.
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    pub at: Spot,
}

/// A number from the file: the value read, and the exact text it was read from.
#[derive(Clone, Debug, PartialEq)]
pub struct Figure {
    pub value: f64,
    pub text: String,
    pub at: Spot,
}

/// A date from the file.
#[derive(Clone, Debug, PartialEq)]
pub struct Dated {
    pub date: NaiveDate,
    pub text: String,
    pub at: Spot,
}

/// What a row of a transactions list records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Buy,
    Sell,
}

/// How a row's kind was decided.
#[derive(Clone, Debug, PartialEq)]
pub enum KindBasis {
    /// The row's own word for it.
    Word(Text),
    /// No column says; the quantity's sign does.
    Sign,
    /// A holdings list: there is no trade, only a quantity held.
    Held,
}

/// A row read with certainty.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub line: usize,
    pub kind: Option<Kind>,
    pub kind_basis: KindBasis,
    pub date: Option<Dated>,
    pub name: Option<Text>,
    pub isin: Option<Text>,
    pub ticker: Option<Text>,
    pub class: Option<Text>,
    /// Signed: a sale is negative, whichever way the file wrote it.
    pub quantity: Figure,
    pub price: Option<Figure>,
    pub fees: Option<Figure>,
    /// The currency of the price, and where it was read.
    pub currency: Option<Text>,
}

impl Entry {
    /// The best name the row has for its security.
    pub fn title(&self) -> String {
        self.name
            .as_ref()
            .or(self.ticker.as_ref())
            .or(self.isin.as_ref())
            .map_or_else(|| "this security".to_string(), |text| text.text.clone())
    }
}

/// Why a row was set aside.
#[derive(Clone, Debug, PartialEq)]
pub enum Aside {
    /// A row that isn't a trade: a dividend, a deposit, a fee. Holds the row's word for it.
    NotATrade(String),
    /// A total or a subtotal.
    Total,
    /// Couldn't be read with certainty, and why.
    Unsure(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SetAside {
    pub line: usize,
    pub why: Aside,
}

/// Whether the table lists trades or what is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Transactions,
    Holdings,
}

/// Everything the rules made of a table.
#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    pub shape: Shape,
    pub entries: Vec<Entry>,
    pub aside: Vec<SetAside>,
    /// The date a holdings list is as of, when it says.
    pub as_of: Option<NaiveDate>,
}

const BUY: [&str; 12] = [
    "buy",
    "bought",
    "purchase",
    "kauf",
    "wertpapierkauf",
    "sparplan",
    "savings plan",
    "sparplanausfuehrung",
    "kauf sparplan",
    "zeichnung",
    "subscription",
    "market buy",
];
const SELL: [&str; 7] = [
    "sell",
    "sold",
    "sale",
    "verkauf",
    "wertpapierverkauf",
    "market sell",
    "redemption",
];
const NOT_TRADES: [&str; 30] = [
    "dividend",
    "dividende",
    "ausschuettung",
    "distribution",
    "interest",
    "zinsen",
    "zins",
    "deposit",
    "einzahlung",
    "withdrawal",
    "auszahlung",
    "fee",
    "fees",
    "gebuehr",
    "gebuehren",
    "tax",
    "steuer",
    "steuern",
    "vorabpauschale",
    "transfer",
    "uebertrag",
    "split",
    "coupon",
    "kupon",
    "cash",
    "einbuchung",
    "ausbuchung",
    "corporate action",
    "kapitalmassnahme",
    "reinvestment",
];

/// What a row's word for its kind means. `Err` holds the word when it is something other than
/// a trade, and `None` when the word is not known.
pub fn kind_of(word: &str) -> Option<Result<Kind, String>> {
    let folded = fold(word);
    let has = |list: &[&str]| {
        list.iter()
            .any(|each| folded == *each || folded.split(' ').any(|part| part == *each))
    };
    // "Verkauf" holds "kauf", so sales are looked for first.
    if has(&SELL) {
        Some(Ok(Kind::Sell))
    } else if has(&BUY) {
        Some(Ok(Kind::Buy))
    } else if has(&NOT_TRADES) {
        Some(Err(word.trim().to_string()))
    } else {
        None
    }
}

const TOTALS: [&str; 8] = [
    "total",
    "totals",
    "summe",
    "gesamt",
    "gesamtsumme",
    "subtotal",
    "zwischensumme",
    "grand total",
];

/// A total or subtotal: its first cell begins or ends with a word for one (`Summe`,
/// `Transactions Total`), and it has no quantity of its own.
fn is_total(record: &Record, mapping: &Mapping) -> bool {
    let Some(first) = record.cells.iter().find(|cell| !cell.text.is_empty()) else {
        return false;
    };
    let folded = fold(&first.text);
    let words: Vec<&str> = folded.split(' ').collect();
    let named = TOTALS.contains(&folded.as_str())
        || words.first().is_some_and(|word| TOTALS.contains(word))
        || words.last().is_some_and(|word| TOTALS.contains(word));
    let quantity = mapping
        .column(Role::Quantity)
        .and_then(|column| record.cell(column.index));
    named && quantity.is_none()
}

fn text_at(record: &Record, mapping: &Mapping, role: Role) -> Option<Text> {
    let column = mapping.column(role)?;
    record.cell(column.index).map(|cell| Text {
        text: cell.text.clone(),
        at: cell.at,
    })
}

/// A number in a column, read under the mapping's mark. `Err` says why it can't be read.
fn figure_at(
    record: &Record,
    mapping: &Mapping,
    role: Role,
) -> Result<Option<(Figure, Option<String>)>, String> {
    let Some(column) = mapping.column(role) else {
        return Ok(None);
    };
    let Some(cell) = record.cell(column.index) else {
        return Ok(None);
    };
    let label = role.label().to_lowercase();
    let Some(written) = text::written(&cell.text) else {
        return Err(format!("The {label} “{}” isn't a number.", cell.text));
    };
    let decimal = match text::mark_of(&written.body) {
        text::Mark::Decimal(mark) => {
            if mapping.decimal.is_some_and(|decimal| decimal != mark) {
                return Err(format!(
                    "The {label} “{}” uses a different decimal mark from the rest of the file.",
                    cell.text
                ));
            }
            mark
        }
        text::Mark::None => '.',
        text::Mark::Either => match mapping.decimal {
            Some(mark) => mark,
            None => {
                return Err(format!(
                    "The {label} “{}” could be read two ways, and nothing in the file says which.",
                    cell.text
                ));
            }
        },
    };
    let Some(value) = text::value(&written, decimal) else {
        return Err(format!("The {label} “{}” isn't a number.", cell.text));
    };
    Ok(Some((
        Figure {
            value,
            text: cell.text.clone(),
            at: cell.at,
        },
        written.currency,
    )))
}

/// Read the table's rows under `mapping`. Every value in an entry is copied from a cell; a row
/// that can't be read that way is set aside, with the reason.
pub fn read(sheet: &Sheet, mapping: &Mapping) -> Reading {
    let dates: Vec<NaiveDate> = mapping
        .column(Role::Date)
        .map(|column| {
            body(sheet, mapping)
                .filter_map(|record| record.cell(column.index))
                .filter_map(|cell| text::date(&cell.text, mapping.order))
                .collect()
        })
        .unwrap_or_default();
    let dates_differ = dates.windows(2).any(|pair| pair[0] != pair[1]);
    let shape = if mapping.has(Role::Kind) || dates_differ {
        Shape::Transactions
    } else {
        Shape::Holdings
    };
    let as_of = (shape == Shape::Holdings)
        .then(|| dates.first().copied())
        .flatten();

    let mut entries = Vec::new();
    let mut aside = Vec::new();
    for record in body(sheet, mapping) {
        let line = record.line;
        if is_total(record, mapping) {
            aside.push(SetAside {
                line,
                why: Aside::Total,
            });
            continue;
        }
        match entry(record, mapping, shape) {
            Ok(Some(entry)) => entries.push(entry),
            Ok(None) => {}
            Err(why) => aside.push(SetAside { line, why }),
        }
    }
    Reading {
        shape,
        entries,
        aside,
        as_of,
    }
}

fn entry(record: &Record, mapping: &Mapping, shape: Shape) -> Result<Option<Entry>, Aside> {
    let unsure = Aside::Unsure;
    let line = record.line;
    // A repeated header (some exports print one per page) is not a row.
    if mapping.header.is_some()
        && mapping
            .columns
            .iter()
            .filter(|column| !column.header.is_empty())
            .all(|column| {
                record
                    .cell(column.index)
                    .is_some_and(|cell| cell.text == column.header)
            })
    {
        return Ok(None);
    }
    let kind_word = text_at(record, mapping, Role::Kind);
    let word_kind = match &kind_word {
        Some(word) => match kind_of(&word.text) {
            Some(Ok(kind)) => Some(kind),
            Some(Err(word)) => return Err(Aside::NotATrade(word)),
            None => {
                return Err(unsure(format!(
                    "“{}” isn't a kind of trade we know, so this row wasn't read.",
                    word.text
                )));
            }
        },
        None if mapping.has(Role::Kind) => {
            return Err(unsure(
                "This row doesn't say whether it is a purchase or a sale.".to_string(),
            ));
        }
        None => None,
    };
    let name = text_at(record, mapping, Role::Name);
    let ticker = text_at(record, mapping, Role::Ticker);
    let isin = match text_at(record, mapping, Role::Isin) {
        Some(isin) if !text::is_isin(&isin.text) => {
            return Err(unsure(format!("“{}” isn't a valid ISIN.", isin.text)));
        }
        isin => isin,
    };
    if name.is_none() && ticker.is_none() && isin.is_none() {
        return Err(unsure("This row doesn't name a security.".to_string()));
    }
    let Some((quantity, _)) = figure_at(record, mapping, Role::Quantity).map_err(unsure)? else {
        return Err(unsure("This row has no quantity.".to_string()));
    };
    let price = figure_at(record, mapping, Role::Price).map_err(unsure)?;
    let fees = figure_at(record, mapping, Role::Fees).map_err(unsure)?;
    // Totals are read only to be sure they are numbers; nothing is recorded from them.
    figure_at(record, mapping, Role::Amount).map_err(unsure)?;
    let date = match mapping
        .column(Role::Date)
        .and_then(|column| record.cell(column.index))
    {
        Some(cell) => match text::date(&cell.text, mapping.order) {
            Some(date) => Some(Dated {
                date,
                text: cell.text.clone(),
                at: cell.at,
            }),
            None => {
                let why = match text::date_text(&cell.text) {
                    text::DateText::Either { .. } => format!(
                        "The date “{}” could be day-first or month-first, and nothing in the file says which.",
                        cell.text
                    ),
                    _ => format!("“{}” isn't a date we can read.", cell.text),
                };
                return Err(unsure(why));
            }
        },
        None => None,
    };
    if shape == Shape::Transactions && date.is_none() {
        return Err(unsure("This trade has no date.".to_string()));
    }

    let (kind, kind_basis, quantity) = match (shape, word_kind) {
        (Shape::Holdings, _) => {
            if quantity.value < 0.0 {
                return Err(unsure(format!(
                    "A holding of “{}” is less than nothing.",
                    quantity.text
                )));
            }
            (None, KindBasis::Held, quantity)
        }
        (Shape::Transactions, Some(kind)) => {
            let word = kind_word.clone().expect("a kind has its word");
            let signed = match kind {
                Kind::Buy if quantity.value < 0.0 => {
                    return Err(unsure(format!(
                        "A purchase of “{}” shares is negative.",
                        quantity.text
                    )));
                }
                Kind::Buy => quantity.value,
                Kind::Sell => -quantity.value.abs(),
            };
            (
                Some(kind),
                KindBasis::Word(word),
                Figure {
                    value: signed,
                    ..quantity
                },
            )
        }
        (Shape::Transactions, None) => {
            let kind = if quantity.value < 0.0 {
                Kind::Sell
            } else {
                Kind::Buy
            };
            (Some(kind), KindBasis::Sign, quantity)
        }
    };
    if quantity.value == 0.0 {
        return Err(unsure("The quantity is zero.".to_string()));
    }
    let (price, price_currency) = match price {
        Some((price, _)) if price.value < 0.0 => {
            return Err(unsure(format!("The price “{}” is negative.", price.text)));
        }
        Some((price, currency)) => (Some(price), currency),
        None => (None, None),
    };
    let fees = fees.map(|(fees, _)| Figure {
        value: fees.value.abs(),
        ..fees
    });
    let currency = price_currency
        .zip(price.as_ref())
        .map(|(code, price)| Text {
            text: code,
            at: price.at,
        })
        .or_else(|| {
            // The header names it: point at the header's cell.
            let column = mapping.column(Role::Price)?;
            Some(Text {
                text: column.currency.clone()?,
                at: column.at?,
            })
        })
        .or_else(|| {
            let cell = text_at(record, mapping, Role::Currency)?;
            text::is_currency_code(&cell.text.to_uppercase()).then(|| Text {
                text: cell.text.to_uppercase(),
                at: cell.at,
            })
        });
    Ok(Some(Entry {
        line,
        kind,
        kind_basis,
        date,
        name,
        isin,
        ticker,
        class: text_at(record, mapping, Role::Class),
        quantity,
        price,
        fees,
        currency,
    }))
}
