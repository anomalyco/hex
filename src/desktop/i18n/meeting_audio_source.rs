//! Labs › Choose meeting audio's strings: the card that turns the lab on
//! and the Meetings pane's audio source picker. Kept beside the main table so
//! it stays under the source cap, like `meeting_mute`.
//!
//! Not yet reviewed by native speakers, like their neighbouring tables.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Choose meeting audio", ["Wybierz dźwięk spotkania", "选择会议音频", "会議の音声を選択", "Meeting-Audio wählen", "Elegir el audio de la reunión", "Вибрати звук зустрічі"]),
    ("Record one app or one output device in meetings instead of all computer audio", ["Nagrywaj w spotkaniach dźwięk jednej aplikacji lub jednego urządzenia wyjściowego zamiast całego dźwięku komputera.", "在会议中只录制一个应用或一个输出设备的声音，而不是所有电脑音频。", "会議で、コンピュータのすべての音声ではなく、1つのアプリまたは1つの出力デバイスの音声だけを録音します。", "Nimm in Meetings nur den Ton einer App oder eines Ausgabegeräts statt des gesamten Computertons auf.", "Graba en las reuniones solo el audio de una app o de un dispositivo de salida en lugar de todo el audio del ordenador.", "Записуйте в зустрічах лише звук однієї програми або одного пристрою виведення замість усього звуку комп’ютера."]),
    ("All computer audio", ["Cały dźwięk komputera", "所有电脑音频", "コンピュータのすべての音声", "Gesamter Computerton", "Todo el audio del ordenador", "Увесь звук комп’ютера"]),
    ("not playing now", ["nie odtwarza teraz", "当前未播放", "現在再生していません", "spielt gerade nicht", "no se está reproduciendo", "зараз не відтворюється"]),
];

pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Choose meeting audio", "Choisir l’audio des réunions"),
    (
        "Record one app or one output device in meetings instead of all computer audio",
        "Enregistrez le son d’une app ou d’une sortie audio pendant les réunions au lieu de tout le son de l’ordinateur.",
    ),
    ("All computer audio", "Tout le son de l’ordinateur"),
    ("not playing now", "ne joue pas actuellement"),
];

pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Choose meeting audio", "Escolher o áudio da reunião"),
    (
        "Record one app or one output device in meetings instead of all computer audio",
        "Grave o áudio de um app ou de uma saída de áudio nas reuniões em vez de todo o áudio do computador.",
    ),
    ("All computer audio", "Todo o áudio do computador"),
    ("not playing now", "não está reproduzindo agora"),
];

pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Choose meeting audio", "Выбрать звук встречи"),
    (
        "Record one app or one output device in meetings instead of all computer audio",
        "Записывайте на встречах звук одного приложения или одного устройства вывода вместо всего звука компьютера.",
    ),
    ("All computer audio", "Весь звук компьютера"),
    ("not playing now", "сейчас не воспроизводится"),
];

pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Choose meeting audio", "Értekezlet hangjának kiválasztása"),
    (
        "Record one app or one output device in meetings instead of all computer audio",
        "Egy alkalmazás vagy egy kimeneti eszköz hangját rögzítse az értekezleteken a számítógép teljes hangja helyett.",
    ),
    ("All computer audio", "A számítógép teljes hangja"),
    ("not playing now", "most nem játszik"),
];
