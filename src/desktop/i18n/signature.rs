//! Labs › Signature's strings, kept beside the main tables so those stay
//! under the source cap. Columns: Polish, Chinese (Simplified), Japanese,
//! German, Spanish, Ukrainian.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish,
/// Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Signature", ["Podpis", "签名", "署名", "Signatur", "Firma", "Підпис"]),
    (
        "Add a line to the end of each dictation, such as \"Sent from Hex\"",
        [
            "Dodawaj wiersz na końcu każdego dyktowania, np. „Sent from Hex”",
            "在每次听写末尾添加一行，例如“Sent from Hex”",
            "各音声入力の末尾に「Sent from Hex」などの一行を追加します",
            "Füge jeder Diktatzeile eine Zeile wie „Sent from Hex“ am Ende hinzu",
            "Añade una línea al final de cada dictado, como «Sent from Hex»",
            "Додавайте рядок у кінець кожного диктування, наприклад «Sent from Hex»",
        ],
    ),
    ("Use one line", ["Użyj jednego wiersza", "请使用一行", "1 行にしてください", "Verwende eine Zeile", "Usa una sola línea", "Використовуйте один рядок"]),
    (
        "Use 200 characters or fewer",
        [
            "Użyj maksymalnie 200 znaków",
            "请使用不超过 200 个字符",
            "200 文字以内にしてください",
            "Verwende höchstens 200 Zeichen",
            "Usa 200 caracteres o menos",
            "Використовуйте не більше 200 символів",
        ],
    ),
];

#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Signature", "Signature"),
    (
        "Add a line to the end of each dictation, such as \"Sent from Hex\"",
        "Ajoutez une ligne à la fin de chaque dictée, par exemple « Sent from Hex »",
    ),
    ("Use one line", "Utilisez une seule ligne"),
    ("Use 200 characters or fewer", "Utilisez 200 caractères ou moins"),
];

#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Signature", "Assinatura"),
    (
        "Add a line to the end of each dictation, such as \"Sent from Hex\"",
        "Adicione uma linha ao final de cada ditado, como “Sent from Hex”",
    ),
    ("Use one line", "Use uma linha só"),
    ("Use 200 characters or fewer", "Use no máximo 200 caracteres"),
];

#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Signature", "Подпись"),
    (
        "Add a line to the end of each dictation, such as \"Sent from Hex\"",
        "Добавляйте строку в конец каждого диктанта, например «Sent from Hex»",
    ),
    ("Use one line", "Используйте одну строку"),
    ("Use 200 characters or fewer", "Используйте не более 200 символов"),
];
