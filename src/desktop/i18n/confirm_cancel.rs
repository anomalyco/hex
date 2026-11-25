//! Labs › Confirm long cancels' strings: its card and the notice beside the
//! HUD while a cancel waits for its second press. Kept beside the main table
//! so it stays under the source cap.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Confirm long cancels", ["Potwierdzaj anulowanie długich dyktowań", "确认取消较长的听写", "長い音声入力のキャンセルを確認", "Abbrechen langer Diktate bestätigen", "Confirmar la cancelación de dictados largos", "Підтверджувати скасування довгих диктувань"]),
    ("After 10 seconds, cancelling a dictation takes two presses, so one slip keeps your words", ["Po 10 sekundach anulowanie dyktowania wymaga dwóch naciśnięć, więc jedna pomyłka zachowuje twoje słowa", "听写超过 10 秒后，取消需要按两次，误按一次不会丢失你的内容", "10 秒を超えた音声入力のキャンセルは 2 回押しが必要です。1 回の押し間違いでは言葉は消えません", "Nach 10 Sekunden braucht das Abbrechen eines Diktats zwei Drücke, sodass ein Versehen deine Worte behält", "Tras 10 segundos, cancelar un dictado requiere dos pulsaciones, así un descuido conserva tus palabras", "Після 10 секунд скасування диктування потребує двох натискань, тож одна помилка зберігає ваші слова"]),
    ("Press again to discard this dictation", ["Naciśnij ponownie, aby odrzucić to dyktowanie", "再按一次以丢弃这段听写", "もう一度押すとこの音声入力を破棄します", "Erneut drücken, um dieses Diktat zu verwerfen", "Pulsa de nuevo para descartar este dictado", "Натисніть ще раз, щоб відкинути це диктування"]),
];

pub(super) const FRENCH: &[(&str, &str)] = &[
    (
        "Confirm long cancels",
        "Confirmer l’annulation des longues dictées",
    ),
    (
        "After 10 seconds, cancelling a dictation takes two presses, so one slip keeps your words",
        "Après 10 secondes, annuler une dictée demande deux appuis : une erreur garde vos mots",
    ),
    (
        "Press again to discard this dictation",
        "Appuyez à nouveau pour supprimer cette dictée",
    ),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    (
        "Confirm long cancels",
        "Confirmar o cancelamento de ditados longos",
    ),
    (
        "After 10 seconds, cancelling a dictation takes two presses, so one slip keeps your words",
        "Depois de 10 segundos, cancelar um ditado exige dois toques, assim um deslize mantém suas palavras",
    ),
    (
        "Press again to discard this dictation",
        "Pressione de novo para descartar este ditado",
    ),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    (
        "Confirm long cancels",
        "Подтверждать отмену длинных диктовок",
    ),
    (
        "After 10 seconds, cancelling a dictation takes two presses, so one slip keeps your words",
        "После 10 секунд отмена диктовки требует двух нажатий, так что одна ошибка сохранит ваши слова",
    ),
    (
        "Press again to discard this dictation",
        "Нажмите ещё раз, чтобы удалить эту диктовку",
    ),
];

pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    (
        "Confirm long cancels",
        "Hosszú diktálások megszakításának megerősítése",
    ),
    (
        "After 10 seconds, cancelling a dictation takes two presses, so one slip keeps your words",
        "10 másodperc után a diktálás megszakításához két lenyomás kell, így egy elvétés megtartja a szavait",
    ),
    (
        "Press again to discard this dictation",
        "Nyomja meg újra a diktálás elvetéséhez",
    ),
];
