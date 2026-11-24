//! Labs › Dictation style's strings, kept beside the main tables so those
//! stay under the source cap. Columns: Polish, Chinese (Simplified),
//! Japanese, German, Spanish, Ukrainian.

/// Columns: Polish, Chinese (Simplified), Japanese, German, Spanish,
/// Ukrainian.
#[rustfmt::skip]
pub(super) const TRANSLATIONS: &[(&str, [&str; 6])] = &[
    ("Dictation style", ["Styl dyktowania", "听写风格", "音声入力のスタイル", "Diktierstil", "Estilo de dictado", "Стиль диктування"]),
    (
        "Format every dictation as Polished, Casual, Notes, or Verbatim, like the iPhone app, without an AI model",
        [
            "Formatuj każde dyktowanie jako Dopracowane, Swobodne, Notatki lub Dosłowne, jak w aplikacji na iPhone'a, bez modelu AI",
            "像 iPhone 应用一样，把每次听写整理成润色、随意、笔记或原文，不需要 AI 模型",
            "iPhone アプリのように、各音声入力を整文・カジュアル・メモ・そのままのいずれかで整えます。AI モデルは不要です",
            "Formatiere jedes Diktat als Ausgefeilt, Locker, Notizen oder Wörtlich, wie in der iPhone-App, ohne KI-Modell",
            "Da forma a cada dictado como Pulido, Informal, Notas o Literal, como en la app del iPhone, sin un modelo de IA",
            "Оформлюйте кожне диктування як Вигладжене, Невимушене, Нотатки або Дослівне, як у застосунку для iPhone, без моделі ШІ",
        ],
    ),
    ("Polished", ["Dopracowane", "润色", "整文", "Ausgefeilt", "Pulido", "Вигладжене"]),
    ("Casual", ["Swobodne", "随意", "カジュアル", "Locker", "Informal", "Невимушене"]),
    ("Notes", ["Notatki", "笔记", "メモ", "Notizen", "Notas", "Нотатки"]),
    ("Verbatim", ["Dosłowne", "原文", "そのまま", "Wörtlich", "Literal", "Дослівне"]),
    ("Full sentences, fillers trimmed", ["Pełne zdania, bez wtrętów", "完整句子，去掉口头语", "完全な文、つなぎ言葉は削除", "Ganze Sätze, Füllwörter entfernt", "Frases completas, sin muletillas", "Повні речення, без слів-заповнювачів"]),
    ("Texting: lowercase start, no final period", ["Jak SMS: mała litera na początku, bez kropki na końcu", "像发短信：开头小写，结尾不加句号", "メッセージ風：文頭は小文字、最後にピリオドなし", "Wie eine Nachricht: klein am Anfang, kein Punkt am Ende", "Como un mensaje: inicio en minúscula, sin punto final", "Як SMS: рядок з малої літери, без крапки в кінці"]),
    ("Short lines, a list for several items", ["Krótkie wiersze, lista przy kilku pozycjach", "简短行，多项时列成清单", "短い行、複数項目はリストに", "Kurze Zeilen, eine Liste bei mehreren Punkten", "Líneas cortas, una lista si hay varios elementos", "Короткі рядки, список для кількох пунктів"]),
    ("Exact words from the model", ["Dokładne słowa z modelu", "模型说出的原话", "モデルが書いたとおりの言葉", "Genaue Wörter des Modells", "Las palabras exactas del modelo", "Точні слова з моделі"]),
    ("Style", ["Styl", "风格", "スタイル", "Stil", "Estilo", "Стиль"]),
    ("After the rewrite", ["Po przepisaniu", "改写之后", "書き換えの後", "Nach dem Umschreiben", "Después de la reescritura", "Після переписування"]),
];

#[rustfmt::skip]
pub(super) const FRENCH: &[(&str, &str)] = &[
    ("Dictation style", "Style de dictée"),
    (
        "Format every dictation as Polished, Casual, Notes, or Verbatim, like the iPhone app, without an AI model",
        "Mettez en forme chaque dictée en Soigné, Décontracté, Notes ou Mot pour mot, comme dans l’app iPhone, sans modèle d’IA",
    ),
    ("Polished", "Soigné"),
    ("Casual", "Décontracté"),
    ("Notes", "Notes"),
    ("Verbatim", "Mot pour mot"),
    ("Full sentences, fillers trimmed", "Phrases complètes, hésitations supprimées"),
    ("Texting: lowercase start, no final period", "Comme un SMS : minuscule au début, pas de point final"),
    ("Short lines, a list for several items", "Lignes courtes, une liste pour plusieurs éléments"),
    ("Exact words from the model", "Les mots exacts du modèle"),
    ("Style", "Style"),
    ("After the rewrite", "Après la réécriture"),
];

#[rustfmt::skip]
pub(super) const PORTUGUESE: &[(&str, &str)] = &[
    ("Dictation style", "Estilo do ditado"),
    (
        "Format every dictation as Polished, Casual, Notes, or Verbatim, like the iPhone app, without an AI model",
        "Formate cada ditado como Polido, Casual, Notas ou Literal, como no app do iPhone, sem um modelo de IA",
    ),
    ("Polished", "Polido"),
    ("Casual", "Casual"),
    ("Notes", "Notas"),
    ("Verbatim", "Literal"),
    ("Full sentences, fillers trimmed", "Frases completas, sem hesitações"),
    ("Texting: lowercase start, no final period", "Como uma mensagem: início em minúscula, sem ponto final"),
    ("Short lines, a list for several items", "Linhas curtas, uma lista para vários itens"),
    ("Exact words from the model", "As palavras exatas do modelo"),
    ("Style", "Estilo"),
    ("After the rewrite", "Depois da reescrita"),
];

#[rustfmt::skip]
pub(super) const RUSSIAN: &[(&str, &str)] = &[
    ("Dictation style", "Стиль диктовки"),
    (
        "Format every dictation as Polished, Casual, Notes, or Verbatim, like the iPhone app, without an AI model",
        "Оформляйте каждую диктовку как Отточенная, Свободная, Заметки или Дословная, как в приложении для iPhone, без модели ИИ",
    ),
    ("Polished", "Отточенная"),
    ("Casual", "Свободная"),
    ("Notes", "Заметки"),
    ("Verbatim", "Дословная"),
    ("Full sentences, fillers trimmed", "Полные предложения, без слов-заполнителей"),
    ("Texting: lowercase start, no final period", "Как сообщение: со строчной буквы, без точки в конце"),
    ("Short lines, a list for several items", "Короткие строки, список при нескольких пунктах"),
    ("Exact words from the model", "Точные слова модели"),
    ("Style", "Стиль"),
    ("After the rewrite", "После переписывания"),
];

/// Hungarian, for the strings above.
#[rustfmt::skip]
pub(super) const HUNGARIAN: &[(&str, &str)] = &[
    ("Dictation style", "Diktálási stílus"),
    (
        "Format every dictation as Polished, Casual, Notes, or Verbatim, like the iPhone app, without an AI model",
        "Formázza minden diktálását Csiszolt, Kötetlen, Jegyzet vagy Szó szerinti stílusban, mint az iPhone-alkalmazásban, AI-modell nélkül",
    ),
    ("Polished", "Csiszolt"),
    ("Casual", "Kötetlen"),
    ("Notes", "Jegyzet"),
    ("Verbatim", "Szó szerinti"),
    ("Full sentences, fillers trimmed", "Teljes mondatok, töltelékszavak nélkül"),
    ("Texting: lowercase start, no final period", "Üzenetként: kisbetűs kezdés, záró pont nélkül"),
    ("Short lines, a list for several items", "Rövid sorok, lista több elem esetén"),
    ("Exact words from the model", "A modell pontos szavai"),
    ("Style", "Stílus"),
    ("After the rewrite", "Az újraírás után"),
];
