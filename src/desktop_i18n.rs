//! UI translations for the port shells, keyed by the English source
//! string. The language follows the system locale unless a persisted
//! interface-language choice overrides it; unknown locales and
//! untranslated strings fall back to English. Composite strings are `{}`
//! templates filled through [`tr_fill`].

use std::collections::HashMap;
use std::sync::OnceLock;
#[cfg(not(test))]
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
    (Some("uk"), "Українська"),
];

/// Active language column into [`TRANSLATIONS`]; 255 means English.
#[cfg(not(test))]
static CURRENT: AtomicU8 = AtomicU8::new(255);

// Tests render scenes in different languages concurrently, so each test
// thread has its own language.
#[cfg(test)]
thread_local! {
    static CURRENT: std::cell::Cell<u8> = const { std::cell::Cell::new(ENGLISH) };
}

#[cfg(not(test))]
fn current() -> u8 {
    CURRENT.load(Ordering::Relaxed)
}

#[cfg(not(test))]
fn set_current(code: u8) {
    CURRENT.store(code, Ordering::Relaxed);
}

#[cfg(test)]
fn current() -> u8 {
    CURRENT.get()
}

#[cfg(test)]
fn set_current(code: u8) {
    CURRENT.set(code);
}

const ENGLISH: u8 = 255;

fn column_for_code(code: &str) -> u8 {
    match code {
        "pl" => 0,
        "zh" => 1,
        "ja" => 2,
        "de" => 3,
        "es" => 4,
        "uk" => 5,
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
    set_current(code);
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

/// Translate one UI string. Returns the input unchanged for English or for
/// strings missing from the table, so dynamic values pass through safely.
pub(crate) fn tr(english: &str) -> &str {
    let column = current();
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

fn tables() -> &'static [HashMap<&'static str, &'static str>; 6] {
    static TABLES: OnceLock<[HashMap<&'static str, &'static str>; 6]> = OnceLock::new();
    TABLES.get_or_init(|| {
        std::array::from_fn(|column| {
            TRANSLATIONS
                .iter()
                .map(|(english, translated)| (*english, translated[column]))
                .collect()
        })
    })
}

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish, Ukrainian.
#[rustfmt::skip]
const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Don't switch", ["Nie zmieniaj", "不切换", "切り替えない", "Nicht wechseln", "No cambiar", "Не перемикати"]),
    ("Not installed", ["Nie zainstalowano", "未安装", "未インストール", "Nicht installiert", "No instalado", "Не встановлено"]),
    ("Switch model with keyboard language", ["Zmieniaj model z językiem klawiatury", "随键盘语言切换模型", "キーボード言語に合わせてモデルを切り替える", "Modell mit Tastatursprache wechseln", "Cambiar modelo con el idioma del teclado", "Перемикати модель разом із мовою клавіатури"]),
    ("Changing input source selects its mapped model and language; manual choices still work", ["Zmiana źródła wprowadzania wybiera przypisany model i język; nadal można wybrać je ręcznie", "更改输入源时选择对应的模型和语言；仍可手动选择", "入力ソースを変更すると対応するモデルと言語を選択します。手動でも選択できます", "Ein Wechsel der Eingabequelle wählt das zugeordnete Modell und die Sprache; manuelle Auswahl bleibt möglich", "Cambiar la fuente de entrada selecciona su modelo e idioma; la selección manual sigue disponible", "Зміна джерела введення вибирає призначені йому модель і мову; ручний вибір теж працює"]),
    ("Shortcut side", ["Strona skrótu", "快捷键侧", "ショートカットの左右", "Seite des Kürzels", "Lado del atajo", "Бік комбінації клавіш"]),
    ("Labs", ["Laboratorium", "实验室", "ラボ", "Labs", "Labs", "Лабораторія"]),
    ("Record your screen, draw on it, and talk over it", ["Nagrywaj ekran, rysuj po nim i mów w trakcie", "录制屏幕，边画边说", "画面を録画しながら描き、話します", "Bildschirm aufnehmen, darauf zeichnen und dazu sprechen", "Graba la pantalla, dibuja sobre ella y habla mientras tanto", "Записуйте екран, малюйте на ньому й коментуйте голосом"]),
    ("Hex Microphone virtual input and real-time voice filters", ["Wirtualne wejście Hex Microphone i filtry głosu na żywo", "Hex Microphone 虚拟输入与实时声音滤镜", "Hex Microphone 仮想入力とリアルタイムのボイスフィルター", "Virtueller Eingang Hex Microphone und Echtzeit-Stimmfilter", "Entrada virtual Hex Microphone y filtros de voz en tiempo real", "Віртуальний вхід Hex Microphone та голосові фільтри наживо"]),
    ("Live text", ["Tekst na żywo", "实时文字", "ライブテキスト", "Live-Text", "Texto en vivo", "Текст наживо"]),
    ("Show provisional words beside the HUD while dictating (applies on restart)", ["Pokazuj robocze słowa obok HUD-u podczas dyktowania (po ponownym uruchomieniu)", "听写时在 HUD 旁显示临时词语（重启后生效）", "音声入力中に HUD の横に仮の語句を表示します（再起動後に反映）", "Vorläufige Wörter beim Diktieren neben dem HUD zeigen (nach Neustart)", "Muestra palabras provisionales junto al HUD al dictar (al reiniciar)", "Показувати проміжні слова поруч із HUD під час диктування (після перезапуску)"]),
    ("Live chapters", ["Rozdziały na żywo", "实时章节", "ライブチャプター", "Live-Kapitel", "Capítulos en vivo", "Розділи наживо"]),
    ("Break meeting transcripts into chapters while recording", ["Dziel transkrypcje spotkań na rozdziały podczas nagrywania", "在录制会议时将转录划分为章节", "会議の録音中に文字起こしをチャプターに分けます", "Meeting-Transkripte während der Aufnahme in Kapitel gliedern", "Divide las transcripciones de reuniones en capítulos mientras grabas", "Розбивати транскрипти зустрічей на розділи під час запису"]),
    ("Scene and story browser in the window (applies on restart)", ["Przeglądarka scen i makiet w oknie (po ponownym uruchomieniu)", "窗口内的场景与示例浏览器（重启后生效）", "ウィンドウ内のシーンとストーリーのブラウザー（再起動後に反映）", "Szenen- und Story-Browser im Fenster (nach Neustart)", "Explorador de escenas e historias en la ventana (al reiniciar)", "Переглядач сцен та історій у вікні (після перезапуску)"]),
    ("Open apps and press keys by voice", ["Otwieraj aplikacje i naciskaj klawisze głosem", "用语音打开应用和按键", "声でアプリを開き、キーを押します", "Apps öffnen und Tasten drücken per Stimme", "Abre apps y pulsa teclas con la voz", "Відкривайте застосунки й натискайте клавіші голосом"]),
    ("Record calls and keep transcripts", ["Nagrywaj rozmowy i zachowuj transkrypcje", "录制通话并保留转录", "通話を録音して文字起こしを残します", "Anrufe aufnehmen und Transkripte behalten", "Graba llamadas y guarda transcripciones", "Записуйте дзвінки й зберігайте транскрипти"]),
    ("Try unfinished features. They may change or break.", ["Wypróbuj niedokończone funkcje. Mogą się zmienić lub przestać działać.", "试用尚未完成的功能。它们可能会变化或出错。", "未完成の機能を試せます。変更されたり、動かなくなったりすることがあります。", "Probiere unfertige Funktionen aus. Sie können sich ändern oder kaputtgehen.", "Prueba funciones sin terminar. Pueden cambiar o fallar.", "Спробуйте незавершені функції. Вони можуть змінитися або зламатися."]),
    ("On while the Hex Microphone driver is installed. Uninstall the driver in Voice Filter to turn it off.", ["Włączony, dopóki zainstalowany jest sterownik Hex Microphone. Odinstaluj sterownik w Filtrze głosu, aby go wyłączyć.", "安装 Hex Microphone 驱动期间保持开启。在声音滤镜中卸载驱动即可关闭。", "Hex Microphone ドライバーがインストールされている間はオンです。オフにするにはボイスフィルターでドライバーをアンインストールしてください。", "Eingeschaltet, solange der Hex-Microphone-Treiber installiert ist. Deinstalliere den Treiber im Stimmfilter, um ihn auszuschalten.", "Activado mientras el controlador Hex Microphone esté instalado. Desinstala el controlador en Filtro de voz para desactivarlo.", "Увімкнено, доки встановлено драйвер Hex Microphone. Видаліть драйвер у Голосовому фільтрі, щоб вимкнути."]),
    ("Vocabulary biasing", ["Wzmacnianie słownictwa", "词汇偏置", "語彙バイアス", "Vokabular-Gewichtung", "Sesgo de vocabulario", "Підсилення словника"]),
    ("Bias Parakeet transcription toward your vocabulary; rewriting and search use it either way", ["Nakierowuje transkrypcję Parakeet na Twoje słownictwo; przepisywanie i wyszukiwanie korzystają z niego zawsze", "让 Parakeet 转录偏向你的词汇；改写和搜索始终会使用它", "Parakeet の文字起こしをあなたの語彙に寄せます。書き換えと検索では常に使われます", "Gewichtet die Parakeet-Transkription zu deinem Vokabular; Umschreiben und Suche nutzen es ohnehin", "Orienta la transcripción de Parakeet hacia tu vocabulario; la reescritura y la búsqueda lo usan igualmente", "Спрямовує транскрипцію Parakeet до вашого словника; переписування й пошук використовують його завжди"]),
    ("Transcription uses these only with Labs › Vocabulary biasing on; rewriting and search always do", ["Transkrypcja używa ich tylko z włączonym Laboratorium › Wzmacnianie słownictwa; przepisywanie i wyszukiwanie zawsze", "仅在开启实验室 › 词汇偏置时转录才会使用它们；改写和搜索始终会使用", "文字起こしでは ラボ › 語彙バイアス がオンのときだけ使われます。書き換えと検索では常に使われます", "Die Transkription nutzt sie nur mit Labs › Vokabular-Gewichtung; Umschreiben und Suche immer", "La transcripción solo los usa con Labs › Sesgo de vocabulario activado; la reescritura y la búsqueda siempre", "Транскрипція використовує їх лише з увімкненим Лабораторія › Підсилення словника; переписування й пошук — завжди"]),
    ("Mouse shortcuts", ["Skróty myszy", "鼠标快捷键", "マウスショートカット", "Maus-Kürzel", "Atajos de ratón", "Комбінації з мишею"]),
    ("Record mouse buttons as shortcuts", ["Nagrywaj przyciski myszy jako skróty", "将鼠标按键录制为快捷键", "マウスボタンをショートカットとして記録します", "Maustasten als Kürzel aufnehmen", "Graba botones del ratón como atajos", "Записувати кнопки миші як комбінації"]),
    ("While dictating with Pause media: YouTube and other tabs, resumed only if Hex paused them", ["Podczas dyktowania z Wstrzymaj multimedia: YouTube i inne karty, wznawiane tylko, jeśli wstrzymał je Hex", "听写时搭配“暂停媒体”：YouTube 等标签页，仅在由 Hex 暂停时才会恢复", "「メディアを一時停止」での音声入力中：YouTube などのタブ。Hex が止めた場合だけ再開します", "Beim Diktieren mit „Medien pausieren“: YouTube und andere Tabs, nur fortgesetzt, wenn Hex sie pausiert hat", "Al dictar con Pausar multimedia: YouTube y otras pestañas, reanudadas solo si Hex las pausó", "Під час диктування з «Призупиняти медіа»: YouTube та інші вкладки, відновлюються, лише якщо їх призупинив Hex"]),
    ("Mouse shortcuts are in Labs now, so your mouse-button shortcut went back to the default. Turn on Labs › Mouse shortcuts to use one again.", ["Skróty myszy są teraz w Laboratorium, więc skrót przycisku myszy wrócił do domyślnego. Włącz Laboratorium › Skróty myszy, aby znów go używać.", "鼠标快捷键已移至实验室，因此你的鼠标按键快捷键已恢复为默认值。开启实验室 › 鼠标快捷键即可再次使用。", "マウスショートカットはラボに移ったため、マウスボタンのショートカットは既定に戻りました。再び使うにはラボ › マウスショートカットをオンにしてください。", "Maus-Kürzel sind jetzt in Labs, daher wurde dein Maustasten-Kürzel auf den Standard zurückgesetzt. Schalte Labs › Maus-Kürzel ein, um wieder eines zu nutzen.", "Los atajos de ratón están ahora en Labs, así que tu atajo con botón del ratón volvió al predeterminado. Activa Labs › Atajos de ratón para volver a usar uno.", "Комбінації з мишею тепер у Лабораторії, тож вашу комбінацію з кнопкою миші повернуто до типової. Увімкніть Лабораторія › Комбінації з мишею, щоб знову її використати."]),
    ("Send feedback", ["Wyślij opinię", "发送反馈", "フィードバックを送信", "Feedback senden", "Enviar comentarios", "Надіслати відгук"]),
    ("Run setup again", ["Uruchom konfigurację ponownie", "重新运行设置", "セットアップをもう一度実行", "Einrichtung erneut ausführen", "Volver a ejecutar la configuración", "Запустити налаштування знову"]),
    ("Play the first run again. Settings, models, and permissions stay", ["Przejdź pierwsze uruchomienie jeszcze raz. Ustawienia, modele i uprawnienia zostają", "再次体验首次运行。设置、模型和权限保持不变", "初回起動をもう一度行います。設定、モデル、権限はそのままです", "Den ersten Start noch einmal durchlaufen. Einstellungen, Modelle und Berechtigungen bleiben", "Repite el primer inicio. La configuración, los modelos y los permisos se conservan", "Пройти перший запуск ще раз. Налаштування, моделі й дозволи залишаться"]),
    ("Run", ["Uruchom", "运行", "実行", "Ausführen", "Ejecutar", "Запустити"]),
    ("Turn a feature on and it joins the sidebar.", ["Włącz funkcję, a pojawi się na pasku bocznym.", "开启功能后，它会出现在侧边栏中。", "機能をオンにすると、サイドバーに加わります。", "Schalte eine Funktion ein, dann erscheint sie in der Seitenleiste.", "Activa una función y aparecerá en la barra lateral.", "Увімкніть функцію, і вона з’явиться на бічній панелі."]),
    ("Clear history", ["Wyczyść historię", "清除历史记录", "履歴を消去", "Verlauf löschen", "Borrar historial", "Очистити історію"]),
    ("Keep for", ["Przechowuj przez", "保留时长", "保存期間", "Behalten für", "Conservar durante", "Зберігати"]),
    ("Annotate", ["Adnotacje", "Annotate", "Annotate", "Annotate", "Annotate", "Annotate"]),
    ("Tap to start and stop; hold while recording to draw", ["Stuknij, aby zacząć i zakończyć; podczas nagrywania przytrzymaj, aby rysować", "轻按开始和停止；录制时按住即可绘制", "タップで開始と停止、録画中は押したままで描画", "Tippen zum Starten und Stoppen; während der Aufnahme halten zum Zeichnen", "Toca para empezar y parar; mantén pulsado mientras graba para dibujar", "Торкніться, щоб почати й зупинити; під час запису утримуйте, щоб малювати"]),
    ("Tap the shortcut to start recording; talk as you go", ["Stuknij skrót, aby zacząć nagrywać; mów na bieżąco", "轻按快捷键开始录制，边做边说", "ショートカットをタップすると録画が始まります。話しながら進めてください", "Tippe auf das Kürzel, um die Aufnahme zu starten; sprich dabei", "Toca el atajo para empezar a grabar; habla mientras tanto", "Торкніться сполучення клавіш, щоб почати запис; говоріть по ходу"]),
    ("While it records, hold the shortcut and move the pointer; no click needed", ["Podczas nagrywania przytrzymaj skrót i poruszaj wskaźnikiem; bez klikania", "录制时按住快捷键并移动指针，无需点击", "録画中にショートカットを押したままポインタを動かします。クリックは不要です", "Halte während der Aufnahme das Kürzel und bewege den Zeiger; ohne Klicken", "Mientras graba, mantén el atajo y mueve el puntero; sin hacer clic", "Під час запису утримуйте сполучення клавіш і рухайте вказівником; клацати не треба"]),
    ("While it records, hold the shortcut and move the pointer; no click needed. Shift draws too", ["Podczas nagrywania przytrzymaj skrót i poruszaj wskaźnikiem; bez klikania. Shift też rysuje", "录制时按住快捷键并移动指针，无需点击。按住 Shift 也能绘制", "録画中にショートカットを押したままポインタを動かします。クリックは不要です。Shift でも描けます", "Halte während der Aufnahme das Kürzel und bewege den Zeiger; ohne Klicken. Umschalt zeichnet auch", "Mientras graba, mantén el atajo y mueve el puntero; sin hacer clic. Mayús también dibuja", "Під час запису утримуйте сполучення клавіш і рухайте вказівником; клацати не треба. Shift теж малює"]),
    ("Pen", ["Pióro", "画笔", "ペン", "Stift", "Lápiz", "Перо"]),
    ("Or click the pen on the bar, then drag to draw; click it again or press Esc to stop", ["Albo kliknij pióro na pasku i przeciągnij, aby rysować; kliknij je ponownie lub naciśnij Esc, aby zakończyć", "或点击栏上的画笔，然后拖动绘制；再次点击或按 Esc 停止", "またはバーのペンをクリックし、ドラッグして描きます。もう一度クリックするか Esc で終了します", "Oder klicke auf den Stift in der Leiste und ziehe zum Zeichnen; klicke erneut oder drücke Esc zum Beenden", "O haz clic en el lápiz de la barra y arrastra para dibujar; haz clic de nuevo o pulsa Esc para parar", "Або натисніть перо на панелі й перетягуйте, щоб малювати; натисніть його знову або Esc, щоб зупинити"]),
    ("Tap the shortcut again: the annotation is copied, ready to paste", ["Stuknij skrót ponownie: adnotacja zostaje skopiowana i jest gotowa do wklejenia", "再次轻按快捷键：批注已复制，可直接粘贴", "もう一度ショートカットをタップすると、注釈がコピーされ、貼り付けられる状態になります", "Tippe erneut auf das Kürzel: Die Annotation wird kopiert und ist bereit zum Einfügen", "Vuelve a tocar el atajo: la anotación se copia, lista para pegar", "Торкніться сполучення клавіш ще раз: анотацію скопійовано, її можна вставити"]),
    ("Draw", ["Rysuj", "绘制", "描く", "Zeichnen", "Dibujar", "Малювати"]),
    ("Paste it anywhere, or click Send to… on the capsule to pick a session", ["Wklej ją w dowolnym miejscu albo kliknij Wyślij do… na kapsule, aby wybrać sesję", "可粘贴到任意位置，或点击胶囊上的“发送到…”选择会话", "どこにでも貼り付けるか、カプセルの「送信先…」をクリックしてセッションを選びます", "Füge sie überall ein oder klicke auf der Kapsel auf „Senden an…“, um eine Sitzung zu wählen", "Pégala donde quieras o haz clic en Enviar a… en la cápsula para elegir una sesión", "Вставте її будь-де або натисніть «Надіслати до…» на капсулі, щоб вибрати сесію"]),
    #[cfg(hex_capture)]
    ("Record your screen, draw on it, and talk over it with the shortcut below", ["Nagrywaj ekran, rysuj na nim i komentuj głosem za pomocą skrótu poniżej", "用下方快捷键录制屏幕、在上面绘制并配上讲解", "下のショートカットで画面を録画し、描き込み、話しながら説明できます", "Mit dem Kürzel unten den Bildschirm aufnehmen, darauf zeichnen und dazu sprechen", "Graba tu pantalla, dibuja sobre ella y coméntala con el atajo de abajo", "Записуйте екран, малюйте на ньому й коментуйте голосом за допомогою комбінації нижче"]),
    #[cfg(hex_capture)]
    ("Off. Hex records nothing and keeps no shortcut until you turn this on.", ["Wyłączone. Hex niczego nie nagrywa i nie zajmuje skrótu, dopóki tego nie włączysz.", "已关闭。打开之前，Hex 不录制任何内容，也不占用快捷键。", "オフです。オンにするまで、Hex は何も録画せず、ショートカットも使いません。", "Aus. Hex nimmt nichts auf und belegt kein Kürzel, bis du es einschaltest.", "Desactivado. Hex no graba nada ni ocupa ningún atajo hasta que lo actives.", "Вимкнено. Hex нічого не записує й не займає комбінацію клавіш, доки ви це не ввімкнете."]),
    #[cfg(hex_capture)]
    ("The capture shortcut is already in use. Change it below before turning captures on.", ["Skrót nagrywania jest już używany. Zmień go poniżej, zanim włączysz nagrywanie.", "捕获快捷键已被占用。请先在下方更改，再开启捕获。", "キャプチャのショートカットはすでに使われています。オンにする前に下で変更してください。", "Das Aufnahmekürzel wird bereits verwendet. Ändere es unten, bevor du Aufnahmen einschaltest.", "El atajo de captura ya está en uso. Cámbialo abajo antes de activar las capturas.", "Сполучення клавіш для записів уже використовується. Змініть його нижче, перш ніж вмикати записи."]),
    #[cfg(hex_capture)]
    ("Could not update Annotate:", ["Nie udało się zaktualizować Adnotacji:", "无法更新 Annotate：", "Annotate を更新できませんでした:", "Annotate konnte nicht aktualisiert werden:", "No se pudo actualizar Annotate:", "Не вдалося оновити Annotate:"]),
    #[cfg(hex_capture)]
    ("The model Hex dictates with, with a time for every word", ["Model, którym Hex dyktuje, z czasem każdego słowa", "Hex 听写所用的模型，并标注每个词的时间", "Hex が音声入力に使うモデル。単語ごとの時刻付き", "Das Modell, mit dem Hex diktiert, mit einer Zeit für jedes Wort", "El modelo con el que Hex dicta, con el tiempo de cada palabra", "Модель, якою диктує Hex, з часом для кожного слова"]),
    #[cfg(hex_capture)]
    ("Apple's speech recognizer, as Scrawl uses", ["Rozpoznawanie mowy Apple, jak w Scrawl", "Apple 语音识别，与 Scrawl 相同", "Scrawl と同じ Apple の音声認識", "Apples Spracherkennung, wie Scrawl sie nutzt", "El reconocimiento de voz de Apple, como en Scrawl", "Розпізнавання мовлення Apple, як у Scrawl"]),
    #[cfg(hex_capture)]
    ("Video for 7 days", ["Wideo przez 7 dni", "视频保留 7 天", "ビデオは 7 日間", "Video 7 Tage", "Vídeo durante 7 días", "Відео 7 днів"]),
    #[cfg(hex_capture)]
    ("Video for 30 days", ["Wideo przez 30 dni", "视频保留 30 天", "ビデオは 30 日間", "Video 30 Tage", "Vídeo durante 30 días", "Відео 30 днів"]),
    #[cfg(hex_capture)]
    ("Video for {} days", ["Wideo przez {} dni", "视频保留 {} 天", "ビデオは {} 日間", "Video {} Tage", "Vídeo durante {} días", "Відео {} дн."]),
    #[cfg(hex_capture)]
    ("Video and audio go first; annotations are removed after {} days", ["Najpierw znika wideo i dźwięk; adnotacje są usuwane po {} dniach", "先删除视频和音频；批注会在 {} 天后删除", "先に動画と音声が消え、注釈は {} 日後に削除されます", "Video und Audio gehen zuerst; Annotationen werden nach {} Tagen entfernt", "Primero se van el vídeo y el audio; las anotaciones se eliminan a los {} días", "Спершу зникають відео й аудіо; анотації видаляються через {} днів"]),
    #[cfg(hex_capture)]
    ("Video and audio go first; annotations stay until you remove them", ["Najpierw znika wideo i dźwięk; adnotacje zostają, dopóki ich nie usuniesz", "先删除视频和音频；批注会保留到你删除为止", "先に動画と音声が消えます。注釈は削除するまで残ります", "Video und Audio gehen zuerst; Annotationen bleiben, bis du sie entfernst", "Primero se van el vídeo y el audio; las anotaciones se quedan hasta que las elimines", "Спершу зникають відео й аудіо; анотації лишаються, доки ви їх не видалите"]),
    #[cfg(hex_capture)]
    ("Annotations stay until you remove them", ["Adnotacje zostają, dopóki ich nie usuniesz", "批注会保留到你删除为止", "注釈は削除するまで残ります", "Annotationen bleiben, bis du sie entfernst", "Las anotaciones se quedan hasta que las elimines", "Анотації лишаються, доки ви їх не видалите"]),
    #[cfg(hex_capture)]
    ("Hex leaves the Annotate shortcut to Scrawl until you switch", ["Hex zostawia skrót Adnotacji aplikacji Scrawl, dopóki nie przełączysz", "在你切换之前，Hex 会把 Annotate 快捷键留给 Scrawl", "切り替えるまで、Hex は Annotate のショートカットを Scrawl に任せます", "Hex überlässt Scrawl das Annotate-Kürzel, bis du wechselst", "Hex deja el atajo de Annotate a Scrawl hasta que cambies", "Hex залишає комбінацію Annotate для Scrawl, доки ви не перемкнетеся"]),
    #[cfg(hex_capture)]
    ("Waiting for Scrawl to quit…", ["Czekam na zamknięcie Scrawl…", "正在等待 Scrawl 退出…", "Scrawl の終了を待っています…", "Warten, bis Scrawl beendet ist…", "Esperando a que Scrawl se cierre…", "Очікування завершення Scrawl…"]),
    #[cfg(hex_capture)]
    ("Scrawl is running", ["Scrawl jest uruchomiony", "Scrawl 正在运行", "Scrawl が実行中です", "Scrawl läuft", "Scrawl está abierto", "Scrawl запущено"]),
    #[cfg(hex_capture)]
    ("Use Hex for annotations", ["Używaj Hex do adnotacji", "用 Hex 批注", "注釈に Hex を使う", "Hex für Annotationen verwenden", "Usar Hex para anotar", "Використовувати Hex для анотацій"]),
    #[cfg(hex_capture)]
    #[cfg(hex_capture)]
    #[cfg(hex_capture)]
    #[cfg(hex_capture)]
    ("Saved for when Annotate is on", ["Zapisany na czas, gdy Adnotacje są włączone", "Annotate 打开后生效", "Annotate がオンのときに使われます", "Gespeichert für, wenn Annotate an ist", "Guardado para cuando Annotate esté activado", "Збережено на час, коли Annotate увімкнено"]),
    ("Screen Recording", ["Nagrywanie ekranu", "屏幕录制", "画面収録", "Bildschirmaufnahme", "Grabación de pantalla", "Запис екрана"]),
    ("Lets Annotate record your screen", ["Pozwala Adnotacjom nagrywać ekran", "允许 Annotate 录制你的屏幕", "Annotate に画面の録画を許可します", "Erlaubt Annotate, deinen Bildschirm aufzunehmen", "Permite que Annotate grabe tu pantalla", "Дозволяє Annotate записувати ваш екран"]),
    #[cfg(hex_capture)]
    ("Required to record your screen. macOS asks the first time you annotate, or allow it here.", ["Wymagane do nagrywania ekranu. macOS zapyta przy pierwszej adnotacji albo zezwól tutaj.", "录制屏幕需要此权限。macOS 会在你第一次批注时询问，你也可以在这里允许。", "画面の録画に必要です。初めて注釈するときに macOS が尋ねます。ここで許可することもできます。", "Zum Aufnehmen des Bildschirms nötig. macOS fragt beim ersten Annotieren, oder erlaube es hier.", "Necesario para grabar tu pantalla. macOS lo pide la primera vez que anotas, o permítelo aquí.", "Потрібно для запису екрана. macOS запитає під час першої анотації, або дозвольте тут."]),
    #[cfg(hex_capture)]
    ("Show in screen recordings", ["Pokazuj w nagraniach ekranu", "在屏幕录制中显示", "画面収録に表示", "In Bildschirmaufnahmen zeigen", "Mostrar en grabaciones de pantalla", "Показувати в записах екрана"]),
    #[cfg(hex_capture)]
    ("Lets recordings, annotations included, show Hex's capsule, ink, and HUD, for demos", ["Pozwala nagraniom, także adnotacjom, pokazywać kapsułę, atrament i HUD Hex, do prezentacji", "让录屏（包括批注）显示 Hex 的胶囊、墨迹和 HUD，用于演示", "録画（注釈を含む）に Hex のカプセル、インク、HUD を映します。デモ用です", "Lässt Aufnahmen, auch Annotationen, Hex' Kapsel, Tinte und HUD zeigen, für Demos", "Permite que las grabaciones, anotaciones incluidas, muestren la cápsula, la tinta y el HUD de Hex, para demos", "Дозволяє записам, зокрема анотаціям, показувати капсулу, чорнило й HUD Hex, для демонстрацій"]),
    #[cfg(hex_capture)]
    ("Transcriber", ["Transkrypcja", "转写引擎", "文字起こし", "Transkription", "Transcriptor", "Транскрибатор"]),
    #[cfg(hex_capture)]
    ("Transcription", ["Transkrypcja", "转写", "文字起こし", "Transkription", "Transcripción", "Транскрипція"]),
    #[cfg(hex_capture)]
    ("Storage", ["Przechowywanie", "存储", "保存", "Speicher", "Almacenamiento", "Сховище"]),
    ("Annotations folder", ["Folder adnotacji", "批注文件夹", "注釈フォルダ", "Annotationsordner", "Carpeta de anotaciones", "Папка анотацій"]),
    #[cfg(hex_capture)]
    ("Shared with Scrawl, so both list the same annotations", ["Wspólny ze Scrawl, więc obie aplikacje widzą te same adnotacje", "与 Scrawl 共用，两者列出相同的批注", "Scrawl と共有するので、どちらにも同じ注釈が並びます", "Mit Scrawl geteilt, beide zeigen dieselben Annotationen", "Compartida con Scrawl, así ambos muestran las mismas anotaciones", "Спільна зі Scrawl, тож обидва показують ті самі анотації"]),
    #[cfg(hex_capture)]
    ("Keep", ["Przechowuj", "保留", "保持期間", "Behalten", "Conservar", "Зберігати"]),
    ("Add application", ["Dodaj aplikację", "添加应用", "アプリを追加", "App hinzufügen", "Añadir aplicación", "Додати програму"]),
    ("Add transformation", ["Dodaj transformację", "添加转换", "変換を追加", "Transformation hinzufügen", "Añadir transformación", "Додати перетворення"]),
    ("Close", ["Zamknij", "关闭", "閉じる", "Schließen", "Cerrar", "Закрити"]),
    ("All files", ["Wszystkie pliki", "所有文件", "すべてのファイル", "Alle Dateien", "Todos los archivos", "Усі файли"]),
    ("Reorder", ["Zmień kolejność", "重新排序", "並べ替え", "Neu anordnen", "Reordenar", "Змінити порядок"]),
    ("No models match", ["Brak pasujących modeli", "没有匹配的模型", "一致するモデルはありません", "Keine passenden Modelle", "Ningún modelo coincide", "Відповідних моделей немає"]),
    ("Status", ["Stan", "状态", "ステータス", "Status", "Estado", "Стан"]),
    ("Connecting…", ["Łączenie…", "正在连接…", "接続中…", "Verbindung wird hergestellt…", "Conectando…", "Підключення…"]),
    ("Esc to cancel", ["Esc, aby anulować", "按 Esc 取消", "Esc でキャンセル", "Esc zum Abbrechen", "Esc para cancelar", "Esc, щоб скасувати"]),
    ("Desktop shortcuts", ["Skróty pulpitu", "桌面快捷键", "デスクトップのショートカット", "Desktop-Tastenkürzel", "Atajos del escritorio", "Системні комбінації клавіш"]),
    ("Change the keys your desktop gave Hex", ["Zmień klawisze, które pulpit przydzielił Hex", "更改桌面分配给 Hex 的按键", "デスクトップが Hex に割り当てたキーを変更", "Tasten ändern, die der Desktop Hex zugewiesen hat", "Cambia las teclas que el escritorio asignó a Hex", "Змінити клавіші, які система призначила Hex"]),
    ("Granted by your desktop", ["Przyznany przez pulpit", "由桌面授予", "デスクトップが割り当て", "Vom Desktop vergeben", "Concedido por tu escritorio", "Надано системою"]),
    ("Shortcuts", ["Skróty", "快捷键", "ショートカット", "Tastenkürzel", "Atajos", "Комбінації клавіш"]),
    ("Your desktop granted {}", ["Pulpit przyznał {}", "桌面已授予 {}", "デスクトップが {} を割り当て", "Vom Desktop vergeben: {}", "Tu escritorio concedió {}", "Система надала {}"]),
    ("Your desktop did not grant this shortcut", ["Pulpit nie przyznał tego skrótu", "桌面未授予此快捷键", "デスクトップはこのショートカットを割り当てませんでした", "Der Desktop hat dieses Tastenkürzel nicht vergeben", "Tu escritorio no concedió este atajo", "Система не надала цю комбінацію клавіш"]),
    ("Your desktop kept a shortcut for itself. Record another one, or free it in your desktop's keyboard settings (on niri, remove its bind from config.kdl).", ["Pulpit zachował skrót dla siebie. Nagraj inny albo zwolnij go w ustawieniach klawiatury pulpitu (w niri usuń jego przypisanie z config.kdl).", "桌面保留了某个快捷键。请录制其他快捷键，或在桌面的键盘设置中释放它（在 niri 中，从 config.kdl 删除其绑定）。", "デスクトップがショートカットを使用中です。別のショートカットを記録するか、デスクトップのキーボード設定で空けてください（niri では config.kdl からその割り当てを削除）。", "Der Desktop behält ein Tastenkürzel für sich. Nimm ein anderes auf oder gib es in den Tastatureinstellungen des Desktops frei (unter niri die Belegung aus config.kdl entfernen).", "Tu escritorio se reservó un atajo. Graba otro o libéralo en los ajustes de teclado del escritorio (en niri, quita su asignación de config.kdl).", "Система залишила комбінацію клавіш за собою. Запишіть іншу або звільніть її в налаштуваннях клавіатури системи (у niri видаліть її призначення з config.kdl)."]),
    ("Not granted", ["Nie przyznano", "未授予", "未割り当て", "Nicht vergeben", "No concedido", "Не надано"]),
    ("Your desktop did not grant every shortcut. Record another one, or free it in your desktop's keyboard settings.", ["Pulpit nie przyznał wszystkich skrótów. Nagraj inny albo zwolnij go w ustawieniach klawiatury pulpitu.", "桌面未授予所有快捷键。请录制其他快捷键，或在桌面的键盘设置中释放它。", "デスクトップがすべてのショートカットを割り当てませんでした。別のショートカットを記録するか、デスクトップのキーボード設定で空けてください。", "Der Desktop hat nicht jedes Tastenkürzel vergeben. Nimm ein anderes auf oder gib es in den Tastatureinstellungen des Desktops frei.", "Tu escritorio no concedió todos los atajos. Graba otro o libéralo en los ajustes de teclado del escritorio.", "Система надала не всі комбінації клавіш. Призначте іншу або звільніть її в налаштуваннях клавіатури системи."]),
    ("Your desktop has no shortcuts portal. Bind these commands in its keyboard settings.", ["Pulpit nie ma portalu skrótów. Przypisz te polecenia w jego ustawieniach klawiatury.", "你的桌面没有快捷键门户。请在其键盘设置中绑定这些命令。", "デスクトップにショートカットポータルがありません。キーボード設定でこれらのコマンドを割り当ててください。", "Dein Desktop hat kein Tastenkürzel-Portal. Weise diese Befehle in seinen Tastatureinstellungen zu.", "Tu escritorio no tiene portal de atajos. Asigna estos comandos en sus ajustes de teclado.", "У вашій системі немає порталу комбінацій клавіш. Призначте ці команди в її налаштуваннях клавіатури."]),
    ("Hold to dictate", ["Przytrzymaj, aby dyktować", "按住听写", "押している間音声入力", "Halten zum Diktieren", "Mantén pulsado para dictar", "Утримуйте, щоб диктувати"]),
    ("Bind start to the key press and stop to its release", ["Przypisz start do naciśnięcia klawisza, a stop do jego zwolnienia", "将 start 绑定到按下按键，将 stop 绑定到松开按键", "start をキーを押したとき、stop を離したときに割り当て", "start an das Drücken und stop an das Loslassen der Taste binden", "Asigna start al pulsar la tecla y stop al soltarla", "Привʼяжіть start до натискання клавіші, а stop до її відпускання"]),
    ("Toggle dictation", ["Przełącz dyktowanie", "切换听写", "音声入力を切り替え", "Diktat umschalten", "Alternar dictado", "Перемкнути диктування"]),
    ("Cancel dictation", ["Anuluj dyktowanie", "取消听写", "音声入力をキャンセル", "Diktat abbrechen", "Cancelar dictado", "Скасувати диктування"]),
    ("Discard the current recording", ["Odrzuć bieżące nagranie", "丢弃当前录音", "現在の録音を破棄", "Aktuelle Aufnahme verwerfen", "Descartar la grabación actual", "Відкинути поточний запис"]),
    ("Ready anywhere · Use your desktop's shortcuts to dictate", ["Gotowe wszędzie · Dyktuj skrótami pulpitu", "随处可用 · 使用桌面快捷键听写", "どこでも利用可能 · デスクトップのショートカットで音声入力", "Überall bereit · Diktiere mit den Tastenkürzeln des Desktops", "Listo en cualquier lugar · Dicta con los atajos del escritorio", "Готово всюди · Диктуйте за допомогою системних комбінацій клавіш"]),
    ("Interface language", ["Język interfejsu", "界面语言", "表示言語", "Sprache der Oberfläche", "Idioma de la interfaz", "Мова інтерфейсу"]),
    ("Language of the Hex interface", ["Język interfejsu Hex", "Hex 界面语言", "Hex の表示言語", "Sprache der Hex-Oberfläche", "Idioma de la interfaz de Hex", "Мова інтерфейсу Hex"]),
    ("System", ["Systemowy", "跟随系统", "システム", "System", "Sistema", "Системна"]),
    ("Recognition hints", ["Podpowiedzi rozpoznawania", "识别提示", "認識ヒント", "Erkennungshinweise", "Sugerencias de reconocimiento", "Підказки для розпізнавання"]),
    ("{} doesn't use these terms; rewriting and search still do", ["{} nie korzysta z tych terminów; przepisywanie i wyszukiwanie nadal tak", "{} 不使用这些词语；改写和搜索仍会使用", "{} はこれらの用語を使いません。書き換えと検索では引き続き使われます", "{} nutzt diese Begriffe nicht; Umschreiben und Suche schon", "{} no usa estos términos; la reescritura y la búsqueda sí", "{} не використовує ці терміни; переписування й пошук їх використовують"]),
    ("Fastest", ["Najszybszy", "最快", "最速", "Am schnellsten", "Más rápido", "Найшвидша"]),
    ("Most accurate", ["Najdokładniejszy", "最准确", "最高精度", "Am genauesten", "Más preciso", "Найточніша"]),
    ("Use", ["Użyj", "使用", "使用", "Verwenden", "Usar", "Вибрати"]),
    ("Download", ["Pobierz", "下载", "ダウンロード", "Herunterladen", "Descargar", "Завантажити"]),
    ("Cancel", ["Anuluj", "取消", "キャンセル", "Abbrechen", "Cancelar", "Скасувати"]),
    ("Active", ["Aktywny", "使用中", "有効", "Aktiv", "Activo", "Активна"]),
    ("Auto", ["Auto", "自动", "自動", "Auto", "Auto", "Авто"]),
    ("Downloading", ["Pobieranie", "下载中", "ダウンロード中", "Lädt herunter", "Descargando", "Завантаження"]),
    ("Verifying model", ["Weryfikacja modelu", "正在校验模型", "モデルを検証中", "Modell wird geprüft", "Verificando modelo", "Перевірка моделі"]),
    ("Loading model", ["Wczytywanie modelu", "正在加载模型", "モデルを読み込み中", "Modell wird geladen", "Cargando modelo", "Завантаження моделі в памʼять"]),
    ("Model could not be installed.", ["Nie udało się zainstalować modelu.", "无法安装模型。", "モデルをインストールできませんでした。", "Modell konnte nicht installiert werden.", "No se pudo instalar el modelo.", "Не вдалося встановити модель."]),
    ("Installed", ["Zainstalowany", "已安装", "インストール済み", "Installiert", "Instalado", "Встановлено"]),
    ("Recommended", ["Zalecany", "推荐", "おすすめ", "Empfohlen", "Recomendado", "Рекомендована"]),
    ("Streams", ["Strumieniowy", "流式", "ストリーミング", "Streamt", "En streaming", "Потокова"]),
    ("Transcribes while you speak on a GPU", ["Transkrybuje w trakcie mówienia na GPU", "使用 GPU 时边说边转写", "GPU で話しながら文字起こし", "Transkribiert auf einer GPU, während Sie sprechen", "Transcribe mientras hablas en una GPU", "Розпізнає мовлення, поки ви говорите, використовуючи GPU"]),
    ("Settings", ["Ustawienia", "设置", "設定", "Einstellungen", "Ajustes", "Налаштування"]),
    ("General", ["Ogólne", "通用", "一般", "Allgemein", "General", "Загальні"]),
    ("Experiments", ["Eksperymenty", "实验", "実験", "Experimente", "Experimentos", "Експерименти"]),
    ("Voice Filter", ["Filtr głosu", "声音滤镜", "ボイスフィルター", "Stimmfilter", "Filtro de voz", "Голосовий фільтр"]),
    ("Recordings", ["Nagrania", "录音", "録音", "Aufnahmen", "Grabaciones", "Записи"]),
    ("Advanced", ["Zaawansowane", "高级", "詳細", "Erweitert", "Avanzado", "Додатково"]),
    ("Output", ["Wyjście", "输出", "出力", "Ausgabe", "Salida", "Виведення"]),
    ("Dictation HUD", ["Wskaźnik dyktowania", "听写指示器", "音声入力インジケーター", "Diktatanzeige", "Indicador de dictado", "HUD диктування"]),
    ("Call recording", ["Nagrywanie rozmów", "通话录音", "通話の録音", "Anrufaufzeichnung", "Grabación de llamadas", "Запис дзвінків"]),
    ("Check for updates", ["Sprawdź aktualizacje", "检查更新", "アップデートを確認", "Nach Updates suchen", "Buscar actualizaciones", "Перевірити оновлення"]),
    ("Checking for updates…", ["Sprawdzanie aktualizacji…", "正在检查更新…", "アップデートを確認中…", "Suche nach Updates…", "Buscando actualizaciones…", "Перевірка оновлень…"]),
    ("Start listening when Hex opens", ["Zacznij nasłuchiwać po otwarciu Hex", "Hex 打开时开始监听", "Hex の起動時に待ち受けを開始", "Beim Öffnen von Hex zuhören", "Empezar a escuchar al abrir Hex", "Починати слухати, коли Hex відкривається"]),
    ("Say a phrase to open apps and websites or press keys", ["Powiedz frazę, aby otworzyć aplikacje i strony lub nacisnąć klawisze", "说出短语即可打开应用和网站或按下按键", "フレーズを話してアプリやウェブサイトを開いたり、キーを押したりします", "Sagen Sie eine Phrase, um Apps und Websites zu öffnen oder Tasten zu drücken", "Di una frase para abrir apps y sitios web o pulsar teclas", "Скажіть фразу, щоб відкрити програми й вебсайти або натиснути клавіші"]),
    ("Keep the microphone ready?", ["Utrzymywać mikrofon w gotowości?", "保持麦克风就绪？", "マイクを待機状態にしますか？", "Mikrofon bereithalten?", "¿Mantener el micrófono listo?", "Тримати мікрофон напоготові?"]),
    ("Commands listen all the time, so Hex stops releasing the microphone when idle", ["Polecenia nasłuchują cały czas, więc Hex przestanie zwalniać mikrofon w bezczynności", "命令会一直监听，因此 Hex 将不再在空闲时释放麦克风", "コマンドは常に聞き取るため、Hex はアイドル時にマイクを解放しなくなります", "Befehle hören ständig zu, daher gibt Hex das Mikrofon im Leerlauf nicht mehr frei", "Los comandos escuchan todo el tiempo, así que Hex deja de liberar el micrófono en reposo", "Команди слухають постійно, тож Hex більше не звільняє мікрофон під час простою"]),
    ("Modes", ["Tryby", "模式", "モード", "Modi", "Modos", "Режими"]),
    ("History", ["Historia", "历史记录", "履歴", "Verlauf", "Historial", "Історія"]),
    ("Add mode", ["Dodaj tryb", "添加模式", "モードを追加", "Modus hinzufügen", "Añadir modo", "Додати режим"]),
    ("Add correction", ["Dodaj poprawkę", "添加更正", "修正を追加", "Korrektur hinzufügen", "Añadir corrección", "Додати виправлення"]),
    ("Vocabulary", ["Słownictwo", "词汇表", "語彙", "Vokabular", "Vocabulario", "Словник"]),
    ("Names and terms", ["Nazwy i terminy", "名称和术语", "名前と用語", "Namen und Begriffe", "Nombres y términos", "Імена та терміни"]),
    ("Transcription and rewriting spell them right, and search finds them when misheard", ["Transkrypcja i przepisywanie zapisują je poprawnie, a wyszukiwanie znajduje je, gdy zostaną źle usłyszane", "转写和改写时拼写正确，听错时搜索也能找到", "文字起こしと書き直しで正しく綴られ、聞き間違えられても検索で見つかります", "Transkription und Umschreiben schreiben sie richtig, und die Suche findet sie auch falsch verstanden", "La transcripción y la reescritura los escriben bien y la búsqueda los encuentra aunque se oigan mal", "Транскрипція й переписування пишуть їх правильно, а пошук знаходить їх, навіть коли їх почуто неправильно"]),
    ("Terms with spaces match only when said in full", ["Terminy ze spacjami pasują tylko wypowiedziane w całości", "带空格的术语只有完整说出时才会匹配", "スペースを含む用語は、全体を言ったときだけ一致します", "Begriffe mit Leerzeichen greifen nur, wenn sie ganz gesagt werden", "Los términos con espacios solo coinciden si se dicen completos", "Терміни з пробілами збігаються, лише коли їх сказано повністю"]),
    ("Light", ["Lekkie", "轻", "弱", "Leicht", "Ligero", "Слабке"]),
    ("Strong", ["Mocne", "强", "強", "Stark", "Fuerte", "Сильне"]),
    ("Add term", ["Dodaj termin", "添加术语", "用語を追加", "Begriff hinzufügen", "Añadir término", "Додати термін"]),
    ("1 term", ["1 termin", "1 个术语", "1 件の用語", "1 Begriff", "1 término", "1 термін"]),
    ("{} terms", ["Terminy: {}", "{} 个术语", "{} 件の用語", "{} Begriffe", "{} términos", "Термінів: {}"]),
    ("Term (e.g. abattoir)", ["Termin (np. abattoir)", "术语（如 abattoir）", "用語（例: abattoir）", "Begriff (z. B. abattoir)", "Término (p. ej., abattoir)", "Термін (напр., abattoir)"]),
    ("Sounds like (e.g. avatar, a bat war)", ["Brzmi jak (np. avatar, a bat war)", "听起来像（如 avatar, a bat war）", "聞こえ方（例: avatar, a bat war）", "Klingt wie (z. B. avatar, a bat war)", "Suena como (p. ej., avatar, a bat war)", "Звучить як (напр., avatar, a bat war)"]),
    ("Up to 16 spellings per term are saved, and 64 characters per term or spelling.", ["Zapisywanych jest do 16 pisowni na termin i do 64 znaków na termin lub pisownię.", "每个术语最多保存 16 种拼写，每个术语或拼写最多 64 个字符。", "保存されるのは用語ごとに最大 16 個の表記、用語や表記ごとに最大 64 文字です。", "Gespeichert werden bis zu 16 Schreibweisen pro Begriff und 64 Zeichen pro Begriff oder Schreibweise.", "Se guardan hasta 16 grafías por término y 64 caracteres por término o grafía.", "Зберігається до 16 написань для кожного терміна й до 64 символів у терміні чи написанні."]),
    ("Clear all", ["Wyczyść wszystko", "全部清除", "すべて消去", "Alle löschen", "Borrar todo", "Очистити все"]),
    ("Really clear all?", ["Na pewno wyczyścić?", "确定全部清除？", "本当にすべて消去？", "Wirklich alle löschen?", "¿Borrar todo?", "Справді очистити все?"]),
    ("Copy", ["Kopiuj", "复制", "コピー", "Kopieren", "Copiar", "Копіювати"]),
    ("(nothing)", ["(nic)", "（无）", "（なし）", "(nichts)", "(nada)", "(нічого)"]),
    ("Replace words", ["Zamień słowa", "替换词语", "単語を置き換え", "Wörter ersetzen", "Reemplazar palabras", "Замінити слова"]),
    ("Rewrite with OpenCode", ["Przepisz z OpenCode", "用 OpenCode 改写", "OpenCode で書き直す", "Mit OpenCode umschreiben", "Reescribir con OpenCode", "Переписати з OpenCode"]),
    ("Lowercase", ["Małe litery", "小写", "小文字", "Kleinbuchstaben", "Minúsculas", "Малі літери"]),
    ("Lowercases everything", ["Zmienia wszystko na małe litery", "全部转为小写", "すべて小文字にします", "Schreibt alles klein", "Pone todo en minúsculas", "Робить усе малими літерами"]),
    ("No final period", ["Bez kropki na końcu", "去掉末尾句号", "最後の句点なし", "Kein Schlusspunkt", "Sin punto final", "Без крапки в кінці"]),
    ("Drops the last . ! or ?", ["Usuwa ostatnią . ! lub ?", "去掉最后的 . ! 或 ?", "最後の . ! ? を削除します", "Entfernt das letzte . ! oder ?", "Quita el último . ! o ?", "Прибирає останню . ! чи ?"]),
    ("SpongeBob case", ["Styl SpongeBoba", "海绵宝宝大小写", "スポンジ・ボブ表記", "SpongeBob-Schreibweise", "Estilo Bob Esponja", "Стиль Губки Боба"]),
    ("aLtErNaTeS every letter", ["nAp RzEmIeNnIe litery", "字母大小写交替", "文字を交互に大小にします", "wEcHsElT jeden Buchstaben", "aLtErNa cada letra", "чЕрГуЄ кожну літеру"]),
    ("Copied. Paste it into your agent and say what you want", ["Skopiowano. Wklej to swojemu agentowi i powiedz, czego chcesz", "已复制。粘贴给你的智能体，说出你想要的", "コピーしました。エージェントに貼り付けて、望むことを伝えてください", "Kopiert. Füge es bei deinem Agenten ein und sag, was du willst", "Copiado. Pégalo en tu agente y di lo que quieres", "Скопійовано. Вставте це своєму агентові й скажіть, чого хочете"]),
    ("Tell it what you want, like “no final period in Slack”", ["Powiedz mu, czego chcesz, np. „bez kropki na końcu w Slacku”", "告诉它你想要什么，比如“在 Slack 里不要末尾句号”", "「Slack では最後の句点なし」のように、望むことを伝えてください", "Sag ihm, was du willst, etwa „kein Schlusspunkt in Slack“", "Dile lo que quieres, como «sin punto final en Slack»", "Скажіть йому, чого хочете, наприклад «без крапки в кінці в Slack»"]),
    ("Your agent writes these steps", ["Te kroki pisze twój agent", "这些步骤由你的智能体编写", "これらのステップはエージェントが書きます", "Diese Schritte schreibt dein Agent", "Tu agente escribe estos pasos", "Ці кроки пише ваш агент"]),
    ("Open file", ["Otwórz plik", "打开文件", "ファイルを開く", "Datei öffnen", "Abrir archivo", "Відкрити файл"]),
    ("Edit with agent", ["Edytuj z agentem", "用智能体编辑", "エージェントで編集", "Mit Agent bearbeiten", "Editar con agente", "Редагувати з агентом"]),
    ("No steps yet. Hex pastes your words as it heard them", ["Brak kroków. Hex wkleja twoje słowa tak, jak je usłyszał", "还没有步骤。Hex 会按听到的内容粘贴", "まだステップはありません。Hex は聞き取ったとおりに貼り付けます", "Noch keine Schritte. Hex fügt deine Worte so ein, wie es sie gehört hat", "Aún no hay pasos. Hex pega tus palabras tal como las oyó", "Кроків ще немає. Hex вставляє ваші слова так, як їх почув"]),
    ("Every dictation", ["Każde dyktowanie", "每次听写", "すべての音声入力", "Jedes Diktat", "Cada dictado", "Кожне диктування"]),
    ("Runs top to bottom", ["Od góry do dołu", "从上到下运行", "上から順に実行", "Läuft von oben nach unten", "Se ejecuta de arriba abajo", "Виконується згори донизу"]),
    ("Built in", ["Wbudowany", "内置", "組み込み", "Eingebaut", "Integrado", "Вбудований"]),
    ("Type what you might say", ["Wpisz, co możesz powiedzieć", "输入你可能会说的话", "話しそうなことを入力", "Tippe, was du sagen würdest", "Escribe lo que dirías", "Введіть, що ви могли б сказати"]),
    ("What Hex pastes", ["Co wklei Hex", "Hex 粘贴的内容", "Hex が貼り付ける内容", "Was Hex einfügt", "Lo que pega Hex", "Що вставить Hex"]),
    ("Try", ["Wypróbuj", "试试", "試す", "Testen", "Probar", "Спробувати"]),
    ("In some apps", ["W niektórych aplikacjach", "在某些应用中", "一部のアプリで", "In manchen Apps", "En algunas apps", "У деяких застосунках"]),
    ("Used there instead", ["Tam zamiast tego", "在那里替代使用", "そこでは代わりに使用", "Dort stattdessen", "Se usan en su lugar", "Там замість цього"]),
    ("Pastes your words as Hex heard them", ["Wkleja słowa tak, jak Hex je usłyszał", "按 Hex 听到的内容粘贴", "Hex が聞き取ったとおりに貼り付けます", "Fügt deine Worte so ein, wie Hex sie gehört hat", "Pega tus palabras tal como Hex las oyó", "Вставляє слова так, як Hex їх почув"]),
    ("Only: {}", ["Tylko: {}", "仅：{}", "のみ：{}", "Nur: {}", "Solo: {}", "Лише: {}"]),
    ("Copied", ["Skopiowano", "已复制", "コピーしました", "Kopiert", "Copiado", "Скопійовано"]),
    ("Copy new", ["Kopiuj nowe", "复制新内容", "新しい分をコピー", "Neues kopieren", "Copiar lo nuevo", "Копіювати нове"]),
    ("Copy new ({})", ["Kopiuj nowe ({})", "复制新内容（{}）", "新しい分をコピー（{}）", "Neues kopieren ({})", "Copiar lo nuevo ({})", "Копіювати нове ({})"]),
    ("Copy all", ["Kopiuj wszystko", "全部复制", "すべてコピー", "Alles kopieren", "Copiar todo", "Копіювати все"]),
    ("Copy section", ["Kopiuj sekcję", "复制本节", "このセクションをコピー", "Abschnitt kopieren", "Copiar sección", "Копіювати розділ"]),
    ("Chapters", ["Rozdziały", "章节", "チャプター", "Kapitel", "Capítulos", "Розділи"]),
    ("Delete", ["Usuń", "删除", "削除", "Löschen", "Eliminar", "Видалити"]),
    ("Remove", ["Usuń", "移除", "削除", "Entfernen", "Quitar", "Вилучити"]),
    ("Dictation", ["Dyktowanie", "听写", "音声入力", "Diktat", "Dictado", "Диктування"]),
    ("Send", ["Wysłano", "发送", "送信", "Senden", "Enviar", "Надсилання"]),
    ("Printed", ["Wypisano", "已输出", "出力", "Ausgegeben", "Impreso", "Виведено"]),
    ("Voice Action", ["Voice Action", "Voice Action", "Voice Action", "Voice Action", "Voice Action", "Voice Action"]),
    ("Application", ["Aplikacja", "应用", "アプリ", "Anwendung", "Aplicación", "Програма"]),
    ("Global", ["Globalny", "全局", "グローバル", "Global", "Global", "Глобальний"]),
    ("Fallback for everything", ["Tryb zapasowy dla wszystkiego", "其他所有情况的后备模式", "その他すべてのフォールバック", "Fallback für alles", "Alternativa para todo", "Резервний режим для всього"]),
    ("No activation rules", ["Brak reguł aktywacji", "无激活规则", "有効化ルールなし", "Keine Aktivierungsregeln", "Sin reglas de activación", "Немає правил активації"]),
    ("Untitled mode", ["Tryb bez nazwy", "未命名模式", "名称未設定のモード", "Unbenannter Modus", "Modo sin título", "Режим без назви"]),
    ("Used unless a more specific mode matches.", ["Używany, jeśli nie pasuje bardziej szczegółowy tryb.", "在没有更具体的模式匹配时使用。", "より具体的なモードが一致しない場合に使用します。", "Wird verwendet, wenn kein spezifischerer Modus passt.", "Se usa si no coincide un modo más específico.", "Використовується, якщо не відповідає жоден конкретніший режим."]),
    ("Name", ["Nazwa", "名称", "名前", "Name", "Nombre", "Назва"]),
    ("Applications", ["Aplikacje", "应用", "アプリケーション", "Anwendungen", "Aplicaciones", "Програми"]),
    ("Switch when an app is frontmost", ["Przełącz, gdy aplikacja jest na pierwszym planie", "当前台应用匹配时切换", "アプリが最前面のときに切り替え", "Wechseln, wenn eine App im Vordergrund ist", "Cambiar cuando una aplicación esté en primer plano", "Перемикати, коли програма на передньому плані"]),
    ("Done", ["Gotowe", "完成", "完了", "Fertig", "Listo", "Готово"]),
    ("Corrections", ["Korekty", "更正", "修正", "Korrekturen", "Correcciones", "Виправлення"]),
    ("Local transcription", ["Lokalna transkrypcja", "本地转写", "ローカル文字起こし", "Lokale Transkription", "Transcripción local", "Локальне розпізнавання мовлення"]),
    ("Language and on-device speech model", ["Język i model mowy na urządzeniu", "语言与本地语音模型", "言語とオンデバイス音声モデル", "Sprache und lokales Sprachmodell", "Idioma y modelo de voz local", "Мова й модель розпізнавання мовлення на пристрої"]),
    ("Microphone", ["Mikrofon", "麦克风", "マイク", "Mikrofon", "Micrófono", "Мікрофон"]),
    ("Dictation shortcut", ["Skrót dyktowania", "听写快捷键", "音声入力ショートカット", "Diktat-Tastenkürzel", "Atajo de dictado", "Комбінація клавіш для диктування"]),
    ("Double-tap to lock", ["Podwójne naciśnięcie blokuje", "双击锁定", "ダブルタップでロック", "Doppeltippen zum Sperren", "Doble pulsación para bloquear", "Двічі натисніть, щоб зафіксувати"]),
    ("Double-tap only", ["Tylko podwójne naciśnięcie", "仅限双击", "ダブルタップのみ", "Nur Doppeltippen", "Solo doble pulsación", "Лише подвійне натискання"]),
    ("Wait for two complete shortcut taps before recording", ["Nagrywanie zaczyna się dopiero po dwóch pełnych naciśnięciach skrótu", "需要连按两次快捷键才开始录音", "ショートカットを2回押し切ってから録音を開始", "Aufnahme erst nach zwei vollständigen Kürzel-Tipps", "La grabación espera dos pulsaciones completas del atajo", "Починати запис лише після двох повних натискань комбінації клавіш"]),
    ("Paste last dictation", ["Wklej ostatnie dyktowanie", "粘贴上次听写", "前回の音声入力を貼り付け", "Letztes Diktat einfügen", "Pegar último dictado", "Вставити останнє диктування"]),
    ("Capture", ["Nagrywanie", "采集", "キャプチャ", "Aufnahme", "Captura", "Запис"]),
    ("Processing", ["Przetwarzanie", "处理", "処理", "Verarbeitung", "Procesamiento", "Обробка"]),
    ("Shortcut", ["Skrót", "快捷键", "ショートカット", "Kürzel", "Atajo", "Комбінація клавіш"]),
    ("OpenCode", ["OpenCode", "OpenCode", "OpenCode", "OpenCode", "OpenCode", "OpenCode"]),
    ("Default", ["Domyślny", "默认", "既定", "Standard", "Predeterminado", "Типово"]),
    ("Last dictation failed: {}", ["Ostatnie dyktowanie nie powiodło się: {}", "上次听写失败：{}", "前回の音声入力に失敗しました: {}", "Letztes Diktat fehlgeschlagen: {}", "El último dictado falló: {}", "Не вдалося виконати останнє диктування: {}"]),
    ("Version {}", ["Wersja {}", "版本 {}", "バージョン {}", "Version {}", "Versión {}", "Версія {}"]),
    ("Welcome to Hex", ["Witaj w Hex", "欢迎使用 Hex", "Hex へようこそ", "Willkommen bei Hex", "Bienvenido a Hex", "Вітаємо в Hex"]),
    ("Hold", ["Przytrzymaj", "按住", "押しながら", "Halte", "Mantén", "Утримувати"]),
    ("Activity", ["Aktywność", "活动", "アクティビティ", "Aktivität", "Actividad", "Активність"]),
    ("Voice commands", ["Polecenia głosowe", "语音命令", "音声コマンド", "Sprachbefehle", "Comandos de voz", "Голосові команди"]),
    ("While dictating", ["Podczas dyktowania", "听写时", "音声入力中", "Während des Diktats", "Mientras dictas", "Під час диктування"]),
    ("Mute", ["Wycisz", "静音", "ミュート", "Stummschalten", "Silenciar", "Тиша"]),
    ("Pause media", ["Wstrzymaj media", "暂停媒体", "メディアを一時停止", "Medien pausieren", "Pausar medios", "Пауза медіа"]),
    ("Lower audio", ["Ścisz dźwięk", "降低音量", "音量を下げる", "Audio absenken", "Bajar audio", "Приглушити звук"]),
    ("Lower to", ["Ścisz do", "降低至", "音量の割合", "Absenken auf", "Bajar al", "Приглушити до"]),
    ("Percent of the current output volume", ["Procent bieżącej głośności wyjściowej", "当前输出音量的百分比", "現在の出力音量に対する割合", "Prozent der aktuellen Ausgabelautstärke", "Porcentaje del volumen de salida actual", "Відсоток поточної гучності виходу"]),
    ("Volume while recording", ["Głośność podczas nagrywania", "录音时的音量", "録音中の音量", "Lautstärke während der Aufnahme", "Volumen durante la grabación", "Гучність під час запису"]),
    ("Also pause the browser", ["Wstrzymuj też przeglądarkę", "同时暂停浏览器", "ブラウザーも一時停止", "Auch den Browser pausieren", "Pausar también el navegador", "Також призупиняти браузер"]),
    ("YouTube and other tabs, resumed only if Hex paused them", ["YouTube i inne karty, wznawiane tylko wtedy, gdy wstrzymał je Hex", "YouTube 和其他标签页，仅在由 Hex 暂停时才会恢复", "YouTube などのタブ。Hex が一時停止した場合のみ再開します", "YouTube und andere Tabs, nur fortgesetzt, wenn Hex sie pausiert hat", "YouTube y otras pestañas; solo se reanudan si Hex las pausó", "YouTube та інші вкладки; відновлюються, лише якщо їх призупинив Hex"]),
    ("Lets Hex hear whether a tab is playing", ["Pozwala Hex usłyszeć, czy karta odtwarza dźwięk", "让 Hex 能听出标签页是否在播放", "タブが再生中かどうかを Hex が聞き取れるようにします", "Damit Hex hört, ob ein Tab abspielt", "Permite a Hex oír si una pestaña está reproduciendo", "Дозволяє Hex чути, чи відтворює вкладка звук"]),
    ("Do nothing", ["Nic nie rób", "不处理", "何もしない", "Nichts tun", "No hacer nada", "Нічого"]),
    ("Recording indicator", ["Wskaźnik nagrywania", "录音指示器", "録音インジケーター", "Aufnahmeanzeige", "Indicador de grabación", "Індикатор запису"]),
    ("Show the dictation pill at the top or bottom of the screen", ["Pokazuj pastylkę dyktowania u góry lub u dołu ekranu", "在屏幕顶部或底部显示听写胶囊", "画面の上または下に音声入力ピルを表示", "Diktat-Pille oben oder unten am Bildschirm anzeigen", "Muestra la píldora de dictado arriba o abajo de la pantalla", "Показувати панель диктування у верхній або нижній частині екрана"]),
    ("Checks for new Hex releases in the background", ["Sprawdza nowe wydania Hex w tle", "在后台检查 Hex 的新版本", "バックグラウンドで Hex の新しいリリースを確認", "Prüft im Hintergrund auf neue Hex-Versionen", "Busca nuevas versiones de Hex en segundo plano", "Перевіряти у фоновому режимі, чи доступні нові версії Hex"]),
    ("Launch at login", ["Uruchamiaj przy logowaniu", "登录时启动", "ログイン時に起動", "Beim Anmelden starten", "Iniciar al iniciar sesión", "Запускати під час входу"]),
    ("Top", ["Góra", "顶部", "上", "Oben", "Arriba", "Угорі"]),
    ("Bottom", ["Dół", "底部", "下", "Unten", "Abajo", "Унизу"]),
    ("Off", ["Wył.", "关", "オフ", "Aus", "No", "Вимк."]),
    ("Disabled", ["Wyłączone", "已禁用", "無効", "Deaktiviert", "Desactivado", "Вимкнено"]),
    ("System default", ["Domyślny systemowy", "系统默认", "システムのデフォルト", "Systemstandard", "Predeterminado del sistema", "Системний типовий"]),
    ("Hex Microphone is the system default, so Hex listens to {}", ["Hex Microphone jest domyślny w systemie, więc Hex słucha przez {}", "Hex Microphone 是系统默认设备，因此 Hex 使用 {}", "Hex Microphone がシステムのデフォルトのため、Hex は {} を使います", "Hex Microphone ist der Systemstandard, daher nutzt Hex {}", "Hex Microphone es el predeterminado del sistema, así que Hex usa {}", "Hex Microphone — системний типовий мікрофон, тож Hex слухає {}"]),
    ("not connected", ["niepodłączony", "未连接", "未接続", "nicht verbunden", "no conectado", "не під'єднано"]),
    ("{built_in} is off while the lid is closed, so Hex listens to {input}", ["{built_in} jest wyłączony przy zamkniętej pokrywie, więc Hex słucha przez {input}", "合盖时 {built_in} 处于关闭状态，因此 Hex 使用 {input}", "蓋を閉じている間は {built_in} がオフのため、Hex は {input} を使います", "{built_in} ist bei geschlossenem Deckel aus, daher nutzt Hex {input}", "{built_in} está apagado con la tapa cerrada, así que Hex usa {input}", "{built_in} вимкнено, поки кришку закрито, тож Hex слухає {input}"]),
    ("{} is off while the lid is closed", ["{} jest wyłączony przy zamkniętej pokrywie", "合盖时 {} 处于关闭状态", "蓋を閉じている間は {} がオフです", "{} ist bei geschlossenem Deckel aus", "{} está apagado con la tapa cerrada", "{} вимкнено, поки кришку закрито"]),
    ("off while the lid is closed", ["wyłączony przy zamkniętej pokrywie", "合盖时关闭", "蓋を閉じている間はオフ", "bei geschlossenem Deckel aus", "apagado con la tapa cerrada", "вимкнено, поки кришку закрито"]),
    ("{} isn't connected; using your Mac's input", ["{} nie jest podłączony; używany jest mikrofon Maca", "{} 未连接；正在使用 Mac 的输入设备", "{} は接続されていません。Mac の入力を使用中", "{} ist nicht verbunden; der Eingang des Mac wird verwendet", "{} no está conectado; se usa la entrada del Mac", "{} не під'єднано; використовується вхід вашого Mac"]),
    ("{} isn't connected; using the system input", ["{} nie jest podłączony; używane jest wejście systemowe", "{} 未连接；正在使用系统输入设备", "{} は接続されていません。システムの入力を使用中", "{} ist nicht verbunden; der Systemeingang wird verwendet", "{} no está conectado; se usa la entrada del sistema", "{} не під'єднано; використовується системний вхід"]),
    ("24 hours", ["24 godziny", "24 小时", "24 時間", "24 Stunden", "24 horas", "24 години"]),
    ("7 days", ["7 dni", "7 天", "7 日間", "7 Tage", "7 días", "7 днів"]),
    ("30 days", ["30 dni", "30 天", "30 日間", "30 Tage", "30 días", "30 днів"]),
    ("Forever", ["Bezterminowo", "永久", "無期限", "Unbegrenzt", "Para siempre", "Завжди"]),
    ("No corrections in this mode.", ["Brak korekt w tym trybie.", "此模式中没有更正规则。", "このモードには修正がありません。", "Keine Korrekturen in diesem Modus.", "No hay correcciones en este modo.", "У цьому режимі немає виправлень."]),
    ("No transformations in this mode.", ["Brak transformacji w tym trybie.", "此模式中没有转换。", "このモードには変換がありません。", "Keine Transformationen in diesem Modus.", "No hay transformaciones en este modo.", "У цьому режимі немає перетворень."]),
    ("History is off.", ["Historia jest wyłączona.", "历史记录已关闭。", "履歴はオフです。", "Verlauf ist aus.", "El historial está desactivado.", "Історію вимкнено."]),
    ("Choose how long to keep dictations to see them here.", ["Wybierz, jak długo przechowywać dyktowania, aby widzieć je tutaj.", "选择听写的保留时长，即可在此处查看。", "音声入力を保存する期間を選ぶと、ここに表示されます。", "Wähle, wie lange Diktate behalten werden, um sie hier zu sehen.", "Elige cuánto tiempo guardar los dictados para verlos aquí.", "Виберіть, як довго зберігати диктування, щоб бачити їх тут."]),
    ("Everything you dictate is kept here.", ["Wszystko, co podyktujesz, jest przechowywane tutaj.", "你听写的所有内容都会保存在这里。", "音声入力した内容はすべてここに保存されます。", "Alles, was du diktierst, wird hier aufbewahrt.", "Todo lo que dictes se guarda aquí.", "Усе, що ви диктуєте, зберігається тут."]),
    ("Everything you dictate is kept here for {}.", ["Wszystko, co podyktujesz, jest przechowywane tutaj przez {}.", "你听写的所有内容都会在这里保存 {}。", "音声入力した内容はここに {} 保存されます。", "Alles, was du diktierst, wird hier {} lang aufbewahrt.", "Todo lo que dictes se guarda aquí durante {}.", "Усе, що ви диктуєте, зберігається тут {}."]),
    ("No dictations retained yet.", ["Brak zapisanych dyktowań.", "尚无保存的听写。", "保存された音声入力はまだありません。", "Noch keine Diktate gespeichert.", "Aún no hay dictados guardados.", "Збережених диктувань ще немає."]),
    ("No dictations match.", ["Brak pasujących dyktowań.", "没有匹配的听写。", "一致する音声入力はありません。", "Keine passenden Diktate.", "No hay dictados que coincidan.", "Відповідних диктувань немає."]),
    ("Select a history entry.", ["Wybierz wpis z historii.", "选择一条历史记录。", "履歴の項目を選択してください。", "Wählen Sie einen Verlaufseintrag.", "Selecciona una entrada del historial.", "Виберіть запис з історії."]),
    ("History could not be loaded.", ["Nie udało się wczytać historii.", "无法加载历史记录。", "履歴を読み込めませんでした。", "Verlauf konnte nicht geladen werden.", "No se pudo cargar el historial.", "Не вдалося завантажити історію."]),
    ("Hex could not apply this change.", ["Hex nie mógł zastosować tej zmiany.", "Hex 无法应用此更改。", "Hex はこの変更を適用できませんでした。", "Hex konnte diese Änderung nicht übernehmen.", "Hex no pudo aplicar este cambio.", "Hex не вдалося застосувати цю зміну."]),
    ("Raw transcript", ["Surowa transkrypcja", "原始转写", "生の文字起こし", "Rohtranskript", "Transcripción sin procesar", "Початковий транскрипт"]),
    ("Ready", ["Gotowy", "就绪", "準備完了", "Bereit", "Listo", "Готово"]),
    ("Listening", ["Nasłuchuje", "正在监听", "リッスン中", "Hört zu", "Escuchando", "Прослуховування"]),
    ("Model required", ["Wymagany model", "需要模型", "モデルが必要", "Modell erforderlich", "Se requiere modelo", "Потрібна модель"]),
    ("Unavailable", ["Niedostępny", "不可用", "利用不可", "Nicht verfügbar", "No disponible", "Недоступно"]),
    ("Recording", ["Nagrywanie", "录音中", "録音中", "Aufnahme", "Grabando", "Запис"]),
    ("Transcribing", ["Transkrypcja", "转写中", "文字起こし中", "Transkribiert", "Transcribiendo", "Розпізнавання"]),
    ("Add", ["Dodaj", "添加", "追加", "Hinzufügen", "Añadir", "Додати"]),
    ("Add Application", ["Dodaj aplikację", "添加应用", "アプリを追加", "App hinzufügen", "Añadir aplicación", "Додати програму"]),
    ("Add a key", ["Dodaj klawisz", "添加一个按键", "キーを追加してください", "Taste hinzufügen", "Añade una tecla", "Додайте клавішу"]),
    ("Add a key Linux can register", ["Dodaj klawisz, który Linux może zarejestrować", "添加一个 Linux 可以注册的按键", "Linux で登録できるキーを追加してください", "Taste hinzufügen, die Linux registrieren kann", "Añade una tecla que Linux pueda registrar", "Додайте клавішу, яку Linux може зареєструвати"]),
    ("Let Hex hear you", ["Pozwól Hex cię słyszeć", "让 Hex 听到你", "Hex にマイクを使わせてください", "Lass Hex dich hören", "Deja que Hex te escuche", "Дозвольте Hex чути вас"]),
    ("Windows has the microphone turned off for desktop apps.", ["Windows wyłączył mikrofon dla aplikacji klasycznych.", "Windows 已关闭桌面应用的麦克风访问权限。", "Windows でデスクトップ アプリのマイクがオフになっています。", "Windows hat das Mikrofon für Desktop-Apps ausgeschaltet.", "Windows tiene el micrófono desactivado para las aplicaciones de escritorio.", "Windows вимкнув мікрофон для класичних програм."]),
    ("Turn on “Let desktop apps access your microphone” in Windows Settings.", ["Włącz „Zezwalaj aplikacjom klasycznym na dostęp do mikrofonu” w Ustawieniach systemu Windows.", "在 Windows 设置中打开“允许桌面应用访问你的麦克风”。", "Windows の設定で「デスクトップ アプリがマイクにアクセスできるようにする」をオンにしてください。", "Aktiviere „Desktop-Apps den Zugriff auf Ihr Mikrofon erlauben“ in den Windows-Einstellungen.", "Activa «Permitir que las aplicaciones de escritorio accedan al micrófono» en Configuración de Windows.", "Увімкніть «Дозволити класичним програмам доступ до мікрофона» в параметрах Windows."]),
    ("Add a modifier", ["Dodaj modyfikator", "添加一个修饰键", "修飾キーを追加してください", "Sondertaste hinzufügen", "Añade un modificador", "Додайте клавішу-модифікатор"]),
    ("Already in use", ["Już w użyciu", "已被使用", "すでに使用されています", "Bereits belegt", "Ya está en uso", "Уже використовується"]),
    ("Behavior", ["Zachowanie", "行为", "動作", "Verhalten", "Comportamiento", "Поведінка"]),
    ("Change shortcut", ["Zmień skrót", "更改快捷键", "ショートカットを変更", "Kürzel ändern", "Cambiar atajo", "Змінити"]),
    ("Check again", ["Sprawdź ponownie", "再次检查", "再確認", "Erneut prüfen", "Comprobar de nuevo", "Перевірити ще раз"]),
    ("Check failed; retrying later", ["Sprawdzanie nie powiodło się; ponowimy później", "检查失败；稍后重试", "確認に失敗しました。後で再試行します", "Prüfung fehlgeschlagen; später erneut", "La comprobación falló; se reintentará más tarde", "Не вдалося перевірити; повторна спроба пізніше"]),
    ("Checking for OpenCode", ["Szukanie OpenCode", "正在查找 OpenCode", "OpenCode を確認中", "Suche nach OpenCode", "Buscando OpenCode", "Перевірка OpenCode"]),
    ("Checking…", ["Sprawdzanie…", "正在检查…", "確認中…", "Wird geprüft…", "Comprobando…", "Перевірка…"]),
    ("Choose how much reasoning the model should use", ["Wybierz, ile rozumowania ma użyć model", "选择模型应使用多少推理", "モデルが使う推論量を選択", "Lege fest, wie viel der Modell nachdenken soll", "Elige cuánto razonamiento debe usar el modelo", "Виберіть рівень міркування моделі"]),
    ("Clear shortcut", ["Wyczyść skrót", "清除快捷键", "ショートカットをクリア", "Kürzel löschen", "Borrar atajo", "Очистити сполучення клавіш"]),
    ("Click to record a new shortcut", ["Kliknij, aby nagrać nowy skrót", "点击以录制新的快捷键", "クリックして新しいショートカットを記録", "Klicken, um ein neues Kürzel aufzunehmen", "Haz clic para grabar un atajo nuevo", "Натисніть, щоб записати нову комбінацію клавіш"]),
    ("Confirm delete", ["Potwierdź usunięcie", "确认删除", "削除を確定", "Löschen bestätigen", "Confirmar eliminación", "Підтвердити видалення"]),
    ("Create a config file to define reusable transformations.", ["Utwórz plik konfiguracji, aby zdefiniować transformacje wielokrotnego użytku.", "创建一个配置文件来定义可复用的转换。", "設定ファイルを作成して、再利用できる変換を定義します。", "Lege eine Konfigurationsdatei an, um wiederverwendbare Transformationen zu definieren.", "Crea un archivo de configuración para definir transformaciones reutilizables.", "Створіть файл конфігурації, щоб визначити повторно використовувані перетворення."]),
    ("Custom transformations", ["Własne transformacje", "自定义转换", "カスタム変換", "Eigene Transformationen", "Transformaciones personalizadas", "Власні перетворення"]),
    ("Custom transformations are unavailable.", ["Własne transformacje są niedostępne.", "自定义转换不可用。", "カスタム変換を利用できません。", "Eigene Transformationen sind nicht verfügbar.", "Las transformaciones personalizadas no están disponibles.", "Власні перетворення недоступні."]),
    ("Deadline", ["Limit czasu", "期限", "期限", "Zeitlimit", "Plazo", "Ліміт часу"]),
    ("Delete mode", ["Usuń tryb", "删除模式", "モードを削除", "Modus löschen", "Eliminar modo", "Видалити режим"]),
    ("Describe how OpenCode should rewrite your dictation…", ["Opisz, jak OpenCode ma przepisać twoje dyktowanie…", "描述 OpenCode 应如何改写你的听写内容…", "OpenCode が音声入力をどう書き直すか説明してください…", "Beschreibe, wie OpenCode dein Diktat umschreiben soll…", "Describe cómo debe reescribir OpenCode tu dictado…", "Опишіть, як OpenCode має переписувати продиктований текст…"]),
    ("Disable", ["Wyłącz", "停用", "無効にする", "Deaktivieren", "Desactivar", "Вимкнути"]),
    ("Either", ["Dowolny", "任意", "どちらでも", "Beide", "Cualquiera", "Обидві"]),
    ("Enable", ["Włącz", "启用", "有効にする", "Aktivieren", "Activar", "Увімкнути"]),
    ("Enable on launch", ["Włączaj przy uruchomieniu", "启动时启用", "起動時に有効にする", "Beim Start aktivieren", "Activar al iniciar", "Вмикати під час запуску"]),
    ("Every registered transformation is already included.", ["Wszystkie zarejestrowane transformacje są już dodane.", "所有已注册的转换都已包含。", "登録済みの変換はすべて追加されています。", "Alle registrierten Transformationen sind bereits enthalten.", "Todas las transformaciones registradas ya están incluidas.", "Усі зареєстровані перетворення вже додано."]),
    ("Finding applications…", ["Wyszukiwanie aplikacji…", "正在查找应用…", "アプリを検索中…", "Apps werden gesucht…", "Buscando aplicaciones…", "Пошук програм…"]),
    ("Hex could not load the OpenCode model catalog.", ["Hex nie mógł wczytać katalogu modeli OpenCode.", "Hex 无法加载 OpenCode 模型目录。", "Hex は OpenCode のモデル一覧を読み込めませんでした。", "Hex konnte den OpenCode-Modellkatalog nicht laden.", "Hex no pudo cargar el catálogo de modelos de OpenCode.", "Hex не вдалося завантажити каталог моделей OpenCode."]),
    ("Input device for dictation and commands", ["Urządzenie wejściowe do dyktowania i poleceń", "用于听写和命令的输入设备", "音声入力とコマンドに使う入力デバイス", "Eingabegerät für Diktat und Befehle", "Dispositivo de entrada para dictado y comandos", "Пристрій введення для диктування й команд"]),
    ("Install and configure OpenCode to transform dictated text.", ["Zainstaluj i skonfiguruj OpenCode, aby przekształcać dyktowany tekst.", "安装并配置 OpenCode 以转换听写文本。", "OpenCode をインストールして設定すると、音声入力したテキストを変換できます。", "Installiere und konfiguriere OpenCode, um diktierten Text umzuwandeln.", "Instala y configura OpenCode para transformar el texto dictado.", "Встановіть і налаштуйте OpenCode, щоб перетворювати продиктований текст."]),
    ("Instructions", ["Instrukcje", "说明", "指示", "Anweisungen", "Instrucciones", "Інструкції"]),
    ("Keep Hex in the menu bar when Settings is closed", ["Zostaw Hex na pasku menu po zamknięciu Ustawień", "关闭设置后将 Hex 保留在菜单栏", "設定を閉じても Hex をメニューバーに表示", "Hex in der Menüleiste behalten, wenn die Einstellungen geschlossen sind", "Mantener Hex en la barra de menús al cerrar Ajustes", "Залишати Hex на смузі меню, коли вікно налаштувань закрите"]),
    ("Keep ready (fast)", ["Gotowy (szybko)", "保持就绪（快）", "常に待機（高速）", "Bereit halten (schnell)", "Listo (rápido)", "Готовий (швидко)"]),
    ("Keeps the microphone open for instant start and pre-roll", ["Utrzymuje otwarty mikrofon dla natychmiastowego startu i wstępnego nagrywania", "保持麦克风打开，以便立即开始并预录音", "マイクを開いたままにし、すぐに開始してプリロールを使います", "Hält das Mikrofon offen für sofortigen Start und Vorlauf", "Mantiene el micrófono abierto para empezar al instante con pregrabación", "Мікрофон залишається ввімкненим для миттєвого початку й попереднього захоплення аудіо"]),
    ("Latency", ["Opóźnienie", "延迟", "レイテンシー", "Latenz", "Latencia", "Затримка"]),
    ("Left", ["Lewy", "左", "左", "Links", "Izquierda", "Ліва"]),
    ("Loading OpenCode…", ["Wczytywanie OpenCode…", "正在加载 OpenCode…", "OpenCode を読み込み中…", "OpenCode wird geladen…", "Cargando OpenCode…", "Завантаження OpenCode…"]),
    ("Loading custom transformations…", ["Wczytywanie własnych transformacji…", "正在加载自定义转换…", "カスタム変換を読み込み中…", "Eigene Transformationen werden geladen…", "Cargando las transformaciones personalizadas…", "Завантаження власних перетворень…"]),
    ("Meetings", ["Spotkania", "会议", "会議", "Besprechungen", "Reuniones", "Зустрічі"]),
    ("Microphone mode", ["Tryb mikrofonu", "麦克风模式", "マイクモード", "Mikrofonmodus", "Modo del micrófono", "Режим мікрофона"]),
    ("Mode name", ["Nazwa trybu", "模式名称", "モード名", "Modusname", "Nombre del modo", "Назва режиму"]),
    ("Mute or pause other audio during dictation", ["Wycisz lub wstrzymaj inne dźwięki podczas dyktowania", "听写时静音或暂停其他音频", "音声入力中はほかの音声をミュートまたは一時停止", "Andere Audiowiedergabe während des Diktierens stummschalten oder pausieren", "Silencia o pausa otros audios durante el dictado", "Вимикати звук або призупиняти інше аудіо під час диктування"]),
    ("Names and terms Whisper should expect, e.g. OpenCode, Effect…", ["Nazwy i terminy, których Whisper ma się spodziewać, np. OpenCode, Effect…", "Whisper 应识别的名称和术语，例如 OpenCode、Effect…", "Whisper が想定すべき名前や用語（例: OpenCode、Effect…）", "Namen und Begriffe, die Whisper erwarten soll, z. B. OpenCode, Effect…", "Nombres y términos que Whisper debe esperar, p. ej. OpenCode, Effect…", "Імена й терміни, які Whisper має очікувати, наприклад, OpenCode, Effect…"]),
    ("Names and terms to prime the speech model", ["Nazwy i terminy podpowiadane modelowi mowy", "用于提示语音模型的名称和术语", "音声モデルに与える名前や用語", "Namen und Begriffe als Hinweis für das Sprachmodell", "Nombres y términos para orientar al modelo de voz", "Імена й терміни для підказок моделі розпізнавання мовлення"]),
    ("No applications found.", ["Nie znaleziono aplikacji.", "未找到应用。", "アプリが見つかりません。", "Keine Apps gefunden.", "No se encontraron aplicaciones.", "Програм не знайдено."]),
    ("Open OpenCode setup", ["Otwórz konfigurację OpenCode", "打开 OpenCode 设置", "OpenCode のセットアップを開く", "OpenCode-Einrichtung öffnen", "Abrir configuración de OpenCode", "Відкрити налаштування OpenCode"]),
    ("Open settings", ["Otwórz ustawienia", "打开设置", "設定を開く", "Einstellungen öffnen", "Abrir ajustes", "Відкрити налаштування"]),
    ("Calendar", ["Kalendarz", "日历", "カレンダー", "Kalender", "Calendario", "Календар"]),
    ("Calendars", ["Kalendarze", "日历", "カレンダー", "Kalender", "Calendarios", "Календарі"]),
    ("Calendar access is off", ["Dostęp do kalendarza jest wyłączony", "日历访问已关闭", "カレンダーへのアクセスがオフです", "Kalenderzugriff ist aus", "El acceso al calendario está desactivado", "Доступ до календаря вимкнено"]),
    ("Allow Hex under Privacy & Security › Calendars to get meeting reminders", ["Zezwól Hex w Prywatność i ochrona › Kalendarze, aby dostawać przypomnienia o spotkaniach", "在“隐私与安全性 › 日历”中允许 Hex 以获取会议提醒", "会議のリマインダーを受け取るには、「プライバシーとセキュリティ › カレンダー」で Hex を許可してください", "Erlaube Hex unter „Datenschutz & Sicherheit › Kalender“, um an Meetings erinnert zu werden", "Permite Hex en Privacidad y seguridad › Calendarios para recibir recordatorios de reuniones", "Дозвольте Hex у розділі Приватність і безпека › Календарі, щоб отримувати нагадування про зустрічі"]),
    ("Names each recording after its calendar event", ["Nadaje nagraniom nazwy wydarzeń z kalendarza", "用日历事件为录音命名", "録音にカレンダーの予定名を付けます", "Benennt Aufnahmen nach dem Kalendertermin", "Nombra cada grabación según su evento del calendario", "Називає кожен запис за подією в календарі"]),
    ("Connect calendar", ["Połącz kalendarz", "连接日历", "カレンダーを接続", "Kalender verbinden", "Conectar calendario", "Під'єднати календар"]),
    ("Remind me before meetings", ["Przypominaj przed spotkaniami", "会议前提醒我", "会議の前に知らせる", "Vor Meetings erinnern", "Recordarme antes de las reuniones", "Нагадувати перед зустрічами"]),
    ("Shows a reminder with Record and Join buttons", ["Pokazuje przypomnienie z przyciskami Nagraj i Dołącz", "显示带有录制和加入按钮的提醒", "録音と参加のボタン付きでリマインダーを表示します", "Zeigt eine Erinnerung mit den Tasten Aufnehmen und Beitreten", "Muestra un recordatorio con los botones Grabar y Unirse", "Показує нагадування з кнопками «Записати» і «Приєднатися»"]),
    ("Remind me", ["Przypomnij", "提醒时间", "通知のタイミング", "Erinnern", "Recordarme", "Нагадати"]),
    ("At start", ["Na początku", "开始时", "開始時", "Zu Beginn", "Al empezar", "На початку"]),
    ("1 min", ["1 min", "1 分钟", "1 分", "1 Min.", "1 min", "1 хв"]),
    ("2 min", ["2 min", "2 分钟", "2 分", "2 Min.", "2 min", "2 хв"]),
    ("5 min", ["5 min", "5 分钟", "5 分", "5 Min.", "5 min", "5 хв"]),
    ("No meetings in the next two days", ["Brak spotkań w ciągu najbliższych dwóch dni", "未来两天没有会议", "今後 2 日間に会議はありません", "Keine Meetings in den nächsten zwei Tagen", "No hay reuniones en los próximos dos días", "Немає зустрічей у найближчі два дні"]),
    ("All calendars", ["Wszystkie kalendarze", "所有日历", "すべてのカレンダー", "Alle Kalender", "Todos los calendarios", "Усі календарі"]),
    ("{included} of {total} calendars", ["{included} z {total} kalendarzy", "{total} 个日历中的 {included} 个", "{total} 件中 {included} 件のカレンダー", "{included} von {total} Kalendern", "{included} de {total} calendarios", "{included} з {total} календарів"]),
    ("Tomorrow", ["Jutro", "明天", "明日", "Morgen", "Mañana", "Завтра"]),
    ("Starts in {} min", ["Zaczyna się za {} min", "{} 分钟后开始", "{} 分後に開始", "Beginnt in {} Min.", "Empieza en {} min", "Почнеться за {} хв"]),
    ("Starting now", ["Zaczyna się teraz", "即将开始", "まもなく開始", "Beginnt jetzt", "Empieza ahora", "Починається зараз"]),
    ("Started {} min ago", ["Zaczęło się {} min temu", "已开始 {} 分钟", "{} 分前に開始", "Vor {} Min. begonnen", "Empezó hace {} min", "Почалася {} хв тому"]),
    ("{} · Hex records automatically", ["{} · Hex nagra automatycznie", "{} · Hex 会自动录制", "{} · Hex が自動で録音します", "{} · Hex nimmt automatisch auf", "{} · Hex grabará automáticamente", "{} · Hex записує автоматично"]),
    ("Don't record", ["Nie nagrywaj", "不录制", "録音しない", "Nicht aufnehmen", "No grabar", "Не записувати"]),
    ("Join", ["Dołącz", "加入", "参加", "Beitreten", "Unirse", "Приєднатися"]),
    ("Join & record", ["Dołącz i nagraj", "加入并录制", "参加して録音", "Beitreten & aufnehmen", "Unirse y grabar", "Приєднатися й записати"]),
    ("OpenCode beta required", ["Wymagana beta OpenCode", "需要 OpenCode 测试版", "OpenCode ベータ版が必要です", "OpenCode-Beta erforderlich", "Se requiere OpenCode beta", "Потрібна бета-версія OpenCode"]),
    ("OpenCode is unavailable", ["OpenCode jest niedostępny", "OpenCode 不可用", "OpenCode を利用できません", "OpenCode ist nicht verfügbar", "OpenCode no está disponible", "OpenCode недоступний"]),
    ("OpenCode model", ["Model OpenCode", "OpenCode 模型", "OpenCode モデル", "OpenCode-Modell", "Modelo de OpenCode", "Модель OpenCode"]),
    ("OpenCode transformation", ["Transformacja OpenCode", "OpenCode 转换", "OpenCode 変換", "OpenCode-Transformation", "Transformación de OpenCode", "Перетворення за допомогою OpenCode"]),
    ("OpenCode transformation will be available after Hex connects to OpenCode.", ["Transformacja OpenCode będzie dostępna, gdy Hex połączy się z OpenCode.", "Hex 连接到 OpenCode 后即可使用 OpenCode 转换。", "Hex が OpenCode に接続すると OpenCode 変換を使えます。", "Die OpenCode-Transformation ist verfügbar, sobald Hex mit OpenCode verbunden ist.", "La transformación de OpenCode estará disponible cuando Hex se conecte a OpenCode.", "Перетворення за допомогою OpenCode стане доступним після підключення Hex до OpenCode."]),
    ("Opens only while holding the shortcut. Disables pre-roll.", ["Otwiera się tylko podczas przytrzymania skrótu. Wyłącza wstępne nagrywanie.", "仅在按住快捷键时打开。会停用预录音。", "ショートカットを押している間だけ開きます。プリロールは無効になります。", "Öffnet sich nur, solange das Kürzel gehalten wird. Deaktiviert Vorlauf.", "Solo se abre mientras mantienes el atajo. Desactiva la pregrabación.", "Вмикає мікрофон лише під час утримання комбінації клавіш. Вимикає попереднє захоплення аудіо."]),
    ("Opens the microphone on shortcut press without pre-roll.", ["Otwiera mikrofon po naciśnięciu skrótu, bez wstępnego nagrywania.", "按下快捷键时打开麦克风，不预录音。", "ショートカットを押したときにプリロールなしでマイクを開きます。", "Öffnet das Mikrofon beim Drücken des Kürzels ohne Vorlauf.", "Abre el micrófono al pulsar el atajo, sin pregrabación.", "Вмикає мікрофон після натискання комбінації клавіш без попереднього захоплення аудіо."]),
    ("Share dictation shortcut with other apps", ["Udostępniaj skrót dyktowania innym aplikacjom", "将听写快捷键共享给其他应用", "ディクテーションのショートカットを他のアプリと共有", "Diktierkürzel für andere Apps freigeben", "Compartir el atajo de dictado con otras aplicaciones", "Ділитися сполученням клавіш диктування з іншими програмами"]),
    ("Use another microphone instead of Bluetooth", ["Używaj innego mikrofonu zamiast Bluetooth", "使用其他麦克风而不是蓝牙", "Bluetooth の代わりに別のマイクを使う", "Anderes Mikrofon statt Bluetooth verwenden", "Usar otro micrófono en lugar de Bluetooth", "Використовувати інший мікрофон замість Bluetooth"]),
    ("Use the Mac's microphone instead of Bluetooth", ["Używaj mikrofonu Maca zamiast Bluetooth", "使用 Mac 的麦克风而不是蓝牙", "Bluetooth の代わりに Mac のマイクを使う", "Mikrofon des Mac statt Bluetooth verwenden", "Usar el micrófono del Mac en lugar de Bluetooth", "Використовувати мікрофон Mac замість Bluetooth"]),
    ("Keeps AirPods and other headsets out of call-quality audio", ["Chroni AirPods i inne słuchawki przed jakością dźwięku rozmowy", "避免 AirPods 和其他耳机切换到通话音质", "AirPods などのヘッドセットが通話用の音質にならないようにします", "Hält AirPods und andere Headsets aus der Anrufqualität heraus", "Evita que los AirPods y otros auriculares pasen a calidad de llamada", "Не дає AirPods та іншим гарнітурам перейти на якість дзвінка"]),
    ("{bluetooth} is Bluetooth, so Hex listens to {input}", ["{bluetooth} to Bluetooth, więc Hex słucha przez {input}", "{bluetooth} 是蓝牙设备，因此 Hex 使用 {input}", "{bluetooth} は Bluetooth のため、Hex は {input} を使います", "{bluetooth} ist Bluetooth, daher nutzt Hex {input}", "{bluetooth} es Bluetooth, así que Hex usa {input}", "{bluetooth} — це Bluetooth, тож Hex слухає {input}"]),
    ("Lets apps such as Discord detect the same key", ["Umożliwia aplikacjom takim jak Discord wykrywanie tego samego klawisza", "允许 Discord 等应用检测同一个按键", "Discord などのアプリが同じキーを検出できるようにします", "Ermöglicht Apps wie Discord, dieselbe Taste zu erkennen", "Permite que aplicaciones como Discord detecten la misma tecla", "Дозволяє програмам на кшталт Discord бачити ту саму клавішу"]),
    ("Paste with Ctrl+Shift+V so terminals accept it", ["Wklejaj przez Ctrl+Shift+V, aby terminale to przyjmowały", "使用 Ctrl+Shift+V 粘贴，以便终端接受", "端末で受け付けられるよう Ctrl+Shift+V で貼り付け", "Mit Strg+Umschalt+V einfügen, damit Terminals es annehmen", "Pegar con Ctrl+Mayús+V para que los terminales lo acepten", "Вставляти за допомогою Ctrl+Shift+V, щоб термінали приймали текст"]),
    ("Permissions needed", ["Wymagane uprawnienia", "需要权限", "権限が必要です", "Berechtigungen erforderlich", "Se necesitan permisos", "Потрібні дозволи"]),
    ("Preparing voice commands", ["Przygotowywanie poleceń głosowych", "正在准备语音命令", "音声コマンドを準備中", "Sprachbefehle werden vorbereitet", "Preparando los comandos de voz", "Підготовка голосових команд"]),
    ("Press a shortcut", ["Naciśnij skrót", "按下快捷键", "ショートカットを押してください", "Drücke ein Kürzel", "Pulsa un atajo", "Натисніть комбінацію клавіш"]),
    ("Press shortcut", ["Naciśnij skrót", "按下快捷键", "ショートカットを押す", "Kürzel drücken", "Pulsa el atajo", "Натисніть комбінацію клавіш"]),
    ("Press to start dictating, press again to transcribe", ["Naciśnij, aby zacząć dyktować, i ponownie, aby transkrybować", "按下开始听写，再按一次进行转写", "押すと音声入力を開始し、もう一度押すと文字起こし", "Drücken zum Diktieren, erneut drücken zum Transkribieren", "Pulsa para empezar a dictar y otra vez para transcribir", "Натисніть, щоб почати диктувати, і натисніть знову, щоб розпізнати мовлення"]),
    ("Re-paste the most recent dictation", ["Wklej ponownie ostatnie dyktowanie", "再次粘贴最近一次听写", "最新の音声入力をもう一度貼り付け", "Letztes Diktat erneut einfügen", "Volver a pegar el dictado más reciente", "Повторно вставити останнє диктування"]),
    ("Recording, cancellation, and error tones", ["Dźwięki nagrywania, anulowania i błędów", "录音、取消和错误提示音", "録音・キャンセル・エラーの効果音", "Töne für Aufnahme, Abbruch und Fehler", "Tonos de grabación, cancelación y error", "Звуки запису, скасування та помилок"]),
    ("Refresh", ["Odśwież", "刷新", "更新", "Aktualisieren", "Actualizar", "Оновити"]),
    ("Release microphone", ["Zwolnij mikrofon", "释放麦克风", "マイクを解放", "Mikrofon freigeben", "Liberar micrófono", "Звільнити мікрофон"]),
    ("Release the microphone when idle?", ["Zwalniać mikrofon w bezczynności?", "空闲时释放麦克风？", "待機中はマイクを解放しますか？", "Mikrofon im Leerlauf freigeben?", "¿Liberar el micrófono en reposo?", "Звільняти мікрофон, коли він не використовується?"]),
    ("Release when idle", ["Zwalniaj w bezczynności", "空闲时释放", "待機中は解放", "Im Leerlauf freigeben", "Liberar en reposo", "Звільняти в простої"]),
    ("Replacement (e.g. OpenCode)", ["Zamiennik (np. OpenCode)", "替换为（例如 OpenCode）", "置換後（例: OpenCode）", "Ersetzung (z. B. OpenCode)", "Reemplazo (p. ej. OpenCode)", "Заміна (наприклад, OpenCode)"]),
    ("Switch to {channel} {version}", ["Przełącz na {channel} {version}", "切换到{channel} {version}", "{channel} {version} に切り替え", "Zu {channel} {version} wechseln", "Cambiar a {channel} {version}", "Перейти на {channel} {version}"]),
    ("Restart to update", ["Uruchom ponownie, aby zaktualizować", "重启以更新", "再起動してアップデート", "Zum Aktualisieren neu starten", "Reiniciar para actualizar", "Перезапустити для оновлення"]),
    ("Retry", ["Ponów", "重试", "再試行", "Erneut versuchen", "Reintentar", "Повторити спробу"]),
    ("Retry setup", ["Ponów konfigurację", "重试设置", "セットアップを再試行", "Einrichtung wiederholen", "Reintentar configuración", "Повторити налаштування"]),
    ("Right", ["Prawy", "右", "右", "Rechts", "Derecha", "Права"]),
    ("Saved", ["Zapisano", "已保存", "保存しました", "Gespeichert", "Guardado", "Збережено"]),
    ("Saving…", ["Zapisywanie…", "正在保存…", "保存中…", "Wird gespeichert…", "Guardando…", "Збереження…"]),
    ("Search applications", ["Szukaj aplikacji", "搜索应用", "アプリを検索", "Apps suchen", "Buscar aplicaciones", "Пошук програм"]),
    ("Search available models", ["Szukaj dostępnych modeli", "搜索可用模型", "利用可能なモデルを検索", "Verfügbare Modelle suchen", "Buscar modelos disponibles", "Пошук доступних моделей"]),
    ("Search history", ["Szukaj w historii", "搜索历史记录", "履歴を検索", "Verlauf durchsuchen", "Buscar en el historial", "Пошук в історії"]),
    ("Seconds", ["Sekundy", "秒", "秒", "Sekunden", "Segundos", "Секунди"]),
    ("Set up", ["Skonfiguruj", "设置", "セットアップ", "Einrichten", "Configurar", "Налаштувати"]),
    ("Shortcut mode", ["Tryb skrótu", "快捷键模式", "ショートカットモード", "Kürzelmodus", "Modo del atajo", "Режим комбінації клавіш"]),
    ("Show Dock icon", ["Pokaż ikonę w Docku", "显示程序坞图标", "Dock にアイコンを表示", "Dock-Symbol anzeigen", "Mostrar icono en el Dock", "Показувати іконку в Dock"]),
    ("Update channel", ["Kanał aktualizacji", "更新渠道", "アップデートチャンネル", "Update-Kanal", "Canal de actualizaciones", "Канал оновлень"]),
    ("Stable", ["Stabilny", "稳定版", "安定版", "Stabil", "Estable", "Стабільний"]),
    ("Dev", ["Deweloperski", "开发版", "開発版", "Entwicklung", "Desarrollo", "Dev"]),
    ("Sound volume", ["Głośność dźwięków", "提示音音量", "効果音の音量", "Lautstärke der Töne", "Volumen de sonidos", "Гучність звуку"]),
    ("Sounds", ["Dźwięki", "提示音", "効果音", "Töne", "Sonidos", "Звуки"]),
    ("Hex signature", ["Sygnatura Hex", "Hex 标志音", "Hex シグネチャー", "Hex-Signatur", "Firma de Hex", "Фірмовий Hex"]),
    ("Classic", ["Klasyczne", "经典", "クラシック", "Klassisch", "Clásico", "Класичний"]),
    ("Click", ["Klik", "咔嗒", "クリック", "Klick", "Clic", "Клацання"]),
    ("Soft tone", ["Łagodny ton", "柔和音", "ソフトトーン", "Sanfter Ton", "Tono suave", "М’який тон"]),
    ("Wood", ["Drewno", "木质", "ウッド", "Holz", "Madera", "Дерево"]),
    ("Thock", ["Stuk", "键盘闷响", "コトコト", "Thock", "Thock", "Глухий стук"]),
    ("Warm synth", ["Ciepły syntezator", "温暖合成器", "ウォームシンセ", "Warmer Synth", "Sintetizador cálido", "Теплий синтезатор"]),
    ("A kalimba motif that rises and comes home", ["Motyw kalimby, który wznosi się i wraca do domu", "上行后回到主音的卡林巴旋律", "上がって主音に戻るカリンバのモチーフ", "Ein Kalimba-Motiv, das aufsteigt und heimkehrt", "Un motivo de kalimba que sube y vuelve a casa", "Мотив калімби, що здіймається й повертається додому"]),
    ("The tones Hex first shipped with", ["Dźwięki, z którymi Hex zadebiutował", "Hex 最初自带的提示音", "Hex が最初に搭載していた音", "Die Töne, mit denen Hex zuerst erschien", "Los tonos con los que Hex se lanzó", "Звуки, з якими Hex вийшов уперше"]),
    ("Mechanical key clicks", ["Kliknięcia mechanicznych klawiszy", "机械键盘的咔嗒声", "メカニカルキーのクリック音", "Mechanische Tastenklicks", "Clics de teclas mecánicas", "Клацання механічних клавіш"]),
    ("Warm, rounded blips", ["Ciepłe, zaokrąglone piknięcia", "温暖圆润的短音", "温かく丸みのあるブリップ音", "Warme, runde Blips", "Pitidos cálidos y redondos", "Теплі, округлі сигнали"]),
    ("Soft marimba and hollow wood blocks", ["Miękka marimba i puste drewniane klocki", "柔和的马林巴与空心木鱼", "柔らかなマリンバと中空のウッドブロック", "Sanfte Marimba und hohle Holzblöcke", "Marimba suave y bloques de madera huecos", "М’яка марімба й порожнисті дерев’яні бруски"]),
    ("A deep, creamy keyboard key", ["Głęboki, kremowy klawisz", "低沉圆润的键盘按键", "深くまろやかなキーボードのキー", "Eine tiefe, cremige Tastaturtaste", "Una tecla de teclado grave y cremosa", "Глибока, оксамитова клавіша"]),
    ("A mellow analogue synth", ["Łagodny syntezator analogowy", "柔和的模拟合成器", "まろやかなアナログシンセ", "Ein sanfter Analog-Synth", "Un sintetizador analógico suave", "Лагідний аналоговий синтезатор"]),
    ("Start", ["Start", "开始", "開始", "Start", "Inicio", "Початок"]),
    ("Error", ["Błąd", "错误", "エラー", "Fehler", "Error", "Помилка"]),
    ("Play", ["Odtwórz", "播放", "再生", "Abspielen", "Reproducir", "Відтворити"]),
    ("Sound volume is off", ["Głośność dźwięków jest wyłączona", "提示音音量已关闭", "サウンドの音量はオフです", "Die Tonlautstärke ist aus", "El volumen de los sonidos está desactivado", "Гучність звуків вимкнено"]),
    ("Spoken phrase (e.g. open code)", ["Wypowiadana fraza (np. open code)", "说出的短语（例如 open code）", "話すフレーズ（例: open code）", "Gesprochene Phrase (z. B. open code)", "Frase hablada (p. ej. open code)", "Продиктована фраза (наприклад, open code)"]),
    ("Start Hex when you log in", ["Uruchamiaj Hex po zalogowaniu", "登录时启动 Hex", "ログイン時に Hex を起動", "Hex beim Anmelden starten", "Iniciar Hex al iniciar sesión", "Запускати Hex під час входу в систему"]),
    ("System instructions for rewriting dictation in this mode", ["Instrukcje systemowe do przepisywania dyktowania w tym trybie", "此模式下改写听写内容的系统说明", "このモードで音声入力を書き直すためのシステム指示", "Systemanweisungen zum Umschreiben von Diktaten in diesem Modus", "Instrucciones del sistema para reescribir el dictado en este modo", "Системні інструкції для переписування диктування в цьому режимі"]),
    ("Terminal paste", ["Wklejanie w terminalu", "终端粘贴", "端末への貼り付け", "Einfügen im Terminal", "Pegado en terminal", "Вставлення в термінал"]),
    ("The config could not be created.", ["Nie udało się utworzyć konfiguracji.", "无法创建配置。", "設定を作成できませんでした。", "Die Konfiguration konnte nicht erstellt werden.", "No se pudo crear la configuración.", "Не вдалося створити конфігурацію."]),
    ("Thinking", ["Rozumowanie", "思考", "思考", "Denken", "Razonamiento", "Міркування"]),
    ("Toggle", ["Przełącz", "切换", "トグル", "Umschalten", "Alternar", "Перемикати"]),
    ("How it works", ["Jak to działa", "工作原理", "仕組み", "So funktioniert es", "Cómo funciona", "Як це працює"]),
    ("How the shortcut works", ["Jak działa skrót", "快捷键如何工作", "ショートカットの仕組み", "So funktioniert das Kürzel", "Cómo funciona el atajo", "Як працює сполучення клавіш"]),
    ("Hold to talk. Let go to paste.", ["Przytrzymaj, aby mówić. Puść, aby wkleić.", "按住说话，松开粘贴。", "押したまま話し、離すと貼り付けます。", "Halten zum Sprechen. Loslassen zum Einfügen.", "Mantén para hablar. Suelta para pegar.", "Утримуйте, щоб говорити. Відпустіть, щоб вставити."]),
    ("Press to start, press to stop.", ["Naciśnij, aby zacząć, naciśnij, aby zakończyć.", "按一下开始，再按一下停止。", "押して開始、もう一度押して停止。", "Drücken zum Starten, erneut drücken zum Stoppen.", "Pulsa para empezar, pulsa para parar.", "Натисніть, щоб почати, натисніть, щоб зупинити."]),
    ("Tap twice to talk hands-free. Press once to stop.", ["Stuknij dwa razy, aby mówić bez trzymania. Naciśnij raz, aby zakończyć.", "连按两次即可免按住说话，按一次停止。", "2回タップすると押さずに話せます。1回押すと停止します。", "Zweimal tippen, um freihändig zu sprechen. Einmal drücken zum Stoppen.", "Toca dos veces para hablar sin mantener. Pulsa una vez para parar.", "Натисніть двічі, щоб говорити без рук. Натисніть раз, щоб зупинити."]),
    ("Transformation requires OpenCode", ["Transformacja wymaga OpenCode", "转换需要 OpenCode", "変換には OpenCode が必要です", "Transformation erfordert OpenCode", "La transformación requiere OpenCode", "Для перетворення потрібен OpenCode"]),
    ("Transformations", ["Transformacje", "转换", "変換", "Transformationen", "Transformaciones", "Перетворення"]),
    ("Turn Hex on in every app whenever it opens", ["Włączaj Hex we wszystkich aplikacjach przy każdym uruchomieniu", "每次打开时在所有应用中启用 Hex", "起動するたびに、すべてのアプリで Hex をオンにする", "Hex bei jedem Start in allen Apps einschalten", "Activar Hex en todas las apps cada vez que se abra", "Вмикати Hex в усіх програмах під час кожного запуску"]),
    ("Up to date", ["Aktualne", "已是最新", "最新です", "Aktuell", "Actualizado", "Актуальна версія"]),
    ("Update", ["Aktualizuj", "更新", "アップデート", "Aktualisieren", "Actualizar", "Оновити"]),
    ("Use OpenCode default", ["Użyj domyślnego OpenCode", "使用 OpenCode 默认值", "OpenCode の既定を使用", "OpenCode-Standard verwenden", "Usar el predeterminado de OpenCode", "Використовувати типову модель OpenCode"]),
    ("Requires OpenCode", ["Wymaga OpenCode", "需要 OpenCode", "OpenCode が必要です", "Erfordert OpenCode", "Requiere OpenCode", "Потрібен OpenCode"]),
    ("Requires OpenCode V2", ["Wymaga OpenCode V2", "需要 OpenCode V2", "OpenCode V2 が必要です", "Erfordert OpenCode V2", "Requiere OpenCode V2", "Потрібен OpenCode V2"]),
    ("The installed opencode is OpenCode V1, which Hex can't use. Install OpenCode V2.", ["Zainstalowany opencode to OpenCode V1, którego Hex nie może używać. Zainstaluj OpenCode V2.", "已安装的 opencode 是 OpenCode V1，Hex 无法使用。请安装 OpenCode V2。", "インストールされている opencode は OpenCode V1 のため、Hex では使えません。OpenCode V2 をインストールしてください。", "Das installierte opencode ist OpenCode V1, das Hex nicht verwenden kann. Installiere OpenCode V2.", "El opencode instalado es OpenCode V1, que Hex no puede usar. Instala OpenCode V2.", "Встановлений opencode — це OpenCode V1, який Hex не може використовувати. Встановіть OpenCode V2."]),
    ("Transformation requires OpenCode V2", ["Transformacja wymaga OpenCode V2", "转换需要 OpenCode V2", "変換には OpenCode V2 が必要です", "Transformation erfordert OpenCode V2", "La transformación requiere OpenCode V2", "Для перетворення потрібен OpenCode V2"]),
    ("Configure mouse shortcuts in your Wayland desktop using the hex commands", ["Skonfiguruj skróty myszy w ustawieniach pulpitu Wayland za pomocą poleceń hex", "使用 hex 命令在 Wayland 桌面设置中配置鼠标快捷键", "hex コマンドを使って Wayland デスクトップでマウスショートカットを設定してください", "Maus-Shortcuts in den Wayland-Desktop-Einstellungen mit den hex-Befehlen konfigurieren", "Configura los atajos del ratón en tu escritorio Wayland mediante los comandos hex", "Налаштуйте сполучення з мишею у своєму робочому столі Wayland за допомогою команд hex"]),
    ("Wayland doesn't report the frontmost app; these apply under X11", ["Wayland nie podaje aktywnej aplikacji; te reguły działają w X11", "Wayland 不报告前台应用；这些规则仅在 X11 下生效", "Wayland は最前面のアプリを通知しないため、これは X11 でのみ有効です", "Wayland meldet die aktive App nicht; diese Regeln gelten unter X11", "Wayland no informa de la app en primer plano; esto se aplica en X11", "Wayland не повідомляє, яка програма на передньому плані; ці правила діють у X11"]),
    ("Accessibility", ["Dostępność", "辅助功能", "アクセシビリティ", "Bedienungshilfen", "Accesibilidad", "Доступність"]),
    ("Allow", ["Zezwól", "允许", "許可", "Erlauben", "Permitir", "Дозволити"]),
    ("Hex cannot recognize the dictation shortcut or paste into other apps.", ["Hex nie rozpozna skrótu dyktowania ani nie wklei tekstu w innych aplikacjach.", "Hex 无法在其他应用中识别听写快捷键或粘贴文字。", "Hex はほかのアプリで音声入力ショートカットを認識できず、貼り付けもできません。", "Hex erkennt das Diktierkürzel in anderen Apps nicht und kann dort nichts einfügen.", "Hex no puede reconocer el atajo de dictado ni pegar en otras apps.", "Hex не може розпізнати сполучення клавіш для диктування або вставити текст в інші програми."]),
    ("Hex cannot record dictation until microphone access is restored.", ["Hex nie może nagrywać dyktowania, dopóki dostęp do mikrofonu nie zostanie przywrócony.", "在恢复麦克风访问权限之前，Hex 无法录制听写。", "マイクへのアクセスが戻るまで、Hex は音声入力を録音できません。", "Hex kann keine Diktate aufnehmen, bis der Mikrofonzugriff wiederhergestellt ist.", "Hex no puede grabar dictados hasta que se restablezca el acceso al micrófono.", "Hex не може записувати диктування, доки доступ до мікрофона не буде відновлено."]),
    ("The command model could not load. Retry or turn off commands; hotkey dictation is unaffected.", ["Nie udało się wczytać modelu poleceń. Ponów lub wyłącz polecenia; dyktowanie skrótem działa normalnie.", "无法加载命令模型。请重试或关闭命令；快捷键听写不受影响。", "コマンドモデルを読み込めませんでした。再試行するかコマンドをオフにしてください。ショートカットでの音声入力には影響しません。", "Das Befehlsmodell konnte nicht geladen werden. Versuche es erneut oder schalte Befehle aus; das Diktieren per Kürzel ist nicht betroffen.", "No se pudo cargar el modelo de comandos. Reinténtalo o desactiva los comandos; el dictado con atajo no se ve afectado.", "Не вдалося завантажити модель команд. Повторіть спробу або вимкніть команди; диктування за допомогою комбінації клавіш працюватиме й надалі."]),
    ("The command model is loading. Hotkey dictation does not require it.", ["Model poleceń się wczytuje. Dyktowanie skrótem go nie wymaga.", "正在加载命令模型。快捷键听写不需要它。", "コマンドモデルを読み込み中です。ショートカットでの音声入力には不要です。", "Das Befehlsmodell wird geladen. Das Diktieren per Kürzel braucht es nicht.", "El modelo de comandos se está cargando. El dictado con atajo no lo necesita.", "Модель команд завантажується. Для диктування за допомогою комбінації клавіш вона не потрібна."]),
    ("Voice commands are unavailable", ["Polecenia głosowe są niedostępne", "语音命令不可用", "音声コマンドを利用できません", "Sprachbefehle sind nicht verfügbar", "Los comandos de voz no están disponibles", "Голосові команди недоступні"]),
    ("Turn off commands", ["Wyłącz polecenia", "关闭命令", "コマンドをオフにする", "Befehle ausschalten", "Desactivar comandos", "Вимкнути команди"]),
    ("Keep microphone ready & enable commands", ["Utrzymuj mikrofon i włącz polecenia", "保持麦克风就绪并启用命令", "マイクを待機させてコマンドを有効にする", "Mikrofon bereit halten und Befehle aktivieren", "Mantener el micrófono listo y activar comandos", "Тримати мікрофон напоготові й увімкнути команди"]),
    ("Commands", ["Polecenia", "命令", "コマンド", "Befehle", "Comandos", "Команди"]),
    ("Copy agent prompt", ["Kopiuj prompt dla agenta", "复制智能体提示词", "エージェント用プロンプトをコピー", "Agenten-Prompt kopieren", "Copiar prompt para el agente", "Копіювати запит для агента"]),
    ("Create config", ["Utwórz konfigurację", "创建配置", "設定を作成", "Konfiguration erstellen", "Crear configuración", "Створити конфігурацію"]),
    ("Creating config…", ["Tworzenie konfiguracji…", "正在创建配置…", "設定を作成中…", "Konfiguration wird erstellt…", "Creando configuración…", "Створення конфігурації…"]),
    ("Edit config", ["Edytuj konfigurację", "编辑配置", "設定を編集", "Konfiguration bearbeiten", "Editar configuración", "Редагувати конфігурацію"]),
    ("Personal commands are not loading.", ["Osobiste polecenia się nie wczytują.", "个人命令无法加载。", "個人コマンドを読み込めません。", "Persönliche Befehle werden nicht geladen.", "Los comandos personales no se cargan.", "Не вдається завантажити власні команди."]),
    ("No commands yet.", ["Brak poleceń.", "暂无命令。", "コマンドはまだありません。", "Noch keine Befehle.", "Aún no hay comandos.", "Команд ще немає."]),
    ("No matching commands.", ["Brak pasujących poleceń.", "没有匹配的命令。", "一致するコマンドはありません。", "Keine passenden Befehle.", "No hay comandos que coincidan.", "Відповідних команд не знайдено."]),
    ("Search commands", ["Szukaj poleceń", "搜索命令", "コマンドを検索", "Befehle suchen", "Buscar comandos", "Пошук команд"]),
    ("Aliases", ["Aliasy", "别名", "エイリアス", "Aliasse", "Alias", "Псевдоніми"]),
    ("Context", ["Kontekst", "上下文", "コンテキスト", "Kontext", "Contexto", "Контекст"]),
    ("Current context", ["Bieżący kontekst", "当前上下文", "現在のコンテキスト", "Aktueller Kontext", "Contexto actual", "Поточний контекст"]),
    ("None", ["Brak", "无", "なし", "Keine", "Ninguno", "Немає"]),
    ("Keyboard", ["Klawiatura", "键盘", "キーボード", "Tastatur", "Teclado", "Клавіатура"]),
    ("Browser navigation", ["Nawigacja w przeglądarce", "浏览器导航", "ブラウザーの移動", "Browsernavigation", "Navegación del navegador", "Навігація в браузері"]),
    ("Application navigation", ["Nawigacja w aplikacji", "应用导航", "アプリ内の移動", "App-Navigation", "Navegación de la aplicación", "Навігація в програмі"]),
    ("Open applications", ["Otwieranie aplikacji", "打开应用", "アプリを開く", "Apps öffnen", "Abrir aplicaciones", "Відкриття програм"]),
    ("Open websites", ["Otwieranie stron", "打开网站", "ウェブサイトを開く", "Websites öffnen", "Abrir sitios web", "Відкриття вебсайтів"]),
    ("Voice control", ["Sterowanie głosem", "语音控制", "音声操作", "Sprachsteuerung", "Control por voz", "Голосове керування"]),
    ("Meetings (beta)", ["Spotkania (beta)", "会议（测试版）", "会議（ベータ）", "Besprechungen (Beta)", "Reuniones (beta)", "Зустрічі (бета)"]),
    ("Offer to record calls and save local transcripts", ["Proponuj nagrywanie rozmów i zapisuj lokalne transkrypcje", "提供通话录音并保存本地转写", "通話の録音を提案し、ローカルに文字起こしを保存", "Aufnahme von Anrufen anbieten und lokale Transkripte speichern", "Ofrecer grabar llamadas y guardar transcripciones locales", "Пропонувати записувати дзвінки й зберігати транскрипти локально"]),
    ("Screen & System Audio Recording", ["Nagrywanie ekranu i dźwięku systemu", "屏幕与系统音频录制", "画面とシステムオーディオの収録", "Bildschirm- und Systemaudioaufnahme", "Grabación de pantalla y audio del sistema", "Записування системного звуку й екрана"]),
    ("Captures the other participants' audio", ["Nagrywa dźwięk pozostałych uczestników", "录制其他参与者的音频", "ほかの参加者の音声を収録します", "Nimmt den Ton der anderen Teilnehmenden auf", "Graba el audio de los demás participantes", "Записує аудіо інших учасників"]),
    ("Never offer", ["Nigdy nie proponuj", "永不提示", "提案しない", "Nie anbieten", "No ofrecer nunca", "Більше не пропонувати"]),
    ("Detected calls", ["Wykryte rozmowy", "检测到的通话", "検出された通話", "Erkannte Anrufe", "Llamadas detectadas", "Виявлені дзвінки"]),
    ("Ask", ["Pytaj", "询问", "確認する", "Fragen", "Preguntar", "Питати"]),
    ("Always record", ["Zawsze nagrywaj", "始终录制", "常に録音", "Immer aufnehmen", "Grabar siempre", "Завжди записувати"]),
    ("Slack huddles", ["Huddle w Slacku", "Slack 语音小组会", "Slack ハドル", "Slack-Huddles", "Huddles de Slack", "Хадли в Slack"]),
    ("Recording automatically", ["Automatyczne nagrywanie", "正在自动录制", "自動で録音中", "Automatische Aufnahme", "Grabando automáticamente", "Автоматичний запис"]),
    ("Started automatically · {}", ["Rozpoczęto automatycznie · {}", "已自动开始 · {}", "自動で開始 · {}", "Automatisch gestartet · {}", "Iniciada automáticamente · {}", "Розпочато автоматично · {}"]),
    ("Required to hear the other participants.", ["Wymagane, aby słyszeć pozostałych uczestników.", "需要此权限才能听到其他参与者。", "ほかの参加者の声を聞くために必要です。", "Nötig, um die anderen Teilnehmenden zu hören.", "Necesario para oír a los demás participantes.", "Потрібно, щоб чути інших учасників."]),
    ("Transcript folder", ["Folder transkrypcji", "转写文件夹", "文字起こしフォルダー", "Transkriptordner", "Carpeta de transcripciones", "Папка транскриптів"]),
    ("Finished meetings are saved here as Markdown", ["Zakończone spotkania są zapisywane tu jako Markdown", "已结束的会议以 Markdown 格式保存在此处", "終了した会議はここに Markdown で保存されます", "Beendete Besprechungen werden hier als Markdown gespeichert", "Las reuniones terminadas se guardan aquí en Markdown", "Завершені зустрічі зберігаються тут у форматі Markdown"]),
    ("Reveal", ["Pokaż", "显示", "表示", "Anzeigen", "Mostrar", "Показати"]),
    ("Use Folder", ["Użyj folderu", "使用此文件夹", "このフォルダーを使用", "Ordner verwenden", "Usar carpeta", "Використати папку"]),
    ("Summarize with Apple Intelligence", ["Podsumuj za pomocą Apple Intelligence", "使用 Apple 智能进行总结", "Apple Intelligence で要約", "Mit Apple Intelligence zusammenfassen", "Resumir con Apple Intelligence", "Підсумовувати за допомогою Apple Intelligence"]),
    ("Summarize with OpenCode", ["Podsumuj za pomocą OpenCode", "使用 OpenCode 进行总结", "OpenCode で要約", "Mit OpenCode zusammenfassen", "Resumir con OpenCode", "Підсумовувати за допомогою OpenCode"]),
    ("When Apple Intelligence is off or unavailable. Sends the transcript to your OpenCode model", ["Gdy Apple Intelligence jest wyłączone lub niedostępne. Wysyła transkrypcję do Twojego modelu OpenCode", "在 Apple 智能关闭或不可用时使用。会将转写内容发送到你的 OpenCode 模型", "Apple Intelligence がオフまたは使えないとき。文字起こしを OpenCode のモデルに送信します", "Wenn Apple Intelligence aus oder nicht verfügbar ist. Sendet das Transkript an dein OpenCode-Modell", "Cuando Apple Intelligence está desactivado o no disponible. Envía la transcripción a tu modelo de OpenCode", "Коли Apple Intelligence вимкнено або недоступно. Надсилає транскрипт вашій моделі OpenCode"]),
    ("Meeting titles and short summaries. Sends the transcript to your OpenCode model", ["Tytuły i krótkie podsumowania spotkań. Wysyła transkrypcję do Twojego modelu OpenCode", "会议标题和简短总结。会将转写内容发送到你的 OpenCode 模型", "会議のタイトルと短い要約。文字起こしを OpenCode のモデルに送信します", "Titel und kurze Zusammenfassungen von Besprechungen. Sendet das Transkript an dein OpenCode-Modell", "Títulos y resúmenes breves de reuniones. Envía la transcripción a tu modelo de OpenCode", "Назви й короткі підсумки зустрічей. Надсилає транскрипт вашій моделі OpenCode"]),
    ("Record meeting", ["Nagraj spotkanie", "录制会议", "会議を録音", "Besprechung aufnehmen", "Grabar reunión", "Записати зустріч"]),
    ("Copy last meeting transcript", ["Kopiuj ostatnią transkrypcję spotkania", "复制上次会议的转写", "前回の会議の文字起こしをコピー", "Letztes Besprechungstranskript kopieren", "Copiar la última transcripción de reunión", "Копіювати транскрипт останньої зустрічі"]),
    ("Show recording indicator", ["Pokaż wskaźnik nagrywania", "显示录音指示器", "録音インジケーターを表示", "Aufnahmeanzeige einblenden", "Mostrar indicador de grabación", "Показувати індикатор запису"]),
    ("Floating pill to copy live transcript turns or stop recording", ["Pływająca kapsułka do kopiowania bieżącej transkrypcji lub zatrzymania nagrywania", "用于复制实时转写内容或停止录音的悬浮胶囊", "ライブ文字起こしのコピーや録音停止ができるフローティングピル", "Schwebende Kapsel zum Kopieren von Live-Transkripten oder Stoppen der Aufnahme", "Píldora flotante para copiar turnos de transcripción en vivo o detener la grabación", "Плаваюча панель для копіювання окремих частин поточного транскрипту або зупинки запису"]),
    ("Start recording", ["Rozpocznij nagrywanie", "开始录音", "録音を開始", "Aufnahme starten", "Empezar a grabar", "Почати запис"]),
    ("Starting…", ["Uruchamianie…", "正在启动…", "開始中…", "Wird gestartet…", "Iniciando…", "Запуск…"]),
    ("Stop & transcribe", ["Zatrzymaj i transkrybuj", "停止并转写", "停止して文字起こし", "Stoppen und transkribieren", "Detener y transcribir", "Зупинити й розпізнати мовлення"]),
    ("Change anytime in Settings", ["Zmienisz to w Ustawieniach", "可随时在设置中更改", "設定でいつでも変更できます", "Jederzeit in den Einstellungen änderbar", "Cámbialo cuando quieras en Ajustes", "Це можна будь-коли змінити в налаштуваннях"]),
    ("Not now", ["Nie teraz", "以后再说", "今はしない", "Nicht jetzt", "Ahora no", "Не зараз"]),
    ("Record", ["Nagrywaj", "录制", "録音", "Aufnehmen", "Grabar", "Записати"]),
    ("{} left the call", ["{} opuścił rozmowę", "{} 已离开通话", "{} が通話を終了しました", "{} hat den Anruf verlassen", "{} salió de la llamada", "{} більше не в дзвінку"]),
    ("Hex is still recording.", ["Hex nadal nagrywa.", "Hex 仍在录制。", "Hex はまだ録音中です。", "Hex nimmt weiter auf.", "Hex sigue grabando.", "Hex досі записує."]),
    ("Keep recording", ["Nagrywaj dalej", "继续录制", "録音を続ける", "Weiter aufnehmen", "Seguir grabando", "Продовжити запис"]),
    ("Markdown not saved: {}", ["Nie zapisano Markdown: {}", "未保存 Markdown：{}", "Markdown を保存できませんでした: {}", "Markdown nicht gespeichert: {}", "No se guardó el Markdown: {}", "Файл Markdown не збережено: {}"]),
    ("Saved in Meetings", ["Zapisano w Spotkaniach", "已保存到会议", "ミーティングに保存しました", "In Meetings gespeichert", "Guardado en Reuniones", "Збережено в розділі «Зустрічі»"]),
    ("Meeting transcript ready", ["Transkrypcja spotkania gotowa", "会议转写已就绪", "ミーティングの文字起こしができました", "Meeting-Transkript fertig", "Transcripción de la reunión lista", "Транскрипт зустрічі готовий"]),
    ("Ready · {}", ["Gotowe · {}", "已就绪 · {}", "完了 · {}", "Fertig · {}", "Lista · {}", "Готово · {}"]),
    ("Open", ["Otwórz", "打开", "開く", "Öffnen", "Abrir", "Відкрити"]),
    ("Finalizing…", ["Kończenie…", "正在完成…", "仕上げ中…", "Wird abgeschlossen…", "Finalizando…", "Завершення…"]),
    ("Failed", ["Niepowodzenie", "失败", "失敗", "Fehlgeschlagen", "Fallido", "Не вдалося"]),
    ("Interrupted", ["Przerwano", "已中断", "中断", "Unterbrochen", "Interrumpido", "Перервано"]),
    ("Search meetings", ["Szukaj spotkań", "搜索会议", "会議を検索", "Besprechungen suchen", "Buscar reuniones", "Пошук зустрічей"]),
    ("No meetings match.", ["Brak pasujących spotkań.", "没有匹配的会议。", "一致する会議はありません。", "Keine passenden Besprechungen.", "No hay reuniones que coincidan.", "Відповідних зустрічей немає."]),
    ("No meetings yet.", ["Brak spotkań.", "暂无会议。", "会議はまだありません。", "Noch keine Besprechungen.", "Aún no hay reuniones.", "Зустрічей ще немає."]),
    ("Meetings could not be loaded.", ["Nie udało się wczytać spotkań.", "无法加载会议。", "会議を読み込めませんでした。", "Besprechungen konnten nicht geladen werden.", "No se pudieron cargar las reuniones.", "Не вдалося завантажити зустрічі."]),
    ("Meeting could not be recorded.", ["Nie udało się nagrać spotkania.", "无法录制会议。", "会議を録音できませんでした。", "Die Besprechung konnte nicht aufgenommen werden.", "No se pudo grabar la reunión.", "Не вдалося записати зустріч."]),
    ("Meeting could not be updated.", ["Nie udało się zaktualizować spotkania.", "无法更新会议。", "会議を更新できませんでした。", "Die Besprechung konnte nicht aktualisiert werden.", "No se pudo actualizar la reunión.", "Не вдалося оновити зустріч."]),
    ("Recording failed.", ["Nagrywanie nie powiodło się.", "录音失败。", "録音に失敗しました。", "Aufnahme fehlgeschlagen.", "La grabación falló.", "Не вдалося записати аудіо."]),
    ("Markdown not saved.", ["Nie zapisano pliku Markdown.", "未保存 Markdown。", "Markdown を保存できませんでした。", "Markdown nicht gespeichert.", "No se guardó el Markdown.", "Файл Markdown не збережено."]),
    ("Notes could not be written.", ["Nie udało się zapisać notatek.", "无法写入笔记。", "メモを書き込めませんでした。", "Notizen konnten nicht geschrieben werden.", "No se pudieron escribir las notas.", "Не вдалося зберегти нотатки."]),
    ("Live transcript unavailable.", ["Bieżąca transkrypcja jest niedostępna.", "实时转写不可用。", "ライブ文字起こしを利用できません。", "Live-Transkript nicht verfügbar.", "La transcripción en vivo no está disponible.", "Поточний транскрипт недоступний."]),
    ("Select a meeting.", ["Wybierz spotkanie.", "选择一个会议。", "会議を選択してください。", "Wähle eine Besprechung.", "Selecciona una reunión.", "Виберіть зустріч."]),
    ("Record a call to see its notes and transcript here.", ["Nagraj rozmowę, aby zobaczyć tu jej notatki i transkrypcję.", "录制通话后，可在此处查看其笔记和转写。", "通話を録音すると、ここにメモと文字起こしが表示されます。", "Nimm einen Anruf auf, um hier Notizen und Transkript zu sehen.", "Graba una llamada para ver aquí sus notas y transcripción.", "Запишіть дзвінок, щоб переглянути тут нотатки й транскрипт."]),
    ("Copy summary", ["Kopiuj podsumowanie", "复制摘要", "要約をコピー", "Zusammenfassung kopieren", "Copiar resumen", "Копіювати підсумок"]),
    ("Copy transcript", ["Kopiuj transkrypcję", "复制转写", "文字起こしをコピー", "Transkript kopieren", "Copiar transcripción", "Копіювати транскрипт"]),
    ("Rename", ["Zmień nazwę", "重命名", "名前を変更", "Umbenennen", "Cambiar nombre", "Перейменувати"]),
    ("Save", ["Zapisz", "保存", "保存", "Speichern", "Guardar", "Зберегти"]),
    ("Reveal Markdown", ["Pokaż plik Markdown", "显示 Markdown", "Markdown を表示", "Markdown anzeigen", "Mostrar Markdown", "Показати файл Markdown"]),
    ("Reveal files", ["Pokaż pliki", "显示文件", "ファイルを表示", "Dateien anzeigen", "Mostrar archivos", "Показати файли"]),
    ("Delete…", ["Usuń…", "删除…", "削除…", "Löschen…", "Eliminar…", "Видалити…"]),
    ("Trash recording", ["Przenieś nagranie do kosza", "将录音移到废纸篓", "録音をゴミ箱に入れる", "Aufnahme in den Papierkorb", "Mover la grabación a la papelera", "Перемістити запис до Смітника"]),
    ("Trash recording & Markdown", ["Przenieś nagranie i Markdown do kosza", "将录音和 Markdown 移到废纸篓", "録音と Markdown をゴミ箱に入れる", "Aufnahme und Markdown in den Papierkorb", "Mover la grabación y el Markdown a la papelera", "Перемістити запис і файл Markdown до Смітника"]),
    ("Regenerate summary", ["Wygeneruj podsumowanie ponownie", "重新生成摘要", "要約を再生成", "Zusammenfassung neu erstellen", "Regenerar resumen", "Створити підсумок повторно"]),
    ("Summarize", ["Podsumuj", "总结", "要約", "Zusammenfassen", "Resumir", "Підсумувати"]),
    ("Summarizing…", ["Podsumowywanie…", "正在总结…", "要約中…", "Wird zusammengefasst…", "Resumiendo…", "Створення підсумку…"]),
    ("Summary", ["Podsumowanie", "摘要", "要約", "Zusammenfassung", "Resumen", "Підсумок"]),
    ("Nothing to summarize.", ["Nie ma czego podsumować.", "没有可总结的内容。", "要約する内容がありません。", "Nichts zum Zusammenfassen.", "No hay nada que resumir.", "Немає чого підсумовувати."]),
    ("Writing a title and summary with Apple Intelligence…", ["Tworzenie tytułu i podsumowania za pomocą Apple Intelligence…", "正在使用 Apple 智能撰写标题和摘要…", "Apple Intelligence でタイトルと要約を作成中…", "Titel und Zusammenfassung werden mit Apple Intelligence geschrieben…", "Escribiendo un título y un resumen con Apple Intelligence…", "Створення назви й підсумку за допомогою Apple Intelligence…"]),
    ("Building the final transcript…", ["Tworzenie końcowej transkrypcji…", "正在生成最终转写…", "最終的な文字起こしを作成中…", "Endgültiges Transkript wird erstellt…", "Creando la transcripción final…", "Створення остаточного транскрипту…"]),
    ("Listening for speech…", ["Nasłuchiwanie mowy…", "正在聆听语音…", "音声を待機中…", "Warte auf Sprache…", "Escuchando…", "Очікування мовлення…"]),
    ("Preparing local audio capture…", ["Przygotowywanie lokalnego nagrywania dźwięku…", "正在准备本地音频捕获…", "ローカル音声の収録を準備中…", "Lokale Audioaufnahme wird vorbereitet…", "Preparando la captura de audio local…", "Підготовка локального запису аудіо…"]),
    ("Live transcription is unavailable. Audio is still recording safely.", ["Transkrypcja na żywo jest niedostępna. Dźwięk nadal jest bezpiecznie nagrywany.", "实时转写不可用。音频仍在安全录制中。", "ライブ文字起こしは利用できません。音声は引き続き安全に録音されています。", "Live-Transkription ist nicht verfügbar. Der Ton wird weiterhin sicher aufgenommen.", "La transcripción en vivo no está disponible. El audio se sigue grabando de forma segura.", "Розпізнавання мовлення наживо недоступне. Аудіо й далі безпечно записується."]),
    ("No recoverable transcript is available.", ["Brak transkrypcji do odzyskania.", "没有可恢复的转写。", "復元できる文字起こしはありません。", "Kein wiederherstellbares Transkript verfügbar.", "No hay ninguna transcripción recuperable.", "Немає доступного для відновлення транскрипту."]),
    ("No speech was transcribed.", ["Nie przetranskrybowano żadnej mowy.", "没有转写到任何语音。", "文字起こしされた音声はありません。", "Es wurde keine Sprache transkribiert.", "No se transcribió ninguna voz.", "Мовлення не розпізнано."]),
    ("Draft transcript", ["Szkic transkrypcji", "转写草稿", "文字起こしの下書き", "Transkriptentwurf", "Borrador de la transcripción", "Чернетка транскрипту"]),
    ("Live transcript", ["Transkrypcja na żywo", "实时转写", "ライブ文字起こし", "Live-Transkript", "Transcripción en vivo", "Поточний транскрипт"]),
    ("Recovered draft", ["Odzyskany szkic", "已恢复的草稿", "復元した下書き", "Wiederhergestellter Entwurf", "Borrador recuperado", "Відновлена чернетка"]),
    ("Transcript", ["Transkrypcja", "转写", "文字起こし", "Transkript", "Transcripción", "Транскрипт"]),
    ("Transcript unavailable.", ["Transkrypcja jest niedostępna.", "转写不可用。", "文字起こしを利用できません。", "Transkript nicht verfügbar.", "La transcripción no está disponible.", "Транскрипт недоступний."]),
    ("Listening…", ["Słucham…", "正在聆听…", "聞き取り中…", "Hört zu…", "Escuchando…", "Прослуховування…"]),
    ("< 1 min", ["< 1 min", "< 1 分钟", "1 分未満", "< 1 Min.", "< 1 min", "< 1 хв"]),
    ("{} min", ["{} min", "{} 分钟", "{} 分", "{} Min.", "{} min", "{} хв"]),
    ("{hours} h {minutes} min", ["{hours} godz. {minutes} min", "{hours} 小时 {minutes} 分钟", "{hours} 時間 {minutes} 分", "{hours} Std. {minutes} Min.", "{hours} h {minutes} min", "{hours} год {minutes} хв"]),
    ("{} pause", ["{} pauzy", "暂停 {}", "{} の休止", "{} Pause", "{} de pausa", "пауза {}"]),
    ("Accuracy", ["Dokładność", "准确率", "精度", "Genauigkeit", "Precisión", "Точність"]),
    ("Size", ["Rozmiar", "大小", "サイズ", "Größe", "Tamaño", "Розмір"]),
    ("Speed", ["Szybkość", "速度", "速度", "Geschwindigkeit", "Velocidad", "Швидкість"]),
    ("Model could not be deleted.", ["Nie udało się usunąć modelu.", "无法删除模型。", "モデルを削除できませんでした。", "Das Modell konnte nicht gelöscht werden.", "No se pudo eliminar el modelo.", "Не вдалося видалити модель."]),
    ("Delete {}?", ["Usunąć {}?", "删除 {}？", "{} を削除しますか？", "{} löschen?", "¿Eliminar {}?", "Видалити {}?"]),
    ("100 languages", ["100 języków", "100 种语言", "100 言語", "100 Sprachen", "100 idiomas", "100 мов"]),
    ("14 languages", ["14 języków", "14 种语言", "14 言語", "14 Sprachen", "14 idiomas", "14 мов"]),
    ("25 languages", ["25 języków", "25 种语言", "25 言語", "25 Sprachen", "25 idiomas", "25 мов"]),
    ("30 languages", ["30 języków", "30 种语言", "30 言語", "30 Sprachen", "30 idiomas", "30 мов"]),
    ("5 languages", ["5 języków", "5 种语言", "5 言語", "5 Sprachen", "5 idiomas", "5 мов"]),
    ("English", ["Angielski", "英语", "英語", "Englisch", "Inglés", "Англійська"]),
    ("English, without filler words", ["Angielski, bez wtrąceń", "英语，去除填充词", "英語、フィラーなし", "Englisch, ohne Füllwörter", "Inglés, sin muletillas", "Англійська, без слів-паразитів"]),
    ("System locales", ["Języki systemu", "系统区域设置", "システムのロケール", "Systemsprachen", "Idiomas del sistema", "Мови системи"]),
    ("Apple managed", ["Zarządzany przez Apple", "由 Apple 管理", "Apple が管理", "Von Apple verwaltet", "Gestionado por Apple", "Керується Apple"]),
    ("English benchmark", ["Test dla angielskiego", "英语基准测试", "英語ベンチマーク", "Englisch-Benchmark", "Prueba en inglés", "Тестовано для англійської"]),
    ("Mandarin benchmark", ["Test dla mandaryńskiego", "普通话基准测试", "中国語（普通話）ベンチマーク", "Mandarin-Benchmark", "Prueba en mandarín", "Тестовано для китайської (путунхуа)"]),
    ("On device", ["Na urządzeniu", "设备端", "デバイス上", "Auf dem Gerät", "En el dispositivo", "На пристрої"]),
    ("No timestamps", ["Bez znaczników czasu", "无时间戳", "タイムスタンプなし", "Keine Zeitstempel", "Sin marcas de tiempo", "Без часових позначок"]),
    ("Segment timestamps", ["Znaczniki czasu segmentów", "分段时间戳", "セグメントのタイムスタンプ", "Segment-Zeitstempel", "Marcas de tiempo por segmento", "Часові позначки сегментів"]),
    ("Token timestamps", ["Znaczniki czasu tokenów", "词元时间戳", "トークンのタイムスタンプ", "Token-Zeitstempel", "Marcas de tiempo por token", "Часові позначки токенів"]),
    ("Show Hex", ["Pokaż Hex", "显示 Hex", "Hex を表示", "Hex anzeigen", "Mostrar Hex", "Показати Hex"]),
    ("Enable / Disable Hex", ["Włącz / wyłącz Hex", "启用 / 停用 Hex", "Hex を有効 / 無効にする", "Hex aktivieren / deaktivieren", "Activar / desactivar Hex", "Увімкнути / вимкнути Hex"]),
    ("Updated by your package manager", ["Aktualizowany przez menedżer pakietów", "由你的软件包管理器更新", "パッケージマネージャーで更新します", "Wird von deinem Paketmanager aktualisiert", "Se actualiza con tu gestor de paquetes", "Оновлюється через менеджер пакетів"]),
    ("Local", ["Lokalna", "本地", "ローカル", "Lokal", "Local", "Локальна"]),
    ("Test", ["Test", "测试", "テスト", "Test", "Prueba", "Тест"]),
    ("Local build · updates off", ["Lokalna kompilacja · aktualizacje wyłączone", "本地构建 · 更新已关闭", "ローカルビルド · 更新はオフ", "Lokaler Build · Updates aus", "Compilación local · actualizaciones desactivadas", "Локальна збірка · оновлення вимкнено"]),
    ("Updated from your source checkout", ["Aktualizowany z Twojej kopii źródeł", "通过你的源代码检出更新", "ソースのチェックアウトから更新します", "Wird aus deinem Quellcode-Checkout aktualisiert", "Se actualiza desde tu copia del código fuente", "Оновлюється з локальної копії вихідного коду"]),
    ("Quit Hex", ["Zakończ Hex", "退出 Hex", "Hex を終了", "Hex beenden", "Salir de Hex", "Вийти з Hex"]),
    ("Hex Dictation", ["Dyktowanie Hex", "Hex 听写", "Hex 音声入力", "Hex-Diktat", "Dictado de Hex", "Диктування Hex"]),
    ("Hex Dictation: recording", ["Dyktowanie Hex: nagrywanie", "Hex 听写：录音中", "Hex 音声入力: 録音中", "Hex-Diktat: Aufnahme", "Dictado de Hex: grabando", "Диктування Hex: запис"]),
    ("Hex Dictation: transcribing", ["Dyktowanie Hex: transkrypcja", "Hex 听写：转写中", "Hex 音声入力: 文字起こし中", "Hex-Diktat: transkribiert", "Dictado de Hex: transcribiendo", "Диктування Hex: розпізнавання"]),
    ("Hex Dictation: update available", ["Dyktowanie Hex: dostępna aktualizacja", "Hex 听写：有可用更新", "Hex 音声入力: アップデートがあります", "Hex-Diktat: Update verfügbar", "Dictado de Hex: actualización disponible", "Диктування Hex: доступне оновлення"]),
    ("Download update", ["Pobierz aktualizację", "下载更新", "アップデートをダウンロード", "Update herunterladen", "Descargar actualización", "Завантажити оновлення"]),
    ("Downloading update", ["Pobieranie aktualizacji", "正在下载更新", "アップデートをダウンロード中", "Update wird geladen", "Descargando actualización", "Завантаження оновлення"]),
    ("Managed by your organization", ["Zarządzane przez Twoją organizację", "由你的组织管理", "組織によって管理されています", "Von deiner Organisation verwaltet", "Gestionado por tu organización", "Керується вашою організацією"]),
    ("Enabled", ["Włączony", "已启用", "有効", "Aktiviert", "Activado", "Увімкнено"]),
    ("Sleeping", ["Uśpiony", "休眠中", "スリープ中", "Schläft", "En reposo", "У режимі очікування"]),
    ("Enabling…", ["Włączanie…", "正在启用…", "有効化中…", "Wird aktiviert…", "Activando…", "Увімкнення…"]),
    ("Disabling…", ["Wyłączanie…", "正在停用…", "無効化中…", "Wird deaktiviert…", "Desactivando…", "Вимкнення…"]),
    ("Couldn't enable", ["Nie udało się włączyć", "无法启用", "有効にできませんでした", "Aktivierung fehlgeschlagen", "No se pudo activar", "Не вдалося ввімкнути"]),
    ("Saving settings", ["Zapisywanie ustawień", "正在保存设置", "設定を保存中", "Einstellungen werden gespeichert", "Guardando ajustes", "Збереження налаштувань"]),
    ("Service disconnected", ["Usługa rozłączona", "服务已断开", "サービスとの接続が切れました", "Dienst getrennt", "Servicio desconectado", "Звʼязок зі службою втрачено"]),
    ("Linux can't bind both sides. Use one with a key, like Space", ["Linux nie obsługuje obu stron naraz. Użyj jednej z klawiszem, np. Spacją", "Linux 无法绑定左右两侧。请用其中一侧加一个按键，例如空格", "Linux では左右両方は登録できません。片側とキー（Space など）を使ってください", "Linux kann nicht beide Seiten belegen. Nimm eine mit einer Taste, etwa Leertaste", "Linux no puede usar ambos lados. Usa uno con una tecla, como Espacio", "Linux не прив’язує обидві сторони. Використайте одну з клавішею, як-от Пробіл"]),
    ("Linux needs a key too. Hold these and press one, like Space", ["Linux wymaga też klawisza. Przytrzymaj te i naciśnij jeden, np. Spację", "Linux 还需要一个按键。按住这些键再按一个键，例如空格", "Linux ではキーも必要です。これらを押したままキー（Space など）を押してください", "Linux braucht auch eine Taste. Halte diese und drücke eine, etwa Leertaste", "Linux también necesita una tecla. Mantén estas y pulsa una, como Espacio", "Linux потрібна ще клавіша. Утримуйте ці й натисніть одну, як-от Пробіл"]),
    ("Unsupported key", ["Nieobsługiwany klawisz", "不支持的按键", "未対応のキー", "Nicht unterstützte Taste", "Tecla no compatible", "Непідтримувана клавіша"]),
    ("Choose a file…", ["Wybierz plik…", "选择文件…", "ファイルを選択…", "Datei auswählen…", "Elegir un archivo…", "Вибрати файл…"]),
    ("Downloading {}%", ["Pobieranie {}%", "正在下载 {}%", "ダウンロード中 {}%", "Wird geladen {} %", "Descargando {} %", "Завантаження {}%"]),
    ("Loading model · {}s", ["Wczytywanie modelu · {} s", "正在加载模型 · {} 秒", "モデルを読み込み中 · {} 秒", "Modell wird geladen · {} s", "Cargando modelo · {} s", "Завантаження моделі в памʼять · {} с"]),
    ("Enable Hex to dictate into any app with {}", ["Włącz Hex, aby dyktować w dowolnej aplikacji za pomocą {}", "启用 Hex 后，可用 {} 在任意应用中听写", "Hex を有効にすると、{} でどのアプリにも音声入力できます", "Aktiviere Hex, um mit {} in jede App zu diktieren", "Activa Hex para dictar en cualquier app con {}", "Увімкніть Hex, щоб диктувати в будь-якій програмі за допомогою {}"]),
    ("Ready anywhere · Hold {} to dictate", ["Gotowy wszędzie · Przytrzymaj {}, aby dyktować", "随处可用 · 按住 {} 进行听写", "どこでも使えます · {} を押している間に音声入力", "Überall bereit · {} halten zum Diktieren", "Listo en cualquier lugar · Mantén {} para dictar", "Готово всюди · Утримуйте {}, щоб диктувати"]),
    ("Ready anywhere · Press {} to start and stop dictating", ["Gotowy wszędzie · Naciśnij {}, aby zacząć i zakończyć dyktowanie", "随处可用 · 按 {} 开始和停止听写", "どこでも使えます · {} を押して音声入力を開始・停止", "Überall bereit · {} drücken zum Starten und Beenden des Diktats", "Listo en cualquier lugar · Pulsa {} para empezar y terminar de dictar", "Готово всюди · Натисніть {}, щоб почати й завершити диктування"]),
    ("Keep: {}", ["Przechowuj: {}", "保留：{}", "保持: {}", "Behalten: {}", "Conservar: {}", "Зберігати: {}"]),
    ("No applications match \"{}\".", ["Żadna aplikacja nie pasuje do „{}”.", "没有与“{}”匹配的应用。", "「{}」に一致するアプリはありません。", "Keine App passt zu „{}“.", "Ninguna aplicación coincide con «{}».", "Програм, що відповідають запиту «{}», не знайдено."]),
    ("OpenCode models unavailable: {}", ["Modele OpenCode są niedostępne: {}", "OpenCode 模型不可用：{}", "OpenCode のモデルを利用できません: {}", "OpenCode-Modelle nicht verfügbar: {}", "Modelos de OpenCode no disponibles: {}", "Моделі OpenCode недоступні: {}"]),
    ("{} available models", ["Dostępne modele: {}", "{} 个可用模型", "利用可能なモデル {} 件", "{} verfügbare Modelle", "{} modelos disponibles", "Доступні моделі: {}"]),
    ("Mode {}", ["Tryb {}", "模式 {}", "モード {}", "Modus {}", "Modo {}", "Режим {}"]),
    ("{name} already activates {conflict}. Remove it there before reassigning it.", ["{name} już aktywuje tryb {conflict}. Usuń go tam przed ponownym przypisaniem.", "{name} 已用于激活 {conflict}。请先从那里移除再重新分配。", "{name} はすでに {conflict} を有効にしています。割り当て直す前にそちらから削除してください。", "{name} aktiviert bereits {conflict}. Entferne es dort, bevor du es neu zuweist.", "{name} ya activa {conflict}. Quítalo de allí antes de reasignarlo.", "{name} уже активує режим {conflict}. Вилучіть його звідти, перш ніж призначати повторно."]),
    ("Could not load settings: {}", ["Nie udało się wczytać ustawień: {}", "无法加载设置：{}", "設定を読み込めませんでした: {}", "Einstellungen konnten nicht geladen werden: {}", "No se pudieron cargar los ajustes: {}", "Не вдалося завантажити налаштування: {}"]),
    ("Could not restart Hex: {}", ["Nie udało się ponownie uruchomić Hex: {}", "无法重启 Hex：{}", "Hex を再起動できませんでした: {}", "Hex konnte nicht neu gestartet werden: {}", "No se pudo reiniciar Hex: {}", "Не вдалося перезапустити Hex: {}"]),
    ("Could not start listener: {}", ["Nie udało się uruchomić nasłuchiwania: {}", "无法启动监听：{}", "リスナーを開始できませんでした: {}", "Listener konnte nicht gestartet werden: {}", "No se pudo iniciar la escucha: {}", "Не вдалося запустити службу прослуховування: {}"]),
    ("{audio} ms audio · {inference} ms inference", ["{audio} ms dźwięku · {inference} ms wnioskowania", "音频 {audio} 毫秒 · 推理 {inference} 毫秒", "音声 {audio} ms · 推論 {inference} ms", "{audio} ms Audio · {inference} ms Inferenz", "{audio} ms de audio · {inference} ms de inferencia", "{audio} мс аудіо · {inference} мс обробки"]),
    ("{} ms total", ["łącznie {} ms", "总计 {} 毫秒", "合計 {} ms", "{} ms gesamt", "{} ms en total", "Усього: {} мс"]),
    ("fell back: {}", ["użyto oryginału: {}", "已回退为原文：{}", "元のテキストを使用: {}", "Rohtext verwendet: {}", "se usó el texto original: {}", "використано оригінал: {}"]),
    ("Other", ["Inne", "其他", "その他", "Sonstige", "Otros", "Інше"]),
    ("Switch Hex on in the list", ["Włącz Hex na liście", "在列表中打开 Hex", "リストで Hex をオンにしてください", "Hex in der Liste einschalten", "Activa Hex en la lista", "Увімкніть Hex у списку"]),
    ("Speech model ready", ["Model mowy gotowy", "语音模型已就绪", "音声モデルの準備完了", "Sprachmodell bereit", "Modelo de voz listo", "Модель готова"]),
    ("Download stopped", ["Pobieranie przerwane", "下载已停止", "ダウンロードが停止しました", "Download angehalten", "Descarga detenida", "Завантаження зупинено"]),
    ("Hex starts listening once the speech model has downloaded.", ["Hex zacznie słuchać, gdy model mowy się pobierze.", "语音模型下载完成后，Hex 就会开始聆听。", "音声モデルのダウンロードが終わると、Hex が聞き取りを始めます。", "Hex hört zu, sobald das Sprachmodell geladen ist.", "Hex empezará a escuchar cuando termine de descargarse el modelo de voz.", "Hex почне слухати після завантаження моделі розпізнавання мовлення."]),
    ("Device Control and Data Access", ["Sterowanie urządzeniem i dostęp do danych", "设备控制与数据访问", "デバイス制御とデータアクセス", "Gerätesteuerung und Datenzugriff", "Control del dispositivo y acceso a datos", "Керування пристроями й доступ до даних"]),
    ("Allowed", ["Dozwolone", "已允许", "許可済み", "Erlaubt", "Permitido", "Дозволено"]),
    ("Back", ["Wstecz", "返回", "戻る", "Zurück", "Atrás", "Назад"]),
    ("Continue", ["Dalej", "继续", "続ける", "Weiter", "Continuar", "Продовжити"]),
    ("Download a speech model", ["Pobierz model mowy", "下载语音模型", "音声モデルをダウンロード", "Sprachmodell herunterladen", "Descarga un modelo de voz", "Завантажити модель розпізнавання мовлення"]),
    ("Drag Hex into the list, or switch it on if it is there", ["Przeciągnij Hex na listę albo włącz go, jeśli już tam jest", "把 Hex 拖进列表；如果已在列表中，请打开开关", "Hex をリストにドラッグするか、すでにあればオンにします", "Zieh Hex in die Liste oder schalte es ein, falls es schon dort ist", "Arrastra Hex a la lista o actívalo si ya está", "Перетягніть Hex до списку або ввімкніть його, якщо він там уже є"]),
    ("Fix names, tone, and formatting per app.", ["Poprawiaj nazwy, ton i formatowanie dla każdej aplikacji.", "按应用修正名称、语气和格式。", "アプリごとに名前、口調、書式を整えます。", "Korrigiere Namen, Ton und Formatierung pro App.", "Corrige nombres, tono y formato por app.", "Виправляйте назви, тон і форматування для кожної програми."]),
    ("Get started", ["Zaczynamy", "开始", "はじめる", "Los geht's", "Empezar", "Почати"]),
    ("Give it a try", ["Wypróbuj", "试一试", "試してみましょう", "Probier es aus", "Pruébalo", "Спробуйте"]),
    ("Hears you only while you hold the shortcut", ["Słyszy cię tylko, gdy trzymasz skrót", "仅在按住快捷键时收听", "ショートカットを押している間だけ聞き取ります", "Hört nur zu, während du das Kürzel hältst", "Solo te escucha mientras mantienes el atajo", "Слухає вас, лише поки ви утримуєте комбінацію клавіш"]),
    ("Hex did not catch any words. Speak a little closer, or pick another microphone below.", ["Hex nie usłyszał żadnych słów. Mów bliżej albo wybierz inny mikrofon poniżej.", "Hex 没有听到任何文字。请靠近一点说话，或在下方选择其他麦克风。", "Hex は言葉を聞き取れませんでした。もう少し近くで話すか、下で別のマイクを選んでください。", "Hex hat keine Wörter erkannt. Sprich etwas näher oder wähle unten ein anderes Mikrofon.", "Hex no captó ninguna palabra. Habla un poco más cerca o elige otro micrófono abajo.", "Hex не розчув жодного слова. Говоріть трохи ближче або виберіть інший мікрофон нижче."]),
    ("The lid is closed, so the built-in mic is off.", ["Pokrywa jest zamknięta, więc wbudowany mikrofon jest wyłączony.", "合盖后内置麦克风已关闭。", "蓋が閉じているため、内蔵マイクはオフです。", "Der Deckel ist zu, daher ist das eingebaute Mikrofon aus.", "La tapa está cerrada, así que el micrófono integrado está apagado.", "Кришку закрито, тож вбудований мікрофон вимкнено."]),
    ("No sound. Pick another mic?", ["Brak dźwięku. Wybrać inny mikrofon?", "没有声音。换个麦克风？", "音が聞こえません。別のマイクにしますか？", "Kein Ton. Anderes Mikrofon wählen?", "No hay sonido. ¿Eliges otro micrófono?", "Немає звуку. Вибрати інший мікрофон?"]),
    ("Microphone, {}", ["Mikrofon, {}", "麦克风，{}", "マイク、{}", "Mikrofon, {}", "Micrófono, {}", "Мікрофон, {}"]),
    ("Hex needs to reopen before it can hear the shortcut.", ["Hex musi zostać ponownie otwarty, aby usłyszeć skrót.", "Hex 需要重新打开才能识别快捷键。", "ショートカットを認識するには Hex を開き直す必要があります。", "Hex muss neu geöffnet werden, um das Kürzel zu erkennen.", "Hex necesita reabrirse para detectar el atajo.", "Щоб Hex розпізнавав комбінацію клавіш, його потрібно перезапустити."]),
    ("Hex transcribes on this computer, so it needs a local model first.", ["Hex transkrybuje na tym komputerze, więc najpierw potrzebuje lokalnego modelu.", "Hex 在这台电脑上转写，因此需要先下载本地模型。", "Hex はこのコンピュータ上で文字起こしするため、まずローカルモデルが必要です。", "Hex transkribiert auf diesem Computer und braucht dafür zuerst ein lokales Modell.", "Hex transcribe en este ordenador, así que primero necesita un modelo local.", "Hex розпізнає мовлення на цьому компʼютері, тому спочатку потрібна локальна модель."]),
    ("Hex waits in the system tray. Use {} in any app to dictate.", ["Hex czeka w zasobniku systemowym. Użyj {} w dowolnej aplikacji, aby dyktować.", "Hex 待在系统托盘中。在任意应用中使用 {} 即可听写。", "Hex はシステムトレイで待機しています。どのアプリでも {} で音声入力できます。", "Hex wartet im Infobereich. Nutze {} in jeder App zum Diktieren.", "Hex espera en la bandeja del sistema. Usa {} en cualquier app para dictar.", "Hex працює в області сповіщень. Використовуйте {}, щоб диктувати в будь-якій програмі."]),
    ("Hex keeps running after you close this window. Use {} in any app to dictate.", ["Hex działa dalej po zamknięciu tego okna. Użyj {} w dowolnej aplikacji, aby dyktować.", "关闭此窗口后 Hex 仍会继续运行。在任意应用中使用 {} 即可听写。", "このウィンドウを閉じても Hex は動き続けます。どのアプリでも {} で音声入力できます。", "Hex läuft weiter, wenn du dieses Fenster schließt. Nutze {} in jeder App zum Diktieren.", "Hex sigue funcionando al cerrar esta ventana. Usa {} en cualquier app para dictar.", "Hex продовжує працювати після закриття цього вікна. Використовуйте {}, щоб диктувати в будь-якій програмі."]),
    ("Hex waits in the menu bar. Use {} in any app to dictate.", ["Hex czeka na pasku menu. Użyj {} w dowolnej aplikacji, aby dyktować.", "Hex 待在菜单栏中。在任意应用中使用 {} 即可听写。", "Hex はメニューバーで待機しています。どのアプリでも {} で音声入力できます。", "Hex wartet in der Menüleiste. Nutze {} in jeder App zum Diktieren.", "Hex espera en la barra de menús. Usa {} en cualquier app para dictar.", "Hex залишається на смузі меню. Використовуйте {}, щоб диктувати в будь-якій програмі."]),
    ("Hold a key, say what you mean, and Hex types it wherever your cursor is. Your voice is transcribed on this Mac.", ["Przytrzymaj klawisz, powiedz, co chcesz, a Hex wpisze to tam, gdzie jest kursor. Głos jest transkrybowany na tym Macu.", "按住一个键，说出你想说的，Hex 会把它输入到光标所在的位置。语音在这台 Mac 上转写。", "キーを押したまま話すと、Hex がカーソルの位置に入力します。音声はこの Mac 上で文字起こしされます。", "Halte eine Taste, sag, was du meinst, und Hex tippt es dort, wo dein Cursor steht. Deine Stimme wird auf diesem Mac transkribiert.", "Mantén una tecla, di lo que quieres y Hex lo escribe donde esté el cursor. Tu voz se transcribe en este Mac.", "Утримуйте клавішу, скажіть, що маєте на увазі, і Hex надрукує це там, де стоїть курсор. Ваше мовлення розпізнається на цьому Mac."]),
    ("Hold a key, say what you mean, and Hex types it wherever your cursor is. Your voice is transcribed on this computer.", ["Przytrzymaj klawisz, powiedz, co chcesz, a Hex wpisze to tam, gdzie jest kursor. Głos jest transkrybowany na tym komputerze.", "按住一个键，说出你想说的，Hex 会把它输入到光标所在的位置。语音在这台电脑上转写。", "キーを押したまま話すと、Hex がカーソルの位置に入力します。音声はこのコンピュータ上で文字起こしされます。", "Halte eine Taste, sag, was du meinst, und Hex tippt es dort, wo dein Cursor steht. Deine Stimme wird auf diesem Computer transkribiert.", "Mantén una tecla, di lo que quieres y Hex lo escribe donde esté el cursor. Tu voz se transcribe en este ordenador.", "Утримуйте клавішу, скажіть, що маєте на увазі, і Hex надрукує це там, де стоїть курсор. Ваше мовлення розпізнається на цьому компʼютері."]),
    ("Hold to talk", ["Przytrzymaj, aby mówić", "按住说话", "押したまま話す", "Halten zum Sprechen", "Mantén para hablar", "Утримуйте, щоб говорити"]),
    ("Hold {}, say a sentence, then let go.", ["Przytrzymaj {}, powiedz zdanie i puść.", "按住 {}，说一句话，然后松开。", "{} を押したまま一文話して、離します。", "Halte {}, sag einen Satz und lass dann los.", "Mantén {}, di una frase y suelta.", "Утримуйте {}, промовте речення, а потім відпустіть."]),
    ("Let Hex hear you and type for you", ["Pozwól Hex słyszeć cię i pisać za ciebie", "让 Hex 听见你并替你输入", "Hex が聞き取り、入力できるようにする", "Lass Hex zuhören und für dich tippen", "Deja que Hex te escuche y escriba por ti", "Дозвольте Hex слухати вас і друкувати за вас"]),
    ("Listening… let go of {} when you are done.", ["Słucham… puść {}, gdy skończysz.", "正在聆听… 说完后松开 {}。", "聞き取り中… 話し終えたら {} を離してください。", "Hört zu … lass {} los, wenn du fertig bist.", "Escuchando… suelta {} cuando termines.", "Прослуховування… відпустіть {}, коли закінчите."]),
    ("Listening… press {} again when you are done.", ["Słucham… naciśnij {} ponownie, gdy skończysz.", "正在聆听… 说完后再按一次 {}。", "聞き取り中… 話し終えたらもう一度 {} を押してください。", "Hört zu … drück {} erneut, wenn du fertig bist.", "Escuchando… pulsa {} otra vez cuando termines.", "Прослуховування… натисніть {} ще раз, коли закінчите."]),
    ("Missed the text field? Paste it again anywhere.", ["Nie trafiło w pole tekstowe? Wklej ponownie w dowolnym miejscu.", "没粘贴到文本框？可以在任意位置再次粘贴。", "テキスト欄から外れましたか？どこにでももう一度貼り付けられます。", "Textfeld verpasst? Füge es überall erneut ein.", "¿No cayó en el campo de texto? Pégalo de nuevo donde quieras.", "Не потрапили в текстове поле? Вставте текст ще раз у будь-якому місці."]),
    ("Notices the shortcut and pastes your words in any app", ["Rozpoznaje skrót i wkleja słowa w każdej aplikacji", "在所有应用中识别快捷键并粘贴文字", "どのアプリでもショートカットを認識し、言葉を貼り付けます", "Erkennt das Kürzel und fügt deine Worte in jeder App ein", "Detecta el atajo y pega tus palabras en cualquier app", "Помічає сполучення клавіш і вставляє ваші слова в будь-якій програмі"]),
    ("Paste your last dictation", ["Wklej ostatnie dyktowanie", "粘贴上一次听写", "直前の音声入力を貼り付け", "Letztes Diktat einfügen", "Pega tu último dictado", "Вставити останнє диктування"]),
    ("Press to talk", ["Naciśnij, aby mówić", "按下说话", "押して話す", "Drücken zum Sprechen", "Pulsa para hablar", "Натисніть, щоб говорити"]),
    ("Press {}, say a sentence, then press it again.", ["Naciśnij {}, powiedz zdanie i naciśnij ponownie.", "按下 {}，说一句话，再按一次。", "{} を押して一文話し、もう一度押します。", "Drück {}, sag einen Satz und drück dann erneut.", "Pulsa {}, di una frase y vuelve a pulsar.", "Натисніть {}, промовте речення, а потім натисніть ще раз."]),
    ("Quit and reopen Hex to finish setup. macOS applies some permissions only after a restart.", ["Zamknij i ponownie otwórz Hex, aby dokończyć konfigurację. macOS stosuje niektóre uprawnienia dopiero po ponownym uruchomieniu.", "请退出并重新打开 Hex 以完成设置。macOS 的部分权限需要重启后才会生效。", "設定を完了するには Hex を終了して開き直してください。macOS の一部の許可は再起動後に有効になります。", "Beende Hex und öffne es erneut, um die Einrichtung abzuschließen. macOS übernimmt manche Berechtigungen erst nach einem Neustart.", "Cierra y vuelve a abrir Hex para terminar la configuración. macOS aplica algunos permisos solo tras reiniciar.", "Завершіть роботу Hex і відкрийте його знову, щоб завершити налаштування. macOS застосовує деякі дозволи лише після перезапуску."]),
    ("Reopen Hex", ["Otwórz Hex ponownie", "重新打开 Hex", "Hex を開き直す", "Hex neu öffnen", "Reabrir Hex", "Відкрити Hex знову"]),
    ("Restart Hex", ["Uruchom Hex ponownie", "重新启动 Hex", "Hex を再起動", "Hex neu starten", "Reiniciar Hex", "Перезапустити Hex"]),
    ("Turns on when Hex restarts", ["Włączy się po ponownym uruchomieniu Hex", "Hex 重新启动后生效", "Hex の再起動後に有効になります", "Wird nach einem Neustart von Hex aktiv", "Se activa cuando Hex se reinicia", "Увімкнеться після перезапуску Hex"]),
    ("Runs entirely on this computer", ["Działa w całości na tym komputerze", "完全在这台电脑上运行", "すべてこのコンピュータ上で動作します", "Läuft vollständig auf diesem Computer", "Funciona por completo en este ordenador", "Працює повністю на цьому компʼютері"]),
    ("Skip", ["Pomiń", "跳过", "スキップ", "Überspringen", "Omitir", "Пропустити"]),
    ("Something went wrong while transcribing. Try once more.", ["Coś poszło nie tak podczas transkrypcji. Spróbuj jeszcze raz.", "转写时出错了。请再试一次。", "文字起こし中に問題が発生しました。もう一度お試しください。", "Beim Transkribieren ist etwas schiefgegangen. Versuch es noch einmal.", "Algo salió mal al transcribir. Inténtalo otra vez.", "Під час розпізнавання мовлення сталася помилка. Спробуйте ще раз."]),
    ("Speech model", ["Model mowy", "语音模型", "音声モデル", "Sprachmodell", "Modelo de voz", "Модель"]),
    ("Language", ["Język", "语言", "言語", "Sprache", "Idioma", "Мова"]),
    ("{model} doesn't support {previous}, so the language changed to {language}", ["{model} nie obsługuje języka: {previous}, więc język zmieniono na: {language}", "{model} 不支持{previous}，因此语言已改为{language}", "{model} は{previous}に対応していないため、言語を{language}に変更しました", "{model} unterstützt {previous} nicht, daher wurde die Sprache auf {language} geändert", "{model} no admite {previous}, así que el idioma cambió a {language}", "{model} не підтримує {previous}, тому мову змінено на {language}"]),
    ("Doesn't support {language}; switches the language to {fallback}", ["Nie obsługuje języka: {language}; przełącza język na: {fallback}", "不支持{language}；会将语言切换为{fallback}", "{language}に非対応。言語を{fallback}に切り替えます", "Unterstützt {language} nicht; stellt die Sprache auf {fallback} um", "No admite {language}; cambia el idioma a {fallback}", "Не підтримує {language}; мову буде змінено на {fallback}"]),
    ("Start using Hex", ["Zacznij używać Hex", "开始使用 Hex", "Hex を使い始める", "Hex verwenden", "Empezar a usar Hex", "Почати користуватися Hex"]),
    ("That was a quick tap. Keep holding {} while you speak.", ["To było krótkie stuknięcie. Trzymaj {} podczas mówienia.", "这只是轻点了一下。说话时请一直按住 {}。", "短いタップでした。話している間は {} を押し続けてください。", "Das war nur ein kurzes Tippen. Halte {} gedrückt, während du sprichst.", "Fue un toque rápido. Mantén {} mientras hablas.", "Ви надто швидко натиснули. Продовжуйте утримувати {}, поки говорите."]),
    ("That was too short to hear. Speak a full sentence before you finish.", ["To było za krótkie. Powiedz całe zdanie, zanim skończysz.", "太短了，没听清。请说完一整句话再结束。", "短すぎて聞き取れませんでした。一文話してから終了してください。", "Das war zu kurz. Sprich einen ganzen Satz, bevor du beendest.", "Fue demasiado corto. Di una frase completa antes de terminar.", "Запис був надто коротким. Промовте повне речення, перш ніж завершити."]),
    ("That's it. Hex pastes wherever your cursor is, in any app.", ["Gotowe. Hex wkleja tam, gdzie jest kursor, w każdej aplikacji.", "就是这样。在任何应用中，Hex 都会粘贴到光标处。", "これで完了です。Hex はどのアプリでもカーソルの位置に貼り付けます。", "Das war's. Hex fügt in jeder App dort ein, wo dein Cursor steht.", "Eso es todo. Hex pega donde esté el cursor, en cualquier app.", "Ось і все. Hex вставляє текст у місці курсора в будь-якій програмі."]),
    ("The download stopped. Check your connection and try again.", ["Pobieranie zostało przerwane. Sprawdź połączenie i spróbuj ponownie.", "下载已中断。请检查网络连接后重试。", "ダウンロードが止まりました。接続を確認してもう一度お試しください。", "Der Download wurde unterbrochen. Prüf deine Verbindung und versuch es erneut.", "La descarga se detuvo. Revisa tu conexión e inténtalo de nuevo.", "Завантаження зупинилося. Перевірте підключення й спробуйте ще раз."]),
    ("Transcribing…", ["Transkrybowanie…", "正在转写…", "文字起こし中…", "Transkribiert …", "Transcribiendo…", "Розпізнавання мовлення…"]),
    ("Turned off earlier. Switch Hex on in System Settings.", ["Wcześniej wyłączono. Włącz Hex w Ustawieniach systemowych.", "之前已关闭。请在系统设置中打开 Hex。", "以前にオフにされています。システム設定で Hex をオンにしてください。", "Zuvor ausgeschaltet. Schalte Hex in den Systemeinstellungen ein.", "Se desactivó antes. Activa Hex en Ajustes del Sistema.", "Раніше було вимкнено. Увімкніть Hex у Системних параметрах."]),
    ("When you want more", ["Gdy zechcesz więcej", "想要更多时", "もっと使いたくなったら", "Wenn du mehr willst", "Cuando quieras más", "Коли захочете більшого"]),
    ("You're all set", ["Wszystko gotowe", "一切就绪", "準備完了", "Alles bereit", "Todo listo", "Усе готово"]),
    ("Your words will appear here", ["Tu pojawią się twoje słowa", "你的文字会显示在这里", "ここに言葉が表示されます", "Hier erscheinen deine Worte", "Tus palabras aparecerán aquí", "Тут зʼявиться ваш текст"]),
    // Fun / Hex Microphone (experimental)
    ("Available", ["Dostępny", "可用", "利用可能", "Verfügbar", "Disponible", "Доступний"]),
    ("Virtual microphone", ["Wirtualny mikrofon", "虚拟麦克风", "仮想マイク", "Virtuelles Mikrofon", "Micrófono virtual", "Віртуальний мікрофон"]),
    ("Could not start Hex Microphone: {}", ["Nie udało się uruchomić Hex Microphone: {}", "无法启动 Hex Microphone：{}", "Hex Microphone を開始できませんでした: {}", "Hex Microphone konnte nicht gestartet werden: {}", "No se pudo iniciar Hex Microphone: {}", "Не вдалося запустити Hex Microphone: {}"]),
    ("Starting Hex Microphone…", ["Uruchamianie Hex Microphone…", "正在启动 Hex Microphone…", "Hex Microphone を開始中…", "Hex Microphone wird gestartet…", "Iniciando Hex Microphone…", "Запуск Hex Microphone…"]),
    ("Call hears", ["Rozmówcy słyszą", "通话对方听到", "通話相手に聞こえる音", "Anruf hört", "La llamada oye", "Що чує співрозмовник"]),
    ("Carrier", ["Fala nośna", "载波频率", "キャリア周波数", "Trägerfrequenz", "Portadora", "Несуча частота"]),
    ("Chipmunk", ["Wiewiórka", "花栗鼠", "チップマンク", "Chipmunk", "Ardilla", "Бурундук"]),
    ("Choose Hex Microphone as your input in Slack, Zoom, or Discord", ["Wybierz Hex Microphone jako wejście w Slacku, Zoomie lub Discordzie", "在 Slack、Zoom 或 Discord 中选择 Hex Microphone 作为输入设备", "Slack、Zoom、または Discord の入力として Hex Microphone を選択してください", "Wähle Hex Microphone als Eingabegerät in Slack, Zoom oder Discord", "Elige Hex Microphone como entrada en Slack, Zoom o Discord", "Виберіть Hex Microphone як пристрій введення в Slack, Zoom або Discord"]),
    ("Comfort noise", ["Szum tła", "舒适背景音", "コンフォートノイズ", "Komfortrauschen", "Ruido de confort", "Комфортний шум"]),
    ("Compare your raw microphone with what the call hears during dictation", ["Porównaj surowy dźwięk mikrofonu z tym, co słyszą rozmówcy podczas dyktowania", "对比原始麦克风声音与听写时通话对方听到的声音", "元のマイク音声とディクテーション中に相手へ届く音を比較します", "Vergleiche dein Originalmikrofon mit dem, was der Anruf beim Diktieren hört", "Compara tu micrófono original con lo que oye la llamada mientras dictas", "Порівняйте необроблений звук із мікрофона з тим, що чують співрозмовники під час диктування"]),
    ("Crunch", ["Przester", "失真度", "クランチ", "Crunch", "Saturación", "Спотворення"]),
    ("Depth", ["Głębokość", "深度", "深さ", "Tiefe", "Profundidad", "Глибина"]),
    ("Driver not installed", ["Sterownik niezainstalowany", "驱动未安装", "ドライバ未インストール", "Treiber nicht installiert", "Controlador no instalado", "Драйвер не встановлено"]),
    ("Effect strength", ["Siła efektu", "效果强度", "エフェクトの強さ", "Effektstärke", "Intensidad del efecto", "Інтенсивність ефекту"]),
    ("Example", ["Przykład", "示例", "サンプル", "Beispiel", "Ejemplo", "Приклад"]),
    ("Experimental: Hex microphone", ["Eksperymentalne: mikrofon Hex", "实验功能：Hex 麦克风", "実験的機能: Hex マイク", "Experimentell: Hex-Mikrofon", "Experimental: micrófono Hex", "Експериментальна функція: мікрофон Hex"]),
    ("Far", ["Z daleka", "远处", "遠くから", "Weit weg", "Lejos", "Здалеку"]),
    ("Hex Microphone", ["Hex Microphone", "Hex Microphone", "Hex Microphone", "Hex Microphone", "Hex Microphone", "Hex Microphone"]),
    ("In use by {}", ["Używany przez: {}", "{} 正在使用", "{} が使用中", "Verwendet von {}", "En uso por {}", "Використовується: {}"]),
    ("Install", ["Zainstaluj", "安装", "インストール", "Installieren", "Instalar", "Встановити"]),
    ("Install the CoreAudio driver to choose Hex Microphone in Slack, Zoom, or Discord", ["Zainstaluj sterownik CoreAudio, aby wybrać Hex Microphone w Slacku, Zoomie lub Discordzie", "安装 CoreAudio 驱动后即可在 Slack、Zoom 或 Discord 中选择 Hex Microphone", "CoreAudio ドライバをインストールすると、Slack・Zoom・Discord で Hex Microphone を選べます", "Installiere den CoreAudio-Treiber, um Hex Microphone in Slack, Zoom oder Discord auszuwählen", "Instala el controlador CoreAudio para elegir Hex Microphone en Slack, Zoom o Discord", "Встановіть драйвер CoreAudio, щоб вибрати Hex Microphone у Slack, Zoom або Discord"]),
    ("Installation cancelled", ["Instalacja anulowana", "已取消安装", "インストールがキャンセルされました", "Installation abgebrochen", "Instalación cancelada", "Встановлення скасовано"]),
    ("Installed v{}", ["Zainstalowano v{}", "已安装 v{}", "v{} インストール済み", "v{} installiert", "Instalado v{}", "Встановлено v{}"]),
    ("Installing driver and restarting Core Audio…", ["Instalowanie sterownika i restartowanie Core Audio…", "正在安装驱动并重启 Core Audio…", "ドライバをインストールし Core Audio を再起動中…", "Treiber wird installiert und Core Audio neu gestartet…", "Instalando controlador y reiniciando Core Audio…", "Встановлення драйвера й перезапуск Core Audio…"]),
    ("Installing…", ["Instalowanie…", "正在安装…", "インストール中…", "Wird installiert…", "Instalando…", "Встановлення…"]),
    ("Cancelled", ["Anulowano", "已取消", "キャンセル", "Abgebrochen", "Cancelado", "Скасовано"]),
    ("Could not install driver: {}", ["Nie udało się zainstalować sterownika: {}", "无法安装驱动：{}", "ドライバをインストールできませんでした: {}", "Treiber konnte nicht installiert werden: {}", "No se pudo instalar el controlador: {}", "Не вдалося встановити драйвер: {}"]),
    ("Could not update driver: {}", ["Nie udało się zaktualizować sterownika: {}", "无法更新驱动：{}", "ドライバを更新できませんでした: {}", "Treiber konnte nicht aktualisiert werden: {}", "No se pudo actualizar el controlador: {}", "Не вдалося оновити драйвер: {}"]),
    ("Driver removed", ["Sterownik usunięty", "驱动已移除", "ドライバを削除しました", "Treiber entfernt", "Controlador eliminado", "Драйвер видалено"]),
    ("Removed", ["Usunięto", "已移除", "削除済み", "Entfernt", "Eliminado", "Видалено"]),
    ("Removing driver and restarting Core Audio…", ["Usuwanie sterownika i restartowanie Core Audio…", "正在移除驱动并重启 Core Audio…", "ドライバを削除し Core Audio を再起動中…", "Treiber wird entfernt und Core Audio neu gestartet…", "Eliminando controlador y reiniciando Core Audio…", "Видалення драйвера й перезапуск Core Audio…"]),
    ("Uninstall", ["Odinstaluj", "卸载", "アンインストール", "Deinstallieren", "Desinstalar", "Видалити"]),
    ("Uninstall cancelled", ["Odinstalowanie anulowane", "已取消卸载", "アンインストールがキャンセルされました", "Deinstallation abgebrochen", "Desinstalación cancelada", "Видалення скасовано"]),
    ("Uninstalling…", ["Odinstalowywanie…", "正在卸载…", "アンインストール中…", "Wird deinstalliert…", "Desinstalando…", "Видалення…"]),
    ("Update cancelled", ["Aktualizacja anulowana", "已取消更新", "更新がキャンセルされました", "Update abgebrochen", "Actualización cancelada", "Оновлення скасовано"]),
    ("Update driver to v{}", ["Zaktualizuj sterownik do v{}", "将驱动更新到 v{}", "ドライバを v{} に更新", "Treiber auf v{} aktualisieren", "Actualizar controlador a v{}", "Оновити драйвер до v{}"]),
    ("Update v{}", ["Aktualizacja v{}", "更新 v{}", "v{} に更新", "Update v{}", "Actualizar v{}", "Оновлення v{}"]),
    ("v{} installed", ["Zainstalowano v{}", "已安装 v{}", "v{} インストール済み", "v{} installiert", "v{} instalado", "v{} встановлено"]),
    ("Keeps a quiet room tone while muted so callers know the line is live", ["Zachowuje cichy szum pokoju podczas wyciszenia, by rozmówcy wiedzieli, że połączenie trwa", "静音时保留轻微环境底噪，让对方知道通话仍在线", "ミュート中もわずかなルームトーンを残し、通話が繋がっていることを伝えます", "Behält beim Stummschalten einen leisen Raumton bei, damit die Leitung lebendig wirkt", "Mantiene un tono ambiental suave al silenciar para que la llamada siga viva", "Підтримує тихий фоновий шум, коли мікрофон вимкнено, щоб співрозмовники знали, що звʼязок активний"]),
    ("Level", ["Poziom", "音量", "レベル", "Pegel", "Nivel", "Рівень"]),
    ("Mode while dictating", ["Tryb podczas dyktowania", "听写时的声音模式", "ディクテーション中のモード", "Modus beim Diktieren", "Modo al dictar", "Режим під час диктування"]),
    ("Muffled", ["Przytłumiony", "捂麦", "こもった声", "Gedämpft", "Amortiguado", "Приглушений"]),
    ("Muted", ["Wyciszony", "静音", "ミュート", "Stumm", "Silenciado", "Без звуку"]),
    ("Normal", ["Normalny", "正常", "通常", "Normal", "Normal", "Звичайний"]),
    ("Open Fun", ["Otwórz Zabawę", "打开趣玩", "遊び場を開く", "Spaß öffnen", "Abrir Diversión", "Відкрити розваги"]),
    ("Passes your voice through unchanged while you dictate", ["Przekazuje twój głos bez zmian podczas dyktowania", "听写时原样传递你的声音", "ディクテーション中も声をそのまま通します", "Leitet deine Stimme beim Diktieren unverändert weiter", "Deja pasar tu voz sin cambios mientras dictas", "Передає ваш голос без змін під час диктування"]),
    ("Pitch", ["Wysokość tonu", "音高", "ピッチ", "Tonhöhe", "Tono", "Висота тону"]),
    ("Play what call hears", ["Odtwórz co słyszy rozmówca", "播放通话对方听到的声音", "相手に聞こえる音を再生", "Abspielen, was der Anruf hört", "Reproducir lo que oye la llamada", "Відтворити те, що чує співрозмовник"]),
    ("Play your voice", ["Odtwórz swój głos", "播放你的原声", "自分の声を再生", "Eigene Stimme abspielen", "Reproducir tu voz", "Відтворити ваш голос"]),
    ("Quiet", ["Cichy", "轻声", "小声", "Leise", "Suave", "Тихий"]),
    ("Record 5s", ["Nagraj 5 s", "录制 5 秒", "5秒録音", "5 Sek. aufnehmen", "Grabar 5 s", "Записати 5 с"]),
    ("Reset", ["Resetuj", "重置", "リセット", "Zurücksetzen", "Restablecer", "Скинути"]),
    ("Robot", ["Robot", "机器人", "ロボット", "Roboter", "Robot", "Робот"]),
    ("Room", ["Pogłos", "房间混响", "ルーム残響", "Raumklang", "Sala", "Кімната"]),
    ("Room tone", ["Szum pokoju", "环境底噪", "ルームトーン", "Raumton", "Tono ambiental", "Фоновий шум приміщення"]),
    ("Silences your voice with soft room tone while you dictate", ["Wycisza twój głos delikatnym szumem pokoju podczas dyktowania", "听写时用柔和的环境底噪隐去你的声音", "ディクテーション中は柔らかなルームトーンで声をミュートします", "Schaltet deine Stimme beim Diktieren mit sanftem Raumton stumm", "Silencia tu voz con un suave tono ambiental mientras dictas", "Заглушує ваш голос, додаючи тихий фоновий шум приміщення під час диктування"]),
    ("Sounds like a hand over the microphone while you dictate", ["Brzmi jak dłoń zakrywająca mikrofon podczas dyktowania", "听写时听起来像用手捂住了麦克风", "ディクテーション中、マイクを手で覆ったような音になります", "Klingt beim Diktieren wie eine Hand über dem Mikrofon", "Suena como si taparas el micrófono con la mano mientras dictas", "Звучить так, ніби під час диктування мікрофон прикрито рукою"]),
    ("Sounds like a handheld radio while you dictate", ["Brzmi jak krótkofalówka podczas dyktowania", "听写时听起来像对讲机", "ディクテーション中、トランシーバーのような音になります", "Klingt beim Diktieren wie ein Funkgerät", "Suena como un walkie-talkie mientras dictas", "Звучить як рація під час диктування"]),
    ("Sounds like you are across the room while you dictate", ["Brzmi jakbyś mówił z drugiego końca pokoju podczas dyktowania", "听写时听起来像在房间另一头说话", "ディクテーション中、部屋の離れた場所から話しているように聞こえます", "Klingt beim Diktieren, als wärst du am anderen Ende des Raums", "Suena como si hablaras desde el otro lado de la habitación mientras dictas", "Звучить так, ніби ви говорите з іншого кінця кімнати під час диктування"]),
    ("Sounds pitched up while you dictate", ["Podwyższa ton głosu podczas dyktowania", "听写时声音变尖", "ディクテーション中、声が高くなります", "Klingt beim Diktieren höher gepitcht", "Suena con tono agudo mientras dictas", "Звучить вище за тоном під час диктування"]),
    ("Sounds robotic while you dictate", ["Brzmi jak głos robota podczas dyktowania", "听写时听起来像机器人", "ディクテーション中、ロボットのような声になります", "Klingt beim Diktieren wie ein Roboter", "Suena robótico mientras dictas", "Звучить як голос робота під час диктування"]),
    ("Sounds submerged while you dictate", ["Brzmi jak spod wody podczas dyktowania", "听写时听起来像在水下", "ディクテーション中、水中にいるような音になります", "Klingt beim Diktieren wie unter Wasser", "Suena como bajo el agua mientras dictas", "Звучить так, ніби ви під водою, під час диктування"]),
    ("Stays in memory", ["Tylko w pamięci", "仅保存在内存中", "メモリ内のみに保持", "Bleibt im Speicher", "Solo en memoria", "Залишається в памʼяті"]),
    ("Stop", ["Zatrzymaj", "停止", "停止", "Stopp", "Detener", "Зупинити"]),
    ("Stop recording", ["Zatrzymaj nagrywanie", "停止录制", "録音を停止", "Aufnahme stoppen", "Detener grabación", "Зупинити запис"]),
    ("Streaming 48 kHz audio to {}", ["Strumieniowanie 48 kHz do: {}", "正在向 {} 传输 48 kHz 音频", "{} へ 48 kHz 音声を配信中", "Streamt 48-kHz-Audio an {}", "Transmitiendo audio de 48 kHz a {}", "Передавання аудіо 48 кГц до {}"]),
    ("Try Hex Microphone and preview how callers hear you while you dictate", ["Wypróbuj Hex Microphone i posłuchaj, jak słyszą cię rozmówcy podczas dyktowania", "试用 Hex Microphone，预览听写时通话对方听到的效果", "Hex Microphone を試し、ディクテーション中に相手へどう聞こえるか確認します", "Teste Hex Microphone und höre, wie Anrufer dich beim Diktieren hören", "Prueba Hex Microphone y escucha cómo te oyen en la llamada mientras dictas", "Спробуйте Hex Microphone і перевірте, як вас чують співрозмовники під час диктування"]),
    ("Try it", ["Wypróbuj", "试听与录制", "試してみる", "Ausprobieren", "Pruébalo", "Спробувати"]),
    ("Turns your voice down to a whisper while you dictate", ["Ścisza twój głos do szeptu podczas dyktowania", "听写时将你的声音压低到耳语音量", "ディクテーション中、声をささやき声まで下げます", "Senkt deine Stimme beim Diktieren auf Flüstern ab", "Baja tu voz a un susurro mientras dictas", "Знижує гучність вашого голосу до шепоту під час диктування"]),
    ("Underwater", ["Pod wodą", "水下", "水中", "Unter Wasser", "Bajo el agua", "Під водою"]),
    ("Virtual microphone driver", ["Sterownik wirtualnego mikrofonu", "虚拟麦克风驱动", "仮想マイクドライバ", "Virtueller Mikrofontreiber", "Controlador de micrófono virtual", "Драйвер віртуального мікрофона"]),
    ("Walkie", ["Krótkofalówka", "对讲机", "トランシーバー", "Walkie-Talkie", "Walkie", "Рація"]),
    ("Wall", ["Tłumienie", "闷音滤波", "こもり具合", "Dämpfung", "Filtro", "Приглушення"]),
    ("You", ["Ty", "你", "あなた", "Du", "Tú", "Ви"]),
    ("Your 5s sample", ["Twoja próbka 5 s", "你的 5 秒录音", "あなたの5秒サンプル", "Deine 5-Sek.-Aufnahme", "Tu muestra de 5 s", "Ваш зразок на 5 с"]),
    ("Your voice", ["Twój głos", "你的声音", "自分の声", "Deine Stimme", "Tu voz", "Ваш голос"]),
    // The Rune Meetings pane and the OS-only Settings rows.
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

    /// Every source that translates, on every OS.
    const SOURCES: &[&str] = &[
        include_str!("opencode_catalog.rs"),
        include_str!("rune_ui.rs"),
        include_str!("rune_ui/activity.rs"),
        include_str!("rune_ui/explainer.rs"),
        include_str!("rune_ui/fun.rs"),
        include_str!("rune_ui/general.rs"),
        include_str!("rune_ui/history.rs"),
        include_str!("rune_ui/hud_lab.rs"),
        include_str!("rune_ui/labs.rs"),
        include_str!("rune_ui/linux_host.rs"),
        include_str!("rune_ui/list_detail.rs"),
        include_str!("rune_ui/lists.rs"),
        include_str!("rune_ui/macos_host.rs"),
        include_str!("rune_ui/meeting_capsules.rs"),
        include_str!("rune_ui/meetings.rs"),
        include_str!("rune_ui/models.rs"),
        include_str!("rune_ui/modes.rs"),
        include_str!("rune_ui/onboarding.rs"),
        include_str!("rune_ui/output.rs"),
        include_str!("rune_ui/picker.rs"),
        include_str!("rune_ui/platform_settings.rs"),
        #[cfg(hex_capture)]
        include_str!("rune_ui/scrawl.rs"),
        include_str!("rune_ui/settings.rs"),
        include_str!("rune_ui/sound_sets.rs"),
        include_str!("rune_ui/vocabulary.rs"),
        include_str!("rune_ui/windows_host.rs"),
        include_str!("tray_menu.rs"),
        include_str!("rune_ui/meetings_host.rs"),
        include_str!("windows_app.rs"),
        include_str!("windows_rune.rs"),
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

    /// The sound picker translates each set's name and character and each
    /// cue's name, which are not literals at the call.
    #[test]
    fn every_sound_set_string_has_every_language() {
        use crate::feedback::{Cue, SoundSet};
        let table: std::collections::HashMap<_, _> = TRANSLATIONS.iter().copied().collect();
        let strings = SoundSet::ALL
            .into_iter()
            .flat_map(|set| [set.name(), set.character()])
            .chain(Cue::ALL.into_iter().map(Cue::name));
        for english in strings {
            let translations = table
                .get(english)
                .unwrap_or_else(|| panic!("{english:?} has no translations"));
            assert!(
                translations
                    .iter()
                    .all(|translation| !translation.is_empty())
            );
        }
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
