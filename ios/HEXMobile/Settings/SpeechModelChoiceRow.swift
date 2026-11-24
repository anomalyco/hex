import SwiftUI

/// Settings › Dictation › Model: the speech model the keyboard dictates with, English or
/// Multilingual. Multilingual is `MultilingualParakeetSettings` (the setting Experiments had),
/// so Hex's own dictation follows it too. English unless picked.
enum SpeechModelChoice: String, CaseIterable, Hashable {
    case english
    case multilingual

    static var current: SpeechModelChoice {
        MultilingualParakeetSettings().isEnabled ? .multilingual : .english
    }

    var label: String {
        switch self {
        case .english: String(localized: "English")
        case .multilingual: String(localized: "Multilingual")
        }
    }

    /// What the choice means, before anything downloads: its languages and its download.
    @MainActor
    var detail: String {
        switch self {
        case .english:
            String(localized: "For English. A \(KeyboardParakeetDownload.format(KeyboardParakeetDownload.bytes)) download, once, on Wi-Fi.")
        case .multilingual:
            String(localized: "25 languages, even mixed in one sentence: \(Self.multilingualLanguages). A \(ParakeetUltraTranscriber.downloadSize) download, once, as soon as you pick it. Hex dictates them with it too.")
        }
    }

    /// The multilingual model's 25 languages by name, in the iPhone's language.
    static var multilingualLanguages: String {
        let names = MultilingualParakeetSettings.languages.compactMap { Locale.current.localizedString(forLanguageCode: $0) }
        return ListFormatter.localizedString(byJoining: names.sorted { $0.localizedStandardCompare($1) == .orderedAscending })
    }

    /// Picked in Settings: Multilingual downloads at once beside the engine Hex dictates with;
    /// English stops that download (a complete one stays for a switch back) and gets English's
    /// model as Speech model in keyboard does.
    @MainActor
    func select() {
        guard self != Self.current else { return }
        MultilingualParakeetSettings().isEnabled = self == .multilingual
        DictationController.shared.multilingualParakeetChanged()
        DictationController.shared.startKeyboardParakeetDownloadIfWanted()
    }
}

/// The Model row in the speech model card: English · Multilingual, what the picked one means,
/// and that a downloaded model stays.
struct SpeechModelChoiceRow: View {
    let choice: SpeechModelChoice
    let onSelect: (SpeechModelChoice) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 12) {
                SettingsRowLabel(title: String(localized: "Model"))
                Spacer(minLength: 8)
                SettingsSegments(
                    items: SpeechModelChoice.allCases,
                    selected: choice,
                    label: \.label,
                    onSelect: onSelect
                )
            }
            Text(choice.detail)
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("speech-model-choice-detail")
            Text("Other languages use Apple's speech recognition. A downloaded model stays, so switching back is instant.")
                .font(.system(size: 12))
                .foregroundStyle(HexTheme.textFaint)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }
}
