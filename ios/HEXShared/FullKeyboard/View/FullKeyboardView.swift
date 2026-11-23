import Combine
import UIKit

/// The experimental full keyboard: a 3-slot suggestion strip, QWERTY keys with the Hex key
/// right of space, long-press accents, and the recording panel that replaces both while Hex records.
/// Dictation state comes from the same `KeyboardViewModel` the dictation bar uses.
@MainActor
final class FullKeyboardView: UIView {
    let model: KeyboardViewModel
    let engine: FullKeyboardEngine
    weak var hostController: UIInputViewController? {
        didSet {
            configureGlobe()
            // `init` resolves the language before the controller attaches (it is the caller's
            // to set), so report what is already known, not only later changes.
            if let reportedInputModeLanguage {
                hostController?.primaryLanguage = reportedInputModeLanguage
            }
        }
    }

    /// Fixtures force light or dark and portrait or landscape; the keyboard follows its field.
    var appearanceOverride: Bool? { didSet { applyAppearance() } }
    var orientationOverride: KeyboardOrientation? { didSet { setNeedsLayout() } }
    /// Haptics need Full Access.
    var allowsHaptics: () -> Bool = { false }
    /// Draws a stand-in backdrop where there is no system keyboard behind the view.
    var drawsBackdrop = false { didSet { applyAppearance() } }
    /// The keys typed, deleted, or pasted something (Learn how I type follows dictations).
    var onTextEdit: (() -> Void)?
    /// Undo dictation took the last dictation back.
    var onUndoDictation: (() -> Void)?
    /// The return key is about to type its line break.
    var onReturnKey: (() -> Void)?

    let stripView = KeyboardStripView()
    /// The strip's two-thumb selection: the range, then its actions (Settings › Experiments).
    let selectionStrip = TrackpadSelectionStripView()
    /// The keys' touch surface; tests feed it touch samples.
    let plane: KeyboardKeyPlaneView
    let callout = KeyCalloutView()
    let accentCallout = AccentCalloutView()
    let hexKey = HexKeyControl()
    /// A theme's own opaque panel behind every key and the strip, at the glass panel's own
    /// frame and top corners (`KeyboardThemePanelView`): this view's own background stays
    /// clear, since filling its whole rect would draw square corners over the app. Hidden
    /// under Automatic, which leaves iOS's glass exactly as it was.
    let themePanel = KeyboardThemePanelView()
    let globeButton = UIButton(type: .custom)
    /// The words' side of the soft edge the recording bar's dots come out of the Hex key behind
    /// (`StripRevealMask`); only while the bar is coming, shown, or going.
    private var wordsMask: StripRevealMask?
    private var recordingPanel: HexKeyRecordingPanel?
    /// The words so far over the keys while recording (Show words as you speak).
    let livePreview = LiveDictationPreviewHost()
    private var styleMenu: HexStyleMenuView?
    private(set) var palette = FullKeyboardPalette(isDark: false)
    private var didApplyPalette = false
    private var subscriptions: Set<AnyCancellable> = []
    private(set) var showsRecording = false
    /// The Hex key's last tap or hold started a dictation here (not by opening Hex).
    private var dictatingFromHexKey = false
    /// A finger on the bar where no word is: holding it dictates as holding the Hex key does.
    private var barHold = HexKeyGesture()
    private var barHoldTouch: UITouch?
    private var barHoldWork: DispatchWorkItem?
    private var lastDictation: DictationUndo?
    private let inputModeListTouch = InputModeListTouch()
    /// The model's stage last seen, for a selection's dictation ending
    /// (`FullKeyboardView+Selection`).
    var lastStagePhase: KeyboardViewModel.StagePhase?
    var selectionDictationStarted = false
    var selectionDictationClear: DispatchWorkItem?
    /// A selection took the strip: whether it was hidden before, so it goes back that way.
    var stripWasHiddenBeforeSelection = false
    var selectionTookStrip = false

    /// A word you typed whose slot you held: the strip offers Add to vocabulary until the text
    /// changes (`FullKeyboardView+Vocabulary`).
    var vocabularyOffer: String?
    private var undoHideWork: DispatchWorkItem?
    /// The Hex key was tapped without Full Access: the strip says what dictation needs.
    var showsFullAccessHint = false
    var fullAccessHintHideWork: DispatchWorkItem?
    /// How long the Full Access hint stays unless a key or the strip is used first.
    static let fullAccessHintDuration: TimeInterval = 8
    private let haptics = UIImpactFeedbackGenerator(style: .light)
    /// Every key's touch-down tick (letters, space, delete and its repeats, shift, 123,
    /// return, suggestions), as hard as Settings › Keyboard › Strength asks. The Hex key's hold
    /// and its menu keep `haptics`' fuller thud.
    var hapticStrength: KeyHapticStrength = .medium {
        didSet {
            guard hapticStrength != oldValue else { return }
            keyTicks = UIImpactFeedbackGenerator(style: Self.keyTickStyle(hapticStrength))
            if allowsHaptics() { keyTicks.prepare() }
        }
    }
    private var keyTicks = UIImpactFeedbackGenerator(style: FullKeyboardView.keyTickStyle(.medium))

    /// Apple's key haptic is a crisp tick, so every strength is a rigid impact (the sharpest
    /// style) but Strong, a fuller medium one. Medium, the default, is 0.8 of a rigid tap;
    /// Hex's was a 0.65 light impact, which testers found much weaker than Apple's.
    static func keyTickStyle(_ strength: KeyHapticStrength) -> UIImpactFeedbackGenerator.FeedbackStyle {
        strength == .strong ? .medium : .rigid
    }

    static func keyTickIntensity(_ strength: KeyHapticStrength) -> CGFloat {
        switch strength {
        case .light: 0.5
        case .medium: 0.8
        case .strong: 1
        }
    }
    private let selectionHaptics = UISelectionFeedbackGenerator()
    let clipboard = KeyboardClipboardTracker()
    /// What the strip's Paste chip offers now.
    var stripOffer: KeyboardClipboardOffer?
    /// Settings › Experiments › Hide word suggestions: the strip keeps its height, chips,
    /// status and notices but shows no words (`refreshStrip`).
    private var hidesWordSuggestions = false
    /// Settings › Experiments › Emoji shortcodes, cached by `syncEmojiShortcodes`: the strip
    /// offers emoji for a ":name" and the closing ":" completes one while it is on.
    private(set) var emojiShortcodesEnabled = false
    /// Settings › Experiments › Keyboard themes: Automatic keeps iOS's glass; another theme
    /// paints the keyboard's own colours. Read again by `syncSettings`, which rebuilds the
    /// palette when it changes.
    private var keyboardTheme = FullKeyboardSettings().effectiveKeyboardTheme
    /// A paste iOS asked about or refused: the strip points to the setting that stops it.
    var showsAllowPasteHint = false
    var showsModelNotice = false
    var allowPasteHintHideWork: DispatchWorkItem?
    static let allowPasteHintDuration: TimeInterval = 8

    // MARK: Emoji (Settings › Keyboard › Emoji; FullKeyboardView+Emoji.swift)

    /// The keys' target: the field, or the emoji search while you search.
    let routedTarget: EmojiRoutedTarget
    /// Fixtures turn the emoji experiment on or off; otherwise the App Group's switch.
    var emojiOverride: Bool? { didSet { refreshEmojiKey() } }
    /// Fixtures and tests turn the ABC experiment on or off; otherwise the App Group's switch.
    var emojiLettersUnderEmojiKeyOverride: Bool? { didSet { setNeedsLayout() } }
    /// Where emoji use is kept; fixtures give a seeded store.
    var makeEmojiUsage: () -> EmojiUsageStore = { EmojiUsageStore() }
    var emojiLibrary: EmojiLibrary?
    var emojiPanel: EmojiPanelView?
    var emojiSearchField: EmojiSearchField?
    var emojiResults: EmojiRowView?
    var emojiResultEntries: [Int] = []
    var emojiSearchLift: CGFloat = 0
    /// Record typing logs, held back while you type a search.
    var suspendedTypingLog: TypingLogger?
    /// From the emoji key to the panel's suggestions laid out, for probes.
    var lastEmojiPanelLatency: TimeInterval?

    /// Exposed so unit tests can verify lazy allocation and teardown of the Metal recording panel.
    var hasAllocatedRecordingPanel: Bool { recordingPanel != nil }

    init(model: KeyboardViewModel, target: KeyboardTextTarget, autocorrect: AutocorrectEngine? = nil) {
        self.model = model
        routedTarget = EmojiRoutedTarget(document: target)
        engine = FullKeyboardEngine(target: routedTarget, autocorrect: autocorrect)
        plane = KeyboardKeyPlaneView(engine: engine)
        super.init(frame: .zero)
        backgroundColor = .clear

        // Under the glass panel's rim, under every key: the keys' pop-ups overflow this
        // view, which must never clip them, so the panel is a subview, not this view's fill.
        insertSubview(themePanel, at: 0)
        addSubview(stripView)
        addSubview(plane)
        addSubview(selectionStrip)
        addSubview(hexKey)
        addSubview(globeButton)
        addSubview(callout)
        addSubview(accentCallout)
        accentCallout.isHidden = true

        stripView.onSelectSuggestion = { [weak self] suggestion in
            guard let self else { return }
            self.keyFeedback()
            if suggestion.kind == .emoji {
                if self.acceptEmojiShortcode(suggestion) { return }
                self.recordEmojiUse(suggestion.replacement)
            }
            self.engine.acceptSuggestion(suggestion)
        }
        stripView.onHoldSuggestion = { [weak self] suggestion in self?.offerVocabulary(suggestion) }
        stripView.onUndoDictation = { [weak self] in self?.undoDictation() }
        stripView.onPaste = { [weak self] in self?.pasteFromClipboard() }
        stripView.onOpenHexFromHint = { [weak self] in self?.openSetupFromHint() }
        stripView.onDismissNotice = { [weak self] in self?.dismissModelNotice() }
        stripView.onOpenNotice = { [weak self] in
            guard let self else { return }
            self.keyFeedback()
            if self.showsAllowPasteHint {
                self.openPasteSettingsFromHint()
                return
            }
            if let word = self.vocabularyOffer {
                self.addToVocabulary(word)
                return
            }
            if self.modelNotice != nil { return self.openHexForModel() }
            self.model.onOpenURL?(KeyboardLaunchRequest.translationSettings.url)
        }
        stripView.onNextKeyboard = { [weak self] in
            // Never where Apple's keyboard is split, where the strip doesn't offer it either
            // (`InputModeListTouch`).
            guard let self, self.currentPosture?.splitsKeyboard == false else { return }
            self.typingLog?.ui("next_keyboard")
            self.hostController?.advanceToNextInputMode()
        }
        stripView.onKeyboardListTouch = { [weak self] chip, event in
            if event.touches(for: chip)?.first?.phase == .began { self?.typingLog?.ui("keyboard_list") }
            guard let self, let controller = self.hostController else { return }
            self.inputModeListTouch.forward(chip, with: event, to: controller, alwaysHoldsBackTap: true)
        }
        selectionStrip.onAction = { [weak self] action in self?.selectionAction(action) }

        globeButton.accessibilityLabel = "Next keyboard"
        globeButton.isAccessibilityElement = true
        globeButton.addTarget(self, action: #selector(globeTapped), for: .touchUpInside)

        hexKey.onPress = { [weak self] at in self?.model.onHexKeyPress?(at) }
        hexKey.onPressAbandoned = { [weak self] in self?.model.onHexKeyAbandon?() }
        hexKey.onHold = { [weak self] in self?.hexKeyHeld() }
        hexKey.onHoldRelease = { [weak self] in self?.hexKeyReleased() }
        hexKey.onMenuOpen = { [weak self] point in
            guard let self else { return }
            // Styles and languages are for dictation, which needs Full Access.
            guard self.model.hasFullAccess else {
                self.showFullAccessHint()
                return
            }
            self.typingLog?.ui("hex_menu")
            self.showStyleMenu()
            self.forwardMenuPoint(point, release: false)
        }
        hexKey.onMenuDrag = { [weak self] point in self?.forwardMenuPoint(point, release: false) }
        hexKey.onMenuRelease = { [weak self] point in self?.forwardMenuPoint(point, release: true) }

        plane.onKeyDown = { [weak self] _ in self?.keyFeedback() }
        plane.onAccentMenuOpened = { [weak self] in self?.selectionFeedback() }
        engine.onSelectionStep = { [weak self] in self?.selectionFeedback() }
        engine.onChange = { [weak self] in self?.engineChanged() }
        engine.onEdit = { [weak self] in
            guard let self else { return }
            if self.routedTarget.isSearching {
                self.refreshEmojiSearch()
                return
            }
            self.hideUndo()
            self.vocabularyOffer = nil
            self.hideFullAccessHint()
            self.hideAllowPasteHint()
            self.hideModelNotice()
            self.refreshReturnKey()
            self.refreshStrip()
            self.onTextEdit?()
        }
        engine.onReturn = { [weak self] in
            guard self?.routedTarget.isSearching == false else { return }
            self?.onReturnKey?()
        }
        engine.onEmojiKey = { [weak self] in self?.emojiKeyTapped() }
        engine.onDismissKey = { [weak self] in self?.hostController?.dismissKeyboard() }
        engine.onLanguageSwipe = { [weak self] step in self?.switchLanguage(by: step) }
        routedTarget.onSearchReturn = { [weak self] in self?.endEmojiSearch() }

        model.objectWillChange
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.refreshFromModel() }
            .store(in: &subscriptions)

        registerForTraitChanges([UITraitUserInterfaceStyle.self, UITraitVerticalSizeClass.self]) { (view: FullKeyboardView, _) in
            view.applyAppearance()
            view.setNeedsLayout()
        }

        engine.autocorrect.isEnabled = FullKeyboardSettings().autocorrect
        engine.autocorrect.allowsSwearing = FullKeyboardSettings().suggestSwearWords
        engine.autocorrect.setVocabulary(CustomVocabularySettings().terms)
        engine.autoCapitalizes = FullKeyboardSettings().autoCapitalization
        engine.showsKeyPopups = FullKeyboardSettings().keyPopups
        engine.trackpadSelectionState.enabled = FullKeyboardSettings().twoThumbSelection
        engine.deletesSwipedWordsLetterByLetter = FullKeyboardSettings().deletesSwipedWordsLetterByLetter
        engine.quickAccentsEnabled = FullKeyboardSettings().quickAccents
        syncEmojiShortcodes()
        hidesWordSuggestions = FullKeyboardSettings().hideWordSuggestions
        syncHexPlacement()
        syncNumberRow()
        syncJoinsUnfoldedKeyboard()
        syncDvorakPunctuation()
        syncCommaPeriodKeys()
        refreshEmojiKey()
        syncSpatialAutocorrect()
        refreshLanguageSignals()
        syncLanguage()
        syncSwipeTyping()
        engine.configureForField()
        applyAppearance()
        livePreview.attach(to: self)
        refreshFromModel()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    // MARK: - Typing logs

    /// Record typing logs (Experiments): set by the keyboard while a session may be logged,
    /// nil otherwise. The app's fixtures never set it.
    var typingLog: TypingLogger? {
        get { routedTarget.isSearching ? suspendedTypingLog : engine.typingLog }
        set {
            if routedTarget.isSearching {
                suspendedTypingLog = newValue
            } else {
                engine.typingLog = newValue
            }
        }
    }

    /// A typing log session began: write the layout, shift, and suggestions as they are now.
    func typingLogSessionBegan() {
        guard let log = engine.typingLog else { return }
        log.viewGeometry(size: bounds.size, safeArea: safeAreaInsets)
        engine.logState("session")
        log.commitGeometry()
        log.suggestionsChanged(engine.suggestions)
    }

    @objc private func globeTapped() {
        typingLog?.ui("globe")
    }

    // MARK: - Size

    static func orientation(for traits: UITraitCollection) -> KeyboardOrientation {
        traits.verticalSizeClass == .compact ? .landscape : .portrait
    }

    var orientation: KeyboardOrientation {
        orientationOverride ?? padOrientation ?? Self.orientation(for: traitCollection)
    }

    /// On an iPad, which way up its screen is (`KeyboardPadPlacement.orientation`); nil on an iPhone.
    private var padOrientation: KeyboardOrientation?

    var preferredHeight: CGFloat {
        keysHeight + lift
    }

    /// The strip and the keys, without the lift.
    private var keysHeight: CGFloat {
        KeyboardLayout.height(
            for: orientation, posture: engine.posture, phoneSize: engine.phoneSize, pad: engine.pad, numberRow: engine.hasNumberRow
        )
    }

    /// Room above the strip for the emoji search results. The strip itself keeps its height in
    /// every state, the recording bar's included.
    private var lift: CGFloat { emojiSearchLift }

    /// Asks the host to apply `preferredHeight` now (the keyboard extension resizes itself).
    var onPreferredHeightChange: (() -> Void)?

    /// How much of the lift the view actually has yet, so nothing jumps before the host grows it.
    private var appliedLift: CGFloat {
        max(0, min(lift, bounds.height - keysHeight))
    }

    /// Reads which device and posture the keyboard is on, and how far iOS set it in from the
    /// screen's sides, from its window. The Duo's unfolded screen has regular size classes either
    /// way up, so the size classes alone can't tell its postures apart.
    private func syncPosture() {
        guard let screen = window?.windowScene?.screen ?? window?.screen else { return }
        let size = screen.bounds.size
        let posture = KeyboardPosture.current(screenSize: size, isFoldable: Self.isFoldable)
        let inset = posture == .phone ? 0 : max(0, (size.width - bounds.width) / 2)
        let phoneSize = KeyboardPhoneSize.current(screenSize: size)
        let pad = Self.isPad ? KeyboardPadPlacement.current(screenSize: size, keyboardWidth: bounds.width) : nil
        let padOrientation = pad.map { KeyboardPadPlacement.orientation(screenSize: size, placement: $0, keyboardWidth: bounds.width) }
        let postureChanges = posture != engine.posture
        let heightChanges = postureChanges || phoneSize != engine.phoneSize
            || pad != engine.pad || padOrientation != self.padOrientation
        engine.posture = posture
        engine.phoneSize = phoneSize
        engine.windowInset = inset
        engine.pad = pad
        self.padOrientation = padOrientation
        syncGlobe()
        if postureChanges { refreshStrip() }
        if heightChanges { onPreferredHeightChange?() }
    }

    /// The posture as the keyboard's window shows it now; nil on the Duo before it has one.
    var currentPosture: KeyboardPosture? { KeyboardPosture.current(in: window) }

    /// Hex's globe key where iOS asks for one, except floating on an iPad: iPadOS draws its own
    /// globe in the band under a floating keyboard.
    private func syncGlobe() {
        let globe = model.needsGlobeKey && engine.pad != .floating
        guard engine.showsGlobe != globe else { return }
        engine.showsGlobe = globe
        setNeedsLayout()
    }

    /// An iPad (the keyboard runs as an iPad keyboard even though the app is iPhone-only).
    static let isPad = UIDevice.current.userInterfaceIdiom == .pad

    /// The iPhone Duo (`KeyboardPosture.foldableModels`), read once per process.
    static let isFoldable: Bool = {
        if let simulated = ProcessInfo.processInfo.environment["SIMULATOR_MODEL_IDENTIFIER"] {
            return KeyboardPosture.foldableModels.contains(simulated)
        }
        var system = utsname()
        uname(&system)
        let model = withUnsafeBytes(of: &system.machine) { raw in
            String(decoding: raw.prefix { $0 != 0 }, as: UTF8.self)
        }
        return KeyboardPosture.foldableModels.contains(model)
    }()

    // MARK: - Host events

    func warmUpTextCheckerAsync() {
        DispatchQueue.main.async { [weak self] in
            guard let self, let checker = self.engine.autocorrect.checker as? SystemTextChecker else { return }
            checker.warmUp()
            // German's capitalisation list reads with the dictionary, off the open path, and only
            // while the German model is on: the dictionary engine never asks for it.
            if self.engine.language == .german, self.engine.letterOdds != nil,
               self.engine.autocorrect.germanCapitals == nil {
                self.engine.autocorrect.germanCapitals = GermanCapitals.bundled()
            }
            self.engine.refreshSuggestions()
        }
    }

    func loadLexicon(_ lexicon: UILexicon) {
        engine.autocorrect.loadLexicon(lexicon)
        engine.refreshSuggestions()
    }

    // MARK: - Language

    /// Fixtures pin one layout; otherwise Settings › Keyboard › Languages, whose Automatic
    /// follows the dictation language.
    var languageOverride: KeyboardLanguage? {
        didSet { syncLanguage() }
    }

    /// Fixtures pin how the letters are arranged; otherwise Settings › Keyboard › Languages ›
    /// Layout for the keys' language. Without the App Group (an iPhone that keeps it from a
    /// keyboard without Full Access) every language types on its own layout, as before.
    var letterLayoutOverride: KeyboardLetterLayout? {
        didSet { syncLanguage() }
    }

    /// Where Languages and the language last switched to are kept; tests give their own.
    var languageSettings = LanguageSettings() {
        didSet { syncLanguage() }
    }

    /// The languages the space bar switches between and the one typing now.
    private(set) var languages = KeyboardLanguageList(languages: [.english], current: .english)
    /// The input mode's language last reported to the host (`UIInputViewController.primaryLanguage`),
    /// so the host's spelling and autocorrection follow the language typing.
    private(set) var reportedInputModeLanguage: String?
    /// The language this keyboard last switched to, ahead of the App Group's copy, which an
    /// iPhone may not let a keyboard without Full Access write.
    private var switchedLanguage: KeyboardLanguage?
    private static let spellCheckLanguages = UITextChecker.availableLanguages

    /// The iPhone's keyboards and preferred languages, read when the keyboard appears, for
    /// Automatic before any dictation language is known.
    private var inputModeLanguages: [String] = []
    private var preferredLanguages: [String] = []

    private func refreshLanguageSignals() {
        inputModeLanguages = UITextInputMode.activeInputModes.compactMap(\.primaryLanguage)
        preferredLanguages = Locale.preferredLanguages
    }

    private func syncLanguage() {
        let settings = languageSettings
        let list = if let languageOverride {
            KeyboardLanguageList(languages: [languageOverride], current: languageOverride)
        } else {
            KeyboardLanguageList.resolve(
                chosen: settings.keyboardLanguages,
                stored: switchedLanguage ?? settings.currentKeyboardLanguage,
                automatic: KeyboardLanguage.automatic(
                    dictationLanguage: settings.effectiveDictationLanguage,
                    inputModes: inputModeLanguages,
                    preferredLanguages: preferredLanguages
                )
            )
        }
        // The keys' language's layout: with English and Spanish, or German, on one keyboard, the first one's.
        let letters = letterLayoutOverride.map(list.layout.offered) ?? settings.letterLayout(for: list.layout)
        // The host spells and autocorrects in the input mode's language, which supersedes the
        // Info.plist's `mul` (`primaryLanguage`): report every change, a space-bar switch too.
        let identifier = list.current.inputModeLanguage
        if identifier != reportedInputModeLanguage {
            reportedInputModeLanguage = identifier
            hostController?.primaryLanguage = identifier
        }
        guard list != languages || engine.language != list.current || engine.letterLayout != letters else { return }
        languages = list
        engine.switchesLanguage = list.switches
        engine.layoutLanguage = list.layout == list.current ? nil : list.layout
        engine.letterLayout = letters
        engine.language = list.current
        engine.partnerLanguage = list.partner(available: Self.spellCheckLanguages)
        syncSpatialAutocorrect()
        refreshSpaceLabel()
        syncSwipeTyping()
        setNeedsLayout()
    }

    /// A swipe along the space bar: the next or previous language (its letters, accents and
    /// autocorrect), kept for next time. Dictation on Automatic follows it
    /// (`LanguageSettings.keyboardDictationLanguage`); with Switch dictation too, a dictation
    /// language picked by hand goes back to Automatic, so it follows too.
    func switchLanguage(by step: Int) {
        guard languages.switches, languageOverride == nil else { return }
        let next = languages.switched(by: step)
        typingLog?.ui("keyboard_language", [TypingLogField("language", .string(next.rawValue))])
        switchedLanguage = next
        languageSettings.currentKeyboardLanguage = next
        selectionFeedback()
        syncLanguage()
        guard languageSettings.dictationFollowsKeyboard, model.hasFullAccess,
              languageSettings.dictationLanguage != nil else { return }
        model.onLanguageSelected?(nil)
    }

    /// With two or more languages the space bar names the keyboard's; otherwise, with Full
    /// Access, the dictation language's.
    private func refreshSpaceLabel() {
        plane.spaceLabel = languages.spaceLabel ?? (model.hasFullAccess ? model.languageSummary : nil)
    }

    /// Settings › Experiments › Emoji shortcodes, in the App Group: cached for the strip, and
    /// the engine's closing ":" hook is set only while it is on, so an off keyboard types a
    /// colon exactly as it did before the experiment.
    private func syncEmojiShortcodes() {
        emojiShortcodesEnabled = FullKeyboardSettings().emojiShortcodes
        engine.emojiShortcode = emojiShortcodesEnabled
            ? { [weak self] before in self?.emojiShortcodeReplacement(before: before) }
            : nil
    }

    func syncSettings() {
        syncLanguage()
        // Learn each language separately: each keyboard language's own learned words, and the
        // partner's own; Learn how I type's journal filters its fixes the same way.
        let learnsPerLanguage = TypingJournalSettings().learnsPerLanguage
        if engine.autocorrect.learnsPerLanguage != learnsPerLanguage {
            engine.autocorrect.learnsPerLanguage = learnsPerLanguage
            engine.refreshSuggestions()
        }
        let journal = TypingJournal.sharedIfEnabled()
        if engine.journal !== journal {
            engine.journal = journal
        }
        // Adaptive touch keeps its averages on disk only with Full Access; without, in memory.
        let adaptive = AdaptiveTouch.sharedIfEnabled(persistent: hostController?.hasFullAccess == true)
        if engine.adaptiveTouch !== adaptive {
            engine.adaptiveTouch = adaptive
        }
        engine.korean.isEnabled = FullKeyboardSettings().koreanKeyboard
        engine.showsKeyPopups = FullKeyboardSettings().keyPopups
        engine.autoCapitalizes = FullKeyboardSettings().autoCapitalization
        engine.deletesSwipedWordsLetterByLetter = FullKeyboardSettings().deletesSwipedWordsLetterByLetter
        engine.quickAccentsEnabled = FullKeyboardSettings().quickAccents
        syncEmojiShortcodes()
        let selectionEnabled = FullKeyboardSettings().twoThumbSelection
        engine.trackpadSelectionState.enabled = selectionEnabled
        // Turned off mid-selection: end it now rather than let a thumb keep picking.
        if !selectionEnabled, engine.isSelectionActive {
            plane.cancelAllTouches()
        }
        let hidesSuggestions = FullKeyboardSettings().hideWordSuggestions
        if hidesSuggestions != hidesWordSuggestions {
            hidesWordSuggestions = hidesSuggestions
            refreshStrip()
        }
        let theme = FullKeyboardSettings().effectiveKeyboardTheme
        if theme != keyboardTheme {
            keyboardTheme = theme
            applyAppearance()
        }
        hapticStrength = FullKeyboardSettings().hapticStrength
        syncHexPlacement()
        syncNumberRow()
        syncJoinsUnfoldedKeyboard()
        syncDvorakPunctuation()
        syncCommaPeriodKeys()
        refreshEmojiKey()
        let wantsAutocorrect = FullKeyboardSettings().autocorrect
        if engine.autocorrect.isEnabled != wantsAutocorrect {
            engine.autocorrect.isEnabled = wantsAutocorrect
            engine.refreshSuggestions()
        }
        let vocabulary = CustomVocabularySettings().terms
        if engine.autocorrect.vocabulary != vocabulary {
            engine.autocorrect.setVocabulary(vocabulary)
            engine.refreshSuggestions()
        }
        let wantsSwearing = FullKeyboardSettings().suggestSwearWords
        if engine.autocorrect.allowsSwearing != wantsSwearing {
            engine.autocorrect.allowsSwearing = wantsSwearing
            engine.refreshSuggestions()
        }
        syncSpatialAutocorrect()
        syncSwipeTyping()
    }

    /// Fixtures pin where the Hex key sits; otherwise Settings › Keyboard › Hex key.
    var hexPlacementOverride: HexKeyPlacement? { didSet { syncHexPlacement() } }

    /// Fixtures and tests turn Swipe typing on or off; otherwise its switch in Settings › Keyboard.
    var swipeOverride: Bool? { didSet { syncSwipeTyping() } }

    /// Settings › Keyboard › Swipe typing (on by default): on the English and Spanish layouts, a
    /// drag across the letters types a word, read with the layout language's bundled model, and
    /// with Settings › Experiments › Swipe typing in more languages on also in every language
    /// that has one (`FullKeyboardSettings.swipesIn`). Off under VoiceOver, which has its own
    /// way to type. Elsewhere a drag stays a slide to another key.
    private func syncSwipeTyping() {
        let settings = FullKeyboardSettings()
        let on = (swipeOverride ?? settings.swipeTyping) && settings.swipesIn(engine.language)
            && !UIAccessibility.isVoiceOverRunning
        let model = on ? settings.typingModel(for: engine.language) : nil
        if plane.swipe?.model !== model {
            plane.swipe = model.map { SwipeTyping(engine: engine, model: $0) }
        } else {
            plane.swipe?.syncWords()
        }
    }

    /// Settings › Keyboard › Spatial autocorrect (on by default): maps the layout language's
    /// bundled model when the keyboard opens (English, Spanish, French, and German while its experiment
    /// in Settings › Experiments is on; others keep `UITextChecker`), and moves the keys'
    /// centres to where Learn how I type saw you tap. The same word list lets the likely next
    /// letter claim a few points of its neighbour's touch target whether or not Spatial
    /// autocorrect is on (`FullKeyboardEngine.letterOdds`; only while autocorrecting).
    private func syncSpatialAutocorrect() {
        let settings = FullKeyboardSettings()
        let words = settings.typingModel(for: engine.language)
        if engine.letterOdds !== words {
            engine.letterOdds = words
        }
        let model = settings.spatialAutocorrect ? words : nil
        if engine.autocorrect.decoder?.model !== model {
            engine.autocorrect.decoder = model.map { TypingDecoder(model: $0) }
            engine.refreshSuggestions()
        }
        // Bilingual autocorrect reads the same taps with the other language's model, where it has one.
        if let partner = engine.autocorrect.partner {
            let theirs = settings.spatialAutocorrect ? settings.typingModel(for: partner.language) : nil
            if partner.decoder?.model !== theirs {
                partner.decoder = theirs.map { TypingDecoder(model: $0) }
                engine.refreshSuggestions()
            }
        }
        let keys = model == nil ? [:] : (engine.journal?.current.keys ?? [:])
        engine.tapOffsets = TypingDecoder.offsets(from: keys, layout: engine.layout)
    }

    /// The field or its contents changed outside these keys.
    func textDidChange() {
        applyAppearance()
        let field = engine.field
        let route = KeyboardFieldRouting.route(for: engine.target.keyboardType)
        let routeField: KeyboardFieldKind = if case .keys(let kind, _) = route { kind } else { .text }
        // While you search emoji the keys still type the query, whatever the host changed.
        if routedTarget.isSearching {
            refreshReturnKey()
            return
        }
        // A field change (or the host's own edit) ends a range whose text no longer holds,
        // and clears any Dictate still waiting on its words.
        engine.selectionTextChanged()
        if routeField != field {
            plane.cancelAllTouches()
            engine.configureForField()
            refreshEmojiKey()
        } else {
            engine.textDidChange()
        }
        // Moved somewhere it can't take the dictation back from: no button that does nothing.
        if let undo = lastDictation, undoPlan(undo) == nil {
            hideUndo()
        }
        refreshReturnKey()
        refreshStrip()
    }

    /// The keyboard came on screen for a (possibly new) field.
    func prepareForField() {
        closeEmojiPanel()
        refreshLanguageSignals()
        syncSettings()
        plane.cancelAllTouches()
        engine.configureForField()
        refreshReturnKey()
        applyAppearance()
        refreshStrip()
        dismissStyleMenu()
        if allowsHaptics() {
            keyTicks.prepare()
            selectionHaptics.prepare()
        }
    }

    /// Frees non-essential UI and Metal drawables when the extension approaches its memory ceiling.
    func handleMemoryWarning() {
        dismissStyleMenu()
        if emojiPanel == nil {
            emojiLibrary = nil
            EmojiLibrary.evictCached()
        }
        emojiPanel?.glyphs.purge()
        if !showsRecording, let panel = recordingPanel {
            panel.tearDown()
            panel.removeFromSuperview()
            recordingPanel = nil
        }
    }

    /// Hex inserted a dictation between `before` and `after`; offer to take it back until the
    /// next edit, or until it can't be found from the cursor.
    func didInsertDictation(_ text: String, before: String = "", after: String = "") {
        engine.commitHangulComposition()
        lastDictation = DictationUndo(text: text, before: before, after: after)
        engine.clearPendingSelectionDictation()
        refreshStrip()
        undoHideWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.hideUndo() }
        undoHideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 12, execute: work)
        engine.forgetTextAroundKeys()
        engine.textDidChange()
        refreshReturnKey()
    }

    // MARK: - Layout

    override func layoutSubviews() {
        super.layoutSubviews()
        // A theme's panel keeps the glass panel's corners, 1 pt inside its rim.
        themePanel.place(in: bounds)
        syncPosture()
        let isLandscape = orientation == .landscape
        engine.typingLog?.viewGeometry(size: bounds.size, safeArea: safeAreaInsets)
        engine.orientation = orientation
        engine.horizontalInset = isLandscape ? max(safeAreaInsets.left, safeAreaInsets.right) : 0
        engine.width = bounds.width
        let lift = appliedLift
        engine.availableHeight = bounds.height - lift
        engine.typingLog?.commitGeometry()
        let layout = engine.layout

        let content = CGRect(x: 0, y: lift, width: bounds.width, height: bounds.height - lift)
        plane.frame = content
        callout.frame = content
        accentCallout.frame = content
        stripView.frame = layout.stripFrame.offsetBy(dx: 0, dy: lift)
        selectionStrip.frame = stripView.frame
        stripView.slotFrames = layout.stripSlotFrames.map { $0.offsetBy(dx: -layout.stripFrame.minX, dy: -layout.stripFrame.minY) }
        stripView.pillInset = layout.metrics.stripInset
        stripView.textCenterY = layout.metrics.stripTextCenter
        stripView.chipHeight = layout.metrics.stripChipHeight
        stripView.appleBar = layout.metrics.appleBar
        stripView.dividerBesideHex = layout.metrics.appleBar && layout.key(.hex).map { $0.row < 0 } == true
        stripView.hexOnLeading = layout.hexOnLeading
        stripView.barMidX = layout.stripBarFrame.midX - layout.stripFrame.minX
        // The bar from the keyboard's top to the strip's bottom, up to the Hex key beside it.
        var bar = CGRect(x: layout.stripBarFrame.minX, y: 0, width: layout.stripBarFrame.width, height: layout.stripBarFrame.maxY)
        if let hex = layout.key(.hex), hex.row < 0 {
            bar = layout.hexOnLeading
                ? CGRect(x: hex.hitFrame.maxX, y: bar.minY, width: bar.maxX - hex.hitFrame.maxX, height: bar.height)
                : CGRect(x: bar.minX, y: bar.minY, width: hex.hitFrame.minX - bar.minX, height: bar.height)
        }
        stripView.barTouchFrame = bar.offsetBy(dx: -layout.stripFrame.minX, dy: -layout.stripFrame.minY)

        hexKey.cornerRadius = layout.metrics.cornerRadius
        if let hex = layout.key(.hex) {
            hexKey.inStrip = hex.row < 0
            // Bounds and centre: the key may be mid-scale as the recording bar comes and goes.
            let hit = hex.hitFrame.offsetBy(dx: 0, dy: lift)
            hexKey.bounds = CGRect(origin: .zero, size: hit.size)
            hexKey.center = CGPoint(x: hit.midX, y: hit.midY)
            hexKey.keycapFrame = hex.frame.offsetBy(dx: -hex.hitFrame.minX, dy: -hex.hitFrame.minY)
            hexKey.markCenterY = layout.metrics.stripTop + layout.metrics.stripTextCenter - hex.hitFrame.minY
        }
        if let globe = layout.key(.globe), emojiPanel == nil || routedTarget.isSearching {
            globeButton.isHidden = false
            globeButton.frame = globe.hitFrame.offsetBy(dx: 0, dy: lift)
        } else {
            globeButton.isHidden = true
        }
        layoutEmoji(content: content, lift: lift)

        if let panel = recordingPanel {
            panel.frame = recordingPanelFrame
            panel.horizontalInset = layout.horizontalInset
            panel.metrics = layout.metrics
            panel.sideKeyWidth = layout.modifierWidth
            panel.hexOnLeading = layout.hexOnLeading
        }
        if let menu = styleMenu, let hex = layout.key(.hex) {
            menu.frame = bounds
            menu.anchor = hex.frame
        }
        livePreview.layout()
        plane.setNeedsDisplay()
        updateCallout()
    }

    // MARK: - State

    private func applyAppearance() {
        let isDark = appearanceOverride
            ?? FullKeyboardPalette.isDark(traits: traitCollection)
        let next = FullKeyboardPalette(isDark: isDark, theme: keyboardTheme)
        // A theme paints its own opaque panel, so the system glass behind the keys never
        // shows through. The panel, not this view, keeps the glass panel's corners;
        // Automatic leaves both clear as before.
        backgroundColor = drawsBackdrop ? next.backdrop : .clear
        themePanel.isHidden = !next.drawsOwnBackground
        themePanel.backgroundColor = next.drawsOwnBackground ? next.backdrop : .clear
        stripView.inSystemPlatter = !drawsBackdrop
        guard next != palette || !didApplyPalette else { return }
        didApplyPalette = true
        palette = next
        stripView.palette = next
        selectionStrip.palette = next
        plane.palette = next
        callout.palette = next
        callout.setNeedsDisplay()
        accentCallout.palette = next
        hexKey.palette = next
        recordingPanel?.palette = next
        livePreview.palette = next
        styleMenu?.palette = next
        emojiPanel?.palette = next
        emojiSearchField?.palette = next
        emojiResults?.palette = next
    }

    func refreshReturnKey() {
        plane.returnStyle = ReturnKeyStyle(engine.target.returnKeyType)
        plane.returnIsEnabled = ReturnKeyStyle.isEnabled(
            automatically: engine.target.enablesReturnKeyAutomatically,
            hasText: engine.target.hasText
        )
    }

    private func refreshFromModel() {
        hexKey.isWarm = model.dictatesThroughHex
        hexKey.translatingTo = model.translatingTo
        syncGlobe()
        let recording = model.stagePhase == .recording || model.stagePhase == .transcribing
        if recording != showsRecording {
            setRecording(recording)
            livePreview.sync()
        }
        refreshRecordingPanel()
        // A language picked in the switch changes the layout at once when it follows dictation.
        syncLanguage()
        refreshSpaceLabel()
        if let menu = styleMenu {
            menu.style = model.style
            applyLanguages(to: menu)
        }
        trackSelectionDictation()
        refreshStrip()
    }

    private func applyLanguages(to menu: HexStyleMenuView) {
        menu.dictationLanguage = model.dictationLanguage
        menu.automaticLanguage = model.automaticLanguage
        menu.languageChoices = model.showsLanguageSwitch ? model.languageChoices : []
        menu.moreLanguages = model.showsLanguageSwitch ? model.moreLanguages : []
        menu.translationTarget = model.showsTranslationSwitch ? model.translationLanguage : nil
        menu.isTranslating = model.isTranslating
        menu.translationTargets = model.translationTargetChoices
    }

    func refreshStrip() {
        // A two-thumb selection takes the strip: the range while both thumbs are down, then
        // its actions. The keyboard's own strip (suggestions, Undo, chips) waits behind it.
        let selection = engine.selectionPresentation
        selectionStrip.update(selection, canUseClipboard: model.hasFullAccess)
        // Only a selection changes the strip's visibility: the emoji panel and the recording
        // bar hide and show it themselves, and refreshing must leave them alone.
        if selection != nil, !selectionTookStrip {
            stripWasHiddenBeforeSelection = stripView.isHidden
            stripView.isHidden = true
            selectionTookStrip = true
        } else if selection == nil, selectionTookStrip {
            selectionTookStrip = false
            stripView.isHidden = stripWasHiddenBeforeSelection || emojiPanel != nil || showsRecording
        }
        // Without Full Access the strip suggests and corrects like any keyboard's; what
        // dictation needs shows only once the Hex key is tapped (`showFullAccessHint`).
        let status: String? = if model.hasFullAccess, model.stagePhase == .failed {
            model.statusMessage
        } else {
            nil
        }
        var clipboardOffer = clipboard.offer(
            textContentType: engine.target.textContentType,
            keyboardType: engine.target.keyboardType,
            autocorrectionType: engine.target.autocorrectionType,
            fieldIsEmpty: !engine.target.hasText,
            hasFullAccess: model.hasFullAccess,
            canSwitchKeyboard: currentPosture.map { !$0.splitsKeyboard } ?? false
        )
        // A dictation that didn't go in by itself comes before a copy, but not in a code field.
        if model.hasFullAccess, model.offeredDictation != nil {
            if case .code = clipboardOffer {} else { clipboardOffer = .dictation }
        }
        stripOffer = clipboardOffer
        var suggestions = engine.suggestions
        if let shortcodes = emojiShortcodeSuggestions() {
            suggestions = shortcodes
        } else if suggestions.isEmpty || suggestions.first?.kind == .nextWord {
            if lastDictation != nil || status != nil || clipboardOffer != nil {
                suggestions = []
            } else {
                let emoji = emojiForFinishedWord()
                if !emoji.isEmpty {
                    // The finished word's emoji, with next words beside it so every slot fills:
                    // one emoji takes the centre.
                    let words = Array(suggestions.prefix(max(0, 3 - emoji.count)))
                    suggestions = emoji.count == 1 && words.count == 2 ? [words[0], emoji[0], words[1]] : emoji + words
                }
            }
        }
        // The experiment hides the words, not the strip: Undo, Paste, status, notices and the
        // Hex key keep their places, and an empty strip offers the clipboard as it does when no
        // word is suggested. What you typed before a correction (`.revert`) stays, so Delete
        // after a corrected word can still put it back; keep literal, swipe readings, next
        // words and the finished word's emoji go. Autocorrect still reads
        // `engine.suggestions`, never this.
        if hidesWordSuggestions {
            suggestions = suggestions.filter { $0.kind == .revert }
        }
        stripView.update(
            suggestions: suggestions,
            showsUndo: lastDictation != nil,
            statusText: status,
            clipboard: clipboardOffer,
            fullAccessHint: showsFullAccessHint && !model.hasFullAccess,
            notice: allowPasteNotice ?? vocabularyNotice ?? translationNotice ?? modelNotice
        )
    }

    /// The last dictation went in as spoken while Translate what I say was on: what happened,
    /// the fix, and Open Hex, which opens Settings › Dictation.
    private var translationNotice: KeyboardStripNotice? {
        guard model.hasFullAccess, let note = model.translationNote else { return nil }
        return KeyboardStripNotice(title: TranslationFailure.note, detail: note.fix, button: String(localized: "Open Hex"))
    }

    private func refreshRecordingPanel() {
        guard let panel = recordingPanel, showsRecording else { return }
        panel.update(
            phase: model.stagePhase == .transcribing ? .processing : .recording,
            style: model.style
        )
    }

    /// Recording in and out with no shape between: the Hex key hands its spot to ✓ (the key
    /// leaving as ✓ arrives, on the same frame), ✕ slides out from under ✓, and the dots come
    /// out of the key behind a soft edge that runs across the bar, wiping the words as it goes.
    /// Going back, the edge runs home into the key, the words return behind it, ✕ tucks under
    /// ✓ and the key takes its spot back. Reduce Motion crossfades instead.
    private func setRecording(_ recording: Bool) {
        showsRecording = recording
        dismissStyleMenu()
        if recording { closeEmojiPanel(); hideModelNotice() }
        let animated = window != nil
        let travels = animated && !StripMotion.reduced && engine.layout.key(.hex).map { $0.row < 0 } == true
        stripView.holdsStill = recording || recordingPanel != nil
        if recording {
            plane.cancelAllTouches()
            hideUndo()
            let panel = recordingPanel ?? makeRecordingPanel()
            panel.palette = palette
            panel.frame = recordingPanelFrame
            refreshRecordingPanel()
            panel.isHidden = false
            panel.layoutIfNeeded()
            guard travels else {
                endReveal(panel)
                panel.setControls(shown: true)
                fade(in: [panel], out: [stripView, hexKey], duration: animated ? StripMotion.reducedFade : 0)
                return
            }
            panel.alpha = 1
            let fresh = beginReveal(panel)
            if fresh {
                UIView.performWithoutAnimation { panel.setControls(shown: false) }
            }
            StripMotion.animate(StripMotion.grow, fresh: fresh, {
                self.placeReveal(open: true, panel: panel)
            }, completion: { [weak self] _ in
                // Masked away, the words would still take touches.
                guard let self, self.showsRecording else { return }
                self.stripView.isHidden = true
            })
            StripMotion.animate(StripMotion.settle, fresh: fresh) { panel.setControls(shown: true) }
            StripMotion.animate(StripMotion.settleExit, {
                self.hexKey.alpha = 0
                self.hexKey.transform = self.hexKeyTucked
            }, completion: { [weak self] _ in
                // A Hex key still held for push to talk stays in the window (at alpha 0), so its
                // touch keeps arriving and lifting the finger finishes the dictation.
                guard let self, self.showsRecording, !self.hexKey.isTracking else { return }
                self.hexKey.isHidden = true
            })
        } else if let panel = recordingPanel {
            let finish = { [weak self, weak panel] in
                guard let self, !self.showsRecording, let panel, panel === self.recordingPanel else { return }
                self.stripView.holdsStill = false
                self.endReveal(panel)
                panel.tearDown()
                panel.removeFromSuperview()
                self.recordingPanel = nil
            }
            guard travels else {
                endReveal(panel)
                fade(in: [stripView, hexKey], out: [panel], duration: animated ? StripMotion.reducedFade : 0, after: finish)
                return
            }
            stripView.isHidden = false
            stripView.alpha = 1
            let fresh = beginReveal(panel)
            if fresh {
                UIView.performWithoutAnimation { placeReveal(open: true, panel: panel) }
            }
            if hexKey.isHidden || hexKey.alpha == 0 {
                hexKey.isHidden = false
                hexKey.transform = hexKeyTucked
            }
            StripMotion.animate(StripMotion.shrink, fresh: fresh, {
                self.placeReveal(open: false, panel: panel)
            }, completion: { _ in finish() })
            StripMotion.animate(StripMotion.settleExit) { panel.setControls(shown: false) }
            StripMotion.animate(StripMotion.settle) {
                self.hexKey.alpha = 1
                self.hexKey.transform = .identity
            }
        }
    }

    /// The Hex key a little smaller about its keycap's centre, as it leaves and returns.
    private var hexKeyTucked: CGAffineTransform {
        let s = StripMotion.controlScale
        let cap = hexKey.keycapFrame
        let dx = cap.midX - hexKey.bounds.midX
        let dy = cap.midY - hexKey.bounds.midY
        return CGAffineTransform(translationX: dx, y: dy).scaledBy(x: s, y: s).translatedBy(x: -dx, y: -dy)
    }

    /// Masks the words and the dots on the two sides of one soft edge. True when they were
    /// unmasked, so the edge starts at home in the Hex key; false mid-flight, where it carries on.
    private func beginReveal(_ panel: HexKeyRecordingPanel) -> Bool {
        let dotsAfter = !engine.layout.hexOnLeading
        if let wordsMask, wordsMask.opaqueAfter == !dotsAfter, panel.dotsMask?.opaqueAfter == dotsAfter {
            return false
        }
        let words = StripRevealMask(opaqueAfter: !dotsAfter)
        let dots = StripRevealMask(opaqueAfter: dotsAfter)
        stripView.mask = words
        wordsMask = words
        panel.dotsMask = dots
        UIView.performWithoutAnimation { placeReveal(open: false, panel: panel) }
        return true
    }

    private func endReveal(_ panel: HexKeyRecordingPanel) {
        stripView.mask = nil
        wordsMask = nil
        panel.dotsMask = nil
    }

    /// Where the edge's band starts (it is `StripRevealMask.feather` wide): home, at the Hex key's
    /// side of the bar with every dot gathered in and every word shown, or across the bar.
    private func placeReveal(open: Bool, panel: HexKeyRecordingPanel) {
        let well = panel.convert(panel.wellFrame, to: self)
        let words = stripView.frame
        let feather = StripRevealMask.feather
        let band: CGFloat = if engine.layout.hexOnLeading {
            open ? max(well.maxX, words.maxX) : min(well.minX, words.minX) - feather
        } else {
            open ? min(well.minX, words.minX) - feather : max(well.maxX, words.maxX)
        }
        wordsMask?.place(bandStart: band - words.minX)
        panel.dotsMask?.place(bandStart: band - well.minX)
    }

    /// Where the recording panel sits: the suggestion strip, full width and its height, so
    /// neither the keyboard nor the keys move.
    private var recordingPanelFrame: CGRect {
        let m = engine.layout.metrics
        return CGRect(x: 0, y: m.stripTop + appliedLift, width: bounds.width, height: m.stripHeight)
    }

    private func fade(
        in shown: [UIView],
        out hidden: [UIView],
        duration requested: TimeInterval = 0.2,
        delay: TimeInterval = 0,
        after: (() -> Void)? = nil
    ) {
        shown.forEach { $0.isHidden = false }
        let globeHidden = engine.layout.key(.globe) == nil
        let accentHidden = engine.activeAccentMenu == nil
        let duration = window == nil ? 0 : requested
        if duration == 0 {
            shown.forEach { $0.alpha = 1 }
            hidden.forEach {
                $0.alpha = 0
                $0.isHidden = true
            }
            if globeHidden { globeButton.isHidden = true }
            if accentHidden { accentCallout.isHidden = true }
            after?()
            return
        }
        UIView.animate(springDuration: duration, bounce: 0, initialSpringVelocity: 0, delay: delay * MotionDebug.slow, options: [.beginFromCurrentState, .allowUserInteraction]) {
            shown.forEach { $0.alpha = 1 }
            hidden.forEach { $0.alpha = 0 }
        } completion: { _ in
            // A Hex key still held for push to talk stays in the window (at alpha 0), so its
            // touch keeps arriving and lifting the finger finishes the dictation.
            for view in hidden where view.alpha == 0 && (view as? UIControl)?.isTracking != true {
                view.isHidden = true
            }
            if globeHidden { self.globeButton.isHidden = true }
            if accentHidden { self.accentCallout.isHidden = true }
            after?()
        }
    }

    /// The snapshot stage's motion scenes press the Hex key the way a finger would before recording.
    func debugPressHexKey(_ down: Bool) { hexKey.isHighlighted = down }

    /// What the strip shows this frame, from the presentation layers, for `--motion-trace`.
    func debugMotionSample() -> [String: Double] {
        func alpha(_ view: UIView?) -> Double {
            guard let view, !view.isHidden else { return 0 }
            return Double((view.layer.presentation() ?? view.layer).opacity)
        }
        func frame(_ view: UIView) -> CGRect { (view.layer.presentation() ?? view.layer).frame }
        var s: [String: Double] = [
            "strip": alpha(stripView),
            "hex": alpha(hexKey),
            "hexX": frame(hexKey).midX,
            "hexScale": Double((hexKey.layer.presentation() ?? hexKey.layer).transform.m11),
            "edge": wordsMask.map { Double(($0.layer.presentation() ?? $0.layer).frame.minX + StripRevealMask.reach + stripView.frame.minX) } ?? -1,
            "panel": alpha(recordingPanel),
            "keys": alpha(plane),
        ]
        if let panel = recordingPanel {
            s.merge(panel.debugMotionSample()) { a, _ in a }
        }
        s.merge(stripView.debugMotionSample()) { a, _ in a }
        return s
    }

    private func makeRecordingPanel() -> HexKeyRecordingPanel {
        let panel = HexKeyRecordingPanel(loudness: { [weak model] in model?.liveLoudnessDB ?? -62 })
        panel.alpha = 0
        panel.onDone = { [weak self] in
            self?.typingLog?.ui("dictation_stop")
            self?.model.onStopTap?()
        }
        panel.onCancel = { [weak self] in
            self?.typingLog?.ui("dictation_cancel")
            self?.model.onCancelTap?()
        }
        panel.onCycleStyle = { [weak self] in
            guard let self else { return }
            self.typingLog?.ui("style", [
                TypingLogField("style", .string(self.model.style.next.rawValue)),
                TypingLogField("from", .string("recording_panel")),
            ])
            self.model.onStyleSelected?(self.model.style.next)
        }
        panel.hexOnLeading = engine.layout.hexOnLeading
        insertSubview(panel, belowSubview: callout)
        recordingPanel = panel
        setNeedsLayout()
        return panel
    }

    private func engineChanged() {
        // A new selection replaces any Dictate still waiting on its words.
        if engine.isSelectionActive { engine.clearPendingSelectionDictation() }
        plane.setNeedsDisplay()
        updateCallout()
        refreshStrip()
        if globeButton.isHidden != (engine.layout.key(.globe) == nil) || hexKey.frame != engine.layout.key(.hex)?.hitFrame {
            setNeedsLayout()
        }
    }

    private func updateCallout() {
        if let menu = engine.activeAccentMenu {
            endCalloutLinger()
            callout.show(nil, label: "")
            accentCallout.show(menu)
            return
        }
        accentCallout.show(nil)
        let layout = engine.layout
        if engine.showsKeyPopups, let key = engine.previewKey, let balloon = layout.balloon(for: key) {
            endCalloutLinger()
            let label = KeyboardKeyPlaneView.keycapLabel(engine.displayLabel(for: key))
            callout.show(balloon, label: label, style: KeyCalloutView.style(for: label))
            shownPreviewKind = key.kind
        } else if let kind = shownPreviewKind, calloutLingerWork == nil, engine.touches.isEmpty {
            // The finger lifted: Apple's pop-up and its grey key stay up 3 frames more (about
            // 50 ms after the letter lands, iOS 27.2 at 60 fps), then go in one frame.
            plane.lingeringKind = kind
            let work = DispatchWorkItem { [weak self] in self?.endCalloutLinger() }
            calloutLingerWork = work
            DispatchQueue.main.asyncAfter(deadline: .now() + KeyCalloutView.linger, execute: work)
        } else if calloutLingerWork == nil {
            shownPreviewKind = nil
            callout.show(nil, label: "")
        }
    }

    /// The key whose pop-up is up, or lingering after the finger lifted.
    private var shownPreviewKind: KeyKind?
    private var calloutLingerWork: DispatchWorkItem?

    private func endCalloutLinger() {
        calloutLingerWork?.cancel()
        calloutLingerWork = nil
        guard plane.lingeringKind != nil || shownPreviewKind != nil else { return }
        plane.lingeringKind = nil
        if engine.previewKey == nil {
            shownPreviewKind = nil
            callout.show(nil, label: "")
        }
    }

    // MARK: - Actions

    /// The system key click (`KeyboardInputView`), one for every key, delete and shift too:
    /// Apple's keyboard plays other sounds for those, but the click is the only keyboard sound
    /// a keyboard may play. Tests count it.
    var playInputClick: () -> Void = { UIDevice.current.playInputClick() }

    /// A key went down (or delete repeated): its click, and its tick while haptics are allowed.
    func keyFeedback() {
        playInputClick()
        if allowsHaptics() {
            keyTicks.impactOccurred(intensity: Self.keyTickIntensity(hapticStrength))
            keyTicks.prepare()
        }
    }

    func selectionFeedback() {
        if allowsHaptics() {
            selectionHaptics.selectionChanged()
            selectionHaptics.prepare()
        }
    }

    /// A tap or still hold on the Hex key, or a hold on the bar where no word is (`from`).
    func hexKeyHeld(from source: String = "hex_key") {
        // Any dictation the person starts themselves replaces a selection Dictate still
        // waiting on its words.
        if source != "selection_dictate" { engine.clearPendingSelectionDictation() }
        if allowsHaptics() { haptics.impactOccurred() }
        dismissStyleMenu()
        guard model.hasFullAccess else {
            // Dictation needs Full Access (the microphone and Hex's App Group): say so,
            // instead of a dictation that can't start.
            dictatingFromHexKey = false
            showFullAccessHint()
            return
        }
        if hexKey.isWarm {
            typingLog?.ui(source, [TypingLogField("dictation", .string("warm"))])
            dictatingFromHexKey = true
            model.onDictateTap?()
        } else if let local = model.onLocalDictateTap {
            typingLog?.ui(source, [TypingLogField("dictation", .string("keyboard"))])
            dictatingFromHexKey = true
            local()
        } else {
            typingLog?.ui(source, [TypingLogField("dictation", .string("open_hex"))])
            dictatingFromHexKey = false
            model.onOpenURL?(model.coldStartURL)
        }
    }

    /// Push to talk: lifting the finger after a hold finishes the dictation the hold started,
    /// as Done does. A hold that opened Hex has nothing to finish here.
    private func hexKeyReleased(from source: String = "hex_key_release") {
        guard dictatingFromHexKey else { return }
        dictatingFromHexKey = false
        typingLog?.ui("dictation_stop", [TypingLogField("from", .string(source))])
        model.onStopTap?()
    }

    // MARK: - Holding the bar

    /// Apple's long press: a still finger this long on the bar where no word is dictates.
    static let barHoldDuration: TimeInterval = 0.4

    /// Whether a finger at `point` may hold the bar to dictate. Only touches nothing else took
    /// reach here (an empty slot, beside the Hex key, a bar with no words); a word's slot,
    /// the Hex key and the keys take their own. Not while the bar shows the Full Access hint
    /// or a notice, or a dictation, the emoji panel or the style menu is up.
    private func takesBarHold(at point: CGPoint) -> Bool {
        let m = engine.layout.metrics
        let bar = CGRect(x: 0, y: appliedLift, width: bounds.width, height: m.stripTop + m.stripHeight)
        guard bar.contains(point), !showsRecording, emojiPanel == nil, styleMenu == nil,
              !(showsFullAccessHint && !model.hasFullAccess), vocabularyNotice == nil, translationNotice == nil, modelNotice == nil
        else { return false }
        return model.stagePhase == .idle || model.stagePhase == .failed
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard barHoldTouch == nil, let touch = touches.first, takesBarHold(at: touch.location(in: self)) else {
            super.touchesBegan(touches, with: event)
            return
        }
        barHoldTouch = touch
        barHold.touchDown(at: touch.location(in: self))
        let work = DispatchWorkItem { [weak self] in self?.barHoldElapsed() }
        barHoldWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.barHoldDuration, execute: work)
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = barHoldTouch, touches.contains(touch) else {
            super.touchesMoved(touches, with: event)
            return
        }
        // A slide before the hold is no hold (the bar has no menu); after it, moves don't matter.
        if barHold.touchMoved(to: touch.location(in: self)) == .openMenu { endBarHold() }
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = barHoldTouch, touches.contains(touch) else {
            super.touchesEnded(touches, with: event)
            return
        }
        // A tap does nothing; lifting after the hold finishes the dictation it started.
        let output = barHold.touchUp()
        endBarHold()
        if output == .release { hexKeyReleased(from: "bar_release") }
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
        guard let touch = barHoldTouch, touches.contains(touch) else {
            super.touchesCancelled(touches, with: event)
            return
        }
        // As the Hex key's: a hold that already fired keeps dictating (Done finishes it).
        endBarHold()
    }

    private func barHoldElapsed() {
        barHoldWork = nil
        guard barHoldTouch != nil, barHold.holdElapsed() == .hold else { return }
        guard let touch = barHoldTouch, takesBarHold(at: touch.location(in: self)) else {
            endBarHold()
            return
        }
        // The Hex key's own path: its press (in-keyboard dictation arms) then its hold.
        model.onHexKeyPress?(Date().timeIntervalSince1970)
        hexKeyHeld(from: "bar_hold")
    }

    private func endBarHold() {
        barHoldWork?.cancel()
        barHoldWork = nil
        barHold.cancel()
        barHoldTouch = nil
    }

    /// Hold and slide: light the row under the finger, and pick it on release.
    private func forwardMenuPoint(_ point: CGPoint, release: Bool) {
        guard let menu = styleMenu else { return }
        let local = hexKey.convert(point, to: menu)
        if release {
            menu.finishDrag(at: local)
        } else {
            menu.updateDrag(at: local)
        }
    }

    private func showStyleMenu() {
        guard styleMenu == nil, !showsRecording else { return }
        if allowsHaptics() {
            haptics.impactOccurred()
            haptics.prepare()
        }
        plane.cancelAllTouches()
        let menu = HexStyleMenuView()
        menu.palette = palette
        menu.style = model.style
        applyLanguages(to: menu)
        menu.onHoverChanged = { [weak self] in self?.selectionFeedback() }
        menu.onLanguage = { [weak self] language in
            self?.typingLog?.ui("dictation_language", [TypingLogField("language", .string(language ?? "automatic"))])
            self?.model.onLanguageSelected?(language)
            self?.dismissStyleMenu()
        }
        // The menu stays open, so the language is seen to go.
        menu.onRemoveLanguage = { [weak self] language in
            self?.typingLog?.ui("dictation_language_removed", [TypingLogField("language", .string(language))])
            self?.model.onLanguageRemoved?(language)
        }
        menu.onTranslationToggle = { [weak self] on in
            self?.typingLog?.ui("translate", [TypingLogField("on", .bool(on))])
            self?.model.onTranslationToggled?(on)
            self?.dismissStyleMenu()
        }
        menu.onTranslationTarget = { [weak self] target in
            self?.typingLog?.ui("translate", [TypingLogField("target", .string(target))])
            self?.model.onTranslationTargetSelected?(target)
            self?.dismissStyleMenu()
        }
        menu.onStyle = { [weak self] style in
            self?.typingLog?.ui("style", [
                TypingLogField("style", .string(style.rawValue)),
                TypingLogField("from", .string("hex_menu")),
            ])
            self?.model.onStyleSelected?(style)
            self?.dismissStyleMenu()
        }
        menu.onDismiss = { [weak self] in self?.dismissStyleMenu() }
        addSubview(menu)
        styleMenu = menu
        setNeedsLayout()
        layoutIfNeeded()
    }

    func dismissStyleMenu() {
        styleMenu?.removeFromSuperview()
        styleMenu = nil
    }

    /// Fixtures: open the menu without a long press, optionally on another page.
    func debugShowStyleMenu(page: HexStyleMenuView.Page = .styles) {
        showStyleMenu()
        styleMenu?.show(page: page)
    }

    /// Fixtures: the language page with Remove on a language's row.
    func debugOfferRemove(_ language: String) {
        showStyleMenu()
        styleMenu?.debugOfferRemove(language)
    }

    /// Where the dictation is from the cursor, or nil when it can't be told apart from other
    /// text: right before the cursor (perhaps only its end shown), or found once around it.
    private func undoPlan(_ undo: DictationUndo) -> DictationUndo.Plan? {
        let target = engine.target
        let selected = target.selectedText
        guard selected?.isEmpty ?? true else { return nil }
        let text = undo.text
        let before = target.documentContextBeforeInput ?? ""
        let after = target.documentContextAfterInput ?? ""
        // Right before the cursor, with what followed it when it went in still after the cursor.
        if after.hasPrefix(undo.after) || undo.after.hasPrefix(after),
           before.hasSuffix(text) || (!before.isEmpty && before.count < text.count && text.hasSuffix(before)) {
            return DictationUndo.Plan(toEnd: 0, delete: text.count, back: 0)
        }
        return undo.plan(before: before, after: after, selected: selected)
    }

    private func undoDictation() {
        engine.commitHangulComposition()
        guard let undo = lastDictation else { return }
        let text = undo.text
        let before = engine.target.documentContextBeforeInput ?? ""
        if let plan = undoPlan(undo) {
            // The host may show only the end of it; replay removes what the keys remove.
            typingLog?.edit("undo_dictation", before.hasSuffix(text) || plan.toEnd != 0
                ? [TypingLogField("deleted", .string(text))]
                : [TypingLogField("deletedCount", .int(text.count))])
            // Moves stay inside the text the host showed, so it doesn't drop them.
            if plan.toEnd != 0 { engine.target.adjustTextPosition(byCharacterOffset: plan.toEnd) }
            for _ in 0..<plan.delete {
                engine.target.deleteBackward()
            }
            if plan.back != 0 { engine.target.adjustTextPosition(byCharacterOffset: plan.back) }
            onUndoDictation?()
        } else {
            typingLog?.ui("undo_dictation", [TypingLogField("applied", .bool(false))])
        }
        hideUndo()
        engine.forgetTextAroundKeys()
        engine.textDidChange()
        typingLog?.context("undo_dictation")
        refreshReturnKey()
    }

    func hideUndo() {
        undoHideWork?.cancel()
        undoHideWork = nil
        if lastDictation != nil {
            lastDictation = nil
            refreshStrip()
        }
    }

    /// The globe's touches go to UIKit's keyboard list (`InputModeListTouch`).
    private func configureGlobe() {
        globeButton.removeTarget(nil, action: nil, for: .allEvents)
        guard hostController != nil else { return }
        globeButton.addTarget(self, action: #selector(globeTouch(_:with:)), for: .allTouchEvents)
    }

    @objc private func globeTouch(_ sender: UIButton, with event: UIEvent) {
        guard let controller = hostController else { return }
        inputModeListTouch.forward(sender, with: event, to: controller)
    }
}
