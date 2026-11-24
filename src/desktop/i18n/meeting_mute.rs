//! Labs › Mute the microphone while recording's strings, and the switch the
//! Meetings pane, menu bar, and recording capsule offer while a meeting
//! records. Kept beside the main table so it stays under the source cap.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Mute the microphone while recording", ["Wycisz mikrofon podczas nagrywania", "录制时静音麦克风", "録音中にマイクをミュート", "Mikrofon während der Aufnahme stummschalten", "Silenciar el micrófono mientras se graba", "Вимкнути мікрофон під час запису"]),
    ("Adds a mute button to meeting recordings. Hex keeps recording the call's audio but leaves out your microphone until you unmute.", ["Dodaje przycisk wyciszenia do nagrań spotkań. Hex nadal nagrywa dźwięk rozmowy, ale pomija twój mikrofon, dopóki go nie włączysz.", "为会议录音添加静音按钮。Hex 继续录制通话的声音，但在你取消静音之前不录入你的麦克风。", "会議の録音にミュートボタンを追加します。Hex は通話の音声を録音し続けますが、ミュートを解除するまであなたのマイクは含めません。", "Fügt Meeting-Aufnahmen eine Stummschalt-Schaltfläche hinzu. Hex nimmt den Ton des Anrufs weiter auf, lässt dein Mikrofon aber weg, bis du es wieder einschaltest.", "Añade un botón de silencio a las grabaciones de reuniones. Hex sigue grabando el audio de la llamada, pero deja fuera tu micrófono hasta que lo actives.", "Додає кнопку вимкнення звуку до записів зустрічей. Hex і далі записує звук дзвінка, але не додає ваш мікрофон, доки ви його не ввімкнете."]),
    ("Mute microphone", ["Wycisz mikrofon", "静音麦克风", "マイクをミュート", "Mikrofon stummschalten", "Silenciar micrófono", "Вимкнути мікрофон"]),
    ("Unmute microphone", ["Włącz mikrofon", "取消静音", "ミュートを解除", "Mikrofon einschalten", "Activar micrófono", "Увімкнути мікрофон"]),
    ("Microphone muted", ["Mikrofon wyciszony", "麦克风已静音", "マイクはミュート中", "Mikrofon stumm", "Micrófono silenciado", "Мікрофон вимкнено"]),
];

pub(super) const FRENCH: &[(&str, &str)] = &[
    (
        "Mute the microphone while recording",
        "Couper le micro pendant l’enregistrement",
    ),
    (
        "Adds a mute button to meeting recordings. Hex keeps recording the call's audio but leaves out your microphone until you unmute.",
        "Ajoute un bouton de coupure du micro aux enregistrements de réunion. Hex continue d’enregistrer le son de l’appel mais laisse de côté votre micro jusqu’à ce que vous le réactiviez.",
    ),
    ("Mute microphone", "Couper le micro"),
    ("Unmute microphone", "Réactiver le micro"),
    ("Microphone muted", "Micro coupé"),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    (
        "Mute the microphone while recording",
        "Silenciar o microfone durante a gravação",
    ),
    (
        "Adds a mute button to meeting recordings. Hex keeps recording the call's audio but leaves out your microphone until you unmute.",
        "Adiciona um botão de silêncio às gravações de reunião. O Hex continua gravando o áudio da chamada, mas deixa seu microfone de fora até você reativá-lo.",
    ),
    ("Mute microphone", "Silenciar microfone"),
    ("Unmute microphone", "Reativar microfone"),
    ("Microphone muted", "Microfone silenciado"),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    (
        "Mute the microphone while recording",
        "Выключить микрофон во время записи",
    ),
    (
        "Adds a mute button to meeting recordings. Hex keeps recording the call's audio but leaves out your microphone until you unmute.",
        "Добавляет кнопку выключения микрофона к записям встреч. Hex продолжает записывать звук звонка, но не включает ваш микрофон, пока вы его не включите.",
    ),
    ("Mute microphone", "Выключить микрофон"),
    ("Unmute microphone", "Включить микрофон"),
    ("Microphone muted", "Микрофон выключен"),
];
