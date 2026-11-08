//! Failure and microphone messages shared by Settings and the HUD, kept
//! beside the main table for [`super::all_translations`].

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Last dictation failed: {}", ["Ostatnie dyktowanie nie powiodło się: {}", "上次听写失败：{}", "前回の音声入力に失敗しました: {}", "Letztes Diktat fehlgeschlagen: {}", "El último dictado falló: {}", "Не вдалося виконати останнє диктування: {}"]),
    ("Built-in mic is off with the lid closed. Open the lid or connect a mic.", ["Wbudowany mikrofon jest wyłączony przy zamkniętej pokrywie. Otwórz pokrywę lub podłącz mikrofon.", "合盖时内置麦克风关闭。请打开盖子或连接麦克风。", "蓋を閉じている間は内蔵マイクがオフです。蓋を開けるか、マイクを接続してください。", "Das eingebaute Mikrofon ist bei geschlossenem Deckel aus. Öffne den Deckel oder schließe ein Mikrofon an.", "El micrófono integrado está apagado con la tapa cerrada. Abre la tapa o conecta un micrófono.", "Вбудований мікрофон вимкнено, поки кришку закрито. Відкрийте кришку або під'єднайте мікрофон."]),
    ("No microphone is available. Connect a microphone.", ["Brak dostępnego mikrofonu. Podłącz mikrofon.", "没有可用的麦克风。请连接麦克风。", "使用できるマイクがありません。マイクを接続してください。", "Es ist kein Mikrofon verfügbar. Schließe ein Mikrofon an.", "No hay ningún micrófono disponible. Conecta un micrófono.", "Немає доступного мікрофона. Під'єднайте мікрофон."]),
    ("Check failed; retrying later", ["Sprawdzanie nie powiodło się; ponowimy później", "检查失败；稍后重试", "確認に失敗しました。後で再試行します", "Prüfung fehlgeschlagen; später erneut", "La comprobación falló; se reintentará más tarde", "Не вдалося перевірити; повторна спроба пізніше"]),
    ("Failed", ["Niepowodzenie", "失败", "失敗", "Fehlgeschlagen", "Fallido", "Не вдалося"]),
];
