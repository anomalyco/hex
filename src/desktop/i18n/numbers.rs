//! Number formatting that follows the interface language: its decimal
//! separator and its thousands separator.

use super::{
    FRENCH_COLUMN, HUNGARIAN_COLUMN, PORTUGUESE_COLUMN, RUSSIAN_COLUMN, column_for_code, current,
};

/// Whether the active language writes a decimal fraction with a comma:
/// German, Spanish, French, Hungarian, Polish, Portuguese, Russian, and
/// Ukrainian.
pub(crate) fn decimal_comma() -> bool {
    let column = current();
    column == column_for_code("de")
        || column == column_for_code("es")
        || column == column_for_code("pl")
        || column == column_for_code("uk")
        || column == FRENCH_COLUMN
        || column == PORTUGUESE_COLUMN
        || column == RUSSIAN_COLUMN
        || column == HUNGARIAN_COLUMN
}

/// A count with its thousands grouped for the active language: `1,284` in
/// English and Chinese or Japanese, `1.284` in German, Spanish, and
/// Portuguese, and `1 284` with a narrow no-break space in French, Hungarian,
/// Polish, Russian, and Ukrainian.
pub(crate) fn grouped(value: u64) -> String {
    let separator = thousands_separator();
    let digits = value.to_string();
    let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.char_indices() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            grouped.push_str(separator);
        }
        grouped.push(digit);
    }
    grouped
}

fn thousands_separator() -> &'static str {
    let column = current();
    if column == column_for_code("de")
        || column == column_for_code("es")
        || column == PORTUGUESE_COLUMN
    {
        "."
    } else if column == column_for_code("pl")
        || column == column_for_code("uk")
        || column == FRENCH_COLUMN
        || column == RUSSIAN_COLUMN
        || column == HUNGARIAN_COLUMN
    {
        "\u{202f}"
    } else {
        ","
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop::i18n::{column_for_code, set_current};

    /// Runs `check` in `code`'s language, then puts English back for the
    /// tests that share this thread.
    fn in_language(code: &str, check: impl FnOnce()) {
        set_current(column_for_code(code));
        check();
        set_current(column_for_code("en"));
    }

    #[test]
    fn decimal_comma_follows_the_interface_language() {
        for (code, comma) in [
            ("de", true),
            ("es", true),
            ("fr", true),
            ("hu", true),
            ("pl", true),
            ("pt", true),
            ("ru", true),
            ("uk", true),
            ("en", false),
            ("ja", false),
            ("zh", false),
            ("zh-Hant", false),
        ] {
            in_language(code, || assert_eq!(decimal_comma(), comma, "{code}"));
        }
    }

    #[test]
    fn thousands_separate_by_the_interface_language() {
        for (code, expected) in [
            ("en", "1,284"),
            ("ja", "1,284"),
            ("zh", "1,284"),
            ("zh-Hant", "1,284"),
            ("de", "1.284"),
            ("es", "1.284"),
            ("pt", "1.284"),
            ("fr", "1\u{202f}284"),
            ("hu", "1\u{202f}284"),
            ("pl", "1\u{202f}284"),
            ("ru", "1\u{202f}284"),
            ("uk", "1\u{202f}284"),
        ] {
            in_language(code, || assert_eq!(grouped(1_284), expected, "{code}"));
        }
        // Every group boundary, and no others.
        in_language("en", || {
            assert_eq!(grouped(0), "0");
            assert_eq!(grouped(999), "999");
            assert_eq!(grouped(1_000), "1,000");
            assert_eq!(grouped(128_402), "128,402");
        });
        in_language("de", || assert_eq!(grouped(128_402), "128.402"));
    }
}
