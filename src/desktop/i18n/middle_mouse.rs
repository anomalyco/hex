//! Labs › Middle mouse dictation's strings: the card that turns the lab on.
//! Kept beside the main table so it stays under the source cap, like
//! `meeting_mute`.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Middle mouse dictation", ["Dyktowanie środkowym przyciskiem myszy", "鼠标中键听写", "マウス中ボタンで音声入力", "Diktat mit der mittleren Maustaste", "Dictado con el botón central del ratón", "Диктування середньою кнопкою миші"]),
    ("Hold the middle mouse button for half a second to start dictating hands-free, and again to finish. Escape cancels", ["Przytrzymaj środkowy przycisk myszy przez pół sekundy, aby zacząć dyktować bez trzymania, i ponownie, aby zakończyć. Escape anuluje", "按住鼠标中键半秒即可开始免手持听写，再按住一次即可结束。按 Escape 取消", "マウスの中ボタンを0.5秒押し続けるとハンズフリーで音声入力を開始し、もう一度押し続けると終了します。Escape で取り消します", "Halte die mittlere Maustaste eine halbe Sekunde gedrückt, um freihändig zu diktieren, und noch einmal, um fertig zu werden. Escape bricht ab", "Mantén pulsado el botón central del ratón medio segundo para empezar a dictar sin manos, y otra vez para terminar. Escape cancela", "Утримуйте середню кнопку миші пів секунди, щоб почати диктувати без рук, і ще раз, щоб завершити. Escape скасовує"]),
];

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Middle mouse dictation", "Dictée avec le bouton central"),
    (
        "Hold the middle mouse button for half a second to start dictating hands-free, and again to finish. Escape cancels",
        "Maintenez le bouton central de la souris une demi-seconde pour dicter sans les mains, puis à nouveau pour terminer. Échap annule",
    ),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Middle mouse dictation", "Ditado com o botão do meio"),
    (
        "Hold the middle mouse button for half a second to start dictating hands-free, and again to finish. Escape cancels",
        "Segure o botão do meio do mouse por meio segundo para ditar sem as mãos, e de novo para terminar. Esc cancela",
    ),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Middle mouse dictation", "Диктовка средней кнопкой мыши"),
    (
        "Hold the middle mouse button for half a second to start dictating hands-free, and again to finish. Escape cancels",
        "Удерживайте среднюю кнопку мыши полсекунды, чтобы начать диктовку без рук, и ещё раз, чтобы закончить. Escape отменяет",
    ),
];

pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Middle mouse dictation", "Diktálás középső egérgombbal"),
    (
        "Hold the middle mouse button for half a second to start dictating hands-free, and again to finish. Escape cancels",
        "Tartsa lenyomva a középső egérgombot fél másodpercig a kéz nélküli diktáláshoz, majd újra a befejezéshez. Az Escape megszakítja",
    ),
];
