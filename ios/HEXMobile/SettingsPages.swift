import SwiftUI

/// Settings › Dictation: how Hex writes what you say, and the microphone.
struct DictationSettingsPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Bindable var dictation: DictationController
    @State private var cleanupTagger = CleanupTaggerSettings().isEnabled
    @State private var customVocabulary = CustomVocabularySettings().isEnabled

    var body: some View {
        SettingsPageScaffold(title: "Dictation") {
            SettingsCard {
                HStack(spacing: 12) {
                    SettingsRowLabel(title: String(localized: "Style"))
                    Spacer(minLength: 8)
                    Menu {
                        ForEach(DictationStyle.allCases, id: \.self) { candidate in
                            Button {
                                dictation.setStyle(candidate)
                            } label: {
                                if dictation.style == candidate {
                                    Label(candidate.label, systemImage: "checkmark")
                                } else {
                                    Text(candidate.label)
                                }
                            }
                        }
                    } label: {
                        SettingsMenuValue(value: dictation.style.label)
                    }
                }
                .padding(.horizontal, 16)
                .frame(minHeight: 48)

                DictationLanguageRow(dictation: dictation)
            }

            SettingsCard {
                TranslateDictationRow()
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .frame(minHeight: 50)

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Clean up what I say"),
                    detail: String(localized: "Drops ums, repeats and corrected words"),
                    isOn: $cleanupTagger
                )

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Custom vocabulary"),
                    detail: String(localized: "Spell names and terms your way"),
                    isOn: $customVocabulary
                )

                if customVocabulary {
                    CustomVocabularyEditor()
                        .padding(.horizontal, 16)
                        .padding(.top, 2)
                        .padding(.bottom, 14)
                }
            }

            SettingsCard {
                HStack(spacing: 12) {
                    SettingsRowLabel(title: String(localized: "Keep mic warm"))
                    Spacer(minLength: 8)
                    SettingsSegments(
                        items: WarmSessionDuration.allCases,
                        selected: dictation.warmDuration,
                        label: \.shortLabel,
                        onSelect: { duration in
                            withAnimation(HexMotion.settleAnimation(reduceMotion: reduceMotion)) {
                                dictation.selectWarmDuration(duration)
                            }
                        }
                    )
                }
                .padding(.horizontal, 16)
                .frame(minHeight: 50)

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Always use iPhone microphone"),
                    detail: String(localized: "Keeps music playing and starts faster"),
                    isOn: Binding(
                        get: { !dictation.allowBluetoothMic },
                        set: { dictation.setAllowBluetoothMic(!$0) }
                    )
                )
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: customVocabulary)
        .onChange(of: cleanupTagger) { _, enabled in
            CleanupTaggerSettings().isEnabled = enabled
        }
        .onChange(of: customVocabulary) { _, enabled in
            CustomVocabularySettings().isEnabled = enabled
        }
        .onAppear {
            cleanupTagger = CleanupTaggerSettings().isEnabled
            customVocabulary = CustomVocabularySettings().isEnabled
        }
    }
}

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

/// Settings › Keyboard: Full or Dictation bar, the full keyboard's rows (only while it's
/// chosen; Suggest swear words off by default, as Apple's keyboard never offers them), and
/// haptics, which the dictation bar uses too.
struct KeyboardSettingsPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var fullKeyboard = FullKeyboardSettings().isEnabled
    @State private var autocorrect = FullKeyboardSettings().autocorrect
    @State private var suggestSwearWords = FullKeyboardSettings().suggestSwearWords
    @State private var keyPopups = FullKeyboardSettings().keyPopups
    @State private var haptics = FullKeyboardSettings().haptics
    @State private var practiceText = ""

    var body: some View {
        SettingsPageScaffold(title: "Keyboard") {
            SettingsCard {
                HStack(spacing: 12) {
                    SettingsRowLabel(title: String(localized: "Keyboard"))
                    Spacer(minLength: 8)
                    SettingsSegments(
                        items: KeyboardChoice.allCases,
                        selected: fullKeyboard ? .full : .dictationBar,
                        label: \.label,
                        onSelect: { choice in
                            fullKeyboard = choice == .full
                        }
                    )
                }
                .padding(.horizontal, 16)
                .frame(minHeight: 50)

                if fullKeyboard {
                    KeyboardLayoutRow()

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Autocorrect"),
                        detail: String(localized: "Fixes typos when you press space"),
                        isOn: $autocorrect
                    )

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Suggest swear words"),
                        detail: String(localized: "Slurs are never suggested"),
                        isOn: $suggestSwearWords
                    )

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Key pop-ups"),
                        detail: String(localized: "Shows the letter above your finger"),
                        isOn: $keyPopups
                    )
                }

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Haptics"),
                    detail: String(localized: "A light tap as you press Hex's keys"),
                    isOn: $haptics
                )
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
        .onChange(of: fullKeyboard) { _, enabled in
            FullKeyboardSettings().isEnabled = enabled
        }
        .onChange(of: autocorrect) { _, enabled in
            FullKeyboardSettings().autocorrect = enabled
        }
        .onChange(of: suggestSwearWords) { _, enabled in
            FullKeyboardSettings().suggestSwearWords = enabled
        }
        .onChange(of: keyPopups) { _, enabled in
            FullKeyboardSettings().keyPopups = enabled
        }
        .onChange(of: haptics) { _, enabled in
            FullKeyboardSettings().haptics = enabled
        }
        .onAppear {
            fullKeyboard = FullKeyboardSettings().isEnabled
            autocorrect = FullKeyboardSettings().autocorrect
            suggestSwearWords = FullKeyboardSettings().suggestSwearWords
            keyPopups = FullKeyboardSettings().keyPopups
            haptics = FullKeyboardSettings().haptics
        }
    }
}

/// Settings › Privacy: what Hex keeps about how you type, all of it on this iPhone, and
/// under Advanced the typing logs you can record to export.
struct PrivacySettingsPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var learnTyping = TypingJournalSettings().isEnabled
    @State private var typingLogs = TypingLogSettings().isEnabled
    @State private var showTypingLogs = false
    @State private var confirmingTypingLogs = SettingsSnapshotFixture.confirmsTypingLogs

    var body: some View {
        SettingsPageScaffold(title: "Privacy") {
            HStack(spacing: 7) {
                Image(systemName: "lock.fill")
                    .font(.system(size: 11, weight: .medium))
                Text("Everything stays on this iPhone")
                    .font(.system(size: 13))
            }
            .foregroundStyle(HexTheme.textMuted)
            .padding(.horizontal, 16)
            .padding(.bottom, 2)

            SettingsCard {
                // Not only for the full keyboard: it also learns from dictations the
                // dictation bar inserts.
                SettingsToggleRow(
                    title: String(localized: "Learn how I type"),
                    detail: String(localized: "Learns from taps and the words you fix"),
                    isOn: $learnTyping
                )

                if learnTyping {
                    SettingsDivider()
                    TypingInsightsView()
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                }
            }

            SettingsSectionHeader(title: "Advanced")

            SettingsCard {
                // Apart from Learn how I type: this keeps everything, to export and train on.
                // Turning it on asks first (App Review 2.5.14); turning it off doesn't.
                SettingsToggleRow(
                    title: String(localized: "Record typing logs"),
                    detail: String(localized: "Records taps and the text you type for 14 days, on this iPhone, to export. Off by default."),
                    isOn: Binding(
                        get: { typingLogs },
                        set: { on in
                            if on {
                                confirmingTypingLogs = true
                            } else {
                                typingLogs = false
                            }
                        }
                    )
                )
                .alert("Record typing logs?", isPresented: $confirmingTypingLogs) {
                    Button("Cancel", role: .cancel) {}
                    Button("Turn On") {
                        typingLogs = true
                    }
                } message: {
                    Text("Hex's keyboard will record each tap and the text you type or dictate with it, including the text around the cursor, for 14 days. The logs stay on this iPhone for you to export or delete. Password, code and card fields are never recorded.")
                }

                if typingLogs {
                    SettingsDivider()
                    Button {
                        showTypingLogs = true
                    } label: {
                        SettingsLinkLabel(title: String(localized: "Typing logs"))
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: learnTyping)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: typingLogs)
        .sheet(isPresented: $showTypingLogs) {
            TypingLogsView()
        }
        .onChange(of: learnTyping) { _, enabled in
            TypingJournalSettings().isEnabled = enabled
        }
        .onChange(of: typingLogs) { _, enabled in
            TypingLogSettings().isEnabled = enabled
        }
        .onAppear {
            learnTyping = TypingJournalSettings().isEnabled
            typingLogs = TypingLogSettings().isEnabled
        }
    }
}

/// Settings › Experiments: unfinished features, off until turned on. They live in the App
/// Group so the keyboard reads them too.
struct ExperimentsSettingsPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var emoji = FullKeyboardSettings().emoji
    @State private var emojiHistoryEmpty = EmojiUsageStore().isEmpty
    @State private var spatialAutocorrect = FullKeyboardSettings().spatialAutocorrect
    @State private var appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
    @State private var cleanupAvailability = AppleIntelligenceCleanupAvailability.current

    var body: some View {
        SettingsPageScaffold(title: "Experiments") {
            SettingsCard {
                EmojiExperimentRows(emoji: $emoji, historyEmpty: $emojiHistoryEmpty)

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Spatial autocorrect"),
                    detail: FullKeyboardSettings().isEnabled && FullKeyboardSettings().autocorrect
                        ? String(localized: "Reads where your taps land to fix typos, in English and Spanish")
                        : String(localized: "Needs the full keyboard with Autocorrect on"),
                    isOn: $spatialAutocorrect
                )

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Clean up with Apple Intelligence"),
                    detail: cleanupAvailability.reason ?? String(localized: "Fixes misheard words on device"),
                    isOn: $appleIntelligenceCleanup
                )
                .disabled(!cleanupAvailability.isAvailable)
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: emoji)
        .onChange(of: appleIntelligenceCleanup) { _, enabled in
            AppleIntelligenceCleanupSettings().isEnabled = enabled
        }
        .onChange(of: spatialAutocorrect) { _, enabled in
            FullKeyboardSettings().spatialAutocorrect = enabled
        }
        .onAppear {
            emoji = FullKeyboardSettings().emoji
            emojiHistoryEmpty = EmojiUsageStore().isEmpty
            spatialAutocorrect = FullKeyboardSettings().spatialAutocorrect
            appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
            cleanupAvailability = AppleIntelligenceCleanupAvailability.current
        }
    }
}

/// Experiments › Emoji: the full keyboard's emoji key, and while it's on, Clear emoji history.
private struct EmojiExperimentRows: View {
    @Binding var emoji: Bool
    @Binding var historyEmpty: Bool

    var body: some View {
        SettingsToggleRow(
            title: String(localized: "Emoji"),
            detail: FullKeyboardSettings().isEnabled
                ? String(localized: "An emoji key with suggestions as you type")
                : String(localized: "Needs the full keyboard"),
            isOn: $emoji
        )
        .onChange(of: emoji) { _, enabled in
            FullKeyboardSettings().emoji = enabled
        }

        if emoji {
            SettingsDivider()
            Button {
                EmojiUsageStore().clear()
                historyEmpty = true
            } label: {
                Text("Clear emoji history")
                    .font(.system(size: 15, weight: .regular))
                    .foregroundStyle(historyEmpty ? HexTheme.textFaint : HexTheme.textBase)
                    .frame(maxWidth: .infinity, minHeight: 50, alignment: .leading)
                    .padding(.horizontal, 16)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .disabled(historyEmpty)
        }
    }
}
