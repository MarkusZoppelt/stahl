//! Number formatting, matching what `Intl.NumberFormat("en-US")` produced in
//! the previous web UI so figures read the same, and places as people name them.

use std::env;

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

/// Where a file or folder is, as the person looking for it would say. On iOS an app's container
/// path means nothing to anyone: Files calls Stahl's own folder "On My iPhone › Stahl", and
/// iCloud's "iCloud Drive". Elsewhere a path is already that.
pub fn place(path: &str) -> String {
    if !cfg!(target_os = "ios") {
        return path.to_string();
    }
    let home = env::var("HOME").unwrap_or_default();
    let device = env::var("STAHL_DEVICE").unwrap_or_else(|_| "iPhone".to_string());
    place_on_ios(path, &home, &device)
}

/// [`place`] for paths inside a sentence, such as portfolio_rs's health checks: the roots Files
/// names are renamed wherever they appear, and the rest stays as written.
pub fn place_in(text: &str) -> String {
    if !cfg!(target_os = "ios") {
        return text.to_string();
    }
    let home = env::var("HOME").unwrap_or_default();
    let device = env::var("STAHL_DEVICE").unwrap_or_else(|_| "iPhone".to_string());
    place_in_on_ios(text, &home, &device)
}

fn place_in_on_ios(text: &str, home: &str, device: &str) -> String {
    const ICLOUD: &str = "/private/var/mobile/Library/Mobile Documents/com~apple~CloudDocs";
    let text = text.replace(ICLOUD, "iCloud Drive");
    if home.is_empty() {
        return text;
    }
    text.replace(
        &format!("{home}/Documents"),
        &format!("On My {device}/Stahl"),
    )
}

fn place_on_ios(path: &str, home: &str, device: &str) -> String {
    const ICLOUD: &str = "/Mobile Documents/com~apple~CloudDocs";
    let (root, rest) = if let Some(rest) = (!home.is_empty())
        .then(|| path.strip_prefix(&format!("{home}/Documents")))
        .flatten()
    {
        (format!("On My {device} › Stahl"), rest)
    } else if let Some((_, rest)) = path.split_once(ICLOUD) {
        ("iCloud Drive".to_string(), rest)
    } else {
        return path.to_string();
    };
    rest.split('/')
        .filter(|part| !part.is_empty())
        .fold(root, |place, part| format!("{place} › {part}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_on_ios_reads_as_files_names_it() {
        let home = "/var/mobile/Containers/Data/Application/ABC";
        assert_eq!(
            place_on_ios(&format!("{home}/Documents/My Portfolio"), home, "iPad"),
            "On My iPad › Stahl › My Portfolio"
        );
        assert_eq!(
            place_on_ios(&format!("{home}/Documents"), home, "iPhone"),
            "On My iPhone › Stahl"
        );
        assert_eq!(
            place_on_ios(
                "/private/var/mobile/Library/Mobile Documents/com~apple~CloudDocs/Finance/Stahl",
                home,
                "iPhone"
            ),
            "iCloud Drive › Finance › Stahl"
        );
        assert_eq!(
            place_on_ios("/elsewhere/x.json", home, "iPhone"),
            "/elsewhere/x.json"
        );
        assert_eq!(
            place_in_on_ios(
                &format!("Policy file is valid: {home}/Documents/Mine/portfolio/policy.toml"),
                home,
                "iPhone"
            ),
            "Policy file is valid: On My iPhone/Stahl/Mine/portfolio/policy.toml"
        );
    }

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
