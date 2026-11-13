//! The macOS menu bar menu's translated strings, kept beside
//! [`super::TRANSLATIONS`] so the main table stays under the source cap.
//! Most of the menu reuses the window's labels; these are its own.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Pause Hex", ["Wstrzymaj Hex", "暂停 Hex", "Hex を一時停止", "Hex pausieren", "Pausar Hex", "Призупинити Hex"]),
    ("Resume Hex", ["Wznów Hex", "恢复 Hex", "Hex を再開", "Hex fortsetzen", "Reanudar Hex", "Відновити Hex"]),
    ("Restore last capture", ["Przywróć ostatnie przechwycenie", "恢复上次捕获", "前回のキャプチャを復元", "Letzte Aufnahme wiederherstellen", "Restaurar la última captura", "Відновити останнє захоплення"]),
    ("Quit {}", ["Zakończ {}", "退出 {}", "{} を終了", "{} beenden", "Salir de {}", "Вийти з {}"]),
    ("No downloaded models", ["Brak pobranych modeli", "没有已下载的模型", "ダウンロード済みのモデルはありません", "Keine geladenen Modelle", "No hay modelos descargados", "Немає завантажених моделей"]),
    ("Preparing {}…", ["Przygotowywanie {}…", "正在准备 {}…", "{} を準備中…", "{} wird vorbereitet…", "Preparando {}…", "Підготовка {}…"]),
    ("Cancel model switch", ["Anuluj zmianę modelu", "取消切换模型", "モデルの切り替えをキャンセル", "Modellwechsel abbrechen", "Cancelar el cambio de modelo", "Скасувати зміну моделі"]),
    ("Model switch failed — open Settings…", ["Nie udało się zmienić modelu — otwórz Ustawienia…", "切换模型失败 — 打开设置…", "モデルを切り替えられませんでした — 設定を開く…", "Modellwechsel fehlgeschlagen — Einstellungen öffnen…", "No se pudo cambiar de modelo — abrir Ajustes…", "Не вдалося змінити модель — відкрити Налаштування…"]),
    ("Manage models", ["Zarządzaj modelami", "管理模型", "モデルを管理", "Modelle verwalten", "Gestionar modelos", "Керувати моделями"]),
    ("Starting meeting recording…", ["Rozpoczynanie nagrywania spotkania…", "正在开始录制会议…", "会議の録音を開始中…", "Besprechungsaufnahme wird gestartet…", "Iniciando la grabación de la reunión…", "Початок запису зустрічі…"]),
    ("Transcribing meeting…", ["Transkrypcja spotkania…", "正在转写会议…", "会議を文字起こし中…", "Besprechung wird transkribiert…", "Transcribiendo la reunión…", "Розпізнавання зустрічі…"]),
    ("Updates managed by your organization", ["Aktualizacje zarządzane przez Twoją organizację", "更新由你的组织管理", "アップデートは組織によって管理されています", "Updates werden von deiner Organisation verwaltet", "Actualizaciones gestionadas por tu organización", "Оновленнями керує ваша організація"]),
];
