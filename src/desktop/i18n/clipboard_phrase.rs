//! Labs › Clipboard phrase's strings, kept beside the main tables so those
//! stay under the source cap. Not yet reviewed by native speakers, like their
//! neighbouring tables.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish,
/// Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Clipboard phrase", ["Fraza schowka", "剪贴板短语", "クリップボードのフレーズ", "Zwischenablage-Phrase", "Frase del portapapeles", "Фраза буфера обміну"]),
    (
        "Say \"my clipboard\" while dictating and Hex pastes what you copied there, in quotes",
        [
            "Powiedz „my clipboard” podczas dyktowania, a Hex wklei tam skopiowany tekst w cudzysłowie",
            "听写时说“my clipboard”，Hex 会在那里粘贴你复制的内容，并加上引号",
            "音声入力中に「my clipboard」と言うと、コピーした内容を引用符付きでそこに貼り付けます",
            "Sag beim Diktieren „my clipboard“, und Hex fügt dort das Kopierte in Anführungszeichen ein",
            "Di «my clipboard» al dictar y Hex pega ahí lo que copiaste, entre comillas",
            "Скажіть «my clipboard» під час диктування, і Hex вставить туди скопійоване в лапках",
        ],
    ),
];

#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Clipboard phrase", "Phrase du presse-papiers"),
    (
        "Say \"my clipboard\" while dictating and Hex pastes what you copied there, in quotes",
        "Dites « my clipboard » en dictant et Hex colle à cet endroit ce que vous avez copié, entre guillemets",
    ),
];

#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Clipboard phrase", "Frase da área de transferência"),
    (
        "Say \"my clipboard\" while dictating and Hex pastes what you copied there, in quotes",
        "Diga “my clipboard” ao ditar e o Hex cola ali o que você copiou, entre aspas",
    ),
];

#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Clipboard phrase", "Фраза буфера обмена"),
    (
        "Say \"my clipboard\" while dictating and Hex pastes what you copied there, in quotes",
        "Скажите «my clipboard» во время диктовки, и Hex вставит туда скопированное в кавычках",
    ),
];

/// Hungarian, for the strings above.
#[rustfmt::skip]
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Clipboard phrase", "Vágólap-kifejezés"),
    (
        "Say \"my clipboard\" while dictating and Hex pastes what you copied there, in quotes",
        "Mondja azt, hogy „my clipboard” diktálás közben, és a Hex idézőjelben oda illeszti, amit kimásolt",
    ),
];
