//! The Labs rows' translated strings, kept beside [`super::TRANSLATIONS`]
//! so the main table stays under the source cap.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Live text", ["Tekst na żywo", "实时文字", "ライブテキスト", "Live-Text", "Texto en vivo", "Текст наживо"]),
    ("Show provisional words beside the HUD while dictating", ["Pokazuj robocze słowa obok HUD-u podczas dyktowania", "听写时在 HUD 旁显示临时词语", "音声入力中に HUD の横に仮の語句を表示します", "Vorläufige Wörter beim Diktieren neben dem HUD zeigen", "Muestra palabras provisionales junto al HUD al dictar", "Показувати проміжні слова поруч із HUD під час диктування"]),
    ("Failure notices", ["Powiadomienia o błędach", "失败通知", "失敗のお知らせ", "Fehlerhinweise", "Avisos de fallo", "Сповіщення про збої"]),
    ("Show why a dictation failed beside the HUD", ["Pokazuj, dlaczego dyktowanie się nie powiodło, obok HUD-u", "在 HUD 旁显示听写失败的原因", "音声入力に失敗した理由を HUD の横に表示します", "Bei einem fehlgeschlagenen Diktat den Grund neben dem HUD zeigen", "Muestra por qué falló un dictado junto al HUD", "Показувати причину невдалого диктування поруч із HUD"]),
    ("Live typing", ["Pisanie na żywo", "实时输入", "ライブ入力", "Live-Tippen", "Escritura en vivo", "Живе введення"]),
    ("Show provisional words in the focused text field while dictating on Wayland, then type the final text there", ["Pokazuj robocze słowa w aktywnym polu tekstowym podczas dyktowania na Waylandzie, a na końcu wpisz tam gotowy tekst", "在 Wayland 上听写时，将临时词语显示在焦点文本框中，最后把最终文本输入到那里", "Wayland での音声入力中、仮の語句をフォーカス中のテキスト欄に表示し、最後に確定したテキストをそこへ入力します", "Zeigt beim Diktieren unter Wayland vorläufige Wörter im fokussierten Textfeld und tippt am Ende den fertigen Text dort ein", "Muestra palabras provisionales en el campo de texto enfocado al dictar en Wayland y escribe allí el texto final", "Показує проміжні слова в полі введення під час диктування у Wayland і вписує туди остаточний текст"]),
    ("Live chapters", ["Rozdziały na żywo", "实时章节", "ライブチャプター", "Live-Kapitel", "Capítulos en vivo", "Розділи наживо"]),
    ("Break meeting transcripts into chapters while recording", ["Dziel transkrypcje spotkań na rozdziały podczas nagrywania", "在录制会议时将转录划分为章节", "会議の録音中に文字起こしをチャプターに分けます", "Meeting-Transkripte während der Aufnahme in Kapitel gliedern", "Divide las transcripciones de reuniones en capítulos mientras grabas", "Розбивати транскрипти зустрічей на розділи під час запису"]),
    ("Scene and story browser in the window", ["Przeglądarka scen i makiet w oknie", "窗口内的场景与示例浏览器", "ウィンドウ内のシーンとストーリーのブラウザー", "Szenen- und Story-Browser im Fenster", "Explorador de escenas e historias en la ventana", "Переглядач сцен та історій у вікні"]),
    ("Record calls and keep transcripts", ["Nagrywaj rozmowy i zachowuj transkrypcje", "录制通话并保留转录", "通話を録音して文字起こしを残します", "Anrufe aufnehmen und Transkripte behalten", "Graba llamadas y guarda transcripciones", "Записуйте дзвінки й зберігайте транскрипти"]),
];
