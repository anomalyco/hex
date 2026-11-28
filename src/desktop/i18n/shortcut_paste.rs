//! Labs › Shortcut pastes a capture's strings: the card that turns the lab
//! on. Kept beside the main table so it stays under the source cap, like
//! `middle_mouse`.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Shortcut pastes a capture", ["Skrót wkleja zrzut", "快捷键粘贴捕获内容", "ショートカットでキャプチャをペースト", "Kurzbefehl fügt eine Aufnahme ein", "El atajo pega una captura", "Скорочення вставляє запис"]),
    ("Press the Annotate shortcut again to paste a finished capture when its capsule shows Paste", ["Naciśnij ponownie skrót Adnotacji, aby wkleić gotowy zrzut, gdy kapsuła pokazuje Wklej", "当胶囊显示“粘贴”时，再次按下标注快捷键即可粘贴完成的捕获内容", "カプセルに「ペースト」が表示されているときに注釈のショートカットをもう一度押すと、完了したキャプチャをペーストします", "Drücke den Annotieren-Kurzbefehl erneut, um eine fertige Aufnahme einzufügen, wenn ihre Kapsel Einfügen zeigt", "Pulsa de nuevo el atajo de Anotar para pegar una captura terminada cuando su cápsula muestre Pegar", "Натисніть скорочення Анотування ще раз, щоб вставити готовий запис, коли капсула показує Вставити"]),
];

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Shortcut pastes a capture", "Le raccourci colle une capture"),
    (
        "Press the Annotate shortcut again to paste a finished capture when its capsule shows Paste",
        "Appuyez de nouveau sur le raccourci d'annotation pour coller une capture terminée lorsque sa capsule affiche Coller",
    ),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Shortcut pastes a capture", "Atalho cola uma captura"),
    (
        "Press the Annotate shortcut again to paste a finished capture when its capsule shows Paste",
        "Pressione o atalho de Anotar de novo para colar uma captura pronta quando a cápsula mostrar Colar",
    ),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Shortcut pastes a capture", "Сочетание вставляет запись"),
    (
        "Press the Annotate shortcut again to paste a finished capture when its capsule shows Paste",
        "Нажмите сочетание аннотации ещё раз, чтобы вставить готовую запись, когда капсула показывает «Вставить»",
    ),
];

pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Shortcut pastes a capture", "A billentyűparancs beilleszti a rögzítést"),
    (
        "Press the Annotate shortcut again to paste a finished capture when its capsule shows Paste",
        "Nyomja meg újra a jegyzetelés billentyűparancsát egy kész rögzítés beillesztéséhez, amikor a kapszula a Beillesztés gombot mutatja",
    ),
];
