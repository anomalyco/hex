//! Labs › Voice isolation's strings in the languages whose tables live
//! beside the main one: dictation through Apple's voice processing.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Voice isolation", "Isolement de la voix"),
    (
        "Record dictation through Apple's voice processing so Mic Mode can turn on Voice Isolation and cut background noise.",
        "Dictez via le traitement vocal d’Apple pour que le mode Micro puisse activer l’isolement de la voix et réduire le bruit de fond.",
    ),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Voice isolation", "Isolamento de voz"),
    (
        "Record dictation through Apple's voice processing so Mic Mode can turn on Voice Isolation and cut background noise.",
        "Grave o ditado pelo processamento de voz da Apple para que o Modo Microfone possa ativar o Isolamento de Voz e reduzir o ruído de fundo.",
    ),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Voice isolation", "Изоляция голоса"),
    (
        "Record dictation through Apple's voice processing so Mic Mode can turn on Voice Isolation and cut background noise.",
        "Записывайте диктовку через обработку голоса Apple, чтобы режим микрофона мог включить «Изоляцию голоса» и уменьшить фоновый шум.",
    ),
];

#[rustfmt::skip]
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Voice isolation", "Hangszigetelés"),
    (
        "Record dictation through Apple's voice processing so Mic Mode can turn on Voice Isolation and cut background noise.",
        "Diktálj az Apple hangfeldolgozásán keresztül, hogy a Mikrofon mód bekapcsolhassa a Hangszigetelést és csökkenthesse a háttérzajt.",
    ),
];
