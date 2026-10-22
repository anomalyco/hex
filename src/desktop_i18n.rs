//! UI translations for the port shells, keyed by the English source
//! string. The language follows the system locale unless a persisted
//! interface-language choice overrides it; unknown locales and
//! untranslated strings fall back to English. Composite strings are `{}`
//! templates filled through [`tr_fill`].

use std::collections::HashMap;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU8, Ordering};

/// The selectable interface languages: persisted code and native name. The
/// leading `None` entry follows the system display language.
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
/// system locale. Safe to call again whenever the setting changes.
pub(crate) fn apply(setting: Option<&str>) {
    let code = match setting {
        Some(code) => column_for_code(code),
        None => column_for_code(&system_language()),
    };
    CURRENT.store(code, Ordering::Relaxed);
}

pub(crate) fn system_language() -> String {
    #[cfg(target_os = "windows")]
    {
        windows_user_language()
    }
    #[cfg(not(target_os = "windows"))]
    {
        language_from_env(
            std::env::var("LC_ALL").ok().as_deref(),
            std::env::var("LC_MESSAGES").ok().as_deref(),
            std::env::var("LANG").ok().as_deref(),
        )
    }
}

/// The person's first preferred language, for choosing a speech model. On
/// macOS this is the system's language list, which apps opened from Finder
/// see without `LANG`.
pub(crate) fn preferred_language() -> String {
    #[cfg(target_os = "macos")]
    if let Some(language) = macos_preferred_language() {
        return language;
    }
    system_language()
}

#[cfg(target_os = "macos")]
fn macos_preferred_language() -> Option<String> {
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::AnyClass;
    use objc2_foundation::{NSArray, NSString};

    let class = AnyClass::get(c"NSLocale")?;
    let languages: Option<Retained<NSArray<NSString>>> =
        unsafe { msg_send![class, preferredLanguages] };
    let first = languages?.firstObject()?.to_string();
    primary_subtag(&first)
}

/// `pt-BR` and `zh-Hans-CN` yield `pt` and `zh`.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn primary_subtag(tag: &str) -> Option<String> {
    let subtag = tag.split(['-', '_']).next()?.trim().to_ascii_lowercase();
    (!subtag.is_empty()).then_some(subtag)
}

/// The primary subtag of the Windows user locale: `pl-PL` yields `pl`.
#[cfg(target_os = "windows")]
fn windows_user_language() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    const LOCALE_NAME_CAPACITY: usize = 85;
    let mut buffer = [0_u16; LOCALE_NAME_CAPACITY];
    let length = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 1 {
        return "en".into();
    }
    let locale = String::from_utf16_lossy(&buffer[..length as usize - 1]);
    locale
        .split(['-', '_'])
        .next()
        .unwrap_or("en")
        .to_ascii_lowercase()
}

/// The primary language subtag from POSIX locale variables, in their
/// standard precedence: `pl_PL.UTF-8` yields `pl`.
#[cfg_attr(target_os = "windows", allow(dead_code))]
fn language_from_env(
    lc_all: Option<&str>,
    lc_messages: Option<&str>,
    lang: Option<&str>,
) -> String {
    [lc_all, lc_messages, lang]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty() && *value != "C" && *value != "POSIX")
        .map(|value| {
            value
                .chars()
                .take_while(|character| character.is_ascii_alphabetic())
                .collect::<String>()
                .to_lowercase()
        })
        .unwrap_or_default()
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

/// Translate a template with named `{name}` placeholders and fill them, so
/// each language can order the values as its grammar needs.
pub(crate) fn tr_named(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(tr(template).to_string(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

/// A compact, translated age label for an event timestamp.
pub(crate) fn event_age(timestamp_ms: u64) -> String {
    let seconds = crate::history::now_ms().saturating_sub(timestamp_ms) / 1_000;
    let (template, value) = match seconds {
        0..=59 => ("{}s ago", seconds),
        60..=3_599 => ("{}m ago", seconds / 60),
        3_600..=86_399 => ("{}h ago", seconds / 3_600),
        _ => ("{}d ago", seconds / 86_400),
    };
    tr_fill(template, &value.to_string())
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
    ("Desktop shortcuts", ["Skróty pulpitu", "桌面快捷键", "デスクトップのショートカット", "Desktop-Tastenkürzel", "Atajos del escritorio"]),
    ("Granted by your desktop", ["Przyznany przez pulpit", "由桌面授予", "デスクトップが割り当て", "Vom Desktop vergeben", "Concedido por tu escritorio"]),
    ("Not granted", ["Nie przyznano", "未授予", "未割り当て", "Nicht vergeben", "No concedido"]),
    ("Your desktop did not grant every shortcut. Record another one, or free it in your desktop's keyboard settings.", ["Pulpit nie przyznał wszystkich skrótów. Nagraj inny albo zwolnij go w ustawieniach klawiatury pulpitu.", "桌面未授予所有快捷键。请录制其他快捷键，或在桌面的键盘设置中释放它。", "デスクトップがすべてのショートカットを割り当てませんでした。別のショートカットを記録するか、デスクトップのキーボード設定で空けてください。", "Der Desktop hat nicht jedes Tastenkürzel vergeben. Nimm ein anderes auf oder gib es in den Tastatureinstellungen des Desktops frei.", "Tu escritorio no concedió todos los atajos. Graba otro o libéralo en los ajustes de teclado del escritorio."]),
    ("Your desktop has no shortcuts portal. Bind these commands in its keyboard settings.", ["Pulpit nie ma portalu skrótów. Przypisz te polecenia w jego ustawieniach klawiatury.", "你的桌面没有快捷键门户。请在其键盘设置中绑定这些命令。", "デスクトップにショートカットポータルがありません。キーボード設定でこれらのコマンドを割り当ててください。", "Dein Desktop hat kein Tastenkürzel-Portal. Weise diese Befehle in seinen Tastatureinstellungen zu.", "Tu escritorio no tiene portal de atajos. Asigna estos comandos en sus ajustes de teclado."]),
    ("Hold to dictate", ["Przytrzymaj, aby dyktować", "按住听写", "押している間音声入力", "Halten zum Diktieren", "Mantén pulsado para dictar"]),
    ("Bind start to the key press and stop to its release", ["Przypisz start do naciśnięcia klawisza, a stop do jego zwolnienia", "将 start 绑定到按下按键，将 stop 绑定到松开按键", "start をキーを押したとき、stop を離したときに割り当て", "start an das Drücken und stop an das Loslassen der Taste binden", "Asigna start al pulsar la tecla y stop al soltarla"]),
    ("Toggle dictation", ["Przełącz dyktowanie", "切换听写", "音声入力を切り替え", "Diktat umschalten", "Alternar dictado"]),
    ("Cancel dictation", ["Anuluj dyktowanie", "取消听写", "音声入力をキャンセル", "Diktat abbrechen", "Cancelar dictado"]),
    ("Discard the current recording", ["Odrzuć bieżące nagranie", "丢弃当前录音", "現在の録音を破棄", "Aktuelle Aufnahme verwerfen", "Descartar la grabación actual"]),
    ("Ready anywhere · Use your desktop's shortcuts to dictate", ["Gotowe wszędzie · Dyktuj skrótami pulpitu", "随处可用 · 使用桌面快捷键听写", "どこでも利用可能 · デスクトップのショートカットで音声入力", "Überall bereit · Diktiere mit den Tastenkürzeln des Desktops", "Listo en cualquier lugar · Dicta con los atajos del escritorio"]),
    ("Language", ["Język", "语言", "言語", "Sprache", "Idioma"]),
    ("The language you dictate in; Auto detects it", ["Język dyktowania; Auto wykrywa automatycznie", "听写语言；Auto 自动检测", "音声入力の言語。Auto は自動検出", "Diktatsprache; Auto erkennt automatisch", "Idioma del dictado; Auto lo detecta"]),
    ("Interface language", ["Język interfejsu", "界面语言", "表示言語", "Sprache der Oberfläche", "Idioma de la interfaz"]),
    ("Language of the Hex interface", ["Język interfejsu Hex", "Hex 界面语言", "Hex の表示言語", "Sprache der Hex-Oberfläche", "Idioma de la interfaz de Hex"]),
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
    ("Streams", ["Strumieniowy", "流式", "ストリーミング", "Streamt", "En streaming"]),
    ("Transcribes while you speak", ["Transkrybuje w trakcie mówienia", "边说边转写", "話しながら文字起こし", "Transkribiert, während Sie sprechen", "Transcribe mientras hablas"]),
    ("Transcribes while you speak on a GPU", ["Transkrybuje w trakcie mówienia na GPU", "使用 GPU 时边说边转写", "GPU で話しながら文字起こし", "Transkribiert auf einer GPU, während Sie sprechen", "Transcribe mientras hablas en una GPU"]),
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
    ("Voice Action", ["Voice Action", "Voice Action", "Voice Action", "Voice Action", "Voice Action"]),
    ("Application", ["Aplikacja", "应用", "アプリ", "Anwendung", "Aplicación"]),
    ("Problem", ["Problem", "问题", "問題", "Problem", "Problema"]),
    ("Default mode", ["Tryb domyślny", "默认模式", "デフォルトモード", "Standardmodus", "Modo predeterminado"]),
    ("Global", ["Globalny", "全局", "グローバル", "Global", "Global"]),
    ("Fallback for everything", ["Tryb zapasowy dla wszystkiego", "其他所有情况的后备模式", "その他すべてのフォールバック", "Fallback für alles", "Alternativa para todo"]),
    ("No activation rules", ["Brak reguł aktywacji", "无激活规则", "有効化ルールなし", "Keine Aktivierungsregeln", "Sin reglas de activación"]),
    ("Untitled mode", ["Tryb bez nazwy", "未命名模式", "名称未設定のモード", "Unbenannter Modus", "Modo sin título"]),
    ("Used unless a more specific mode matches.", ["Używany, jeśli nie pasuje bardziej szczegółowy tryb.", "在没有更具体的模式匹配时使用。", "より具体的なモードが一致しない場合に使用します。", "Wird verwendet, wenn kein spezifischerer Modus passt.", "Se usa si no coincide un modo más específico."]),
    ("Text processing", ["Przetwarzanie tekstu", "文本处理", "テキスト処理", "Textverarbeitung", "Procesamiento de texto"]),
    ("Name", ["Nazwa", "名称", "名前", "Name", "Nombre"]),
    ("Applications", ["Aplikacje", "应用", "アプリケーション", "Anwendungen", "Aplicaciones"]),
    ("Web pages", ["Strony internetowe", "网页", "ウェブページ", "Webseiten", "Páginas web"]),
    ("Switch when an app is frontmost", ["Przełącz, gdy aplikacja jest na pierwszym planie", "当前台应用匹配时切换", "アプリが最前面のときに切り替え", "Wechseln, wenn eine App im Vordergrund ist", "Cambiar cuando una aplicación esté en primer plano"]),
    ("Switch when a domain is active in Brave", ["Przełącz, gdy domena jest aktywna w Brave", "当域名在 Brave 中处于活动状态时切换", "Brave でドメインが開かれているときに切り替え", "Wechseln, wenn eine Domain in Brave aktiv ist", "Cambiar cuando un dominio esté activo en Brave"]),
    ("Done", ["Gotowe", "完成", "完了", "Fertig", "Listo"]),
    ("Replacements", ["Zamiany", "替换", "置換", "Ersetzungen", "Sustituciones"]),
    ("Corrections", ["Korekty", "更正", "修正", "Korrekturen", "Correcciones"]),
    ("Transcription", ["Transkrypcja", "转写", "文字起こし", "Transkription", "Transcripción"]),
    ("Output", ["Wynik", "输出", "出力", "Ausgabe", "Salida"]),
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
    ("OpenCode not found", ["Nie znaleziono OpenCode", "未找到 OpenCode", "OpenCode が見つかりません", "OpenCode nicht gefunden", "OpenCode no encontrado"]),
    ("Hold the shortcut and speak an instruction. Hex sends it, along with any text you have selected, to OpenCode and pastes the reply at your cursor. If the model returns nothing, nothing is pasted.", ["Przytrzymaj skrót i podaj polecenie. Hex wyśle je wraz z zaznaczonym tekstem do OpenCode i wklei odpowiedź przy kursorze. Jeśli model nic nie zwróci, nic nie zostanie wklejone.", "按住快捷键说出指令。Hex 会将其与选中文本一起发送给 OpenCode，并把回复粘贴到光标处。如果模型没有返回内容，则不会粘贴任何内容。", "ショートカットを押しながら指示を話します。Hex は選択中のテキストとともに OpenCode へ送信し、応答をカーソル位置に貼り付けます。モデルが何も返さなければ何も貼り付けません。", "Kürzel halten und eine Anweisung sprechen. Hex sendet sie mit dem ausgewählten Text an OpenCode und fügt die Antwort am Cursor ein. Liefert das Modell nichts, wird nichts eingefügt.", "Mantén el atajo y di una instrucción. Hex la envía junto al texto seleccionado a OpenCode y pega la respuesta en el cursor. Si el modelo no devuelve nada, no se pega nada."]),
    ("Capture", ["Nagrywanie", "采集", "キャプチャ", "Aufnahme", "Captura"]),
    ("Processing", ["Przetwarzanie", "处理", "処理", "Verarbeitung", "Procesamiento"]),
    ("Shortcut", ["Skrót", "快捷键", "ショートカット", "Kürzel", "Atajo"]),
    ("Hold to speak; selected text is included automatically", ["Przytrzymaj i mów; zaznaczony tekst dołącza się automatycznie", "按住说话；选中的文本会自动附带", "押しながら話す。選択中のテキストは自動で含まれます", "Halten und sprechen; ausgewählter Text wird automatisch mitgesendet", "Mantén pulsado y habla; el texto seleccionado se incluye automáticamente"]),
    ("Press the new shortcut, Esc cancels", ["Naciśnij nowy skrót, Esc anuluje", "按下新快捷键，Esc 取消", "新しいショートカットを押してください。Esc でキャンセル", "Neues Tastenkürzel drücken, Esc bricht ab", "Pulsa el nuevo atajo, Esc cancela"]),
    ("OpenCode", ["OpenCode", "OpenCode", "OpenCode", "OpenCode", "OpenCode"]),
    ("Voice actions run through your local OpenCode install", ["Akcje głosowe działają przez lokalną instalację OpenCode", "语音操作通过本地安装的 OpenCode 运行", "音声アクションはローカルの OpenCode を通じて実行されます", "Sprachaktionen laufen über die lokale OpenCode-Installation", "Las acciones de voz usan tu instalación local de OpenCode"]),
    ("Fulfils each voice action; served by OpenCode", ["Realizuje każdą akcję głosową; dostarczany przez OpenCode", "执行每次语音操作；由 OpenCode 提供", "各音声アクションを実行します。OpenCode が提供", "Führt jede Sprachaktion aus; bereitgestellt von OpenCode", "Ejecuta cada acción de voz; servido por OpenCode"]),
    ("Loading models", ["Wczytywanie modeli", "正在加载模型", "モデルを読み込み中", "Modelle werden geladen", "Cargando modelos"]),
    ("Choose a model", ["Wybierz model", "选择模型", "モデルを選択", "Modell wählen", "Elige un modelo"]),
    ("Models could not be loaded: {}", ["Nie udało się wczytać modeli: {}", "无法加载模型：{}", "モデルを読み込めませんでした: {}", "Modelle konnten nicht geladen werden: {}", "No se pudieron cargar los modelos: {}"]),
    ("Default", ["Domyślny", "默认", "既定", "Standard", "Predeterminado"]),
    ("Last dictation failed: {}", ["Ostatnie dyktowanie nie powiodło się: {}", "上次听写失败：{}", "前回の音声入力に失敗しました: {}", "Letztes Diktat fehlgeschlagen: {}", "El último dictado falló: {}"]),
    ("Preparing {}", ["Przygotowywanie {}", "正在准备 {}", "{} を準備中", "{} wird vorbereitet", "Preparando {}"]),
    ("Restart", ["Uruchom ponownie", "重启", "再起動", "Neu starten", "Reiniciar"]),
    ("Version {}", ["Wersja {}", "版本 {}", "バージョン {}", "Version {}", "Versión {}"]),
    ("Private local dictation for Linux", ["Prywatne lokalne dyktowanie dla Linuksa", "适用于 Linux 的私密本地听写", "Linux のためのプライベートなローカル音声入力", "Privates lokales Diktat für Linux", "Dictado local y privado para Linux"]),
    ("Hold to dictate, release to transcribe", ["Przytrzymaj, aby dyktować; puść, aby przepisać", "按住听写，松开转写", "押しながら話し、離すと文字起こし", "Halten zum Diktieren, loslassen zum Transkribieren", "Mantén para dictar, suelta para transcribir"]),
    ("Double-tap the shortcut for hands-free dictation", ["Dwukrotnie naciśnij skrót, aby dyktować bez trzymania", "双击快捷键进行免持听写", "ショートカットを2回押すとハンズフリーで音声入力", "Kürzel zweimal tippen für freihändiges Diktat", "Pulsa dos veces el atajo para dictar sin manos"]),
    ("Update ready", ["Aktualizacja gotowa", "更新已就绪", "アップデート準備完了", "Update bereit", "Actualización lista"]),
    ("Restart into the verified Linux release.", ["Uruchom ponownie do zweryfikowanego wydania Linuksa.", "重启进入已验证的 Linux 版本。", "検証済みの Linux リリースへ再起動します。", "In die verifizierte Linux-Version neu starten.", "Reinicia a la versión verificada de Linux."]),
    ("Uses the selected input or the system default", ["Używa wybranego wejścia lub domyślnego systemowego", "使用所选输入或系统默认设备", "選択した入力またはシステム既定を使用", "Verwendet den gewählten Eingang oder den Systemstandard", "Usa la entrada seleccionada o la predeterminada del sistema"]),
    ("Press a shortcut...", ["Naciśnij skrót...", "按下快捷键...", "ショートカットを押してください...", "Kürzel drücken...", "Pulsa un atajo..."]),
    ("The transcription change did not apply.", ["Zmiana transkrypcji nie została zastosowana.", "转写更改未生效。", "文字起こしの変更が適用されませんでした。", "Die Transkriptionsänderung wurde nicht übernommen.", "El cambio de transcripción no se aplicó."]),
    ("Automatic microphone", ["Automatyczny mikrofon", "自动麦克风", "自動マイク", "Automatisches Mikrofon", "Micrófono automático"]),
    ("Welcome to Hex", ["Witaj w Hex", "欢迎使用 Hex", "Hex へようこそ", "Willkommen bei Hex", "Bienvenido a Hex"]),
    ("Dictate into any application. Audio never leaves this computer; one speech model download is all it takes.", ["Dyktuj w dowolnej aplikacji. Dźwięk nigdy nie opuszcza tego komputera; wystarczy pobrać jeden model mowy.", "在任何应用中听写。音频绝不离开这台电脑；只需下载一个语音模型。", "どのアプリでも音声入力できます。音声はこのコンピューターから出ません。音声モデルを一つダウンロードするだけです。", "Diktiere in jede Anwendung. Audio verlässt diesen Computer nie; ein Sprachmodell-Download genügt.", "Dicta en cualquier aplicación. El audio nunca sale de este equipo; basta con descargar un modelo de voz."]),
    ("Dictation language", ["Język dyktowania", "听写语言", "音声入力の言語", "Diktatsprache", "Idioma de dictado"]),
    ("Download {}", ["Pobierz {}", "下载 {}", "{} をダウンロード", "{} herunterladen", "Descargar {}"]),
    ("Start dictating", ["Zacznij dyktować", "开始听写", "音声入力を開始", "Diktat starten", "Empezar a dictar"]),
    ("Set up later", ["Skonfiguruj później", "稍后设置", "後で設定", "Später einrichten", "Configurar más tarde"]),
    ("Hold", ["Przytrzymaj", "按住", "押しながら", "Halte", "Mantén"]),
    ("to dictate", ["aby dyktować", "即可听写", "で音声入力", "zum Diktieren", "para dictar"]),
    ("Activity", ["Aktywność", "活动", "アクティビティ", "Aktivität", "Actividad"]),
    ("Session", ["Sesja", "会话", "セッション", "Sitzung", "Sesión"]),
    ("Session started {}", ["Sesja rozpoczęta {}", "会话开始于 {}", "セッション開始: {}", "Sitzung gestartet {}", "Sesión iniciada {}"]),
    ("No session recorded yet", ["Nie zarejestrowano jeszcze sesji", "尚未记录会话", "まだセッションが記録されていません", "Noch keine Sitzung aufgezeichnet", "Aún no hay sesiones registradas"]),
    ("Recent transcripts", ["Ostatnie transkrypcje", "最近的转写", "最近の文字起こし", "Letzte Transkripte", "Transcripciones recientes"]),
    ("Nothing transcribed this session yet.", ["W tej sesji jeszcze nic nie przepisano.", "本次会话尚未转写任何内容。", "このセッションではまだ何も文字起こしされていません。", "In dieser Sitzung wurde noch nichts transkribiert.", "Aún no se ha transcrito nada en esta sesión."]),
    ("Observations", ["Obserwacje", "观测记录", "観測ログ", "Beobachtungen", "Observaciones"]),
    ("Event log", ["Dziennik zdarzeń", "事件日志", "イベントログ", "Ereignisprotokoll", "Registro de eventos"]),
    ("Newest-first session observations on this disk", ["Obserwacje sesji od najnowszych, na tym dysku", "本磁盘上按最新排序的会话观测", "このディスク上の新しい順のセッション観測", "Sitzungsbeobachtungen auf dieser Festplatte, neueste zuerst", "Observaciones de sesión en este disco, primero las más recientes"]),
    ("The device the active session captures from", ["Urządzenie, z którego nagrywa aktywna sesja", "当前会话采集所用的设备", "アクティブなセッションが使用する入力デバイス", "Das Gerät, von dem die aktive Sitzung aufnimmt", "El dispositivo del que captura la sesión activa"]),
    ("Voice commands", ["Polecenia głosowe", "语音命令", "音声コマンド", "Sprachbefehle", "Comandos de voz"]),
    ("Opt-in wake-word commands through the local Moonshine model; applies when listening starts", ["Opcjonalne polecenia ze słowem wybudzającym przez lokalny model Moonshine; działa od startu nasłuchu", "通过本地 Moonshine 模型的自选唤醒词命令；在开始监听时生效", "ローカルの Moonshine モデルによるオプトインのウェイクワードコマンド。リッスン開始時に適用", "Opt-in-Weckwort-Befehle über das lokale Moonshine-Modell; gilt ab dem Start des Zuhörens", "Comandos con palabra de activación mediante el modelo local Moonshine; se aplica al empezar a escuchar"]),
    ("Or when the browser is on one of these sites; sites win over applications", ["Albo gdy przeglądarka jest na jednej z tych stron; strony wygrywają z aplikacjami", "或当浏览器位于这些网站之一时；网站优先于应用", "またはブラウザーがこれらのサイトにあるとき。サイトがアプリより優先されます", "Oder wenn der Browser auf einer dieser Seiten ist; Seiten schlagen Anwendungen", "O cuando el navegador está en uno de estos sitios; los sitios ganan a las aplicaciones"]),
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
    ("Checks for new Hex releases in the background", ["Sprawdza nowe wydania Hex w tle", "在后台检查 Hex 的新版本", "バックグラウンドで Hex の新しいリリースを確認", "Prüft im Hintergrund auf neue Hex-Versionen", "Busca nuevas versiones de Hex en segundo plano"]),
    ("Restart into {}", ["Uruchom ponownie do {}", "重启进入 {}", "再起動して {} へ", "Neu starten in {}", "Reiniciar a {}"]),
    ("Check now", ["Sprawdź teraz", "立即检查", "今すぐ確認", "Jetzt prüfen", "Comprobar ahora"]),
    ("Checking", ["Sprawdzanie", "检查中", "確認中", "Wird geprüft", "Comprobando"]),
    ("Launch at login", ["Uruchamiaj przy logowaniu", "登录时启动", "ログイン時に起動", "Beim Anmelden starten", "Iniciar al iniciar sesión"]),
    ("Start Hex hidden in the Windows system tray after sign-in", ["Uruchamia Hex ukryty w zasobniku systemowym po zalogowaniu", "登录后在系统托盘中隐藏启动 Hex", "サインイン後、Hex をシステムトレイに隠して起動", "Startet Hex nach der Anmeldung versteckt im Infobereich", "Inicia Hex oculto en la bandeja del sistema tras iniciar sesión"]),
    ("Listen on launch", ["Nasłuchuj po uruchomieniu", "启动时监听", "起動時にリッスン", "Beim Start zuhören", "Escuchar al iniciar"]),
    ("Start the global dictation listener when Hex opens", ["Uruchamia globalne nasłuchiwanie dyktowania przy starcie Hex", "Hex 打开时启动全局听写监听", "Hex 起動時にグローバル音声入力リスナーを開始", "Startet den globalen Diktat-Listener beim Öffnen von Hex", "Inicia el escuchador global de dictado al abrir Hex"]),
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
    ("No corrections in this mode.", ["Brak korekt w tym trybie.", "此模式中没有更正规则。", "このモードには修正がありません。", "Keine Korrekturen in diesem Modus.", "No hay correcciones en este modo."]),
    ("History is off. New dictations are not retained.", ["Historia jest wyłączona. Nowe dyktowania nie są zapisywane.", "历史记录已关闭。不会保存新的听写。", "履歴はオフです。新しい音声入力は保存されません。", "Verlauf ist aus. Neue Diktate werden nicht gespeichert.", "El historial está desactivado. No se guardan nuevos dictados."]),
    ("No dictations retained yet.", ["Brak zapisanych dyktowań.", "尚无保存的听写。", "保存された音声入力はまだありません。", "Noch keine Diktate gespeichert.", "Aún no hay dictados guardados."]),
    ("Select a history entry.", ["Wybierz wpis z historii.", "选择一条历史记录。", "履歴の項目を選択してください。", "Wählen Sie einen Verlaufseintrag.", "Selecciona una entrada del historial."]),
    ("History could not be loaded.", ["Nie udało się wczytać historii.", "无法加载历史记录。", "履歴を読み込めませんでした。", "Verlauf konnte nicht geladen werden.", "No se pudo cargar el historial."]),
    ("Text replacements could not be saved.", ["Nie udało się zapisać zamian tekstu.", "无法保存文本替换。", "テキスト置換を保存できませんでした。", "Textersetzungen konnten nicht gespeichert werden.", "No se pudieron guardar las sustituciones."]),
    ("Hex could not apply this change.", ["Hex nie mógł zastosować tej zmiany.", "Hex 无法应用此更改。", "Hex はこの変更を適用できませんでした。", "Hex konnte diese Änderung nicht übernehmen.", "Hex no pudo aplicar este cambio."]),
    ("Microphones could not be enumerated.", ["Nie udało się wykryć mikrofonów.", "无法枚举麦克风。", "マイクを列挙できませんでした。", "Mikrofone konnten nicht ermittelt werden.", "No se pudieron detectar los micrófonos."]),
    ("No models installed yet. Use the download button to install one.", ["Brak zainstalowanych modeli. Użyj przycisku pobierania, aby zainstalować.", "尚未安装模型。请使用下载按钮安装。", "モデルが未インストールです。ダウンロードボタンから追加してください。", "Noch keine Modelle installiert. Nutzen Sie den Download-Button.", "Aún no hay modelos instalados. Usa el botón de descarga."]),
    ("Raw transcript", ["Surowa transkrypcja", "原始转写", "生の文字起こし", "Rohtranskript", "Transcripción sin procesar"]),
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
    ("Add", ["Dodaj", "添加", "追加", "Hinzufügen", "Añadir"]),
    ("Add Application", ["Dodaj aplikację", "添加应用", "アプリを追加", "App hinzufügen", "Añadir aplicación"]),
    ("Add a key", ["Dodaj klawisz", "添加一个按键", "キーを追加してください", "Taste hinzufügen", "Añade una tecla"]),
    ("Add a key Linux can register", ["Dodaj klawisz, który Linux może zarejestrować", "添加一个 Linux 可以注册的按键", "Linux で登録できるキーを追加してください", "Taste hinzufügen, die Linux registrieren kann", "Añade una tecla que Linux pueda registrar"]),
    ("Add a modifier", ["Dodaj modyfikator", "添加一个修饰键", "修飾キーを追加してください", "Sondertaste hinzufügen", "Añade un modificador"]),
    ("Already in use", ["Już w użyciu", "已被使用", "すでに使用されています", "Bereits belegt", "Ya está en uso"]),
    ("Behavior", ["Zachowanie", "行为", "動作", "Verhalten", "Comportamiento"]),
    ("Change shortcut", ["Zmień skrót", "更改快捷键", "ショートカットを変更", "Kürzel ändern", "Cambiar atajo"]),
    ("Check again", ["Sprawdź ponownie", "再次检查", "再確認", "Erneut prüfen", "Comprobar de nuevo"]),
    ("Check failed; retrying later", ["Sprawdzanie nie powiodło się; ponowimy później", "检查失败；稍后重试", "確認に失敗しました。後で再試行します", "Prüfung fehlgeschlagen; später erneut", "La comprobación falló; se reintentará más tarde"]),
    ("Checking for OpenCode", ["Szukanie OpenCode", "正在查找 OpenCode", "OpenCode を確認中", "Suche nach OpenCode", "Buscando OpenCode"]),
    ("Checking…", ["Sprawdzanie…", "正在检查…", "確認中…", "Wird geprüft…", "Comprobando…"]),
    ("Choose a local model to start dictating.", ["Wybierz lokalny model, aby zacząć dyktować.", "选择一个本地模型即可开始听写。", "ローカルモデルを選ぶと音声入力を始められます。", "Wähle ein lokales Modell, um mit dem Diktieren zu beginnen.", "Elige un modelo local para empezar a dictar."]),
    ("Choose how much reasoning the model should use", ["Wybierz, ile rozumowania ma użyć model", "选择模型应使用多少推理", "モデルが使う推論量を選択", "Lege fest, wie viel der Modell nachdenken soll", "Elige cuánto razonamiento debe usar el modelo"]),
    ("Choose model", ["Wybierz model", "选择模型", "モデルを選択", "Modell wählen", "Elegir modelo"]),
    ("Clear", ["Wyczyść", "清除", "クリア", "Löschen", "Borrar"]),
    ("Click to record a new shortcut", ["Kliknij, aby nagrać nowy skrót", "点击以录制新的快捷键", "クリックして新しいショートカットを記録", "Klicken, um ein neues Kürzel aufzunehmen", "Haz clic para grabar un atajo nuevo"]),
    ("Confirm delete", ["Potwierdź usunięcie", "确认删除", "削除を確定", "Löschen bestätigen", "Confirmar eliminación"]),
    ("Could not save shortcut. Try again.", ["Nie udało się zapisać skrótu. Spróbuj ponownie.", "无法保存快捷键。请重试。", "ショートカットを保存できませんでした。もう一度お試しください。", "Kürzel konnte nicht gespeichert werden. Versuche es erneut.", "No se pudo guardar el atajo. Inténtalo de nuevo."]),
    ("Could not update Voice Action:", ["Nie udało się zaktualizować Voice Action:", "无法更新 Voice Action：", "Voice Action を更新できませんでした:", "Voice Action konnte nicht aktualisiert werden:", "No se pudo actualizar Voice Action:"]),
    ("Create a TypeScript workspace to define reusable transformations.", ["Utwórz obszar roboczy TypeScript, aby zdefiniować transformacje wielokrotnego użytku.", "创建一个 TypeScript 工作区来定义可复用的转换。", "TypeScript ワークスペースを作成して、再利用できる変換を定義します。", "Lege einen TypeScript-Arbeitsbereich an, um wiederverwendbare Transformationen zu definieren.", "Crea un espacio de trabajo de TypeScript para definir transformaciones reutilizables."]),
    ("Custom transformations", ["Własne transformacje", "自定义转换", "カスタム変換", "Eigene Transformationen", "Transformaciones personalizadas"]),
    ("Deadline", ["Limit czasu", "期限", "期限", "Zeitlimit", "Plazo"]),
    ("Delete mode", ["Usuń tryb", "删除模式", "モードを削除", "Modus löschen", "Eliminar modo"]),
    ("Delete mode…", ["Usuń tryb…", "删除模式…", "モードを削除…", "Modus löschen…", "Eliminar modo…"]),
    ("Describe how OpenCode should rewrite your dictation…", ["Opisz, jak OpenCode ma przepisać twoje dyktowanie…", "描述 OpenCode 应如何改写你的听写内容…", "OpenCode が音声入力をどう書き直すか説明してください…", "Beschreibe, wie OpenCode dein Diktat umschreiben soll…", "Describe cómo debe reescribir OpenCode tu dictado…"]),
    ("Dictation will be available once your local speech model is ready.", ["Dyktowanie będzie dostępne, gdy lokalny model mowy będzie gotowy.", "本地语音模型就绪后即可使用听写。", "ローカル音声モデルの準備ができると音声入力を使えます。", "Diktieren ist verfügbar, sobald dein lokales Sprachmodell bereit ist.", "El dictado estará disponible cuando tu modelo de voz local esté listo."]),
    ("Disable", ["Wyłącz", "停用", "無効にする", "Deaktivieren", "Desactivar"]),
    ("Download a speech model to start dictating", ["Pobierz model mowy, aby zacząć dyktować", "下载语音模型即可开始听写", "音声入力を始めるには音声モデルをダウンロードしてください", "Lade ein Sprachmodell herunter, um mit dem Diktieren zu beginnen", "Descarga un modelo de voz para empezar a dictar"]),
    ("Either", ["Dowolny", "任意", "どちらでも", "Beide", "Cualquiera"]),
    ("Enable", ["Włącz", "启用", "有効にする", "Aktivieren", "Activar"]),
    ("Enable Voice Action", ["Włącz Voice Action", "启用 Voice Action", "Voice Action を有効にする", "Voice Action aktivieren", "Activar Voice Action"]),
    ("Enable on launch", ["Włączaj przy uruchomieniu", "启动时启用", "起動時に有効にする", "Beim Start aktivieren", "Activar al iniciar"]),
    ("Every registered transformation is already included.", ["Wszystkie zarejestrowane transformacje są już dodane.", "所有已注册的转换都已包含。", "登録済みの変換はすべて追加されています。", "Alle registrierten Transformationen sind bereits enthalten.", "Todas las transformaciones registradas ya están incluidas."]),
    ("Final text", ["Tekst końcowy", "最终文本", "最終テキスト", "Endgültiger Text", "Texto final"]),
    ("Finding applications…", ["Wyszukiwanie aplikacji…", "正在查找应用…", "アプリを検索中…", "Apps werden gesucht…", "Buscando aplicaciones…"]),
    ("Grant permissions and choose a local model to start dictating.", ["Przyznaj uprawnienia i wybierz lokalny model, aby zacząć dyktować.", "授予权限并选择本地模型即可开始听写。", "権限を許可し、ローカルモデルを選ぶと音声入力を始められます。", "Erteile die Berechtigungen und wähle ein lokales Modell, um mit dem Diktieren zu beginnen.", "Concede los permisos y elige un modelo local para empezar a dictar."]),
    ("Hex could not load the OpenCode model catalog.", ["Hex nie mógł wczytać katalogu modeli OpenCode.", "Hex 无法加载 OpenCode 模型目录。", "Hex は OpenCode のモデル一覧を読み込めませんでした。", "Hex konnte den OpenCode-Modellkatalog nicht laden.", "Hex no pudo cargar el catálogo de modelos de OpenCode."]),
    ("Hex will open the microphone when you press the shortcut. This adds a start-up delay and removes pre-roll, so the beginning of speech may be missed if you speak right away.", ["Hex otworzy mikrofon po naciśnięciu skrótu. Dodaje to opóźnienie startu i wyłącza wstępne nagrywanie, więc początek wypowiedzi może zostać pominięty, jeśli zaczniesz mówić od razu.", "Hex 会在你按下快捷键时打开麦克风。这会增加启动延迟并移除预录音，因此如果你立即开始说话，开头部分可能会被漏掉。", "Hex はショートカットを押したときにマイクを開きます。起動に時間がかかりプリロールもなくなるため、すぐに話し始めると冒頭が録音されないことがあります。", "Hex öffnet das Mikrofon erst, wenn du das Kürzel drückst. Das verzögert den Start und entfernt die Vorlaufaufnahme, sodass der Anfang fehlen kann, wenn du sofort sprichst.", "Hex abrirá el micrófono cuando pulses el atajo. Esto añade un retraso al inicio y elimina la pregrabación, así que el comienzo puede perderse si hablas enseguida."]),
    ("Input device for dictation and commands", ["Urządzenie wejściowe do dyktowania i poleceń", "用于听写和命令的输入设备", "音声入力とコマンドに使う入力デバイス", "Eingabegerät für Diktat und Befehle", "Dispositivo de entrada para dictado y comandos"]),
    ("Install OpenCode", ["Zainstaluj OpenCode", "安装 OpenCode", "OpenCode をインストール", "OpenCode installieren", "Instalar OpenCode"]),
    ("Install and configure OpenCode to transform dictated text.", ["Zainstaluj i skonfiguruj OpenCode, aby przekształcać dyktowany tekst.", "安装并配置 OpenCode 以转换听写文本。", "OpenCode をインストールして設定すると、音声入力したテキストを変換できます。", "Installiere und konfiguriere OpenCode, um diktierten Text umzuwandeln.", "Instala y configura OpenCode para transformar el texto dictado."]),
    ("Install and configure OpenCode to turn spoken instructions into paste-ready text.", ["Zainstaluj i skonfiguruj OpenCode, aby zamieniać wypowiedziane polecenia w tekst gotowy do wklejenia.", "安装并配置 OpenCode，将口述指令转换为可直接粘贴的文本。", "OpenCode をインストールして設定すると、話した指示を貼り付けられるテキストに変換できます。", "Installiere und konfiguriere OpenCode, um gesprochene Anweisungen in einfügefertigen Text zu verwandeln.", "Instala y configura OpenCode para convertir instrucciones habladas en texto listo para pegar."]),
    ("Instructions", ["Instrukcje", "说明", "指示", "Anweisungen", "Instrucciones"]),
    ("Keep Hex in the menu bar when Settings is closed", ["Zostaw Hex na pasku menu po zamknięciu Ustawień", "关闭设置后将 Hex 保留在菜单栏", "設定を閉じても Hex をメニューバーに表示", "Hex in der Menüleiste behalten, wenn die Einstellungen geschlossen sind", "Mantener Hex en la barra de menús al cerrar Ajustes"]),
    ("Keep ready (fast)", ["Gotowy (szybko)", "保持就绪（快）", "常に待機（高速）", "Bereit halten (schnell)", "Listo (rápido)"]),
    ("Keeps the microphone open for instant start and pre-roll", ["Utrzymuje otwarty mikrofon dla natychmiastowego startu i wstępnego nagrywania", "保持麦克风打开，以便立即开始并预录音", "マイクを開いたままにし、すぐに開始してプリロールを使います", "Hält das Mikrofon offen für sofortigen Start und Vorlauf", "Mantiene el micrófono abierto para empezar al instante con pregrabación"]),
    ("Latency", ["Opóźnienie", "延迟", "レイテンシー", "Latenz", "Latencia"]),
    ("Left", ["Lewy", "左", "左", "Links", "Izquierda"]),
    ("Loading OpenCode…", ["Wczytywanie OpenCode…", "正在加载 OpenCode…", "OpenCode を読み込み中…", "OpenCode wird geladen…", "Cargando OpenCode…"]),
    ("Loading registered TypeScript transformations…", ["Wczytywanie zarejestrowanych transformacji TypeScript…", "正在加载已注册的 TypeScript 转换…", "登録済みの TypeScript 変換を読み込み中…", "Registrierte TypeScript-Transformationen werden geladen…", "Cargando las transformaciones de TypeScript registradas…"]),
    ("Local dictation model", ["Lokalny model dyktowania", "本地听写模型", "ローカル音声入力モデル", "Lokales Diktiermodell", "Modelo de dictado local"]),
    ("Local models", ["Modele lokalne", "本地模型", "ローカルモデル", "Lokale Modelle", "Modelos locales"]),
    ("Meeting recording requests Screen & System Audio Recording separately when you use it.", ["Nagrywanie spotkań poprosi osobno o nagrywanie ekranu i dźwięku systemu, gdy go użyjesz.", "使用会议录制时，会单独请求“屏幕与系统音频录制”权限。", "会議の録音を使うと、画面とシステムオーディオの収録の許可を別途求めます。", "Die Besprechungsaufnahme fragt bei der Nutzung separat nach Bildschirm- und Systemaudioaufnahme.", "La grabación de reuniones pide por separado la grabación de pantalla y audio del sistema cuando la usas."]),
    ("Meetings", ["Spotkania", "会议", "会議", "Besprechungen", "Reuniones"]),
    ("Microphone mode", ["Tryb mikrofonu", "麦克风模式", "マイクモード", "Mikrofonmodus", "Modo del micrófono"]),
    ("Mode name", ["Nazwa trybu", "模式名称", "モード名", "Modusname", "Nombre del modo"]),
    ("Mute or pause other audio during dictation", ["Wycisz lub wstrzymaj inne dźwięki podczas dyktowania", "听写时静音或暂停其他音频", "音声入力中はほかの音声をミュートまたは一時停止", "Andere Audiowiedergabe während des Diktierens stummschalten oder pausieren", "Silencia o pausa otros audios durante el dictado"]),
    ("Names and terms Whisper should expect, e.g. OpenCode, Effect…", ["Nazwy i terminy, których Whisper ma się spodziewać, np. OpenCode, Effect…", "Whisper 应识别的名称和术语，例如 OpenCode、Effect…", "Whisper が想定すべき名前や用語（例: OpenCode、Effect…）", "Namen und Begriffe, die Whisper erwarten soll, z. B. OpenCode, Effect…", "Nombres y términos que Whisper debe esperar, p. ej. OpenCode, Effect…"]),
    ("Names and terms to prime the speech model", ["Nazwy i terminy podpowiadane modelowi mowy", "用于提示语音模型的名称和术语", "音声モデルに与える名前や用語", "Namen und Begriffe als Hinweis für das Sprachmodell", "Nombres y términos para orientar al modelo de voz"]),
    ("No applications found.", ["Nie znaleziono aplikacji.", "未找到应用。", "アプリが見つかりません。", "Keine Apps gefunden.", "No se encontraron aplicaciones."]),
    ("Off by default. Its saved shortcut is not reserved until you enable Voice Action.", ["Domyślnie wyłączone. Zapisany skrót nie jest zajmowany, dopóki nie włączysz Voice Action.", "默认关闭。在你启用 Voice Action 之前，不会占用其已保存的快捷键。", "既定ではオフです。Voice Action を有効にするまで、保存したショートカットは予約されません。", "Standardmäßig aus. Das gespeicherte Kürzel wird erst belegt, wenn du Voice Action aktivierst.", "Desactivado de forma predeterminada. Su atajo guardado no se reserva hasta que actives Voice Action."]),
    ("Open OpenCode setup", ["Otwórz konfigurację OpenCode", "打开 OpenCode 设置", "OpenCode のセットアップを開く", "OpenCode-Einrichtung öffnen", "Abrir configuración de OpenCode"]),
    ("Open settings", ["Otwórz ustawienia", "打开设置", "設定を開く", "Einstellungen öffnen", "Abrir ajustes"]),
    ("Open setup", ["Otwórz konfigurację", "打开设置向导", "セットアップを開く", "Einrichtung öffnen", "Abrir configuración"]),
    ("OpenCode beta required", ["Wymagana beta OpenCode", "需要 OpenCode 测试版", "OpenCode ベータ版が必要です", "OpenCode-Beta erforderlich", "Se requiere OpenCode beta"]),
    ("OpenCode is unavailable", ["OpenCode jest niedostępny", "OpenCode 不可用", "OpenCode を利用できません", "OpenCode ist nicht verfügbar", "OpenCode no está disponible"]),
    ("OpenCode model", ["Model OpenCode", "OpenCode 模型", "OpenCode モデル", "OpenCode-Modell", "Modelo de OpenCode"]),
    ("OpenCode reported:", ["OpenCode zgłosił:", "OpenCode 报告：", "OpenCode からの報告:", "OpenCode meldete:", "OpenCode informó:"]),
    ("OpenCode transformation", ["Transformacja OpenCode", "OpenCode 转换", "OpenCode 変換", "OpenCode-Transformation", "Transformación de OpenCode"]),
    ("OpenCode transformation will be available after Hex connects to OpenCode.", ["Transformacja OpenCode będzie dostępna, gdy Hex połączy się z OpenCode.", "Hex 连接到 OpenCode 后即可使用 OpenCode 转换。", "Hex が OpenCode に接続すると OpenCode 変換を使えます。", "Die OpenCode-Transformation ist verfügbar, sobald Hex mit OpenCode verbunden ist.", "La transformación de OpenCode estará disponible cuando Hex se conecte a OpenCode."]),
    ("Opens only while holding the shortcut. Disables pre-roll and commands.", ["Otwiera się tylko podczas przytrzymania skrótu. Wyłącza wstępne nagrywanie i polecenia.", "仅在按住快捷键时打开。会停用预录音和命令。", "ショートカットを押している間だけ開きます。プリロールとコマンドは無効になります。", "Öffnet sich nur, solange das Kürzel gehalten wird. Deaktiviert Vorlauf und Befehle.", "Solo se abre mientras mantienes el atajo. Desactiva la pregrabación y los comandos."]),
    ("Opens the microphone on shortcut press without pre-roll and turns Commands off.", ["Otwiera mikrofon po naciśnięciu skrótu, bez wstępnego nagrywania, i wyłącza polecenia.", "按下快捷键时打开麦克风，不预录音，并关闭命令。", "ショートカットを押したときにプリロールなしでマイクを開き、コマンドをオフにします。", "Öffnet das Mikrofon beim Drücken des Kürzels ohne Vorlauf und schaltet Befehle aus.", "Abre el micrófono al pulsar el atajo, sin pregrabación, y desactiva los comandos."]),
    ("Opens the microphone on shortcut press without pre-roll. Commands remain off.", ["Otwiera mikrofon po naciśnięciu skrótu, bez wstępnego nagrywania. Polecenia pozostają wyłączone.", "按下快捷键时打开麦克风，不预录音。命令保持关闭。", "ショートカットを押したときにプリロールなしでマイクを開きます。コマンドはオフのままです。", "Öffnet das Mikrofon beim Drücken des Kürzels ohne Vorlauf. Befehle bleiben aus.", "Abre el micrófono al pulsar el atajo, sin pregrabación. Los comandos siguen desactivados."]),
    ("Paste with Ctrl+Shift+V so terminals accept it", ["Wklejaj przez Ctrl+Shift+V, aby terminale to przyjmowały", "使用 Ctrl+Shift+V 粘贴，以便终端接受", "端末で受け付けられるよう Ctrl+Shift+V で貼り付け", "Mit Strg+Umschalt+V einfügen, damit Terminals es annehmen", "Pegar con Ctrl+Mayús+V para que los terminales lo acepten"]),
    ("Permissions needed", ["Wymagane uprawnienia", "需要权限", "権限が必要です", "Berechtigungen erforderlich", "Se necesitan permisos"]),
    ("Preparing speech model", ["Przygotowywanie modelu mowy", "正在准备语音模型", "音声モデルを準備中", "Sprachmodell wird vorbereitet", "Preparando el modelo de voz"]),
    ("Preparing the TypeScript workspace and installing its dependencies…", ["Przygotowywanie obszaru roboczego TypeScript i instalowanie zależności…", "正在准备 TypeScript 工作区并安装其依赖…", "TypeScript ワークスペースを準備し、依存関係をインストール中…", "TypeScript-Arbeitsbereich wird vorbereitet und Abhängigkeiten werden installiert…", "Preparando el espacio de trabajo de TypeScript e instalando sus dependencias…"]),
    ("Preparing voice commands", ["Przygotowywanie poleceń głosowych", "正在准备语音命令", "音声コマンドを準備中", "Sprachbefehle werden vorbereitet", "Preparando los comandos de voz"]),
    ("Press a shortcut", ["Naciśnij skrót", "按下快捷键", "ショートカットを押してください", "Drücke ein Kürzel", "Pulsa un atajo"]),
    ("Press shortcut", ["Naciśnij skrót", "按下快捷键", "ショートカットを押す", "Kürzel drücken", "Pulsa el atajo"]),
    ("Press to start dictating, press again to transcribe", ["Naciśnij, aby zacząć dyktować, i ponownie, aby transkrybować", "按下开始听写，再按一次进行转写", "押すと音声入力を開始し、もう一度押すと文字起こし", "Drücken zum Diktieren, erneut drücken zum Transkribieren", "Pulsa para empezar a dictar y otra vez para transcribir"]),
    ("Re-paste the most recent dictation", ["Wklej ponownie ostatnie dyktowanie", "再次粘贴最近一次听写", "最新の音声入力をもう一度貼り付け", "Letztes Diktat erneut einfügen", "Volver a pegar el dictado más reciente"]),
    ("Recording, cancellation, and error tones", ["Dźwięki nagrywania, anulowania i błędów", "录音、取消和错误提示音", "録音・キャンセル・エラーの効果音", "Töne für Aufnahme, Abbruch und Fehler", "Tonos de grabación, cancelación y error"]),
    ("Refresh", ["Odśwież", "刷新", "更新", "Aktualisieren", "Actualizar"]),
    ("Registered TypeScript transformations are unavailable.", ["Zarejestrowane transformacje TypeScript są niedostępne.", "已注册的 TypeScript 转换不可用。", "登録済みの TypeScript 変換を利用できません。", "Registrierte TypeScript-Transformationen sind nicht verfügbar.", "Las transformaciones de TypeScript registradas no están disponibles."]),
    ("Release microphone", ["Zwolnij mikrofon", "释放麦克风", "マイクを解放", "Mikrofon freigeben", "Liberar micrófono"]),
    ("Release the microphone when idle?", ["Zwalniać mikrofon w bezczynności?", "空闲时释放麦克风？", "待機中はマイクを解放しますか？", "Mikrofon im Leerlauf freigeben?", "¿Liberar el micrófono en reposo?"]),
    ("Release to save or add key", ["Puść, aby zapisać, lub dodaj klawisz", "松开以保存，或再添加按键", "離すと保存、またはキーを追加", "Loslassen zum Speichern oder Taste hinzufügen", "Suelta para guardar o añade una tecla"]),
    ("Release when idle", ["Zwalniaj w bezczynności", "空闲时释放", "待機中は解放", "Im Leerlauf freigeben", "Liberar en reposo"]),
    ("Replacement (e.g. OpenCode)", ["Zamiennik (np. OpenCode)", "替换为（例如 OpenCode）", "置換後（例: OpenCode）", "Ersetzung (z. B. OpenCode)", "Reemplazo (p. ej. OpenCode)"]),
    ("Restart to update", ["Uruchom ponownie, aby zaktualizować", "重启以更新", "再起動してアップデート", "Zum Aktualisieren neu starten", "Reiniciar para actualizar"]),
    ("Retry", ["Ponów", "重试", "再試行", "Erneut versuchen", "Reintentar"]),
    ("Retry setup", ["Ponów konfigurację", "重试设置", "セットアップを再試行", "Einrichtung wiederholen", "Reintentar configuración"]),
    ("Right", ["Prawy", "右", "右", "Rechts", "Derecha"]),
    ("Saved", ["Zapisano", "已保存", "保存しました", "Gespeichert", "Guardado"]),
    ("Saved for when Voice Action is enabled", ["Zapisany na czas włączenia Voice Action", "已保存，待启用 Voice Action 时生效", "Voice Action を有効にしたときのために保存済み", "Gespeichert für den Fall, dass Voice Action aktiviert wird", "Guardado para cuando actives Voice Action"]),
    ("Saving…", ["Zapisywanie…", "正在保存…", "保存中…", "Wird gespeichert…", "Guardando…"]),
    ("Search applications", ["Szukaj aplikacji", "搜索应用", "アプリを検索", "Apps suchen", "Buscar aplicaciones"]),
    ("Search available models", ["Szukaj dostępnych modeli", "搜索可用模型", "利用可能なモデルを検索", "Verfügbare Modelle suchen", "Buscar modelos disponibles"]),
    ("Search history", ["Szukaj w historii", "搜索历史记录", "履歴を検索", "Verlauf durchsuchen", "Buscar en el historial"]),
    ("Seconds", ["Sekundy", "秒", "秒", "Sekunden", "Segundos"]),
    ("Set up", ["Skonfiguruj", "设置", "セットアップ", "Einrichten", "Configurar"]),
    ("Set up Hex", ["Skonfiguruj Hex", "设置 Hex", "Hex をセットアップ", "Hex einrichten", "Configurar Hex"]),
    ("Settings error:", ["Błąd ustawień:", "设置错误：", "設定エラー:", "Einstellungsfehler:", "Error de ajustes:"]),
    ("Shortcut mode", ["Tryb skrótu", "快捷键模式", "ショートカットモード", "Kürzelmodus", "Modo del atajo"]),
    ("Show Dock icon", ["Pokaż ikonę w Docku", "显示程序坞图标", "Dock にアイコンを表示", "Dock-Symbol anzeigen", "Mostrar icono en el Dock"]),
    ("Sound", ["Dźwięk", "声音", "サウンド", "Klang", "Sonido"]),
    ("Classic", ["Klasyczny", "经典", "クラシック", "Klassisch", "Clásico"]),
    ("Style of the recording and cancellation tones", ["Styl dźwięków nagrywania i anulowania", "录音和取消提示音的风格", "録音とキャンセルの効果音のスタイル", "Klangstil für Aufnahme und Abbruch", "Estilo de los tonos de grabación y cancelación"]),
    ("Sound volume", ["Głośność dźwięków", "提示音音量", "効果音の音量", "Lautstärke der Töne", "Volumen de sonidos"]),
    ("Spoken phrase (e.g. open code)", ["Wypowiadana fraza (np. open code)", "说出的短语（例如 open code）", "話すフレーズ（例: open code）", "Gesprochene Phrase (z. B. open code)", "Frase hablada (p. ej. open code)"]),
    ("Start Hex when you log in", ["Uruchamiaj Hex po zalogowaniu", "登录时启动 Hex", "ログイン時に Hex を起動", "Hex beim Anmelden starten", "Iniciar Hex al iniciar sesión"]),
    ("Switch when a domain is active in your browser", ["Przełącz, gdy domena jest aktywna w przeglądarce", "当浏览器中打开某个域名时切换", "ブラウザでドメインが開いているときに切り替え", "Wechseln, wenn eine Domain im Browser aktiv ist", "Cambiar cuando un dominio esté activo en tu navegador"]),
    ("System instructions for rewriting dictation in this mode", ["Instrukcje systemowe do przepisywania dyktowania w tym trybie", "此模式下改写听写内容的系统说明", "このモードで音声入力を書き直すためのシステム指示", "Systemanweisungen zum Umschreiben von Diktaten in diesem Modus", "Instrucciones del sistema para reescribir el dictado en este modo"]),
    ("Terminal paste", ["Wklejanie w terminalu", "终端粘贴", "端末への貼り付け", "Einfügen im Terminal", "Pegado en terminal"]),
    ("The TypeScript workspace could not be prepared.", ["Nie udało się przygotować obszaru roboczego TypeScript.", "无法准备 TypeScript 工作区。", "TypeScript ワークスペースを準備できませんでした。", "Der TypeScript-Arbeitsbereich konnte nicht vorbereitet werden.", "No se pudo preparar el espacio de trabajo de TypeScript."]),
    ("The Voice Action shortcut is already in use. Change it below or change the other shortcut before enabling Voice Action.", ["Skrót Voice Action jest już używany. Zmień go poniżej lub zmień drugi skrót przed włączeniem Voice Action.", "Voice Action 快捷键已被使用。请在下方更改，或先更改另一个快捷键再启用 Voice Action。", "Voice Action のショートカットはすでに使われています。下で変更するか、もう一方のショートカットを変更してから Voice Action を有効にしてください。", "Das Voice-Action-Kürzel ist bereits belegt. Ändere es unten oder ändere das andere Kürzel, bevor du Voice Action aktivierst.", "El atajo de Voice Action ya está en uso. Cámbialo abajo o cambia el otro atajo antes de activar Voice Action."]),
    ("The dictation shortcut needs a local speech model. Choose one to download first.", ["Skrót dyktowania wymaga lokalnego modelu mowy. Najpierw wybierz model do pobrania.", "听写快捷键需要本地语音模型。请先选择一个下载。", "音声入力ショートカットにはローカル音声モデルが必要です。まずダウンロードするモデルを選んでください。", "Das Diktierkürzel braucht ein lokales Sprachmodell. Wähle zuerst eines zum Herunterladen.", "El atajo de dictado necesita un modelo de voz local. Elige uno para descargarlo primero."]),
    ("The selected model transcribes speech entirely on this Mac.", ["Wybrany model transkrybuje mowę w całości na tym Macu.", "所选模型完全在这台 Mac 上转写语音。", "選択したモデルは、この Mac 上だけで音声を文字起こしします。", "Das gewählte Modell transkribiert Sprache vollständig auf diesem Mac.", "El modelo seleccionado transcribe la voz por completo en este Mac."]),
    ("The selected model transcribes speech entirely on this computer.", ["Wybrany model transkrybuje mowę w całości na tym komputerze.", "所选模型完全在这台电脑上转写语音。", "選択したモデルは、このコンピューター上だけで音声を文字起こしします。", "Das gewählte Modell transkribiert Sprache vollständig auf diesem Computer.", "El modelo seleccionado transcribe la voz por completo en este equipo."]),
    ("Thinking", ["Rozumowanie", "思考", "思考", "Denken", "Razonamiento"]),
    ("Toggle", ["Przełącz", "切换", "トグル", "Umschalten", "Alternar"]),
    ("Transformation requires OpenCode", ["Transformacja wymaga OpenCode", "转换需要 OpenCode", "変換には OpenCode が必要です", "Transformation erfordert OpenCode", "La transformación requiere OpenCode"]),
    ("Transformations", ["Transformacje", "转换", "変換", "Transformationen", "Transformaciones"]),
    ("Turn Hex on in every app whenever it opens", ["Włączaj Hex we wszystkich aplikacjach przy każdym uruchomieniu", "每次打开时在所有应用中启用 Hex", "起動するたびに、すべてのアプリで Hex をオンにする", "Hex bei jedem Start in allen Apps einschalten", "Activar Hex en todas las apps cada vez que se abra"]),
    ("Up to date", ["Aktualne", "已是最新", "最新です", "Aktuell", "Actualizado"]),
    ("Update", ["Aktualizuj", "更新", "アップデート", "Aktualisieren", "Actualizar"]),
    ("Use OpenCode default", ["Użyj domyślnego OpenCode", "使用 OpenCode 默认值", "OpenCode の既定を使用", "OpenCode-Standard verwenden", "Usar el predeterminado de OpenCode"]),
    ("Use the shortcut below with OpenCode to run spoken instructions", ["Użyj poniższego skrótu z OpenCode, aby wykonywać wypowiedziane polecenia", "使用下方快捷键配合 OpenCode 执行口述指令", "下のショートカットと OpenCode で、話した指示を実行します", "Nutze das Kürzel unten mit OpenCode, um gesprochene Anweisungen auszuführen", "Usa el atajo de abajo con OpenCode para ejecutar instrucciones habladas"]),
    ("Voice Action requires OpenCode", ["Voice Action wymaga OpenCode", "Voice Action 需要 OpenCode", "Voice Action には OpenCode が必要です", "Voice Action erfordert OpenCode", "Voice Action requiere OpenCode"]),
    ("Voice Action will be available after Hex connects to OpenCode.", ["Voice Action będzie dostępny, gdy Hex połączy się z OpenCode.", "Hex 连接到 OpenCode 后即可使用 Voice Action。", "Hex が OpenCode に接続すると Voice Action を使えます。", "Voice Action ist verfügbar, sobald Hex mit OpenCode verbunden ist.", "Voice Action estará disponible cuando Hex se conecte a OpenCode."]),
    ("Wayland doesn't report the frontmost app; these apply under X11", ["Wayland nie podaje aktywnej aplikacji; te reguły działają w X11", "Wayland 不报告前台应用；这些规则仅在 X11 下生效", "Wayland は最前面のアプリを通知しないため、これは X11 でのみ有効です", "Wayland meldet die aktive App nicht; diese Regeln gelten unter X11", "Wayland no informa de la app en primer plano; esto se aplica en X11"]),
    ("Sound effects", ["Efekty dźwiękowe", "音效", "効果音", "Soundeffekte", "Efectos de sonido"]),
    ("Accessibility", ["Dostępność", "辅助功能", "アクセシビリティ", "Bedienungshilfen", "Accesibilidad"]),
    ("Allow", ["Zezwól", "允许", "許可", "Erlauben", "Permitir"]),
    ("Hex cannot paste completed dictation into the foreground app.", ["Hex nie może wkleić gotowego dyktowania do aktywnej aplikacji.", "Hex 无法将完成的听写粘贴到前台应用。", "Hex は完了した音声入力を最前面のアプリに貼り付けられません。", "Hex kann fertige Diktate nicht in die aktive App einfügen.", "Hex no puede pegar el dictado terminado en la app en primer plano."]),
    ("Hex cannot recognize the dictation shortcut in other apps.", ["Hex nie rozpozna skrótu dyktowania w innych aplikacjach.", "Hex 无法在其他应用中识别听写快捷键。", "Hex はほかのアプリで音声入力ショートカットを認識できません。", "Hex erkennt das Diktierkürzel in anderen Apps nicht.", "Hex no puede reconocer el atajo de dictado en otras apps."]),
    ("Hex cannot record dictation until microphone access is restored.", ["Hex nie może nagrywać dyktowania, dopóki dostęp do mikrofonu nie zostanie przywrócony.", "在恢复麦克风访问权限之前，Hex 无法录制听写。", "マイクへのアクセスが戻るまで、Hex は音声入力を録音できません。", "Hex kann keine Diktate aufnehmen, bis der Mikrofonzugriff wiederhergestellt ist.", "Hex no puede grabar dictados hasta que se restablezca el acceso al micrófono."]),
    ("Hear commands and dictation.", ["Słysz polecenia i dyktowanie.", "收听命令和听写。", "コマンドと音声入力を聞き取ります。", "Befehle und Diktate hören.", "Escucha comandos y dictado."]),
    ("Input Monitoring", ["Monitorowanie wejścia", "输入监控", "入力監視", "Eingabeüberwachung", "Monitorización de entrada"]),
    ("Paste completed dictation into the foreground app", ["Wklejaj gotowe dyktowanie do aktywnej aplikacji", "将完成的听写粘贴到前台应用", "完了した音声入力を最前面のアプリに貼り付け", "Fertige Diktate in die aktive App einfügen", "Pegar el dictado terminado en la app en primer plano"]),
    ("Paste text and run keyboard commands.", ["Wklejaj tekst i wykonuj polecenia klawiatury.", "粘贴文本并执行键盘命令。", "テキストを貼り付け、キーボードコマンドを実行します。", "Text einfügen und Tastaturbefehle ausführen.", "Pega texto y ejecuta comandos de teclado."]),
    ("Recognize the dictation shortcut anywhere", ["Rozpoznawaj skrót dyktowania wszędzie", "在任何地方识别听写快捷键", "どこでも音声入力ショートカットを認識", "Das Diktierkürzel überall erkennen", "Reconocer el atajo de dictado en cualquier lugar"]),
    ("Record dictation while you hold the shortcut", ["Nagrywaj dyktowanie, gdy przytrzymujesz skrót", "按住快捷键时录制听写", "ショートカットを押している間、音声入力を録音", "Diktat aufnehmen, solange du das Kürzel hältst", "Grabar el dictado mientras mantienes el atajo"]),
    ("The command model could not load. Retry or turn off commands; hotkey dictation is unaffected.", ["Nie udało się wczytać modelu poleceń. Ponów lub wyłącz polecenia; dyktowanie skrótem działa normalnie.", "无法加载命令模型。请重试或关闭命令；快捷键听写不受影响。", "コマンドモデルを読み込めませんでした。再試行するかコマンドをオフにしてください。ショートカットでの音声入力には影響しません。", "Das Befehlsmodell konnte nicht geladen werden. Versuche es erneut oder schalte Befehle aus; das Diktieren per Kürzel ist nicht betroffen.", "No se pudo cargar el modelo de comandos. Reinténtalo o desactiva los comandos; el dictado con atajo no se ve afectado."]),
    ("The command model is loading. Hotkey dictation does not require it.", ["Model poleceń się wczytuje. Dyktowanie skrótem go nie wymaga.", "正在加载命令模型。快捷键听写不需要它。", "コマンドモデルを読み込み中です。ショートカットでの音声入力には不要です。", "Das Befehlsmodell wird geladen. Das Diktieren per Kürzel braucht es nicht.", "El modelo de comandos se está cargando. El dictado con atajo no lo necesita."]),
    ("Voice commands are unavailable", ["Polecenia głosowe są niedostępne", "语音命令不可用", "音声コマンドを利用できません", "Sprachbefehle sind nicht verfügbar", "Los comandos de voz no están disponibles"]),
    ("Turn off commands", ["Wyłącz polecenia", "关闭命令", "コマンドをオフにする", "Befehle ausschalten", "Desactivar comandos"]),
    ("Keep microphone ready & enable commands", ["Utrzymuj mikrofon i włącz polecenia", "保持麦克风就绪并启用命令", "マイクを待機させてコマンドを有効にする", "Mikrofon bereit halten und Befehle aktivieren", "Mantener el micrófono listo y activar comandos"]),
    ("Commands", ["Polecenia", "命令", "コマンド", "Befehle", "Comandos"]),
    ("Copy agent prompt", ["Kopiuj prompt dla agenta", "复制智能体提示词", "エージェント用プロンプトをコピー", "Agenten-Prompt kopieren", "Copiar prompt para el agente"]),
    ("Create config", ["Utwórz konfigurację", "创建配置", "設定を作成", "Konfiguration erstellen", "Crear configuración"]),
    ("Creating config…", ["Tworzenie konfiguracji…", "正在创建配置…", "設定を作成中…", "Konfiguration wird erstellt…", "Creando configuración…"]),
    ("Edit config", ["Edytuj konfigurację", "编辑配置", "設定を編集", "Konfiguration bearbeiten", "Editar configuración"]),
    ("Personal commands are not loading.", ["Osobiste polecenia się nie wczytują.", "个人命令无法加载。", "個人コマンドを読み込めません。", "Persönliche Befehle werden nicht geladen.", "Los comandos personales no se cargan."]),
    ("No commands yet.", ["Brak poleceń.", "暂无命令。", "コマンドはまだありません。", "Noch keine Befehle.", "Aún no hay comandos."]),
    ("No matching commands.", ["Brak pasujących poleceń.", "没有匹配的命令。", "一致するコマンドはありません。", "Keine passenden Befehle.", "No hay comandos que coincidan."]),
    ("Search commands", ["Szukaj poleceń", "搜索命令", "コマンドを検索", "Befehle suchen", "Buscar comandos"]),
    ("Select a command.", ["Wybierz polecenie.", "选择一个命令。", "コマンドを選択してください。", "Wähle einen Befehl.", "Selecciona un comando."]),
    ("Aliases", ["Aliasy", "别名", "エイリアス", "Aliasse", "Alias"]),
    ("Context", ["Kontekst", "上下文", "コンテキスト", "Kontext", "Contexto"]),
    ("Current context", ["Bieżący kontekst", "当前上下文", "現在のコンテキスト", "Aktueller Kontext", "Contexto actual"]),
    ("Task", ["Zadanie", "任务", "タスク", "Aufgabe", "Tarea"]),
    ("None", ["Brak", "无", "なし", "Keine", "Ninguno"]),
    ("Keyboard", ["Klawiatura", "键盘", "キーボード", "Tastatur", "Teclado"]),
    ("Browser navigation", ["Nawigacja w przeglądarce", "浏览器导航", "ブラウザーの移動", "Browsernavigation", "Navegación del navegador"]),
    ("Application navigation", ["Nawigacja w aplikacji", "应用导航", "アプリ内の移動", "App-Navigation", "Navegación de la aplicación"]),
    ("Open applications", ["Otwieranie aplikacji", "打开应用", "アプリを開く", "Apps öffnen", "Abrir aplicaciones"]),
    ("Open websites", ["Otwieranie stron", "打开网站", "ウェブサイトを開く", "Websites öffnen", "Abrir sitios web"]),
    ("Voice control", ["Sterowanie głosem", "语音控制", "音声操作", "Sprachsteuerung", "Control por voz"]),
    ("Meetings (beta)", ["Spotkania (beta)", "会议（测试版）", "会議（ベータ）", "Besprechungen (Beta)", "Reuniones (beta)"]),
    ("Offer to record calls and save local transcripts", ["Proponuj nagrywanie rozmów i zapisuj lokalne transkrypcje", "提供通话录音并保存本地转写", "通話の録音を提案し、ローカルに文字起こしを保存", "Aufnahme von Anrufen anbieten und lokale Transkripte speichern", "Ofrecer grabar llamadas y guardar transcripciones locales"]),
    ("Screen & System Audio Recording", ["Nagrywanie ekranu i dźwięku systemu", "屏幕与系统音频录制", "画面とシステムオーディオの収録", "Bildschirm- und Systemaudioaufnahme", "Grabación de pantalla y audio del sistema"]),
    ("Captures the other participants' audio", ["Nagrywa dźwięk pozostałych uczestników", "录制其他参与者的音频", "ほかの参加者の音声を収録します", "Nimmt den Ton der anderen Teilnehmenden auf", "Graba el audio de los demás participantes"]),
    ("Never offer", ["Nigdy nie proponuj", "永不提示", "提案しない", "Nie anbieten", "No ofrecer nunca"]),
    ("No recording offers for these sites and apps", ["Bez propozycji nagrywania dla tych stron i aplikacji", "这些网站和应用不再提示录制", "これらのサイトとアプリでは録音を提案しません", "Keine Aufnahmeangebote für diese Websites und Apps", "Sin ofertas de grabación para estos sitios y apps"]),
    ("Required to hear the other participants.", ["Wymagane, aby słyszeć pozostałych uczestników.", "需要此权限才能听到其他参与者。", "ほかの参加者の声を聞くために必要です。", "Nötig, um die anderen Teilnehmenden zu hören.", "Necesario para oír a los demás participantes."]),
    ("Transcript folder", ["Folder transkrypcji", "转写文件夹", "文字起こしフォルダー", "Transkriptordner", "Carpeta de transcripciones"]),
    ("Finished meetings are saved here as Markdown", ["Zakończone spotkania są zapisywane tu jako Markdown", "已结束的会议以 Markdown 格式保存在此处", "終了した会議はここに Markdown で保存されます", "Beendete Besprechungen werden hier als Markdown gespeichert", "Las reuniones terminadas se guardan aquí en Markdown"]),
    ("Reveal", ["Pokaż", "显示", "表示", "Anzeigen", "Mostrar"]),
    ("Use Folder", ["Użyj folderu", "使用此文件夹", "このフォルダーを使用", "Ordner verwenden", "Usar carpeta"]),
    ("Summarize with Apple Intelligence", ["Podsumuj za pomocą Apple Intelligence", "使用 Apple 智能进行总结", "Apple Intelligence で要約", "Mit Apple Intelligence zusammenfassen", "Resumir con Apple Intelligence"]),
    ("Show recording indicator", ["Pokaż wskaźnik nagrywania", "显示录音指示器", "録音インジケーターを表示", "Aufnahmeanzeige einblenden", "Mostrar indicador de grabación"]),
    ("Floating pill to copy live transcript turns or stop recording", ["Pływająca kapsułka do kopiowania bieżącej transkrypcji lub zatrzymania nagrywania", "用于复制实时转写内容或停止录音的悬浮胶囊", "ライブ文字起こしのコピーや録音停止ができるフローティングピル", "Schwebende Kapsel zum Kopieren von Live-Transkripten oder Stoppen der Aufnahme", "Píldora flotante para copiar turnos de transcripción en vivo o detener la grabación"]),
    ("Start recording", ["Rozpocznij nagrywanie", "开始录音", "録音を開始", "Aufnahme starten", "Empezar a grabar"]),
    ("Starting…", ["Uruchamianie…", "正在启动…", "開始中…", "Wird gestartet…", "Iniciando…"]),
    ("Stop & transcribe", ["Zatrzymaj i transkrybuj", "停止并转写", "停止して文字起こし", "Stoppen und transkribieren", "Detener y transcribir"]),
    ("Finalizing…", ["Kończenie…", "正在完成…", "仕上げ中…", "Wird abgeschlossen…", "Finalizando…"]),
    ("Failed", ["Niepowodzenie", "失败", "失敗", "Fehlgeschlagen", "Fallido"]),
    ("Interrupted", ["Przerwano", "已中断", "中断", "Unterbrochen", "Interrumpido"]),
    ("Meeting title", ["Tytuł spotkania", "会议标题", "会議のタイトル", "Titel der Besprechung", "Título de la reunión"]),
    ("Search meetings", ["Szukaj spotkań", "搜索会议", "会議を検索", "Besprechungen suchen", "Buscar reuniones"]),
    ("No meetings match.", ["Brak pasujących spotkań.", "没有匹配的会议。", "一致する会議はありません。", "Keine passenden Besprechungen.", "No hay reuniones que coincidan."]),
    ("No meetings yet.", ["Brak spotkań.", "暂无会议。", "会議はまだありません。", "Noch keine Besprechungen.", "Aún no hay reuniones."]),
    ("Meetings could not be loaded.", ["Nie udało się wczytać spotkań.", "无法加载会议。", "会議を読み込めませんでした。", "Besprechungen konnten nicht geladen werden.", "No se pudieron cargar las reuniones."]),
    ("Meeting could not be recorded.", ["Nie udało się nagrać spotkania.", "无法录制会议。", "会議を録音できませんでした。", "Die Besprechung konnte nicht aufgenommen werden.", "No se pudo grabar la reunión."]),
    ("Meeting could not be updated.", ["Nie udało się zaktualizować spotkania.", "无法更新会议。", "会議を更新できませんでした。", "Die Besprechung konnte nicht aktualisiert werden.", "No se pudo actualizar la reunión."]),
    ("Recording failed.", ["Nagrywanie nie powiodło się.", "录音失败。", "録音に失敗しました。", "Aufnahme fehlgeschlagen.", "La grabación falló."]),
    ("Markdown not saved.", ["Nie zapisano pliku Markdown.", "未保存 Markdown。", "Markdown を保存できませんでした。", "Markdown nicht gespeichert.", "No se guardó el Markdown."]),
    ("Notes could not be written.", ["Nie udało się zapisać notatek.", "无法写入笔记。", "メモを書き込めませんでした。", "Notizen konnten nicht geschrieben werden.", "No se pudieron escribir las notas."]),
    ("Live draft unavailable.", ["Bieżący szkic jest niedostępny.", "实时草稿不可用。", "ライブ下書きを利用できません。", "Live-Entwurf nicht verfügbar.", "El borrador en vivo no está disponible."]),
    ("Select a meeting.", ["Wybierz spotkanie.", "选择一个会议。", "会議を選択してください。", "Wähle eine Besprechung.", "Selecciona una reunión."]),
    ("Record a call to see its notes and transcript here.", ["Nagraj rozmowę, aby zobaczyć tu jej notatki i transkrypcję.", "录制通话后，可在此处查看其笔记和转写。", "通話を録音すると、ここにメモと文字起こしが表示されます。", "Nimm einen Anruf auf, um hier Notizen und Transkript zu sehen.", "Graba una llamada para ver aquí sus notas y transcripción."]),
    ("Copy summary", ["Kopiuj podsumowanie", "复制摘要", "要約をコピー", "Zusammenfassung kopieren", "Copiar resumen"]),
    ("Copy transcript", ["Kopiuj transkrypcję", "复制转写", "文字起こしをコピー", "Transkript kopieren", "Copiar transcripción"]),
    ("Rename", ["Zmień nazwę", "重命名", "名前を変更", "Umbenennen", "Cambiar nombre"]),
    ("Save", ["Zapisz", "保存", "保存", "Speichern", "Guardar"]),
    ("Reveal Markdown", ["Pokaż plik Markdown", "显示 Markdown", "Markdown を表示", "Markdown anzeigen", "Mostrar Markdown"]),
    ("Reveal files", ["Pokaż pliki", "显示文件", "ファイルを表示", "Dateien anzeigen", "Mostrar archivos"]),
    ("Delete…", ["Usuń…", "删除…", "削除…", "Löschen…", "Eliminar…"]),
    ("Trash recording", ["Przenieś nagranie do kosza", "将录音移到废纸篓", "録音をゴミ箱に入れる", "Aufnahme in den Papierkorb", "Mover la grabación a la papelera"]),
    ("Trash recording & Markdown", ["Przenieś nagranie i Markdown do kosza", "将录音和 Markdown 移到废纸篓", "録音と Markdown をゴミ箱に入れる", "Aufnahme und Markdown in den Papierkorb", "Mover la grabación y el Markdown a la papelera"]),
    ("Regenerate summary", ["Wygeneruj podsumowanie ponownie", "重新生成摘要", "要約を再生成", "Zusammenfassung neu erstellen", "Regenerar resumen"]),
    ("Summarize", ["Podsumuj", "总结", "要約", "Zusammenfassen", "Resumir"]),
    ("Summarizing…", ["Podsumowywanie…", "正在总结…", "要約中…", "Wird zusammengefasst…", "Resumiendo…"]),
    ("Summary", ["Podsumowanie", "摘要", "要約", "Zusammenfassung", "Resumen"]),
    ("Nothing to summarize.", ["Nie ma czego podsumować.", "没有可总结的内容。", "要約する内容がありません。", "Nichts zum Zusammenfassen.", "No hay nada que resumir."]),
    ("Writing a title and summary with Apple Intelligence…", ["Tworzenie tytułu i podsumowania za pomocą Apple Intelligence…", "正在使用 Apple 智能撰写标题和摘要…", "Apple Intelligence でタイトルと要約を作成中…", "Titel und Zusammenfassung werden mit Apple Intelligence geschrieben…", "Escribiendo un título y un resumen con Apple Intelligence…"]),
    ("Building the final transcript…", ["Tworzenie końcowej transkrypcji…", "正在生成最终转写…", "最終的な文字起こしを作成中…", "Endgültiges Transkript wird erstellt…", "Creando la transcripción final…"]),
    ("Listening for speech…", ["Nasłuchiwanie mowy…", "正在聆听语音…", "音声を待機中…", "Warte auf Sprache…", "Escuchando…"]),
    ("Preparing local audio capture…", ["Przygotowywanie lokalnego nagrywania dźwięku…", "正在准备本地音频捕获…", "ローカル音声の収録を準備中…", "Lokale Audioaufnahme wird vorbereitet…", "Preparando la captura de audio local…"]),
    ("Live transcription is unavailable. Audio is still recording safely.", ["Transkrypcja na żywo jest niedostępna. Dźwięk nadal jest bezpiecznie nagrywany.", "实时转写不可用。音频仍在安全录制中。", "ライブ文字起こしは利用できません。音声は引き続き安全に録音されています。", "Live-Transkription ist nicht verfügbar. Der Ton wird weiterhin sicher aufgenommen.", "La transcripción en vivo no está disponible. El audio se sigue grabando de forma segura."]),
    ("No recoverable transcript is available.", ["Brak transkrypcji do odzyskania.", "没有可恢复的转写。", "復元できる文字起こしはありません。", "Kein wiederherstellbares Transkript verfügbar.", "No hay ninguna transcripción recuperable."]),
    ("No speech was transcribed.", ["Nie przetranskrybowano żadnej mowy.", "没有转写到任何语音。", "文字起こしされた音声はありません。", "Es wurde keine Sprache transkribiert.", "No se transcribió ninguna voz."]),
    ("No transcript selected.", ["Nie wybrano transkrypcji.", "未选择转写。", "文字起こしが選択されていません。", "Kein Transkript ausgewählt.", "No hay ninguna transcripción seleccionada."]),
    ("Draft transcript", ["Szkic transkrypcji", "转写草稿", "文字起こしの下書き", "Transkriptentwurf", "Borrador de la transcripción"]),
    ("Live transcript", ["Transkrypcja na żywo", "实时转写", "ライブ文字起こし", "Live-Transkript", "Transcripción en vivo"]),
    ("Recovered draft", ["Odzyskany szkic", "已恢复的草稿", "復元した下書き", "Wiederhergestellter Entwurf", "Borrador recuperado"]),
    ("Transcript", ["Transkrypcja", "转写", "文字起こし", "Transkript", "Transcripción"]),
    ("Transcript unavailable.", ["Transkrypcja jest niedostępna.", "转写不可用。", "文字起こしを利用できません。", "Transkript nicht verfügbar.", "La transcripción no está disponible."]),
    ("Listening…", ["Słucham…", "正在聆听…", "聞き取り中…", "Hört zu…", "Escuchando…"]),
    ("< 1 min", ["< 1 min", "< 1 分钟", "1 分未満", "< 1 Min.", "< 1 min"]),
    ("{} min", ["{} min", "{} 分钟", "{} 分", "{} Min.", "{} min"]),
    ("{hours} h {minutes} min", ["{hours} godz. {minutes} min", "{hours} 小时 {minutes} 分钟", "{hours} 時間 {minutes} 分", "{hours} Std. {minutes} Min.", "{hours} h {minutes} min"]),
    ("{} pause", ["{} pauzy", "暂停 {}", "{} の休止", "{} Pause", "{} de pausa"]),
    ("Size", ["Rozmiar", "大小", "サイズ", "Größe", "Tamaño"]),
    ("Speed", ["Szybkość", "速度", "速度", "Geschwindigkeit", "Velocidad"]),
    ("Model could not be deleted.", ["Nie udało się usunąć modelu.", "无法删除模型。", "モデルを削除できませんでした。", "Das Modell konnte nicht gelöscht werden.", "No se pudo eliminar el modelo."]),
    ("Delete {}?", ["Usunąć {}?", "删除 {}？", "{} を削除しますか？", "{} löschen?", "¿Eliminar {}?"]),
    ("100 languages", ["100 języków", "100 种语言", "100 言語", "100 Sprachen", "100 idiomas"]),
    ("14 languages", ["14 języków", "14 种语言", "14 言語", "14 Sprachen", "14 idiomas"]),
    ("25 languages", ["25 języków", "25 种语言", "25 言語", "25 Sprachen", "25 idiomas"]),
    ("30 languages", ["30 języków", "30 种语言", "30 言語", "30 Sprachen", "30 idiomas"]),
    ("5 languages", ["5 języków", "5 种语言", "5 言語", "5 Sprachen", "5 idiomas"]),
    ("English", ["Angielski", "英语", "英語", "Englisch", "Inglés"]),
    ("System locales", ["Języki systemu", "系统区域设置", "システムのロケール", "Systemsprachen", "Idiomas del sistema"]),
    ("Apple managed", ["Zarządzany przez Apple", "由 Apple 管理", "Apple が管理", "Von Apple verwaltet", "Gestionado por Apple"]),
    ("English benchmark", ["Test dla angielskiego", "英语基准测试", "英語ベンチマーク", "Englisch-Benchmark", "Prueba en inglés"]),
    ("Mandarin benchmark", ["Test dla mandaryńskiego", "普通话基准测试", "中国語（普通話）ベンチマーク", "Mandarin-Benchmark", "Prueba en mandarín"]),
    ("On device", ["Na urządzeniu", "设备端", "デバイス上", "Auf dem Gerät", "En el dispositivo"]),
    ("No timestamps", ["Bez znaczników czasu", "无时间戳", "タイムスタンプなし", "Keine Zeitstempel", "Sin marcas de tiempo"]),
    ("Segment timestamps", ["Znaczniki czasu segmentów", "分段时间戳", "セグメントのタイムスタンプ", "Segment-Zeitstempel", "Marcas de tiempo por segmento"]),
    ("Token timestamps", ["Znaczniki czasu tokenów", "词元时间戳", "トークンのタイムスタンプ", "Token-Zeitstempel", "Marcas de tiempo por token"]),
    ("Show Hex", ["Pokaż Hex", "显示 Hex", "Hex を表示", "Hex anzeigen", "Mostrar Hex"]),
    ("Enable / Disable Hex", ["Włącz / wyłącz Hex", "启用 / 停用 Hex", "Hex を有効 / 無効にする", "Hex aktivieren / deaktivieren", "Activar / desactivar Hex"]),
    ("Quit Hex", ["Zakończ Hex", "退出 Hex", "Hex を終了", "Hex beenden", "Salir de Hex"]),
    ("Hex Dictation", ["Dyktowanie Hex", "Hex 听写", "Hex 音声入力", "Hex-Diktat", "Dictado de Hex"]),
    ("Enabled", ["Włączony", "已启用", "有効", "Aktiviert", "Activado"]),
    ("Sleeping", ["Uśpiony", "休眠中", "スリープ中", "Schläft", "En reposo"]),
    ("Enabling…", ["Włączanie…", "正在启用…", "有効化中…", "Wird aktiviert…", "Activando…"]),
    ("Disabling…", ["Wyłączanie…", "正在停用…", "無効化中…", "Wird deaktiviert…", "Desactivando…"]),
    ("Applying settings…", ["Stosowanie ustawień…", "正在应用设置…", "設定を適用中…", "Einstellungen werden angewendet…", "Aplicando ajustes…"]),
    ("Couldn't enable", ["Nie udało się włączyć", "无法启用", "有効にできませんでした", "Aktivierung fehlgeschlagen", "No se pudo activar"]),
    ("Saving settings", ["Zapisywanie ustawień", "正在保存设置", "設定を保存中", "Einstellungen werden gespeichert", "Guardando ajustes"]),
    ("Service disconnected", ["Usługa rozłączona", "服务已断开", "サービスとの接続が切れました", "Dienst getrennt", "Servicio desconectado"]),
    ("Use two modifiers or add a key", ["Użyj dwóch modyfikatorów lub dodaj klawisz", "使用两个修饰键或再添加一个按键", "修飾キーを 2 つ使うか、キーを追加してください", "Nutze zwei Sondertasten oder füge eine Taste hinzu", "Usa dos modificadores o añade una tecla"]),
    ("Unsupported key", ["Nieobsługiwany klawisz", "不支持的按键", "未対応のキー", "Nicht unterstützte Taste", "Tecla no compatible"]),
    ("Choose from Finder…", ["Wybierz w Finderze…", "从访达中选择…", "Finder から選択…", "Im Finder auswählen…", "Elegir en el Finder…"]),
    ("Choose an .exe…", ["Wybierz plik .exe…", "选择 .exe 文件…", ".exe を選択…", ".exe auswählen…", "Elegir un .exe…"]),
    ("Choose a file…", ["Wybierz plik…", "选择文件…", "ファイルを選択…", "Datei auswählen…", "Elegir un archivo…"]),
    ("Downloading {}%", ["Pobieranie {}%", "正在下载 {}%", "ダウンロード中 {}%", "Wird geladen {} %", "Descargando {} %"]),
    ("Loading model · {}s", ["Wczytywanie modelu · {} s", "正在加载模型 · {} 秒", "モデルを読み込み中 · {} 秒", "Modell wird geladen · {} s", "Cargando modelo · {} s"]),
    ("Enable Hex to dictate into any app with {}", ["Włącz Hex, aby dyktować w dowolnej aplikacji za pomocą {}", "启用 Hex 后，可用 {} 在任意应用中听写", "Hex を有効にすると、{} でどのアプリにも音声入力できます", "Aktiviere Hex, um mit {} in jede App zu diktieren", "Activa Hex para dictar en cualquier app con {}"]),
    ("Ready anywhere · Hold {} to dictate", ["Gotowy wszędzie · Przytrzymaj {}, aby dyktować", "随处可用 · 按住 {} 进行听写", "どこでも使えます · {} を押している間に音声入力", "Überall bereit · {} halten zum Diktieren", "Listo en cualquier lugar · Mantén {} para dictar"]),
    ("Ready anywhere · Press {} to start and stop dictating", ["Gotowy wszędzie · Naciśnij {}, aby zacząć i zakończyć dyktowanie", "随处可用 · 按 {} 开始和停止听写", "どこでも使えます · {} を押して音声入力を開始・停止", "Überall bereit · {} drücken zum Starten und Beenden des Diktats", "Listo en cualquier lugar · Pulsa {} para empezar y terminar de dictar"]),
    ("Keep: {}", ["Przechowuj: {}", "保留：{}", "保持: {}", "Behalten: {}", "Conservar: {}"]),
    ("No applications match \"{}\".", ["Żadna aplikacja nie pasuje do „{}”.", "没有与“{}”匹配的应用。", "「{}」に一致するアプリはありません。", "Keine App passt zu „{}“.", "Ninguna aplicación coincide con «{}»."]),
    ("OpenCode default: {}", ["Domyślny OpenCode: {}", "OpenCode 默认：{}", "OpenCode の既定: {}", "OpenCode-Standard: {}", "Predeterminado de OpenCode: {}"]),
    ("OpenCode models unavailable: {}", ["Modele OpenCode są niedostępne: {}", "OpenCode 模型不可用：{}", "OpenCode のモデルを利用できません: {}", "OpenCode-Modelle nicht verfügbar: {}", "Modelos de OpenCode no disponibles: {}"]),
    ("{} available models", ["Dostępne modele: {}", "{} 个可用模型", "利用可能なモデル {} 件", "{} verfügbare Modelle", "{} modelos disponibles"]),
    ("Mode {}", ["Tryb {}", "模式 {}", "モード {}", "Modus {}", "Modo {}"]),
    ("{name} already activates {conflict}. Remove it there before reassigning it.", ["{name} już aktywuje tryb {conflict}. Usuń go tam przed ponownym przypisaniem.", "{name} 已用于激活 {conflict}。请先从那里移除再重新分配。", "{name} はすでに {conflict} を有効にしています。割り当て直す前にそちらから削除してください。", "{name} aktiviert bereits {conflict}. Entferne es dort, bevor du es neu zuweist.", "{name} ya activa {conflict}. Quítalo de allí antes de reasignarlo."]),
    ("Could not load app settings: {}", ["Nie udało się wczytać ustawień aplikacji: {}", "无法加载应用设置：{}", "アプリの設定を読み込めませんでした: {}", "App-Einstellungen konnten nicht geladen werden: {}", "No se pudieron cargar los ajustes de la app: {}"]),
    ("Could not load settings: {}", ["Nie udało się wczytać ustawień: {}", "无法加载设置：{}", "設定を読み込めませんでした: {}", "Einstellungen konnten nicht geladen werden: {}", "No se pudieron cargar los ajustes: {}"]),
    ("Could not restart Hex: {}", ["Nie udało się ponownie uruchomić Hex: {}", "无法重启 Hex：{}", "Hex を再起動できませんでした: {}", "Hex konnte nicht neu gestartet werden: {}", "No se pudo reiniciar Hex: {}"]),
    ("Could not start listener: {}", ["Nie udało się uruchomić nasłuchiwania: {}", "无法启动监听：{}", "リスナーを開始できませんでした: {}", "Listener konnte nicht gestartet werden: {}", "No se pudo iniciar la escucha: {}"]),
    ("Finding applications on this Mac…", ["Wyszukiwanie aplikacji na tym Macu…", "正在查找这台 Mac 上的应用…", "この Mac のアプリを検索中…", "Apps auf diesem Mac werden gesucht…", "Buscando aplicaciones en este Mac…"]),
    ("{audio} ms audio · {inference} ms inference", ["{audio} ms dźwięku · {inference} ms wnioskowania", "音频 {audio} 毫秒 · 推理 {inference} 毫秒", "音声 {audio} ms · 推論 {inference} ms", "{audio} ms Audio · {inference} ms Inferenz", "{audio} ms de audio · {inference} ms de inferencia"]),
    ("{} ms total", ["łącznie {} ms", "总计 {} 毫秒", "合計 {} ms", "{} ms gesamt", "{} ms en total"]),
    ("fell back: {}", ["użyto oryginału: {}", "已回退为原文：{}", "元のテキストを使用: {}", "Rohtext verwendet: {}", "se usó el texto original: {}"]),
    ("Other", ["Inne", "其他", "その他", "Sonstige", "Otros"]),
    ("Switch Hex on in the list", ["Włącz Hex na liście", "在列表中打开 Hex", "リストで Hex をオンにしてください", "Hex in der Liste einschalten", "Activa Hex en la lista"]),
    ("Speech model ready", ["Model mowy gotowy", "语音模型已就绪", "音声モデルの準備完了", "Sprachmodell bereit", "Modelo de voz listo"]),
    ("Download stopped", ["Pobieranie przerwane", "下载已停止", "ダウンロードが停止しました", "Download angehalten", "Descarga detenida"]),
    ("Hex starts listening once the speech model has downloaded.", ["Hex zacznie słuchać, gdy model mowy się pobierze.", "语音模型下载完成后，Hex 就会开始聆听。", "音声モデルのダウンロードが終わると、Hex が聞き取りを始めます。", "Hex hört zu, sobald das Sprachmodell geladen ist.", "Hex empezará a escuchar cuando termine de descargarse el modelo de voz."]),
    ("Device Control and Data Access", ["Sterowanie urządzeniem i dostęp do danych", "设备控制与数据访问", "デバイス制御とデータアクセス", "Gerätesteuerung und Datenzugriff", "Control del dispositivo y acceso a datos"]),
    ("Allowed", ["Dozwolone", "已允许", "許可済み", "Erlaubt", "Permitido"]),
    ("Ask OpenCode to write or rewrite. Turn it on in Voice Action.", ["Poproś OpenCode o napisanie lub przeredagowanie. Włącz w Voice Action.", "让 OpenCode 帮你撰写或改写。在 Voice Action 中开启。", "OpenCode に書いてもらう、書き直してもらう。Voice Action でオンにします。", "Lass OpenCode schreiben oder umschreiben. Aktiviere es unter Voice Action.", "Pide a OpenCode que escriba o reescriba. Actívalo en Voice Action."]),
    ("Back", ["Wstecz", "返回", "戻る", "Zurück", "Atrás"]),
    ("Change", ["Zmień", "更改", "変更", "Ändern", "Cambiar"]),
    ("Continue", ["Dalej", "继续", "続ける", "Weiter", "Continuar"]),
    ("Download a speech model", ["Pobierz model mowy", "下载语音模型", "音声モデルをダウンロード", "Sprachmodell herunterladen", "Descarga un modelo de voz"]),
    ("Drag Hex into the list, or switch it on if it is there", ["Przeciągnij Hex na listę albo włącz go, jeśli już tam jest", "把 Hex 拖进列表；如果已在列表中，请打开开关", "Hex をリストにドラッグするか、すでにあればオンにします", "Zieh Hex in die Liste oder schalte es ein, falls es schon dort ist", "Arrastra Hex a la lista o actívalo si ya está"]),
    ("Fix names, tone, and formatting per app.", ["Poprawiaj nazwy, ton i formatowanie dla każdej aplikacji.", "按应用修正名称、语气和格式。", "アプリごとに名前、口調、書式を整えます。", "Korrigiere Namen, Ton und Formatierung pro App.", "Corrige nombres, tono y formato por app."]),
    ("Get started", ["Zaczynamy", "开始", "はじめる", "Los geht's", "Empezar"]),
    ("Give it a try", ["Wypróbuj", "试一试", "試してみましょう", "Probier es aus", "Pruébalo"]),
    ("Hears you only while you hold the shortcut", ["Słyszy cię tylko, gdy trzymasz skrót", "仅在按住快捷键时收听", "ショートカットを押している間だけ聞き取ります", "Hört nur zu, während du das Kürzel hältst", "Solo te escucha mientras mantienes el atajo"]),
    ("Hex did not catch any words. Speak a little closer, or pick another microphone in Settings.", ["Hex nie usłyszał żadnych słów. Mów bliżej albo wybierz inny mikrofon w Ustawieniach.", "Hex 没有听到任何文字。请靠近一点说话，或在设置中选择其他麦克风。", "Hex は言葉を聞き取れませんでした。もう少し近くで話すか、設定で別のマイクを選んでください。", "Hex hat keine Wörter erkannt. Sprich etwas näher oder wähle in den Einstellungen ein anderes Mikrofon.", "Hex no captó ninguna palabra. Habla un poco más cerca o elige otro micrófono en Ajustes."]),
    ("Hex needs to reopen before it can hear the shortcut.", ["Hex musi zostać ponownie otwarty, aby usłyszeć skrót.", "Hex 需要重新打开才能识别快捷键。", "ショートカットを認識するには Hex を開き直す必要があります。", "Hex muss neu geöffnet werden, um das Kürzel zu erkennen.", "Hex necesita reabrirse para detectar el atajo."]),
    ("Hex transcribes on this computer, so it needs a local model first.", ["Hex transkrybuje na tym komputerze, więc najpierw potrzebuje lokalnego modelu.", "Hex 在这台电脑上转写，因此需要先下载本地模型。", "Hex はこのコンピュータ上で文字起こしするため、まずローカルモデルが必要です。", "Hex transkribiert auf diesem Computer und braucht dafür zuerst ein lokales Modell.", "Hex transcribe en este ordenador, así que primero necesita un modelo local."]),
    ("Hex turns what I say into text, right where I'm typing.", ["Hex zamienia to, co mówię, w tekst dokładnie tam, gdzie piszę.", "Hex 把我说的话直接变成文字，就在我打字的地方。", "Hex は話したことを、入力している場所にそのまま文字にします。", "Hex macht aus dem, was ich sage, Text – genau dort, wo ich tippe.", "Hex convierte lo que digo en texto, justo donde estoy escribiendo."]),
    ("Hex waits in the system tray. Use {} in any app to dictate.", ["Hex czeka w zasobniku systemowym. Użyj {} w dowolnej aplikacji, aby dyktować.", "Hex 待在系统托盘中。在任意应用中使用 {} 即可听写。", "Hex はシステムトレイで待機しています。どのアプリでも {} で音声入力できます。", "Hex wartet im Infobereich. Nutze {} in jeder App zum Diktieren.", "Hex espera en la bandeja del sistema. Usa {} en cualquier app para dictar."]),
    ("Hex keeps running after you close this window. Use {} in any app to dictate.", ["Hex działa dalej po zamknięciu tego okna. Użyj {} w dowolnej aplikacji, aby dyktować.", "关闭此窗口后 Hex 仍会继续运行。在任意应用中使用 {} 即可听写。", "このウィンドウを閉じても Hex は動き続けます。どのアプリでも {} で音声入力できます。", "Hex läuft weiter, wenn du dieses Fenster schließt. Nutze {} in jeder App zum Diktieren.", "Hex sigue funcionando al cerrar esta ventana. Usa {} en cualquier app para dictar."]),
    ("Hex waits in the menu bar. Use {} in any app to dictate.", ["Hex czeka na pasku menu. Użyj {} w dowolnej aplikacji, aby dyktować.", "Hex 待在菜单栏中。在任意应用中使用 {} 即可听写。", "Hex はメニューバーで待機しています。どのアプリでも {} で音声入力できます。", "Hex wartet in der Menüleiste. Nutze {} in jeder App zum Diktieren.", "Hex espera en la barra de menús. Usa {} en cualquier app para dictar."]),
    ("Hold a key, say what you mean, and Hex types it wherever your cursor is. Your voice is transcribed on this Mac.", ["Przytrzymaj klawisz, powiedz, co chcesz, a Hex wpisze to tam, gdzie jest kursor. Głos jest transkrybowany na tym Macu.", "按住一个键，说出你想说的，Hex 会把它输入到光标所在的位置。语音在这台 Mac 上转写。", "キーを押したまま話すと、Hex がカーソルの位置に入力します。音声はこの Mac 上で文字起こしされます。", "Halte eine Taste, sag, was du meinst, und Hex tippt es dort, wo dein Cursor steht. Deine Stimme wird auf diesem Mac transkribiert.", "Mantén una tecla, di lo que quieres y Hex lo escribe donde esté el cursor. Tu voz se transcribe en este Mac."]),
    ("Hold a key, say what you mean, and Hex types it wherever your cursor is. Your voice is transcribed on this computer.", ["Przytrzymaj klawisz, powiedz, co chcesz, a Hex wpisze to tam, gdzie jest kursor. Głos jest transkrybowany na tym komputerze.", "按住一个键，说出你想说的，Hex 会把它输入到光标所在的位置。语音在这台电脑上转写。", "キーを押したまま話すと、Hex がカーソルの位置に入力します。音声はこのコンピュータ上で文字起こしされます。", "Halte eine Taste, sag, was du meinst, und Hex tippt es dort, wo dein Cursor steht. Deine Stimme wird auf diesem Computer transkribiert.", "Mantén una tecla, di lo que quieres y Hex lo escribe donde esté el cursor. Tu voz se transcribe en este ordenador."]),
    ("Hold to talk", ["Przytrzymaj, aby mówić", "按住说话", "押したまま話す", "Halten zum Sprechen", "Mantén para hablar"]),
    ("Hold {}, say a sentence, then let go.", ["Przytrzymaj {}, powiedz zdanie i puść.", "按住 {}，说一句话，然后松开。", "{} を押したまま一文話して、離します。", "Halte {}, sag einen Satz und lass dann los.", "Mantén {}, di una frase y suelta."]),
    ("Let Hex hear you and type for you", ["Pozwól Hex słyszeć cię i pisać za ciebie", "让 Hex 听见你并替你输入", "Hex が聞き取り、入力できるようにする", "Lass Hex zuhören und für dich tippen", "Deja que Hex te escuche y escriba por ti"]),
    ("Listening… let go of {} when you are done.", ["Słucham… puść {}, gdy skończysz.", "正在聆听… 说完后松开 {}。", "聞き取り中… 話し終えたら {} を離してください。", "Hört zu … lass {} los, wenn du fertig bist.", "Escuchando… suelta {} cuando termines."]),
    ("Listening… press {} again when you are done.", ["Słucham… naciśnij {} ponownie, gdy skończysz.", "正在聆听… 说完后再按一次 {}。", "聞き取り中… 話し終えたらもう一度 {} を押してください。", "Hört zu … drück {} erneut, wenn du fertig bist.", "Escuchando… pulsa {} otra vez cuando termines."]),
    ("Missed the text field? Paste it again anywhere.", ["Nie trafiło w pole tekstowe? Wklej ponownie w dowolnym miejscu.", "没粘贴到文本框？可以在任意位置再次粘贴。", "テキスト欄から外れましたか？どこにでももう一度貼り付けられます。", "Textfeld verpasst? Füge es überall erneut ein.", "¿No cayó en el campo de texto? Pégalo de nuevo donde quieras."]),
    ("Notices the shortcut in every app", ["Rozpoznaje skrót w każdej aplikacji", "在所有应用中识别快捷键", "どのアプリでもショートカットを認識します", "Erkennt das Kürzel in jeder App", "Detecta el atajo en cualquier app"]),
    ("Paste your last dictation", ["Wklej ostatnie dyktowanie", "粘贴上一次听写", "直前の音声入力を貼り付け", "Letztes Diktat einfügen", "Pega tu último dictado"]),
    ("Pastes your words into the app you are using", ["Wkleja słowa do aplikacji, której używasz", "把文字粘贴到你正在使用的应用", "使っているアプリに言葉を貼り付けます", "Fügt deine Worte in die App ein, die du gerade nutzt", "Pega tus palabras en la app que estás usando"]),
    ("Press to talk", ["Naciśnij, aby mówić", "按下说话", "押して話す", "Drücken zum Sprechen", "Pulsa para hablar"]),
    ("Press {}, say a sentence, then press it again.", ["Naciśnij {}, powiedz zdanie i naciśnij ponownie.", "按下 {}，说一句话，再按一次。", "{} を押して一文話し、もう一度押します。", "Drück {}, sag einen Satz und drück dann erneut.", "Pulsa {}, di una frase y vuelve a pulsar."]),
    ("Quit and reopen Hex to finish setup. macOS applies some permissions only after a restart.", ["Zamknij i ponownie otwórz Hex, aby dokończyć konfigurację. macOS stosuje niektóre uprawnienia dopiero po ponownym uruchomieniu.", "请退出并重新打开 Hex 以完成设置。macOS 的部分权限需要重启后才会生效。", "設定を完了するには Hex を終了して開き直してください。macOS の一部の許可は再起動後に有効になります。", "Beende Hex und öffne es erneut, um die Einrichtung abzuschließen. macOS übernimmt manche Berechtigungen erst nach einem Neustart.", "Cierra y vuelve a abrir Hex para terminar la configuración. macOS aplica algunos permisos solo tras reiniciar."]),
    ("Reopen Hex", ["Otwórz Hex ponownie", "重新打开 Hex", "Hex を開き直す", "Hex neu öffnen", "Reabrir Hex"]),
    ("Runs entirely on this computer", ["Działa w całości na tym komputerze", "完全在这台电脑上运行", "すべてこのコンピュータ上で動作します", "Läuft vollständig auf diesem Computer", "Funciona por completo en este ordenador"]),
    ("Skip", ["Pomiń", "跳过", "スキップ", "Überspringen", "Omitir"]),
    ("Something went wrong while transcribing. Try once more.", ["Coś poszło nie tak podczas transkrypcji. Spróbuj jeszcze raz.", "转写时出错了。请再试一次。", "文字起こし中に問題が発生しました。もう一度お試しください。", "Beim Transkribieren ist etwas schiefgegangen. Versuch es noch einmal.", "Algo salió mal al transcribir. Inténtalo otra vez."]),
    ("Speech model", ["Model mowy", "语音模型", "音声モデル", "Sprachmodell", "Modelo de voz"]),
    ("Start using Hex", ["Zacznij używać Hex", "开始使用 Hex", "Hex を使い始める", "Hex verwenden", "Empezar a usar Hex"]),
    ("That was a quick tap. Keep holding {} while you speak.", ["To było krótkie stuknięcie. Trzymaj {} podczas mówienia.", "这只是轻点了一下。说话时请一直按住 {}。", "短いタップでした。話している間は {} を押し続けてください。", "Das war nur ein kurzes Tippen. Halte {} gedrückt, während du sprichst.", "Fue un toque rápido. Mantén {} mientras hablas."]),
    ("That was too short to hear. Speak a full sentence before you finish.", ["To było za krótkie. Powiedz całe zdanie, zanim skończysz.", "太短了，没听清。请说完一整句话再结束。", "短すぎて聞き取れませんでした。一文話してから終了してください。", "Das war zu kurz. Sprich einen ganzen Satz, bevor du beendest.", "Fue demasiado corto. Di una frase completa antes de terminar."]),
    ("That's it. Hex pastes wherever your cursor is, in any app.", ["Gotowe. Hex wkleja tam, gdzie jest kursor, w każdej aplikacji.", "就是这样。在任何应用中，Hex 都会粘贴到光标处。", "これで完了です。Hex はどのアプリでもカーソルの位置に貼り付けます。", "Das war's. Hex fügt in jeder App dort ein, wo dein Cursor steht.", "Eso es todo. Hex pega donde esté el cursor, en cualquier app."]),
    ("The download stopped. Check your connection and try again.", ["Pobieranie zostało przerwane. Sprawdź połączenie i spróbuj ponownie.", "下载已中断。请检查网络连接后重试。", "ダウンロードが止まりました。接続を確認してもう一度お試しください。", "Der Download wurde unterbrochen. Prüf deine Verbindung und versuch es erneut.", "La descarga se detuvo. Revisa tu conexión e inténtalo de nuevo."]),
    ("Transcribing…", ["Transkrybowanie…", "正在转写…", "文字起こし中…", "Transkribiert …", "Transcribiendo…"]),
    ("Turned off earlier. Switch Hex on in System Settings.", ["Wcześniej wyłączono. Włącz Hex w Ustawieniach systemowych.", "之前已关闭。请在系统设置中打开 Hex。", "以前にオフにされています。システム設定で Hex をオンにしてください。", "Zuvor ausgeschaltet. Schalte Hex in den Systemeinstellungen ein.", "Se desactivó antes. Activa Hex en Ajustes del Sistema."]),
    ("When you want more", ["Gdy zechcesz więcej", "想要更多时", "もっと使いたくなったら", "Wenn du mehr willst", "Cuando quieras más"]),
    ("You're all set", ["Wszystko gotowe", "一切就绪", "準備完了", "Alles bereit", "Todo listo"]),
    ("Your words will appear here", ["Tu pojawią się twoje słowa", "你的文字会显示在这里", "ここに言葉が表示されます", "Hier erscheinen deine Worte", "Tus palabras aparecerán aquí"]),
    // Fun / Hex Microphone (experimental)
    ("Call hears", ["Rozmówcy słyszą", "通话对方听到", "通話相手に聞こえる音", "Anruf hört", "La llamada oye"]),
    ("Carrier", ["Fala nośna", "载波频率", "キャリア周波数", "Trägerfrequenz", "Portadora"]),
    ("Chipmunk", ["Wiewiórka", "花栗鼠", "チップマンク", "Chipmunk", "Ardilla"]),
    ("Choose Hex Microphone as your input in Slack, Zoom, or Discord", ["Wybierz Hex Microphone jako wejście w Slacku, Zoomie lub Discordzie", "在 Slack、Zoom 或 Discord 中选择 Hex Microphone 作为输入设备", "Slack、Zoom、または Discord の入力として Hex Microphone を選択してください", "Wähle Hex Microphone als Eingabegerät in Slack, Zoom oder Discord", "Elige Hex Microphone como entrada en Slack, Zoom o Discord"]),
    ("Comfort noise", ["Szum tła", "舒适背景音", "コンフォートノイズ", "Komfortrauschen", "Ruido de confort"]),
    ("Compare your raw microphone with what the call hears during dictation", ["Porównaj surowy dźwięk mikrofonu z tym, co słyszą rozmówcy podczas dyktowania", "对比原始麦克风声音与听写时通话对方听到的声音", "元のマイク音声とディクテーション中に相手へ届く音を比較します", "Vergleiche dein Originalmikrofon mit dem, was der Anruf beim Diktieren hört", "Compara tu micrófono original con lo que oye la llamada mientras dictas"]),
    ("Copy install command", ["Kopiuj polecenie instalacji", "复制安装命令", "インストールコマンドをコピー", "Installationsbefehl kopieren", "Copiar comando de instalación"]),
    ("Crunch", ["Przester", "失真度", "クランチ", "Crunch", "Saturación"]),
    ("Depth", ["Głębokość", "深度", "深さ", "Tiefe", "Profundidad"]),
    ("Dictating {} – {}", ["Dyktowanie {} – {}", "听写 {} – {}", "ディクテーション {} – {}", "Diktieren {} – {}", "Dictando {} – {}"]),
    ("Driver not installed", ["Sterownik niezainstalowany", "驱动未安装", "ドライバ未インストール", "Treiber nicht installiert", "Controlador no instalado"]),
    ("Effect strength", ["Siła efektu", "效果强度", "エフェクトの強さ", "Effektstärke", "Intensidad del efecto"]),
    ("Example", ["Przykład", "示例", "サンプル", "Beispiel", "Ejemplo"]),
    ("Experimental: Hex microphone", ["Eksperymentalne: mikrofon Hex", "实验功能：Hex 麦克风", "実験的機能: Hex マイク", "Experimentell: Hex-Mikrofon", "Experimental: micrófono Hex"]),
    ("Far", ["Z daleka", "远处", "遠くから", "Weit weg", "Lejos"]),
    ("Fun", ["Zabawa", "趣玩", "遊び場", "Spaß", "Diversión"]),
    ("Hex Microphone", ["Hex Microphone", "Hex Microphone", "Hex Microphone", "Hex Microphone", "Hex Microphone"]),
    ("In use by {}", ["Używany przez: {}", "{} 正在使用", "{} が使用中", "Verwendet von {}", "En uso por {}"]),
    ("Install", ["Zainstaluj", "安装", "インストール", "Installieren", "Instalar"]),
    ("Install the CoreAudio driver to choose Hex Microphone in Slack, Zoom, or Discord", ["Zainstaluj sterownik CoreAudio, aby wybrać Hex Microphone w Slacku, Zoomie lub Discordzie", "安装 CoreAudio 驱动后即可在 Slack、Zoom 或 Discord 中选择 Hex Microphone", "CoreAudio ドライバをインストールすると、Slack・Zoom・Discord で Hex Microphone を選べます", "Installiere den CoreAudio-Treiber, um Hex Microphone in Slack, Zoom oder Discord auszuwählen", "Instala el controlador CoreAudio para elegir Hex Microphone en Slack, Zoom o Discord"]),
    ("Installation cancelled", ["Instalacja anulowana", "已取消安装", "インストールがキャンセルされました", "Installation abgebrochen", "Instalación cancelada"]),
    ("Installed v{}", ["Zainstalowano v{}", "已安装 v{}", "v{} インストール済み", "v{} installiert", "Instalado v{}"]),
    ("Installing driver and restarting Core Audio…", ["Instalowanie sterownika i restartowanie Core Audio…", "正在安装驱动并重启 Core Audio…", "ドライバをインストールし Core Audio を再起動中…", "Treiber wird installiert und Core Audio neu gestartet…", "Instalando controlador y reiniciando Core Audio…"]),
    ("Installing…", ["Instalowanie…", "正在安装…", "インストール中…", "Wird installiert…", "Instalando…"]),
    ("Cancelled", ["Anulowano", "已取消", "キャンセル", "Abgebrochen", "Cancelado"]),
    ("Could not install driver: {}", ["Nie udało się zainstalować sterownika: {}", "无法安装驱动：{}", "ドライバをインストールできませんでした: {}", "Treiber konnte nicht installiert werden: {}", "No se pudo instalar el controlador: {}"]),
    ("Could not update driver: {}", ["Nie udało się zaktualizować sterownika: {}", "无法更新驱动：{}", "ドライバを更新できませんでした: {}", "Treiber konnte nicht aktualisiert werden: {}", "No se pudo actualizar el controlador: {}"]),
    ("Driver removed", ["Sterownik usunięty", "驱动已移除", "ドライバを削除しました", "Treiber entfernt", "Controlador eliminado"]),
    ("Removed", ["Usunięto", "已移除", "削除済み", "Entfernt", "Eliminado"]),
    ("Removing driver and restarting Core Audio…", ["Usuwanie sterownika i restartowanie Core Audio…", "正在移除驱动并重启 Core Audio…", "ドライバを削除し Core Audio を再起動中…", "Treiber wird entfernt und Core Audio neu gestartet…", "Eliminando controlador y reiniciando Core Audio…"]),
    ("Uninstall", ["Odinstaluj", "卸载", "アンインストール", "Deinstallieren", "Desinstalar"]),
    ("Uninstall cancelled", ["Odinstalowanie anulowane", "已取消卸载", "アンインストールがキャンセルされました", "Deinstallation abgebrochen", "Desinstalación cancelada"]),
    ("Uninstalling…", ["Odinstalowywanie…", "正在卸载…", "アンインストール中…", "Wird deinstalliert…", "Desinstalando…"]),
    ("Update cancelled", ["Aktualizacja anulowana", "已取消更新", "更新がキャンセルされました", "Update abgebrochen", "Actualización cancelada"]),
    ("Update driver to v{}", ["Zaktualizuj sterownik do v{}", "将驱动更新到 v{}", "ドライバを v{} に更新", "Treiber auf v{} aktualisieren", "Actualizar controlador a v{}"]),
    ("Update v{}", ["Aktualizacja v{}", "更新 v{}", "v{} に更新", "Update v{}", "Actualizar v{}"]),
    ("v{} installed", ["Zainstalowano v{}", "已安装 v{}", "v{} インストール済み", "v{} installiert", "v{} instalado"]),
    ("Keeps a quiet room tone while muted so callers know the line is live", ["Zachowuje cichy szum pokoju podczas wyciszenia, by rozmówcy wiedzieli, że połączenie trwa", "静音时保留轻微环境底噪，让对方知道通话仍在线", "ミュート中もわずかなルームトーンを残し、通話が繋がっていることを伝えます", "Behält beim Stummschalten einen leisen Raumton bei, damit die Leitung lebendig wirkt", "Mantiene un tono ambiental suave al silenciar para que la llamada siga viva"]),
    ("Level", ["Poziom", "音量", "レベル", "Pegel", "Nivel"]),
    ("Mode while dictating", ["Tryb podczas dyktowania", "听写时的声音模式", "ディクテーション中のモード", "Modus beim Diktieren", "Modo al dictar"]),
    ("Muffled", ["Przytłumiony", "捂麦", "こもった声", "Gedämpft", "Amortiguado"]),
    ("Muted", ["Wyciszony", "静音", "ミュート", "Stumm", "Silenciado"]),
    ("Normal", ["Normalny", "正常", "通常", "Normal", "Normal"]),
    ("Open Fun", ["Otwórz Zabawę", "打开趣玩", "遊び場を開く", "Spaß öffnen", "Abrir Diversión"]),
    ("Passes your voice through unchanged while you dictate", ["Przekazuje twój głos bez zmian podczas dyktowania", "听写时原样传递你的声音", "ディクテーション中も声をそのまま通します", "Leitet deine Stimme beim Diktieren unverändert weiter", "Deja pasar tu voz sin cambios mientras dictas"]),
    ("Pitch", ["Wysokość tonu", "音高", "ピッチ", "Tonhöhe", "Tono"]),
    ("Play what call hears", ["Odtwórz co słyszy rozmówca", "播放通话对方听到的声音", "相手に聞こえる音を再生", "Abspielen, was der Anruf hört", "Reproducir lo que oye la llamada"]),
    ("Play your voice", ["Odtwórz swój głos", "播放你的原声", "自分の声を再生", "Eigene Stimme abspielen", "Reproducir tu voz"]),
    ("Quiet", ["Cichy", "轻声", "小声", "Leise", "Suave"]),
    ("Record 5s", ["Nagraj 5 s", "录制 5 秒", "5秒録音", "5 Sek. aufnehmen", "Grabar 5 s"]),
    ("Reset", ["Resetuj", "重置", "リセット", "Zurücksetzen", "Restablecer"]),
    ("Robot", ["Robot", "机器人", "ロボット", "Roboter", "Robot"]),
    ("Room", ["Pogłos", "房间混响", "ルーム残響", "Raumklang", "Sala"]),
    ("Room tone", ["Szum pokoju", "环境底噪", "ルームトーン", "Raumton", "Tono ambiental"]),
    ("Silences your voice with soft room tone while you dictate", ["Wycisza twój głos delikatnym szumem pokoju podczas dyktowania", "听写时用柔和的环境底噪隐去你的声音", "ディクテーション中は柔らかなルームトーンで声をミュートします", "Schaltet deine Stimme beim Diktieren mit sanftem Raumton stumm", "Silencia tu voz con un suave tono ambiental mientras dictas"]),
    ("Sounds like a hand over the microphone while you dictate", ["Brzmi jak dłoń zakrywająca mikrofon podczas dyktowania", "听写时听起来像用手捂住了麦克风", "ディクテーション中、マイクを手で覆ったような音になります", "Klingt beim Diktieren wie eine Hand über dem Mikrofon", "Suena como si taparas el micrófono con la mano mientras dictas"]),
    ("Sounds like a handheld radio while you dictate", ["Brzmi jak krótkofalówka podczas dyktowania", "听写时听起来像对讲机", "ディクテーション中、トランシーバーのような音になります", "Klingt beim Diktieren wie ein Funkgerät", "Suena como un walkie-talkie mientras dictas"]),
    ("Sounds like you are across the room while you dictate", ["Brzmi jakbyś mówił z drugiego końca pokoju podczas dyktowania", "听写时听起来像在房间另一头说话", "ディクテーション中、部屋の離れた場所から話しているように聞こえます", "Klingt beim Diktieren, als wärst du am anderen Ende des Raums", "Suena como si hablaras desde el otro lado de la habitación mientras dictas"]),
    ("Sounds pitched up while you dictate", ["Podwyższa ton głosu podczas dyktowania", "听写时声音变尖", "ディクテーション中、声が高くなります", "Klingt beim Diktieren höher gepitcht", "Suena con tono agudo mientras dictas"]),
    ("Sounds robotic while you dictate", ["Brzmi jak głos robota podczas dyktowania", "听写时听起来像机器人", "ディクテーション中、ロボットのような声になります", "Klingt beim Diktieren wie ein Roboter", "Suena robótico mientras dictas"]),
    ("Sounds submerged while you dictate", ["Brzmi jak spod wody podczas dyktowania", "听写时听起来像在水下", "ディクテーション中、水中にいるような音になります", "Klingt beim Diktieren wie unter Wasser", "Suena como bajo el agua mientras dictas"]),
    ("Stays in memory", ["Tylko w pamięci", "仅保存在内存中", "メモリ内のみに保持", "Bleibt im Speicher", "Solo en memoria"]),
    ("Stop", ["Zatrzymaj", "停止", "停止", "Stopp", "Detener"]),
    ("Stop recording", ["Zatrzymaj nagrywanie", "停止录制", "録音を停止", "Aufnahme stoppen", "Detener grabación"]),
    ("Streaming 48 kHz audio to {}", ["Strumieniowanie 48 kHz do: {}", "正在向 {} 传输 48 kHz 音频", "{} へ 48 kHz 音声を配信中", "Streamt 48-kHz-Audio an {}", "Transmitiendo audio de 48 kHz a {}"]),
    ("Try Hex Microphone and preview how callers hear you while you dictate", ["Wypróbuj Hex Microphone i posłuchaj, jak słyszą cię rozmówcy podczas dyktowania", "试用 Hex Microphone，预览听写时通话对方听到的效果", "Hex Microphone を試し、ディクテーション中に相手へどう聞こえるか確認します", "Teste Hex Microphone und höre, wie Anrufer dich beim Diktieren hören", "Prueba Hex Microphone y escucha cómo te oyen en la llamada mientras dictas"]),
    ("Try it", ["Wypróbuj", "试听与录制", "試してみる", "Ausprobieren", "Pruébalo"]),
    ("Turns your voice down to a whisper while you dictate", ["Ścisza twój głos do szeptu podczas dyktowania", "听写时将你的声音压低到耳语音量", "ディクテーション中、声をささやき声まで下げます", "Senkt deine Stimme beim Diktieren auf Flüstern ab", "Baja tu voz a un susurro mientras dictas"]),
    ("Underwater", ["Pod wodą", "水下", "水中", "Unter Wasser", "Bajo el agua"]),
    ("Virtual microphone", ["Wirtualny mikrofon", "虚拟麦克风", "仮想マイク", "Virtuelles Mikrofon", "Micrófono virtual"]),
    ("Virtual microphone driver", ["Sterownik wirtualnego mikrofonu", "虚拟麦克风驱动", "仮想マイクドライバ", "Virtueller Mikrofontreiber", "Controlador de micrófono virtual"]),
    ("Walkie", ["Krótkofalówka", "对讲机", "トランシーバー", "Walkie-Talkie", "Walkie"]),
    ("Wall", ["Tłumienie", "闷音滤波", "こもり具合", "Dämpfung", "Filtro"]),
    ("You", ["Ty", "你", "あなた", "Du", "Tú"]),
    ("Your 5s sample", ["Twoja próbka 5 s", "你的 5 秒录音", "あなたの5秒サンプル", "Deine 5-Sek.-Aufnahme", "Tu muestra de 5 s"]),
    ("Your voice", ["Twój głos", "你的声音", "自分の声", "Deine Stimme", "Tu voz"]),
];

#[cfg(test)]
mod tests {
    use super::TRANSLATIONS;
    use super::{language_from_env, primary_subtag};

    #[test]
    fn preferred_language_tags_reduce_to_their_primary_subtag() {
        assert_eq!(primary_subtag("pt-BR").as_deref(), Some("pt"));
        assert_eq!(primary_subtag("zh-Hans-CN").as_deref(), Some("zh"));
        assert_eq!(primary_subtag("en").as_deref(), Some("en"));
        assert_eq!(primary_subtag(""), None);
    }

    #[test]
    fn posix_locale_variables_resolve_in_precedence_order() {
        assert_eq!(language_from_env(Some("pl_PL.UTF-8"), None, None), "pl");
        assert_eq!(
            language_from_env(None, Some("de_DE"), Some("es_ES.UTF-8")),
            "de"
        );
        assert_eq!(language_from_env(None, None, Some("ja_JP.UTF-8")), "ja");
        assert_eq!(language_from_env(Some("C"), None, Some("pl_PL")), "pl");
        assert_eq!(language_from_env(None, None, Some("POSIX")), "");
        assert_eq!(language_from_env(None, None, None), "");
    }

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

    /// Every string the shared window translates, in every OS's source.
    const SOURCES: &[&str] = &[
        include_str!("app_window.rs"),
        include_str!("app_window/macos.rs"),
        include_str!("app_window/windows.rs"),
        include_str!("app_window/linux.rs"),
        include_str!("app_window/tour.rs"),
        include_str!("desktop_transcription_picker.rs"),
        include_str!("windows_app.rs"),
    ];

    /// The string literals passed to `tr`, `tr_fill`, and `tr_named`.
    fn translated_literals(source: &str) -> Vec<String> {
        let mut literals = Vec::new();
        for call in ["tr(", "tr_fill(", "tr_named("] {
            for (start, _) in source.match_indices(call) {
                let preceded_by_ident = source[..start]
                    .chars()
                    .next_back()
                    .is_some_and(|character| character.is_alphanumeric() || character == '_');
                if preceded_by_ident {
                    continue;
                }
                let rest = source[start + call.len()..].trim_start();
                let Some(rest) = rest.strip_prefix('"') else {
                    continue;
                };
                let mut literal = String::new();
                let mut characters = rest.chars();
                while let Some(character) = characters.next() {
                    match character {
                        '"' => break,
                        '\\' => literal.extend(characters.next()),
                        other => literal.push(other),
                    }
                }
                literals.push(literal);
            }
        }
        literals
    }

    #[test]
    fn every_translated_string_has_every_language() {
        let table: std::collections::HashMap<_, _> = TRANSLATIONS.iter().copied().collect();
        let mut checked = 0;
        for source in SOURCES {
            for literal in translated_literals(source) {
                let translations = table
                    .get(literal.as_str())
                    .unwrap_or_else(|| panic!("{literal:?} has no translations"));
                assert!(
                    translations
                        .iter()
                        .all(|translation| !translation.is_empty()),
                    "{literal:?} is missing a language"
                );
                checked += 1;
            }
        }
        assert!(checked > 300, "only {checked} translated strings found");
    }

    #[test]
    fn named_placeholders_survive_translation() {
        for (english, translated) in TRANSLATIONS {
            let names = english
                .split('{')
                .skip(1)
                .filter_map(|part| part.split_once('}'))
                .map(|(name, _)| name)
                .filter(|name| !name.is_empty());
            for name in names {
                for translation in translated {
                    assert!(
                        translation.contains(&format!("{{{name}}}")),
                        "translation of {english:?} lost {{{name}}}: {translation:?}"
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
