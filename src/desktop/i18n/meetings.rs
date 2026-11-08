//! The Rune Meetings pane's strings and the Settings rows that exist on one
//! OS, kept beside [`super::TRANSLATIONS`] so the main table stays under the
//! source cap.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Today", ["Dzisiaj", "今天", "今日", "Heute", "Hoy", "Сьогодні"]),
    ("Yesterday", ["Wczoraj", "昨天", "昨日", "Gestern", "Ayer", "Учора"]),
    ("More", ["Więcej", "更多", "その他", "Mehr", "Más", "Більше"]),
    ("Them", ["Rozmówcy", "对方", "相手", "Andere", "Ellos", "Співрозмовники"]),
    ("Meeting titles and short summaries, written on this Mac", ["Tytuły spotkań i krótkie podsumowania tworzone na tym Macu", "会议标题和简短摘要，在这台 Mac 上生成", "会議のタイトルと短い要約をこの Mac で作成", "Besprechungstitel und kurze Zusammenfassungen, auf diesem Mac erstellt", "Títulos de reuniones y resúmenes breves, escritos en este Mac", "Назви зустрічей і короткі підсумки, створені на цьому Mac"]),
    ("This Mac does not support Apple Intelligence", ["Ten Mac nie obsługuje Apple Intelligence", "这台 Mac 不支持 Apple 智能", "この Mac は Apple Intelligence に対応していません", "Dieser Mac unterstützt Apple Intelligence nicht", "Este Mac no es compatible con Apple Intelligence", "Цей Mac не підтримує Apple Intelligence"]),
    ("Turn on Apple Intelligence in System Settings to use this", ["Włącz Apple Intelligence w Ustawieniach systemowych, aby z tego korzystać", "在“系统设置”中打开 Apple 智能以使用此功能", "使用するにはシステム設定で Apple Intelligence をオンにしてください", "Aktiviere Apple Intelligence in den Systemeinstellungen, um dies zu nutzen", "Activa Apple Intelligence en Ajustes del Sistema para usar esto", "Щоб скористатися цим, увімкніть Apple Intelligence у Системних параметрах"]),
    ("The Apple Intelligence model is still downloading", ["Model Apple Intelligence wciąż się pobiera", "Apple 智能模型仍在下载", "Apple Intelligence のモデルをダウンロード中です", "Das Apple-Intelligence-Modell wird noch geladen", "El modelo de Apple Intelligence aún se está descargando", "Модель Apple Intelligence ще завантажується"]),
    ("Requires macOS 26 or later", ["Wymaga macOS 26 lub nowszego", "需要 macOS 26 或更高版本", "macOS 26 以降が必要です", "Erfordert macOS 26 oder neuer", "Requiere macOS 26 o posterior", "Потрібна macOS 26 або новіша"]),
    ("Apple Intelligence is unavailable", ["Apple Intelligence jest niedostępne", "Apple 智能不可用", "Apple Intelligence は利用できません", "Apple Intelligence ist nicht verfügbar", "Apple Intelligence no está disponible", "Немає доступу до Apple Intelligence"]),
    ("Still grey? Fix it", ["Nadal szare? Napraw", "还是灰色的？修复", "まだグレーのまま？修復", "Noch grau? Beheben", "¿Sigue gris? Reparar", "Ще сіре? Виправити"]),
    ("An older Hex entry can keep this off", ["Starszy wpis Hex może to blokować", "旧版 Hex 条目可能会导致此项无法开启", "古い Hex の項目が妨げになっている場合があります", "Ein älterer Hex-Eintrag kann dies blockieren", "Una entrada anterior de Hex puede impedirlo", "Старий запис Hex може блокувати це"]),
    ("Descriptive meeting file names", ["Opisowe nazwy plików spotkań", "描述性会议文件名", "わかりやすい会議ファイル名", "Beschreibende Dateinamen für Meetings", "Nombres descriptivos para archivos de reuniones", "Описові назви файлів зустрічей"]),
    ("Rename exported meeting Markdown to the meeting's title once notes or a rename provide one, keeping the date and time", ["Zmień nazwę wyeksportowanego Markdownu spotkania na tytuł spotkania, gdy podadzą go notatki lub zmiana nazwy, zachowując datę i godzinę", "当笔记或重命名提供标题后，将导出的会议 Markdown 重命名为该标题，并保留日期和时间", "ノートまたは名前の変更でタイトルが付いたら、日付と時刻を保ったまま、書き出した会議の Markdown をそのタイトルに変更します", "Benennt das exportierte Markdown eines Meetings nach dessen Titel um, sobald Notizen oder eine Umbenennung einen liefern, und behält Datum und Uhrzeit", "Cambia el nombre del Markdown exportado de la reunión al título de la reunión cuando las notas o un cambio de nombre lo aporten, conservando la fecha y la hora", "Перейменовує експортований Markdown зустрічі на її назву, щойно її дадуть нотатки або перейменування, зберігаючи дату й час"]),
    ("Recording failed.", ["Nagrywanie nie powiodło się.", "录音失败。", "録音に失敗しました。", "Aufnahme fehlgeschlagen.", "La grabación falló.", "Не вдалося записати аудіо."]),
];
