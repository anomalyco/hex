import Foundation
@testable import HEX

/// The parity dictionaries' data that isn't a typing model: a short list of common words for
/// each language without a model, names known only capitalised, extra words (abbreviations
/// iOS's French and German dictionaries take), and the inputs the generator draws from.
enum ACParityWords {
    static let lists: [String: [String]] = [
        "fr": ["je", "le", "la", "de", "et", "est", "que", "pas", "pour", "dans", "une", "un", "les", "des", "il", "elle", "nous",
               "vous", "mais", "merci", "bonjour", "oui", "non", "avec", "sur", "très", "bien", "chat", "chien", "maison", "pense",
               "dont", "ça", "déjà", "été", "être", "à", "où", "même", "voilà", "après", "demain", "aujourd'hui", "français",
               "the", "you", "your", "not", "please", "mercredi", "mercure", "thé", "café", "garçon", "noël", "dk", "dl", "tkm",
               "ro", "im", "est-ce", "parce", "quand", "comme", "tout", "faire", "aller", "voir", "venir", "temps", "jour", "ami"],
        "it": ["il", "la", "di", "che", "e", "è", "un", "una", "per", "non", "sono", "mi", "ti", "ci", "si", "no", "io", "tu",
               "perché", "più", "già", "può", "città", "caffè", "lunedì", "venerdì", "grazie", "ciao", "bene", "molto", "casa",
               "anche", "come", "dove", "quando", "sempre", "oggi", "domani", "c'è", "ce", "così", "cosi", "ma", "da", "ne", "le",
               "lo", "ha", "ho", "al", "su", "se", "fare", "andare", "tempo", "giorno", "amico", "libro", "verità", "perche"],
        "pt": ["eu", "o", "a", "de", "que", "e", "do", "da", "em", "um", "uma", "para", "não", "você", "está", "até", "amanhã",
               "obrigado", "sim", "muito", "bem", "casa", "com", "mas", "mais", "como", "quando", "onde", "hoje", "agora",
               "também", "já", "só", "é", "são", "esta", "ate", "voce", "nao", "mãe", "pão", "coração", "ação", "fazer",
               "ir", "ver", "tempo", "dia", "amigo", "livro", "the", "you"],
        "pl": ["nie", "i", "w", "się", "na", "to", "ja", "że", "jest", "tak", "jak", "ale", "co", "do", "mały", "żółw",
               "dzień", "dobry", "dziękuję", "proszę", "być", "mieć", "wiem", "teraz", "jutro", "dzisiaj", "książka", "łódź",
               "źle", "gęś", "ćma", "jest", "tak", "pan", "pani", "dom", "kot", "pies"],
        "cs": ["a", "se", "je", "v", "na", "to", "že", "ale", "já", "ty", "český", "příliš", "žluťoučký", "kůň", "děkuji",
               "prosím", "dobrý", "den", "zítra", "dnes", "jsem", "být", "mít", "vím", "teď", "kniha", "pes", "kočka", "dům"],
        "ro": ["și", "dar", "că", "de", "în", "nu", "să", "la", "eu", "am", "este", "mulțumesc", "bună", "ziua", "mâine",
               "astăzi", "carte", "casă", "câine", "pisică", "fată", "băiat", "țară", "școală"],
        "nb": ["og", "i", "det", "er", "på", "som", "jeg", "vi", "men", "en", "et", "til", "av", "for", "med", "ikke", "har",
               "takk", "hei", "god", "dag", "blåbær", "smørbrød", "kjærlighet", "øl", "å", "være", "gå", "se", "hus", "bil"],
        "ru": ["и", "в", "не", "на", "я", "что", "это", "мы", "но", "он", "она", "они", "как", "так", "да", "нет", "спасибо",
               "привет", "хорошо", "ещё", "всё", "ёлка", "дом", "кот", "собака", "книга", "сегодня", "завтра", "быть"],
        "uk": ["і", "в", "не", "на", "що", "я", "це", "ми", "але", "він", "вона", "так", "ні", "дякую", "привіт", "добре",
               "їжак", "єдиний", "ґанок", "м'ята", "дім", "кіт", "книга", "сьогодні", "завтра"],
        "el": ["και", "να", "το", "θα", "δεν", "η", "ο", "που", "αλλά", "είναι", "καλημέρα", "ευχαριστώ", "ναι", "όχι",
               "λόγος", "αϋπνία", "σπίτι", "βιβλίο", "σήμερα", "αύριο", "καλά", "τι", "κάνεις", "πολύ", "εγώ", "εσύ"],
    ]

    static let names: [String: [String]] = [
        "en": ["Mary", "Paris", "Taylor", "Kate", "Thompson", "Zuri", "Monday", "London", "Zuzanna", "Tchaikovsky", "Kaitlyn"],
        "es": ["María", "Madrid", "Lunes", "García", "Sevilla"],
        "de": ["Haus", "Abbas", "Berlin", "Müller", "Leben", "Abend"],
        "fr": ["Marie", "Lyon", "Zuri", "Dupont"],
        "it": ["Roma", "Giulia"],
        "pt": ["Lisboa", "João"],
        "pl": ["Kraków", "Wałęsa"],
        "cs": ["Praha"],
        "nb": ["Oslo"],
        "ru": ["Москва"],
        "uk": ["Київ"],
        "el": ["Αθήνα"],
    ]

    static let extras: [String: [String]] = [
        "en": ["dont", "im", "tkm", "dk", "ok", "iPhone", "e-mail", "up", "to"],
        "es": ["im", "the", "you", "please"],
        "de": ["dont", "im", "dk", "dl", "tkm", "the", "you", "your", "not", "please", "est"],
    ]

    /// The text replacements and terms a case's lexicon draws from (some from the iOS tests).
    static let lexicon: [(String, String)] = [
        ("omw", "On my way!"), ("addr", "123 Market St"), ("Kit", "Kit"), ("brb", "be right back"),
        ("myem", "me@example.com"), ("ad2", "2 Avenue Road"), ("s.o", "someone"), ("mip", "My iPhone"),
        ("ty", "thank you"), ("tchao", "Tchao"), (" idk ", " I don't know "), ("Kate", "Kate"), ("Bell", "Bell"),
        ("Will", "Will"), ("Thompson", "Thompson"), ("Zuzanna", "Zuzanna"), ("hexkey", "Hexkey"), ("gh", "GitHub"),
        ("fuck", "fuck"), ("Saoirse", "Saoirse"), ("wrk", "work"), ("ilu", "I love you"), ("ttyl", "talk to you later"),
        ("teh", "teh"), ("", "x"), ("x", ""), ("smh", "shaking my head"), ("Mip", "my iPhone"), ("cafe\u{301}", "Café"),
    ]

    static let vocabulary: [String] = [
        "GitHub", "kubectl", "Swift", "OpenCode", "Hex keyboard", "macOS", "Kubernetes", "iOS", "Parakeet", "Tatoeba",
    ]

    /// Strings with the Unicode Swift and Rust must agree on.
    static let tricky: [String] = [
        "cafe\u{301}", "caf\u{E9}", "\u{130}stanbul", "stra\u{DF}e", "STRASSE", "\u{1C5}ungla", "\u{3BB}\u{3CC}\u{3B3}\u{3BF}\u{3C2}",
        "\u{39B}\u{39F}\u{393}\u{39F}\u{3A3}", "\u{212A}elvin", "\u{FB01}x", "\u{1F44D}\u{1F3FD}", "e\u{301}\u{301}", "a\r\nb",
        "\u{3B9}\u{3C3}\u{37E} ", "it\u{2019}s", "I\u{2019}m", "n\u{303}o", "x\u{200D}y", "\u{1F1EB}\u{1F1F7}", "\u{CA}tre",
        "\u{131}", "ǈ", "\u{1E9E}", "na\u{EF}ve", "Ł\u{F3}d\u{17A}", "\u{2126}", "\u{212B}", "a\u{30A}", "ʼ", "м'ята",
    ]
}
