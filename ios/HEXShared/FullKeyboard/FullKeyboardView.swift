import Combine
import UIKit

/// The experimental full keyboard: a 3-slot suggestion strip, QWERTY keys with the Hex key
/// right of space, long-press accents, and the recording panel that replaces both while Hex records.
/// Dictation state comes from the same `KeyboardViewModel` the dictation bar uses.
@MainActor
final class FullKeyboardView: UIView, UIInputViewAudioFeedback {
    let model: KeyboardViewModel
    let engine: FullKeyboardEngine
    weak var hostController: UIInputViewController? {
        didSet { configureGlobe() }
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
    /// The keys' touch surface; tests feed it touch samples.
    let plane: KeyboardKeyPlaneView
    let callout = KeyCalloutView()
    let accentCallout = AccentCalloutView()
    let hexKey = HexKeyControl()
    let globeButton = UIButton(type: .custom)
    /// The capsule that grows from the Hex mark across the strip into the recording bar.
    private let morph = UIView()
    private var recordingPanel: HexKeyRecordingPanel?
    private var styleMenu: HexStyleMenuView?
    private(set) var palette = FullKeyboardPalette(isDark: false)
    private var didApplyPalette = false
    private var subscriptions: Set<AnyCancellable> = []
    private(set) var showsRecording = false
    /// The Hex key's last tap or hold started a dictation here (not by opening Hex).
    private var dictatingFromHexKey = false
    private var lastDictation: String?
    private var undoHideWork: DispatchWorkItem?
    /// The Hex key was tapped without Full Access: the strip says what dictation needs.
    private(set) var showsFullAccessHint = false
    private var fullAccessHintHideWork: DispatchWorkItem?
    /// How long the Full Access hint stays unless a key or the strip is used first.
    static let fullAccessHintDuration: TimeInterval = 8
    private let haptics = UIImpactFeedbackGenerator(style: .light)
    private let selectionHaptics = UISelectionFeedbackGenerator()
    private let clipboard = KeyboardClipboardTracker()

    // MARK: Emoji (Experiments › Emoji; FullKeyboardView+Emoji.swift)

    /// The keys' target: the field, or the emoji search while you search.
    let routedTarget: EmojiRoutedTarget
    /// Fixtures turn the emoji experiment on or off; otherwise the App Group's switch.
    var emojiOverride: Bool? { didSet { refreshEmojiKey() } }
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

        addSubview(stripView)
        morph.isUserInteractionEnabled = false
        morph.isHidden = true
        addSubview(morph)
        addSubview(plane)
        addSubview(hexKey)
        addSubview(globeButton)
        addSubview(callout)
        addSubview(accentCallout)
        accentCallout.isHidden = true

        stripView.onSelectSuggestion = { [weak self] suggestion in
            guard let self else { return }
            self.keyFeedback()
            if suggestion.kind == .emoji {
                self.recordEmojiUse(suggestion.replacement)
            }
            self.engine.acceptSuggestion(suggestion)
        }
        stripView.onUndoDictation = { [weak self] in self?.undoDictation() }
        stripView.onPaste = { [weak self] in self?.pasteFromClipboard() }
        stripView.onOpenHexFromHint = { [weak self] in self?.openSetupFromHint() }
        stripView.onNextKeyboard = { [weak self] in
            self?.typingLog?.ui("next_keyboard")
            self?.hostController?.advanceToNextInputMode()
        }

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
            self.hideFullAccessHint()
            self.refreshReturnKey()
            self.refreshStrip()
            self.onTextEdit?()
        }
        engine.onReturn = { [weak self] in
            guard self?.routedTarget.isSearching == false else { return }
            self?.onReturnKey?()
        }
        engine.onEmojiKey = { [weak self] in self?.emojiKeyTapped() }
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
        engine.autoCapitalizes = FullKeyboardSettings().autoCapitalization
        engine.showsKeyPopups = FullKeyboardSettings().keyPopups
        refreshEmojiKey()
        syncSpatialAutocorrect()
        refreshLanguageSignals()
        syncLanguage()
        syncSwipeTyping()
        engine.configureForField()
        applyAppearance()
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
        orientationOverride ?? Self.orientation(for: traitCollection)
    }

    var preferredHeight: CGFloat {
        KeyboardLayout.height(for: orientation, posture: engine.posture) + lift
    }

    /// Room above the strip for the emoji search results. The strip itself keeps its height in
    /// every state, the recording bar's included.
    private var lift: CGFloat { emojiSearchLift }

    /// Asks the host to apply `preferredHeight` now (the keyboard extension resizes itself).
    var onPreferredHeightChange: (() -> Void)?

    /// How much of the lift the view actually has yet, so nothing jumps before the host grows it.
    private var appliedLift: CGFloat {
        max(0, min(lift, bounds.height - KeyboardLayout.height(for: orientation, posture: engine.posture)))
    }

    /// Reads which device and posture the keyboard is on, and how far iOS set it in from the
    /// screen's sides, from its window. The Duo's unfolded screen has regular size classes either
    /// way up, so the size classes alone can't tell its postures apart.
    private func syncPosture() {
        guard let screen = window?.windowScene?.screen ?? window?.screen else { return }
        let size = screen.bounds.size
        let posture = KeyboardPosture.current(screenSize: size, isFoldable: Self.isFoldable)
        let inset = posture == .phone ? 0 : max(0, (size.width - bounds.width) / 2)
        let heightChanges = posture != engine.posture
        engine.posture = posture
        engine.windowInset = inset
        if heightChanges { onPreferredHeightChange?() }
    }

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

    var enableInputClicksWhenVisible: Bool { true }

    func warmUpTextCheckerAsync() {
        DispatchQueue.main.async { [weak self] in
            guard let self, let checker = self.engine.autocorrect.checker as? SystemTextChecker else { return }
            checker.warmUp()
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

    /// Where Languages and the language last switched to are kept; tests give their own.
    var languageSettings = LanguageSettings() {
        didSet { syncLanguage() }
    }

    /// The languages the space bar switches between and the one typing now.
    private(set) var languages = KeyboardLanguageList(languages: [.english], current: .english)
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
        guard list != languages || engine.language != list.current else { return }
        languages = list
        engine.switchesLanguage = list.switches
        engine.language = list.current
        engine.partnerLanguage = list.partner(available: Self.spellCheckLanguages)
        syncSpatialAutocorrect()
        refreshSpaceLabel()
        syncSwipeTyping()
        setNeedsLayout()
    }

    /// A swipe along the space bar: the next or previous language (its letters, accents and
    /// autocorrect), kept for next time, and with Switch dictation too the dictation
    /// language with it.
    func switchLanguage(by step: Int) {
        guard languages.switches, languageOverride == nil else { return }
        let next = languages.switched(by: step)
        typingLog?.ui("keyboard_language", [TypingLogField("language", .string(next.rawValue))])
        switchedLanguage = next
        languageSettings.currentKeyboardLanguage = next
        selectionFeedback()
        syncLanguage()
        guard languageSettings.dictationFollowsKeyboard, model.hasFullAccess,
              let dictation = next.dictationLanguage(
                  supported: languageSettings.supportedDictationLanguages,
                  preferred: preferredLanguages
              ),
              languageSettings.dictationLanguage != dictation else { return }
        model.onLanguageSelected?(dictation)
    }

    /// With two or more languages the space bar names the keyboard's; otherwise, with Full
    /// Access, the dictation language's.
    private func refreshSpaceLabel() {
        plane.spaceLabel = languages.spaceLabel ?? (model.hasFullAccess ? model.languageSummary : nil)
    }

    func syncSettings() {
        syncLanguage()
        let journal = TypingJournal.sharedIfEnabled()
        if engine.journal !== journal {
            engine.journal = journal
        }
        engine.showsKeyPopups = FullKeyboardSettings().keyPopups
        engine.autoCapitalizes = FullKeyboardSettings().autoCapitalization
        refreshEmojiKey()
        let wantsAutocorrect = FullKeyboardSettings().autocorrect
        if engine.autocorrect.isEnabled != wantsAutocorrect {
            engine.autocorrect.isEnabled = wantsAutocorrect
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

    /// Fixtures and tests turn Swipe typing on or off; otherwise its switch in Experiments.
    var swipeOverride: Bool? { didSet { syncSwipeTyping() } }

    /// Experiments › Swipe typing (off by default): on the English and Spanish layouts, a
    /// drag across the letters types a word, read with the layout language's bundled model.
    /// Off under VoiceOver, which has its own way to type.
    private func syncSwipeTyping() {
        let on = (swipeOverride ?? FullKeyboardSettings().swipeTyping) && engine.language.supportsSwipeTyping
            && !UIAccessibility.isVoiceOverRunning
        let model = on ? TypingDecoderModel.bundled(engine.language.rawValue) : nil
        if plane.swipe?.model !== model {
            plane.swipe = model.map { SwipeTyping(engine: engine, model: $0) }
        } else {
            plane.swipe?.syncWords()
        }
    }

    /// Settings › Keyboard › Spatial autocorrect (on by default): maps the layout language's
    /// bundled model when the keyboard opens (English and Spanish today; others keep `UITextChecker`),
    /// and moves the keys' centres to where Learn how I type saw you tap. The same word list
    /// lets the likely next letter claim a few points of its neighbour's touch target whether
    /// or not the experiment is on (`FullKeyboardEngine.letterOdds`; only while autocorrecting).
    private func syncSpatialAutocorrect() {
        let words = TypingDecoderModel.bundled(engine.language.rawValue)
        if engine.letterOdds !== words {
            engine.letterOdds = words
        }
        let model = FullKeyboardSettings().spatialAutocorrect ? words : nil
        if engine.autocorrect.decoder?.model !== model {
            engine.autocorrect.decoder = model.map { TypingDecoder(model: $0) }
            engine.refreshSuggestions()
        }
        // Bilingual autocorrect reads the same taps with the other language's model, where it has one.
        if let partner = engine.autocorrect.partner {
            let theirs = FullKeyboardSettings().spatialAutocorrect ? TypingDecoderModel.bundled(partner.language.rawValue) : nil
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
        if routeField != field {
            plane.cancelAllTouches()
            engine.configureForField()
            refreshEmojiKey()
        } else {
            engine.textDidChange()
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
            haptics.prepare()
            selectionHaptics.prepare()
        }
    }

    /// Frees non-essential UI and Metal drawables when the extension approaches its memory ceiling.
    func handleMemoryWarning() {
        dismissStyleMenu()
        if emojiPanel == nil {
            emojiLibrary = nil
        }
        if !showsRecording, let panel = recordingPanel {
            panel.tearDown()
            panel.removeFromSuperview()
            recordingPanel = nil
        }
    }

    /// Hex inserted a dictation; offer to take it back until the next edit.
    func didInsertDictation(_ text: String) {
        lastDictation = text
        refreshStrip()
        undoHideWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.hideUndo() }
        undoHideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 12, execute: work)
        engine.textDidChange()
        refreshReturnKey()
    }

    /// Fixtures: show a key held down.
    func debugPress(_ character: String) {
        guard let key = engine.layout.key(.character(character)) else { return }
        engine.touchBegan(id: -1, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
    }

    /// Fixtures: show a key's accent callout with `option` selected.
    func debugHoldAccent(_ character: String, selecting option: String) {
        debugHoldAccent(character)
        guard let menu = engine.activeAccentMenu,
              let index = menu.options.firstIndex(of: option),
              let key = engine.layout.key(.character(character)) else { return }
        let cell = menu.cellFrames[index]
        engine.touchMoved(id: -2, to: CGPoint(x: cell.midX, y: key.frame.midY))
    }

    /// Fixtures: show a key's long-press accent callout.
    func debugHoldAccent(_ character: String, selectIndex: Int? = nil) {
        guard let key = engine.layout.key(.character(character)) else { return }
        let center = CGPoint(x: key.frame.midX, y: key.frame.midY)
        engine.touchBegan(id: -2, at: center)
        _ = engine.openAccentMenuIfEligible(forTouch: -2)
        if let selectIndex,
           let menu = engine.activeAccentMenu,
           selectIndex >= 0, selectIndex < menu.cellFrames.count {
            let cell = menu.cellFrames[selectIndex]
            engine.touchMoved(id: -2, to: CGPoint(x: cell.midX, y: center.y))
        }
    }

    // MARK: - Layout

    override func layoutSubviews() {
        super.layoutSubviews()
        let isLandscape = orientation == .landscape
        engine.typingLog?.viewGeometry(size: bounds.size, safeArea: safeAreaInsets)
        engine.orientation = orientation
        engine.horizontalInset = isLandscape ? max(safeAreaInsets.left, safeAreaInsets.right) : 0
        engine.width = bounds.width
        syncPosture()
        engine.typingLog?.commitGeometry()
        let layout = engine.layout

        let lift = appliedLift
        let content = CGRect(x: 0, y: lift, width: bounds.width, height: bounds.height - lift)
        plane.frame = content
        callout.frame = content
        accentCallout.frame = content
        stripView.frame = layout.stripFrame.offsetBy(dx: 0, dy: lift)

        hexKey.cornerRadius = layout.metrics.cornerRadius
        if let hex = layout.key(.hex) {
            hexKey.inStrip = hex.row < 0
            hexKey.frame = hex.hitFrame.offsetBy(dx: 0, dy: lift)
            hexKey.keycapFrame = hex.frame.offsetBy(dx: -hex.hitFrame.minX, dy: -hex.hitFrame.minY)
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
            panel.sideKeyWidth = layout.letterWidth * KeyboardLayout.modifierWidthRatio
        }
        if let menu = styleMenu, let hex = layout.key(.hex) {
            menu.frame = bounds
            menu.anchor = hex.frame
        }
        plane.setNeedsDisplay()
        updateCallout()
    }

    // MARK: - State

    private func applyAppearance() {
        let isDark = appearanceOverride
            ?? FullKeyboardPalette.isDark(traits: traitCollection)
        let next = FullKeyboardPalette(isDark: isDark)
        backgroundColor = drawsBackdrop ? next.backdrop : .clear
        stripView.inSystemPlatter = !drawsBackdrop
        guard next != palette || !didApplyPalette else { return }
        didApplyPalette = true
        palette = next
        stripView.palette = next
        plane.palette = next
        callout.palette = next
        callout.setNeedsDisplay()
        accentCallout.palette = next
        hexKey.palette = next
        recordingPanel?.palette = next
        styleMenu?.palette = next
        emojiPanel?.palette = next
        emojiSearchField?.palette = next
        emojiResults?.palette = next
    }

    func refreshReturnKey() {
        let type = engine.target.returnKeyType ?? .default
        plane.returnLabel = switch type {
        case .go: "go"
        case .google, .yahoo, .search: "search"
        case .join: "join"
        case .next: "next"
        case .route: "route"
        case .send: "send"
        case .done: "done"
        case .emergencyCall: "SOS"
        case .continue: "continue"
        default: "return"
        }
        plane.returnSymbol = switch type {
        case .send: "arrow.up"
        case .go, .continue, .route, .join: "arrow.right"
        case .google, .yahoo, .search: "magnifyingglass"
        case .done: "checkmark"
        case .emergencyCall: nil
        default: "return"
        }
        plane.returnIsAction = switch type {
        case .go, .google, .yahoo, .search, .join, .route, .send, .done, .continue, .emergencyCall: true
        default: false
        }
        plane.returnIsEnabled = !(engine.target.enablesReturnKeyAutomatically == true && !engine.target.hasText)
    }

    private func refreshFromModel() {
        hexKey.isWarm = model.hasFullAccess && model.isWarmAvailable
        if engine.showsGlobe != model.needsGlobeKey {
            engine.showsGlobe = model.needsGlobeKey
            setNeedsLayout()
        }
        let recording = model.stagePhase == .recording || model.stagePhase == .transcribing
        if recording != showsRecording {
            setRecording(recording)
        }
        refreshRecordingPanel()
        stripView.showsLoggingMark = model.isLoggingTyping
        recordingPanel?.showsLoggingMark = model.isLoggingTyping
        // A language picked in the switch changes the layout at once when it follows dictation.
        syncLanguage()
        refreshSpaceLabel()
        if let menu = styleMenu {
            menu.style = model.style
            applyLanguages(to: menu)
        }
        refreshStrip()
    }

    private func applyLanguages(to menu: HexStyleMenuView) {
        menu.dictationLanguage = model.dictationLanguage
        menu.automaticLanguage = model.automaticLanguage
        menu.languageChoices = model.showsLanguageSwitch ? model.languageChoices : []
        menu.pasteChoice = model.pasteChoice
        menu.pastesTranslation = model.pastesTranslation
    }

    func refreshStrip() {
        // Without Full Access the strip suggests and corrects like any keyboard's; what
        // dictation needs shows only once the Hex key is tapped (`showFullAccessHint`).
        let status: String? = if model.hasFullAccess, model.stagePhase == .failed {
            model.statusMessage
        } else {
            nil
        }
        let clipboardOffer = clipboard.offer(
            textContentType: engine.target.textContentType,
            hasFullAccess: model.hasFullAccess
        )
        var suggestions = engine.suggestions
        if suggestions.isEmpty, lastDictation == nil, status == nil, clipboardOffer == nil {
            suggestions = emojiForFinishedWord()
        }
        stripView.update(
            suggestions: suggestions,
            showsUndo: lastDictation != nil,
            statusText: status,
            clipboard: clipboardOffer,
            fullAccessHint: showsFullAccessHint && !model.hasFullAccess
        )
    }

    // MARK: - Without Full Access

    /// The Hex key without Full Access: the keys keep typing, and the strip says, for a few
    /// seconds, that dictation needs Allow Full Access and where to turn it on. Only its
    /// Open Hex chip opens Hex; nothing opens it by itself.
    func showFullAccessHint() {
        dismissStyleMenu()
        showsFullAccessHint = true
        refreshStrip()
        UIAccessibility.post(
            notification: .announcement,
            argument: "\(FullAccessHintView.title). Settings, Apps, Hex, Keyboards."
        )
        fullAccessHintHideWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.hideFullAccessHint() }
        fullAccessHintHideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.fullAccessHintDuration, execute: work)
    }

    private func hideFullAccessHint() {
        fullAccessHintHideWork?.cancel()
        fullAccessHintHideWork = nil
        guard showsFullAccessHint else { return }
        showsFullAccessHint = false
        refreshStrip()
    }

    private func openSetupFromHint() {
        keyFeedback()
        hideFullAccessHint()
        model.onOpenURL?(KeyboardLaunchRequest.setup.url)
    }

    private func pasteFromClipboard() {
        let pasteboard = UIPasteboard.general
        guard let text = pasteboard.string ?? pasteboard.url?.absoluteString, !text.isEmpty else { return }
        keyFeedback()
        clipboard.markPasted()
        typingLog?.edit("paste", [TypingLogField("inserted", .string(text))])
        engine.target.insertText(text)
        engine.textDidChange()
        typingLog?.context("paste")
        refreshStrip()
        onTextEdit?()
    }

    private func refreshRecordingPanel() {
        guard let panel = recordingPanel, showsRecording else { return }
        panel.update(
            phase: model.stagePhase == .transcribing ? .processing : .recording,
            style: model.style
        )
    }

    private func setRecording(_ recording: Bool) {
        showsRecording = recording
        dismissStyleMenu()
        if recording { closeEmojiPanel() }
        let closingFrames = recording ? nil : morphFrames()
        if recording {
            plane.cancelAllTouches()
            hideUndo()
            let panel = recordingPanel ?? makeRecordingPanel()
            panel.palette = palette
            panel.frame = recordingPanelFrame
            refreshRecordingPanel()
            panel.isHidden = false
            if let (mark, bar) = morphFrames(), window != nil {
                // The mark grows into a capsule and runs across the strip, then the panel takes over.
                runMorph(from: mark, to: bar, appearing: true)
                fade(in: [], out: [stripView, hexKey])
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.16) { [weak self, weak panel] in
                    guard let self, self.showsRecording, let panel else { return }
                    self.fade(in: [panel], out: [self.morph])
                }
            } else {
                fade(in: [panel], out: [stripView, hexKey])
            }
        } else if let panel = recordingPanel {
            panel.scope.phase = .hidden
            if let (_, bar) = closingFrames, let (mark, _) = morphFrames(), window != nil {
                runMorph(from: bar, to: mark, appearing: false)
            }
            fade(in: [stripView, hexKey], out: [panel]) { [weak self, weak panel] in
                guard let self, !self.showsRecording, let panel, panel === self.recordingPanel else { return }
                panel.tearDown()
                panel.removeFromSuperview()
                self.recordingPanel = nil
            }
        }
    }

    /// Where the recording panel sits: the suggestion strip, full width and its height, so
    /// neither the keyboard nor the keys move.
    private var recordingPanelFrame: CGRect {
        let m = engine.layout.metrics
        return CGRect(x: 0, y: m.stripTop + appliedLift, width: bounds.width, height: m.stripHeight)
    }

    /// The Hex mark's frame in the strip and the full-width bar it grows into, or nil when
    /// the Hex key sits in the bottom row.
    private func morphFrames() -> (CGRect, CGRect)? {
        let layout = engine.layout
        guard let hex = layout.key(.hex), hex.row < 0 else { return nil }
        let m = layout.metrics
        let lift = appliedLift
        let mark = hex.frame.offsetBy(dx: 0, dy: lift)
        let bar = CGRect(
            x: layout.horizontalInset + m.side,
            y: m.stripTop + lift + 4,
            width: bounds.width - 2 * (layout.horizontalInset + m.side),
            height: m.stripHeight - 8
        )
        return (mark, bar)
    }

    private func runMorph(from start: CGRect, to end: CGRect, appearing: Bool) {
        morph.layer.removeAllAnimations()
        // Under the recording panel on the way in (the panel fades in over it), above it on the
        // way out so the bar is seen shrinking back into the mark.
        if appearing {
            insertSubview(morph, aboveSubview: stripView)
        } else {
            bringSubviewToFront(morph)
        }
        morph.backgroundColor = palette.chipFill
        morph.frame = start
        morph.layer.cornerRadius = start.height / 2
        morph.alpha = appearing ? 0.6 : 1
        morph.isHidden = false
        UIView.animate(
            withDuration: 0.34,
            delay: 0,
            usingSpringWithDamping: 0.82,
            initialSpringVelocity: 0,
            options: [.beginFromCurrentState, .allowUserInteraction]
        ) {
            self.morph.frame = end
            self.morph.layer.cornerRadius = end.height / 2
            if appearing { self.morph.alpha = 1 }
        }
        guard !appearing else { return }
        UIView.animate(withDuration: 0.12, delay: 0.24, options: [.allowUserInteraction]) {
            self.morph.alpha = 0
        } completion: { _ in
            if !self.showsRecording {
                self.morph.isHidden = true
            }
        }
    }

    private func fade(in shown: [UIView], out hidden: [UIView], after: (() -> Void)? = nil) {
        shown.forEach { $0.isHidden = false }
        let globeHidden = engine.layout.key(.globe) == nil
        let accentHidden = engine.activeAccentMenu == nil
        let duration = window == nil ? 0 : 0.18
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
        UIView.animate(withDuration: duration, delay: 0, options: [.beginFromCurrentState, .allowUserInteraction]) {
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
        panel.showsLoggingMark = model.isLoggingTyping
        insertSubview(panel, belowSubview: callout)
        recordingPanel = panel
        setNeedsLayout()
        return panel
    }

    private func engineChanged() {
        plane.setNeedsDisplay()
        updateCallout()
        refreshStrip()
        if globeButton.isHidden != (engine.layout.key(.globe) == nil) || hexKey.frame != engine.layout.key(.hex)?.hitFrame {
            setNeedsLayout()
        }
    }

    private func updateCallout() {
        if let menu = engine.activeAccentMenu {
            callout.show(nil, label: "")
            accentCallout.show(menu)
            return
        }
        accentCallout.show(nil)
        let layout = engine.layout
        if engine.showsKeyPopups, let key = engine.previewKey, let balloon = layout.balloon(for: key) {
            callout.show(balloon, label: engine.displayLabel(for: key))
        } else {
            callout.show(nil, label: "")
        }
    }

    // MARK: - Actions

    func keyFeedback() {
        UIDevice.current.playInputClick()
        if allowsHaptics() {
            haptics.impactOccurred(intensity: 0.65)
            haptics.prepare()
        }
    }

    func selectionFeedback() {
        if allowsHaptics() {
            selectionHaptics.selectionChanged()
            selectionHaptics.prepare()
        }
    }

    /// A tap or still hold on the Hex key.
    private func hexKeyHeld() {
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
            typingLog?.ui("hex_key", [TypingLogField("dictation", .string("warm"))])
            dictatingFromHexKey = true
            model.onDictateTap?()
        } else if let local = model.onLocalDictateTap {
            typingLog?.ui("hex_key", [TypingLogField("dictation", .string("keyboard"))])
            dictatingFromHexKey = true
            local()
        } else {
            typingLog?.ui("hex_key", [TypingLogField("dictation", .string("open_hex"))])
            dictatingFromHexKey = false
            model.onOpenURL?(model.coldStartURL)
        }
    }

    /// Push to talk: lifting the finger after a hold finishes the dictation the hold started,
    /// as Done does. A hold that opened Hex has nothing to finish here.
    private func hexKeyReleased() {
        guard dictatingFromHexKey else { return }
        dictatingFromHexKey = false
        typingLog?.ui("dictation_stop", [TypingLogField("from", .string("hex_key_release"))])
        model.onStopTap?()
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
        menu.onPasteTranslation = { [weak self] translated in
            self?.typingLog?.ui("paste_translation", [TypingLogField("translated", .bool(translated))])
            self?.model.onPasteTranslationSelected?(translated)
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

    private func undoDictation() {
        guard let text = lastDictation else { return }
        let before = engine.target.documentContextBeforeInput ?? ""
        // Only take back text that is still right before the cursor.
        if before.hasSuffix(text) || (!before.isEmpty && before.count < text.count && text.hasSuffix(before)) {
            // The host may show only the end of it; replay removes what the keys remove.
            typingLog?.edit("undo_dictation", before.hasSuffix(text)
                ? [TypingLogField("deleted", .string(text))]
                : [TypingLogField("deletedCount", .int(text.count))])
            for _ in 0..<text.count {
                engine.target.deleteBackward()
            }
            onUndoDictation?()
        } else {
            typingLog?.ui("undo_dictation", [TypingLogField("applied", .bool(false))])
        }
        hideUndo()
        engine.textDidChange()
        typingLog?.context("undo_dictation")
        refreshReturnKey()
    }

    private func hideUndo() {
        undoHideWork?.cancel()
        undoHideWork = nil
        if lastDictation != nil {
            lastDictation = nil
            refreshStrip()
        }
    }

    private func configureGlobe() {
        globeButton.removeTarget(nil, action: nil, for: .allEvents)
        if let controller = hostController {
            globeButton.addTarget(
                controller,
                action: #selector(UIInputViewController.handleInputModeList(from:with:)),
                for: .allTouchEvents
            )
        }
    }
}