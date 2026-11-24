//! Labs › Dictate while annotating's strings, kept beside the main tables
//! so those stay under the source cap.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish,
/// Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Dictate while annotating", ["Dyktuj podczas adnotacji", "标注时听写", "注釈中の音声入力", "Diktieren beim Annotieren", "Dictar mientras anotas", "Диктувати під час анотації"]),
    (
        "Dictate while a capture records: the text pastes as usual, and the capture keeps going",
        [
            "Dyktuj, gdy trwa nagranie: tekst wklei się jak zwykle, a nagranie trwa dalej",
            "在录制期间听写：文字照常粘贴，录制继续进行",
            "キャプチャの収録中にも音声入力できます。テキストはいつもどおり貼り付き、収録は続きます",
            "Diktiere, während eine Aufnahme läuft: Der Text wird wie gewohnt eingefügt, und die Aufnahme läuft weiter",
            "Dicta mientras hay una captura: el texto se pega como siempre y la captura sigue",
            "Диктуйте, коли триває запис: текст вставиться як завжди, а запис триватиме",
        ],
    ),
];

#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Dictate while annotating", "Dicter pendant une annotation"),
    (
        "Dictate while a capture records: the text pastes as usual, and the capture keeps going",
        "Dictez pendant qu’une capture s’enregistre : le texte se colle comme d’habitude et la capture continue",
    ),
];

#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Dictate while annotating", "Ditar enquanto anota"),
    (
        "Dictate while a capture records: the text pastes as usual, and the capture keeps going",
        "Dite enquanto uma captura grava: o texto é colado como sempre e a captura continua",
    ),
];

#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Dictate while annotating", "Диктовка во время аннотации"),
    (
        "Dictate while a capture records: the text pastes as usual, and the capture keeps going",
        "Диктуйте, пока идёт запись: текст вставится как обычно, а запись продолжится",
    ),
];

#[rustfmt::skip]
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Dictate while annotating", "Diktálás Annotate közben"),
    (
        "Dictate while a capture records: the text pastes as usual, and the capture keeps going",
        "Diktálj, miközben egy rögzítés készül: a szöveg a szokásos módon beillesztődik, a rögzítés pedig folytatódik",
    ),
];
