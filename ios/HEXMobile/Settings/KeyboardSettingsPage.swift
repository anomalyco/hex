import SwiftUI
import UIKit

/// Which keyboard Hex shows: the full keyboard (letters, with a Hex key to dictate) or the
/// dictation bar. Stored as `FullKeyboardSettings.isEnabled`.
private enum KeyboardChoice: CaseIterable {
    case full
    case dictationBar

    var label: String {
        switch self {
        case .full: String(localized: "Full")
        case .dictationBar: String(localized: "Dictation bar")
        }
    }
}

/// Settings › Keyboard › Auto-start, while the dictation bar is chosen.
private extension DictationBarAutoStart {
    var label: String {
        switch self {
        case .never: String(localized: "Never")
        case .whenSwitching: String(localized: "On switch")
        case .always: String(localized: "Always")
        }
    }

    var detail: String {
        switch self {
        case .never: String(localized: "Tap Dictate to start")
        case .whenSwitching: String(localized: "When you switch to Hex")
        case .always: String(localized: "Each time the dictation bar opens")
        }
    }
}

private extension KeyHapticStrength {
    var label: String {
        switch self {
        case .light: String(localized: "Light")
        case .medium: String(localized: "Medium")
        case .strong: String(localized: "Strong")
        }
    }
}

private enum HexKeySide: CaseIterable {
    case left
    case right

    var label: String {
        switch self {
        case .left: String(localized: "Left")
        case .right: String(localized: "Right")
        }
    }
}

/// Settings › Keyboard: grouped by task into Layout, Typing, and Feedback so the page scans
/// cleanly instead of one 11-row card.
struct KeyboardSettingsPage: View {
    /// `KeyboardSetupMonitor.fullAccessOn`.
    let fullAccessOn: Bool
    var needsAllowPasteSetting = false
    var onDismissAllowPasteSetting: () -> Void = {}
    @Environment(\.openURL) private var openURL
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var revision = 0
    @State private var emojiHistoryEmpty = EmojiUsageStore().isEmpty
    @State private var confirmingClearEmoji = false
    @State private var practiceText = ""

    private var settings: FullKeyboardSettings { FullKeyboardSettings() }

    private var fullKeyboard: Bool {
        _ = revision
        return settings.isEnabled
    }

    private var autocorrect: Bool {
        _ = revision
        return settings.autocorrect
    }

    private var haptics: Bool {
        _ = revision
        return settings.haptics
    }

    private var emoji: Bool {
        _ = revision
        return settings.emoji
    }

    private func binding<T>(_ keyPath: WritableKeyPath<FullKeyboardSettings, T>) -> Binding<T> {
        Binding(
            get: {
                _ = revision
                return FullKeyboardSettings()[keyPath: keyPath]
            },
            set: { value in
                var current = FullKeyboardSettings()
                current[keyPath: keyPath] = value
                revision += 1
            }
        )
    }

    /// The detail of a row that needs Full Access: none while it's on, and while it isn't, that
    /// the row needs it (as Swipe typing says it needs the full keyboard).
    private var fullAccessNeed: String? {
        fullAccessOn ? nil : String(localized: "Needs Full Access")
    }

    /// Swipe typing's detail: the layouts that swipe.
    private var swipeLanguages: String {
        _ = revision
        let current = FullKeyboardSettings()
        guard current.swipeMoreLanguages else { return String(localized: "English and Spanish only") }
        guard current.spatialAutocorrect, current.germanSpatialAutocorrect else {
            return String(localized: "English, Spanish, French and Polish")
        }
        return String(localized: "English, Spanish, French, Polish and German")
    }

    var body: some View {
        SettingsPageScaffold(title: "Keyboard") {
            if needsAllowPasteSetting {
                AllowPasteBanner(
                    openSettings: {
                        if let url = URL(string: UIApplication.openSettingsURLString) {
                            openURL(url)
                        }
                    },
                    dismiss: onDismissAllowPasteSetting
                )
            }

            // Layout: mode, languages, Hex key placement, number row.
            SettingsCard {
                HStack(spacing: 12) {
                    SettingsRowLabel(
                        title: String(localized: "Keyboard"),
                        detail: fullKeyboard ? nil : fullAccessNeed
                    )
                    Spacer(minLength: 8)
                    SettingsSegments(
                        items: KeyboardChoice.allCases,
                        selected: fullKeyboard ? .full : .dictationBar,
                        label: \.label,
                        onSelect: { choice in
                            FullKeyboardSettings().isEnabled = choice == .full
                            revision += 1
                        }
                    )
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .frame(minHeight: 50)

                if !fullKeyboard {
                    SettingsDivider()

                    let autoStart = { _ = revision; return settings.dictationBarAutoStart }()
                    HStack(spacing: 12) {
                        SettingsRowLabel(title: String(localized: "Auto-start"), detail: autoStart.detail)
                        Spacer(minLength: 8)
                        SettingsSegments(
                            items: DictationBarAutoStart.allCases,
                            selected: autoStart,
                            label: \.label,
                            onSelect: { choice in
                                FullKeyboardSettings().dictationBarAutoStart = choice
                                revision += 1
                            }
                        )
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .frame(minHeight: 50)
                }

                if fullKeyboard {
                    KeyboardLanguagesRow()

                    SettingsDivider()

                    let hexOnLeft = { _ = revision; return settings.hexKeyOnLeft }()
                    HStack(spacing: 12) {
                        SettingsRowLabel(title: String(localized: "Hex key"))
                        Spacer(minLength: 8)
                        SettingsSegments(
                            items: HexKeySide.allCases,
                            selected: hexOnLeft ? .left : .right,
                            label: \.label,
                            onSelect: { side in
                                FullKeyboardSettings().hexKeyOnLeft = side == .left
                                revision += 1
                            }
                        )
                    }
                    .padding(.horizontal, 16)
                    .frame(minHeight: 50)

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Number row"),
                        detail: String(localized: "Shows 1 to 0 above the letters"),
                        isOn: binding(\.numberRow)
                    )
                }
            }

            // Typing: autocorrect, swipe, vocabulary.
            if fullKeyboard {
                SettingsSectionHeader(title: "Typing")

                SettingsCard {
                    SettingsToggleRow(
                        title: String(localized: "Auto-capitalization"),
                        isOn: binding(\.autoCapitalization)
                    )

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Autocorrect"),
                        detail: String(localized: "Fixes typos when you press space"),
                        isOn: binding(\.autocorrect)
                    )

                    if autocorrect {
                        SettingsDivider()

                        SettingsToggleRow(
                            title: String(localized: "Spatial autocorrect"),
                            detail: String(localized: "English, Spanish, French and Polish only"),
                            isOn: binding(\.spatialAutocorrect)
                        )
                    }

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Swipe typing"),
                        detail: swipeLanguages,
                        isOn: binding(\.swipeTyping)
                    )

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Suggest swear words"),
                        isOn: binding(\.suggestSwearWords)
                    )

                    SettingsDivider()

                    KeyboardVocabularyRow()

                    // Settings › Experiments › Typing configs: the config in use and its history.
                    if TypingConfigSettings().isEnabled {
                        SettingsDivider()

                        KeyboardTypingConfigsRow(fullAccessOn: fullAccessOn)
                    }
                }
            }

            // Feedback & extras: key pop-ups, haptics, emoji.
            SettingsSectionHeader(title: "Feedback")

            SettingsCard {
                if fullKeyboard {
                    SettingsToggleRow(
                        title: String(localized: "Key pop-ups"),
                        isOn: binding(\.keyPopups)
                    )

                    SettingsDivider()
                }

                SettingsToggleRow(
                    title: String(localized: "Haptics"),
                    detail: fullAccessNeed,
                    isOn: binding(\.haptics)
                )

                if haptics {
                    SettingsDivider()

                    let strength = { _ = revision; return settings.hapticStrength }()
                    HStack(spacing: 12) {
                        SettingsRowLabel(title: String(localized: "Strength"))
                        Spacer(minLength: 8)
                        SettingsSegments(
                            items: KeyHapticStrength.allCases,
                            selected: strength,
                            label: \.label,
                            onSelect: { choice in
                                FullKeyboardSettings().hapticStrength = choice
                                revision += 1
                            }
                        )
                    }
                    .padding(.horizontal, 16)
                    .frame(minHeight: 50)
                }

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Emoji"),
                    detail: fullKeyboard
                        ? String(localized: "Emoji key and suggestions")
                        : String(localized: "An emoji key on the dictation bar"),
                    isOn: binding(\.emoji)
                )

                if emoji {
                    SettingsDivider()
                    Button {
                        confirmingClearEmoji = true
                    } label: {
                        Text("Clear emoji history")
                            .font(.system(size: 15, weight: .regular))
                            .foregroundStyle(emojiHistoryEmpty ? HexTheme.textFaint : HexTheme.danger)
                            .frame(maxWidth: .infinity, minHeight: 50, alignment: .leading)
                            .padding(.horizontal, 16)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .disabled(emojiHistoryEmpty)
                    .confirmationDialog("Clear emoji history?", isPresented: $confirmingClearEmoji, titleVisibility: .visible) {
                        Button("Clear", role: .destructive) {
                            EmojiUsageStore().clear()
                            emojiHistoryEmpty = true
                        }
                    }
                }
            }

            TextField("Test keyboard…", text: $practiceText, axis: .vertical)
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textBase)
                .lineLimit(1...4)
                .padding(.horizontal, 16)
                .padding(.vertical, 13)
                .background(
                    HexTheme.bgBase,
                    in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
                )
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: fullKeyboard)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: autocorrect)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: haptics)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: emoji)
        .onAppear {
            emojiHistoryEmpty = EmojiUsageStore().isEmpty
            revision += 1
        }
    }
}
