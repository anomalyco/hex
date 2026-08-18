pub(crate) fn normalize(text: &str) -> String {
    text.split_whitespace()
        .filter_map(|word| {
            let word = word.trim_matches(|character: char| character.is_ascii_punctuation());
            if word.is_empty() {
                return None;
            }
            Some(match word.to_ascii_lowercase().as_str() {
                "zero" => "0".to_string(),
                "one" => "1".to_string(),
                "two" => "2".to_string(),
                "three" => "3".to_string(),
                "four" => "4".to_string(),
                "five" => "5".to_string(),
                "six" => "6".to_string(),
                "seven" => "7".to_string(),
                "eight" => "8".to_string(),
                "nine" => "9".to_string(),
                _ => word.to_ascii_lowercase(),
            })
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_spoken_numbers_and_punctuation() {
        assert_eq!(normalize("Move LEFT, three!"), "move left 3");
    }
}
