import Foundation

/// Autocorrect's built-in word lists (`AutocorrectEngine`).
extension AutocorrectEngine {
    /// Valid words where an apostrophe form exists, offered as a suggestion without clobbering the word.
    static let suggestOnlyContractions: [String: String] = [
        "ill": "I'll",
        "id": "I'd",
        "were": "we're",
        "wed": "we'd",
        "well": "we'll",
        "its": "it's",
        "lets": "let's",
        "hell": "he'll",
        "shell": "she'll",
    ]

    /// Words a period after them makes an abbreviation ("approx.", "etc."): a period never
    /// corrects them.
    static let abbreviations: Set<String> = [
        "approx", "appt", "apt", "assn", "asst", "ave", "blvd", "cf", "co", "corp", "dept", "dr",
        "eg", "est", "etc", "fig", "govt", "ie", "inc", "jr", "lb", "lbs", "ltd", "misc", "mr",
        "mrs", "ms", "mt", "no", "nr", "oz", "pp", "prof", "pt", "rd", "sr", "st", "vol", "vs", "wk", "yr",
    ]

    /// Common chat & engineering shorthand that cautious autocorrect never rewrites.
    static let shorthandAllowlist: Set<String> = [
        "ok", "okay", "ty", "thx", "pls", "plz", "tmrw", "tbh", "idk", "imo", "imho",
        "lol", "lmao", "omg", "brb", "btw", "afaik", "fyi", "nvm", "rn", "irl", "jk",
        "np", "yw", "smh", "ikr", "ngl", "fr", "gg", "dm", "pr", "prs", "ci", "ui",
        "ux", "ios", "macos", "api", "apis", "url", "urls", "json", "sql", "ssh", "cli",
        "gpu", "cpu", "ram", "sdk", "hex", "wisp", "seer", "rune", "kit", "gonna",
        "wanna", "gotta", "kinda", "sorta", "yeah", "yep", "nope", "nah", "hey", "yo",
    ]

    /// ~125 unambiguous missing-apostrophe contractions and high-frequency QWERTY transpositions.
    static let unambiguousFixes: [String: String] = [
        // Pronoun & missing-apostrophe contractions
        "i": "I",
        "im": "I'm",
        "ive": "I've",
        "dont": "don't",
        "cant": "can't",
        "wont": "won't",
        "didnt": "didn't",
        "doesnt": "doesn't",
        "isnt": "isn't",
        "arent": "aren't",
        "wasnt": "wasn't",
        "werent": "weren't",
        "hasnt": "hasn't",
        "havent": "haven't",
        "hadnt": "hadn't",
        "wouldnt": "wouldn't",
        "couldnt": "couldn't",
        "shouldnt": "shouldn't",
        "mustnt": "mustn't",
        "neednt": "needn't",
        "youre": "you're",
        "youve": "you've",
        "youd": "you'd",
        "youll": "you'll",
        "theyre": "they're",
        "theyve": "they've",
        "theyd": "they'd",
        "theyll": "they'll",
        "weve": "we've",
        "shes": "she's",
        "hes": "he's",
        "itll": "it'll",
        "thats": "that's",
        "theres": "there's",
        "heres": "here's",
        "wheres": "where's",
        "whos": "who's",
        "whats": "what's",
        "hows": "how's",
        "whens": "when's",
        "whys": "why's",
        "aint": "ain't",
        "yall": "y'all",
        "oclock": "o'clock",
        "maam": "ma'am",
        "cmon": "c'mon",
        "wouldve": "would've",
        "couldve": "could've",
        "shouldve": "should've",
        "mightve": "might've",
        "mustve": "must've",
        // High-frequency QWERTY transpositions & fat-finger typos
        "teh": "the",
        "hte": "the",
        "taht": "that",
        "thta": "that",
        "adn": "and",
        "nad": "and",
        "wiht": "with",
        "iwth": "with",
        "waht": "what",
        "hwat": "what",
        "wich": "which",
        "whcih": "which",
        "thier": "their",
        "theri": "their",
        "recieve": "receive",
        "definately": "definitely",
        "seperate": "separate",
        "occured": "occurred",
        "untill": "until",
        "abotu": "about",
        "baout": "about",
        "becuase": "because",
        "beacuse": "because",
        "jsut": "just",
        "jstu": "just",
        "knwo": "know",
        "konw": "know",
        "tiem": "time",
        "tmie": "time",
        "shoudl": "should",
        "hsould": "should",
        "woudl": "would",
        "owuld": "would",
        "coudl": "could",
        "oculd": "could",
        "ahve": "have",
        "hvae": "have",
        "maek": "make",
        "mkae": "make",
        "thign": "thing",
        "hting": "thing",
        "peopel": "people",
        "poeple": "people",
        "workign": "working",
        "goign": "going",
        "doign": "doing",
        "runnign": "running",
        "comign": "coming",
        "thnaks": "thanks",
        "htanks": "thanks",
        "plase": "please",
        "pleaes": "please",
        "yuor": "your",
        "yoru": "your",
        "yuo": "you",
        "oyu": "you",
        "fomr": "from",
        "verry": "very",
        "evrey": "every",
        "realyl": "really",
        "realy": "really",
        "actualyl": "actually",
        "probalby": "probably",
        "tomorow": "tomorrow",
        "tommorow": "tomorrow",
        "tonihgt": "tonight",
        "tongiht": "tonight",
        "mesasge": "message",
        "messgae": "message",
        "keybaord": "keyboard",
        "keboard": "keyboard",
        "iphnoe": "iPhone",
        "ipohne": "iPhone",
    ]
}

extension AutocorrectEngine {
    /// French's elided words, which end in an apostrophe before the next word ("l'homme",
    /// "qu'il", "jusqu'ici"), commonest first.
    static let frenchElisions = ["l", "d", "j", "qu", "n", "c", "s", "m", "t", "jusqu", "lorsqu", "puisqu", "quoiqu", "quelqu", "presqu"]

    /// A French word with an elision and a slip on either side of its apostrophe: the word after
    /// it is corrected as a word of its own ("l'hpmme" -> "l'homme"), and a head that is no
    /// elision becomes the commonest one a key away ("k'homme" -> "l'homme", "qi'il" ->
    /// "qu'il"). Nil outside French, when neither side changes, for a word the dictionary knows
    /// whole ("aujourd'hui"), and for an apostrophe without a letter on both sides ("l'").
    func frenchElisionFix(for token: String) -> String? {
        guard [.french, .frenchCanadian, .frenchSwiss].contains(language),
              token.filter({ $0 == "'" }).count == 1, let mark = token.firstIndex(of: "'") else { return nil }
        let head = String(token[..<mark])
        let tail = String(token[token.index(after: mark)...])
        guard !head.isEmpty, !tail.isEmpty, head.allSatisfy(\.isLetter), tail.allSatisfy(\.isLetter),
              !checker.isWordValid(token) else { return nil }
        var fixedHead = head
        let lowerHead = head.lowercased()
        if !Self.frenchElisions.contains(lowerHead) {
            var best: (word: String, cost: Double)?
            for word in Self.frenchElisions {
                guard let cost = Self.singleEditKeyboardCost(from: lowerHead, to: word, language: layoutLanguage, letters: layoutLetters),
                      cost > 0, cost <= 0.65 else { continue }
                if best == nil || cost < best!.cost { best = (word, cost) }
            }
            guard let best else { return nil }
            fixedHead = Self.matchCase(of: head, onto: best.word)
        }
        var fixedTail = tail
        if let word = ownReplacement(for: tail), word.allSatisfy(\.isLetter) {
            fixedTail = word
        }
        guard fixedHead != head || fixedTail != tail else { return nil }
        return fixedHead + "'" + fixedTail
    }
}
