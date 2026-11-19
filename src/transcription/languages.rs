//! The languages Hex names. The catalog order is the order defaults,
//! validation, the CLI, and the API use; menus sort it by name.

use super::models::AUTO_LANGUAGE;

pub const LANGUAGES: &[(&str, &str)] = &[
    (AUTO_LANGUAGE, "Auto"),
    ("en", "English"),
    ("zh", "Mandarin Chinese"),
    ("yue", "Cantonese"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("es", "Spanish"),
    ("fr", "French"),
    ("de", "German"),
    ("pt", "Portuguese"),
    ("it", "Italian"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
    ("bn", "Bangla"),
    ("vi", "Vietnamese"),
    ("ru", "Russian"),
    ("uk", "Ukrainian"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("tr", "Turkish"),
    ("sv", "Swedish"),
    ("da", "Danish"),
    ("no", "Norwegian"),
    ("fi", "Finnish"),
    ("cs", "Czech"),
    ("el", "Greek"),
    ("ro", "Romanian"),
    ("hu", "Hungarian"),
    ("id", "Indonesian"),
    ("ms", "Malay"),
    ("th", "Thai"),
    ("fa", "Persian"),
    ("he", "Hebrew"),
    ("bg", "Bulgarian"),
    ("hr", "Croatian"),
    ("et", "Estonian"),
    ("lv", "Latvian"),
    ("lt", "Lithuanian"),
    ("mt", "Maltese"),
    ("sk", "Slovak"),
    ("sl", "Slovenian"),
    ("mk", "Macedonian"),
    ("fil", "Filipino"),
    ("km", "Khmer"),
    ("az", "Azerbaijani"),
];

/// Names for the other languages Whisper knows, which Settings lists only
/// for a model that declares them (an import), after [`LANGUAGES`].
const OTHER_LANGUAGES: &[(&str, &str)] = &[
    ("nn", "Norwegian Nynorsk"),
    ("ca", "Catalan"),
    ("ta", "Tamil"),
    ("ur", "Urdu"),
    ("la", "Latin"),
    ("mi", "Maori"),
    ("ml", "Malayalam"),
    ("cy", "Welsh"),
    ("te", "Telugu"),
    ("sr", "Serbian"),
    ("kn", "Kannada"),
    ("br", "Breton"),
    ("eu", "Basque"),
    ("is", "Icelandic"),
    ("hy", "Armenian"),
    ("ne", "Nepali"),
    ("mn", "Mongolian"),
    ("bs", "Bosnian"),
    ("kk", "Kazakh"),
    ("sq", "Albanian"),
    ("sw", "Swahili"),
    ("gl", "Galician"),
    ("mr", "Marathi"),
    ("pa", "Punjabi"),
    ("si", "Sinhala"),
    ("sn", "Shona"),
    ("yo", "Yoruba"),
    ("so", "Somali"),
    ("af", "Afrikaans"),
    ("oc", "Occitan"),
    ("ka", "Georgian"),
    ("be", "Belarusian"),
    ("tg", "Tajik"),
    ("sd", "Sindhi"),
    ("gu", "Gujarati"),
    ("am", "Amharic"),
    ("yi", "Yiddish"),
    ("lo", "Lao"),
    ("uz", "Uzbek"),
    ("fo", "Faroese"),
    ("ht", "Haitian Creole"),
    ("ps", "Pashto"),
    ("tk", "Turkmen"),
    ("sa", "Sanskrit"),
    ("lb", "Luxembourgish"),
    ("my", "Burmese"),
    ("bo", "Tibetan"),
    ("tl", "Tagalog"),
    ("mg", "Malagasy"),
    ("as", "Assamese"),
    ("tt", "Tatar"),
    ("haw", "Hawaiian"),
    ("ln", "Lingala"),
    ("ha", "Hausa"),
    ("ba", "Bashkir"),
    ("jw", "Javanese"),
    ("su", "Sundanese"),
];

/// Whether Settings lists `code` for every model that supports it.
pub(crate) fn is_listed(code: &str) -> bool {
    LANGUAGES.iter().any(|(candidate, _)| *candidate == code)
}

/// The language's English name, or its code when Hex has none.
pub fn language_name(code: &str) -> &str {
    LANGUAGES
        .iter()
        .chain(OTHER_LANGUAGES)
        .find_map(|(candidate, name)| (*candidate == code).then_some(*name))
        .unwrap_or(code)
}

/// `codes` in the order a menu offers them: Auto first when the model
/// detects the language, then every language by the name it shows.
pub fn menu_order(codes: &[&'static str]) -> Vec<&'static str> {
    let mut ordered = codes.to_vec();
    ordered.sort_by_cached_key(|code| (*code != AUTO_LANGUAGE, language_name(code).to_lowercase()));
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_order_puts_auto_first_then_names_alphabetically() {
        let ordered = menu_order(&["en", "zh", "mt", "ms", AUTO_LANGUAGE, "yue"]);
        assert_eq!(ordered, [AUTO_LANGUAGE, "yue", "en", "ms", "mt", "zh"]);
    }

    #[test]
    fn menu_order_sorts_the_whole_catalog_by_name() {
        let codes: Vec<&'static str> = LANGUAGES.iter().map(|(code, _)| *code).collect();
        let ordered = menu_order(&codes);
        assert_eq!(ordered.len(), codes.len());
        assert_eq!(ordered[0], AUTO_LANGUAGE, "Auto stays first");
        assert_eq!(ordered[1], "ar", "Arabic is first by name, not English");
        let malay = ordered.iter().position(|code| *code == "ms").unwrap();
        let maltese = ordered.iter().position(|code| *code == "mt").unwrap();
        let mandarin = ordered.iter().position(|code| *code == "zh").unwrap();
        assert!(malay < maltese && maltese < mandarin);
    }

    #[test]
    fn menu_order_leaves_out_auto_when_the_model_takes_no_hint() {
        let ordered = menu_order(&["ru", "en"]);
        assert_eq!(ordered, ["en", "ru"]);
    }
}
