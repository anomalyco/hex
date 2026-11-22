//! Labs › Dictate while annotating's strings, kept beside the main tables
//! so those stay under the source cap.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish,
/// Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Dictate while annotating", ["Dyktuj podczas adnotacji", "标注时听写", "注釈中の音声入力", "Diktieren beim Annotieren", "Dictar mientras anotas", "Диктувати під час анотації"]),
    (
        "Dictate while a capture records: hold the shortcut and the text pastes as usual, while the capture keeps going",
        [
            "Dyktuj, gdy trwa nagranie: przytrzymaj skrót, a tekst wklei się jak zwykle, a nagranie trwa dalej",
            "在录制期间听写：按住快捷键，文字照常粘贴，录制继续进行",
            "キャプチャの収録中にも音声入力できます。ショートカットを押している間、テキストはいつもどおり貼り付き、収録は続きます",
            "Diktiere, während eine Aufnahme läuft: Halte das Kürzel, der Text wird wie gewohnt eingefügt, und die Aufnahme läuft weiter",
            "Dicta mientras hay una captura: mantén el atajo y el texto se pega como siempre, mientras la captura sigue",
            "Диктуйте, коли триває запис: утримуйте комбінацію, і текст вставиться як завжди, а запис триватиме",
        ],
    ),
];

#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Dictate while annotating", "Dicter pendant une annotation"),
    (
        "Dictate while a capture records: hold the shortcut and the text pastes as usual, while the capture keeps going",
        "Dictez pendant qu’une capture s’enregistre : maintenez le raccourci et le texte se colle comme d’habitude, pendant que la capture continue",
    ),
];

#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Dictate while annotating", "Ditar enquanto anota"),
    (
        "Dictate while a capture records: hold the shortcut and the text pastes as usual, while the capture keeps going",
        "Dite enquanto uma captura grava: mantenha o atalho e o texto é colado como sempre, enquanto a captura continua",
    ),
];

#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Dictate while annotating", "Диктовка во время аннотации"),
    (
        "Dictate while a capture records: hold the shortcut and the text pastes as usual, while the capture keeps going",
        "Диктуйте, пока идёт запись: удерживайте сочетание, и текст вставится как обычно, а запись продолжится",
    ),
];
