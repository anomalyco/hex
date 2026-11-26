import SwiftUI

/// Settings › Labs: unfinished features grouped by domain (Dictation, Languages,
/// Keyboard layout, Typing), off until turned on. They live in the App Group so the keyboard
/// reads them too.
struct ExperimentsSettingsPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var revision = 0
    @State private var cleanupAvailability = AppleIntelligenceCleanupAvailability.current
    @State private var adaptiveTouchStatus = AdaptiveTouchStatus.sessionOnly
    @State private var confirmingTouchReset = false

    private func toggleBinding(_ get: @escaping () -> Bool, _ set: @escaping (Bool) -> Void) -> Binding<Bool> {
        Binding(
            get: {
                _ = revision
                return get()
            },
            set: { value in
                set(value)
                revision += 1
            }
        )
    }

    private func keyboardBinding(_ keyPath: WritableKeyPath<FullKeyboardSettings, Bool>) -> Binding<Bool> {
        toggleBinding(
            { FullKeyboardSettings()[keyPath: keyPath] },
            {
                var current = FullKeyboardSettings()
                current[keyPath: keyPath] = $0
            }
        )
    }

    private var adaptiveTouchOn: Bool {
        _ = revision
        return AdaptiveTouchSettings().isEnabled
    }

    private var keyboardThemesOn: Bool {
        _ = revision
        return FullKeyboardSettings().keyboardThemesEnabled
    }

    private var spatialAutocorrectOn: Bool {
        _ = revision
        let settings = FullKeyboardSettings()
        return settings.autocorrect && settings.spatialAutocorrect
    }

    var body: some View {
        SettingsPageScaffold(title: "Labs") {
            dictationSection
            languagesSection
            layoutSection
            typingSection

            // Testers tell the team how an experiment went (absent without a report key).
            if Feedback.isAvailable {
                SettingsCard {
                    FeedbackRow(screen: "Settings › Labs", dictation: DictationController.shared)
                }
                .padding(.top, 8)
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: adaptiveTouchOn)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: keyboardThemesOn)
        .onAppear {
            cleanupAvailability = AppleIntelligenceCleanupAvailability.current
            refreshAdaptiveTouchStatus()
            revision += 1
        }
    }

    @ViewBuilder
    private var dictationSection: some View {
        SettingsSectionHeader(title: "Dictation")

        SettingsCard {
            SettingsToggleRow(
                title: String(localized: "Two-language dictation"),
                detail: String(localized: "Checks a second keyboard language and keeps the better match"),
                isOn: toggleBinding(
                    { TwoLanguageDictationSettings().isEnabled },
                    { TwoLanguageDictationSettings().isEnabled = $0 }
                )
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Voice isolation"),
                detail: String(localized: "Lets Mic Mode in Control Center isolate your voice. Other audio may go quiet, and Bluetooth headphones may drop to call quality while you dictate."),
                isOn: toggleBinding(
                    { VoiceIsolationSettings().isEnabled },
                    { VoiceIsolationSettings().isEnabled = $0 }
                )
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Clean up with Apple Intelligence"),
                detail: cleanupAvailability.reason ?? String(localized: "Fixes misheard words on device"),
                isOn: toggleBinding(
                    { AppleIntelligenceCleanupSettings().isEnabled },
                    { AppleIntelligenceCleanupSettings().isEnabled = $0 }
                )
            )
            .disabled(!cleanupAvailability.isAvailable)
        }
    }

    @ViewBuilder
    private var languagesSection: some View {
        SettingsSectionHeader(title: "Languages")

        SettingsCard {
            SettingsToggleRow(
                title: String(localized: "Hinglish (Hindi in Latin letters)"),
                detail: String(localized: "Adds Hinglish to dictation languages"),
                isOn: toggleBinding(
                    { HinglishSettings().isEnabled },
                    { HinglishSettings().isEnabled = $0 }
                )
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Greeklish (Greek in Latin letters)"),
                detail: String(localized: "Adds Greeklish to dictation languages"),
                isOn: toggleBinding(
                    { GreeklishSettings().isEnabled },
                    { GreeklishSettings().isEnabled = $0 }
                )
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Korean keyboard (두벌식)"),
                detail: String(localized: "Adds 한국어, composing syllables as you type"),
                isOn: keyboardBinding(\.koreanKeyboard)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Swipe typing in more languages"),
                detail: String(localized: "French and Polish, and German with German spatial autocorrect"),
                isOn: keyboardBinding(\.swipeMoreLanguages)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "German spatial autocorrect"),
                detail: String(localized: "Needs Spatial autocorrect"),
                isOn: keyboardBinding(\.germanSpatialAutocorrect)
            )
            .disabled(!spatialAutocorrectOn)
        }
    }

    @ViewBuilder
    private var layoutSection: some View {
        SettingsSectionHeader(title: "Keyboard layout")

        SettingsCard {
            SettingsToggleRow(
                title: String(localized: "Comma and period keys"),
                detail: String(localized: "Comma and period beside the space bar"),
                isOn: keyboardBinding(\.commaPeriodKeys)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Dvorak punctuation keys"),
                detail: String(localized: "Adds comma, period and semicolon keys to the English Dvorak keyboard"),
                isOn: keyboardBinding(\.dvorakPunctuation)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Quick accents"),
                detail: String(localized: "Holding a letter selects its main accent, so letting go types it. Slide to pick another."),
                isOn: keyboardBinding(\.quickAccents)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "ABC under the emoji key"),
                detail: String(localized: "In the emoji panel, ABC takes the emoji key's place"),
                isOn: keyboardBinding(\.emojiLettersUnderEmojiKey)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Key colors"),
                isOn: keyboardBinding(\.keyboardThemesEnabled)
            )

            if keyboardThemesOn {
                SettingsDivider()
                KeyboardThemeRow(
                    theme: Binding(
                        get: {
                            _ = revision
                            return FullKeyboardSettings().keyboardTheme
                        },
                        set: { value in
                            FullKeyboardSettings().keyboardTheme = value
                            revision += 1
                        }
                    )
                )
            }

            // Only relevant on foldable hardware (iPhone Duo): hide on slab iPhones.
            if FullKeyboardView.isFoldable {
                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Join the keyboard when unfolded"),
                    detail: String(localized: "On the iPhone Duo unfolded, keeps the keys on one keyboard instead of splitting them"),
                    isOn: keyboardBinding(\.joinsKeyboardWhenUnfolded)
                )
            }
        }
    }

    @ViewBuilder
    private var typingSection: some View {
        SettingsSectionHeader(title: "Typing")

        SettingsCard {
            SettingsToggleRow(
                title: String(localized: "Adaptive touch"),
                detail: String(localized: "Learns where you tap each key"),
                isOn: toggleBinding(
                    { AdaptiveTouchSettings().isEnabled },
                    { enabled in
                        AdaptiveTouchSettings().isEnabled = enabled
                        refreshAdaptiveTouchStatus()
                    }
                )
            )

            if adaptiveTouchOn {
                Text(adaptiveTouchStatusText)
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 10)
                    .accessibilityIdentifier("adaptive-touch-status")

                SettingsDivider()

                Button {
                    confirmingTouchReset = true
                } label: {
                    Text("Reset touch learning")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.danger)
                        .frame(maxWidth: .infinity, minHeight: 50, alignment: .leading)
                        .padding(.horizontal, 16)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .confirmationDialog("Reset touch learning?", isPresented: $confirmingTouchReset, titleVisibility: .visible) {
                    Button("Reset touch learning", role: .destructive) {
                        AdaptiveTouchSettings().requestReset()
                        refreshAdaptiveTouchStatus()
                    }
                }
            }

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Two-thumb selection"),
                detail: String(localized: "On the space bar trackpad, a second thumb selects text"),
                isOn: keyboardBinding(\.twoThumbSelection)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Delete swiped words letter by letter"),
                isOn: keyboardBinding(\.deletesSwipedWordsLetterByLetter)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Hide word suggestions"),
                detail: String(localized: "Autocorrect still works"),
                isOn: keyboardBinding(\.hideWordSuggestions)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Emoji shortcodes"),
                detail: String(localized: "Type a colon and a name, like :pizza, to pick an emoji"),
                isOn: keyboardBinding(\.emojiShortcodes)
            )

            SettingsDivider()

            SettingsToggleRow(
                title: String(localized: "Learn each language separately"),
                detail: String(localized: "Learned words and fixes stay with their language"),
                isOn: toggleBinding(
                    { TypingJournalSettings().learnsPerLanguage },
                    { TypingJournalSettings().learnsPerLanguage = $0 }
                )
            )
        }
    }

    private var adaptiveTouchStatusText: String {
        switch adaptiveTouchStatus {
        case .sessionOnly:
            String(localized: "Without Full Access, resets when the keyboard closes")
        case .learning(let taps):
            String(localized: "Learning: \(taps) of \(AdaptiveTouchModel.activeTaps) taps")
        case .active:
            String(localized: "Active, using your own key centers")
        }
    }

    private func refreshAdaptiveTouchStatus() {
        adaptiveTouchStatus = SettingsSnapshotFixture.adaptiveTouchStatus
            ?? AdaptiveTouchStatus.current(fullAccessConfirmed: KeyboardBridge().fullAccessSeenAt != nil)
    }
}

/// Experiments › Key colors: the colour the full keyboard's keys take while it's on.
private struct KeyboardThemeRow: View {
    @Binding var theme: KeyboardTheme

    var body: some View {
        HStack(spacing: 12) {
            SettingsRowLabel(title: String(localized: "Color"))
            Spacer(minLength: 8)
            Menu {
                ForEach(KeyboardTheme.allCases, id: \.self) { option in
                    Button {
                        theme = option
                    } label: {
                        if theme == option {
                            Label(option.name, systemImage: "checkmark")
                        } else {
                            Text(option.name)
                        }
                    }
                }
            } label: {
                SettingsMenuValue(value: theme.name)
            }
        }
        .padding(.horizontal, 16)
        .frame(minHeight: 50)
    }
}
