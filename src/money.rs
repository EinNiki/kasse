//! Money helpers. All amounts are `i64` cents, never floats.

/// Largest amount accepted from user input (999,999.99 EUR).
const MAX_INPUT_CENTS: i64 = 99_999_999;

/// Formats cents as German currency, e.g. 123456 -> "1.234,56 €".
pub fn format_eur(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let abs = cents.unsigned_abs();
    let euros = (abs / 100).to_string();
    let mut grouped = String::new();
    for (i, ch) in euros.chars().enumerate() {
        if i > 0 && (euros.len() - i).is_multiple_of(3) {
            grouped.push('.');
        }
        grouped.push(ch);
    }
    format!("{sign}{grouped},{:02} €", abs % 100)
}

/// Parses user input like "3", "3,5", "3.50", "1.234,56" or "2,00 €" into cents.
/// Returns `None` for negative numbers, more than two decimals or garbage.
pub fn parse_eur(input: &str) -> Option<i64> {
    let s = input.trim().trim_end_matches('€').trim();
    // With a comma, dots are thousands separators; without one, a dot is the decimal mark.
    let normalized = if s.contains(',') {
        s.replace('.', "").replace(',', ".")
    } else {
        s.to_string()
    };
    let (int_part, frac_part) = normalized
        .split_once('.')
        .unwrap_or((normalized.as_str(), ""));
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    let digits = |p: &str| p.chars().all(|c| c.is_ascii_digit());
    if !digits(int_part) || !digits(frac_part) || frac_part.len() > 2 {
        return None;
    }
    let euros: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    let frac: i64 = match frac_part.len() {
        0 => 0,
        1 => frac_part.parse::<i64>().ok()? * 10,
        _ => frac_part.parse().ok()?,
    };
    let cents = euros.checked_mul(100)?.checked_add(frac)?;
    (cents <= MAX_INPUT_CENTS).then_some(cents)
}

/// `change_cents = paid_cents - total_cents`. `None` when the customer paid too little.
pub fn change_cents(total_cents: i64, paid_cents: i64) -> Option<i64> {
    (paid_cents >= total_cents).then(|| paid_cents - total_cents)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_german_currency() {
        assert_eq!(format_eur(0), "0,00 €");
        assert_eq!(format_eur(5), "0,05 €");
        assert_eq!(format_eur(300), "3,00 €");
        assert_eq!(format_eur(123456), "1.234,56 €");
        assert_eq!(format_eur(100000000), "1.000.000,00 €");
        assert_eq!(format_eur(-250), "-2,50 €");
    }

    #[test]
    fn parses_common_inputs() {
        assert_eq!(parse_eur("3"), Some(300));
        assert_eq!(parse_eur("3,5"), Some(350));
        assert_eq!(parse_eur("3,50"), Some(350));
        assert_eq!(parse_eur("3.50"), Some(350));
        assert_eq!(parse_eur(" 2,00 € "), Some(200));
        assert_eq!(parse_eur("1.234,56"), Some(123456));
        assert_eq!(parse_eur("0,05"), Some(5));
        assert_eq!(parse_eur(",5"), Some(50));
        assert_eq!(parse_eur("0"), Some(0));
    }

    #[test]
    fn rejects_invalid_inputs() {
        for bad in [
            "",
            " ",
            "abc",
            "-3",
            "3,555",
            "3,5,5",
            "1e3",
            "99999999999999999999",
            "1000000",
            "€",
        ] {
            assert_eq!(parse_eur(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn change_calculation() {
        assert_eq!(change_cents(500, 1000), Some(500));
        assert_eq!(change_cents(500, 500), Some(0), "exactly the same amount");
        assert_eq!(change_cents(500, 499), None);
        assert_eq!(change_cents(0, 0), Some(0));
    }
}
