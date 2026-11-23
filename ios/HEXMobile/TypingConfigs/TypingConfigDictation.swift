import Foundation

/// The active typing config's dictation fixes for Hex's own dictation (a dictation the keyboard
/// hands to Hex, and the Action Button), applied after Learn how I type's as in the keyboard
/// (`TypingConfigActive.dictationCorrections`); never to a dictation Hex translates, whose
/// words are another language's.
enum TypingConfigDictation {
    @MainActor
    static func corrections(translating: Bool, settings: TypingConfigSettings = TypingConfigSettings()) -> [DictationCorrection] {
        guard !translating, settings.isEnabled else { return [] }
        // The app reads the App Group itself, so it needs no Full Access.
        return TypingConfigActive.dictationCorrections(fullAccess: true)
    }
}
