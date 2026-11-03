//! The languages Hex names, in the order Settings offers them.

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
    ("az", "Azerbaijani"),
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
    ("km", "Khmer"),
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
