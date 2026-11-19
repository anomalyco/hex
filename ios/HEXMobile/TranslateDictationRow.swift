import SwiftUI
@preconcurrency import Translation

/// Experiments › Translate what I say: the toggle, the target language, and the
/// one-time download of Apple's on-device language pack.
struct TranslateDictationRow: View {
    @State private var enabled = DictationTranslationSettings().isEnabled
    @State private var target = DictationTranslationSettings().targetLanguage
    @State private var languages: [Locale.Language] = []
    @State private var status: LanguageAvailability.Status?
    @State private var download: TranslationSession.Configuration?

    private var source: Locale.Language { Locale.current.language }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle(isOn: $enabled) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Translate what I say")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    Text("Speak your language, insert another, translated on this iPhone")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }
            .tint(HexTheme.success)
            .onChange(of: enabled) { _, value in
                DictationTranslationSettings().isEnabled = value
            }

            if enabled {
                HStack {
                    Text("Insert in")
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.textMuted)
                    Spacer()
                    Picker("Insert in", selection: $target) {
                        ForEach(languages, id: \.minimalIdentifier) { language in
                            Text(name(of: language)).tag(language.minimalIdentifier)
                        }
                    }
                    .pickerStyle(.menu)
                }
                .onChange(of: target) { _, value in
                    DictationTranslationSettings().targetLanguage = value
                    Task { await refreshStatus() }
                }

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
        .translationTask(download) { session in
            try? await session.prepareTranslation()
            await refreshStatus()
        }
        .task {
            let supported = await LanguageAvailability().supportedLanguages
            languages = supported
                .filter { $0.languageCode != source.languageCode }
                .sorted { name(of: $0) < name(of: $1) }
            await refreshStatus()
        }
    }

    private func refreshStatus() async {
        status = await LanguageAvailability().status(from: source, to: Locale.Language(identifier: target))
    }

    private func name(of language: Locale.Language) -> String {
        Locale.current.localizedString(forIdentifier: language.minimalIdentifier) ?? language.minimalIdentifier
    }
}
