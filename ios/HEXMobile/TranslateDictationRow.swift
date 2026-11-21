import SwiftUI
@preconcurrency import Translation

/// Settings › Dictation › Translate what I say: the switch (the keyboards' Translate to
/// <language> is the same one), the target language, the one-time download of Apple's
/// on-device language pack, and why the last dictation went in as spoken, with its fix. The
/// keyboard's note after such a dictation opens this page.
struct TranslateDictationRow: View {
    @State private var enabled = DictationTranslationSettings().isEnabled
    @State private var target = DictationTranslationSettings().targetLanguage
    @State private var lastFailure = DictationTranslationSettings().lastFailure
    @State private var languages: [Locale.Language] = []
    @State private var status: LanguageAvailability.Status?
    @State private var download: TranslationSession.Configuration?

    /// What you dictate in: the language of the dictation that last went in as spoken, else
    /// Settings › Dictation language, or the language it last resolved.
    private var source: Locale.Language {
        if let lastFailure, lastFailure.target == Locale.Language(identifier: target).minimalIdentifier {
            return Locale.Language(identifier: lastFailure.source)
        }
        return LanguageSettings().effectiveDictationLanguage.map { Locale(identifier: $0).language }
            ?? Locale.current.language
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
                let settings = DictationTranslationSettings()
                if settings.isEnabled != value { settings.isEnabled = value }
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

                if Locale.Language(identifier: target).languageCode == source.languageCode {
                    // You dictate in the target already: switch the dictation language from the
                    // keyboard's Hex key to speak another one and paste in this one.
                    Text("Switch the language from the Hex key to speak another one.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                } else {
                switch status {
                case .installed where lastFailure?.reason == .needsHexOpen:
                    // Before iOS 26 the session lives in Hex's window (TranslationSessionHost).
                    Text("iOS 18 couldn't translate the last dictation while Hex was in the background. Dictations you finish in Hex translate.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                case .installed where lastFailure?.reason == .failed:
                    // Apple's own pack is there but translating failed, as when its Translate
                    // app was removed.
                    Text("Couldn't translate the last dictation. If it keeps happening, reinstall Apple's Translate app.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                    downloadButton(again: true)
                case .installed:
                    Text("Ready. Works offline.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                case .supported:
                    if lastFailure != nil {
                        Text(TranslationFailure.note)
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                    downloadButton(again: false)
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
            // The keyboard may have changed the switch or failed to translate, and the pack
            // may have been removed in iOS Settings meanwhile.
            reload()
            Task { await refreshStatus() }
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
        lastFailure = settings.lastFailure
    }

    /// Asks Apple to download the pair. Asking again for the same pair invalidates the last
    /// request, or `translationTask` would not run it again after a cancelled download.
    private func downloadButton(again: Bool) -> some View {
        let language = name(of: Locale.Language(identifier: lastFailure?.language ?? target))
        return Button(again ? "Download \(language) again" : "Download \(language)") {
            let configuration = TranslationSession.Configuration(source: source, target: Locale.Language(identifier: target))
            if download == configuration {
                download?.invalidate()
            } else {
                download = configuration
            }
        }
        .font(.system(size: 13, weight: .medium))
    }

    private func refreshStatus() async {
        let current = await LanguageAvailability().status(from: source, to: Locale.Language(identifier: target))
        status = current
        // Downloaded now: the note about a missing pack is out of date.
        if current == .installed, lastFailure?.reason == .notDownloaded {
            DictationTranslationSettings().lastFailure = nil
            lastFailure = nil
        }
    }

    private func name(of language: Locale.Language) -> String {
        Locale.current.localizedString(forIdentifier: language.minimalIdentifier) ?? language.minimalIdentifier
    }
}