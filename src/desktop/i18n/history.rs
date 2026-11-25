//! History's dictation stats strings (Labs › Dictation stats), keyed by the
//! English source string like [`super::TRANSLATIONS`].

/// The strings by their English source, in the six columns of
/// [`super::TRANSLATIONS`]: Polish, Chinese (Simplified), Japanese, German,
/// Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Sent to", ["Wysłano do", "已发送到", "送信先", "Gesendet an", "Enviado a", "Надіслано до"]),
    ("Nothing here yet", ["Jeszcze nic tu nie ma", "这里还没有内容", "まだ何もありません", "Noch nichts hier", "Aún no hay nada aquí", "Тут ще нічого немає"]),
    ("Dictations, annotations, and captures show here.", ["Tutaj pojawiają się dyktowania, adnotacje i zrzuty ekranu.", "听写、标注和截图会显示在这里。", "音声入力、注釈、キャプチャがここに表示されます。", "Diktate, Anmerkungen und Aufnahmen erscheinen hier.", "Los dictados, las anotaciones y las capturas aparecen aquí.", "Тут з’являються диктування, анотації та знімки."]),
    ("Move to Trash…", ["Przenieś do kosza…", "移到废纸篓…", "ゴミ箱に入れる…", "In den Papierkorb legen…", "Trasladar a la papelera…", "Перемістити в смітник…"]),
    ("Paste last", ["Wklej ostatni element", "粘贴最近一项", "最新の項目を貼り付け", "Letztes Element einfügen", "Pegar último elemento", "Вставити останній елемент"]),
    ("Annotations", ["Adnotacje", "标注", "注釈", "Annotationen", "Anotaciones", "Анотації"]),
    ("Edit", ["Edytuj", "编辑", "編集", "Bearbeiten", "Editar", "Редагувати"]),
    ("Show all History", ["Pokaż całą historię", "显示所有历史记录", "すべての履歴を表示", "Gesamten Verlauf anzeigen", "Mostrar todo el historial", "Показати всю історію"]),
    ("All types", ["Wszystkie typy", "所有类型", "すべての種類", "Alle Typen", "Todos los tipos", "Усі типи"]),
    ("Dictations", ["Dyktowania", "听写", "音声入力", "Diktate", "Dictados", "Диктування"]),
    ("Screen recordings", ["Nagrania ekranu", "屏幕录像", "画面収録", "Bildschirmaufnahmen", "Grabaciones de pantalla", "Записи екрана"]),
    ("Type", ["Typ", "类型", "種類", "Typ", "Tipo", "Тип"]),
    ("No matching items", ["Brak pasujących elementów", "没有匹配项", "一致する項目はありません", "Keine passenden Einträge", "No hay elementos coincidentes", "Немає відповідних елементів"]),
    ("Clear dictation history", ["Wyczyść historię dyktowania", "清除听写历史记录", "音声入力履歴を消去", "Diktatverlauf löschen", "Borrar historial de dictados", "Очистити історію диктування"]),
    ("Clear dictation history…", ["Wyczyść historię dyktowania…", "清除听写历史记录…", "音声入力履歴を消去…", "Diktatverlauf löschen…", "Borrar historial de dictados…", "Очистити історію диктування…"]),
    ("Words", ["Słowa", "字词", "単語", "Wörter", "Palabras", "Слова"]),
    ("Words per minute", ["Słowa na minutę", "每分钟字词", "毎分の単語数", "Wörter pro Minute", "Palabras por minuto", "Слів за хвилину"]),
    ("Daily streak", ["Seria dni", "连续天数", "連続日数", "Tage in Folge", "Días seguidos", "Днів поспіль"]),
    ("Dictations per day", ["Dyktowania na dzień", "每天听写", "1日あたりの音声入力", "Diktate pro Tag", "Dictados por día", "Диктувань на день"]),
    ("Top apps", ["Najczęstsze aplikacje", "常用应用", "よく使うアプリ", "Häufigste Apps", "Apps principales", "Найчастіші програми"]),
    ("Changed by Output", ["Zmienione przez Wyjście", "被输出更改", "出力による変更", "Von der Ausgabe geändert", "Cambiados por Salida", "Змінено Виведенням"]),
    ("Output time", ["Czas wyjścia", "输出时间", "出力時間", "Ausgabezeit", "Tiempo de salida", "Час виведення"]),
    ("median {median} s · slowest {slowest} s", ["mediana {median} s · najwolniej {slowest} s", "中位数 {median} 秒 · 最慢 {slowest} 秒", "中央値 {median} 秒 · 最遅 {slowest} 秒", "Median {median} s · langsamster {slowest} s", "mediana {median} s · más lento {slowest} s", "медіана {median} с · найповільніше {slowest} с"]),
    ("Dictations per day: {today} today, {busiest} on the busiest day", ["Dyktowania na dzień: dziś {today}, najwięcej {busiest} w ciągu dnia", "每天听写：今天 {today}，最多一天 {busiest}", "1日あたりの音声入力: 今日 {today}、最も多い日は {busiest}", "Diktate pro Tag: heute {today}, am meisten {busiest} an einem Tag", "Dictados por día: hoy {today}, {busiest} el día con más", "Диктувань на день: сьогодні {today}, найбільше {busiest} за день"]),
    ("Time saved", ["Zaoszczędzony czas", "节省的时间", "節約できた時間", "Gesparte Zeit", "Tiempo ahorrado", "Зекономлений час"]),
    ("Busiest hour", ["Najgorętsza godzina", "最常听写的时段", "最も多い時間帯", "Aktivste Stunde", "Hora con más dictados", "Найактивніша година"]),
    ("Typing speed", ["Szybkość pisania", "打字速度", "タイピング速度", "Tippgeschwindigkeit", "Velocidad de escritura", "Швидкість друку"]),
    ("{} wpm", ["{} słów/min", "{} 字/分", "{} 語/分", "{} Wörter/Min.", "{} palabras/min", "{} слів/хв"]),
    ("Past year", ["Ostatni rok", "过去一年", "過去1年", "Letztes Jahr", "Último año", "Останній рік"]),
    ("Dictations by hour: busiest at {hour}", ["Dyktowania według godziny: najwięcej o {hour}", "按小时听写：最多在 {hour}", "時間帯別の音声入力: 最も多いのは {hour}", "Diktate nach Stunde: am meisten um {hour}", "Dictados por hora: más a las {hour}", "Диктування за годиною: найбільше о {hour}"]),
    ("Dictations per day over the past year: {total} in total", ["Dyktowania na dzień w ostatnim roku: łącznie {total}", "过去一年每天的听写：共 {total}", "過去1年の1日あたりの音声入力: 合計 {total}", "Diktate pro Tag im letzten Jahr: insgesamt {total}", "Dictados por día durante el último año: {total} en total", "Диктування на день за останній рік: усього {total}"]),
];
