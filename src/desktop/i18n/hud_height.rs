//! Labs › HUD height's strings in the languages whose tables live beside
//! the main one: how high the dictation HUD sits above the Dock.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("HUD height", "Hauteur du HUD"),
    (
        "Choose how high the dictation HUD sits above the Dock when it's at the bottom",
        "Choisissez à quelle hauteur le HUD de dictée se place au-dessus du Dock lorsqu'il est en bas",
    ),
    ("Height", "Hauteur"),
    ("{} pt", "{} pt"),
    ("Lower", "Descendre"),
    ("Raise", "Monter"),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("HUD height", "Altura do HUD"),
    (
        "Choose how high the dictation HUD sits above the Dock when it's at the bottom",
        "Escolha a que altura o HUD de ditado fica acima do Dock quando está embaixo",
    ),
    ("Height", "Altura"),
    ("{} pt", "{} pt"),
    ("Lower", "Baixar"),
    ("Raise", "Subir"),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("HUD height", "Высота HUD"),
    (
        "Choose how high the dictation HUD sits above the Dock when it's at the bottom",
        "Выберите, насколько высоко HUD диктовки стоит над Dock, когда он внизу",
    ),
    ("Height", "Высота"),
    ("{} pt", "{} пт"),
    ("Lower", "Ниже"),
    ("Raise", "Выше"),
];
