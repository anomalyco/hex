import SwiftUI
@preconcurrency import Translation

/// Settings › Dictation › Translate what I say: the switch (the keyboards' Translate to
/// <language> is the same one), the target language, the one-time download of Apple's
/// on-device language pack, and why the last dictation went in as spoken, with its fix. The
/// keyboard's note after such a dictation opens this page.
///
/// The pair it checks and downloads is the one the keyboard translates: from the language the
/// keyboard dictates in (`DictationTranslationSettings.keyboardSpokenLanguage`), never Hex's
/// own Automatic pick, into the target. Turning translation on or picking a target here asks
/// Apple for the pair's packs at once, with its own download sheet; opening the page only
/// shows where the pair stands, with the download button.
struct TranslateDictationRow: View {
    @State private var enabled = DictationTranslationSettings().isEnabled
    @State private var target = DictationTranslationSettings().targetLanguage
    @State private var lastFailure = DictationTranslationSettings().lastFailure
    @State private var languages: [Locale.Language] = []
    /// Apple's answer for `checked`, the pair it was asked about.
    @State private var status: LanguageAvailability.Status?
    @State private var checked: Pair?
    @State private var download: TranslationSession.Configuration?

    private struct Pair: Equatable {
        let source: Locale.Language
        let target: Locale.Language
    }

    /// What the keyboard dictates in (BCP 47), or nil until Hex knows.
    private var spoken: String? {
        DictationTranslationSettings().keyboardSpokenLanguage(
            languages: LanguageSettings(),
            iPhoneLanguages: DictationTranslationSettings.iPhoneLanguages
        )
    }

    /// The pair the keyboard translates, or nil while the spoken language is unknown or is the
    /// target's language already.
    private var pair: Pair? {
        guard let spoken else { return nil }
        let source = Locale(identifier: spoken).language
        let target = Locale.Language(identifier: target)
        guard source.languageCode != target.languageCode else { return nil }
        return Pair(source: source, target: target)
    }

    /// Why the last dictation went in as spoken, when it was this pair.
    private var failure: TranslationFailure? {
        guard let lastFailure, let pair,
              lastFailure.source == pair.source.minimalIdentifier,
              lastFailure.target == pair.target.minimalIdentifier else { return nil }
        return lastFailure
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Toggle(isOn: Binding(get: { enabled }, set: turn)) {
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

            if enabled {
                HStack {
                    Text("Translate to")
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.textMuted)
                    Spacer()
                    Menu {
                        ForEach(languages, id: \.minimalIdentifier) { language in
                            Button {
                                pick(language.minimalIdentifier)
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
                    .accessibilityLabel("Translate to")
                }

                state
            }
        }
        .translationTask(download) { session in
            try? await session.prepareTranslation()
            await check()
        }
        .onAppear(perform: reload)
        .onReceive(NotificationCenter.default.publisher(for: UIApplication.willEnterForegroundNotification)) { _ in
            // The keyboard may have changed the switch, dictated in another language or failed
            // to translate, and the pack may have been removed in iOS Settings meanwhile.
            reload()
            Task { await check() }
        }
        // A new dictation language picked above this row is a new pair to check.
        .task(id: spoken) { await check() }
        .task {
            // Every language, including the one you dictate in now: you can switch the
            // dictation language per dictation, so English must be pickable while you
            // usually dictate in English. A dictation already in the target pastes as spoken.
            let supported = await LanguageAvailability().supportedLanguages
            var seen = Set<String>()
            languages = supported
                .filter { seen.insert($0.minimalIdentifier).inserted }
                .sorted { name(of: $0) < name(of: $1) }
        }
    }

    /// Where the pair stands, said only for the pair Apple was asked about.
    @ViewBuilder private var state: some View {
        if spoken == nil {
            note("Dictate once with the Hex keyboard, then check back here.")
        } else if let pair {
            if checked == pair, let status {
                switch status {
                case .installed where failure?.reason == .needsHexOpen:
                    // Before iOS 26 the session lives in Hex's window (TranslationSessionHost).
                    note("On iOS 18, dictations translate only with Hex open.")
                case .installed where failure?.reason == .failed:
                    // Apple's own pack is there but translating failed, as when its Translate
                    // app was removed.
                    note("Couldn't translate the last dictation. If it keeps happening, reinstall Apple's Translate app.")
                    downloadButton(for: pair, again: true)
                case .installed:
                    note("\(name(of: pair.source)) to \(name(of: pair.target)) is ready. Works offline.")
                case .supported:
                    if failure != nil {
                        note(verbatim: TranslationFailure.note)
                    }
                    downloadButton(for: pair, again: false)
                case .unsupported:
                    note("Apple can't translate \(name(of: pair.source)) to \(name(of: pair.target)) on this iPhone. Pick another language.")
                default:
                    EmptyView()
                }
            }
        } else {
            // You dictate in the target already: switch the dictation language from the
            // keyboard's Hex key to speak another one and paste in this one.
            note("Change the spoken language from the Hex key.")
        }
    }

    private func note(_ text: LocalizedStringKey) -> some View {
        Text(text)
            .font(.system(size: 12))
            .foregroundStyle(HexTheme.textMuted)
    }

    private func note(verbatim text: String) -> some View {
        Text(verbatim: text)
            .font(.system(size: 12))
            .foregroundStyle(HexTheme.textMuted)
    }

    /// The switch, turned by the person: on, it asks Apple for the pair's packs.
    private func turn(_ on: Bool) {
        enabled = on
        let settings = DictationTranslationSettings()
        if settings.isEnabled != on { settings.isEnabled = on }
        if on { Task { await check(personActed: true) } }
    }

    /// A target picked by the person: it asks Apple for the new pair's packs.
    private func pick(_ language: String) {
        target = language
        DictationTranslationSettings().targetLanguage = language
        Task { await check(personActed: true) }
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
    private func requestDownload(_ pair: Pair) {
        let configuration = TranslationSession.Configuration(source: pair.source, target: pair.target)
        if download == configuration {
            download?.invalidate()
        } else {
            download = configuration
        }
    }

    /// Names the language that isn't English, which Apple pairs with most languages.
    private func downloadButton(for pair: Pair, again: Bool) -> some View {
        let language = name(of: pair.target.languageCode == .english ? pair.source : pair.target)
        return Button(again ? "Download \(language) again" : "Download \(language)") {
            requestDownload(pair)
        }
        .font(.system(size: 13, weight: .medium))
    }

    /// Asks Apple where the pair stands; after the person turned translation on or picked a
    /// target, a pair whose packs aren't here gets Apple's download sheet too.
    private func check(personActed: Bool = false) async {
        guard enabled, let pair else {
            status = nil
            checked = nil
            return
        }
        let current = await Self.availability(from: pair.source, to: pair.target)
        // The person may have picked another target meanwhile.
        guard pair == self.pair else { return }
        status = current
        checked = pair
        // Downloaded now: the note about a missing pack is out of date.
        if current == .installed, lastFailure?.reason == .notDownloaded {
            DictationTranslationSettings().lastFailure = nil
            lastFailure = nil
        }
        if DictationTranslationSettings.asksToDownload(current, afterPersonActed: personActed) {
            requestDownload(pair)
        }
    }

    private static func availability(from source: Locale.Language, to target: Locale.Language) async -> LanguageAvailability.Status {
        #if DEBUG && targetEnvironment(simulator)
        // The simulator's Translation reports every pair unsupported: `-hexTranslationStatus
        // installed|supported|unsupported` stands in for Apple's answer, for screenshots.
        switch UserDefaults.standard.string(forKey: "hexTranslationStatus") {
        case "installed": return .installed
        case "supported": return .supported
        case "unsupported": return .unsupported
        default: break
        }
        #endif
        return await LanguageAvailability().status(from: source, to: target)
    }

    private func name(of language: Locale.Language) -> String {
        Locale.current.localizedString(forIdentifier: language.minimalIdentifier) ?? language.minimalIdentifier
    }
}
