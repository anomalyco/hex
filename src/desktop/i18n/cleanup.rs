//! Labs › Clean up what I say's strings in the languages whose tables live
//! beside the main one: Hex's tagger taking ums, repeats, and spoken
//! corrections out of English dictations.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Clean up what I say", "Nettoyer ce que je dis"),
    ("Clean up what I say v2", "Nettoyer ce que je dis v2"),
    (
        "Test the next cleanup model on English dictations, which also writes spoken numbers, emails, and links as text, from a model folder on this Mac",
        "Testez le prochain modèle de nettoyage sur les dictées en anglais, qui écrit aussi en texte les nombres, e-mails et liens prononcés, depuis un dossier de modèle sur ce Mac",
    ),
    (
        "Remove ums, repeats, and things you correct out loud from English dictations, with a small model on this Mac that never adds a word",
        "Retire des dictées en anglais les « um », les répétitions et ce que vous corrigez à voix haute, avec un petit modèle sur ce Mac qui n'ajoute jamais un mot",
    ),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Clean up what I say", "Limpar o que eu digo"),
    ("Clean up what I say v2", "Limpar o que eu digo v2"),
    (
        "Test the next cleanup model on English dictations, which also writes spoken numbers, emails, and links as text, from a model folder on this Mac",
        "Teste o próximo modelo de limpeza nos ditados em inglês, que também escreve como texto os números, e-mails e links falados, a partir de uma pasta de modelo neste Mac",
    ),
    (
        "Remove ums, repeats, and things you correct out loud from English dictations, with a small model on this Mac that never adds a word",
        "Remove dos ditados em inglês os “um”, as repetições e o que você corrige em voz alta, com um modelo pequeno neste Mac que nunca acrescenta uma palavra",
    ),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Clean up what I say", "Очищать сказанное"),
    ("Clean up what I say v2", "Очищать сказанное v2"),
    (
        "Test the next cleanup model on English dictations, which also writes spoken numbers, emails, and links as text, from a model folder on this Mac",
        "Проверяйте следующую модель очистки на диктовках на английском: она также записывает произнесённые числа, адреса e-mail и ссылки текстом, из папки модели на этом Mac",
    ),
    (
        "Remove ums, repeats, and things you correct out loud from English dictations, with a small model on this Mac that never adds a word",
        "Убирает из диктовок на английском «um», повторы и то, что вы поправляете вслух, с помощью небольшой модели на этом Mac, которая никогда не добавляет слов",
    ),
];

/// Hungarian, for the strings above.
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Clean up what I say", "A kimondottak megtisztítása"),
    ("Clean up what I say v2", "A kimondottak megtisztítása v2"),
    (
        "Test the next cleanup model on English dictations, which also writes spoken numbers, emails, and links as text, from a model folder on this Mac",
        "Az angol diktálásokon kipróbálhatod a következő tisztító modellt, amely a kimondott számokat, e-mail-címeket és linkeket is szövegként írja, egy modellmappából ezen a Macen",
    ),
    (
        "Remove ums, repeats, and things you correct out loud from English dictations, with a small model on this Mac that never adds a word",
        "Eltávolítja az angol diktálásokból az „um”-okat, az ismétléseket és a hangosan kimondott javításokat egy kis modellel ezen a Macen, amely soha nem ad hozzá szót",
    ),
];
