//! Number formatting, matching what `Intl.NumberFormat("en-US")` produced in
//! the previous web UI so figures read the same.

/// The symbol en-US uses for a currency, falling back to its code.
fn currency_symbol(code: &str) -> String {
    match code {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "JPY" => "¥",
        "INR" => "₹",
        "KRW" => "₩",
        "ILS" => "₪",
        "VND" => "₫",
        "CNY" => "CN¥",
        "CAD" => "CA$",
        "AUD" => "A$",
        "NZD" => "NZ$",
        "HKD" => "HK$",
        "MXN" => "MX$",
        "TWD" => "NT$",
        "BRL" => "R$",
        // A code reads as a word, so it gets a non-breaking space.
        other => return format!("{other}\u{a0}"),
    }
    .to_string()
}

/// Insert thousands separators into a string of ASCII digits.
fn group_thousands(digits: &str) -> String {
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `-€8,878.50`
pub fn currency(value: f64, code: &str) -> String {
    with_prefix(value, &currency_symbol(code))
}

/// `-8,878.50`, for an amount whose currency isn't known.
pub fn amount(value: f64) -> String {
    with_prefix(value, "")
}

fn with_prefix(value: f64, prefix: &str) -> String {
    let fixed = format!("{:.2}", value.abs());
    let (whole, fraction) = fixed.split_once('.').unwrap_or((&fixed, "00"));
    // `-0.001` rounds to zero, and a zero has no sign.
    let is_zero = whole.bytes().all(|b| b == b'0') && fraction.bytes().all(|b| b == b'0');
    let sign = if value.is_sign_negative() && !is_zero {
        "-"
    } else {
        ""
    };
    format!("{sign}{prefix}{}.{fraction}", group_thousands(whole))
}

/// What money reads as in privacy mode.
pub const HIDDEN: &str = "••••••";

/// `+€12.00`, for changes where the direction matters.
pub fn signed_currency(value: f64, code: &str) -> String {
    let formatted = currency(value, code);
    if value >= 0.005 {
        format!("+{formatted}")
    } else {
        formatted
    }
}

/// `+1.23%` or `-1.23%`
pub fn percent(value: f64) -> String {
    format!("{}{value:.2}%", if value >= 0.0 { "+" } else { "" })
}

/// [`percent`], with `-` standing in for a missing value.
pub fn optional_percent(value: Option<f64>) -> String {
    value.map_or_else(|| "-".to_string(), percent)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_currency_like_intl() {
        assert_eq!(currency(123_727.05, "EUR"), "€123,727.05");
        assert_eq!(currency(-8_878.5, "EUR"), "-€8,878.50");
        assert_eq!(currency(0.0, "USD"), "$0.00");
        assert_eq!(currency(999.0, "GBP"), "£999.00");
        assert_eq!(currency(1_000.0, "GBP"), "£1,000.00");
        assert_eq!(currency(1_234_567.891, "USD"), "$1,234,567.89");
    }

    #[test]
    fn an_amount_has_no_currency() {
        assert_eq!(amount(-8_878.5), "-8,878.50");
        assert_eq!(amount(42_000.0), "42,000.00");
    }

    #[test]
    fn signs_changes_but_not_a_zero() {
        assert_eq!(signed_currency(12.0, "EUR"), "+€12.00");
        assert_eq!(signed_currency(-12.0, "EUR"), "-€12.00");
        assert_eq!(signed_currency(0.0, "EUR"), "€0.00");
    }

    #[test]
    fn unknown_currency_uses_its_code() {
        assert_eq!(currency(1_500.0, "CHF"), "CHF\u{a0}1,500.00");
    }

    #[test]
    fn rounding_to_zero_drops_the_sign() {
        assert_eq!(currency(-0.001, "EUR"), "€0.00");
        assert_eq!(currency(-0.0, "EUR"), "€0.00");
    }

    #[test]
    fn formats_percent_with_sign() {
        assert_eq!(percent(1.234), "+1.23%");
        assert_eq!(percent(-0.52), "-0.52%");
        assert_eq!(percent(0.0), "+0.00%");
        assert_eq!(optional_percent(None), "-");
        assert_eq!(optional_percent(Some(2.0)), "+2.00%");
    }
}
