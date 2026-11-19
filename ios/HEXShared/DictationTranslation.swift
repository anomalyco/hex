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
}

enum DictationTranslator {
    /// The translated text, or nil when the experiment is off, the speech is already in
    /// the target language, or the pack isn't downloaded.
    @MainActor
    static func translateIfEnabled(
        _ text: String,
        settings: DictationTranslationSettings = DictationTranslationSettings()
    ) async -> String? {
        guard settings.isEnabled, #available(iOS 26.0, *) else { return nil }
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, let source = detectedLanguage(of: trimmed) else { return nil }
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
