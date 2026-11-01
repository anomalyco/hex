//! The `gsettings` command: GNOME's settings, read and written as GNOME
//! Settings itself would.

use color_eyre::eyre::{Result, WrapErr, eyre};

/// A string as a GVariant literal gsettings parses: single-quoted, with
/// quotes and backslashes escaped.
pub(crate) fn string_text(value: &str) -> String {
    let mut quoted = String::from("'");
    for character in value.chars() {
        if matches!(character, '\'' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('\'');
    quoted
}

pub(crate) fn gsettings(arguments: &[&str]) -> Result<String> {
    let output = std::process::Command::new("gsettings")
        .args(arguments)
        .stdin(std::process::Stdio::null())
        .output()
        .wrap_err("could not run gsettings")?;
    if !output.status.success() {
        return Err(eyre!(
            "gsettings {} failed: {}",
            arguments.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// A GVariant `as` as gsettings prints it: `['a', 'b']` or `@as []`.
pub(crate) fn string_list(text: &str) -> Vec<String> {
    let text = text.trim().trim_start_matches("@as").trim();
    text.trim_start_matches('[')
        .trim_end_matches(']')
        .split(',')
        .map(|item| {
            item.trim()
                .trim_matches(|c| c == '\'' || c == '"')
                .to_owned()
        })
        .filter(|item| !item.is_empty())
        .collect()
}

/// The items (app ids, object paths) never contain quotes or backslashes, so
/// they need no escaping.
pub(crate) fn string_list_text(items: &[&str]) -> String {
    if items.is_empty() {
        return "@as []".into();
    }
    let quoted = items
        .iter()
        .map(|item| format!("'{item}'"))
        .collect::<Vec<_>>();
    format!("[{}]", quoted.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gsettings_string_lists_round_trip() {
        assert_eq!(
            string_list("['hex', 'anomaly.hex']"),
            ["hex", "anomaly.hex"]
        );
        assert!(string_list("@as []").is_empty());
        assert_eq!(string_list_text(&["anomaly.hex"]), "['anomaly.hex']");
        assert_eq!(string_list_text(&[]), "@as []");
    }

    #[test]
    fn strings_quote_for_gsettings() {
        assert_eq!(string_text("Hex"), "'Hex'");
        assert_eq!(string_text("it's \\ ok"), "'it\\'s \\\\ ok'");
    }
}
