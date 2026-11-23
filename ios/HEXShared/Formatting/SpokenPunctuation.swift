import Foundation

/// Settings › Dictation › Spoken punctuation, in the shared App Group so in-keyboard dictation
/// reads it too. On by default.
nonisolated struct DictationPunctuationSettings {
    static let enabledKey = "settings.dictationSpokenPunctuation"

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Punctuation said by name ("comma", "question mark", "open quote") is written as the mark
    /// (`SpokenPunctuation`), in English, Spanish, French, German, Italian and Portuguese.
    var isEnabled: Bool {
        get { defaults?.object(forKey: Self.enabledKey) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }
}

/// Punctuation said by name written as the mark, with string rules and no model, in English,
/// Spanish, French, German, Italian and Portuguese; other languages are left as they are. Runs
/// inside a style, after its fillers and Format lists (`SpokenLists`, so "dash … dash …" stays a
/// list and "comma, new line" keeps its comma) and before the style's own punctuation
/// (`TranscriptFormatter.format(_:style:language:lists:punctuation:endsWithPeriod:)`), so each
/// style capitalizes and ends sentences its own way; French spacing comes after (`FrenchSpacing`).
///
/// Apple's `DictationTranscriber` already writes the marks (macOS 27.2.1, `say` voices: "That is
/// amazing!", "Here is the plan: we leave at noon", "(after lunch)", "#Blessed"), so its text has no
/// names left. `SpeechTranscriber` writes only some ("comma", "question mark", French "point" and
/// "virgule", German "Komma" and "Fragezeichen") and leaves the rest as words, often with commas
/// around them ("I came semicolon, I saw", "open Paren after lunch close, Paren"); Parakeet writes
/// every name as words ("noon, comma, see you soon, period", "tonight question mark?"). A mark the
/// model already wrote beside a name is replaced, never doubled.
///
/// - Marks attach to the word before them ("noon, see you"), a dash is spaced ("good – really
///   good"), "hyphen", "apostrophe" and "arroba" ("arobase", "chiocciola", "At-Zeichen") join
///   their words ("well-known", "John's", "ana@gmail"; English "at sign" is
///   `SpokenCleanupRules.spokenSymbols`'s), "hashtag" and "dollar sign" go on the next word
///   ("#blessed"), and a word after ".", "?" or "!" starts with a capital.
/// - Quotes and brackets open and close: “ ” (« » in French, „ “ in German), ( ). A closing name
///   counts only while one is open, and a bare "quote" ("comillas", "guillemets",
///   "Anführungszeichen", "virgolette", "aspas") opens only with a closing name later, never in
///   "quote unquote".
/// - Guards keep prose. A name is no command right after a determiner, a preposition, a number
///   or a word that makes it a noun ("a period of time", "the comma is", "a dash of salt", "his
///   colon", "trial period", "Oxford comma", "ein wichtiger Punkt"), right before a word that
///   continues the noun ("question mark tattoo", "colon cancer", "period of", "punto de vista"),
///   or inside a compound ("comma-separated"). The names that are everyday nouns ("period",
///   "point", "Punkt", "punto", "ponto", "question mark", "dash", "smiley", "dos puntos") need
///   command-like placement as well: the end of the line, a mark after them, or a next word that
///   starts a clause ("I", "see", "thanks", "nos", "à", a capital outside German). A colon needs
///   words after it.
nonisolated enum SpokenPunctuation {
    static func format(_ text: String, language: String?) -> String {
        guard let table = Table.forLanguage(language), !text.isEmpty else { return text }
        var writer = Writer(tokens: tokenize(text), table: table)
        return writer.write()
    }

    // MARK: Words

    fileprivate struct Token {
        let text: String
        let isWord: Bool
        /// Lowercased, accents folded, curly apostrophes straight: "D’Interrogation" reads
        /// "d'interrogation".
        let folded: String
    }

    fileprivate static func fold(_ text: String) -> String {
        text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)
            .replacingOccurrences(of: "’", with: "'")
    }

    /// Alternating words (letters, digits, apostrophes) and the separators between them.
    private static func tokenize(_ text: String) -> [Token] {
        var tokens: [Token] = []
        var current = ""
        var currentIsWord: Bool?
        func flush() {
            guard let isWord = currentIsWord else { return }
            tokens.append(Token(text: current, isWord: isWord, folded: isWord ? fold(current) : current))
        }
        for character in text {
            let isWord = character.isLetter || character.isNumber || character == "'" || character == "’"
            if currentIsWord == isWord {
                current.append(character)
            } else {
                flush()
                current = String(character)
                currentIsWord = isWord
            }
        }
        flush()
        return tokens
    }

    /// Marks a speech model puts around a spoken name: the name's mark replaces them.
    private static let pauseMarks = ",.;:!?…"
    private static let openers: Set<Character> = ["(", "[", "“", "«", "„", "¿", "¡", "\"", "‘"]
    private static let closers: Set<Character> = [")", "]", "”", "»", "“", "\"", "’"]
    /// Endings after "apostrophe": "John apostrophe S" is "John's".
    private static let contractions: Set<String> = ["s", "t", "d", "m", "ll", "re", "ve"]

    // MARK: Writing

    private struct Writer {
        let tokens: [Token]
        let table: Table
        var out = ""
        var capitalizeNext = false
        var lowercaseNext = false

        init(tokens: [Token], table: Table) {
            self.tokens = tokens
            self.table = table
        }

        mutating func write() -> String {
            var index = 0
            while index < tokens.count {
                if tokens[index].isWord, let (command, end) = command(at: index) {
                    if allows(command, from: index, to: end) {
                        index = apply(command, end: end)
                    } else {
                        // A name kept as said stays whole: "a dos puntos" never loses its "puntos".
                        for token in tokens[index..<end] {
                            emit(token)
                        }
                        index = end
                    }
                    continue
                }
                emit(tokens[index])
                index += 1
            }
            return out
        }

        private mutating func emit(_ token: Token) {
            guard token.isWord else {
                out += token.text
                return
            }
            if lowercaseNext, contractions.contains(token.folded) {
                out += token.text.lowercased()
            } else if capitalizeNext {
                out += capitalized(token.text)
            } else {
                out += token.text
            }
            capitalizeNext = false
            lowercaseNext = false
        }

        private func capitalized(_ word: String) -> String {
            guard let first = word.first, first.isLowercase,
                  !word.dropFirst().contains(where: \.isUppercase) else { return word }
            return first.uppercased() + word.dropFirst()
        }

        /// The command whose words start at `index`, longest first, and where its words end.
        func command(at index: Int) -> (Command, Int)? {
            for command in table.commands {
                if let end = phraseEnd(command.words, at: index) { return (command, end) }
            }
            return nil
        }

        /// Where `words` said one after another start at `index` end. Between them only spaces,
        /// a hyphen ("point-virgule") or one comma a model heard as a pause ("close, Paren").
        private func phraseEnd(_ words: [String], at index: Int) -> Int? {
            var position = index
            for (number, word) in words.enumerated() {
                if number > 0 {
                    guard position < tokens.count, !tokens[position].isWord else { return nil }
                    let separator = tokens[position].text
                    guard separator.count <= 3, separator.allSatisfy({ " -,".contains($0) }),
                          separator.filter({ $0 == "," }).count <= 1 else { return nil }
                    position += 1
                }
                guard position < tokens.count, tokens[position].isWord, tokens[position].folded == word else { return nil }
                position += 1
            }
            return position
        }

        private func count(_ character: Character) -> Int {
            out.reduce(0) { $1 == character ? $0 + 1 : $0 }
        }

        private func isOpen(_ pair: Pair) -> Bool {
            switch pair {
            case .paren:
                return count("(") > count(")")
            case .quote:
                return count(table.quotes.open) > count(table.quotes.close) || count("\"") % 2 == 1
            }
        }

        /// Whether a closing quote name follows `end`, with words between.
        private func closingQuote(after end: Int, toggles: Bool) -> Bool {
            var index = end
            var wordsBetween = false
            while index < tokens.count {
                if tokens[index].text.contains("\n") { return false }
                if tokens[index].isWord, let (found, commandEnd) = command(at: index) {
                    switch found.mark {
                    case .close(.quote): return wordsBetween
                    case .toggleQuote: return toggles && wordsBetween
                    default: index = commandEnd
                    }
                    wordsBetween = true
                    continue
                }
                wordsBetween = wordsBetween || tokens[index].isWord
                index += 1
            }
            return false
        }

        private func allows(_ command: Command, from index: Int, to end: Int) -> Bool {
            let before = index >= 1 ? tokens[index - 1].text : ""
            let after = end < tokens.count ? tokens[end].text : ""
            let previous: Token? = index >= 2 && !before.contains("\n") ? tokens[index - 2] : nil
            let next: Token? = end + 1 < tokens.count && !after.contains("\n") ? tokens[end + 1] : nil
            let nextDirect = next != nil && after.allSatisfy { $0 == " " || $0 == "\t" }
            let mark = command.mark
            let family = command.family

            // A compound: "comma-separated", "pre-period", "and/or".
            if let first = after.first, first == "-" || first == "/" { return false }
            if family != .hyphen, let last = before.last, last == "-" || last == "/" { return false }

            // A name: "I'll text Dash later", "George Smiley".
            if table.capitalizesSentences, tokens[index].text.first?.isUppercase == true, let previous {
                if family == .dash, !before.contains(where: { ".!?".contains($0) }) { return false }
                if family == .smiley, previous.text.first?.isUppercase == true { return false }
            }

            var opens = false
            var closes = false
            switch mark {
            case .open, .prefix:
                opens = true
            case .toggleQuote(let toggles):
                opens = !isOpen(.quote)
                closes = !opens
                if opens {
                    guard next != nil, closingQuote(after: end, toggles: toggles) else { return false }
                }
            case .close(let pair):
                guard isOpen(pair) else { return false }
                closes = true
            default:
                break
            }

            if !opens {
                // Attached to something said before it on the line; a quote or bracket that is
                // open closes after any word ("hello, their close quote").
                guard let previous else { return false }
                if !closes, isNounPhrase(before: previous, at: index, family: family) { return false }
            } else if case .prefix = mark, let previous, isNounPhrase(before: previous, at: index, family: family) {
                return false
            }

            if case .open = mark {
                // "The open quote was missing."
                if let previous, table.blockers.contains(previous.folded) { return false }
                if nextDirect, let next, table.followers.contains(next.folded) { return false }
                return next != nil
            }
            if case .toggleQuote = mark, opens {
                return true
            }

            // A decimal or a time: "two comma five", "3 colon 30".
            if [.comma, .period, .colon].contains(family), let previous, nextDirect, let next,
               isNumber(previous), isNumber(next) {
                return false
            }

            if nextDirect, let next {
                if table.followers.contains(next.folded) || table.familyFollowers[family]?.contains(next.folded) == true
                    || table.followerPrefixes.contains(where: next.folded.hasPrefix) {
                    return false
                }
            }

            switch mark {
            case .join:
                let joinable = family == .hyphen ? " \t-" : " \t"
                guard before.allSatisfy({ joinable.contains($0) }), nextDirect else { return false }
            case .prefix:
                guard nextDirect else { return false }
            default:
                break
            }

            if command.needsFollowing, next == nil { return false }
            if command.strict {
                let atLineEnd = next == nil
                let pauseAfter = after.first(where: { $0 != " " && $0 != "\t" }).map {
                    pauseMarks.contains($0) || closers.contains($0)
                } ?? false
                let startsClause = nextDirect && next.map {
                    table.starters.contains($0.folded) || table.starterPrefixes.contains(where: $0.folded.hasPrefix)
                        || (table.capitalizesSentences && $0.text.first?.isUppercase == true)
                } ?? false
                let anotherCommand = next != nil && self.command(at: end + 1) != nil
                guard atLineEnd || pauseAfter || startsClause || anotherCommand else { return false }
            }
            return true
        }

        private func isNumber(_ token: Token) -> Bool {
            token.folded.allSatisfy(\.isNumber) || table.numbers.contains(token.folded)
        }

        /// Whether the word before a name makes the name a noun: "a period", "the comma",
        /// "trial period", "two comma five", "Sarah's colon", "ein wichtiger Punkt".
        private func isNounPhrase(before previous: Token, at index: Int, family: Family) -> Bool {
            let word = previous.folded
            if table.blockers.contains(word) || table.familyBlockers[family]?.contains(word) == true { return true }
            // An English or German sentence may end on a preposition or a particle ("Who is it
            // for?", "Ruf mich an."), a noun phrase not.
            if table.prepositions.contains(word),
               !(table.prepositionsEndSentences && [.question, .exclamation, .period].contains(family)) {
                return true
            }
            if table.possessives, word.hasSuffix("'s"), !table.starters.contains(word) { return true }
            // German nouns are capitalized: a lowercase word between an article and the name is
            // an adjective ("ein wichtiger Punkt"), not the end of a clause ("Ich komme morgen").
            if table.adjectivesBeforeNouns, previous.text.first?.isLowercase == true, index >= 4,
               tokens[index - 3].text.allSatisfy({ $0 == " " }), table.blockers.contains(tokens[index - 4].folded) {
                return true
            }
            return false
        }

        /// Writes the command's mark and returns where reading goes on.
        private mutating func apply(_ command: Command, end: Int) -> Int {
            let after = end < tokens.count ? tokens[end].text : ""
            let resume = end < tokens.count ? end + 1 : end
            let wordAfter = end + 1 < tokens.count
            func rest(dropping set: String) -> String {
                String(after.drop { set.contains($0) })
            }
            func spaced(_ rest: String) -> String {
                rest.isEmpty && wordAfter ? " " : rest
            }

            switch command.mark {
            case .attach(let mark):
                trimEnd(" \t" + pauseMarks)
                out += mark
                out += spaced(rest(dropping: " \t" + pauseMarks))
                capitalizeNext = ".?!".contains(mark)
            case .lineEnd(let mark, let breaks):
                trimEnd(" \t" + pauseMarks)
                out += mark + breaks
                out += rest(dropping: " \t\n" + pauseMarks)
                capitalizeNext = true
            case .open(let mark):
                openWith(mark)
                out += rest(dropping: " \t" + pauseMarks)
            case .close(let pair):
                closeWith(pair)
                out += spaced(after)
            case .toggleQuote:
                if isOpen(.quote) {
                    closeWith(.quote)
                    out += spaced(after)
                } else {
                    openWith(String(table.quotes.open))
                    out += rest(dropping: " \t" + pauseMarks)
                }
            case .join(let mark):
                trimEnd(command.family == .hyphen ? " \t-" : " \t")
                out += mark
                out += rest(dropping: " \t")
                lowercaseNext = command.family == .apostrophe
            case .prefix(let mark):
                openWith(mark)
                out += rest(dropping: " \t")
            case .spaced(let mark):
                trimEnd(" \t,;:")
                out += " " + mark
                out += spaced(rest(dropping: " \t,;:"))
            case .emoticon(let face):
                trimEnd(" \t,")
                out += " " + face
                out += spaced(rest(dropping: " \t,."))
            }
            return resume
        }

        private mutating func trimEnd(_ set: String) {
            while let last = out.last, set.contains(last) {
                out.removeLast()
            }
        }

        /// An opening mark with a space before it, none after.
        private mutating func openWith(_ mark: String) {
            trimEnd(" \t")
            if let last = out.last, !last.isNewline, !openers.contains(last) {
                out += " "
            }
            out += mark
        }

        /// A closing mark against the word before it; a straight quote closes a straight one.
        private mutating func closeWith(_ pair: Pair) {
            let straight = pair == .quote && count("\"") % 2 == 1
            trimEnd(" \t,;:")
            switch pair {
            case .paren: out += ")"
            case .quote: out.append(straight ? "\"" : table.quotes.close)
            }
        }
    }

    // MARK: Commands

    fileprivate enum Pair: Equatable {
        case paren
        case quote
    }

    fileprivate enum Mark {
        /// Against the word before: , . ? ! : ; …
        case attach(String)
        /// A mark, then line breaks: "punto y aparte".
        case lineEnd(String, String)
        /// Before the next word: ( ¿ ¡ “
        case open(String)
        case close(Pair)
        /// A bare "quote": opens, or closes the quote that is open. `toggles`: two of them make
        /// a pair ("Anführungszeichen … Anführungszeichen"); English needs "end quote".
        case toggleQuote(toggles: Bool)
        /// Between two words, no spaces: - ' @
        case join(String)
        /// On the next word: # $
        case prefix(String)
        /// With a space each side: –
        case spaced(String)
        case emoticon(String)
    }

    fileprivate enum Family: Hashable {
        case comma, period, question, exclamation, colon, semicolon, ellipsis, dash, hyphen
        case apostrophe, at, hashtag, dollar, quote, paren, smiley
    }

    fileprivate struct Command {
        let words: [String]
        let mark: Mark
        let family: Family
        /// Needs command-like placement: the line's end, a mark after it, or a clause start.
        let strict: Bool
        /// Needs words after it on the line.
        let needsFollowing: Bool
    }

    // MARK: Words per language

    private struct Table {
        let commands: [Command]
        /// Words before a name that make it a noun: articles, possessives, prepositions,
        /// numbers, adjectives.
        let blockers: Set<String>
        /// Prepositions, which block too.
        let prepositions: Set<String>
        /// Number words: "two comma five" is a decimal.
        let numbers: Set<String>
        /// Whether a sentence may end on one (English, German), so they don't block ".", "?", "!".
        let prepositionsEndSentences: Bool
        let familyBlockers: [Family: Set<String>]
        /// Words after a name that continue the noun: "is", "of", "de".
        let followers: Set<String>
        let familyFollowers: [Family: Set<String>]
        /// "d'eau" after "point".
        let followerPrefixes: [String]
        /// Words that start a clause after a name said at its end.
        let starters: Set<String>
        /// "j'arrive", "c'est".
        let starterPrefixes: [String]
        let quotes: (open: Character, close: Character)
        /// Whether a capital starts a sentence (not German, where every noun has one).
        let capitalizesSentences: Bool
        let adjectivesBeforeNouns: Bool
        /// English "Sarah's colon".
        let possessives: Bool

        init(
            commands: [(String, Mark, Family, Placement)],
            blockers: [String],
            numbers: [String],
            prepositions: [String],
            prepositionsEndSentences: Bool = false,
            familyBlockers: [Family: [String]] = [:],
            followers: [String],
            familyFollowers: [Family: [String]] = [:],
            followerPrefixes: [String] = [],
            starters: [String],
            starterPrefixes: [String] = [],
            quotes: (open: Character, close: Character),
            capitalizesSentences: Bool = true,
            adjectivesBeforeNouns: Bool = false,
            possessives: Bool = false
        ) {
            let fold = SpokenPunctuation.fold
            self.commands = commands
                .map { phrase, mark, family, placement in
                    Command(
                        words: fold(phrase).split(whereSeparator: { $0 == " " || $0 == "-" }).map(String.init),
                        mark: mark,
                        family: family,
                        strict: placement == .strict || placement == .strictFollowing,
                        needsFollowing: placement == .following || placement == .strictFollowing
                    )
                }
                .sorted { $0.words.count > $1.words.count }
            self.blockers = Set(blockers.map(fold))
            self.prepositions = Set(prepositions.map(fold))
            self.numbers = Set(numbers.map(fold))
            self.prepositionsEndSentences = prepositionsEndSentences
            self.familyBlockers = familyBlockers.mapValues { Set($0.map(fold)) }
            self.followers = Set(followers.map(fold))
            self.familyFollowers = familyFollowers.mapValues { Set($0.map(fold)) }
            self.followerPrefixes = followerPrefixes.map(fold)
            self.starters = Set(starters.map(fold))
            self.starterPrefixes = starterPrefixes.map(fold)
            self.quotes = quotes
            self.capitalizesSentences = capitalizesSentences
            self.adjectivesBeforeNouns = adjectivesBeforeNouns
            self.possessives = possessives
        }

        static func forLanguage(_ language: String?) -> Table? {
            let code = language.flatMap { Locale(identifier: $0).language.languageCode?.identifier } ?? "en"
            switch code {
            case "en": return english
            case "es": return spanish
            case "fr": return french
            case "de": return german
            case "it": return italian
            case "pt": return portuguese
            default: return nil
            }
        }
    }

    private enum Placement {
        case lenient, strict, following, strictFollowing
    }

    private static let english = Table(
        commands: [
            ("question mark", .attach("?"), .question, .strict),
            ("exclamation point", .attach("!"), .exclamation, .strict),
            ("exclamation mark", .attach("!"), .exclamation, .strict),
            ("full stop", .attach("."), .period, .lenient),
            ("period", .attach("."), .period, .strict),
            ("comma", .attach(","), .comma, .lenient),
            ("semicolon", .attach(";"), .semicolon, .lenient),
            ("semi colon", .attach(";"), .semicolon, .lenient),
            ("colon", .attach(":"), .colon, .following),
            ("ellipsis", .attach("…"), .ellipsis, .lenient),
            ("dot dot dot", .attach("…"), .ellipsis, .lenient),
            ("em dash", .join("—"), .dash, .strict),
            ("dash", .spaced("–"), .dash, .lenient),
            ("hyphen", .join("-"), .hyphen, .lenient),
            ("apostrophe", .join("'"), .apostrophe, .lenient),
            // "at sign" is Clean up's (`SpokenCleanupRules.spokenSymbols`), which runs first.
            ("hashtag", .prefix("#"), .hashtag, .lenient),
            ("dollar sign", .prefix("$"), .dollar, .lenient),
            ("open quote", .open("“"), .quote, .lenient),
            ("open quotes", .open("“"), .quote, .lenient),
            ("begin quote", .open("“"), .quote, .lenient),
            ("close quote", .close(.quote), .quote, .lenient),
            ("close quotes", .close(.quote), .quote, .lenient),
            ("end quote", .close(.quote), .quote, .lenient),
            ("end quotes", .close(.quote), .quote, .lenient),
            ("unquote", .close(.quote), .quote, .lenient),
            ("quote", .toggleQuote(toggles: false), .quote, .lenient),
            ("open paren", .open("("), .paren, .lenient),
            ("open parenthesis", .open("("), .paren, .lenient),
            ("open parentheses", .open("("), .paren, .lenient),
            ("close paren", .close(.paren), .paren, .lenient),
            ("close parenthesis", .close(.paren), .paren, .lenient),
            ("close parentheses", .close(.paren), .paren, .lenient),
            ("smiley face", .emoticon(":-)"), .smiley, .strict),
            ("smiley", .emoticon(":-)"), .smiley, .strict),
            ("frowny face", .emoticon(":-("), .smiley, .strict),
            ("frowny", .emoticon(":-("), .smiley, .strict),
            ("winky face", .emoticon(";-)"), .smiley, .strict),
            ("winky", .emoticon(";-)"), .smiley, .strict),
        ],
        blockers: [
            "a", "an", "the", "this", "that", "these", "those", "my", "your", "his", "her", "its", "our", "their",
            "no", "every", "each", "any", "some", "another", "what", "which", "whose", "one", "of", "big", "huge", "giant", "little", "small",
            "large", "tiny", "red", "black", "blue", "green", "white", "yellow", "gold", "golden", "silver", "bold",
            "bright", "extra", "missing", "stray", "misplaced", "single", "double", "triple", "final", "last",
            "first", "second", "third", "next", "same", "whole", "entire", "upside", "inverted", "mad", "quick",
            "em", "en", "more", "fewer", "less",
        ],
        numbers: ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten"],
        prepositions: ["in", "on", "at", "to", "for", "with", "by", "from", "about", "into", "than", "like", "without", "per"],
        prepositionsEndSentences: true,
        familyBlockers: [
            .comma: ["oxford", "serial", "decimal"],
            .period: [
                "trial", "grace", "time", "free", "waiting", "cooling", "notice", "billing", "school", "class",
                "study", "rest", "transition", "transitional", "warranty", "introductory", "honeymoon", "probation",
                "probationary", "historical", "victorian", "jurassic", "cretaceous", "triassic", "renaissance",
                "colonial", "medieval", "early", "late", "given", "certain", "limited", "specific", "extended",
                "brief", "reporting", "accounting", "fiscal", "lunch", "recovery", "adjustment", "settling",
                "initial", "current", "previous", "short", "long", "missed", "heavy", "light", "fourth", "fifth",
                "sixth", "seventh", "eighth", "dry", "rainy", "wet", "holiday", "summer", "winter", "spring",
                "fall", "lockdown", "quiet", "busy", "difficult", "hard", "tough", "rough", "happy", "sad",
            ],
            .colon: ["sigmoid", "transverse", "descending", "ascending", "inflamed", "spastic", "irritable", "healthy"],
            .dash: [
                "desperate", "frantic", "yard", "metre", "meter", "hundred", "fifty", "mile", "i", "you", "we",
                "they", "he", "she", "i'll", "we'll", "you'll", "they'll", "i'd", "will", "must", "gotta", "gonna",
                "can", "could", "should", "would", "might", "let's", "and",
            ],
            .hashtag: [
                "i", "you", "we", "they", "he", "she", "will", "can", "can't", "don't", "do", "didn't", "would",
                "should", "could", "must", "never", "always", "just", "popular", "trending", "viral", "new", "own",
                "official", "branded",
            ],
        ],
        followers: ["of", "is", "was", "are", "were", "isn't", "wasn't", "aren't", "weren't", "has", "hasn't", "had"],
        familyFollowers: [
            .comma: ["after", "before", "between", "in", "inside", "instead", "splice", "splices", "separated", "delimited", "key", "button", "placement", "rule", "rules", "usage", "sign", "symbol"],
            .period: [
                "after", "before", "in", "during", "when", "where", "for", "piece", "pieces", "drama", "dramas", "costume",
                "costumes", "cramps", "cramp", "pain", "pains", "tracker", "app", "poverty", "products", "product",
                "underwear", "blood", "end", "ends", "ended", "starts", "started", "begins", "began", "lasts", "lasted",
                "correct", "accurate", "film", "films", "features", "furniture", "room", "rooms", "key", "button",
                "sign", "symbol", "to",
            ],
            .question: ["tattoo", "tattoos", "key", "keys", "button", "sign", "symbol", "symbols", "icon", "emoji", "shape", "shaped", "necklace", "over", "hanging", "hangs", "hung", "on", "in", "after", "at"],
            .exclamation: ["tattoo", "tattoos", "key", "keys", "button", "sign", "symbol", "symbols", "icon", "emoji", "shape", "on", "in", "after", "at"],
            .colon: [
                "cancer", "cancers", "surgery", "health", "cleanse", "cleansing", "polyps", "polyp", "screening",
                "screenings", "exam", "problems", "problem", "issues", "wall", "lining", "tumor", "tumour", "disease",
                "inflammation", "function", "symbol", "sign", "key", "character", "in", "after", "between", "and", "or",
            ],
            .semicolon: ["in", "after", "between", "key", "sign", "symbol", "usage"],
            .ellipsis: ["in", "after", "key", "sign", "symbol", "usage"],
            .dash: [
                "for", "to", "cam", "cams", "board", "button", "light", "lights", "through", "across", "into", "out",
                "off", "away", "over", "around", "home", "back", "past", "down", "up", "in", "on", "mat",
            ],
            .hyphen: ["in", "after", "between", "key", "sign", "symbol", "usage"],
            .apostrophe: ["in", "after", "key", "sign", "symbol", "usage"],
            .at: ["key", "symbol"],
            .hashtag: ["on", "in", "for", "and", "or", "to"],
            .dollar: ["on", "in", "for", "and", "or", "key", "symbol"],
            .smiley: ["emoji", "emojis", "icon", "sticker", "stickers", "faces", "ball", "balls"],
        ],
        starters: [
            "i", "i'm", "i'll", "i've", "i'd", "you", "you're", "you'll", "we", "we're", "we'll", "he", "he's",
            "she", "she's", "they", "they're", "it", "it's", "that's", "there", "there's", "here", "here's", "this",
            "so", "and", "but", "or", "then", "because", "also", "plus", "thanks", "thank", "please", "see", "let",
            "let's", "talk", "call", "hope", "love", "cheers", "ok", "okay", "yes", "yeah", "no", "hi", "hey",
            "hello", "sorry", "maybe", "anyway", "what", "what's", "why", "how", "when", "where", "who", "if",
            "sure", "best", "regards", "just", "can", "could", "would", "will", "do", "did", "don't", "didn't",
            "not", "great", "cool", "talk", "speak", "bye", "goodbye", "kind", "warm",
        ],
        quotes: ("“", "”"),
        possessives: true
    )

    private static let spanish = Table(
        commands: [
            ("punto y coma", .attach(";"), .semicolon, .lenient),
            ("punto y aparte", .lineEnd(".", "\n\n"), .period, .lenient),
            ("punto y seguido", .attach("."), .period, .lenient),
            ("punto final", .attach("."), .period, .lenient),
            ("punto", .attach("."), .period, .strict),
            ("coma", .attach(","), .comma, .lenient),
            ("dos puntos", .attach(":"), .colon, .strictFollowing),
            ("abrir signo de interrogación", .open("¿"), .question, .lenient),
            ("abre signo de interrogación", .open("¿"), .question, .lenient),
            ("signo de interrogación de apertura", .open("¿"), .question, .lenient),
            ("abrir interrogación", .open("¿"), .question, .lenient),
            ("cerrar signo de interrogación", .attach("?"), .question, .lenient),
            ("cierra signo de interrogación", .attach("?"), .question, .lenient),
            ("signo de interrogación de cierre", .attach("?"), .question, .lenient),
            ("cerrar interrogación", .attach("?"), .question, .lenient),
            ("signo de interrogación", .attach("?"), .question, .strict),
            ("abrir signo de exclamación", .open("¡"), .exclamation, .lenient),
            ("abre signo de exclamación", .open("¡"), .exclamation, .lenient),
            ("signo de exclamación de apertura", .open("¡"), .exclamation, .lenient),
            ("abrir exclamación", .open("¡"), .exclamation, .lenient),
            ("cerrar signo de exclamación", .attach("!"), .exclamation, .lenient),
            ("cierra signo de exclamación", .attach("!"), .exclamation, .lenient),
            ("signo de exclamación de cierre", .attach("!"), .exclamation, .lenient),
            ("cerrar exclamación", .attach("!"), .exclamation, .lenient),
            ("signo de exclamación", .attach("!"), .exclamation, .strict),
            ("signo de admiración", .attach("!"), .exclamation, .strict),
            ("abrir comillas", .open("“"), .quote, .lenient),
            ("abre comillas", .open("“"), .quote, .lenient),
            ("comillas de apertura", .open("“"), .quote, .lenient),
            ("comillas iniciales", .open("“"), .quote, .lenient),
            ("cerrar comillas", .close(.quote), .quote, .lenient),
            ("cierra comillas", .close(.quote), .quote, .lenient),
            ("comillas de cierre", .close(.quote), .quote, .lenient),
            ("comillas finales", .close(.quote), .quote, .lenient),
            ("comillas", .toggleQuote(toggles: true), .quote, .lenient),
            ("abrir paréntesis", .open("("), .paren, .lenient),
            ("abre paréntesis", .open("("), .paren, .lenient),
            ("paréntesis de apertura", .open("("), .paren, .lenient),
            ("cerrar paréntesis", .close(.paren), .paren, .lenient),
            ("cierra paréntesis", .close(.paren), .paren, .lenient),
            ("paréntesis de cierre", .close(.paren), .paren, .lenient),
            ("puntos suspensivos", .attach("…"), .ellipsis, .lenient),
            ("guion", .join("-"), .hyphen, .lenient),
            ("arroba", .join("@"), .at, .lenient),
            ("apóstrofo", .join("'"), .apostrophe, .lenient),
            ("hashtag", .prefix("#"), .hashtag, .lenient),
        ],
        blockers: [
            "el", "la", "los", "las", "un", "una", "unos", "unas", "este", "esta", "ese", "esa", "aquel", "aquella",
            "mi", "mis", "tu", "tus", "su", "sus", "nuestro", "nuestra", "cada", "otro", "otra", "ningún", "ninguna",
            "algún", "alguna", "del", "al", "de", "buen", "gran", "mismo", "misma", "primer", "último", "tercer", "cuyo", "cuya",
        ],
        numbers: ["cero", "uno", "dos", "tres", "cuatro", "cinco", "seis", "siete", "ocho", "nueve", "diez"],
        prepositions: ["a", "en", "con", "por", "para", "sin", "entre", "sobre", "hasta", "desde", "ante", "tras"],
        followers: ["de", "del", "es", "era", "fue", "son", "está", "estaba", "para", "en", "que"],
        familyFollowers: [
            .period: ["medio", "muerto", "fuerte", "débil", "clave", "crítico", "cardinal", "cardinales", "culminante", "neutro", "álgido", "exacto"],
            .comma: ["inducido", "etílico", "profundo", "diabético"],
            .colon: ["más", "menos", "importantes", "clave", "principales"],
        ],
        starters: [
            "yo", "tú", "ella", "nosotros", "nosotras", "ellos", "ellas", "usted", "ustedes", "y", "pero", "o",
            "entonces", "porque", "también", "gracias", "por", "nos", "te", "me", "se", "que", "qué", "como",
            "cómo", "cuando", "donde", "hola", "sí", "si", "no", "vale", "bueno", "oye", "saludos", "espero", "ya",
            "pues", "luego", "después", "tengo", "estoy", "voy", "vamos", "llego", "quiero", "besos", "abrazos",
            "hasta", "adiós", "chao", "mira", "claro",
        ],
        quotes: ("“", "”")
    )

    private static let french = Table(
        commands: [
            ("point virgule", .attach(";"), .semicolon, .lenient),
            ("point d'interrogation", .attach("?"), .question, .strict),
            ("point d'exclamation", .attach("!"), .exclamation, .strict),
            ("points de suspension", .attach("…"), .ellipsis, .lenient),
            ("point final", .attach("."), .period, .lenient),
            ("point à la ligne", .lineEnd(".", "\n"), .period, .lenient),
            ("point", .attach("."), .period, .strict),
            ("virgule", .attach(","), .comma, .lenient),
            ("deux points", .attach(":"), .colon, .strictFollowing),
            ("ouvrir les guillemets", .open("«"), .quote, .lenient),
            ("ouvrez les guillemets", .open("«"), .quote, .lenient),
            ("ouvrir guillemets", .open("«"), .quote, .lenient),
            ("ouvrez guillemets", .open("«"), .quote, .lenient),
            ("guillemets ouvrants", .open("«"), .quote, .lenient),
            ("guillemet ouvrant", .open("«"), .quote, .lenient),
            ("fermer les guillemets", .close(.quote), .quote, .lenient),
            ("fermez les guillemets", .close(.quote), .quote, .lenient),
            ("fermer guillemets", .close(.quote), .quote, .lenient),
            ("fermez guillemets", .close(.quote), .quote, .lenient),
            ("guillemets fermants", .close(.quote), .quote, .lenient),
            ("guillemet fermant", .close(.quote), .quote, .lenient),
            ("fin des guillemets", .close(.quote), .quote, .lenient),
            ("guillemets", .toggleQuote(toggles: true), .quote, .lenient),
            ("ouvrir la parenthèse", .open("("), .paren, .lenient),
            ("ouvrez la parenthèse", .open("("), .paren, .lenient),
            ("ouvrir parenthèse", .open("("), .paren, .lenient),
            ("ouvrez parenthèse", .open("("), .paren, .lenient),
            ("parenthèse ouvrante", .open("("), .paren, .lenient),
            ("fermer la parenthèse", .close(.paren), .paren, .lenient),
            ("fermez la parenthèse", .close(.paren), .paren, .lenient),
            ("fermer parenthèse", .close(.paren), .paren, .lenient),
            ("fermez parenthèse", .close(.paren), .paren, .lenient),
            ("parenthèse fermante", .close(.paren), .paren, .lenient),
            ("tiret", .spaced("–"), .dash, .strict),
            ("trait d'union", .join("-"), .hyphen, .lenient),
            ("arobase", .join("@"), .at, .lenient),
            ("apostrophe", .join("'"), .apostrophe, .lenient),
            ("hashtag", .prefix("#"), .hashtag, .lenient),
        ],
        blockers: [
            "le", "la", "les", "l'", "un", "une", "des", "du", "de", "d'", "ce", "cet", "cette", "ces", "mon", "ma",
            "mes", "ton", "ta", "tes", "son", "sa", "ses", "notre", "nos", "votre", "vos", "leur", "leurs", "aucun",
            "aucune", "chaque", "quel", "quelle", "quelque", "au", "aux", "bon", "mauvais", "dernier", "premier", "seul", "petit", "grand",
            "gros", "même", "d'un",
            "d'une", "qu'un", "qu'une", "l'un",
        ],
        numbers: ["zéro", "un", "deux", "trois", "quatre", "cinq", "six", "sept", "huit", "neuf", "dix"],
        prepositions: ["à", "en", "sur", "par", "pour", "avec", "sans", "dans", "entre", "vers", "chez"],
        followers: ["de", "du", "des", "est", "était", "sont", "important", "importante", "essentiel", "principal", "central", "faible", "fort", "commun", "précis", "crucial", "culminant", "clé", "mort", "noir", "chaud", "flottante", "décimale"],
        followerPrefixes: ["d'"],
        starters: [
            "je", "tu", "il", "elle", "on", "nous", "vous", "ils", "elles", "ce", "ça", "et", "mais", "ou", "donc",
            "alors", "puis", "parce", "merci", "bonjour", "bonsoir", "salut", "coucou", "oui", "non", "à",
            "bisous", "bises", "si", "quand", "comment", "pourquoi", "où", "qui", "que", "voilà", "ok",
            "cordialement", "bonne", "bonnes", "d'accord", "désolé", "désolée",
        ],
        starterPrefixes: ["j'", "c'", "qu'", "n'", "s'", "m'", "t'"],
        quotes: ("«", "»")
    )

    private static let german = Table(
        commands: [
            ("komma", .attach(","), .comma, .lenient),
            ("punkt", .attach("."), .period, .strict),
            ("fragezeichen", .attach("?"), .question, .strict),
            ("ausrufezeichen", .attach("!"), .exclamation, .strict),
            ("ausrufungszeichen", .attach("!"), .exclamation, .strict),
            ("doppelpunkt", .attach(":"), .colon, .following),
            ("semikolon", .attach(";"), .semicolon, .lenient),
            ("strichpunkt", .attach(";"), .semicolon, .lenient),
            ("auslassungspunkte", .attach("…"), .ellipsis, .lenient),
            ("anführungszeichen unten", .open("„"), .quote, .lenient),
            ("anführungszeichen auf", .open("„"), .quote, .lenient),
            ("anführungszeichen oben", .close(.quote), .quote, .lenient),
            ("anführungszeichen zu", .close(.quote), .quote, .lenient),
            ("abführungszeichen", .close(.quote), .quote, .lenient),
            ("anführungszeichen", .toggleQuote(toggles: true), .quote, .lenient),
            ("klammer auf", .open("("), .paren, .lenient),
            ("öffnende klammer", .open("("), .paren, .lenient),
            ("klammer zu", .close(.paren), .paren, .lenient),
            ("schließende klammer", .close(.paren), .paren, .lenient),
            ("gedankenstrich", .spaced("–"), .dash, .strict),
            ("bindestrich", .join("-"), .hyphen, .lenient),
            ("at zeichen", .join("@"), .at, .lenient),
            ("klammeraffe", .join("@"), .at, .lenient),
            ("apostroph", .join("'"), .apostrophe, .lenient),
            ("hashtag", .prefix("#"), .hashtag, .lenient),
        ],
        blockers: [
            "der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem", "einer", "eines", "kein",
            "keine", "keinen", "keinem", "keiner", "mein", "meine", "meinen", "meinem", "meiner", "dein", "deine",
            "deinen", "sein", "seine", "seinen", "seinem", "ihr", "ihre", "ihren", "ihrem", "unser", "unsere",
            "unseren", "euer", "eure", "dieser", "diese", "dieses", "diesen", "diesem", "jeder", "jede", "jedes",
            "jeden", "welcher", "welche", "welches", "zum", "zur", "im", "am", "um", "beim", "vom", "ins", "ans",
            "springende",
        ],
        numbers: ["null", "eins", "zwei", "drei", "vier", "fünf", "sechs", "sieben", "acht", "neun", "zehn"],
        prepositions: ["mit", "ohne", "für", "auf", "an", "in", "nach", "vor", "bei", "von", "zu", "aus", "über", "unter", "gegen", "durch", "als"],
        prepositionsEndSentences: true,
        followers: [
            "ist", "war", "sind", "waren", "hat", "hatte", "für", "der", "des", "von", "auf", "eins", "zwei", "drei",
            "vier", "fünf", "sechs", "sieben", "acht", "neun", "zehn", "elf", "zwölf", "uhr",
        ],
        starters: [
            "ich", "du", "er", "sie", "es", "wir", "ihr", "und", "aber", "oder", "dann", "also", "denn", "weil",
            "danke", "bitte", "hallo", "hi", "hey", "ja", "nein", "bis", "viele", "liebe", "lieber", "gruß",
            "grüße", "wie", "was", "wann", "wo", "warum", "wer", "wenn", "dass", "ok", "okay", "mal", "sorry",
            "tschüss", "schöne", "schönen", "mach", "machs",
        ],
        quotes: ("„", "“"),
        capitalizesSentences: false,
        adjectivesBeforeNouns: true
    )

    private static let italian = Table(
        commands: [
            ("punto e virgola", .attach(";"), .semicolon, .lenient),
            ("punto interrogativo", .attach("?"), .question, .strict),
            ("punto di domanda", .attach("?"), .question, .strict),
            ("punto esclamativo", .attach("!"), .exclamation, .strict),
            ("punto fermo", .attach("."), .period, .lenient),
            ("punto a capo", .lineEnd(".", "\n"), .period, .lenient),
            ("punto", .attach("."), .period, .strict),
            ("virgola", .attach(","), .comma, .lenient),
            ("due punti", .attach(":"), .colon, .strictFollowing),
            ("puntini di sospensione", .attach("…"), .ellipsis, .lenient),
            ("tre puntini", .attach("…"), .ellipsis, .lenient),
            ("apri virgolette", .open("“"), .quote, .lenient),
            ("aperte virgolette", .open("“"), .quote, .lenient),
            ("aperta virgolette", .open("“"), .quote, .lenient),
            ("virgolette aperte", .open("“"), .quote, .lenient),
            ("chiudi virgolette", .close(.quote), .quote, .lenient),
            ("chiuse virgolette", .close(.quote), .quote, .lenient),
            ("chiusa virgolette", .close(.quote), .quote, .lenient),
            ("virgolette chiuse", .close(.quote), .quote, .lenient),
            ("virgolette", .toggleQuote(toggles: true), .quote, .lenient),
            ("apri parentesi", .open("("), .paren, .lenient),
            ("aperta parentesi", .open("("), .paren, .lenient),
            ("parentesi aperta", .open("("), .paren, .lenient),
            ("chiudi parentesi", .close(.paren), .paren, .lenient),
            ("chiusa parentesi", .close(.paren), .paren, .lenient),
            ("parentesi chiusa", .close(.paren), .paren, .lenient),
            ("lineetta", .spaced("–"), .dash, .strict),
            ("trattino", .join("-"), .hyphen, .lenient),
            ("chiocciola", .join("@"), .at, .lenient),
            ("apostrofo", .join("'"), .apostrophe, .lenient),
            ("hashtag", .prefix("#"), .hashtag, .lenient),
            ("cancelletto", .prefix("#"), .hashtag, .lenient),
        ],
        blockers: [
            "il", "lo", "la", "i", "gli", "le", "l'", "un", "uno", "una", "un'", "questo", "questa", "quel", "quello",
            "quella", "quei", "quegli", "quelle", "mio", "mia", "miei", "mie", "tuo", "tua", "suo", "sua", "nostro",
            "nostra", "vostro", "vostra", "ogni", "nessun", "nessuno", "nessuna", "del", "della", "dello", "dei",
            "degli", "delle", "al", "alla", "allo", "ai", "agli", "alle", "nel", "nella", "nello", "nei", "sul",
            "sulla", "di", "buon", "bel", "gran", "grande",
            "ultimo", "primo", "stesso",
        ],
        numbers: ["zero", "uno", "due", "tre", "quattro", "cinque", "sei", "sette", "otto", "nove", "dieci"],
        prepositions: ["a", "da", "in", "con", "su", "per", "tra", "fra"],
        followers: ["di", "del", "della", "dello", "dei", "era", "sono", "importante", "fondamentale", "chiave", "centrale", "debole", "forte", "cruciale", "cardine", "morto", "vendita", "cui", "in"],
        followerPrefixes: ["d'"],
        starters: [
            "io", "tu", "lui", "lei", "noi", "voi", "loro", "e", "ma", "o", "poi", "quindi", "perché", "allora",
            "grazie", "ciao", "sì", "si", "no", "che", "come", "quando", "dove", "chi", "ci", "ti", "mi", "vi", "ok",
            "va", "a", "buongiorno", "buonasera", "salve", "ecco", "dai", "baci", "ho", "hai", "ha", "abbiamo",
            "vado", "arrivo", "vengo", "sono",
        ],
        starterPrefixes: ["c'", "l'"],
        quotes: ("“", "”")
    )

    private static let portuguese = Table(
        commands: [
            ("ponto e vírgula", .attach(";"), .semicolon, .lenient),
            ("ponto de interrogação", .attach("?"), .question, .strict),
            ("ponto de exclamação", .attach("!"), .exclamation, .strict),
            ("ponto final", .attach("."), .period, .lenient),
            ("ponto parágrafo", .lineEnd(".", "\n\n"), .period, .lenient),
            ("ponto", .attach("."), .period, .strict),
            ("vírgula", .attach(","), .comma, .lenient),
            ("dois pontos", .attach(":"), .colon, .strictFollowing),
            ("reticências", .attach("…"), .ellipsis, .lenient),
            ("abre aspas", .open("“"), .quote, .lenient),
            ("abrir aspas", .open("“"), .quote, .lenient),
            ("fecha aspas", .close(.quote), .quote, .lenient),
            ("fechar aspas", .close(.quote), .quote, .lenient),
            ("aspas", .toggleQuote(toggles: true), .quote, .lenient),
            ("abre parênteses", .open("("), .paren, .lenient),
            ("abre parêntese", .open("("), .paren, .lenient),
            ("abrir parênteses", .open("("), .paren, .lenient),
            ("abrir parêntese", .open("("), .paren, .lenient),
            ("fecha parênteses", .close(.paren), .paren, .lenient),
            ("fecha parêntese", .close(.paren), .paren, .lenient),
            ("fechar parênteses", .close(.paren), .paren, .lenient),
            ("fechar parêntese", .close(.paren), .paren, .lenient),
            ("travessão", .spaced("–"), .dash, .strict),
            ("hífen", .join("-"), .hyphen, .lenient),
            ("arroba", .join("@"), .at, .lenient),
            ("apóstrofo", .join("'"), .apostrophe, .lenient),
            ("hashtag", .prefix("#"), .hashtag, .lenient),
        ],
        blockers: [
            "o", "a", "os", "as", "um", "uma", "uns", "umas", "este", "esta", "estes", "estas", "esse", "essa",
            "esses", "essas", "aquele", "aquela", "meu", "minha", "meus", "minhas", "teu", "tua", "seu", "sua",
            "seus", "suas", "nosso", "nossa", "cada", "nenhum", "nenhuma", "algum", "alguma", "outro", "outra",
            "do", "da", "dos", "das", "no", "na", "nas", "ao", "aos", "pelo", "pela", "pelos", "pelas", "de", "bom", "grande", "mesmo", "último", "primeiro", "dois", "duas",
        ],
        numbers: ["zero", "um", "uma", "dois", "duas", "três", "quatro", "cinco", "seis", "sete", "oito", "nove", "dez"],
        prepositions: ["em", "com", "por", "para", "sem", "entre", "sobre"],
        followers: ["de", "do", "da", "dos", "das", "era", "são", "foi", "principal", "importante", "chave", "central", "fraco", "forte", "crucial", "em", "morto", "alto", "baixo"],
        starters: [
            "eu", "tu", "você", "ele", "ela", "nós", "eles", "elas", "vocês", "e", "mas", "ou", "então", "porque",
            "também", "obrigado", "obrigada", "olá", "oi", "sim", "não", "que", "como", "quando", "onde", "quem",
            "até", "te", "me", "se", "beijos", "beijo", "abraço", "abraços", "tudo", "estou", "vou", "vamos",
            "chego", "tenho",
        ],
        quotes: ("“", "”")
    )
}
