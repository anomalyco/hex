//! Labs › Model switch shortcut's French, Portuguese, Russian, and Hungarian
//! strings, kept beside the language tables so those stay under the source
//! cap.

#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Model switch shortcut", "Raccourci de changement de modèle"),
    ("Swaps dictation to the other model, and back", "Bascule la dictée sur l'autre modèle, et inversement"),
    ("Other model", "Autre modèle"),
    ("The model and language the shortcut switches to", "Le modèle et la langue vers lesquels le raccourci bascule"),
    ("One shortcut swaps dictation to another model and language, and pressing it again swaps back", "Un raccourci bascule la dictée vers un autre modèle et une autre langue, et appuyer à nouveau revient en arrière"),
    ("Couldn't switch the speech model: {}", "Impossible de changer de modèle de parole : {}"),
    ("Couldn't switch the speech model: {} isn't installed", "Impossible de changer de modèle de parole : {} n'est pas installé"),
];

#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Model switch shortcut", "Atalho para trocar de modelo"),
    ("Swaps dictation to the other model, and back", "Troca o ditado para o outro modelo, e volta"),
    ("Other model", "Outro modelo"),
    ("The model and language the shortcut switches to", "O modelo e o idioma para os quais o atalho troca"),
    ("One shortcut swaps dictation to another model and language, and pressing it again swaps back", "Um atalho troca o ditado para outro modelo e idioma, e pressionar de novo volta"),
    ("Couldn't switch the speech model: {}", "Não foi possível trocar o modelo de fala: {}"),
    ("Couldn't switch the speech model: {} isn't installed", "Não foi possível trocar o modelo de fala: {} não está instalado"),
];

#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Model switch shortcut", "Сочетание для смены модели"),
    ("Swaps dictation to the other model, and back", "Переключает диктовку на другую модель и обратно"),
    ("Other model", "Другая модель"),
    ("The model and language the shortcut switches to", "Модель и язык, на которые переключает сочетание"),
    ("One shortcut swaps dictation to another model and language, and pressing it again swaps back", "Одно сочетание переключает диктовку на другую модель и язык, а повторное нажатие возвращает назад"),
    ("Couldn't switch the speech model: {}", "Не удалось переключить модель распознавания речи: {}"),
    ("Couldn't switch the speech model: {} isn't installed", "Не удалось переключить модель распознавания речи: {} не установлена"),
];

#[rustfmt::skip]
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Model switch shortcut", "Modellváltó billentyűparancs"),
    ("Swaps dictation to the other model, and back", "Átváltja a diktálást a másik modellre, és vissza"),
    ("Other model", "Másik modell"),
    ("The model and language the shortcut switches to", "A modell és a nyelv, amire a billentyűparancs vált"),
    ("One shortcut swaps dictation to another model and language, and pressing it again swaps back", "Egy billentyűparancs másik modellre és nyelvre váltja a diktálást, újabb megnyomásra pedig visszavált"),
    ("Couldn't switch the speech model: {}", "Nem sikerült átváltani a beszédfelismerő modellt: {}"),
    ("Couldn't switch the speech model: {} isn't installed", "Nem sikerült átváltani a beszédfelismerő modellt: {} nincs telepítve"),
];
