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
    @State private var numbersAsDigits = DictationNumberSettings().isEnabled
    @State private var endsWithPeriod = DictationPeriodSettings().isEnabled
    @State private var showsWhatISay = LiveDictationSettings().showsPreview
    @State private var typesAsISpeak = LiveDictationSettings().typesIntoField
    /// Keep mic warm stays on the page once picked, even Off where it would hide.
    @State private var keepsWarmRow = false

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
                    detail: String(localized: "No capital letters"),
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
                    detail: String(localized: "Say “comma” or “question mark”"),
                    isOn: $spokenPunctuation
                )

                SettingsDivider()

                // Before Clean up what I say, on every path but Verbatim (`SpokenNumbers`).
                SettingsToggleRow(
                    title: String(localized: "Numbers as digits"),
                    detail: String(localized: "“Five point five” becomes 5.5"),
                    isOn: $numbersAsDigits
                )

                SettingsDivider()

                // Inside the style, on every path but Verbatim (`TranscriptFormatter`).
                SettingsToggleRow(
                    title: String(localized: "End with a period"),
                    detail: String(localized: "Off still keeps ? and !"),
                    isOn: $endsWithPeriod
                )

                DictationLanguageRow(dictation: dictation)
            }

            // While the keyboard records by itself (iOS 26 and later): its live words.
            if #available(iOS 26.0, *) {
                SettingsCard {
                    SettingsToggleRow(title: String(localized: "Show what I say"), isOn: $showsWhatISay)

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Type as I speak"),
                        detail: String(localized: "Words can change until you stop"),
                        isOn: $typesAsISpeak
                    )
                }
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
                // Hidden where the keyboard dictates by itself and it would say Off.
                if keepsWarmRow || dictation.showsKeepMicWarm {
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

                    SettingsDivider()
                }

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
        .onChange(of: numbersAsDigits) { _, enabled in
            DictationNumberSettings().isEnabled = enabled
        }
        .onChange(of: endsWithPeriod) { _, enabled in
            DictationPeriodSettings().isEnabled = enabled
        }
        .onChange(of: showsWhatISay) { _, enabled in
            LiveDictationSettings().showsPreview = enabled
        }
        .onChange(of: typesAsISpeak) { _, enabled in
            LiveDictationSettings().typesIntoField = enabled
        }
        .onAppear {
            cleanupTagger = CleanupTaggerSettings().isEnabled
            customVocabulary = CustomVocabularySettings().isEnabled
            lowercase = DictationLowercaseSettings().isEnabled
            formatLists = DictationListSettings().isEnabled
            spokenPunctuation = DictationPunctuationSettings().isEnabled
            numbersAsDigits = DictationNumberSettings().isEnabled
            endsWithPeriod = DictationPeriodSettings().isEnabled
            showsWhatISay = LiveDictationSettings().showsPreview
            typesAsISpeak = LiveDictationSettings().typesIntoField
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
        case .whenSwitching: String(localized: "When you switch to Hex")
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

/// Settings › Keyboard: Full or Dictation bar, on an iPhone and an iPad alike (with Auto-start
/// under the bar, On switch by default), the full keyboard's rows (only while it's
/// chosen; Auto-capitalization, on by default as Apple's, sits above Autocorrect; Spatial autocorrect only while Autocorrect is on, and Suggest swear words off by
/// default, as Apple's keyboard never offers them, then Vocabulary, Hex's list of your own
/// words; Hex key, Right by default, under Languages, then Number row, off by default),
/// and haptics, which the dictation bar uses too. Below, Swipe typing, which says it needs
/// the full keyboard while the dictation bar is chosen, and Emoji (on by default, with Clear
/// emoji history while it's on), the bar's emoji key too. The dictation bar and Haptics say
/// they need Full Access until a keyboard with it has reported in.
struct KeyboardSettingsPage: View {
    /// `KeyboardSetupMonitor.fullAccessOn`.
    let fullAccessOn: Bool
    var needsAllowPasteSetting = false
    var onDismissAllowPasteSetting: () -> Void = {}
    @Environment(\.openURL) private var openURL
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var fullKeyboard = FullKeyboardSettings().isEnabled
    @State private var autoCapitalization = FullKeyboardSettings().autoCapitalization
    @State private var hexKeyOnLeft = FullKeyboardSettings().hexKeyOnLeft
    @State private var numberRow = FullKeyboardSettings().numberRow
    @State private var autocorrect = FullKeyboardSettings().autocorrect
    @State private var spatialAutocorrect = FullKeyboardSettings().spatialAutocorrect
    @State private var suggestSwearWords = FullKeyboardSettings().suggestSwearWords
    @State private var keyPopups = FullKeyboardSettings().keyPopups
    @State private var haptics = FullKeyboardSettings().haptics
    @State private var swipeTyping = FullKeyboardSettings().swipeTyping
    @State private var swipeMoreLanguages = FullKeyboardSettings().swipeMoreLanguages
    @State private var emoji = FullKeyboardSettings().emoji
    @State private var emojiHistoryEmpty = EmojiUsageStore().isEmpty
    @State private var hapticStrength = FullKeyboardSettings().hapticStrength
    @State private var autoStart = FullKeyboardSettings().dictationBarAutoStart
    @State private var practiceText = ""

    /// The detail of a row that needs Full Access: none while it's on, and while it isn't, that
    /// the row needs it (as Swipe typing says it needs the full keyboard).
    private var fullAccessNeed: String? {
        fullAccessOn ? nil : String(localized: "Needs Full Access")
    }

    /// Swipe typing's detail: the layouts that swipe, which Settings › Experiments › Swipe
    /// typing in more languages widens.
    private var swipeLanguages: String {
        swipeMoreLanguages ? String(localized: "French, Polish and German too") : String(localized: "English and Spanish only")
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
                            fullKeyboard = choice == .full
                        }
                    )
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .frame(minHeight: 50)

                if !fullKeyboard {
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
                        title: String(localized: "Number row"),
                        detail: String(localized: "Shows 1 to 0 above the letters"),
                        isOn: $numberRow
                    )

                    SettingsDivider()

                    SettingsToggleRow(
                        title: String(localized: "Auto-capitalization"),
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
                            detail: String(localized: "English, Spanish, French and Polish only"),
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
                        isOn: $keyPopups
                    )
                }

                SettingsDivider()

                SettingsToggleRow(
                    title: String(localized: "Haptics"),
                    detail: fullAccessNeed,
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
                    detail: fullKeyboard ? swipeLanguages : String(localized: "Needs the full keyboard"),
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
        .onChange(of: numberRow) { _, enabled in
            FullKeyboardSettings().numberRow = enabled
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
            numberRow = FullKeyboardSettings().numberRow
            autocorrect = FullKeyboardSettings().autocorrect
            spatialAutocorrect = FullKeyboardSettings().spatialAutocorrect
            suggestSwearWords = FullKeyboardSettings().suggestSwearWords
            keyPopups = FullKeyboardSettings().keyPopups
            haptics = FullKeyboardSettings().haptics
            swipeTyping = FullKeyboardSettings().swipeTyping
            swipeMoreLanguages = FullKeyboardSettings().swipeMoreLanguages
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
                    detail: String(localized: "Learns on this iPhone from your taps and the words you fix in Hex"),
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
                    detail: String(localized: "Keeps 14 days of taps and typed text on this iPhone, to export"),
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
                    Text("The Hex keyboard will record each tap and the text you type or dictate, including text around the cursor, for 14 days. Logs stay on this iPhone. Password, code and card fields are never recorded.")
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
    @State private var greeklish = GreeklishSettings().isEnabled
    @State private var twoLanguageDictation = TwoLanguageDictationSettings().isEnabled
    @State private var appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
    @State private var cleanupAvailability = AppleIntelligenceCleanupAvailability.current
    @State private var adaptiveTouch = AdaptiveTouchSettings().isEnabled
    @State private var adaptiveTouchStatus = AdaptiveTouchStatus.sessionOnly
    @State private var confirmingTouchReset = false
    @State private var germanSpatialAutocorrect = FullKeyboardSettings().germanSpatialAutocorrect
    @State private var swipeMoreLanguages = FullKeyboardSettings().swipeMoreLanguages
    @State private var koreanKeyboard = FullKeyboardSettings().koreanKeyboard
    @State private var dvorakPunctuation = FullKeyboardSettings().dvorakPunctuation
    @State private var commaPeriodKeys = FullKeyboardSettings().commaPeriodKeys
    @State private var quickAccents = FullKeyboardSettings().quickAccents
    @State private var emojiLettersUnderEmojiKey = FullKeyboardSettings().emojiLettersUnderEmojiKey
    @State private var keyboardThemes = FullKeyboardSettings().keyboardThemesEnabled
    @State private var keyboardTheme = FullKeyboardSettings().keyboardTheme
    @State private var twoThumbSelection = FullKeyboardSettings().twoThumbSelection
    @State private var deletesSwipedWordsLetterByLetter = FullKeyboardSettings().deletesSwipedWordsLetterByLetter
    @State private var hideWordSuggestions = FullKeyboardSettings().hideWordSuggestions
    @State private var joinsKeyboardWhenUnfolded = FullKeyboardSettings().joinsKeyboardWhenUnfolded
    @State private var learnPerLanguage = TypingJournalSettings().learnsPerLanguage
    @State private var multilingualParakeet = MultilingualParakeetSettings().isEnabled
    private let multilingualParakeetFits = MultilingualParakeetSettings.fits()
    @State private var keyboardParakeet = KeyboardParakeetSettings().isOn
    private let keyboardParakeetFits = KeyboardParakeetSettings.fits()

    var body: some View {
        switches
            .onChange(of: twoThumbSelection) { _, enabled in
                FullKeyboardSettings().twoThumbSelection = enabled
            }
            .onChange(of: deletesSwipedWordsLetterByLetter) { _, enabled in
                FullKeyboardSettings().deletesSwipedWordsLetterByLetter = enabled
            }
            .onChange(of: hideWordSuggestions) { _, enabled in
                FullKeyboardSettings().hideWordSuggestions = enabled
            }
            .onChange(of: joinsKeyboardWhenUnfolded) { _, enabled in
                FullKeyboardSettings().joinsKeyboardWhenUnfolded = enabled
            }
            .onChange(of: learnPerLanguage) { _, enabled in
                TypingJournalSettings().learnsPerLanguage = enabled
            }
            .onChange(of: multilingualParakeet) { _, enabled in
                guard enabled != MultilingualParakeetSettings().isEnabled else { return }
                MultilingualParakeetSettings().isEnabled = enabled
                // On: downloads beside the engine that dictates; off: stops and deletes it.
                DictationController.shared.multilingualParakeetChanged()
            }
            .onChange(of: keyboardParakeet) { _, enabled in
                guard enabled != KeyboardParakeetSettings().isEnabled else { return }
                KeyboardParakeetSettings().isEnabled = enabled
                DictationController.shared.keyboardParakeetChanged()
            }
            .onAppear { loadSettings() }
    }

    /// The page and the switches that only change a stored setting, split into shorter
    /// modifier chains so the compiler type-checks each one in reasonable time.
    private var toggles: some View {
        page
            .onChange(of: appleIntelligenceCleanup) { _, enabled in
                AppleIntelligenceCleanupSettings().isEnabled = enabled
            }
            .onChange(of: hinglish) { _, enabled in
                HinglishSettings().isEnabled = enabled
            }
            .onChange(of: greeklish) { _, enabled in
                GreeklishSettings().isEnabled = enabled
            }
            .onChange(of: twoLanguageDictation) { _, enabled in
                TwoLanguageDictationSettings().isEnabled = enabled
            }
            .onChange(of: adaptiveTouch) { _, enabled in
                AdaptiveTouchSettings().isEnabled = enabled
                refreshAdaptiveTouchStatus()
            }
    }

    private var switches: some View {
        toggles
            .onChange(of: germanSpatialAutocorrect) { _, enabled in
                FullKeyboardSettings().germanSpatialAutocorrect = enabled
            }
            .onChange(of: swipeMoreLanguages) { _, enabled in
                FullKeyboardSettings().swipeMoreLanguages = enabled
            }
            .onChange(of: koreanKeyboard) { _, enabled in
                FullKeyboardSettings().koreanKeyboard = enabled
            }
            .onChange(of: dvorakPunctuation) { _, enabled in
                FullKeyboardSettings().dvorakPunctuation = enabled
            }
            .onChange(of: commaPeriodKeys) { _, enabled in
                FullKeyboardSettings().commaPeriodKeys = enabled
            }
            .onChange(of: quickAccents) { _, enabled in
                FullKeyboardSettings().quickAccents = enabled
            }
            .onChange(of: emojiLettersUnderEmojiKey) { _, enabled in
                FullKeyboardSettings().emojiLettersUnderEmojiKey = enabled
            }
            .onChange(of: keyboardThemes) { _, enabled in
                FullKeyboardSettings().keyboardThemesEnabled = enabled
            }
            .onChange(of: keyboardTheme) { _, theme in
                FullKeyboardSettings().keyboardTheme = theme
            }
    }

    private var page: some View {
        SettingsPageScaffold(title: "Experiments") {
            SettingsCard { rows }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: adaptiveTouch)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: keyboardThemes)
    }

    /// The experiment switches, in the card's order.
    @ViewBuilder
    private var rows: some View {
        SettingsToggleRow(
            title: String(localized: "Hinglish (Hindi in Latin letters)"),
            detail: String(localized: "Adds Hinglish to dictation languages"),
            isOn: $hinglish
        )

        SettingsDivider()

        SettingsToggleRow(
            title: String(localized: "Greeklish (Greek in Latin letters)"),
            detail: String(localized: "Adds Greeklish to dictation languages"),
            isOn: $greeklish
        )

        SettingsDivider()

        // Off by default: dictation keeps the one language it resolved.
        SettingsToggleRow(
            title: String(localized: "Two-language dictation"),
            detail: String(localized: "Also reads dictation in another of your keyboard languages, when its Apple Speech model is downloaded, and keeps the better one for each phrase."),
            isOn: $twoLanguageDictation
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
            detail: String(localized: "Learns on this iPhone where you tap each key. Stores no text."),
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
            .confirmationDialog("Reset touch learning?", isPresented: $confirmingTouchReset, titleVisibility: .visible) {
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
            detail: String(localized: "Needs Spatial autocorrect"),
            isOn: $germanSpatialAutocorrect
        )

        SettingsDivider()

        // Off by default: only English and Spanish swipe, and any other language's drag stays
        // a slide to another key.
        SettingsToggleRow(
            title: String(localized: "Swipe typing in more languages"),
            detail: String(localized: "French, Polish and German too"),
            isOn: $swipeMoreLanguages
        )

        SettingsDivider()

        // Off by default: 한국어 isn't offered in Languages and a stored "ko" is ignored.
        SettingsToggleRow(
            title: String(localized: "Korean keyboard (두벌식)"),
            detail: String(localized: "Adds 한국어, composing syllables as you type"),
            isOn: $koreanKeyboard
        )

        SettingsDivider()

        // Off by default: English's Dvorak keeps Apple's keys until this is on.
        SettingsToggleRow(
            title: String(localized: "Dvorak punctuation keys"),
            detail: String(localized: "Adds comma, period and semicolon keys to the English Dvorak keyboard."),
            isOn: $dvorakPunctuation
        )

        SettingsDivider()

        // Off by default: the space bar keeps Apple's width until this is on.
        SettingsToggleRow(
            title: String(localized: "Comma and period keys"),
            detail: String(localized: "Puts comma and period keys beside the space bar on the letters keyboard."),
            isOn: $commaPeriodKeys
        )

        SettingsDivider()

        // Off by default: the callout opens on the key's own letter, as before.
        SettingsToggleRow(
            title: String(localized: "Quick accents"),
            detail: String(localized: "Holding a letter selects its main accent, so letting go types it. Slide to pick another."),
            isOn: $quickAccents
        )

        SettingsDivider()

        // Off by default: the emoji panel's ABC stays where it always was.
        SettingsToggleRow(
            title: String(localized: "ABC under the emoji key"),
            detail: String(localized: "In the emoji panel, ABC takes the emoji key's place"),
            isOn: $emojiLettersUnderEmojiKey
        )

        SettingsDivider()

        // Off by default: the full keyboard keeps iOS's glass.
        SettingsToggleRow(
            title: String(localized: "Keyboard themes"),
            detail: String(localized: "Choose a color for the full keyboard"),
            isOn: $keyboardThemes
        )

        if keyboardThemes {
            SettingsDivider()
            KeyboardThemeRow(theme: $keyboardTheme)
        }

        SettingsDivider()

        // Off by default: a second finger on the trackpad is ignored, as before.
        SettingsToggleRow(
            title: String(localized: "Two-thumb selection"),
            detail: String(localized: "On the space bar trackpad, a second thumb selects text"),
            isOn: $twoThumbSelection
        )

        SettingsDivider()

        // Off by default: delete right after a swipe takes one letter, as anywhere else.
        SettingsToggleRow(
            title: String(localized: "Delete swiped words letter by letter"),
            detail: String(localized: "Instead of the whole word at once"),
            isOn: $deletesSwipedWordsLetterByLetter
        )

        SettingsDivider()

        // Off by default: the strip above the keys hides its words, not itself.
        SettingsToggleRow(
            title: String(localized: "Hide word suggestions"),
            detail: String(localized: "Autocorrect still works"),
            isOn: $hideWordSuggestions
        )

        SettingsDivider()

        // Off by default: the Duo unfolded still splits as Apple's does there.
        SettingsToggleRow(
            title: String(localized: "Join the keyboard when unfolded"),
            detail: String(localized: "On the iPhone Duo unfolded, keeps the keys on one keyboard instead of splitting them."),
            isOn: $joinsKeyboardWhenUnfolded
        )

        SettingsDivider()

        // Off by default: one shared list of learned words and fixes, as before.
        SettingsToggleRow(
            title: String(localized: "Learn each language separately"),
            detail: String(localized: "Learned words and fixes stay with their language"),
            isOn: $learnPerLanguage
        )

        SettingsDivider()

        // Off by default: Parakeet Ultra (ParakeetUltraTranscriber) for its 25 languages,
        // downloaded when turned on and deleted when turned off.
        SettingsToggleRow(
            title: String(localized: "Multilingual model (Parakeet)"),
            detail: multilingualParakeetFits
                ? String(localized: "25 languages, even mixed in one sentence. \(Self.multilingualParakeetSize) download. Once ready, keyboard dictation opens Hex.")
                : String(localized: "Needs an iPhone with 6 GB of memory or more"),
            isOn: $multilingualParakeet
        )
        .disabled(!multilingualParakeetFits)

        if multilingualParakeet {
            MultilingualModelStatus(
                download: DictationController.shared.multilingualDownload,
                retry: { DictationController.shared.retryMultilingualParakeet() },
                turnOff: { multilingualParakeet = false }
            )
            .padding(.horizontal, 12)
            .padding(.bottom, 12)
        }

        SettingsDivider()

        // On unless turned off: the keyboard's own English dictation transcribes with Parakeet in
        // the keyboard once downloaded (by itself on Wi-Fi) and compiled there; off, it keeps
        // Apple's speech recognition.
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
    }

    /// Reads every switch back from its setting, when the page appears.
    private func loadSettings() {
        hinglish = HinglishSettings().isEnabled
        greeklish = GreeklishSettings().isEnabled
        twoLanguageDictation = TwoLanguageDictationSettings().isEnabled
        appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
        cleanupAvailability = AppleIntelligenceCleanupAvailability.current
        adaptiveTouch = AdaptiveTouchSettings().isEnabled
        refreshAdaptiveTouchStatus()
        germanSpatialAutocorrect = FullKeyboardSettings().germanSpatialAutocorrect
        swipeMoreLanguages = FullKeyboardSettings().swipeMoreLanguages
        koreanKeyboard = FullKeyboardSettings().koreanKeyboard
        dvorakPunctuation = FullKeyboardSettings().dvorakPunctuation
        commaPeriodKeys = FullKeyboardSettings().commaPeriodKeys
        quickAccents = FullKeyboardSettings().quickAccents
        emojiLettersUnderEmojiKey = FullKeyboardSettings().emojiLettersUnderEmojiKey
        keyboardThemes = FullKeyboardSettings().keyboardThemesEnabled
        keyboardTheme = FullKeyboardSettings().keyboardTheme
        twoThumbSelection = FullKeyboardSettings().twoThumbSelection
        deletesSwipedWordsLetterByLetter = FullKeyboardSettings().deletesSwipedWordsLetterByLetter
        hideWordSuggestions = FullKeyboardSettings().hideWordSuggestions
        joinsKeyboardWhenUnfolded = FullKeyboardSettings().joinsKeyboardWhenUnfolded
        learnPerLanguage = TypingJournalSettings().learnsPerLanguage
        multilingualParakeet = MultilingualParakeetSettings().isEnabled
        keyboardParakeet = KeyboardParakeetSettings().isOn
    }

    private static var multilingualParakeetSize: String {
        ParakeetUltraTranscriber.downloadSize
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

/// Experiments › Keyboard themes: the theme the full keyboard draws while it's on.
private struct KeyboardThemeRow: View {
    @Binding var theme: KeyboardTheme

    var body: some View {
        HStack(spacing: 12) {
            SettingsRowLabel(title: String(localized: "Theme"))
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

/// Keyboard › Emoji: the full keyboard's emoji key, and while it's on, Clear emoji history.
private struct EmojiRows: View {
    @Binding var emoji: Bool
    @Binding var historyEmpty: Bool
    let fullKeyboard: Bool

    var body: some View {
        SettingsToggleRow(
            title: String(localized: "Emoji"),
            detail: fullKeyboard
                ? String(localized: "Emoji key and suggestions")
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
