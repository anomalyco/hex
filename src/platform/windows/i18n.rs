//! UI translations for the Windows shell, keyed by the English source
//! string. The language follows the Windows user locale; unknown locales and
//! untranslated strings fall back to English. Composite strings are `{}`
//! templates filled through [`tr_fill`].

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// The selectable interface languages: persisted code and native name. The
/// leading `None` entry follows the Windows display language.
pub(crate) const LANGUAGE_CHOICES: &[(Option<&str>, &str)] = &[
    (None, "System"),
    (Some("en"), "English"),
    (Some("pl"), "Polski"),
    (Some("zh"), "中文"),
    (Some("ja"), "日本語"),
    (Some("de"), "Deutsch"),
    (Some("es"), "Español"),
];

/// Active language column into [`TRANSLATIONS`]; 255 means English.
static CURRENT: AtomicU8 = AtomicU8::new(255);

const ENGLISH: u8 = 255;

fn column_for_code(code: &str) -> u8 {
    match code {
        "pl" => 0,
        "zh" => 1,
        "ja" => 2,
        "de" => 3,
        "es" => 4,
        _ => ENGLISH,
    }
}

/// Resolve and activate the persisted language choice; `None` follows the
/// Windows user locale. Safe to call again whenever the setting changes.
pub(crate) fn apply(setting: Option<&str>) {
    let code = match setting {
        Some(code) => column_for_code(code),
        None => column_for_code(&crate::windows_settings::user_language()),
    };
    CURRENT.store(code, Ordering::Relaxed);
}

/// The native display name for the persisted choice.
pub(crate) fn choice_name(setting: Option<&str>) -> &'static str {
    LANGUAGE_CHOICES
        .iter()
        .find(|(code, _)| *code == setting)
        .map(|(_, name)| *name)
        .unwrap_or("System")
}

/// Translate one UI string. Returns the input unchanged for English or for
/// strings missing from the table, so dynamic values pass through safely.
pub(crate) fn tr(english: &str) -> &str {
    let column = CURRENT.load(Ordering::Relaxed);
    if column == ENGLISH {
        return english;
    }
    tables()[column as usize]
        .get(english)
        .copied()
        .unwrap_or(english)
}

/// Translate a `{}` template and substitute the dynamic value, keeping
/// language-specific word order intact.
pub(crate) fn tr_fill(template: &str, value: &str) -> String {
    tr(template).replace("{}", value)
}

fn tables() -> &'static [HashMap<&'static str, &'static str>; 5] {
    static TABLES: OnceLock<[HashMap<&'static str, &'static str>; 5]> = OnceLock::new();
    TABLES.get_or_init(|| {
        std::array::from_fn(|column| {
            TRANSLATIONS
                .iter()
                .map(|(english, translated)| (*english, translated[column]))
                .collect()
        })
    })
}

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish.
#[rustfmt::skip]
const TRANSLATIONS: &[(&str, [&str; 5])] = &[
    ("Language", ["Język", "语言", "言語", "Sprache", "Idioma"]),
    ("The language you dictate in; Auto detects it", ["Język dyktowania; Auto wykrywa automatycznie", "听写语言；Auto 自动检测", "音声入力の言語。Auto は自動検出", "Diktatsprache; Auto erkennt automatisch", "Idioma del dictado; Auto lo detecta"]),
    ("Interface language", ["Język interfejsu", "界面语言", "表示言語", "Sprache der Oberfläche", "Idioma de la interfaz"]),
    ("Language of the HEX interface", ["Język interfejsu HEX", "HEX 界面语言", "HEX の表示言語", "Sprache der HEX-Oberfläche", "Idioma de la interfaz de HEX"]),
    ("System", ["Systemowy", "跟随系统", "システム", "System", "Sistema"]),
    ("Model", ["Model", "模型", "モデル", "Modell", "Modelo"]),
    ("On-device speech model", ["Model mowy na urządzeniu", "本地语音模型", "オンデバイス音声モデル", "Lokales Sprachmodell", "Modelo de voz local"]),
    ("Choose any on-device speech model", ["Wybierz dowolny lokalny model mowy", "选择任意本地语音模型", "任意のオンデバイス音声モデルを選択", "Beliebiges lokales Sprachmodell wählen", "Elige cualquier modelo de voz local"]),
    ("Browse", ["Przeglądaj", "浏览", "参照", "Durchsuchen", "Explorar"]),
    ("Speech models", ["Modele mowy", "语音模型", "音声モデル", "Sprachmodelle", "Modelos de voz"]),
    ("All languages", ["Wszystkie języki", "所有语言", "すべての言語", "Alle Sprachen", "Todos los idiomas"]),
    ("Recognition hints", ["Podpowiedzi rozpoznawania", "识别提示", "認識ヒント", "Erkennungshinweise", "Sugerencias de reconocimiento"]),
    ("Names and terms to softly prime the speech model", ["Nazwy i terminy delikatnie podpowiadane modelowi mowy", "用于轻度引导语音模型的名称和术语", "音声モデルにそっと知らせる名前や用語", "Namen und Begriffe, die das Sprachmodell sanft vorbereiten", "Nombres y términos que orientan suavemente al modelo de voz"]),
    ("Fastest", ["Najszybszy", "最快", "最速", "Am schnellsten", "Más rápido"]),
    ("Most accurate", ["Najdokładniejszy", "最准确", "最高精度", "Am genauesten", "Más preciso"]),
    ("Switches dictation to {}", ["Zmieni język dyktowania na {}", "会将听写语言切换为 {}", "音声入力言語を {} に切り替えます", "Wechselt die Diktiersprache zu {}", "Cambia el dictado a {}"]),
    ("Use", ["Użyj", "使用", "使用", "Verwenden", "Usar"]),
    ("Download", ["Pobierz", "下载", "ダウンロード", "Herunterladen", "Descargar"]),
    ("Cancel", ["Anuluj", "取消", "キャンセル", "Abbrechen", "Cancelar"]),
    ("Active", ["Aktywny", "使用中", "有効", "Aktiv", "Activo"]),
    ("Auto", ["Auto", "自动", "自動", "Auto", "Auto"]),
    ("Downloading", ["Pobieranie", "下载中", "ダウンロード中", "Lädt herunter", "Descargando"]),
    ("Verifying model", ["Weryfikacja modelu", "正在校验模型", "モデルを検証中", "Modell wird geprüft", "Verificando modelo"]),
    ("Loading model", ["Wczytywanie modelu", "正在加载模型", "モデルを読み込み中", "Modell wird geladen", "Cargando modelo"]),
    ("Model could not be installed.", ["Nie udało się zainstalować modelu.", "无法安装模型。", "モデルをインストールできませんでした。", "Modell konnte nicht installiert werden.", "No se pudo instalar el modelo."]),
    ("Installed", ["Zainstalowany", "已安装", "インストール済み", "Installiert", "Instalado"]),
    ("Recommended", ["Zalecany", "推荐", "おすすめ", "Empfohlen", "Recomendado"]),
    ("Settings", ["Ustawienia", "设置", "設定", "Einstellungen", "Ajustes"]),
    ("Modes", ["Tryby", "模式", "モード", "Modi", "Modos"]),
    ("History", ["Historia", "历史记录", "履歴", "Verlauf", "Historial"]),
    ("Stop listening", ["Zatrzymaj nasłuchiwanie", "停止监听", "リッスンを停止", "Zuhören beenden", "Dejar de escuchar"]),
    ("Start listening", ["Rozpocznij nasłuchiwanie", "开始监听", "リッスンを開始", "Zuhören starten", "Empezar a escuchar"]),
    ("Install model", ["Zainstaluj model", "安装模型", "モデルをインストール", "Modell installieren", "Instalar modelo"]),
    ("Add replacement", ["Dodaj zamianę", "添加替换", "置換を追加", "Ersetzung hinzufügen", "Añadir sustitución"]),
    ("Add mode", ["Dodaj tryb", "添加模式", "モードを追加", "Modus hinzufügen", "Añadir modo"]),
    ("Remove mode", ["Usuń tryb", "移除模式", "モードを削除", "Modus entfernen", "Quitar modo"]),
    ("Add correction", ["Dodaj poprawkę", "添加更正", "修正を追加", "Korrektur hinzufügen", "Añadir corrección"]),
    ("Application modes", ["Tryby aplikacji", "应用模式", "アプリモード", "Anwendungsmodi", "Modos de aplicación"]),
    ("Applies when the focused application contains any of these names", ["Działa, gdy aktywna aplikacja zawiera którąś z tych nazw", "当前台应用包含这些名称之一时生效", "前面のアプリ名に次のいずれかが含まれるとき適用", "Gilt, wenn die fokussierte Anwendung einen dieser Namen enthält", "Se aplica cuando la aplicación activa contiene alguno de estos nombres"]),
    ("No modes yet. Add one to correct text in specific applications.", ["Brak trybów. Dodaj pierwszy, aby poprawiać tekst w wybranych aplikacjach.", "还没有模式。添加一个以在特定应用中更正文本。", "モードはまだありません。特定のアプリで文字を修正するには追加してください。", "Noch keine Modi. Fügen Sie einen hinzu, um Text in bestimmten Anwendungen zu korrigieren.", "Aún no hay modos. Añade uno para corregir texto en aplicaciones concretas."]),
    ("Clear all", ["Wyczyść wszystko", "全部清除", "すべて消去", "Alle löschen", "Borrar todo"]),
    ("Really clear all?", ["Na pewno wyczyścić?", "确定全部清除？", "本当にすべて消去？", "Wirklich alle löschen?", "¿Borrar todo?"]),
    ("Copy", ["Kopiuj", "复制", "コピー", "Kopieren", "Copiar"]),
    ("Copied", ["Skopiowano", "已复制", "コピーしました", "Kopiert", "Copiado"]),
    ("Delete", ["Usuń", "删除", "削除", "Löschen", "Eliminar"]),
    ("Dismiss", ["Zamknij", "关闭", "閉じる", "Schließen", "Descartar"]),
    ("Remove", ["Usuń", "移除", "削除", "Entfernen", "Quitar"]),
    ("Keep", ["Przechowuj", "保留", "保持", "Aufbewahren", "Conservar"]),
    ("Dictation", ["Dyktowanie", "听写", "音声入力", "Diktat", "Dictado"]),
    ("Send", ["Wysłano", "发送", "送信", "Senden", "Enviar"]),
    ("Voice Action", ["Akcja głosowa", "语音操作", "音声アクション", "Sprachaktion", "Acción de voz"]),
    ("Application", ["Aplikacja", "应用", "アプリ", "Anwendung", "Aplicación"]),
    ("Problem", ["Problem", "问题", "問題", "Problem", "Problema"]),
    ("Default mode", ["Tryb domyślny", "默认模式", "デフォルトモード", "Standardmodus", "Modo predeterminado"]),
    ("Replacements", ["Zamiany", "替换", "置換", "Ersetzungen", "Sustituciones"]),
    ("Local transcription", ["Lokalna transkrypcja", "本地转写", "ローカル文字起こし", "Lokale Transkription", "Transcripción local"]),
    ("Language and on-device speech model", ["Język i model mowy na urządzeniu", "语言与本地语音模型", "言語とオンデバイス音声モデル", "Sprache und lokales Sprachmodell", "Idioma y modelo de voz local"]),
    ("Microphone", ["Mikrofon", "麦克风", "マイク", "Mikrofon", "Micrófono"]),
    ("Uses the selected WASAPI input or the Windows default", ["Używa wybranego wejścia WASAPI lub domyślnego w Windows", "使用所选 WASAPI 输入或 Windows 默认设备", "選択した WASAPI 入力または Windows の既定を使用", "Verwendet den gewählten WASAPI-Eingang oder den Windows-Standard", "Usa la entrada WASAPI seleccionada o la predeterminada de Windows"]),
    ("Dictation shortcut", ["Skrót dyktowania", "听写快捷键", "音声入力ショートカット", "Diktat-Tastenkürzel", "Atajo de dictado"]),
    ("Hold while speaking; release to transcribe and paste", ["Przytrzymaj podczas mówienia; puść, aby przepisać i wkleić", "说话时按住；松开即转写并粘贴", "話す間押し続け、離すと文字起こしして貼り付け", "Beim Sprechen halten; loslassen zum Transkribieren und Einfügen", "Mantén pulsado al hablar; suelta para transcribir y pegar"]),
    ("Double-tap to lock", ["Podwójne naciśnięcie blokuje", "双击锁定", "ダブルタップでロック", "Doppeltippen zum Sperren", "Doble pulsación para bloquear"]),
    ("Double-tap only", ["Tylko podwójne naciśnięcie", "仅限双击", "ダブルタップのみ", "Nur Doppeltippen", "Solo doble pulsación"]),
    ("Wait for two complete shortcut taps before recording", ["Nagrywanie zaczyna się dopiero po dwóch pełnych naciśnięciach skrótu", "需要连按两次快捷键才开始录音", "ショートカットを2回押し切ってから録音を開始", "Aufnahme erst nach zwei vollständigen Kürzel-Tipps", "La grabación espera dos pulsaciones completas del atajo"]),
    ("Tap the shortcut twice, then speak hands-free; press it again to finish", ["Naciśnij skrót dwa razy i mów bez trzymania; naciśnij ponownie, aby zakończyć", "连按两次快捷键即可免按说话；再按一次结束", "ショートカットを2回押すとハンズフリーで話せます。もう一度押すと終了", "Kürzel zweimal drücken und freihändig sprechen; erneut drücken zum Beenden", "Pulsa el atajo dos veces y habla sin manos; púlsalo de nuevo para terminar"]),
    ("Paste last dictation", ["Wklej ostatnie dyktowanie", "粘贴上次听写", "前回の音声入力を貼り付け", "Letztes Diktat einfügen", "Pegar último dictado"]),
    ("Insert the most recent completed dictation at the current focus", ["Wstawia ostatnie ukończone dyktowanie w bieżącym miejscu", "在当前光标处插入最近完成的听写", "直近の音声入力を現在のフォーカス位置に挿入", "Fügt das letzte Diktat an der aktuellen Position ein", "Inserta el último dictado completado en el foco actual"]),
    ("While dictating", ["Podczas dyktowania", "听写时", "音声入力中", "Während des Diktats", "Mientras dictas"]),
    ("Release microphone while idle", ["Zwalniaj mikrofon w bezczynności", "空闲时释放麦克风", "待機中はマイクを解放", "Mikrofon im Leerlauf freigeben", "Liberar el micrófono en reposo"]),
    ("Adds first-capture latency and disables audio pre-roll", ["Dodaje opóźnienie pierwszego nagrania i wyłącza pre-roll audio", "会增加首次采集延迟并停用音频预录", "初回キャプチャに遅延が生じ、音声プリロールが無効になります", "Erhöht die Latenz der ersten Aufnahme und deaktiviert den Audio-Pre-Roll", "Añade latencia a la primera captura y desactiva el pre-roll de audio"]),
    ("Control other audio while a dictation records", ["Steruj innym dźwiękiem podczas nagrywania dyktowania", "听写录音时控制其他音频", "音声入力の録音中に他のオーディオを制御", "Andere Audioquellen während der Diktataufnahme steuern", "Controla el resto del audio mientras se graba el dictado"]),
    ("Mute", ["Wycisz", "静音", "ミュート", "Stummschalten", "Silenciar"]),
    ("Pause media", ["Wstrzymaj media", "暂停媒体", "メディアを一時停止", "Medien pausieren", "Pausar medios"]),
    ("Do nothing", ["Nic nie rób", "不处理", "何もしない", "Nichts tun", "No hacer nada"]),
    ("Recording indicator", ["Wskaźnik nagrywania", "录音指示器", "録音インジケーター", "Aufnahmeanzeige", "Indicador de grabación"]),
    ("Show the dictation pill at the top or bottom of the screen", ["Pokazuj pastylkę dyktowania u góry lub u dołu ekranu", "在屏幕顶部或底部显示听写胶囊", "画面の上または下に音声入力ピルを表示", "Diktat-Pille oben oder unten am Bildschirm anzeigen", "Muestra la píldora de dictado arriba o abajo de la pantalla"]),
    ("Feedback volume", ["Głośność dźwięków", "提示音音量", "フィードバック音量", "Feedback-Lautstärke", "Volumen de avisos"]),
    ("Recording start, stop, and cancellation tones", ["Dźwięki startu, stopu i anulowania nagrywania", "开始、停止与取消提示音", "録音開始・停止・キャンセルの音", "Töne für Start, Stopp und Abbruch", "Tonos de inicio, parada y cancelación"]),
    ("Software updates", ["Aktualizacje aplikacji", "软件更新", "ソフトウェア更新", "Software-Updates", "Actualizaciones de software"]),
    ("Checks the GitHub releases page in the background", ["Sprawdza stronę wydań GitHub w tle", "在后台检查 GitHub 发布页面", "バックグラウンドで GitHub リリースを確認", "Prüft die GitHub-Releases-Seite im Hintergrund", "Comprueba la página de versiones de GitHub en segundo plano"]),
    ("Check now", ["Sprawdź teraz", "立即检查", "今すぐ確認", "Jetzt prüfen", "Comprobar ahora"]),
    ("Checking", ["Sprawdzanie", "检查中", "確認中", "Wird geprüft", "Comprobando"]),
    ("Launch at login", ["Uruchamiaj przy logowaniu", "登录时启动", "ログイン時に起動", "Beim Anmelden starten", "Iniciar al iniciar sesión"]),
    ("Start HEX hidden in the Windows system tray after sign-in", ["Uruchamia HEX ukryty w zasobniku systemowym po zalogowaniu", "登录后在系统托盘中隐藏启动 HEX", "サインイン後、HEX をシステムトレイに隠して起動", "Startet HEX nach der Anmeldung versteckt im Infobereich", "Inicia HEX oculto en la bandeja del sistema tras iniciar sesión"]),
    ("Listen on launch", ["Nasłuchuj po uruchomieniu", "启动时监听", "起動時にリッスン", "Beim Start zuhören", "Escuchar al iniciar"]),
    ("Start the global dictation listener when HEX opens", ["Uruchamia globalne nasłuchiwanie dyktowania przy starcie HEX", "HEX 打开时启动全局听写监听", "HEX 起動時にグローバル音声入力リスナーを開始", "Startet den globalen Diktat-Listener beim Öffnen von HEX", "Inicia el escuchador global de dictado al abrir HEX"]),
    ("Text replacements", ["Zamiany tekstu", "文本替换", "テキスト置換", "Textersetzungen", "Sustituciones de texto"]),
    ("Exact phrase-boundary corrections run before every Windows paste", ["Dokładne poprawki fraz stosowane przed każdym wklejeniem", "在每次粘贴前应用精确的短语替换", "貼り付け前に適用される正確なフレーズ置換", "Exakte Phrasenkorrekturen vor jedem Einfügen", "Correcciones exactas de frases antes de cada pegado"]),
    ("Top", ["Góra", "顶部", "上", "Oben", "Arriba"]),
    ("Bottom", ["Dół", "底部", "下", "Unten", "Abajo"]),
    ("Off", ["Wył.", "关", "オフ", "Aus", "No"]),
    ("Disabled", ["Wyłączone", "已禁用", "無効", "Deaktiviert", "Desactivado"]),
    ("Automatic", ["Automatyczny", "自动", "自動", "Automatisch", "Automático"]),
    ("24 hours", ["24 godziny", "24 小时", "24 時間", "24 Stunden", "24 horas"]),
    ("7 days", ["7 dni", "7 天", "7 日間", "7 Tage", "7 días"]),
    ("30 days", ["30 dni", "30 天", "30 日間", "30 Tage", "30 días"]),
    ("Forever", ["Bezterminowo", "永久", "無期限", "Unbegrenzt", "Para siempre"]),
    ("Hold {} to dictate", ["Przytrzymaj {}, aby dyktować", "按住 {} 开始听写", "{} を押し続けて話す", "{} halten zum Diktieren", "Mantén {} para dictar"]),
    ("Update to {}", ["Aktualizuj do {}", "更新到 {}", "{} に更新", "Auf {} aktualisieren", "Actualizar a {}"]),
    ("{} rules", ["Reguły: {}", "{} 条规则", "{} 件のルール", "{} Regeln", "{} reglas"]),
    ("{}s ago", ["{} s temu", "{} 秒前", "{} 秒前", "vor {} s", "hace {} s"]),
    ("{}m ago", ["{} min temu", "{} 分钟前", "{} 分前", "vor {} min", "hace {} min"]),
    ("{}h ago", ["{} godz. temu", "{} 小时前", "{} 時間前", "vor {} Std.", "hace {} h"]),
    ("{}d ago", ["{} dni temu", "{} 天前", "{} 日前", "vor {} Tagen", "hace {} días"]),
    ("No replacements yet. Add one to correct recurring phrases.", ["Brak zamian. Dodaj pierwszą, aby poprawiać powtarzające się frazy.", "还没有替换规则。添加一条以纠正常见短语。", "置換はまだありません。よく使うフレーズの修正を追加しましょう。", "Noch keine Ersetzungen. Fügen Sie eine hinzu, um wiederkehrende Phrasen zu korrigieren.", "Aún no hay sustituciones. Añade una para corregir frases recurrentes."]),
    ("History is off. New dictations are not retained.", ["Historia jest wyłączona. Nowe dyktowania nie są zapisywane.", "历史记录已关闭。不会保存新的听写。", "履歴はオフです。新しい音声入力は保存されません。", "Verlauf ist aus. Neue Diktate werden nicht gespeichert.", "El historial está desactivado. No se guardan nuevos dictados."]),
    ("No dictations retained yet.", ["Brak zapisanych dyktowań.", "尚无保存的听写。", "保存された音声入力はまだありません。", "Noch keine Diktate gespeichert.", "Aún no hay dictados guardados."]),
    ("Select a history entry.", ["Wybierz wpis z historii.", "选择一条历史记录。", "履歴の項目を選択してください。", "Wählen Sie einen Verlaufseintrag.", "Selecciona una entrada del historial."]),
    ("History could not be loaded.", ["Nie udało się wczytać historii.", "无法加载历史记录。", "履歴を読み込めませんでした。", "Verlauf konnte nicht geladen werden.", "No se pudo cargar el historial."]),
    ("Text replacements could not be saved.", ["Nie udało się zapisać zamian tekstu.", "无法保存文本替换。", "テキスト置換を保存できませんでした。", "Textersetzungen konnten nicht gespeichert werden.", "No se pudieron guardar las sustituciones."]),
    ("HEX could not apply this change.", ["HEX nie mógł zastosować tej zmiany.", "HEX 无法应用此更改。", "HEX はこの変更を適用できませんでした。", "HEX konnte diese Änderung nicht übernehmen.", "HEX no pudo aplicar este cambio."]),
    ("Microphones could not be enumerated.", ["Nie udało się wykryć mikrofonów.", "无法枚举麦克风。", "マイクを列挙できませんでした。", "Mikrofone konnten nicht ermittelt werden.", "No se pudieron detectar los micrófonos."]),
    ("No models installed yet. Use the download button to install one.", ["Brak zainstalowanych modeli. Użyj przycisku pobierania, aby zainstalować.", "尚未安装模型。请使用下载按钮安装。", "モデルが未インストールです。ダウンロードボタンから追加してください。", "Noch keine Modelle installiert. Nutzen Sie den Download-Button.", "Aún no hay modelos instalados. Usa el botón de descarga."]),
    ("Raw transcript", ["Surowa transkrypcja", "原始转写", "生の文字起こし", "Rohtranskript", "Transcripción sin procesar"]),
    ("Choose dictation shortcut", ["Wybierz skrót dyktowania", "选择听写快捷键", "音声入力ショートカットを選択", "Diktat-Kürzel wählen", "Elige el atajo de dictado"]),
    ("Hold the shortcut to record; release it to transcribe and paste.", ["Przytrzymaj skrót, aby nagrywać; puść, aby przepisać i wkleić.", "按住快捷键录音；松开即转写并粘贴。", "押している間録音し、離すと文字起こしして貼り付けます。", "Kürzel halten zum Aufnehmen; loslassen zum Transkribieren und Einfügen.", "Mantén el atajo para grabar; suéltalo para transcribir y pegar."]),
    ("Recommended Windows push-to-talk shortcut", ["Zalecany skrót push-to-talk dla Windows", "推荐的 Windows 按键通话快捷键", "推奨の Windows プッシュトゥトーク", "Empfohlenes Push-to-talk-Kürzel", "Atajo pulsar-para-hablar recomendado"]),
    ("Three-key fallback for keyboards without a Windows key", ["Alternatywa dla klawiatur bez klawisza Windows", "适用于无 Win 键键盘的备选组合", "Windows キーがないキーボード向けの代替", "Alternative für Tastaturen ohne Windows-Taste", "Alternativa para teclados sin tecla Windows"]),
    ("Escape still cancels the active recording.", ["Escape nadal anuluje aktywne nagrywanie.", "Esc 仍可取消当前录音。", "Esc で録音をキャンセルできます。", "Escape bricht die Aufnahme weiterhin ab.", "Escape sigue cancelando la grabación."]),
    ("Current", ["Bieżący", "当前", "現在", "Aktuell", "Actual"]),
    ("Ready", ["Gotowy", "就绪", "準備完了", "Bereit", "Listo"]),
    ("Listening", ["Nasłuchuje", "正在监听", "リッスン中", "Hört zu", "Escuchando"]),
    ("Starting", ["Uruchamianie", "启动中", "起動中", "Startet", "Iniciando"]),
    ("Stopping", ["Zatrzymywanie", "停止中", "停止中", "Stoppt", "Deteniendo"]),
    ("Model required", ["Wymagany model", "需要模型", "モデルが必要", "Modell erforderlich", "Se requiere modelo"]),
    ("Preparing model", ["Przygotowywanie modelu", "正在准备模型", "モデルを準備中", "Modell wird vorbereitet", "Preparando modelo"]),
    ("Applying settings", ["Stosowanie ustawień", "正在应用设置", "設定を適用中", "Einstellungen werden übernommen", "Aplicando ajustes"]),
    ("Unavailable", ["Niedostępny", "不可用", "利用不可", "Nicht verfügbar", "No disponible"]),
    ("Recording", ["Nagrywanie", "录音中", "録音中", "Aufnahme", "Grabando"]),
    ("Transcribing", ["Transkrypcja", "转写中", "文字起こし中", "Transkribiert", "Transcribiendo"]),
];

#[cfg(test)]
mod tests {
    use super::TRANSLATIONS;

    #[test]
    fn templates_keep_their_placeholder_in_every_language() {
        for (english, translated) in TRANSLATIONS {
            if english.contains("{}") {
                for translation in translated {
                    assert!(
                        translation.contains("{}"),
                        "translation of {english:?} lost its placeholder: {translation:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn keys_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for (english, _) in TRANSLATIONS {
            assert!(
                seen.insert(english),
                "duplicate translation key {english:?}"
            );
        }
    }
}
