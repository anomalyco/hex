//! Labs › HUD height's and Labs › Indicator follows mouse's strings in the
//! languages whose tables live beside the main one: where the dictation HUD
//! sits, above the Dock or beside the pointer.
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
    ("Indicator follows mouse", "L'indicateur suit la souris"),
    (
        "Show the dictation indicator next to your mouse pointer instead of at the top or bottom of the screen",
        "Affichez l'indicateur de dictée à côté du pointeur de la souris au lieu du haut ou du bas de l'écran",
    ),
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
    ("Indicator follows mouse", "Indicador segue o mouse"),
    (
        "Show the dictation indicator next to your mouse pointer instead of at the top or bottom of the screen",
        "Mostre o indicador de ditado ao lado do ponteiro do mouse em vez de no topo ou na parte de baixo da tela",
    ),
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
    ("Indicator follows mouse", "Индикатор следует за мышью"),
    (
        "Show the dictation indicator next to your mouse pointer instead of at the top or bottom of the screen",
        "Показывать индикатор диктовки рядом с указателем мыши, а не вверху или внизу экрана",
    ),
];

/// Hungarian, for the strings above.
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("HUD height", "Kijelző magassága"),
    (
        "Choose how high the dictation HUD sits above the Dock when it's at the bottom",
        "Válassza ki, milyen magasan legyen a diktálás kijelzője a Dock felett, amikor alul van",
    ),
    ("Height", "Magasság"),
    ("{} pt", "{} pt"),
    ("Lower", "Lejjebb"),
    ("Raise", "Feljebb"),
    ("Indicator follows mouse", "A jelző követi az egeret"),
    (
        "Show the dictation indicator next to your mouse pointer instead of at the top or bottom of the screen",
        "A diktálás jelzője az egérmutató mellett jelenjen meg a képernyő teteje vagy alja helyett",
    ),
];
