import SwiftUI
@preconcurrency import Translation

/// Settings › Dictation › Translate what I say: the toggle, the target language, the switch the
/// keyboard's menus also have (paste translated or as spoken), and the one-time download
/// of Apple's on-device language pack.
struct TranslateDictationRow: View {
    @State private var enabled = DictationTranslationSettings().isEnabled
    @State private var target = DictationTranslationSettings().targetLanguage
    @State private var pastesTranslation = DictationTranslationSettings().pastesTranslation
    @State private var languages: [Locale.Language] = []
    @State private var status: LanguageAvailability.Status?
    @State private var download: TranslationSession.Configuration?

    /// What you dictate in: Settings › Dictation language, or the language it last resolved.
    private var source: Locale.Language {
        LanguageSettings().effectiveDictationLanguage.map { Locale(identifier: $0).language }
            ?? Locale.current.language
    }

    /// "Paste in English" for what you dictate in, or nil when that is the target already.
    private var pasteChoice: TranslationPasteChoice? {
        TranslationPasteChoice(
            target: target,
            source: LanguageSettings().effectiveDictationLanguage ?? Locale.current.identifier
        )
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle(isOn: $enabled) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Translate what I say")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    Text("Speak one language, insert another")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }
            .tint(HexTheme.success)
            .onChange(of: enabled) { _, value in
                DictationTranslationSettings().isEnabled = value
                // Turning it on means translating, whatever the switch said last time.
                if value {
                    pastesTranslation = true
                }
            }

            if enabled {
                HStack {
                    Text("Insert in")
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.textMuted)
                    Spacer()
                    Menu {
                        ForEach(languages, id: \.minimalIdentifier) { language in
                            Button {
                                target = language.minimalIdentifier
                            } label: {
                                if target == language.minimalIdentifier {
                                    Label(name(of: language), systemImage: "checkmark")
                                } else {
                                    Text(name(of: language))
                                }
                            }
                        }
                    } label: {
                        SettingsMenuValue(value: name(of: Locale.Language(identifier: target)))
                    }
                    .accessibilityLabel("Insert in")
                }
                .onChange(of: target) { _, value in
                    DictationTranslationSettings().targetLanguage = value
                    Task { await refreshStatus() }
                }

                if let pasteChoice {
                    Toggle(isOn: $pastesTranslation) {
                        Text(pasteChoice.translatedTitle)
                            .font(.system(size: 13))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                    .tint(HexTheme.success)
                    .onChange(of: pastesTranslation) { _, value in
                        DictationTranslationSettings().pastesTranslation = value
                    }
                }

                if Locale.Language(identifier: target).languageCode == source.languageCode {
                    // You dictate in the target already: switch the dictation language from the
                    // keyboard's Hex key to speak another one and paste in this one.
                    Text("Switch the language from the Hex key to speak another one.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                } else {
                switch status {
                case .installed:
                    Text("Ready. Works offline.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                case .supported:
                    Button("Download \(name(of: Locale.Language(identifier: target)))") {
                        download = TranslationSession.Configuration(
                            source: source,
                            target: Locale.Language(identifier: target)
                        )
                    }
                    .font(.system(size: 13, weight: .medium))
                case .unsupported:
                    Text("Apple can't translate between these languages.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                default:
                    EmptyView()
                }
                }
            }
        }
        .translationTask(download) { session in
            try? await session.prepareTranslation()
            await refreshStatus()
        }
        .onAppear(perform: reload)
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.willEnterForegroundNotification)) { _ in
            reload()
        }
        .task {
            // Every language, including the one you dictate in now: you can switch the
            // dictation language per dictation, so English must be pickable while you
            // usually dictate in English. A dictation already in the target pastes as spoken.
            let supported = await LanguageAvailability().supportedLanguages
            var seen = Set<String>()
            languages = supported
                .filter { seen.insert($0.minimalIdentifier).inserted }
                .sorted { name(of: $0) < name(of: $1) }
            await refreshStatus()
        }
    }

    /// The keyboard's Translate to <language> switch and its targets change these too.
    private func reload() {
        let settings = DictationTranslationSettings()
        enabled = settings.isEnabled
        target = settings.targetLanguage
        pastesTranslation = settings.pastesTranslation
    }

    private func refreshStatus() async {
        status = await LanguageAvailability().status(from: source, to: Locale.Language(identifier: target))
    }

    private func name(of language: Locale.Language) -> String {
        Locale.current.localizedString(forIdentifier: language.minimalIdentifier) ?? language.minimalIdentifier
    }
}