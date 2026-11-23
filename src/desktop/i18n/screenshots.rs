//! Labs › Screenshots' recording strings, kept beside [`super::TRANSLATIONS`]
//! and the French, Portuguese, and Russian tables so those stay under the
//! source cap.

use std::collections::HashMap;

/// The language whose own table (here, and in the Annotate Agents section)
/// merges over the main rows.
#[derive(Clone, Copy)]
pub(super) enum Language {
    French,
    Portuguese,
    Russian,
}

/// A one-language table with these strings added.
pub(super) fn merged(
    table: &[(&'static str, &'static str)],
    language: Language,
) -> HashMap<&'static str, &'static str> {
    let (extra, agents, signature, style, hud_height, voice_isolation, annotate, calls, edits) =
        match language {
            Language::French => (
                FRENCH,
                super::agents::FRENCH,
                super::signature::FRENCH,
                super::style::FRENCH,
                super::hud_height::FRENCH,
                super::voice_isolation::FRENCH,
                super::annotate_dictation::FRENCH,
                super::calls::FRENCH,
                super::transcript_edits::FRENCH,
            ),
            Language::Portuguese => (
                PORTUGUESE,
                super::agents::PORTUGUESE,
                super::signature::PORTUGUESE,
                super::style::PORTUGUESE,
                super::hud_height::PORTUGUESE,
                super::voice_isolation::PORTUGUESE,
                super::annotate_dictation::PORTUGUESE,
                super::calls::PORTUGUESE,
                super::transcript_edits::PORTUGUESE,
            ),
            Language::Russian => (
                RUSSIAN,
                super::agents::RUSSIAN,
                super::signature::RUSSIAN,
                super::style::RUSSIAN,
                super::hud_height::RUSSIAN,
                super::voice_isolation::RUSSIAN,
                super::annotate_dictation::RUSSIAN,
                super::calls::RUSSIAN,
                super::transcript_edits::RUSSIAN,
            ),
        };

    table
        .iter()
        .chain(extra)
        .chain(agents)
        .chain(signature)
        .chain(style)
        .chain(hud_height)
        .chain(voice_isolation)
        .chain(annotate)
        .chain(calls)
        .chain(edits)
        .copied()
        .collect()
}

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Scale Retina to 1x", ["Skaluj Retina do 1x", "将 Retina 缩放到 1x", "Retina を 1x に縮小", "Retina auf 1x skalieren", "Escalar Retina a 1x", "Масштабувати Retina до 1x"]),
    ("Max resolution", ["Maks. rozdzielczość", "最大分辨率", "最大解像度", "Max. Auflösung", "Resolución máxima", "Макс. роздільність"]),
    ("Frame rate", ["Liczba klatek", "帧率", "フレームレート", "Bildrate", "Fotogramas por segundo", "Частота кадрів"]),
    ("Quality", ["Jakość", "画质", "画質", "Qualität", "Calidad", "Якість"]),
    ("30 fps", ["30 kl./s", "30 帧/秒", "30 fps", "30 fps", "30 fps", "30 кадр/с"]),
    ("60 fps", ["60 kl./s", "60 帧/秒", "60 fps", "60 fps", "60 fps", "60 кадр/с"]),
    ("High", ["Wysoka", "高", "高", "Hoch", "Alta", "Висока"]),
    ("Medium", ["Średnia", "中", "中", "Mittel", "Media", "Середня"]),
    ("Low", ["Niska", "低", "低", "Niedrig", "Baja", "Низька"]),
    ("1440p", ["1440p", "1440p", "1440p", "1440p", "1440p", "1440p"]),
    ("1080p", ["1080p", "1080p", "1080p", "1080p", "1080p", "1080p"]),
    ("720p", ["720p", "720p", "720p", "720p", "720p", "720p"]),
    ("Full screen", ["Cały ekran", "全屏", "全画面", "Ganzer Bildschirm", "Pantalla completa", "Увесь екран"]),
    ("No microphone", ["Bez mikrofonu", "无麦克风", "マイクなし", "Kein Mikrofon", "Sin micrófono", "Без мікрофона"]),
    ("Computer audio", ["Dźwięk komputera", "电脑音频", "コンピュータのオーディオ", "Computer-Audio", "Audio del ordenador", "Звук комп’ютера"]),
    ("Options", ["Opcje", "选项", "オプション", "Optionen", "Opciones", "Параметри"]),
    ("Same as dictation", ["Jak przy dyktowaniu", "与听写相同", "音声入力と同じ", "Wie beim Diktieren", "El mismo que el dictado", "Як для диктування"]),
    ("Record computer audio", ["Nagrywaj dźwięk komputera", "录制电脑音频", "コンピュータのオーディオを録音", "Computer-Audio aufnehmen", "Grabar el audio del ordenador", "Записувати звук комп’ютера"]),
    ("Display {}", ["Ekran {}", "显示器 {}", "ディスプレイ {}", "Display {}", "Pantalla {}", "Дисплей {}"]),
    ("Export size", ["Rozmiar eksportu", "导出尺寸", "書き出しサイズ", "Exportgröße", "Tamaño de exportación", "Розмір експорту"]),
    ("Pause", ["Wstrzymaj", "暂停", "一時停止", "Pause", "Pausar", "Пауза"]),
];

#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Scale Retina to 1x", "Ramener Retina à 1x"),
    ("Max resolution", "Résolution max."),
    ("Frame rate", "Fréquence d’images"),
    ("Quality", "Qualité"),
    ("30 fps", "30 i/s"),
    ("60 fps", "60 i/s"),
    ("High", "Élevée"),
    ("Medium", "Moyenne"),
    ("Low", "Faible"),
    ("1440p", "1440p"),
    ("1080p", "1080p"),
    ("720p", "720p"),
    ("Full screen", "Plein écran"),
    ("No microphone", "Aucun micro"),
    ("Computer audio", "Son de l’ordinateur"),
    ("Options", "Options"),
    ("Same as dictation", "Comme la dictée"),
    ("Record computer audio", "Enregistrer le son de l’ordinateur"),
    ("Display {}", "Écran {}"),
    ("Export size", "Taille d’export"),
    ("Pause", "Pause"),
];

#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Scale Retina to 1x", "Reduzir Retina para 1x"),
    ("Max resolution", "Resolução máxima"),
    ("Frame rate", "Taxa de quadros"),
    ("Quality", "Qualidade"),
    ("30 fps", "30 qps"),
    ("60 fps", "60 qps"),
    ("High", "Alta"),
    ("Medium", "Média"),
    ("Low", "Baixa"),
    ("1440p", "1440p"),
    ("1080p", "1080p"),
    ("720p", "720p"),
    ("Full screen", "Tela inteira"),
    ("No microphone", "Sem microfone"),
    ("Computer audio", "Áudio do computador"),
    ("Options", "Opções"),
    ("Same as dictation", "O mesmo do ditado"),
    ("Record computer audio", "Gravar o áudio do computador"),
    ("Display {}", "Tela {}"),
    ("Export size", "Tamanho da exportação"),
    ("Pause", "Pausar"),
];

#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Scale Retina to 1x", "Масштабировать Retina до 1x"),
    ("Max resolution", "Макс. разрешение"),
    ("Frame rate", "Частота кадров"),
    ("Quality", "Качество"),
    ("30 fps", "30 кадр/с"),
    ("60 fps", "60 кадр/с"),
    ("High", "Высокое"),
    ("Medium", "Среднее"),
    ("Low", "Низкое"),
    ("1440p", "1440p"),
    ("1080p", "1080p"),
    ("720p", "720p"),
    ("Full screen", "Весь экран"),
    ("No microphone", "Без микрофона"),
    ("Computer audio", "Звук компьютера"),
    ("Options", "Параметры"),
    ("Same as dictation", "Как для диктовки"),
    ("Record computer audio", "Записывать звук компьютера"),
    ("Display {}", "Дисплей {}"),
    ("Export size", "Размер экспорта"),
    ("Pause", "Пауза"),
];
