//! A broker's file, handed over: read, understood, compared with the record, and brought back
//! as a short list of changes for the owner to approve.
//!
//! - [`text`] decodes the bytes and splits them into cells that remember where they sit.
//! - [`rules`] works out which column is which, and reads each row or says why it can't.
//! - [`reconcile`] compares the rows with the positions on record.
//! - [`apply`] writes what the owner approved.
//! - [`model`] asks a model to say which column is which, when the owner wants it to, and
//!   checks every figure that comes of it against the file.
//!
//! The iron rule: every figure in a proposed change is copied from the file, and knows exactly
//! where in the file it came from.

pub mod apply;
pub mod model;
pub mod reconcile;
pub mod rules;
pub mod text;

use std::mem;

use rules::{Mapping, Reading};
use text::{NotText, Sheet};

/// The most rows a table may have to be reviewed here.
pub const MAX_ROWS: usize = 5_000;

/// What was made of a file.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    /// It can't be read, and this says why, in plain words.
    Unreadable(String),
    /// A table, but the rules couldn't tell which column is which. `missing` says what.
    Unmapped {
        sheet: Sheet,
        mapping: Mapping,
        missing: Vec<&'static str>,
    },
    /// Read under a mapping: the rows it found and the ones it set aside.
    Read {
        sheet: Sheet,
        mapping: Mapping,
        reading: Reading,
    },
}

/// Words joined the way a person says them: "a", "a and b", "a, b and c".
pub fn spoken(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_string(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

/// What the start of a file says it is, when it is something other than text.
fn kind_of_bytes(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF") {
        Some("pdf")
    } else if bytes.starts_with(b"PK\x03\x04") {
        Some("workbook")
    } else if bytes.starts_with(&[0xd0, 0xcf, 0x11, 0xe0]) {
        Some("old-workbook")
    } else {
        None
    }
}

/// Read a file's bytes and make what can be made of them. `name` is only for what is said.
pub fn understand(bytes: &[u8]) -> Outcome {
    match kind_of_bytes(bytes) {
        Some("pdf") => {
            return Outcome::Unreadable(
                "This is a PDF. Statements in PDF can't be read with certainty yet: the figures \
                 in them are placed on a page rather than set in columns. Your broker's website \
                 will offer the same list as a CSV export, which can."
                    .to_string(),
            );
        }
        Some(_) => {
            return Outcome::Unreadable(
                "This is a spreadsheet workbook, and only plain CSV exports can be read. Most \
                 brokers offer a CSV beside the Excel download; or open this one and save it \
                 as CSV."
                    .to_string(),
            );
        }
        None => {}
    }
    let text = match text::decode(bytes) {
        Ok(text) => text,
        Err(NotText::Empty) => return Outcome::Unreadable("The file is empty.".to_string()),
        Err(NotText::TooLarge(size)) => {
            return Outcome::Unreadable(format!(
                "The file is {:.1} MB, far larger than a broker's export, so it wasn't read.",
                size as f64 / 1_048_576.0
            ));
        }
        Err(NotText::Binary) => {
            return Outcome::Unreadable(
                "This isn't a text file, so it can't be read. A broker's CSV export can."
                    .to_string(),
            );
        }
    };
    let Some(sheet) = text::sheet(&text) else {
        return Outcome::Unreadable(
            "There is no table in this file: no two lines in a row are split into columns the \
             same way."
                .to_string(),
        );
    };
    if sheet.records.len() > MAX_ROWS {
        return Outcome::Unreadable(format!(
            "This table has {} rows, far more than a broker's export, so it wasn't read.",
            sheet.records.len()
        ));
    }
    let mapping = rules::map(&sheet);
    // Money and no securities: an account statement, not a broker's export. There is nothing
    // in it to map by hand.
    if mapping.missing().len() == 2 && mapping.has(rules::Role::Amount) {
        return Outcome::Unreadable(
            "This looks like an account statement: it lists sums of money, and no securities or \
             quantities. A broker's export of your transactions or holdings can be read."
                .to_string(),
        );
    }
    with(sheet, mapping)
}

/// Read `sheet` under `mapping`, or say what the mapping still lacks.
pub fn with(sheet: Sheet, mapping: Mapping) -> Outcome {
    let missing = mapping.missing();
    if !missing.is_empty() {
        return Outcome::Unmapped {
            sheet,
            mapping,
            missing,
        };
    }
    let mut reading = rules::read(&sheet, &mapping);
    // The iron rule, checked for every row however its columns were decided.
    let (kept, refused) = model::gate(&sheet, mem::take(&mut reading.entries));
    reading.entries = kept;
    reading.aside.extend(refused);
    reading.aside.sort_by_key(|aside| aside.line);
    Outcome::Read {
        sheet,
        mapping,
        reading,
    }
}

/// After the owner (or a model) changed the mapping: the formats are settled again from the
/// columns as they now are, and the rows read again. A date order the owner chose is kept.
pub fn remap(sheet: Sheet, mut mapping: Mapping) -> Outcome {
    let chosen_order = mapping.order;
    rules::settle_formats(&sheet, &mut mapping);
    if mapping.order.is_none() {
        mapping.order = chosen_order;
    }
    with(sheet, mapping)
}
