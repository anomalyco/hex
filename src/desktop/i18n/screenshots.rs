//! Labs › Screenshots' recording strings, kept beside [`super::TRANSLATIONS`]
//! and the French, Portuguese, and Russian tables so those stay under the
//! source cap.

use std::collections::HashMap;

/// A one-language table with these strings added.
pub(super) fn merged(
    table: &[(&'static str, &'static str)],
    extra: &[(&'static str, &'static str)],
) -> HashMap<&'static str, &'static str> {
    table.iter().chain(extra).copied().collect()
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
];
