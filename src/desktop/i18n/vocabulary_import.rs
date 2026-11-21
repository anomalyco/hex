//! The Vocabulary sheet's import strings, kept beside
//! [`super::TRANSLATIONS`] so the main table stays under the source cap.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Import…", ["Importuj…", "导入…", "インポート…", "Importieren…", "Importar…", "Імпортувати…"]),
    ("CSV or text files", ["Pliki CSV lub tekstowe", "CSV 或文本文件", "CSV またはテキストファイル", "CSV- oder Textdateien", "Archivos CSV o de texto", "Файли CSV або текстові"]),
    ("Any file", ["Dowolny plik", "任意文件", "すべてのファイル", "Alle Dateien", "Cualquier archivo", "Будь-який файл"]),
    ("Added {} terms", ["Dodano terminów: {}", "已添加 {} 个术语", "{} 件の用語を追加しました", "{} Begriffe hinzugefügt", "{} términos añadidos", "Додано термінів: {}"]),
    ("Added 1 term", ["Dodano 1 termin", "已添加 1 个术语", "1 件の用語を追加しました", "1 Begriff hinzugefügt", "1 término añadido", "Додано 1 термін"]),
    ("Updated existing terms", ["Zaktualizowano istniejące terminy", "已更新现有术语", "既存の用語を更新しました", "Vorhandene Begriffe aktualisiert", "Términos existentes actualizados", "Існуючі терміни оновлено"]),
    ("No new terms", ["Brak nowych terminów", "没有新术语", "新しい用語はありません", "Keine neuen Begriffe", "No hay términos nuevos", "Нових термінів немає"]),
    ("Couldn't read that file", ["Nie udało się odczytać tego pliku", "无法读取该文件", "このファイルを読み込めませんでした", "Die Datei konnte nicht gelesen werden", "No se pudo leer ese archivo", "Не вдалося прочитати цей файл"]),
    ("Some didn't fit", ["Niektóre się nie zmieściły", "部分内容未能加入", "一部は入りませんでした", "Einige passten nicht", "Algunos no cupieron", "Деякі не вмістилися"]),
];
