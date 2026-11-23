import SwiftUI

/// Single choice for live dictation feedback on iOS 26+: off, preview in the keyboard bar,
/// or typing live into the text field. Backed by `LiveDictationSettings`'s existing keys so
/// existing preferences migrate without data loss.
enum LiveDictationMode: String, CaseIterable {
    case off
    case preview
    case typeIntoField

    var label: String {
        switch self {
        case .off: String(localized: "Off")
        case .preview: String(localized: "In bar")
        case .typeIntoField: String(localized: "Into field")
        }
    }

    var detail: String? {
        switch self {
        case .off, .preview: nil
        case .typeIntoField: String(localized: "Words can change until you stop")
        }
    }

    static func read(from settings: LiveDictationSettings = LiveDictationSettings()) -> LiveDictationMode {
        if settings.typesIntoField { return .typeIntoField }
        if settings.showsPreview { return .preview }
        return .off
    }

    func write(to settings: LiveDictationSettings = LiveDictationSettings()) {
        switch self {
        case .off:
            settings.showsPreview = false
            settings.typesIntoField = false
        case .preview:
            settings.showsPreview = true
            settings.typesIntoField = false
        case .typeIntoField:
            settings.showsPreview = false
            settings.typesIntoField = true
        }
    }
}

/// Settings › Dictation: Style and Language first, then Formatting (hiding controls Verbatim
/// ignores), live words, cleanup/vocabulary, and the speech model + microphone.
struct DictationSettingsPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Bindable var dictation: DictationController
    /// From Home's speech model card: the page starts at Speech model in keyboard.
    var opensAtSpeechModel = false
    static let speechModelRow = "speech-model-in-keyboard"
    @State private var revision = 0
    @State private var keyboardParakeet = KeyboardParakeetSettings().isOn
    private let keyboardParakeetFits = KeyboardParakeetSettings.fits()
    /// Keep mic warm stays on the page once picked, even Off where it would hide.
    @State private var keepsWarmRow = false

    private var showsStyleFormatting: Bool {
        dictation.style != .verbatim
    }

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

    private var liveMode: LiveDictationMode {
        _ = revision
        return LiveDictationMode.read()
    }

    private var customVocabularyOn: Bool {
        _ = revision
        return CustomVocabularySettings().isEnabled
    }

    var body: some View {
        SettingsPageScaffold(title: "Dictation", startsAt: opensAtSpeechModel ? Self.speechModelRow : nil) {
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

                SettingsDivider()

                TranslateDictationRow()
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .frame(minHeight: 50)
            }

            SettingsSectionHeader(title: "Formatting")

            SettingsCard {
                // After the style, on every path (`DictationPipeline`).
                SettingsToggleRow(
                    title: String(localized: "Lowercase"),
                    isOn: toggleBinding(
                        { DictationLowercaseSettings().isEnabled },
                        { DictationLowercaseSettings().isEnabled = $0 }
                    )
                )

                SettingsDivider()

                // Inside the style, on every path (`SpokenLists`).
                SettingsToggleRow(
                    title: String(localized: "Format lists"),
                    detail: String(localized: "Say “first, second” or “bullet” for a list"),
                    isOn: toggleBinding(
                        { DictationListSettings().isEnabled },
                        { DictationListSettings().isEnabled = $0 }
                    )
                )

                // Spoken punctuation, Numbers as digits, and End with a period are ignored in
                // Verbatim, so hide them when Verbatim is active.
                if showsStyleFormatting {
                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Spoken punctuation"),
                        detail: String(localized: "Say “comma” or “question mark”"),
                        isOn: toggleBinding(
                            { DictationPunctuationSettings().isEnabled },
                            { DictationPunctuationSettings().isEnabled = $0 }
                        )
                    )

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Numbers as digits"),
                        detail: String(localized: "“Five point five” becomes 5.5"),
                        isOn: toggleBinding(
                            { DictationNumberSettings().isEnabled },
                            { DictationNumberSettings().isEnabled = $0 }
                        )
                    )

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "End with a period"),
                        detail: String(localized: "Off still keeps ? and !"),
                        isOn: toggleBinding(
                            { DictationPeriodSettings().isEnabled },
                            { DictationPeriodSettings().isEnabled = $0 }
                        )
                    )
                }
            }

            // While the keyboard records by itself (iOS 26 and later): one 3-way choice for live
            // words instead of two interacting switches.
            if #available(iOS 26.0, *) {
                SettingsCard {
                    HStack(spacing: 12) {
                        SettingsRowLabel(
                            title: String(localized: "Live words"),
                            detail: liveMode.detail
                        )
                        Spacer(minLength: 8)
                        SettingsSegments(
                            items: LiveDictationMode.allCases,
                            selected: liveMode,
                            label: \.label,
                            onSelect: { mode in
                                mode.write()
                                revision += 1
                            }
                        )
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .frame(minHeight: 50)
                }
            }

            SettingsCard {
                SettingsToggleRow(
                    title: String(localized: "Clean up what I say"),
                    detail: String(localized: "Drops ums, repeats and corrected words"),
                    isOn: toggleBinding(
                        { CleanupTaggerSettings().isEnabled },
                        { CleanupTaggerSettings().isEnabled = $0 }
                    )
                )

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Custom vocabulary"),
                    detail: String(localized: "Spell names and terms your way"),
                    isOn: toggleBinding(
                        { CustomVocabularySettings().isEnabled },
                        { CustomVocabularySettings().isEnabled = $0 }
                    )
                )

                if customVocabularyOn {
                    CustomVocabularyEditor()
                        .padding(.horizontal, 16)
                        .padding(.top, 2)
                        .padding(.bottom, 14)
                }
            }

            SettingsCard {
                SettingsToggleRow(
                    title: String(localized: "Speech model in keyboard"),
                    detail: keyboardParakeetFits
                        ? String(localized: "For English. Uses about \(KeyboardParakeetDownload.size), downloaded on Wi-Fi. Until then, the keyboard uses Apple's speech recognition.")
                        : String(localized: "Needs an iPhone with 6 GB of memory or more"),
                    isOn: $keyboardParakeet
                )
                .disabled(!keyboardParakeetFits)

                if keyboardParakeet {
                    KeyboardParakeetStatus(download: KeyboardParakeetDownload.shared)
                        .padding(.horizontal, 12)
                        .padding(.bottom, 12)
                }

                // Hidden where the keyboard dictates by itself and it would say Off.
                if keepsWarmRow || dictation.showsKeepMicWarm {
                    SettingsDivider()

                    HStack(spacing: 12) {
                        SettingsRowLabel(title: String(localized: "Keep mic warm"))
                        Spacer(minLength: 8)
                        SettingsSegments(
                            items: WarmSessionDuration.allCases,
                            selected: dictation.warmDuration,
                            label: \.shortLabel,
                            onSelect: { duration in
                                keepsWarmRow = true
                                withAnimation(HexMotion.settleAnimation(reduceMotion: reduceMotion)) {
                                    dictation.selectWarmDuration(duration)
                                }
                            }
                        )
                    }
                    .padding(.horizontal, 16)
                    .frame(minHeight: 50)
                }

                SettingsDivider()

                // With a car, headset or headphones connected (`MicrophoneRoute`).
                HStack(spacing: 12) {
                    SettingsRowLabel(
                        title: String(localized: "Microphone"),
                        detail: dictation.microphone.detail
                    )
                    Spacer(minLength: 8)
                    Menu {
                        ForEach(MicrophoneChoice.allCases, id: \.self) { candidate in
                            Button {
                                dictation.setMicrophone(candidate)
                            } label: {
                                if dictation.microphone == candidate {
                                    Label(candidate.label, systemImage: "checkmark")
                                } else {
                                    Text(candidate.label)
                                }
                            }
                        }
                    } label: {
                        SettingsMenuValue(value: dictation.microphone.label)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .frame(minHeight: 50)
            }
            .id(Self.speechModelRow)
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: customVocabularyOn)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: showsStyleFormatting)
        .onChange(of: keyboardParakeet) { _, enabled in
            guard enabled != KeyboardParakeetSettings().isEnabled else { return }
            KeyboardParakeetSettings().isEnabled = enabled
            DictationController.shared.keyboardParakeetChanged()
        }
        .onAppear {
            keyboardParakeet = KeyboardParakeetSettings().isOn
            revision += 1
        }
    }
}
