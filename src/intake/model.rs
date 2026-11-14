//! A model's help with a table the rules couldn't map, under the iron rule.
//!
//! A model may say "column 4 is the quantity". It may never supply the quantity. Its answer
//! names columns, and each name must come with a value the model saw in that column, which is
//! checked against the file: a model that describes a column it didn't read is not listened to.
//! The rows are then read by the rules, from the cells, and every figure is checked once more
//! by [`gate`] to be literally where it says it is.

use serde::Deserialize;

use super::{
    rules::{Aside, Basis, Entry, Mapping, Role, SetAside, body},
    text::{self, Sheet, Spot},
};
use crate::{ask::Request, facts::Table};

/// How many records of the table a model is shown: enough to see its columns, and no more of
/// the owner's file than that.
pub const EXCERPT_RECORDS: usize = 12;

/// What the model is told.
pub const SYSTEM: &str = "You map the columns of a table exported by a stockbroker. You are \
shown its first lines, each cell numbered by its column from 0. Answer with one JSON object and \
nothing else, in this shape:\n\
{\"header\": <the line number of the header row, or null>, \"columns\": [{\"column\": <number>, \
\"role\": <role>, \"example\": <one value copied exactly from that column>}]}\n\
Roles: date (the trade date), kind (buy or sell), name, isin, ticker, quantity (shares or units, \
not money), price (per share or unit), total (money for the whole row), currency, fees. Leave out \
columns you are unsure of. Never invent or compute a value: every example must be copied \
character for character from the table.";

/// The part of the file a model is shown, exactly: the first records, each cell numbered.
pub fn excerpt(sheet: &Sheet) -> String {
    let mut out = String::new();
    for record in sheet.records.iter().take(EXCERPT_RECORDS) {
        let cells: Vec<String> = record
            .cells
            .iter()
            .enumerate()
            .map(|(index, cell)| format!("[{index}] {}", cell.text))
            .collect();
        out.push_str(&format!("line {}: {}\n", record.line, cells.join(" | ")));
    }
    out
}

/// The request to send: the instructions and the excerpt, and nothing about the portfolio.
pub fn request(sheet: &Sheet) -> Request {
    Request {
        question: excerpt(sheet),
        conversation: Vec::new(),
        system: SYSTEM.to_string(),
        facts: Table::default(),
        cancel: None,
    }
}

#[derive(Deserialize)]
struct Answer {
    header: Option<usize>,
    #[serde(default)]
    columns: Vec<Named>,
}

#[derive(Deserialize)]
struct Named {
    column: usize,
    role: String,
    example: Option<String>,
}

/// The JSON object in a model's answer, which may come wrapped in a code fence or a sentence.
fn object_in(answer: &str) -> Option<&str> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    (end > start).then(|| &answer[start..=end])
}

/// What came of a model's answer: the mapping it leads to, and what of it was refused.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub mapping: Mapping,
    pub refused: Vec<String>,
}

/// Read a model's answer into a mapping over `base`. Only roles for columns the table has, whose
/// example is really in that column, are taken; the rest are refused and said so. Roles the
/// rules settled from headers are kept.
pub fn suggestion(answer: &str, sheet: &Sheet, base: &Mapping) -> Result<Suggestion, String> {
    let json =
        object_in(answer).ok_or_else(|| "The model's answer had no mapping in it.".to_string())?;
    let answer: Answer = serde_json::from_str(json)
        .map_err(|_| "The model's answer couldn't be read.".to_string())?;
    let mut mapping = base.clone();
    if base.header.is_none()
        && let Some(line) = answer.header
        && let Some(at) = sheet.records.iter().position(|record| record.line == line)
    {
        mapping.header = Some(at);
        for column in &mut mapping.columns {
            if let Some(cell) = sheet.records[at].cells.get(column.index) {
                column.header = cell.text.clone();
                column.at = Some(cell.at);
            }
        }
    }
    let mut refused = Vec::new();
    for named in answer.columns {
        let Some(role) = Role::from_key(named.role.trim()) else {
            refused.push(format!("“{}” isn't a kind of column.", named.role));
            continue;
        };
        let Some(column) = mapping
            .columns
            .iter()
            .find(|column| column.index == named.column)
        else {
            refused.push(format!("The table has no column {}.", named.column));
            continue;
        };
        if column.basis == Some(Basis::Header) {
            continue;
        }
        let seen = named.example.as_deref().is_some_and(|example| {
            body(sheet, &mapping)
                .filter_map(|record| record.cell(named.column))
                .any(|cell| cell.text == example.trim())
        });
        if !seen {
            refused.push(format!(
                "Column {} as {}: the value the model gave for it isn't in that column.",
                named.column,
                role.label().to_lowercase()
            ));
            continue;
        }
        mapping.assign(named.column, Some(role), Basis::Model);
    }
    Ok(Suggestion { mapping, refused })
}

/// Whether `text` is exactly what the file has at `at`. A header's currency points at the
/// header's cell, which holds more than the code, so text inside the spot also counts there.
fn literal(sheet: &Sheet, at: Spot, text: &str, inside: bool) -> bool {
    let Some(line) = sheet.lines.get(at.line) else {
        return false;
    };
    let Some(found) = line.get(at.start..at.end) else {
        return false;
    };
    if inside {
        found.contains(text)
    } else {
        found == text
    }
}

/// Whether the file says `currency` where the entry says it does: the code written out (a
/// header's `Kurs (EUR)`, a currency column's `EUR`) or a symbol that means it (`$271.86`).
fn currency_literal(sheet: &Sheet, at: Spot, currency: &str) -> bool {
    let Some(line) = sheet.lines.get(at.line) else {
        return false;
    };
    let Some(found) = line.get(at.start..at.end) else {
        return false;
    };
    found.contains(currency)
        || text::written(found).is_some_and(|written| written.currency.as_deref() == Some(currency))
}

/// The last check before anything is proposed: every figure and every word in an entry must be
/// exactly what the file has where the entry says it came from. An entry that fails is set
/// aside; nothing of it is proposed.
pub fn gate(sheet: &Sheet, entries: Vec<Entry>) -> (Vec<Entry>, Vec<SetAside>) {
    let mut kept = Vec::new();
    let mut refused = Vec::new();
    for entry in entries {
        let figures = [
            Some(&entry.quantity),
            entry.price.as_ref(),
            entry.fees.as_ref(),
        ];
        let words = [
            entry.name.as_ref(),
            entry.isin.as_ref(),
            entry.ticker.as_ref(),
            entry.class.as_ref(),
        ];
        let figures_ok = figures
            .into_iter()
            .flatten()
            .all(|figure| literal(sheet, figure.at, &figure.text, false));
        let words_ok = words
            .into_iter()
            .flatten()
            .all(|word| literal(sheet, word.at, &word.text, false));
        let date_ok = entry
            .date
            .as_ref()
            .is_none_or(|date| literal(sheet, date.at, &date.text, false));
        let currency_ok = entry
            .currency
            .as_ref()
            .is_none_or(|currency| currency_literal(sheet, currency.at, &currency.text));
        if figures_ok && words_ok && date_ok && currency_ok {
            kept.push(entry);
        } else {
            refused.push(SetAside {
                line: entry.line,
                why: Aside::Unsure(
                    "A figure for this row isn't in the file where it was said to be, so the row \
                     wasn't read."
                        .to_string(),
                ),
            });
        }
    }
    (kept, refused)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intake::{Outcome, rules, text};

    /// A table no header names: the rules can't tell which number is which.
    const BARE: &str = "\
2026-11-03;IE00B5BMR087;iShares Core S&P 500;3;512,40
2026-11-05;US0378331005;Apple;10;201,15
";

    fn sheet() -> Sheet {
        text::sheet(BARE).unwrap()
    }

    #[test]
    fn the_rules_alone_ask_for_help_with_a_bare_table() {
        let Outcome::Unmapped {
            missing, mapping, ..
        } = crate::intake::understand(BARE.as_bytes())
        else {
            panic!("a bare table can't be read with certainty");
        };
        assert_eq!(missing, vec!["which column is the quantity"]);
        assert_eq!(
            mapping.column(Role::Isin).map(|c| c.index),
            Some(1),
            "ISINs read by their check digit"
        );
    }

    #[test]
    fn a_model_names_columns_and_the_rows_are_read_from_the_cells() {
        let sheet = sheet();
        let base = rules::map(&sheet);
        let answer = r#"Here you are: {"header": null, "columns": [
            {"column": 3, "role": "quantity", "example": "3"},
            {"column": 4, "role": "price", "example": "512,40"},
            {"column": 2, "role": "name", "example": "Apple"}]}"#;
        let suggestion = suggestion(answer, &sheet, &base).unwrap();
        assert!(suggestion.refused.is_empty(), "{:?}", suggestion.refused);
        let Outcome::Read { reading, .. } = crate::intake::remap(sheet, suggestion.mapping) else {
            panic!("mapped");
        };
        assert_eq!(reading.entries.len(), 2);
        assert_eq!(reading.entries[0].quantity.value, 3.0);
        assert_eq!(reading.entries[0].price.as_ref().unwrap().text, "512,40");
    }

    #[test]
    fn a_column_described_with_a_value_not_in_it_is_refused() {
        let sheet = sheet();
        let base = rules::map(&sheet);
        // The model says column 3 is the quantity and that it saw 30 there. It didn't.
        let answer = r#"{"header": null, "columns": [
            {"column": 3, "role": "quantity", "example": "30"},
            {"column": 9, "role": "price", "example": "1"},
            {"column": 4, "role": "price"}]}"#;
        let suggestion = suggestion(answer, &sheet, &base).unwrap();
        assert_eq!(suggestion.refused.len(), 3, "{:?}", suggestion.refused);
        assert!(!suggestion.mapping.has(Role::Quantity));
        assert!(!suggestion.mapping.has(Role::Price));
    }

    #[test]
    fn a_figure_not_literally_in_the_file_rejects_its_row() {
        let sheet = sheet();
        let base = rules::map(&sheet);
        let answer = r#"{"columns": [{"column": 3, "role": "quantity", "example": "3"},
            {"column": 4, "role": "price", "example": "512,40"}]}"#;
        let mapping = suggestion(answer, &sheet, &base).unwrap().mapping;
        let reading = rules::read(&sheet, &mapping);
        let (kept, refused) = gate(&sheet, reading.entries.clone());
        assert_eq!(kept.len(), 2, "figures copied from cells pass");
        assert!(refused.is_empty());

        // A figure whose value came from anywhere but the file, however it got there.
        let mut forged = reading.entries.clone();
        forged[0].quantity.text = "30".to_string();
        forged[0].quantity.value = 30.0;
        // And one that points at the wrong place.
        forged[1].price.as_mut().unwrap().at.start += 1;
        let (kept, refused) = gate(&sheet, forged);
        assert!(kept.is_empty());
        assert_eq!(refused.len(), 2);
        assert!(matches!(refused[0].why, Aside::Unsure(_)));
    }

    #[test]
    fn the_model_is_shown_the_start_of_the_file_and_nothing_else() {
        let many: String = (0..40)
            .map(|n| format!("2026-01-01;Name {n};{n};1,5\n"))
            .collect();
        let sheet = text::sheet(&many).unwrap();
        let request = request(&sheet);
        assert_eq!(request.question.lines().count(), EXCERPT_RECORDS);
        assert!(request.question.contains("[1] Name 0"));
        assert!(!request.question.contains("Name 12"));
        assert!(
            request.facts.facts.is_empty(),
            "nothing about the portfolio goes with it"
        );
        assert!(request.conversation.is_empty());
    }

    #[test]
    fn an_answer_without_a_mapping_says_so() {
        let sheet = sheet();
        let base = rules::map(&sheet);
        assert!(suggestion("I can't help with that.", &sheet, &base).is_err());
        assert!(suggestion("{not json}", &sheet, &base).is_err());
    }
}
