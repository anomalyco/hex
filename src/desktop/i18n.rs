//! UI translations for the port shells, keyed by the English source
//! string. The language follows the system locale unless a persisted
//! interface-language choice overrides it; unknown locales and
//! untranslated strings fall back to English. Composite strings are `{}`
//! templates filled through [`tr_fill`].

use std::collections::HashMap;
use std::sync::OnceLock;

#[cfg(not(test))]
use std::sync::atomic::{AtomicU8, Ordering};
use strings::TRANSLATIONS;

mod agents;
mod calls;
mod cleanup;
mod clipboard_phrase;
mod clock;
mod confirm_cancel;
mod failures;
mod french;
mod history;
mod hud_height;
mod hungarian;
mod labs;
mod meeting_audio_source;
mod meeting_mute;
mod meetings;
mod menu;
mod middle_mouse;
mod model_switch;
pub(crate) mod numbers;
mod onboarding;
mod portuguese;
mod routing;
mod russian;
mod screenshots;
mod signature;
mod speaker_names;
mod strings;
mod style;
mod text_language;
mod traditional_chinese;
mod transcript_edits;
mod voice_isolation;

pub(crate) use clock::{clock_time, twelve_hour_clock};

/// A persisted interface-language code (`None` follows the system) and its
/// native name.
pub(crate) type LanguageChoice = (Option<&'static str>, &'static str);

/// The selectable interface languages: persisted code and native name. The
/// leading `None` entry follows the system display language. Português
/// (Brasil), Français, 繁體中文, Русский, and Magyar have not had a native
/// speaker's review yet.
pub(crate) const LANGUAGE_CHOICES: &[LanguageChoice] = &[
    (None, "System"),
    (Some("en"), "English"),
    (Some("pl"), "Polski"),
    (Some("zh"), "简体中文"),
    (Some("zh-Hant"), "繁體中文"),
    (Some("ja"), "日本語"),
    (Some("de"), "Deutsch"),
    (Some("es"), "Español"),
    (Some("uk"), "Українська"),
    (Some("pt"), "Português (Brasil)"),
    (Some("fr"), "Français"),
    (Some("ru"), "Русский"),
    (Some("hu"), "Magyar"),
];

/// [`LANGUAGE_CHOICES`] in the order the select offers them: System first,
/// then every language by the name it displays.
pub(crate) fn language_menu_order() -> Vec<LanguageChoice> {
    let mut ordered = LANGUAGE_CHOICES.to_vec();
    ordered.sort_by_cached_key(|(code, name)| (code.is_some(), tr(name).to_lowercase()));
    ordered
}

/// Active language column into [`TRANSLATIONS`]; 255 means English.
#[cfg(not(test))]
static CURRENT: AtomicU8 = AtomicU8::new(255);

// Tests render scenes in different languages concurrently, so each test
// thread has its own language.
#[cfg(test)]
thread_local! {
    static CURRENT: std::cell::Cell<u8> = const { std::cell::Cell::new(ENGLISH) };
}

#[cfg(not(test))]
fn current() -> u8 {
    CURRENT.load(Ordering::Relaxed)
}

#[cfg(test)]
fn current() -> u8 {
    CURRENT.get()
}

#[cfg(not(test))]
fn set_current(code: u8) {
    CURRENT.store(code, Ordering::Relaxed);
}

#[cfg(test)]
fn set_current(code: u8) {
    CURRENT.set(code);
}

const ENGLISH: u8 = 255;
/// Brazilian Portuguese, kept in its own table ([`portuguese::TRANSLATIONS`]).
const PORTUGUESE_COLUMN: u8 = 6;
/// French, kept in its own table ([`french::TRANSLATIONS`]).
const FRENCH_COLUMN: u8 = 7;
/// Traditional Chinese, drawn from the Simplified column
/// ([`traditional_chinese::table`]).
const TRADITIONAL_CHINESE_COLUMN: u8 = 8;
/// Russian, kept in its own table ([`russian::TRANSLATIONS`]).
const RUSSIAN_COLUMN: u8 = 9;
/// Hungarian, kept in its own table ([`hungarian::TRANSLATIONS`]).
const HUNGARIAN_COLUMN: u8 = 10;

/// The column for a persisted code; `pt` (any Portuguese system language)
/// is Brazilian Portuguese, `fr` any French one, `ru` any Russian one, and
/// `hu` any Hungarian one.
fn column_for_code(code: &str) -> u8 {
    match code {
        "pl" => 0,
        "zh" => 1,
        "zh-Hant" => TRADITIONAL_CHINESE_COLUMN,
        "ja" => 2,
        "de" => 3,
        "es" => 4,
        "uk" => 5,
        "pt" => PORTUGUESE_COLUMN,
        "fr" => FRENCH_COLUMN,
        "ru" => RUSSIAN_COLUMN,
        "hu" => HUNGARIAN_COLUMN,
        _ => ENGLISH,
    }
}

/// Resolve and activate the persisted language choice; `None` follows the
/// system locale. Safe to call again whenever the setting changes.
pub(crate) fn apply(setting: Option<&str>) {
    let locale = preferred_locale();
    let code = match setting {
        Some(code) => column_for_code(code),
        None => column_for_language_tag(&locale),
    };
    set_current(code);
    // Tests keep a language per thread; Rune's is the process's.
    #[cfg(not(test))]
    rune::text::set_language(text_language::for_interface(code, &locale));
}

/// The column for a system locale such as `zh-Hans-CN`, `zh_TW.UTF-8`, or
/// `pl`; unknown or empty tags stay English. Chinese with the Hant script,
/// or the Taiwan, Hong Kong, or Macau region, is Traditional Chinese.
fn column_for_language_tag(tag: &str) -> u8 {
    // POSIX locales carry an encoding and modifier: `zh_TW.UTF-8@euro`.
    let tag = tag.split(['.', '@']).next().unwrap_or_default();
    match primary_subtag(tag).as_deref() {
        Some("zh") if is_traditional_chinese(tag) => TRADITIONAL_CHINESE_COLUMN,
        Some(code) => column_for_code(code),
        None => ENGLISH,
    }
}

/// Whether a Chinese locale's script or region asks for Traditional
/// characters. An explicit script wins over the region (`zh-Hans-HK`).
fn is_traditional_chinese(tag: &str) -> bool {
    for subtag in tag.split(['-', '_']).skip(1) {
        match subtag.trim().to_ascii_lowercase().as_str() {
            "hant" | "tw" | "hk" | "mo" => return true,
            "hans" => return false,
            _ => {}
        }
    }
    false
}

pub(crate) fn system_language() -> String {
    #[cfg(target_os = "windows")]
    {
        windows_user_language()
    }
    #[cfg(not(target_os = "windows"))]
    {
        language_from_env(
            std::env::var("LC_ALL").ok().as_deref(),
            std::env::var("LC_MESSAGES").ok().as_deref(),
            std::env::var("LANG").ok().as_deref(),
        )
    }
}

/// The system locale with its script and region, for the interface
/// language: the Windows user locale or the POSIX locale variables.
fn system_locale() -> String {
    #[cfg(target_os = "windows")]
    {
        windows_user_locale()
    }
    #[cfg(not(target_os = "windows"))]
    {
        locale_from_env(
            std::env::var("LC_ALL").ok().as_deref(),
            std::env::var("LC_MESSAGES").ok().as_deref(),
            std::env::var("LANG").ok().as_deref(),
        )
    }
}

/// The person's first preferred locale in full (`zh-Hant-TW`), for the
/// interface language; on macOS the system's language list.
fn preferred_locale() -> String {
    #[cfg(target_os = "macos")]
    if let Some(locale) = macos_preferred_locale() {
        return locale;
    }
    system_locale()
}

/// The person's first preferred language, for choosing a speech model. On
/// macOS this is the system's language list, which apps opened from Finder
/// see without `LANG`.
pub(crate) fn preferred_language() -> String {
    #[cfg(target_os = "macos")]
    if let Some(language) = macos_preferred_language() {
        return language;
    }
    system_language()
}

#[cfg(target_os = "macos")]
fn macos_preferred_language() -> Option<String> {
    primary_subtag(&macos_preferred_locale()?)
}

/// The first entry of `NSLocale.preferredLanguages`, such as `zh-Hant-TW`.
#[cfg(target_os = "macos")]
fn macos_preferred_locale() -> Option<String> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyClass;
    use objc2_foundation::{NSArray, NSString};

    let class = AnyClass::get(c"NSLocale")?;
    let languages: Option<Retained<NSArray<NSString>>> =
        unsafe { msg_send![class, preferredLanguages] };
    Some(languages?.firstObject()?.to_string())
}

/// `pt-BR` and `zh-Hans-CN` yield `pt` and `zh`.
fn primary_subtag(tag: &str) -> Option<String> {
    let subtag = tag.split(['-', '_']).next()?.trim().to_ascii_lowercase();
    (!subtag.is_empty()).then_some(subtag)
}

/// The primary subtag of the Windows user locale: `pl-PL` yields `pl`.
#[cfg(target_os = "windows")]
fn windows_user_language() -> String {
    windows_user_locale()
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase()
}

/// The Windows user locale name in full, such as `zh-TW`; `en` if unread.
#[cfg(target_os = "windows")]
fn windows_user_locale() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    const LOCALE_NAME_CAPACITY: usize = 85;
    let mut buffer = [0_u16; LOCALE_NAME_CAPACITY];
    let length = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 1 {
        return "en".into();
    }
    String::from_utf16_lossy(&buffer[..length as usize - 1])
}

/// The primary language subtag from POSIX locale variables, in their
/// standard precedence: `pl_PL.UTF-8` yields `pl`.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn language_from_env(
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
) -> String {
    [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty() && *value != "C" && *value != "POSIX")
        .map(|value| {
            value
                .chars()
                .take_while(|character| character.is_ascii_alphabetic())
                .collect::<String>()
                .to_lowercase()
        })
        .unwrap_or_default()
}

/// The first set POSIX locale variable in full, in the same precedence as
/// [`language_from_env`]: `zh_TW.UTF-8` stays `zh_TW.UTF-8`.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn locale_from_env(lc_all: Option<&str>, lc_messages: Option<&str>, lang: Option<&str>) -> String {
    [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty() && *value != "C" && *value != "POSIX")
        .unwrap_or_default()
        .to_owned()
}

/// Translate one UI string. Returns the input unchanged for English or for
/// strings missing from the table, so dynamic values pass through safely.
pub(crate) fn tr(english: &str) -> &str {
    let column = current();
    if column == ENGLISH {
        return english;
    }
    if column == TRADITIONAL_CHINESE_COLUMN {
        return match traditional_chinese::table().get(english) {
            Some(text) => text.as_str(),
            None => english,
        };
    }
    let table = match column {
        PORTUGUESE_COLUMN => portuguese::table(),
        FRENCH_COLUMN => french::table(),
        RUSSIAN_COLUMN => russian::table(),
        HUNGARIAN_COLUMN => hungarian::table(),
        _ => &tables()[column as usize],
    };
    table.get(english).copied().unwrap_or(english)
}

/// A menu label in the interface language: macOS's title case in English
/// (`english_title`), else the window's sentence-case label translated, with
/// the title's trailing `…` kept.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn menu_label(english_title: &str, translated: &str) -> String {
    if current() == ENGLISH {
        return english_title.into();
    }
    match english_title.strip_suffix('…') {
        Some(_) if !translated.ends_with('…') => format!("{translated}…"),
        _ => translated.into(),
    }
}

/// Translate a `{}` template and substitute the dynamic value, keeping
/// language-specific word order intact.
pub(crate) fn tr_fill(template: &str, value: &str) -> String {
    tr(template).replace("{}", value)
}

/// Translate a template with named `{name}` placeholders and fill them, so
/// each language can order the values as its grammar needs.
pub(crate) fn tr_named(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(tr(template).to_string(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

/// A compact, translated age label for an event timestamp.
pub(crate) fn event_age(timestamp_ms: u64) -> String {
    event_age_at(timestamp_ms, crate::history::now_ms())
}

pub(crate) fn event_age_at(timestamp_ms: u64, now: u64) -> String {
    let seconds = now.saturating_sub(timestamp_ms) / 1_000;
    let (template, value) = match seconds {
        0..=59 => ("{}s ago", seconds),
        60..=3_599 => ("{}m ago", seconds / 60),
        3_600..=86_399 => ("{}h ago", seconds / 3_600),
        _ => ("{}d ago", seconds / 86_400),
    };
    tr_fill(template, &value.to_string())
}

fn tables() -> &'static [HashMap<&'static str, &'static str>; 6] {
    static TABLES: OnceLock<[HashMap<&'static str, &'static str>; 6]> = OnceLock::new();
    TABLES.get_or_init(|| {
        std::array::from_fn(|column| {
            all_translations()
                .map(|(english, translated)| (*english, translated[column]))
                .collect()
        })
    })
}

/// Every translated string: the main, Meetings, Labs, microphone mute,
/// Dictation style, failure, first-run, menu bar, History, Screenshots, and
/// Annotate Agents rows.
pub(super) fn all_translations() -> impl Iterator<Item = &'static (&'static str, [&'static str; 6])>
{
    TRANSLATIONS
        .iter()
        .chain(meetings::TRANSLATIONS)
        .chain(labs::TRANSLATIONS)
        .chain(meeting_audio_source::TRANSLATIONS)
        .chain(meeting_mute::TRANSLATIONS)
        .chain(confirm_cancel::TRANSLATIONS)
        .chain(middle_mouse::TRANSLATIONS)
        .chain(speaker_names::TRANSLATIONS)
        .chain(style::TRANSLATIONS)
        .chain(signature::TRANSLATIONS)
        .chain(clipboard_phrase::TRANSLATIONS)
        .chain(agents::TRANSLATIONS)
        .chain(failures::TRANSLATIONS)
        .chain(onboarding::TRANSLATIONS)
        .chain(menu::TRANSLATIONS)
        .chain(history::TRANSLATIONS)
        .chain(screenshots::TRANSLATIONS)
        .chain(routing::TRANSLATIONS)
}

#[cfg(test)]
mod tests {
    use super::all_translations;
    use super::{language_from_env, primary_subtag};

    #[test]
    fn system_language_tags_choose_their_interface_column() {
        use super::{
            ENGLISH, LANGUAGE_CHOICES, PORTUGUESE_COLUMN, column_for_code, column_for_language_tag,
        };
        assert_eq!(column_for_language_tag("zh-Hans-CN"), column_for_code("zh"));
        assert_eq!(column_for_language_tag("zh"), column_for_code("zh"));
        assert_eq!(column_for_language_tag("pl"), column_for_code("pl"));
        assert_eq!(column_for_language_tag("pt-BR"), PORTUGUESE_COLUMN);
        assert_eq!(column_for_language_tag("pt_PT"), PORTUGUESE_COLUMN);
        assert_eq!(column_for_code("pt"), PORTUGUESE_COLUMN);
        assert_eq!(column_for_language_tag(""), ENGLISH);
        assert!(LANGUAGE_CHOICES.contains(&(Some("pt"), "Português (Brasil)")));
        assert_eq!(column_for_language_tag("fr-FR"), super::FRENCH_COLUMN);
        assert_eq!(column_for_language_tag("fr_CA"), super::FRENCH_COLUMN);
        assert_eq!(column_for_code("fr"), super::FRENCH_COLUMN);
        assert!(LANGUAGE_CHOICES.contains(&(Some("fr"), "Français")));
        assert_eq!(column_for_language_tag("ru-RU"), super::RUSSIAN_COLUMN);
        assert_eq!(
            column_for_language_tag("ru_RU.UTF-8"),
            super::RUSSIAN_COLUMN
        );
        assert_eq!(column_for_code("ru"), super::RUSSIAN_COLUMN);
        assert!(LANGUAGE_CHOICES.contains(&(Some("ru"), "Русский")));
        assert_eq!(column_for_language_tag("hu-HU"), super::HUNGARIAN_COLUMN);
        assert_eq!(
            column_for_language_tag("hu_HU.UTF-8"),
            super::HUNGARIAN_COLUMN
        );
        assert_eq!(column_for_code("hu"), super::HUNGARIAN_COLUMN);
        assert!(LANGUAGE_CHOICES.contains(&(Some("hu"), "Magyar")));
    }

    #[test]
    fn the_interface_language_menu_is_system_first_then_alphabetical() {
        use super::{LANGUAGE_CHOICES, language_menu_order, tr};
        let ordered = language_menu_order();
        assert_eq!(ordered[0], (None, "System"), "System stays first");
        let labels: Vec<&str> = ordered[1..].iter().map(|(_, name)| tr(name)).collect();
        assert!(
            labels
                .windows(2)
                .all(|pair| pair[0].to_lowercase() <= pair[1].to_lowercase()),
            "{labels:?}"
        );
        let mut codes: Vec<Option<&str>> = ordered.iter().map(|(code, _)| *code).collect();
        codes.sort();
        let mut wanted: Vec<Option<&str>> =
            LANGUAGE_CHOICES.iter().map(|(code, _)| *code).collect();
        wanted.sort();
        assert_eq!(codes, wanted, "every language is still offered once");
    }

    #[test]
    fn traditional_chinese_locales_choose_traditional_and_others_simplified() {
        use super::{
            LANGUAGE_CHOICES, TRADITIONAL_CHINESE_COLUMN, column_for_code, column_for_language_tag,
            locale_from_env,
        };
        for tag in [
            "zh-Hant-TW",
            "zh-Hant",
            "zh-TW",
            "zh_HK.UTF-8",
            "zh-MO",
            "zh-Hant-CN",
        ] {
            assert_eq!(
                column_for_language_tag(tag),
                TRADITIONAL_CHINESE_COLUMN,
                "{tag}"
            );
        }
        for tag in [
            "zh-Hans-CN",
            "zh-CN",
            "zh-SG",
            "zh-Hans-HK",
            "zh_CN.UTF-8",
            "zh",
        ] {
            assert_eq!(column_for_language_tag(tag), column_for_code("zh"), "{tag}");
        }
        assert_eq!(column_for_code("zh-Hant"), TRADITIONAL_CHINESE_COLUMN);
        assert!(LANGUAGE_CHOICES.contains(&(Some("zh"), "简体中文")));
        assert!(LANGUAGE_CHOICES.contains(&(Some("zh-Hant"), "繁體中文")));
        let locale = locale_from_env(None, Some("zh_TW.UTF-8"), Some("en_US.UTF-8"));
        assert_eq!(locale, "zh_TW.UTF-8");
        assert_eq!(column_for_language_tag(&locale), TRADITIONAL_CHINESE_COLUMN);
        assert_eq!(locale_from_env(Some("C"), None, None), "");
    }

    #[test]
    fn traditional_chinese_translates_with_its_column() {
        super::set_current(super::TRADITIONAL_CHINESE_COLUMN);
        assert_eq!(super::tr("Don't switch"), "不切換");
        assert_eq!(super::tr("Not installed"), "未安裝");
        assert_eq!(super::tr_fill("Remove {}?", "x"), "移除 x？");
        super::set_current(super::ENGLISH);
        assert_eq!(super::tr("Don't switch"), "Don't switch");
    }

    #[test]
    fn french_translates_with_its_column() {
        super::set_current(super::FRENCH_COLUMN);
        assert_eq!(super::tr("Don't switch"), "Ne pas changer");
        assert_eq!(super::tr_fill("Remove {}?", "x"), "Supprimer x ?");
        super::set_current(super::ENGLISH);
        assert_eq!(super::tr("Don't switch"), "Don't switch");
    }

    #[test]
    fn russian_translates_with_its_column() {
        super::set_current(super::RUSSIAN_COLUMN);
        assert_eq!(super::tr("Don't switch"), "Не переключать");
        assert_eq!(super::tr_fill("Remove {}?", "x"), "Удалить x?");
        super::set_current(super::ENGLISH);
        assert_eq!(super::tr("Don't switch"), "Don't switch");
    }

    #[test]
    fn hungarian_translates_with_its_column() {
        super::set_current(super::HUNGARIAN_COLUMN);
        assert_eq!(super::tr("Don't switch"), "Ne váltson");
        assert_eq!(super::tr_fill("Remove {}?", "x"), "Eltávolítja ezt: x?");
        super::set_current(super::ENGLISH);
        assert_eq!(super::tr("Don't switch"), "Don't switch");
    }

    #[test]
    fn portuguese_translates_with_its_column() {
        super::set_current(super::PORTUGUESE_COLUMN);
        assert_eq!(super::tr("Labs"), "Labs");
        assert_eq!(super::tr("Don't switch"), "Não trocar");
        assert_eq!(super::tr_fill("Remove {}?", "x"), "Remover x?");
        super::set_current(super::ENGLISH);
        assert_eq!(super::tr("Don't switch"), "Don't switch");
    }

    #[test]
    fn clock_times_are_twelve_hour_in_english_and_twenty_four_hour_elsewhere() {
        super::apply(Some("en"));
        assert_eq!(super::clock_time(13, 5), "1:05 PM");
        assert_eq!(super::clock_time(1, 5), "1:05 AM");
        assert_eq!(super::clock_time(0, 5), "12:05 AM");
        assert_eq!(super::clock_time(12, 0), "12:00 PM");
        assert_eq!(super::twelve_hour_clock(14, 30), "2:30 PM");
        super::apply(Some("de"));
        assert_eq!(super::clock_time(13, 5), "13:05");
        assert_eq!(super::clock_time(1, 5), "01:05");
        assert_eq!(super::clock_time(0, 5), "00:05");
        assert_eq!(super::clock_time(12, 0), "12:00");
        // Reading a title written in English works in any language.
        assert_eq!(super::twelve_hour_clock(14, 30), "2:30 PM");
        super::apply(Some("en"));
    }

    #[test]
    fn preferred_language_tags_reduce_to_their_primary_subtag() {
        assert_eq!(primary_subtag("pt-BR").as_deref(), Some("pt"));
        assert_eq!(primary_subtag("zh-Hans-CN").as_deref(), Some("zh"));
        assert_eq!(primary_subtag("en").as_deref(), Some("en"));
        assert_eq!(primary_subtag(""), None);
    }

    #[test]
    fn posix_locale_variables_resolve_in_precedence_order() {
        assert_eq!(language_from_env(Some("pl_PL.UTF-8"), None, None), "pl");
        assert_eq!(
            language_from_env(None, Some("de_DE"), Some("es_ES.UTF-8")),
            "de"
        );
        assert_eq!(language_from_env(None, None, Some("ja_JP.UTF-8")), "ja");
        assert_eq!(language_from_env(Some("C"), None, Some("pl_PL")), "pl");
        assert_eq!(language_from_env(None, None, Some("POSIX")), "");
        assert_eq!(language_from_env(None, None, None), "");
    }

    fn extra_language_tables() -> [(
        &'static str,
        &'static std::collections::HashMap<&'static str, &'static str>,
    ); 4] {
        [
            ("French", super::french::table()),
            ("Portuguese", super::portuguese::table()),
            ("Russian", super::russian::table()),
            ("Hungarian", super::hungarian::table()),
        ]
    }

    fn extra_language_entries() -> [(&'static str, Vec<(&'static str, &'static str)>); 4] {
        [
            ("French", super::french::entries().collect()),
            ("Portuguese", super::portuguese::entries().collect()),
            ("Russian", super::russian::entries().collect()),
            ("Hungarian", super::hungarian::entries().collect()),
        ]
    }

    /// Asserts that `english` has a non-empty translation in every supported
    /// non-English language: the 6-column table, French, Portuguese, Russian,
    /// Hungarian, and Traditional Chinese.
    fn assert_every_language(
        six_column: &std::collections::HashMap<&'static str, [&'static str; 6]>,
        english: &str,
    ) {
        let translations = six_column
            .get(english)
            .unwrap_or_else(|| panic!("{english:?} has no translations"));
        assert!(
            translations
                .iter()
                .all(|translation| !translation.is_empty()),
            "{english:?} is missing a language"
        );
        for (language, table) in extra_language_tables() {
            assert!(
                table.get(english).is_some_and(|text| !text.is_empty()),
                "{language} has no translation for {english:?}"
            );
        }
        assert!(
            super::traditional_chinese::table()
                .get(english)
                .is_some_and(|text| !text.is_empty()),
            "Traditional Chinese has no translation for {english:?}"
        );
    }

    #[test]
    fn templates_keep_their_placeholder_in_every_language() {
        for (english, translated) in all_translations() {
            if english.contains("{}") {
                for translation in translated {
                    assert!(
                        translation.contains("{}"),
                        "translation of {english:?} lost its placeholder: {translation:?}"
                    );
                }
            }
        }
        for (language, entries) in extra_language_entries() {
            for (english, translation) in entries {
                if english.contains("{}") {
                    assert!(
                        translation.contains("{}"),
                        "{language} translation of {english:?} lost its placeholder: {translation:?}"
                    );
                }
            }
        }
        for (english, translation) in super::traditional_chinese::table() {
            if english.contains("{}") {
                assert!(
                    translation.contains("{}"),
                    "Traditional Chinese translation of {english:?} lost its placeholder: {translation:?}"
                );
            }
        }
    }

    /// Every source that translates, on every OS.
    const SOURCES: &[&str] = &[
        include_str!("activity.rs"),
        include_str!("opencode_catalog.rs"),
        include_str!("../rune_ui.rs"),
        include_str!("../rune_ui/activity.rs"),
        include_str!("../rune_ui/drop_transcribe.rs"),
        include_str!("../rune_ui/explainer.rs"),
        include_str!("../rune_ui/fun.rs"),
        include_str!("../rune_ui/general.rs"),
        include_str!("../rune_ui/general/extra_shortcuts.rs"),
        include_str!("../rune_ui/general/page.rs"),
        include_str!("../rune_ui/find_popover.rs"),
        include_str!("../rune_ui/find_popover/catalog.rs"),
        include_str!("../rune_ui/history.rs"),
        include_str!("../rune_ui/history/heatmap.rs"),
        include_str!("../rune_ui/history/stats.rs"),
        include_str!("../rune_ui/history/selection.rs"),
        include_str!("../rune_ui/hud_lab.rs"),
        include_str!("../rune_ui/labs.rs"),
        include_str!("../rune_ui/labs/group.rs"),
        include_str!("../rune_ui/labs/clipboard_phrase.rs"),
        include_str!("../rune_ui/labs/hud_height.rs"),
        include_str!("../rune_ui/labs/screenshots.rs"),
        include_str!("../rune_ui/labs/signature.rs"),
        include_str!("../rune_ui/labs/store.rs"),
        include_str!("../rune_ui/labs/stop_phrase.rs"),
        include_str!("../rune_ui/labs/voice_prompt.rs"),
        include_str!("../rune_ui/linux_host.rs"),
        include_str!("../rune_ui/list_detail.rs"),
        include_str!("../rune_ui/lists.rs"),
        include_str!("../rune_ui/macos_host.rs"),
        include_str!("../rune_ui/meeting_capsules.rs"),
        include_str!("../rune_ui/meeting_capsules/call_ended.rs"),
        include_str!("../rune_ui/meetings.rs"),
        include_str!("../rune_ui/meetings/selection.rs"),
        include_str!("../rune_ui/meetings/setup.rs"),
        include_str!("../rune_ui/models.rs"),
        include_str!("../rune_ui/found.rs"),
        include_str!("../rune_ui/found_choice.rs"),
        include_str!("../rune_ui/model_details.rs"),
        include_str!("../rune_ui/model_import.rs"),
        include_str!("../rune_ui/modes.rs"),
        include_str!("../rune_ui/modes/neutral_voice.rs"),
        include_str!("../rune_ui/onboarding_kit/first_run.rs"),
        include_str!("../rune_ui/onboarding_kit/flows.rs"),
        include_str!("../rune_ui/onboarding_kit/game.rs"),
        include_str!("../rune_ui/onboarding_kit/game/certificate.rs"),
        include_str!("../rune_ui/onboarding_kit/landing.rs"),
        include_str!("../rune_ui/onboarding_kit/live.rs"),
        include_str!("../rune_ui/onboarding_kit/permissions.rs"),
        include_str!("../rune_ui/onboarding_kit/pill.rs"),
        include_str!("../rune_ui/onboarding_kit/stories.rs"),
        include_str!("../rune_ui/onboarding_kit/versus.rs"),
        include_str!("../rune_ui/output.rs"),
        include_str!("../rune_ui/screenshot_thumbnail.rs"),
        include_str!("../rune_ui/screenshot_thumbnail/files.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/screenshot_thumbnail/card.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/captures.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/screenshot_thumbnail/editor.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/screenshot_thumbnail/editor/layers.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/screenshot_thumbnail/editor/view.rs"),
        include_str!("../screenshots.rs"),
        include_str!("../rune_ui/picker.rs"),
        include_str!("../rune_ui/picker/compare.rs"),
        include_str!("../rune_ui/picker/facts.rs"),
        include_str!("../rune_ui/picker/list.rs"),
        include_str!("../rune_ui/platform_settings.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/scrawl.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/mod.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/flow.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/flow/step.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/list_view.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/flow/routes.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/flow/done.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/flow/press.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/flow/words.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/surface.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/undo_ring.rs"),
        #[cfg(hex_capture)]
        include_str!("../rune_ui/capture_capsule/view.rs"),
        include_str!("../rune_ui/settings.rs"),
        include_str!("../rune_ui/shortcut_mode.rs"),
        include_str!("../rune_ui/sound_sets.rs"),
        include_str!("../rune_ui/calls.rs"),
        include_str!("../rune_ui/meeting_capsules/offer.rs"),
        include_str!("../rune_ui/speech_model.rs"),
        include_str!("../rune_ui/vocabulary.rs"),
        include_str!("../rune_ui/windows_host.rs"),
        include_str!("tray_menu.rs"),
        include_str!("../updater/presentation.rs"),
        include_str!("../calendar_menu.rs"),
        include_str!("../macos_rune/status_item.rs"),
        include_str!("../rune_ui/history/unified.rs"),
        include_str!("../rune_ui/onboarding_kit/first_screen.rs"),
        include_str!("../rune_ui/onboarding_kit/login.rs"),
        include_str!("../rune_ui/onboarding_kit/versus/stage_fx.rs"),
        include_str!("../rune_ui/meetings_host.rs"),
        include_str!("../windows/app.rs"),
        include_str!("../windows/rune.rs"),
        include_str!("../rune_hud.rs"),
    ];

    /// The string literals passed to `tr`, `tr_fill`, and `tr_named`.
    fn translated_literals(source: &str) -> Vec<String> {
        let mut literals = Vec::new();
        for call in ["tr(", "tr_fill(", "tr_named("] {
            for (start, _) in source.match_indices(call) {
                let preceded_by_ident = source[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|character| character.is_alphanumeric() || character == '_');
                if preceded_by_ident {
                    continue;
                }
                let rest = source[start + call.len()..].trim_start();
                let Some(rest) = rest.strip_prefix('"') else {
                    continue;
                };
                let mut literal = String::new();
                let mut characters = rest.chars();
                while let Some(character) = characters.next() {
                    match character {
                        '"' => break,
                        '\\' => literal.extend(characters.next()),
                        other => literal.push(other),
                    }
                }
                literals.push(literal);
            }
        }
        literals
    }

    #[test]
    fn every_interface_string_has_every_language_table() {
        let keys: std::collections::HashSet<&str> =
            all_translations().map(|(english, _)| *english).collect();
        for (language, table) in extra_language_tables() {
            let missing: Vec<&str> = keys
                .iter()
                .copied()
                .filter(|english| table.get(english).is_none_or(|text| text.is_empty()))
                .collect();
            assert!(missing.is_empty(), "no {language} for {missing:?}");
            let stale: Vec<&str> = table
                .keys()
                .copied()
                .filter(|english| !keys.contains(english))
                .collect();
            assert!(stale.is_empty(), "stale {language} keys: {stale:?}");
        }
        let traditional = super::traditional_chinese::table();
        let missing: Vec<&str> = keys
            .iter()
            .copied()
            .filter(|english| traditional.get(english).is_none_or(String::is_empty))
            .collect();
        assert!(missing.is_empty(), "no Traditional Chinese for {missing:?}");
    }

    #[test]
    fn every_translated_string_has_every_language() {
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        let mut checked = 0;
        for source in SOURCES {
            for literal in translated_literals(source) {
                assert_every_language(&table, literal.as_str());
                checked += 1;
            }
        }
        assert!(checked > 300, "only {checked} translated strings found");
    }

    /// The refused-paste notice is looked up through a constant, not a `tr`
    /// literal at its call site, so its table entry is checked here.
    #[test]
    fn the_refused_paste_notice_has_every_language() {
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        assert_every_language(&table, crate::paste::POST_EVENT_REFUSED);
    }

    /// The blocked Windows paste notice is looked up through a constant too.
    #[test]
    fn the_blocked_windows_paste_notice_has_every_language() {
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        assert_every_language(&table, crate::paste::WINDOWS_PASTE_BLOCKED);
    }

    /// Labs › Confirm long cancels' notice is looked up through a constant
    /// too.
    #[test]
    fn the_confirm_cancel_notice_has_every_language() {
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        assert_every_language(&table, crate::dictation::cancel_confirm::NOTICE);
    }

    /// The sound picker translates each set's name and character, which are
    /// not literals at the call.
    #[test]
    fn every_sound_set_string_has_every_language() {
        use crate::feedback::SoundSet;
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        let strings = SoundSet::ALL
            .into_iter()
            .flat_map(|set| [set.name(), set.character()]);
        for english in strings {
            assert_every_language(&table, english);
        }
    }

    /// Each Labs card translates its feature's title and line, and each
    /// group header its title (`labs::Feature::title`, `summary`,
    /// `labs::Group::title`), which are not literals at the
    /// call. Playground keeps its English name.
    #[test]
    fn every_lab_card_string_has_every_language() {
        use crate::labs::Feature;
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        let strings = Feature::ALL
            .into_iter()
            .flat_map(|feature| [feature.title(), feature.summary()])
            .chain(crate::labs::Group::ALL.map(crate::labs::Group::title))
            .filter(|english| *english != "Playground");
        for english in strings {
            assert_every_language(&table, english);
        }
    }

    /// The special-keys lab names one OS's keys in its summary, so a given
    /// build's `Feature::summary` is only one of the two sentences. Both are
    /// shown to someone, and both need every language on every OS.
    #[test]
    fn both_special_key_summaries_have_every_language() {
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        for english in [
            "Use Menu, Pause, or Scroll Lock as a shortcut by itself, such as Caps Lock remapped to Menu",
            "Use Insert, Home, End, Page Up, or Page Down as a shortcut by itself",
        ] {
            assert_every_language(&table, english);
        }
    }

    /// The listener card translates the status each OS's host publishes,
    /// which is not a literal at the call.
    #[test]
    fn every_listener_status_has_every_language() {
        use crate::desktop::host::ListenerStatus;
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        for english in ListenerStatus::ALL.into_iter().map(ListenerStatus::name) {
            assert_every_language(&table, english);
        }
    }

    /// The Appearance lab's theme picker translates each choice's name, which
    /// is not a literal at the call.
    #[test]
    fn every_appearance_theme_string_has_every_language() {
        use crate::app_settings::AppearanceTheme;
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        for english in AppearanceTheme::ALL.into_iter().map(AppearanceTheme::name) {
            assert_every_language(&table, english);
        }
    }

    /// The Dictation style lab translates each choice's name and hint, which
    /// are not literals at the call.
    #[test]
    fn every_dictation_style_string_has_every_language() {
        use crate::app_settings::DictationStyle;
        let table: std::collections::HashMap<_, _> = all_translations().copied().collect();
        for english in DictationStyle::ALL
            .into_iter()
            .flat_map(|style| [style.name(), style.hint()])
        {
            assert_every_language(&table, english);
        }
    }

    #[test]
    fn named_placeholders_survive_translation() {
        let assert_named = |language: &str, english: &str, translation: &str| {
            let names = english
                .split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}'))
                .map(|(name, _)| name)
                .filter(|name| !name.is_empty());
            for name in names {
                assert!(
                    translation.contains(&format!("{{{name}}}")),
                    "{language} translation of {english:?} lost {{{name}}}: {translation:?}"
                );
            }
        };
        for (english, translated) in all_translations() {
            for translation in translated {
                assert_named("six-column", english, translation);
            }
        }
        for (language, entries) in extra_language_entries() {
            for (english, translation) in entries {
                assert_named(language, english, translation);
            }
        }
        for (english, translation) in super::traditional_chinese::table() {
            assert_named("Traditional Chinese", english, translation);
        }
    }

    #[test]
    fn keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (english, _) in all_translations() {
            assert!(
                seen.insert(english),
                "duplicate translation key {english:?}"
            );
        }
        for (language, entries) in extra_language_entries() {
            let mut seen = std::collections::HashSet::new();
            for (english, _) in entries {
                assert!(
                    seen.insert(english),
                    "duplicate {language} translation key {english:?}"
                );
            }
        }
    }
}
