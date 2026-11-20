import SwiftUI

/// Settings › Dictation language: Automatic, the first iPhone language Apple Speech
/// transcribes, or any language `SpeechTranscriber` or `DictationTranscriber` has. Shown
/// on iOS 26, where Hex dictates with Apple Speech; before it, Parakeet is English only.
struct DictationLanguageRow: View {
    @Bindable var dictation: DictationController
    @State private var options: [SpeechLanguageOption] = []
    @State private var automaticName: String?

    var body: some View {
        if #available(iOS 26.0, *) {
            VStack(spacing: 0) {
                LanguageRowDivider()
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Dictation language")
                            .font(.system(size: 15, weight: .regular))
                            .foregroundStyle(HexTheme.textBase)
                        if dictation.dictationLanguage == nil, let automaticName {
                            Text(automaticName)
                                .font(.system(size: 12))
                                .foregroundStyle(HexTheme.textMuted)
                                .lineLimit(1)
                        }
                    }
                    Spacer(minLength: 8)
                    Menu {
                        Button {
                            dictation.setDictationLanguage(nil)
                        } label: {
                            if dictation.dictationLanguage == nil {
                                Label("Automatic", systemImage: "checkmark")
                            } else {
                                Text("Automatic")
                            }
                        }
                        Divider()
                        ForEach(options) { option in
                            Button {
                                dictation.setDictationLanguage(option.id)
                            } label: {
                                if dictation.dictationLanguage == option.id {
                                    Label(option.name, systemImage: "checkmark")
                                } else {
                                    Text(option.name)
                                }
                            }
                        }
                    } label: {
                        LanguageMenuValue(value: selectedName)
                    }
                }
                .padding(.horizontal, 16)
                .frame(minHeight: 48)
            }
            .task {
                // The keyboard may have switched the language while Hex was away.
                dictation.followDictationLanguageSetting()
                options = await AppleSpeechLanguages.options()
                automaticName = await AppleSpeechLanguages.automatic().map {
                    SpeechLanguagePicker.displayName(for: $0.identifier)
                }
            }
        }
    }

    private var selectedName: String {
        guard let id = dictation.dictationLanguage else { return "Automatic" }
        return SpeechLanguagePicker.shortNames(for: options.map(\.id))[id]
            ?? SpeechLanguagePicker.displayName(for: id)
    }
}

/// The settings card's row divider.
private struct LanguageRowDivider: View {
    var body: some View {
        Rectangle()
            .fill(HexTheme.borderWeak)
            .frame(height: 0.5)
            .padding(.leading, 16)
    }
}

/// A menu's current value with the up-down chevron, as the settings card's other menus show it.
private struct LanguageMenuValue: View {
    let value: String

    var body: some View {
        HStack(spacing: 5) {
            Text(value)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(HexTheme.textMuted)
                .lineLimit(1)
            Image(systemName: "chevron.up.chevron.down")
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
        }
        .padding(.vertical, 6)
        .contentShape(Rectangle())
    }
}
