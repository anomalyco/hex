//! The language Rune shapes text in (`rune::text::set_language`), which
//! picks the fonts Han characters fall back to and their locale forms.

use super::{TRADITIONAL_CHINESE_COLUMN, column_for_code, is_traditional_chinese, primary_subtag};

/// The language Rune shapes text in, which picks the fonts Han characters
/// fall back to (#1382): the interface language when it's Chinese or
/// Japanese, otherwise a Chinese, Japanese, or Korean system locale (Hex has
/// no Korean interface), so text in it keeps its own forms.
pub(super) fn for_interface(column: u8, locale: &str) -> Option<&'static str> {
    let tag = locale.split(['.', '@']).next().unwrap_or_default();
    let chinese = |traditional: bool| match traditional {
        true if hong_kong(tag) => "zh-HK",
        true => "zh-Hant",
        false => "zh-Hans",
    };
    if column == column_for_code("ja") {
        return Some("ja");
    }
    if column == column_for_code("zh") {
        return Some("zh-Hans");
    }
    if column == TRADITIONAL_CHINESE_COLUMN {
        return Some(chinese(true));
    }
    match primary_subtag(tag)?.as_str() {
        "ja" => Some("ja"),
        "ko" => Some("ko"),
        "zh" => Some(chinese(is_traditional_chinese(tag))),
        _ => None,
    }
}

/// Whether a Chinese locale is Hong Kong's or Macau's, which draw some
/// characters unlike Taiwan.
fn hong_kong(tag: &str) -> bool {
    tag.split(['-', '_'])
        .skip(1)
        .any(|subtag| matches!(subtag.trim().to_ascii_lowercase().as_str(), "hk" | "mo"))
}

#[cfg(test)]
mod tests {
    use super::super::ENGLISH;
    use super::*;

    /// Han text follows the interface language, else a CJK system locale.
    #[test]
    fn text_language_follows_the_interface_language() {
        assert_eq!(for_interface(column_for_code("ja"), "en-US"), Some("ja"));
        assert_eq!(
            for_interface(column_for_code("zh"), "ja-JP"),
            Some("zh-Hans")
        );
        assert_eq!(
            for_interface(TRADITIONAL_CHINESE_COLUMN, "zh-Hant-TW"),
            Some("zh-Hant")
        );
        assert_eq!(
            for_interface(TRADITIONAL_CHINESE_COLUMN, "zh_HK.UTF-8"),
            Some("zh-HK")
        );
        assert_eq!(for_interface(ENGLISH, "ko-KR"), Some("ko"));
        assert_eq!(for_interface(ENGLISH, "ja_JP.UTF-8"), Some("ja"));
        assert_eq!(for_interface(ENGLISH, "zh-Hant-TW"), Some("zh-Hant"));
        assert_eq!(for_interface(column_for_code("de"), "de-DE"), None);
    }
}
