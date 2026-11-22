import SwiftUI

/// Settings › Dictation: how Hex writes what you say, and the microphone.
struct DictationSettingsPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Bindable var dictation: DictationController
    @State private var cleanupTagger = CleanupTaggerSettings().isEnabled
    @State private var customVocabulary = CustomVocabularySettings().isEnabled
    @State private var lowercase = DictationLowercaseSettings().isEnabled
    @State private var formatLists = DictationListSettings().isEnabled
    @State private var spokenPunctuation = DictationPunctuationSettings().isEnabled
    @State private var endsWithPeriod = DictationPeriodSettings().isEnabled

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

                SettingsDivider()

                // After the style, on every path (`DictationPipeline`).
                SettingsToggleRow(
                    title: String(localized: "Lowercase"),
                    detail: String(localized: "Lowercases everything"),
                    isOn: $lowercase
                )

                SettingsDivider()

                // Inside the style, on every path (`SpokenLists`).
                SettingsToggleRow(
                    title: String(localized: "Format lists"),
                    detail: String(localized: "Say “first, second” or “bullet” for a list"),
                    isOn: $formatLists
                )

                SettingsDivider()

                // Inside the style, on every path but Verbatim (`SpokenPunctuation`).
                SettingsToggleRow(
                    title: String(localized: "Spoken punctuation"),
                    detail: String(localized: "Say “comma” or “question mark” for the mark"),
                    isOn: $spokenPunctuation
                )

                SettingsDivider()

                // Inside the style, on every path but Verbatim (`TranscriptFormatter`).
                SettingsToggleRow(
                    title: String(localized: "End with a period"),
                    detail: String(localized: "Off drops the final period, keeps ? and !"),
                    isOn: $endsWithPeriod
                )

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
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: customVocabulary)
        .onChange(of: cleanupTagger) { _, enabled in
            CleanupTaggerSettings().isEnabled = enabled
        }
        .onChange(of: customVocabulary) { _, enabled in
            CustomVocabularySettings().isEnabled = enabled
        }
        .onChange(of: lowercase) { _, enabled in
            DictationLowercaseSettings().isEnabled = enabled
        }
        .onChange(of: formatLists) { _, enabled in
            DictationListSettings().isEnabled = enabled
        }
        .onChange(of: spokenPunctuation) { _, enabled in
            DictationPunctuationSettings().isEnabled = enabled
        }
        .onChange(of: endsWithPeriod) { _, enabled in
            DictationPeriodSettings().isEnabled = enabled
        }
        .onAppear {
            cleanupTagger = CleanupTaggerSettings().isEnabled
            customVocabulary = CustomVocabularySettings().isEnabled
            lowercase = DictationLowercaseSettings().isEnabled
            formatLists = DictationListSettings().isEnabled
            spokenPunctuation = DictationPunctuationSettings().isEnabled
            endsWithPeriod = DictationPeriodSettings().isEnabled
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
        case .whenSwitching: String(localized: "When you switch back to Hex in a field")
        case .always: String(localized: "Each time the dictation bar opens")
        }
    }
}

/// Settings › Keyboard › Hex key: which end of the strip the full keyboard's Hex key sits at.
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

/// Settings › Keyboard: Full or Dictation bar (with Auto-start under the bar, On switch by
/// default, and always on iPad, which shows the bar), the full keyboard's rows (only while it's
/// chosen; Auto-capitalization, on by default as Apple's, sits above Autocorrect; Spatial autocorrect only while Autocorrect is on, and Suggest swear words off by
/// default, as Apple's keyboard never offers them, then Vocabulary, Hex's list of your own
/// words; Hex key, Right by default, under Languages),
/// and haptics, which the dictation bar uses too. Below, Swipe typing, which says it needs
/// the full keyboard while the dictation bar is chosen, and Emoji (on by default, with Clear
/// emoji history while it's on), the bar's emoji key too. The dictation bar and Haptics say
/// they need Full Access, and say it isn't on yet until a keyboard with it has reported in.
struct KeyboardSettingsPage: View {
    /// `KeyboardSetupMonitor.fullAccessOn`.
    let fullAccessOn: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var fullKeyboard = FullKeyboardSettings().isEnabled
    @State private var autoCapitalization = FullKeyboardSettings().autoCapitalization
    @State private var hexKeyOnLeft = FullKeyboardSettings().hexKeyOnLeft
    @State private var autocorrect = FullKeyboardSettings().autocorrect
    @State private var spatialAutocorrect = FullKeyboardSettings().spatialAutocorrect
    @State private var suggestSwearWords = FullKeyboardSettings().suggestSwearWords
    @State private var keyPopups = FullKeyboardSettings().keyPopups
    @State private var haptics = FullKeyboardSettings().haptics
    @State private var swipeTyping = FullKeyboardSettings().swipeTyping
    @State private var emoji = FullKeyboardSettings().emoji
    @State private var emojiHistoryEmpty = EmojiUsageStore().isEmpty
    @State private var hapticStrength = FullKeyboardSettings().hapticStrength
    @State private var autoStart = FullKeyboardSettings().dictationBarAutoStart
    @State private var practiceText = ""

    /// The full keyboard is iPhone only: on an iPad the keyboard shows the dictation bar, whichever
    /// is chosen. Hex runs there as an iPhone app, whose interface idiom says phone, so this reads
    /// the hardware model (the simulated one in the simulator).
    private static let runsOnPad: Bool = {
        if let simulated = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] {
            return simulated.hasPrefix("iPad")
        }
        var system = utsname()
        uname(&system)
        let model = withUnsafeBytes(of: &system.machine) { raw in
            String(decoding: raw.prefix { $0 != 0 }, as: UTF8.self)
        }
        return model.hasPrefix("iPad")
    }()

    /// A row's detail for something that needs Full Access: the detail, or while Full Access
    /// isn't on, that it isn't (as Swipe typing says it needs the full keyboard).
    private func fullAccessNeed(_ detail: String) -> String {
        fullAccessOn ? detail : String(localized: "Needs Full Access, which isn't on yet")
    }

    var body: some View {
        SettingsPageScaffold(title: "Keyboard") {
            SettingsCard {
                HStack(spacing: 12) {
                    SettingsRowLabel(
                        title: String(localized: "Keyboard"),
                        detail: Self.runsOnPad
                            ? String(localized: "On iPad, Hex shows the dictation bar")
                            : fullKeyboard ? nil : fullAccessNeed(String(localized: "The dictation bar needs Full Access"))
                    )
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
                .padding(.vertical, 10)
                .frame(minHeight: 50)

                if !fullKeyboard || Self.runsOnPad {
                    SettingsDivider()

                    HStack(spacing: 12) {
                        SettingsRowLabel(title: String(localized: "Auto-start"), detail: autoStart.detail)
                        Spacer(minLength: 8)
                        SettingsSegments(
                            items: DictationBarAutoStart.allCases,
                            selected: autoStart,
                            label: \.label,
                            onSelect: { autoStart = $0 }
                        )
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .frame(minHeight: 50)
                }

                if fullKeyboard {
                    KeyboardLanguagesRow()

                    SettingsDivider()

                    HStack(spacing: 12) {
                        SettingsRowLabel(title: String(localized: "Hex key"))
                        Spacer(minLength: 8)
                        SettingsSegments(
                            items: HexKeySide.allCases,
                            selected: hexKeyOnLeft ? .left : .right,
                            label: \.label,
                            onSelect: { side in
                                hexKeyOnLeft = side == .left
                            }
                        )
                    }
                    .padding(.horizontal, 16)
                    .frame(minHeight: 50)

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Auto-capitalization"),
                        detail: String(localized: "Capitalizes the start of sentences"),
                        isOn: $autoCapitalization
                    )

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Autocorrect"),
                        detail: String(localized: "Fixes typos when you press space"),
                        isOn: $autocorrect
                    )

                    if autocorrect {
                        SettingsDivider()

                        SettingsToggleRow(
                            title: String(localized: "Spatial autocorrect"),
                            detail: String(localized: "Uses where you tap, in English and Spanish"),
                            isOn: $spatialAutocorrect
                        )
                    }

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Suggest swear words"),
                        detail: String(localized: "Slurs are never suggested"),
                        isOn: $suggestSwearWords
                    )

                    SettingsDivider()

                    KeyboardVocabularyRow()

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
                    detail: fullAccessNeed(String(localized: "A tap as you press Hex's keys. Needs Full Access")),
                    isOn: $haptics
                )

                if haptics {
                    SettingsDivider()

                    HStack(spacing: 12) {
                        SettingsRowLabel(title: String(localized: "Strength"))
                        Spacer(minLength: 8)
                        SettingsSegments(
                            items: KeyHapticStrength.allCases,
                            selected: hapticStrength,
                            label: \.label,
                            onSelect: { hapticStrength = $0 }
                        )
                    }
                    .padding(.horizontal, 16)
                    .frame(minHeight: 50)
                }
            }

            SettingsCard {
                SettingsToggleRow(
                    title: String(localized: "Swipe typing"),
                    detail: fullKeyboard
                        ? String(localized: "Slide across a word's letters, in English and Spanish")
                        : String(localized: "Needs the full keyboard"),
                    isOn: $swipeTyping
                )

                SettingsDivider()

                EmojiRows(emoji: $emoji, historyEmpty: $emojiHistoryEmpty, fullKeyboard: fullKeyboard)
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
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: emoji)
        .onChange(of: fullKeyboard) { _, enabled in
            FullKeyboardSettings().isEnabled = enabled
        }
        .onChange(of: autoCapitalization) { _, enabled in
            FullKeyboardSettings().autoCapitalization = enabled
        }
        .onChange(of: hexKeyOnLeft) { _, onLeft in
            FullKeyboardSettings().hexKeyOnLeft = onLeft
        }
        .onChange(of: autocorrect) { _, enabled in
            FullKeyboardSettings().autocorrect = enabled
        }
        .onChange(of: spatialAutocorrect) { _, enabled in
            FullKeyboardSettings().spatialAutocorrect = enabled
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
        .onChange(of: swipeTyping) { _, enabled in
            FullKeyboardSettings().swipeTyping = enabled
        }
        .onChange(of: hapticStrength) { _, strength in
            FullKeyboardSettings().hapticStrength = strength
        }
        .onChange(of: autoStart) { _, choice in
            FullKeyboardSettings().dictationBarAutoStart = choice
        }
        .onAppear {
            fullKeyboard = FullKeyboardSettings().isEnabled
            autoCapitalization = FullKeyboardSettings().autoCapitalization
            hexKeyOnLeft = FullKeyboardSettings().hexKeyOnLeft
            autocorrect = FullKeyboardSettings().autocorrect
            spatialAutocorrect = FullKeyboardSettings().spatialAutocorrect
            suggestSwearWords = FullKeyboardSettings().suggestSwearWords
            keyPopups = FullKeyboardSettings().keyPopups
            haptics = FullKeyboardSettings().haptics
            swipeTyping = FullKeyboardSettings().swipeTyping
            emoji = FullKeyboardSettings().emoji
            emojiHistoryEmpty = EmojiUsageStore().isEmpty
            hapticStrength = FullKeyboardSettings().hapticStrength
            autoStart = FullKeyboardSettings().dictationBarAutoStart
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
                // dictation bar inserts. On until turned off (`TypingJournalSettings`).
                SettingsToggleRow(
                    title: String(localized: "Learn how I type"),
                    detail: String(localized: "Learns on this iPhone from your taps and the words you fix with Hex, never from what you write elsewhere"),
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
    @State private var hinglish = HinglishSettings().isEnabled
    @State private var appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
    @State private var cleanupAvailability = AppleIntelligenceCleanupAvailability.current
    @State private var adaptiveTouch = AdaptiveTouchSettings().isEnabled
    @State private var adaptiveTouchStatus = AdaptiveTouchStatus.sessionOnly
    @State private var confirmingTouchReset = false
    @State private var germanSpatialAutocorrect = FullKeyboardSettings().germanSpatialAutocorrect
    @State private var emojiLettersUnderEmojiKey = FullKeyboardSettings().emojiLettersUnderEmojiKey
    @State private var twoThumbSelection = FullKeyboardSettings().twoThumbSelection
    @State private var learnPerLanguage = TypingJournalSettings().learnsPerLanguage
    @State private var multilingualParakeet = MultilingualParakeetSettings().isEnabled
    private let multilingualParakeetFits = MultilingualParakeetSettings.fits()

    var body: some View {
        SettingsPageScaffold(title: "Experiments") {
            SettingsCard {
                SettingsToggleRow(
                    title: String(localized: "Hinglish (Hindi in Latin letters)"),
                    detail: String(localized: "Pick Hinglish as your dictation language"),
                    isOn: $hinglish
                )

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Clean up with Apple Intelligence"),
                    detail: cleanupAvailability.reason ?? String(localized: "Fixes misheard words on device"),
                    isOn: $appleIntelligenceCleanup
                )
                .disabled(!cleanupAvailability.isAvailable)

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Adaptive touch"),
                    detail: String(localized: "Learns where you land on the keys on this device. Stores averages, not what you type."),
                    isOn: $adaptiveTouch
                )

                if adaptiveTouch {
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
                            .foregroundStyle(HexTheme.textBase)
                            .frame(maxWidth: .infinity, minHeight: 50, alignment: .leading)
                            .padding(.horizontal, 16)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .confirmationDialog("Forget where you land on the keys?", isPresented: $confirmingTouchReset, titleVisibility: .visible) {
                        Button("Reset touch learning", role: .destructive) {
                            AdaptiveTouchSettings().requestReset()
                            refreshAdaptiveTouchStatus()
                        }
                    }
                }

                SettingsDivider()

                // Off by default: German keeps the dictionary engine until this is on, and
                // then only while Spatial autocorrect is on too (Settings › Keyboard).
                SettingsToggleRow(
                    title: String(localized: "German spatial autocorrect"),
                    detail: String(localized: "Uses where you tap on the Deutsch keyboard"),
                    isOn: $germanSpatialAutocorrect
                )

                SettingsDivider()

                // Off by default: the emoji panel's ABC stays where it always was.
                SettingsToggleRow(
                    title: String(localized: "ABC under the emoji key"),
                    detail: String(localized: "In the emoji panel, ABC sits where the emoji key was, so you can switch back without moving your thumb."),
                    isOn: $emojiLettersUnderEmojiKey
                )

                SettingsDivider()

                // Off by default: a second finger on the trackpad is ignored, as before.
                SettingsToggleRow(
                    title: String(localized: "Two-thumb selection"),
                    detail: String(localized: "With the space bar as a trackpad, a second thumb selects: each thumb moves one end."),
                    isOn: $twoThumbSelection
                )

                SettingsDivider()

                // Off by default: one shared list of learned words and fixes, as before.
                SettingsToggleRow(
                    title: String(localized: "Learn each language separately"),
                    detail: String(localized: "Keeps learned words and fixes with the language you used them in."),
                    isOn: $learnPerLanguage
                )

                SettingsDivider()

                // Off by default: Parakeet Ultra (ParakeetUltraTranscriber) for its 25 languages,
                // downloaded when turned on and deleted when turned off.
                SettingsToggleRow(
                    title: String(localized: "Multilingual model (Parakeet)"),
                    detail: multilingualParakeetFits
                        ? String(localized: "25 languages, even mixed in a sentence. \(Self.multilingualParakeetSize) download. Keyboard dictation opens Hex.")
                        : String(localized: "Needs an iPhone with 6 GB of memory or more"),
                    isOn: $multilingualParakeet
                )
                .disabled(!multilingualParakeetFits)
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: adaptiveTouch)
        .onChange(of: appleIntelligenceCleanup) { _, enabled in
            AppleIntelligenceCleanupSettings().isEnabled = enabled
        }
        .onChange(of: hinglish) { _, enabled in
            HinglishSettings().isEnabled = enabled
        }
        .onChange(of: adaptiveTouch) { _, enabled in
            AdaptiveTouchSettings().isEnabled = enabled
            refreshAdaptiveTouchStatus()
        }
        .onChange(of: germanSpatialAutocorrect) { _, enabled in
            FullKeyboardSettings().germanSpatialAutocorrect = enabled
        }
        .onChange(of: emojiLettersUnderEmojiKey) { _, enabled in
            FullKeyboardSettings().emojiLettersUnderEmojiKey = enabled
        }
        .onChange(of: twoThumbSelection) { _, enabled in
            FullKeyboardSettings().twoThumbSelection = enabled
        }
        .onChange(of: learnPerLanguage) { _, enabled in
            TypingJournalSettings().learnsPerLanguage = enabled
        }
        .onChange(of: multilingualParakeet) { _, enabled in
            guard enabled != MultilingualParakeetSettings().isEnabled else { return }
            MultilingualParakeetSettings().isEnabled = enabled
            if enabled {
                // Turning it on is the way to try again after a load that ended Hex.
                ParakeetLoadGuard(model: ParakeetUltraTranscriber.loadGuardModel).forget()
            } else {
                ParakeetUltraModel.remove()
            }
            DictationController.shared.multilingualParakeetChanged()
        }
        .onAppear {
            hinglish = HinglishSettings().isEnabled
            appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
            cleanupAvailability = AppleIntelligenceCleanupAvailability.current
            adaptiveTouch = AdaptiveTouchSettings().isEnabled
            refreshAdaptiveTouchStatus()
            germanSpatialAutocorrect = FullKeyboardSettings().germanSpatialAutocorrect
            emojiLettersUnderEmojiKey = FullKeyboardSettings().emojiLettersUnderEmojiKey
            twoThumbSelection = FullKeyboardSettings().twoThumbSelection
            learnPerLanguage = TypingJournalSettings().learnsPerLanguage
            multilingualParakeet = MultilingualParakeetSettings().isEnabled
        }
    }

    private static var multilingualParakeetSize: String {
        ParakeetUltraTranscriber.downloadSize
    }

    private var adaptiveTouchStatusText: String {
        switch adaptiveTouchStatus {
        case .sessionOnly:
            String(localized: "Session only: without Full Access, it forgets when the keyboard closes")
        case .learning(let taps):
            String(localized: "Learning: \(taps) of \(AdaptiveTouchModel.activeTaps) taps")
        case .active:
            String(localized: "Active: your taps are read with your own key centres")
        }
    }

    private func refreshAdaptiveTouchStatus() {
        adaptiveTouchStatus = SettingsSnapshotFixture.adaptiveTouchStatus
            ?? AdaptiveTouchStatus.current(fullAccessConfirmed: KeyboardBridge().fullAccessSeenAt != nil)
    }
}

/// Keyboard › Emoji: the full keyboard's emoji key, and while it's on, Clear emoji history.
private struct EmojiRows: View {
    @Binding var emoji: Bool
    @Binding var historyEmpty: Bool
    let fullKeyboard: Bool

    var body: some View {
        SettingsToggleRow(
            title: String(localized: "Emoji"),
            detail: fullKeyboard
                ? String(localized: "An emoji key with suggestions as you type")
                : String(localized: "An emoji key on the dictation bar"),
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
