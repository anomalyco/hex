//! Labs › Always listening's strings: the card that turns the lab on and
//! its pause picker. Kept beside the main table so it stays under the source
//! cap, like `middle_mouse`.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Always listening", ["Ciągłe słuchanie", "始终聆听", "常に聞き取る", "Immer zuhören", "Escucha continua", "Постійне прослуховування"]),
    ("Speak to start a dictation and pause to finish it, with no shortcut. The shortcut pauses listening", ["Zacznij mówić, aby rozpocząć dyktowanie, i zrób pauzę, aby je zakończyć, bez skrótu. Skrót wstrzymuje słuchanie", "开口说话即可开始听写，停顿即可结束，无需快捷键。快捷键可暂停聆听", "話し始めると音声入力が始まり、間を置くと終了します。ショートカットは不要です。ショートカットで聞き取りを一時停止します", "Sprich, um ein Diktat zu starten, und mach eine Pause, um es zu beenden, ganz ohne Tastenkürzel. Das Tastenkürzel pausiert das Zuhören", "Habla para empezar un dictado y haz una pausa para terminarlo, sin atajo. El atajo pausa la escucha", "Говоріть, щоб почати диктування, і зробіть паузу, щоб завершити його, без комбінації клавіш. Комбінація призупиняє прослуховування"]),
    ("Finish after a pause of", ["Zakończ po pauzie trwającej", "停顿多久后结束", "この長さの間で終了", "Beenden nach einer Pause von", "Terminar tras una pausa de", "Завершувати після паузи"]),
];

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Always listening", "Écoute permanente"),
    (
        "Speak to start a dictation and pause to finish it, with no shortcut. The shortcut pauses listening",
        "Parlez pour commencer une dictée et faites une pause pour la terminer, sans raccourci. Le raccourci met l’écoute en pause",
    ),
    ("Finish after a pause of", "Terminer après une pause de"),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Always listening", "Sempre ouvindo"),
    (
        "Speak to start a dictation and pause to finish it, with no shortcut. The shortcut pauses listening",
        "Fale para começar um ditado e faça uma pausa para terminar, sem atalho. O atalho pausa a escuta",
    ),
    ("Finish after a pause of", "Terminar após uma pausa de"),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Always listening", "Постоянное прослушивание"),
    (
        "Speak to start a dictation and pause to finish it, with no shortcut. The shortcut pauses listening",
        "Говорите, чтобы начать диктовку, и сделайте паузу, чтобы закончить её, без сочетания клавиш. Сочетание приостанавливает прослушивание",
    ),
    ("Finish after a pause of", "Завершать после паузы"),
];

pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Always listening", "Folyamatos figyelés"),
    (
        "Speak to start a dictation and pause to finish it, with no shortcut. The shortcut pauses listening",
        "Beszéljen a diktálás indításához, és tartson szünetet a befejezéshez, billentyűparancs nélkül. A billentyűparancs szünetelteti a figyelést",
    ),
    ("Finish after a pause of", "Befejezés ennyi szünet után"),
];
