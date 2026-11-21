import Foundation
import NaturalLanguage
@preconcurrency import Translation

/// Experiment (Translate what I say): speak one language, insert another. Translation
/// runs on the iPhone with Apple's Translation framework once the language pack is
/// downloaded in Hex. When it can't, the transcript goes in as spoken and the keyboard says
/// why (`TranslationFailure`).
struct DictationTranslationSettings {
    private enum Key {
        static let enabled = "experiments.translateDictation"
        static let target = "experiments.translateDictation.target"
        /// Build 56's Paste in <language> pair kept "as spoken" here, which left translation
        /// off with Settings' switch still on. It now reads as the switch off, and the next
        /// switch removes it.
        static let pastesTranslation = "experiments.translateDictation.pastesTranslation"
        static let lastFailure = "experiments.translateDictation.lastFailure"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// The one switch: Settings' Translate what I say and the Hex key menu's Translate to
    /// <language>. Only they change it; nothing turns it off by itself.
    var isEnabled: Bool {
        get {
            guard defaults?.bool(forKey: Key.enabled) == true else { return false }
            return defaults?.object(forKey: Key.pastesTranslation) as? Bool ?? true
        }
        nonmutating set {
            defaults?.set(newValue, forKey: Key.enabled)
            defaults?.removeObject(forKey: Key.pastesTranslation)
        }
    }

    /// BCP 47 language code, such as `ja`.
    var targetLanguage: String {
        get { defaults?.string(forKey: Key.target) ?? "ja" }
        nonmutating set { defaults?.set(newValue, forKey: Key.target) }
    }

    /// Why the last dictation went in as spoken, until one translates: Settings shows it and
    /// offers the fix.
    var lastFailure: TranslationFailure? {
        get {
            defaults?.data(forKey: Key.lastFailure).flatMap { try? JSONDecoder().decode(TranslationFailure.self, from: $0) }
        }
        nonmutating set {
            defaults?.set(newValue.flatMap { try? JSONEncoder().encode($0) }, forKey: Key.lastFailure)
        }
    }

    /// The most targets the Hex key menu lists; Settings has every language.
    static let maxTargetChoices = 6

    /// Fill the Hex key menu's targets after your own languages, so a phone set up in one
    /// language still has some to pick from.
    static let commonTargets = ["es", "zh", "fr", "de", "ja", "ko", "pt", "it"]

    /// The targets the Hex key menu offers, as minimal identifiers (`en`, `zh`, `zh-TW`):
    /// the current one, English, the dictation languages and the iPhone's preferred
    /// languages, then `commonTargets`, one per identifier. `supported` (Apple's translation
    /// languages, nil until listed) drops the ones Apple can't translate into; the current
    /// one always stays.
    static func targetChoices(
        current: String,
        dictation: [String],
        preferred: [String],
        supported: [String]?
    ) -> [String] {
        let allowed = supported.map(Set.init)
        let currentMinimal = Locale.Language(identifier: current).minimalIdentifier
        var seen = Set<String>()
        var choices: [String] = []
        for identifier in [current, "en"] + dictation + preferred + commonTargets {
            let minimal = Locale.Language(identifier: identifier).minimalIdentifier
            guard !minimal.isEmpty, seen.insert(minimal).inserted else { continue }
            if minimal != currentMinimal, let allowed, !allowed.contains(minimal) { continue }
            choices.append(minimal)
            if choices.count == maxTargetChoices { break }
        }
        return choices
    }

    /// The switch for a dictation in `source` (BCP 47, nil when unknown), or nil while the
    /// experiment is off or you dictate in the target language.
    func pasteChoice(source: String?) -> TranslationPasteChoice? {
        guard isEnabled else { return nil }
        return TranslationPasteChoice(target: targetLanguage, source: source)
    }
}

/// Translate what I say's switch where you dictate: "Paste in English" (translated) or
/// "Paste in Spanish" (as spoken), named in Hex's interface language like the rest of its words.
struct TranslationPasteChoice: Equatable {
    let target: String
    let source: String?

    /// Nil when `source` is the target language already.
    init?(target: String, source: String?) {
        if let source, Self.code(of: source) == Self.code(of: target) { return nil }
        self.target = target
        self.source = source
    }

    var translatedTitle: String {
        let name = Self.name(of: Locale.Language(identifier: target).minimalIdentifier)
        return String(localized: "Paste in \(name)")
    }

    /// The Hex key menu's switch: "Translate to English".
    static func toggleTitle(target: String) -> String {
        let name = name(of: Locale.Language(identifier: target).minimalIdentifier)
        return String(localized: "Translate to \(name)")
    }

    /// The badge on the Hex key while dictations paste translated: "EN", "JA".
    static func badge(target: String) -> String {
        code(of: target).uppercased()
    }

    /// What the badge says to VoiceOver: "Translating to English".
    static func translatingLabel(target: String) -> String {
        let name = name(of: Locale.Language(identifier: target).minimalIdentifier)
        return String(localized: "Translating to \(name)")
    }

    var spokenTitle: String {
        guard let source else { return String(localized: "Paste as spoken") }
        let name = Self.name(of: Self.code(of: source))
        return String(localized: "Paste in \(name)")
    }

    /// "Spanish", "Chinese, Traditional"; Norwegian without "Bokmål".
    static func name(of identifier: String) -> String {
        if ["nb", "nn", "no"].contains(code(of: identifier)) {
            return Locale.hexInterface.localizedString(forLanguageCode: "no") ?? "Norwegian"
        }
        return Locale.hexInterface.localizedString(forIdentifier: identifier) ?? identifier
    }

    private static func code(of identifier: String) -> String {
        Locale(identifier: identifier).language.languageCode?.identifier ?? identifier
    }
}

/// Why a dictation went in as spoken while Translate what I say was on. The keyboard says so
/// as it pastes, with the fix, and Settings keeps it (`DictationTranslationSettings.lastFailure`).
nonisolated struct TranslationFailure: Codable, Equatable, Sendable {
    enum Reason: String, Codable, Sendable {
        /// Apple's languages for the pair aren't on the iPhone: never downloaded, removed in
        /// iOS Settings, or still downloading.
        case notDownloaded
        /// Apple can't translate from the one into the other.
        case unsupported
        /// The translation failed twice, each time in a new session.
        case failed
    }

    let reason: Reason
    /// Minimal identifiers (`zh`, `en`).
    let source: String
    let target: String
    /// Apple's error, for diagnosis; never the dictated text.
    var detail: String?

    init(_ reason: Reason, source: Locale.Language, target: Locale.Language, detail: String? = nil) {
        self.reason = reason
        self.source = source.minimalIdentifier
        self.target = target.minimalIdentifier
        self.detail = detail
    }

    /// What the keyboard says after pasting as spoken.
    static var note: String { String(localized: "Couldn't translate, pasted as spoken") }

    /// The language Settings downloads: the one that isn't English, which Apple pairs with
    /// most languages.
    var language: String {
        Locale.Language(identifier: target).languageCode == .english ? source : target
    }

    /// The fix, short enough for the keyboard's second line: "Download Chinese in Settings".
    var fix: String {
        let name = TranslationPasteChoice.name(of: language)
        return switch reason {
        case .notDownloaded: String(localized: "Download \(name) in Settings")
        case .unsupported: String(localized: "Pick another language in Settings")
        case .failed: String(localized: "Download \(name) again in Settings")
        }
    }
}

/// What a dictation's translation came to.
nonisolated enum TranslationOutcome: Equatable, Sendable {
    /// Off, nothing to translate, or already in the target language.
    case skipped
    case translated(String)
    /// Pasted as spoken, and why.
    case failed(TranslationFailure)

    var text: String? {
        if case .translated(let text) = self { text } else { nil }
    }

    var failure: TranslationFailure? {
        if case .failed(let failure) = self { failure } else { nil }
    }
}

/// Apple's Translation framework behind a seam tests stand in for: whether a pair's languages
/// are on the iPhone, and one translation.
nonisolated struct TranslationEngine: Sendable {
    var status: @MainActor @Sendable (Locale.Language, Locale.Language) async -> LanguageAvailability.Status
    var translate: @MainActor @Sendable (String, Locale.Language, Locale.Language) async throws -> String

    @available(iOS 26.0, *)
    static var apple: TranslationEngine {
        TranslationEngine(
            status: { source, target in await LanguageAvailability().status(from: source, to: target) },
            translate: { text, source, target in
                // A new session for every translation, never one kept: a session whose
                // translation service went away (the keyboard or Hex suspended, the service
                // restarted) can't be used again.
                try await TranslationSession(installedSource: source, target: target).translate(text).targetText
            }
        )
    }
}

enum DictationTranslator {
    /// The translation, or why it went in as spoken. Skipped when the experiment is off, the
    /// text is empty or already in the target language, or before iOS 26. `source` is the
    /// dictation language; without one the text's language is guessed. A failed translation
    /// is tried once more in a new session; a missing language pack isn't retried. The last
    /// failure is kept for Settings until a translation works.
    @MainActor
    static func translate(
        _ text: String,
        source: Locale.Language? = nil,
        settings: DictationTranslationSettings = DictationTranslationSettings(),
        engine: TranslationEngine? = nil
    ) async -> TranslationOutcome {
        guard settings.isEnabled else { return .skipped }
        let engine: TranslationEngine? = if #available(iOS 26.0, *) { engine ?? .apple } else { engine }
        guard let engine else { return .skipped }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let source = source ?? detectedLanguage(of: trimmed) else { return .skipped }
        let target = Locale.Language(identifier: settings.targetLanguage)
        guard source.languageCode != target.languageCode else { return .skipped }
        let outcome = await attempt(trimmed, from: source, to: target, engine: engine)
        switch outcome {
        case .translated: if settings.lastFailure != nil { settings.lastFailure = nil }
        case .failed(let failure): settings.lastFailure = failure
        case .skipped: break
        }
        return outcome
    }

    @MainActor
    private static func attempt(
        _ text: String,
        from source: Locale.Language,
        to target: Locale.Language,
        engine: TranslationEngine
    ) async -> TranslationOutcome {
        switch await engine.status(source, target) {
        case .unsupported: return .failed(TranslationFailure(.unsupported, source: source, target: target))
        case .supported: return .failed(TranslationFailure(.notDownloaded, source: source, target: target))
        default: break
        }
        var failure = TranslationFailure(.failed, source: source, target: target)
        for _ in 0..<2 {
            do {
                let translated = try await engine.translate(text, source, target)
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                if !translated.isEmpty { return .translated(translated) }
            } catch {
                failure = TranslationFailure(reason(for: error), source: source, target: target, detail: describe(error))
                // Another session can't download a language or pair two Apple can't.
                if failure.reason != .failed { break }
            }
        }
        return .failed(failure)
    }

    static func reason(for error: any Error) -> TranslationFailure.Reason {
        if #available(iOS 26.0, *), TranslationError.notInstalled ~= error { return .notDownloaded }
        if TranslationError.unsupportedSourceLanguage ~= error
            || TranslationError.unsupportedTargetLanguage ~= error
            || TranslationError.unsupportedLanguagePairing ~= error {
            return .unsupported
        }
        return .failed
    }

    private static func describe(_ error: any Error) -> String {
        let error = error as NSError
        let reason = error.localizedFailureReason.map { ": \($0)" } ?? ""
        return "\(error.domain) \(error.code)\(reason)"
    }

    /// The languages Apple translates into, as minimal identifiers (`en`, `zh-TW`).
    @available(iOS 26.0, *)
    static func supportedTargets() async -> [String] {
        await LanguageAvailability().supportedLanguages.map(\.minimalIdentifier)
    }

    static func detectedLanguage(of text: String) -> Locale.Language? {
        guard let language = NLLanguageRecognizer.dominantLanguage(for: text) else { return nil }
        return Locale.Language(identifier: language.rawValue)
    }
}
