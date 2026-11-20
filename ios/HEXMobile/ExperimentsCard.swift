import SwiftUI

/// Unfinished features, off until turned on here. They live in the App Group so the
/// keyboard reads them too.
struct ExperimentsCard: View {
    @State private var fullKeyboard = FullKeyboardSettings().isEnabled
    @State private var cleanupTagger = CleanupTaggerSettings().isEnabled
    @State private var appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
    @State private var customVocabulary = CustomVocabularySettings().isEnabled
    @State private var cleanupAvailability = AppleIntelligenceCleanupAvailability.current
    @State private var autocorrect = FullKeyboardSettings().autocorrect
    @State private var keyPopups = FullKeyboardSettings().keyPopups
    @State private var haptics = FullKeyboardSettings().haptics
    @State private var learnTyping = TypingJournalSettings().isEnabled
    @State private var typingLogs = TypingLogSettings().isEnabled
    @State private var showTypingLogs = false

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Experiments")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
                .padding(.leading, 16)

            VStack(spacing: 0) {
                Toggle(isOn: $fullKeyboard) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Full keyboard")
                            .font(.system(size: 15, weight: .regular))
                            .foregroundStyle(HexTheme.textBase)
                        Text("Type with Hex, with a Hex key to dictate")
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
                .tint(HexTheme.success)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .onChange(of: fullKeyboard) { _, enabled in
                    FullKeyboardSettings().isEnabled = enabled
                }

                if fullKeyboard {
                    Divider()
                        .overlay(HexTheme.borderWeak)
                        .padding(.leading, 16)

                    Toggle(isOn: $autocorrect) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Autocorrect")
                                .font(.system(size: 15, weight: .regular))
                                .foregroundStyle(HexTheme.textBase)
                            Text("Fix contractions, shortcuts, and one-edit typos on space")
                                .font(.system(size: 12))
                                .foregroundStyle(HexTheme.textMuted)
                        }
                    }
                    .tint(HexTheme.success)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .onChange(of: autocorrect) { _, enabled in
                        FullKeyboardSettings().autocorrect = enabled
                    }

                    Divider()
                        .overlay(HexTheme.borderWeak)
                        .padding(.leading, 16)

                    Toggle(isOn: $keyPopups) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Key pop-ups")
                                .font(.system(size: 15, weight: .regular))
                                .foregroundStyle(HexTheme.textBase)
                            Text("Show a bigger letter above the key you press")
                                .font(.system(size: 12))
                                .foregroundStyle(HexTheme.textMuted)
                        }
                    }
                    .tint(HexTheme.success)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .onChange(of: keyPopups) { _, enabled in
                        FullKeyboardSettings().keyPopups = enabled
                    }

                    Divider()
                        .overlay(HexTheme.borderWeak)
                        .padding(.leading, 16)

                    Toggle(isOn: $haptics) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Haptics")
                                .font(.system(size: 15, weight: .regular))
                                .foregroundStyle(HexTheme.textBase)
                            Text("A light tap under your finger as you type")
                                .font(.system(size: 12))
                                .foregroundStyle(HexTheme.textMuted)
                        }
                    }
                    .tint(HexTheme.success)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .onChange(of: haptics) { _, enabled in
                        FullKeyboardSettings().haptics = enabled
                    }
                }

                Divider()
                    .overlay(HexTheme.borderWeak)
                    .padding(.leading, 16)

                // Not only for the full keyboard: it also learns from dictations the
                // dictation bar inserts.
                Toggle(isOn: $learnTyping) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Learn how I type")
                            .font(.system(size: 15, weight: .regular))
                            .foregroundStyle(HexTheme.textBase)
                        Text("Keeps where you tap and the words you fix, typed or dictated, on this iPhone, never what you write")
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
                .tint(HexTheme.success)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .onChange(of: learnTyping) { _, enabled in
                    TypingJournalSettings().isEnabled = enabled
                }

                if learnTyping {
                    TypingInsightsView()
                        .padding(.horizontal, 16)
                        .padding(.bottom, 12)
                }

                Divider()
                    .overlay(HexTheme.borderWeak)
                    .padding(.leading, 16)

                // Apart from Learn how I type: this keeps everything, to export and train on.
                Toggle(isOn: $typingLogs) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Record typing logs")
                            .font(.system(size: 15, weight: .regular))
                            .foregroundStyle(HexTheme.textBase)
                        Text("Saves every tap, where it lands, and the text you type or dictate with Hex's keyboard on this iPhone for 14 days, so you can export it. Nothing is sent.")
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
                .tint(HexTheme.success)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .onChange(of: typingLogs) { _, enabled in
                    TypingLogSettings().isEnabled = enabled
                }

                if typingLogs {
                    Button {
                        showTypingLogs = true
                    } label: {
                        HStack {
                            Text("Typing logs")
                                .font(.system(size: 13, weight: .medium))
                                .foregroundStyle(HexTheme.textBase)
                            Spacer()
                            Image(systemName: "chevron.right")
                                .font(.system(size: 12, weight: .semibold))
                                .foregroundStyle(HexTheme.textFaint)
                        }
                        .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 12)
                    .sheet(isPresented: $showTypingLogs) {
                        TypingLogsView()
                    }
                }
            }
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
            .onChange(of: fullKeyboard) { _, enabled in
                FullKeyboardSettings().isEnabled = enabled
            }

            Toggle(isOn: $cleanupTagger) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Clean up what I say")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    Text("Removes ums, repeats and things you corrected (3pm, actually 4pm), on this iPhone")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }
            .tint(HexTheme.success)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
            .onChange(of: cleanupTagger) { _, enabled in
                CleanupTaggerSettings().isEnabled = enabled
            }

            Toggle(isOn: $appleIntelligenceCleanup) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Clean up with Apple Intelligence")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    Text("Fix misheard words and spoken corrections on device")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                    if let reason = cleanupAvailability.reason {
                        Text(reason)
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
            }
            .tint(HexTheme.success)
            .disabled(!cleanupAvailability.isAvailable)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
            .onChange(of: appleIntelligenceCleanup) { _, enabled in
                AppleIntelligenceCleanupSettings().isEnabled = enabled
            }

            Toggle(isOn: $customVocabulary) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Custom vocabulary")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    Text("Spell names and terms your way")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }
            .tint(HexTheme.success)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
            .onChange(of: customVocabulary) { _, enabled in
                CustomVocabularySettings().isEnabled = enabled
            }

            TranslateDictationRow()
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .background(
                    HexTheme.bgBase,
                    in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
                )

            if customVocabulary {
                CustomVocabularyEditor()
                    .padding(12)
                    .background(
                        HexTheme.bgBase,
                        in: RoundedRectangle(
                            cornerRadius: HexTheme.cardRadius,
                            style: .continuous
                        )
                    )
            }
        }
        .onAppear {
            fullKeyboard = FullKeyboardSettings().isEnabled
            cleanupTagger = CleanupTaggerSettings().isEnabled
            appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
            customVocabulary = CustomVocabularySettings().isEnabled
            cleanupAvailability = AppleIntelligenceCleanupAvailability.current
            autocorrect = FullKeyboardSettings().autocorrect
            keyPopups = FullKeyboardSettings().keyPopups
            haptics = FullKeyboardSettings().haptics
            learnTyping = TypingJournalSettings().isEnabled
            typingLogs = TypingLogSettings().isEnabled
        }
    }
}
