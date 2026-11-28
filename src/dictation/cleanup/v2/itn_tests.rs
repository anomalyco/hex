//! The reference's own ITN cases (`tests/test_itn.py`), one for one.

use super::*;

fn words(spoken: &str) -> Vec<&str> {
    spoken.split_whitespace().collect()
}

#[test]
fn numbers_are_written_as_digits() {
    for (spoken, written) in [
        ("eight eight four one", "8841"),
        ("four oh one seven", "4017"),
        ("twenty four hundred", "2400"),
        ("two hundred fifty", "250"),
        ("one thousand two hundred", "1200"),
        ("forty two", "42"),
        ("three point five", "3.5"),
        ("zero point zero five", "0.05"),
        ("twenty four hundred dollars", "$2,400"),
        ("five hundred dollars", "$500"),
        ("forty two dollars and fifty cents", "$42.50"),
        ("twenty euros", "\u{20ac}20"),
        ("eighty pounds", "\u{a3}80"),
        ("twenty percent", "20%"),
        ("three point five percent", "3.5%"),
        ("seven thirty", "7:30"),
        ("seven thirty pm", "7:30 pm"),
        ("seven oh five am", "7:05 am"),
        ("nine o'clock", "9:00"),
        ("twenty first", "21st"),
        ("ninth", "9th"),
        ("fifty k", "50k"),
        ("twelve oh eight", "1208"),
        ("twenty oh four", "2004"),
        ("twelve oh eight pm", "12:08 pm"),
        ("one million two hundred thousand", "1200000"),
        ("three billion dollars", "$3,000,000,000"),
    ] {
        assert_eq!(number(&words(spoken)).as_deref(), Some(written), "{spoken}");
    }
}

#[test]
fn a_number_it_does_not_recognise_stays_as_heard() {
    for spoken in [
        "hello world",
        "twenty thirty",
        "foo dollars",
        "two two hundred",
        "hundred hundred",
    ] {
        assert_eq!(number(&words(spoken)), None, "{spoken}");
    }
    // Past u128 there is no amount to write.
    let huge = "9".repeat(45);
    assert_eq!(number(&[huge.as_str(), "dollars"]), None);
}

#[test]
fn units_are_written_with_their_abbreviation() {
    for (spoken, written) in [
        ("two hundred fifty milliseconds", "250 milliseconds"),
        ("fifty megabytes", "50 MB"),
        ("four gigabytes", "4 GB"),
        ("seventy two degrees fahrenheit", "72 \u{b0}F"),
        ("twenty one degrees celsius", "21 \u{b0}C"),
        ("twelve kilograms", "12 kg"),
        ("five kilometers", "5 km"),
        ("sixty seconds", "60 seconds"),
        ("ten minutes", "10 minutes"),
        ("twenty five milligrams", "25 milligrams"),
        ("three point five gigahertz", "3.5 GHz"),
        ("one second", "1 second"),
    ] {
        assert_eq!(unit(&words(spoken)).as_deref(), Some(written), "{spoken}");
    }
    for spoken in ["many milliseconds", "five bananas", "twenty second"] {
        assert_eq!(unit(&words(spoken)), None, "{spoken}");
    }
}

#[test]
fn emails_are_joined_at_their_spoken_symbols() {
    for (spoken, written) in [
        ("john at foo dot com", "john@foo.com"),
        (
            "priya dot raman at northwind dot io",
            "priya.raman@northwind.io",
        ),
        ("priya.raman at northwind.io", "priya.raman@northwind.io"),
        (
            "alice underscore chen at example dot org",
            "alice_chen@example.org",
        ),
        ("ops at northwind.io", "ops@northwind.io"),
    ] {
        assert_eq!(email(&words(spoken)).as_deref(), Some(written), "{spoken}");
    }
    for spoken in ["meet at noon", "at foo dot com", "be ops at northwind.io"] {
        assert_eq!(email(&words(spoken)), None, "{spoken}");
    }
}

#[test]
fn links_are_joined_at_their_spoken_symbols() {
    for (spoken, written) in [
        ("foo dot com slash docs", "foo.com/docs"),
        (
            "https colon slash slash docs dot example dot com slash v2",
            "https://docs.example.com/v2",
        ),
        ("w w w dot example dot org", "www.example.org"),
        (
            "northwind.io slash api slash streams",
            "northwind.io/api/streams",
        ),
    ] {
        assert_eq!(url(&words(spoken)).as_deref(), Some(written), "{spoken}");
    }
    assert_eq!(url(&words("hello world")), None);
}

#[test]
fn code_is_joined_and_spoken_identifiers_are_camel_cased() {
    for (spoken, written) in [
        ("foo underscore bar", "foo_bar"),
        ("billing dot t s", "billing.ts"),
        ("src slash main dot r s", "src/main.rs"),
        ("dash dash dry dash run", "--dry-run"),
        ("use state", "useState"),
        ("get user by id", "getUserById"),
        ("at slash", "@/"),
        ("at slash components", "@/components"),
        ("tilde slash", "~/"),
        ("dot dot slash", "../"),
        ("foo colon colon bar", "foo::bar"),
    ] {
        assert_eq!(code(&words(spoken)).as_deref(), Some(written), "{spoken}");
    }
    assert_eq!(code(&words("single")), None);
}

#[test]
fn the_punctuation_around_a_span_stays_with_it() {
    assert_eq!(
        format_with_punctuation(Kind::Number, &words("(two hundred fifty dollars),")).as_deref(),
        Some("($250),")
    );
    assert_eq!(
        format_with_punctuation(Kind::Email, &words("\u{201c}ops at northwind.io.\u{201d}"))
            .as_deref(),
        Some("\u{201c}ops@northwind.io.\u{201d}")
    );
    assert_eq!(
        format_with_punctuation(Kind::Number, &words("hello,")),
        None
    );
}
