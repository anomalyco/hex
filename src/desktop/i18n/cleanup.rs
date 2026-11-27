//! Labs › Clean up what I say's strings in the languages whose tables live
//! beside the main one: Hex's tagger taking ums, repeats, and spoken
//! corrections out of English dictations.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Clean up what I say", "Nettoyer ce que je dis"),
    (
        "Remove ums, repeats, and things you correct out loud from English dictations, with a small model on this Mac that never adds a word",
        "Retire des dictées en anglais les « um », les répétitions et ce que vous corrigez à voix haute, avec un petit modèle sur ce Mac qui n'ajoute jamais un mot",
    ),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Clean up what I say", "Limpar o que eu digo"),
    (
        "Remove ums, repeats, and things you correct out loud from English dictations, with a small model on this Mac that never adds a word",
        "Remove dos ditados em inglês os “um”, as repetições e o que você corrige em voz alta, com um modelo pequeno neste Mac que nunca acrescenta uma palavra",
    ),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Clean up what I say", "Очищать сказанное"),
    (
        "Remove ums, repeats, and things you correct out loud from English dictations, with a small model on this Mac that never adds a word",
        "Убирает из диктовок на английском «um», повторы и то, что вы поправляете вслух, с помощью небольшой модели на этом Mac, которая никогда не добавляет слов",
    ),
];

/// Hungarian, for the strings above.
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Clean up what I say", "A kimondottak megtisztítása"),
    (
        "Remove ums, repeats, and things you correct out loud from English dictations, with a small model on this Mac that never adds a word",
        "Eltávolítja az angol diktálásokból az „um”-okat, az ismétléseket és a hangosan kimondott javításokat egy kis modellel ezen a Macen, amely soha nem ad hozzá szót",
    ),
];
