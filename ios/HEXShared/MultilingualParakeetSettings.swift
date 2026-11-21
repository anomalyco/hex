import Foundation

/// Experiments › Multilingual model (Parakeet), in the shared App Group so the keyboard reads
/// it too. Off by default. While it's on, Hex dictates its 25 languages with Parakeet Ultra
/// (`ParakeetUltraTranscriber`), which hears them mixed within a sentence (Dutch with English
/// words), and the Hex key hands its dictations to Hex, as before iOS 26: the model can't load
/// within a keyboard's memory limit.
nonisolated struct MultilingualParakeetSettings {
    static let enabledKey = "experiments.multilingualParakeet"
    static let launchArgument = "--experiment-multilingual-parakeet"

    /// Parakeet Ultra's languages (ISO 639-1), those of NVIDIA Parakeet TDT 0.6B v3.
    static let languages: Set<String> = [
        "bg", "cs", "da", "de", "el", "en", "es", "et", "fi", "fr", "hr", "hu", "it",
        "lt", "lv", "mt", "nl", "pl", "pt", "ro", "ru", "sk", "sl", "sv", "uk",
    ]

    /// As Parakeet V2: 6 GB iPhones report about 5.5 GB, 4 GB ones (iPhone 12, 13) under 4 GB.
    static let minimumPhysicalMemory: UInt64 = 5 << 30

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Self.enabledKey) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }

    /// Whether this iPhone has the memory to load it at all.
    static func fits(physicalMemory: UInt64 = ProcessInfo.processInfo.physicalMemory) -> Bool {
        physicalMemory >= minimumPhysicalMemory
    }

    static func supports(_ language: String?) -> Bool {
        guard let language, let code = Locale(identifier: language).language.languageCode?.identifier else {
            return false
        }
        return languages.contains(code)
    }

    /// The language (BCP 47) Parakeet Ultra dictates in, or nil while it doesn't dictate: the
    /// experiment off, an iPhone without the memory, or a language it doesn't know, which
    /// stays with the engine Hex uses without it. The one set, else the keyboard's language
    /// Automatic follows, else the iPhone's first language.
    func language(
        set: String? = LanguageSettings().dictationLanguage,
        keyboard: String? = LanguageSettings().keyboardDictationLanguage(),
        preferred: [String] = Locale.preferredLanguages,
        physicalMemory: UInt64 = ProcessInfo.processInfo.physicalMemory
    ) -> String? {
        guard isEnabled, Self.fits(physicalMemory: physicalMemory) else { return nil }
        guard let wanted = set ?? keyboard ?? preferred.first else { return nil }
        return Self.supports(wanted) ? wanted : nil
    }

    /// Simulator scenes pass `--experiment-multilingual-parakeet` to turn the experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
    }
}
