import Foundation
import NaturalLanguage
@preconcurrency import Translation

/// Experiment (Translate what I say): speak one language, insert another. Translation
/// runs on the iPhone with Apple's Translation framework once the language pack is
/// downloaded in Hex; with no pack, the original transcript goes in unchanged.
struct DictationTranslationSettings {
    private enum Key {
        static let enabled = "experiments.translateDictation"
        static let target = "experiments.translateDictation.target"
        static let pastesTranslation = "experiments.translateDictation.pastesTranslation"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Key.enabled) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.enabled) }
    }

    /// BCP 47 language code, such as `ja`.
    var targetLanguage: String {
        get { defaults?.string(forKey: Key.target) ?? "ja" }
        nonmutating set { defaults?.set(newValue, forKey: Key.target) }
    }

    /// The switch where you dictate: paste translated (the default) or as spoken. It keeps
    /// the last choice for the dictations after it.
    var pastesTranslation: Bool {
        get { defaults?.object(forKey: Key.pastesTranslation) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Key.pastesTranslation) }
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

enum DictationTranslator {
    /// The translated text, or nil when the experiment is off, the switch says to paste as
    /// spoken, the speech is already in the target language, or the pack isn't downloaded.
    /// `source` is the dictation language; without one the text's language is guessed.
    @MainActor
    static func translateIfEnabled(
        _ text: String,
        source: Locale.Language? = nil,
        settings: DictationTranslationSettings = DictationTranslationSettings()
    ) async -> String? {
        guard settings.isEnabled, settings.pastesTranslation, #available(iOS 26.0, *) else { return nil }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let source = source ?? detectedLanguage(of: trimmed) else { return nil }
        let target = Locale.Language(identifier: settings.targetLanguage)
        guard source.languageCode != target.languageCode else { return nil }
        guard await LanguageAvailability().status(from: source, to: target) == .installed else { return nil }
        do {
            let session = TranslationSession(installedSource: source, target: target)
            let response = try await session.translate(trimmed)
            let translated = response.targetText.trimmingCharacters(in: .whitespacesAndNewlines)
            return translated.isEmpty ? nil : translated
        } catch {
            return nil
        }
    }

    static func detectedLanguage(of text: String) -> Locale.Language? {
        guard let language = NLLanguageRecognizer.dominantLanguage(for: text) else { return nil }
        return Locale.Language(identifier: language.rawValue)
    }
}
