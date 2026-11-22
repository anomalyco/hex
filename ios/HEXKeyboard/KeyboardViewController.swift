import os
import SwiftUI
import UIKit

@MainActor
final class KeyboardViewController: UIInputViewController {
    private let bridge = KeyboardBridge()
    private let viewModel = KeyboardViewModel()
    private let localDictation = KeyboardLocalDictation()
    private var pollTimer: Timer?
    private var coldStartJobID = UUID().uuidString
    private var lastURLRefreshBucket: Int = 0
    private var lastFullAccessReport: KeyboardFullAccessReport?
    /// When this keyboard process started (its first load): its Full Access reports' generation.
    private static let processLaunchedAt = Date().timeIntervalSince1970
    private var surface: UIView?
    private var fullKeyboard: FullKeyboardView?
    /// The dictation bar's emoji panel, at the full keyboard's height while it's open, and
    /// the index it reads (kept while the keyboard runs, as the full keyboard's).
    private var barEmojiPanel: EmojiPanelView?
    private var barEmojiLibrary: EmojiLibrary?
    private var heightConstraint: NSLayoutConstraint?
    /// The dictation bar's top edge, `barTopInset` below the keyboard's.
    private var barTopConstraint: NSLayoutConstraint?
    /// A thin material under the surface, mounted only when iOS kept an earlier Hex keyboard on
    /// screen in this field (`KeyboardsOnScreen`): then Apple's keys may be under this one, and
    /// through the material they have no shapes. Otherwise Hex allocates no blur view and sits
    /// clear on iOS's glass panel.
    private var backdrop: KeyboardBackdropView?
    private var didConfigureView = false
    /// Hex's keyboards on screen in this process, and this one's name among them.
    private static var onScreen = KeyboardsOnScreen() {
        didSet {
            guard onScreen != oldValue else { return }
            NotificationCenter.default.post(name: onScreenDidChange, object: nil)
        }
    }
    private static let onScreenDidChange = Notification.Name("anomaly.hex.mobile.keyboard.onScreenDidChange")
    private final class WeakInputView {
        weak var view: KeyboardInputView?
        init(_ view: KeyboardInputView) { self.view = view }
    }
    private static var inputViews: [Int: WeakInputView] = [:]
    private static var lastKeyboardID = 0
    private let keyboardID: Int = {
        KeyboardViewController.lastKeyboardID += 1
        return KeyboardViewController.lastKeyboardID
    }()
    private var pollInterval: TimeInterval = 0
    private var didHandOffCurrentField = false
    /// A number pad's hand-off waits for the Duo's posture (`handOffNumericPadIfNeeded`).
    private var handOffWaitsForPosture = false
    private var didWarmLexicon = false
    /// Learn how I type: what becomes of each dictation Hex inserts.
    private let dictationEdits = DictationEditTracker()
    private lazy var documentTarget = DocumentProxyTextTarget { [weak self] in self?.textDocumentProxy }
    /// Record typing logs: the process's logger while the experiment is on.
    private var typingLog: TypingLogger?
    /// The field the current typing log session is for.
    private var typingLogTraits: TypingLogTraits?
    /// Follows dictations for typing logs' outcomes, apart from Learn how I type's tracker.
    private let logDictationEdits = DictationEditTracker()
    private var pendingHostContext: DispatchWorkItem?
    /// Hides the translation note (`showTranslationNote`).
    private var translationNoteWork: DispatchWorkItem?
    /// How long the note that a dictation went in as spoken stays up.
    static let translationNoteDuration: TimeInterval = 8
    private var isShown = false
    /// Retired while a newer Hex keyboard sits over this one in the same field (`KeyboardsOnScreen`):
    /// keeps listening and restores its surface if the newer one goes away.
    private var isRetired = false
    /// This appearance hasn't yet checked Settings › Keyboard › Auto-start, which waits until
    /// its keys are up (and UIKit has named the field).
    private var autoStartPending = false
    private let switchGuess = KeyboardSwitchGuessStore()
    /// Notes when the field's app goes to the background, for `KeyboardSwitchGuess`; one per process.
    private static let hostLeaveObserver: NSObjectProtocol = NotificationCenter.default.addObserver(
        forName: .NSExtensionHostDidEnterBackground,
        object: nil,
        queue: .main
    ) { _ in
        KeyboardSwitchGuessStore().hostLeft(at: Date().timeIntervalSince1970)
    }
    /// The dictation bar's key haptic; the full keyboard has its own generator.
    private lazy var barHaptics = UIImpactFeedbackGenerator(style: FullKeyboardView.keyTickStyle(hapticStrength))
    /// Settings › Keyboard › Strength as of the last bridge sync.
    private var hapticStrength = KeyHapticStrength.medium {
        didSet {
            if hapticStrength != oldValue { barHaptics = UIImpactFeedbackGenerator(style: FullKeyboardView.keyTickStyle(hapticStrength)) }
        }
    }
    /// The dictation bar's selection ticks, as the full keyboard's.
    private lazy var barSelectionHaptics = UISelectionFeedbackGenerator()
    /// The Hex key's last touch-down (wall clock), where a start's probe timeline begins.
    private var hexKeyPressedAt: TimeInterval?
    private var warmStartTimeline: WarmStartTimeline?
    /// What `scheduleLaunchWork` will still run once the keys are up.
    private var launchWork: [DispatchWorkItem] = []
    /// iOS warned this keyboard about memory: optional work stays off.
    private var memoryWarned = false
    /// The launch's optional work ran: Translation's languages may be listed from now on.
    private var mayRunOptionalWork = false

    /// Haptics need Full Access, and Settings › Keyboard › Haptics covers both keyboards.
    private var allowsKeyHaptics: Bool {
        hasFullAccess && hapticsSetting
    }

    /// Settings › Keyboard › Haptics as of the last bridge sync (on appearing and every poll),
    /// so a key press doesn't open the App Group's settings to ask.
    private var hapticsSetting = true

    /// UIKit's own input view, as a `KeyboardInputView` so the keys can click.
    override func loadView() {
        super.loadView()
        let system = inputView
        let input = KeyboardInputView(frame: system?.frame ?? .zero, inputViewStyle: system?.inputViewStyle ?? .keyboard)
        input.allowsSelfSizing = system?.allowsSelfSizing ?? false
        input.autoresizingMask = system?.autoresizingMask ?? []
        input.translatesAutoresizingMaskIntoConstraints = system?.translatesAutoresizingMaskIntoConstraints ?? true
        inputView = input
        if viewIfLoaded !== input { view = input }
        Self.inputViews[keyboardID] = WeakInputView(input)
    }

    deinit {
        // iOS frees a keyboard it never said disappeared without another word.
        let id = keyboardID
        Task { @MainActor in
            KeyboardViewController.onScreen.disappeared(id)
            KeyboardViewController.inputViews.removeValue(forKey: id)?.view?.unmountHexViews()
        }
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        reportFullAccess(force: true)
        // Before any typing log starts: the keyboard may update before Hex is opened.
        TypingLogSettings().turnOffOnce()
        KeyboardLocalDictation.allowHapticsWhileRecording()
        _ = Self.hostLeaveObserver
        configureView()
        syncFromBridge(animated: false)
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        reportFullAccess(force: true)
        isRetired = false
        autoStartPending = true
        coldStartJobID = UUID().uuidString
        didHandOffCurrentField = false
        handOffWaitsForPosture = false
        // The keyboard may come back for another field: settle on where the dictation was last seen.
        dictationEdits.finish(nil)
        mountSurface()
        fullKeyboard?.prepareForField()
        isShown = true
        Self.onScreen.appeared(keyboardID, in: currentDocument)
        updateBackdrop()
        beginTypingLogSession(trigger: "appear")
        handOffNumericPadIfNeeded()
        syncFromBridge(animated: false)
        schedulePoll(force: true)
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        // iOS holds back touches near the screen's side edges in a keyboard's window while it
        // waits to see whether they start a system gesture: on an iPhone they reached the keys
        // about 80 ms late (Record typing logs), so q, a, p, l, shift and delete lagged and
        // could land after the next key. Apple's own keyboard has no such delay.
        view.window?.gestureRecognizers?.forEach { $0.delaysTouchesBegan = false }
        // Onboarding's practice tells Hex's keyboard on screen from another one, or iOS's
        // dictation, by these.
        bridge.recordKeyboardShown(at: Date().timeIntervalSince1970)
        scheduleLaunchWork()
    }

    /// The work a keyboard needs only once its keys are up waits for its first frame
    /// (`KeyboardLaunchWork`): the text checker and lexicon, then, while memory allows, Apple
    /// Speech's and Translation's languages and the capture page. Until that frame iOS shows
    /// an empty panel, and it goes back to the previous keyboard when a keyboard is slow to
    /// draw or ends while it starts.
    private func scheduleLaunchWork() {
        launchWork.forEach { $0.cancel() }
        let lexicon = DispatchWorkItem { [weak self] in
            guard let self, self.isShown else { return }
            self.warmFullKeyboardLexiconIfNeeded()
        }
        let optional = DispatchWorkItem { [weak self] in
            guard let self, self.isShown,
                  KeyboardLaunchWork.allowsOptionalWork(
                      headroom: os_proc_available_memory(),
                      memoryWarned: self.memoryWarned
                  ) else { return }
            self.mayRunOptionalWork = true
            self.listDictationLanguagesIfNeeded()
            self.listTranslationLanguagesIfNeeded()
            self.prepareLocalDictationIfNeeded()
        }
        // After the optional work's turn: opening the microphone is the heaviest thing the
        // keyboard does, and iOS gives up on a keyboard slow to draw.
        let autoStart = DispatchWorkItem { [weak self] in
            self?.autoStartDictationIfPending()
            // Experiments › Speech model in keyboard: last, and it loads off the main thread.
            if let self, self.isShown, self.hasFullAccess, !self.memoryWarned { KeyboardParakeet.shared.warmUp() }
        }
        launchWork = [lexicon, optional, autoStart]
        DispatchQueue.main.asyncAfter(deadline: .now() + KeyboardLaunchWork.afterFirstFrame, execute: lexicon)
        DispatchQueue.main.asyncAfter(deadline: .now() + KeyboardLaunchWork.optionalWorkDelay, execute: optional)
        DispatchQueue.main.asyncAfter(deadline: .now() + KeyboardLaunchWork.autoStartDelay, execute: autoStart)
    }

    /// Settings › Keyboard › Auto-start for this appearance: Always, or When switching if you
    /// seem to have switched back to Hex in this field (`KeyboardSwitchGuess`).
    private func wantsAutoStart() -> Bool {
        switch FullKeyboardSettings().dictationBarAutoStart {
        case .never: false
        case .always: true
        case .whenSwitching: switchGuess.guess.isSwitchBack(to: currentDocument)
        }
    }

    /// Starts dictating as Dictate's tap would, only on the dictation bar, idle, and where it
    /// dictates without leaving the field: a warm Hex session or in-keyboard dictation, never
    /// by opening Hex.
    private func autoStartDictationIfPending() {
        guard autoStartPending, isShown else { return }
        autoStartPending = false
        guard wantsAutoStart(), fullKeyboard == nil, barEmojiPanel == nil, viewModel.hasFullAccess,
              viewModel.stagePhase == .idle,
              KeyboardFieldRouting.route(for: textDocumentProxy.keyboardType) != .handOff
        else { return }
        if viewModel.dictatesThroughHex {
            startWarmDictation()
        } else if let local = viewModel.onLocalDictateTap {
            local()
        }
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        bridge.recordKeyboardHidden(at: Date().timeIntervalSince1970)
        switchGuess.keyboardHid(in: currentDocument, at: Date().timeIntervalSince1970)
        autoStartPending = false
        handOffWaitsForPosture = false
        isRetired = false
        if dictationEdits.isTracking { dictationEdits.finish(editContext) }
        TypingJournal.sharedIfEnabled()?.save()
        AdaptiveTouch.current?.save()
        isShown = false
        Self.onScreen.disappeared(keyboardID)
        launchWork.forEach { $0.cancel() }
        launchWork = []
        fullKeyboard?.engine.commitHangulComposition()
        closeBarEmojiPanel()
        // A dictation under way finishes without the keyboard, and the next one delivers it.
        localDictation.keyboardWillHide()
        endTypingLogSession(reason: "disappear", settleDictation: editContext)
        pollTimer?.invalidate()
        pollTimer = nil
    }

    override func viewDidDisappear(_ animated: Bool) {
        super.viewDidDisappear(animated)
        guard !isShown else { return }
        tearDownSurface()
    }

    override func didReceiveMemoryWarning() {
        super.didReceiveMemoryWarning()
        memoryWarned = true
        fullKeyboard?.handleMemoryWarning()
        closeBarEmojiPanel()
        barEmojiLibrary = nil
        EmojiLibrary.evictCached()
        localDictation.handleMemoryWarning()
        typingLog?.flush()
    }

    override func viewWillLayoutSubviews() {
        super.viewWillLayoutSubviews()
        if let fullKeyboard {
            applyHeight(fullKeyboard.preferredHeight)
        } else {
            applyBarInset()
        }
        if handOffWaitsForPosture { handOffNumericPadIfNeeded() }
    }

    /// The dictation bar's own height, under `barTopInset`: 4 pt top padding, the 40 pt
    /// top row (44 pt tap areas around 26 pt pills), a 2 pt gap, the 44 pt stage well, an
    /// 8 pt gap, the 42 pt keys, and 6 pt bottom padding.
    private static let barHeight: CGFloat = 146

    /// On an iPad, iPadOS adds its shortcut bar (undo, redo, paste, and the app's own buttons)
    /// to a custom keyboard. Mostly it sits above the keyboard, but some apps draw it over the
    /// keyboard's top instead: in Notes on an iPad mini (iPadOS 26.6, TestFlight) its buttons
    /// reached about 40 pt down and covered the bar's top row and its language and style trays.
    /// The keyboard can't tell which an app does, so on an iPad the bar keeps that strip clear.
    /// A floating keyboard is iPhone-wide and has no shortcut bar; before the first layout
    /// (no width yet) the keyboard is taken to be docked, as it mostly is.
    private var barTopInset: CGFloat {
        let width = view.bounds.width
        return UIDevice.current.userInterfaceIdiom == .pad && (width == 0 || width >= 500) ? 44 : 0
    }

    /// Keeps the dictation bar's top inset, and with it the keyboard's height, right for the
    /// keyboard's width (it changes as an iPad keyboard floats or docks).
    private func applyBarInset() {
        guard fullKeyboard == nil, barEmojiPanel == nil, let barTopConstraint else { return }
        let inset = barTopInset
        if barTopConstraint.constant != inset { barTopConstraint.constant = inset }
        applyHeight(Self.barHeight + inset)
    }

    /// The keyboard's height. Set once before the first layout (the surface's own height), then
    /// only when the surface asks for another (the emoji search's row), and never animated:
    /// iOS sizes the panel from it while it swaps keyboards.
    private func applyHeight(_ height: CGFloat) {
        guard let heightConstraint else {
            let constraint = view.heightAnchor.constraint(equalToConstant: height)
            constraint.priority = UILayoutPriority(999)
            constraint.isActive = true
            heightConstraint = constraint
            preferredContentSize.height = height
            return
        }
        guard heightConstraint.constant != height else { return }
        UIView.performWithoutAnimation {
            heightConstraint.constant = height
            preferredContentSize.height = height
        }
    }

    override func textWillChange(_ textInput: UITextInput?) {
        super.textWillChange(textInput)
        if KeyboardFieldRouting.route(for: textDocumentProxy.keyboardType) != .handOff {
            didHandOffCurrentField = false
        }
    }

    override func textDidChange(_ textInput: UITextInput?) {
        super.textDidChange(textInput)
        if isShown || isRetired { Self.onScreen.moved(keyboardID, to: currentDocument) }
        updateKeyboardTraits()
        followTypingLogField()
        fullKeyboard?.textDidChange()
        observeDictationEdits(fromKeys: false)
        noteHostTextChange()
        handOffNumericPadIfNeeded()
    }

    /// The cursor moved or a selection changed: a Hangul run that is no longer right before
    /// the cursor commits, as its text is already in the field.
    override func selectionDidChange(_ textInput: UITextInput?) {
        super.selectionDidChange(textInput)
        fullKeyboard?.engine.commitHangulIfHostMoved()
    }

    // MARK: - Record typing logs

    /// Starts a session for the field the keyboard is in, while the experiment is on. A
    /// field that must not be logged gets a `skipped` line and nothing else.
    private func beginTypingLogSession(trigger: String) {
        guard let log = TypingLogger.sharedIfEnabled() else {
            typingLog = nil
            return
        }
        typingLog = log
        log.contextSource = { [weak self] in
            self?.typingLogContext ?? TypingLogContext(before: nil, after: nil, hasText: false)
        }
        let traits = TypingLogTraits(textDocumentProxy, document: TypingLogTraits.documentIdentifier(of: textDocumentProxy))
        typingLogTraits = traits
        log.beginSession(typingLogSessionInfo(trigger: trigger, traits: traits))
        attachTypingLog()
        syncLoggingMark()
    }

    /// `settleDictation` is the text the last dictation's outcome is judged on (nil: where
    /// it was last seen, for a field the keyboard already left).
    private func endTypingLogSession(reason: String, settleDictation context: DictationEditTracker.Context?) {
        pendingHostContext?.cancel()
        pendingHostContext = nil
        guard let log = typingLog else { return }
        if logDictationEdits.isTracking { logDictationEdits.finish(context) }
        fullKeyboard?.typingLog = nil
        log.endSession(reason: reason)
        typingLogTraits = nil
        syncLoggingMark()
    }

    /// Both keyboards show a small mark the whole time typing logs record this field.
    private func syncLoggingMark() {
        let logging = typingLog?.isRecording == true
        if viewModel.isLoggingTyping != logging {
            viewModel.isLoggingTyping = logging
        }
    }

    private func attachTypingLog() {
        guard let fullKeyboard else { return }
        let log = typingLog?.isRecording == true ? typingLog : nil
        guard fullKeyboard.typingLog !== log || log != nil else { return }
        fullKeyboard.typingLog = log
        fullKeyboard.typingLogSessionBegan()
    }

    /// Turned on or off while the keyboard is shown.
    private func syncTypingLogEnabled() {
        let enabled = TypingLogSettings().isEnabled
        if enabled, typingLog == nil, isShown {
            beginTypingLogSession(trigger: "enabled")
        } else if !enabled, typingLog != nil {
            endTypingLogSession(reason: "disabled", settleDictation: nil)
            typingLog = nil
            _ = TypingLogger.sharedIfEnabled()
        }
    }

    /// Another field, or the same field with new traits: a new session (which may be skipped).
    private func followTypingLogField() {
        guard typingLog != nil, isShown else { return }
        let traits = TypingLogTraits(textDocumentProxy, document: TypingLogTraits.documentIdentifier(of: textDocumentProxy))
        if let current = typingLogTraits, !current.isOtherField(than: traits) {
            // The same field: keep its document once UIKit names it.
            if current.document == nil { typingLogTraits?.document = traits.document }
            return
        }
        endTypingLogSession(reason: "field_changed", settleDictation: nil)
        beginTypingLogSession(trigger: "field")
    }

    /// The host changed the text or moved the cursor. Hosts call this in bursts (and some
    /// after Hex's own edits), so the context is written once things settle, at once when
    /// the field was emptied.
    private func noteHostTextChange() {
        guard let log = typingLog, log.isRecording else { return }
        pendingHostContext?.cancel()
        guard textDocumentProxy.hasText else {
            pendingHostContext = nil
            log.context("text_did_change")
            return
        }
        let work = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated {
                self?.pendingHostContext = nil
                self?.typingLog?.context("text_did_change")
            }
        }
        pendingHostContext = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3, execute: work)
    }

    private var typingLogContext: TypingLogContext {
        TypingLogContext(
            before: textDocumentProxy.documentContextBeforeInput,
            after: textDocumentProxy.documentContextAfterInput,
            hasText: textDocumentProxy.hasText
        )
    }

    private func typingLogSessionInfo(trigger: String, traits: TypingLogTraits) -> TypingLogSessionInfo {
        let bundle = Bundle.main.infoDictionary
        let orientation = fullKeyboard?.orientation ?? FullKeyboardView.orientation(for: traitCollection)
        let dark = textDocumentProxy.keyboardAppearance == .dark || traitCollection.userInterfaceStyle == .dark
        return TypingLogSessionInfo(
            trigger: trigger,
            surface: fullKeyboard == nil ? "bar" : "full",
            traits: traits,
            bounds: view.bounds.size,
            safeArea: view.safeAreaInsets,
            orientation: orientation == .landscape ? "landscape" : "portrait",
            appearance: dark ? "dark" : "light",
            device: TypingLogSessionInfo.deviceModel,
            system: "\(UIDevice.current.systemName) \(UIDevice.current.systemVersion)",
            version: bundle?["CFBundleShortVersionString"] as? String ?? "",
            build: bundle?["CFBundleVersion"] as? String ?? "",
            locale: Locale.current.identifier,
            languages: Array(Locale.preferredLanguages.prefix(4)),
            keyboardLanguage: primaryLanguage,
            inputMode: textDocumentProxy.documentInputMode?.primaryLanguage,
            fullAccess: hasFullAccess,
            settings: [
                TypingLogField("autocorrect", .bool(FullKeyboardSettings().autocorrect)),
                TypingLogField("spatialAutocorrect", .bool(FullKeyboardSettings().spatialAutocorrect)),
                TypingLogField("swipeTyping", .bool(FullKeyboardSettings().swipeTyping)),
                TypingLogField("emoji", .bool(FullKeyboardSettings().emoji)),
                TypingLogField("key_popups", .bool(FullKeyboardSettings().keyPopups)),
                TypingLogField("haptics", .bool(FullKeyboardSettings().haptics)),
                TypingLogField("learnHowIType", .bool(TypingJournalSettings().isEnabled)),
                TypingLogField("cleanupTagger", .bool(CleanupTaggerSettings().isEnabled)),
                TypingLogField("hexKeyPlacement", .string(HexKeyPlacement.current.rawValue)),
                TypingLogField("keyboardLayout", .string(fullKeyboard?.engine.language.rawValue ?? "")),
                TypingLogField("dictationLanguage", .string(LanguageSettings().dictationLanguage ?? "automatic")),
                TypingLogField("style", .string(bridge.style.rawValue)),
            ]
        )
    }

    // MARK: - Learn how I type

    /// Dictations are followed only while Learn how I type is on, in plain text fields
    /// that allow autocorrect.
    private var learnsFromDictation: Bool {
        guard TypingJournalSettings().isEnabled,
              case .keys(let kind, _) = KeyboardFieldRouting.route(for: textDocumentProxy.keyboardType)
        else { return false }
        return TypingJournal.allows(field: kind, target: documentTarget)
    }

    private var editContext: DictationEditTracker.Context {
        DictationEditTracker.Context(
            before: textDocumentProxy.documentContextBeforeInput,
            after: textDocumentProxy.documentContextAfterInput,
            hasText: textDocumentProxy.hasText
        )
    }

    private func observeDictationEdits(fromKeys: Bool) {
        guard dictationEdits.isTracking || logDictationEdits.isTracking else { return }
        let context = editContext
        if logDictationEdits.isTracking { logDictationEdits.observe(context, fromKeys: fromKeys) }
        if dictationEdits.isTracking { dictationEdits.observe(context, fromKeys: fromKeys) }
    }

    /// A return key labelled Send ends the dictation's edit window: what is there is sent.
    private func returnKeyWillInsert() {
        guard textDocumentProxy.returnKeyType == .send else { return }
        if logDictationEdits.isTracking { logDictationEdits.finish(editContext) }
        guard dictationEdits.isTracking else { return }
        dictationEdits.finish(editContext)
    }

    private func handOffNumericPadIfNeeded() {
        guard let fullKeyboard else { return }
        if KeyboardFieldRouting.route(for: textDocumentProxy.keyboardType) == .handOff {
            guard !didHandOffCurrentField else { return }
            // The Duo's posture needs the keyboard's window: layout asks again once it has one.
            guard let posture = fullKeyboard.currentPosture else {
                handOffWaitsForPosture = true
                return
            }
            handOffWaitsForPosture = false
            didHandOffCurrentField = true
            if KeyboardFieldRouting.handsOffNumberPad(needsInputModeSwitchKey: needsInputModeSwitchKey, posture: posture) {
                advanceToNextInputMode()
            }
        } else {
            didHandOffCurrentField = false
        }
    }

    private func warmFullKeyboardLexiconIfNeeded() {
        guard let fullKeyboard else { return }
        fullKeyboard.warmUpTextCheckerAsync()
        guard !didWarmLexicon else { return }
        didWarmLexicon = true
        // UIKit calls back on its lexicon-request queue. A main-actor closure here trips
        // Swift's runtime isolation check (EXC_BREAKPOINT, TestFlight build 3), so the
        // callback is nonisolated and hops to the main actor itself.
        requestSupplementaryLexicon { @Sendable [weak self] lexicon in
            Task { @MainActor in
                self?.fullKeyboard?.loadLexicon(lexicon)
            }
        }
    }

    /// The backdrop shows only over an earlier Hex keyboard iOS never took off screen; a keyboard
    /// superseded under a newer one in the same field retires its views and poll timer, and wakes
    /// back up if the newer one disappears while iOS still keeps this one on screen.
    @objc private func updateBackdrop() {
        if isShown, Self.onScreen.isSuperseded(keyboardID) {
            retireSupersededKeyboard()
            return
        }
        if isRetired, !Self.onScreen.isSuperseded(keyboardID) {
            restoreRetiredKeyboard()
        }
        guard isShown else { return }
        if Self.onScreen.isOverAnother(keyboardID) {
            if backdrop == nil {
                let created = KeyboardBackdropView()
                created.translatesAutoresizingMaskIntoConstraints = false
                (view as? KeyboardInputView)?.noteHexSubview(created)
                view.insertSubview(created, at: 0)
                NSLayoutConstraint.activate([
                    created.leadingAnchor.constraint(equalTo: view.leadingAnchor),
                    created.trailingAnchor.constraint(equalTo: view.trailingAnchor),
                    created.topAnchor.constraint(equalTo: view.topAnchor),
                    created.bottomAnchor.constraint(equalTo: view.bottomAnchor),
                ])
                backdrop = created
            }
        } else if let backdrop {
            backdrop.removeFromSuperview()
            self.backdrop = nil
        }
    }

    private func tearDownSurface() {
        handOffWaitsForPosture = false
        closeBarEmojiPanel()
        barEmojiLibrary = nil
        children.forEach { child in
            child.willMove(toParent: nil)
            child.viewIfLoaded?.removeFromSuperview()
            child.removeFromParent()
        }
        surface?.removeFromSuperview()
        surface = nil
        fullKeyboard = nil
        barTopConstraint = nil
        didWarmLexicon = false
        backdrop?.removeFromSuperview()
        backdrop = nil
        (viewIfLoaded as? KeyboardInputView)?.unmountHexViews()
    }

    private func retireSupersededKeyboard() {
        autoStartPending = false
        handOffWaitsForPosture = false
        if dictationEdits.isTracking { dictationEdits.finish(nil) }
        TypingJournal.sharedIfEnabled()?.save()
        AdaptiveTouch.current?.save()
        fullKeyboard?.engine.commitHangulComposition()
        isShown = false
        isRetired = true
        launchWork.forEach { $0.cancel() }
        launchWork = []
        localDictation.keyboardWillHide()
        endTypingLogSession(reason: "superseded", settleDictation: nil)
        pollTimer?.invalidate()
        pollTimer = nil
        tearDownSurface()
    }

    private func restoreRetiredKeyboard() {
        isRetired = false
        autoStartPending = false
        handOffWaitsForPosture = false
        if KeyboardFieldRouting.route(for: textDocumentProxy.keyboardType) == .handOff {
            didHandOffCurrentField = true
        }
        mountSurface()
        fullKeyboard?.prepareForField()
        isShown = true
        syncFromBridge(animated: false)
        schedulePoll(force: true)
        if viewIfLoaded?.window != nil {
            scheduleLaunchWork()
        }
    }

    private func configureView() {
        guard !didConfigureView else { return }
        didConfigureView = true
        view.backgroundColor = .clear
        NotificationCenter.default.addObserver(self, selector: #selector(updateBackdrop), name: Self.onScreenDidChange, object: nil)

        viewModel.inputViewController = self
        dictationEdits.onResult = { result in
            let language = LanguageLearning.dictationCode(LanguageSettings().effectiveDictationLanguage)
            TypingJournal.sharedIfEnabled()?.recordDictation(result, language: language)
        }
        logDictationEdits.onResult = { [weak self] result in
            self?.typingLog?.dictationOutcome(result)
        }
        localDictation.corrections = {
            let language = LanguageLearning.dictationCode(LanguageSettings().effectiveDictationLanguage)
            return TypingJournal.sharedIfEnabled()?.dictationCorrections(language: language) ?? []
        }
        localDictation.currentDocument = { [weak self] in self?.currentDocument }
        viewModel.onPasteDictation = { [weak self] in self?.pasteOfferedDictation() }
        viewModel.onDictateTap = { [weak self] in self?.startWarmDictation() }
        viewModel.onHexKeyPress = { [weak self] at in self?.hexKeyPressed(at: at) }
        viewModel.onHexKeyAbandon = { [weak self] in self?.localDictation.discard() }
        viewModel.onKeyFeedback = { [weak self] in
            UIDevice.current.playInputClick()
            guard let self, self.allowsKeyHaptics else { return }
            self.barHaptics.impactOccurred(intensity: FullKeyboardView.keyTickIntensity(self.hapticStrength))
            self.barHaptics.prepare()
        }
        viewModel.onSelectionStep = { [weak self] in
            guard let self, self.allowsKeyHaptics else { return }
            self.barSelectionHaptics.selectionChanged()
            self.barSelectionHaptics.prepare()
        }
        viewModel.onStopTap = { [weak self] in
            guard let self else { return }
            if self.localDictation.isActive {
                self.localDictation.stop()
                // Loading the cleanup model while recording starts ran older iPhones out of
                // keyboard memory, so it loads after the microphone closes.
                CleanupTagger.prepareIfEnabled(
                    style: self.bridge.style,
                    language: LanguageSettings().effectiveDictationLanguage
                )
            } else {
                self.stopWarmDictation()
            }
        }
        viewModel.onCancelTap = { [weak self] in
            guard let self else { return }
            if self.localDictation.isActive { self.localDictation.cancel() } else { self.cancelWarmDictation() }
        }
        syncLocalDictationAvailability(access: dictationAccess(now: Date().timeIntervalSince1970))
        viewModel.onStyleSelected = { [weak self] style in
            guard let self else { return }
            self.bridge.style = style
            if self.viewModel.style != style {
                self.viewModel.style = style
            }
            self.refreshColdStartURL(now: Date().timeIntervalSince1970, force: true)
        }
        viewModel.onLanguageSelected = { [weak self] language in
            LanguageSettings().chooseDictationLanguage(language)
            self?.languageSignature = nil
            self?.syncLanguageState()
        }
        viewModel.onLanguageRemoved = { [weak self] language in
            self?.typingLog?.ui("dictation_language_removed", [TypingLogField("language", .string(language))])
            LanguageSettings().removeDictationLanguage(language)
            self?.languageSignature = nil
            self?.syncLanguageState()
        }
        viewModel.onTranslationToggled = { [weak self] on in
            DictationTranslationSettings().isEnabled = on
            self?.languageSignature = nil
            self?.syncLanguageState()
        }
        viewModel.onTranslationTargetSelected = { [weak self] target in
            let settings = DictationTranslationSettings()
            settings.targetLanguage = target
            settings.isEnabled = true
            self?.languageSignature = nil
            self?.syncLanguageState()
        }
        viewModel.onSpace = { [weak self] in
            self?.typingLog?.edit("insert", [
                TypingLogField("inserted", .string(" ")),
                TypingLogField("src", .string("bar")),
            ])
            self?.textDocumentProxy.insertText(" ")
            self?.observeDictationEdits(fromKeys: true)
            self?.typingLog?.context("space")
        }
        viewModel.onMoveCursor = { [weak self] offset in
            self?.typingLog?.edit("cursor", [
                TypingLogField("offset", .int(offset)),
                TypingLogField("src", .string("bar")),
            ])
            self?.textDocumentProxy.adjustTextPosition(byCharacterOffset: offset)
        }
        viewModel.onInsertText = { [weak self] text in
            self?.typingLog?.edit("insert", [
                TypingLogField("inserted", .string(text)),
                TypingLogField("src", .string("bar")),
            ])
            self?.textDocumentProxy.insertText(text)
            self?.observeDictationEdits(fromKeys: true)
        }
        viewModel.onEmojiTap = { [weak self] in self?.openBarEmojiPanel() }
        viewModel.onDelete = { [weak self] in
            if let self, let log = self.typingLog {
                let deleted = self.textDocumentProxy.documentContextBeforeInput?.last.map { String($0) }
                log.edit("delete", [
                    deleted.map { TypingLogField("deleted", .string($0)) } ?? TypingLogField("deletedCount", .int(1)),
                    TypingLogField("src", .string("bar")),
                ])
            }
            self?.textDocumentProxy.deleteBackward()
            self?.observeDictationEdits(fromKeys: true)
        }
        viewModel.onReturn = { [weak self] in
            if let self {
                self.typingLog?.edit("return", [
                    TypingLogField("inserted", .string("\n")),
                    TypingLogField("returnKeyType", .string(TypingLogTraits.name(self.textDocumentProxy.returnKeyType ?? .default))),
                    TypingLogField("send", .bool(self.textDocumentProxy.returnKeyType == .send)),
                    TypingLogField("src", .string("bar")),
                ])
            }
            self?.returnKeyWillInsert()
            self?.textDocumentProxy.insertText("\n")
            self?.observeDictationEdits(fromKeys: true)
            self?.typingLog?.context("return")
        }
        viewModel.onOpenURL = { [weak self] url in
            guard let self else { return }
            ContainingAppOpener.open(url, from: self)
        }

        mountSurface()
    }

    /// The full keyboard, or the dictation bar once it was chosen, on an iPhone and an iPad alike.
    /// Without Full Access always the full keyboard: the bar types nothing but spaces and line
    /// breaks, and its Dictate needs Full Access. Only switches while idle.
    private func mountSurface() {
        let wantsFull = FullKeyboardSettings().isEnabled || !hasFullAccess
        guard surface == nil || wantsFull != (fullKeyboard != nil) else { return }
        guard surface == nil || viewModel.stagePhase == .idle || viewModel.stagePhase == .failed else { return }
        closeBarEmojiPanel()

        // Hex's own views only (UIKit may keep views of its own here): the surface, and any
        // keyboard or bar an earlier mount left behind.
        children.forEach { child in
            child.willMove(toParent: nil)
            child.viewIfLoaded?.removeFromSuperview()
            child.removeFromParent()
        }
        surface?.removeFromSuperview()
        fullKeyboard = nil
        barTopConstraint = nil
        didWarmLexicon = false

        let mounted: UIView
        var topInset: CGFloat = 0
        if wantsFull {
            let keyboard = FullKeyboardView(
                model: viewModel,
                target: DocumentProxyTextTarget { [weak self] in self?.textDocumentProxy }
            )
            keyboard.hostController = self
            keyboard.onPreferredHeightChange = { [weak self] in
                self?.view.setNeedsLayout()
                self?.view.layoutIfNeeded()
            }
            keyboard.allowsHaptics = { [weak self] in
                self?.allowsKeyHaptics ?? false
            }
            keyboard.onTextEdit = { [weak self] in self?.observeDictationEdits(fromKeys: true) }
            keyboard.onUndoDictation = { [weak self] in
                self?.dictationEdits.didUndo()
                self?.logDictationEdits.didUndo()
            }
            keyboard.onReturnKey = { [weak self] in self?.returnKeyWillInsert() }
            fullKeyboard = keyboard
            mounted = keyboard
            applyHeight(keyboard.preferredHeight)
        } else {
            let hosting = UIHostingController(
                rootView: KeyboardSurfaceView(model: viewModel, defersDotScopeUntilActive: true)
            )
            hosting.view.backgroundColor = .clear
            addChild(hosting)
            hosting.didMove(toParent: self)
            mounted = hosting.view
            topInset = barTopInset
            applyHeight(Self.barHeight + topInset)
        }
        mounted.translatesAutoresizingMaskIntoConstraints = false
        (view as? KeyboardInputView)?.noteHexSubview(mounted)
        if let backdrop {
            view.insertSubview(mounted, aboveSubview: backdrop)
        } else {
            view.addSubview(mounted)
        }
        let top = mounted.topAnchor.constraint(equalTo: view.topAnchor, constant: topInset)
        NSLayoutConstraint.activate([
            mounted.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            mounted.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            top,
            mounted.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        if !wantsFull { barTopConstraint = top }
        surface = mounted
        attachTypingLog()
        schedulePoll(force: false)
    }

    /// The dictation bar polls at 25 Hz; the full keyboard at 4 Hz, and 30 Hz while recording.
    private func schedulePoll(force: Bool) {
        let busy = viewModel.stagePhase == .recording || viewModel.stagePhase == .transcribing
            || viewModel.pendingCommand != nil
        let interval: TimeInterval = fullKeyboard == nil ? 0.04 : (busy ? 1.0 / 30 : 0.25)
        guard force || (pollTimer != nil && interval != pollInterval) else { return }
        pollInterval = interval
        pollTimer?.invalidate()
        pollTimer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) { [weak self] _ in
            MainActor.assumeIsolated {
                self?.syncFromBridge(animated: true)
            }
        }
    }

    /// Full Access, which dictation needs (Hex's App Group, the microphone). In the simulator
    /// a live Hex session stands in for it.
    private func dictationAccess(now: TimeInterval) -> Bool {
        #if targetEnvironment(simulator)
        return hasFullAccess || bridge.snapshot().isAvailable(at: now)
        #else
        return hasFullAccess
        #endif
    }

    /// Tells Hex what this keyboard sees of Full Access, which only a keyboard can see: as it
    /// loads and each time it appears, before anything else runs, then every 5 s while it's
    /// shown. One small App Group write; it reads nothing from the field.
    private func reportFullAccess(force: Bool) {
        let now = Date().timeIntervalSince1970
        let isOn = hasFullAccess
        if let last = lastFullAccessReport, last.isOn == isOn, now - last.at < (force ? 1 : 5) { return }
        let report = KeyboardFullAccessReport(isOn: isOn, at: now, launchedAt: Self.processLaunchedAt)
        lastFullAccessReport = report
        bridge.recordFullAccess(report)
    }

    private func syncFromBridge(animated: Bool) {
        let now = Date().timeIntervalSince1970
        reportFullAccess(force: false)
        updateKeyboardTraits()
        hapticsSetting = FullKeyboardSettings().haptics
        hapticStrength = FullKeyboardSettings().hapticStrength
        fullKeyboard?.syncSettings()
        syncTypingLogEnabled()
        syncLoggingMark()
        let effectiveFullAccess = dictationAccess(now: now)
        syncLocalDictationAvailability(access: effectiveFullAccess)

        guard effectiveFullAccess else {
            viewModel.applyBridgeState(
                hasFullAccess: false,
                snapshot: .offline,
                isModelInstalled: false,
                style: viewModel.style,
                now: now,
                clearPending: true,
                animated: animated
            )
            return
        }

        if dictationEdits.isTracking {
            dictationEdits.tick { editContext }
        }
        if logDictationEdits.isTracking {
            logDictationEdits.tick { editContext }
        }

        localDictation.healIfStalled(now: now)
        let localSnapshot = localDictation.snapshot(now: now)
        let appSnapshot = bridge.snapshot()
        let snap = localSnapshot ?? appSnapshot
        if let timeline = warmStartTimeline, timeline.observe(snap, now: now) { warmStartTimeline = nil }
        var insertedResult = false

        // The keyboard's own dictation, and Hex's even while one of the keyboard's is on screen.
        // Only while shown: a keyboard iOS starts afresh as you come back from Hex syncs in
        // viewDidLoad, before its field is connected, and text inserted then never reaches the
        // field though the proxy takes it (iPad, TestFlight build 115: nothing went in).
        if isShown {
            if let localSnapshot, insertResult(of: localSnapshot, source: "keyboard", now: now) {
                insertedResult = true
            }
            if insertResult(of: appSnapshot, source: "app", now: now) {
                insertedResult = true
            }
            if insertHeldDictation(now: now) {
                insertedResult = true
            }
        }
        if insertedResult {
            coldStartJobID = UUID().uuidString
        }
        refreshOfferedDictation(app: appSnapshot, now: now)

        syncLanguageState()

        viewModel.applyBridgeState(
            hasFullAccess: true,
            snapshot: snap,
            isModelInstalled: bridge.isModelInstalled,
            style: bridge.style,
            now: now,
            clearPending: insertedResult,
            animated: animated
        )

        if viewModel.stagePhase == .recording || viewModel.stagePhase == .transcribing {
            closeBarEmojiPanel()
        }
        refreshColdStartURL(now: now, force: false)
        schedulePoll(force: false)
    }

    // MARK: - Delivering dictations

    /// The field the keyboard is in, when UIKit names it.
    private var currentDocument: UUID? {
        TypingLogTraits.documentIdentifier(of: textDocumentProxy)
    }

    /// Inserts `snap`'s finished dictation once, while it's deliverable.
    private func insertResult(of snap: KeyboardSnapshot, source: String, now: TimeInterval) -> Bool {
        guard snap.hasDeliverableResult(at: now),
              let resultID = snap.resultID,
              let transcript = snap.transcript,
              bridge.lastInsertedResultID != resultID else { return false }
        insertDictation(
            transcript,
            resultID: resultID,
            heard: snap.heard,
            stages: snap.stages,
            translationFailure: snap.translationFailure,
            source: source
        )
        if source == "keyboard" { localDictation.markDelivered(resultID: resultID) }
        return true
    }

    /// A dictation the keyboard finished after it hid goes into its own field while fresh;
    /// elsewhere or later the strip offers it (`refreshOfferedDictation`).
    private func insertHeldDictation(now: TimeInterval) -> Bool {
        guard let held = bridge.heldDictation, bridge.lastInsertedResultID != held.id,
              held.delivery(in: currentDocument, at: now) == .insert else { return false }
        bridge.heldDictation = nil
        insertDictation(held.text, resultID: held.id, heard: held.heard, stages: nil, source: "keyboard")
        return true
    }

    private struct OfferedDictation {
        let id: String
        let text: String
        let heard: String?
        let held: Bool
    }

    /// What the strip's Paste dictation inserts.
    private var offeredDictation: OfferedDictation?

    /// A held dictation for another field or past its moment, else Hex's result that came
    /// too late to go in by itself (the keyboard wasn't up, or came back after it). Never one
    /// already inserted.
    private func refreshOfferedDictation(app: KeyboardSnapshot, now: TimeInterval) {
        var offer: OfferedDictation?
        if let held = bridge.heldDictation {
            if bridge.lastInsertedResultID == held.id {
                bridge.heldDictation = nil
            } else {
                switch held.delivery(in: currentDocument, at: now) {
                case .offer: offer = OfferedDictation(id: held.id, text: held.text, heard: held.heard, held: true)
                case .expired: bridge.heldDictation = nil
                case .insert: break
                }
            }
        }
        if offer == nil, !app.hasDeliverableResult(at: now), app.hasOfferableResult(at: now),
           let id = app.resultID, let text = app.transcript, bridge.lastInsertedResultID != id {
            offer = OfferedDictation(id: id, text: text, heard: app.heard, held: false)
        }
        offeredDictation = offer
        if viewModel.offeredDictation != offer?.text {
            viewModel.offeredDictation = offer?.text
        }
    }

    private func pasteOfferedDictation() {
        guard let offer = offeredDictation else { return }
        if offer.held { bridge.heldDictation = nil }
        insertDictation(offer.text, resultID: offer.id, heard: offer.heard, stages: nil, source: offer.held ? "keyboard" : "app")
        offeredDictation = nil
        viewModel.offeredDictation = nil
    }

    private func insertDictation(
        _ transcript: String,
        resultID: String,
        heard: String?,
        stages: DictationStages?,
        translationFailure: TranslationFailure? = nil,
        source: String
    ) {
        // A two-thumb Dictate's words replace the range it picked, if the field still shows
        // it; either way, the context below is read after that, so spacing sees the result.
        fullKeyboard?.engine.commitHangulComposition()
        fullKeyboard?.applyPendingSelectionDictation()
        let context = editContext
        let insertion = TranscriptFormatter.insertionText(
            for: transcript,
            beforeContext: context.before,
            afterContext: context.after,
            isWord: dictationWordCheck()
        )
        if learnsFromDictation {
            dictationEdits.willInsert(heard: heard, inserted: insertion, context: context)
        } else {
            dictationEdits.finish(context)
        }
        if let log = typingLog, log.isRecording {
            // A translation can't be compared with what was heard.
            let logHeard = stages?.translated == nil ? (heard ?? stages?.raw) : nil
            logDictationEdits.willInsert(heard: logHeard, inserted: insertion, context: context)
        } else if logDictationEdits.isTracking {
            logDictationEdits.finish(context)
        }
        textDocumentProxy.insertText(insertion)
        fullKeyboard?.didInsertDictation(insertion, before: context.before ?? "", after: context.after ?? "")
        typingLog?.dictation(
            source: source,
            resultID: resultID,
            style: bridge.style.rawValue,
            stages: stages,
            delivered: transcript,
            inserted: insertion
        )
        observeDictationEdits(fromKeys: true)
        bridge.lastInsertedResultID = resultID
        showTranslationNote(translationFailure)
    }

    /// Translate what I say pasted a dictation as spoken: both keyboards say so with the fix
    /// (Open Hex goes to Settings › Dictation) for a few seconds, or until the next dictation.
    private func showTranslationNote(_ failure: TranslationFailure?) {
        translationNoteWork?.cancel()
        translationNoteWork = nil
        if viewModel.translationNote != failure {
            viewModel.translationNote = failure
            fullKeyboard?.refreshStrip()
        }
        guard let failure else { return }
        UIAccessibility.post(notification: .announcement, argument: "\(TranslationFailure.note). \(failure.fix)")
        let work = DispatchWorkItem { [weak self] in self?.showTranslationNote(nil) }
        translationNoteWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.translationNoteDuration, execute: work)
    }

    // MARK: - The dictation bar's emoji panel

    /// The full keyboard's emoji panel in place of the dictation bar, at the full keyboard's
    /// height (the bar's 146 pt can't hold its grid); its back key (in place of ABC) goes back to the bar. No
    /// search: its query is typed on letter keys the bar doesn't have.
    private func openBarEmojiPanel() {
        guard barEmojiPanel == nil, fullKeyboard == nil, let surface else { return }
        let language = viewModel.effectiveLanguage
            .flatMap { Locale(identifier: $0).language.languageCode?.identifier } ?? "en"
        guard let library = barEmojiLibrary?.language == language ? barEmojiLibrary
            : EmojiLibrary.bundled(language: language, bundle: Bundle(for: FullKeyboardView.self)) else { return }
        barEmojiLibrary = library
        typingLog?.ui("emoji_open")
        let orientation = FullKeyboardView.orientation(for: traitCollection)
        let panel = EmojiPanelView(library: library, usage: EmojiUsageStore())
        panel.metrics = KeyboardMetrics.for(orientation, innerWidth: view.bounds.width)
        panel.palette = FullKeyboardPalette(isDark: FullKeyboardPalette.isDark(traits: traitCollection))
        panel.showsKeyPopups = FullKeyboardSettings().keyPopups
        panel.showsSearch = false
        panel.returnsToBar = true
        panel.onKeyDown = { [weak self] in self?.viewModel.onKeyFeedback?() }
        panel.onSelectionStep = { [weak self] in self?.viewModel.onKeyFeedback?() }
        panel.onInsert = { [weak self, weak panel] text, entry, source in
            guard let self else { return }
            self.typingLog?.edit("insert", [
                TypingLogField("inserted", .string(text)),
                TypingLogField("src", .string("emoji_\(source)")),
            ])
            self.textDocumentProxy.insertText(text)
            self.observeDictationEdits(fromKeys: true)
            if let panel, entry < panel.library.entries.count {
                panel.usage.record(panel.library.entries[entry].emoji)
            }
        }
        panel.onDelete = { [weak self] in self?.viewModel.onDelete?() }
        panel.onDeleteRepeat = { [weak self] _ in self?.viewModel.onDelete?() }
        panel.onLetters = { [weak self] in
            self?.typingLog?.ui("emoji_close")
            self?.closeBarEmojiPanel()
        }
        panel.translatesAutoresizingMaskIntoConstraints = false
        (view as? KeyboardInputView)?.noteHexSubview(panel)
        view.insertSubview(panel, aboveSubview: surface)
        NSLayoutConstraint.activate([
            panel.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            panel.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            panel.topAnchor.constraint(equalTo: view.topAnchor),
            panel.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
        surface.isHidden = true
        barEmojiPanel = panel
        applyHeight(KeyboardLayout.height(for: orientation))
        panel.fill(before: textDocumentProxy.documentContextBeforeInput)
    }

    private func closeBarEmojiPanel() {
        guard let panel = barEmojiPanel else { return }
        panel.tearDown()
        panel.removeFromSuperview()
        barEmojiPanel = nil
        surface?.isHidden = false
        applyBarInset()
    }

    // MARK: - In-keyboard dictation

    /// Whether the Dictate and Hex keys dictate in the keyboard; nil until first decided.
    private var localDictationAvailable: Bool?
    private var lastAvailabilityNote: TimeInterval = 0

    /// In-keyboard dictation (iOS 26) needs memory for its capture web view and recording
    /// screen, and iOS ends a keyboard that passes its limit (older iPhones, many apps open).
    /// While the keyboard is short of it, `onLocalDictateTap` is nil, so Dictate and the Hex
    /// key take the cold start that opens Hex, as before iOS 26 (a hold stays a tap there).
    /// The same while the keyboard couldn't get its speech model ("Open Hex to download
    /// English."): Hex downloads it, and the keyboard dictates again once it installs.
    /// Without Full Access it is nil too, and the Hex key says dictation needs Full Access.
    /// The same while Experiments › Multilingual model (Parakeet) dictates the language: only
    /// Hex can load that model, so Dictate and the Hex key open Hex, once Hex has it ready.
    /// Until then the keyboard dictates as it would with the experiment off.
    private func syncLocalDictationAvailability(access: Bool) {
        guard KeyboardLocalDictation.isSupported else { return }
        guard access || localDictation.isActive else {
            if viewModel.onLocalDictateTap != nil { viewModel.onLocalDictateTap = nil }
            localDictationAvailable = nil
            return
        }
        let multilingual = MultilingualParakeetSettings()
        let multilingualOpensHex = multilingual.keyboardOpensHex(language: multilingual.language())
            && !KeyboardParakeet.shared.dictatesMultilingualHere
        let needsHex = !localDictation.isActive && (LanguageSettings().keyboardNeedsHex() || multilingualOpensHex)
        let available = localDictation.isActive || (!needsHex && localDictation.hasMemoryToStart)
        guard available != localDictationAvailable else { return }
        // Noted at most every 5 s, as the footprint can hover at the line.
        let now = Date().timeIntervalSince1970
        if localDictationAvailable != nil || !available, now - lastAvailabilityNote >= 5 {
            lastAvailabilityNote = now
            let change = available
                ? "dictating in the keyboard"
                : multilingualOpensHex ? "multilingual model on, Dictate opens Hex"
                : needsHex ? "speech model missing here, Dictate opens Hex" : "short of memory, Dictate opens Hex"
            KeyboardProbeLog.shared.write("\(change) \(KeyboardProbeLog.memory())")
        }
        localDictationAvailable = available
        viewModel.onLocalDictateTap = available ? { [weak self] in
            guard let self else { return }
            self.localDictation.start(
                in: self.view,
                microphone: self.bridge.microphone,
                pressedAt: self.takeHexKeyPress()
            )
        } : nil
    }

    /// Loads in-keyboard dictation's capture page once the keyboard has shown
    /// (`scheduleLaunchWork`, after its first frame), so the Hex key only waits for the
    /// microphone. Only with Full Access, which dictation needs, and not while a warm Hex
    /// session takes the Hex key instead; `prepare` checks memory.
    private func prepareLocalDictationIfNeeded() {
        guard hasFullAccess, viewModel.onLocalDictateTap != nil, !viewModel.isWarmAvailable else { return }
        localDictation.prepare(in: view)
    }

    // MARK: - Dictation language

    /// The dictation language's dictionary, for lowercasing a dictation that continues a
    /// sentence; nil (case kept) when the iPhone has no dictionary for it.
    private func dictationWordCheck() -> ((String) -> Bool)? {
        let settings = LanguageSettings()
        let language = settings.resolvedDictationLanguage
            ?? settings.effectiveDictationLanguage
            ?? Locale.preferredLanguages.first
            ?? "en-US"
        guard TranscriptFormatter.lowercasesContinuations(in: language),
              let code = KeyboardLanguage.spellCheckLanguage(
            forDictation: language,
            available: UITextChecker.availableLanguages
        ) else { return nil }
        let checker = SystemTextChecker(language: code)
        return { checker.isWordValid($0) }
    }

    /// What `syncLanguageState` last read, so the 25 Hz poll only works out the choices on a change.
    private var languageSignature: String?
    /// Apple Speech's languages are listed once per keyboard process.
    private static var didListDictationLanguages = false

    /// Settings › Dictation language, what Automatic picks, and the switch's choices, plus
    /// Translate what I say's target and paste switch, from the App Group (iOS 26, where
    /// dictation is Apple Speech).
    private func syncLanguageState() {
        guard KeyboardLocalDictation.isSupported else { return }
        let settings = LanguageSettings()
        let explicit = settings.dictationLanguage
        // Automatic follows the keyboard's language while it switches between two or more.
        let automatic = settings.keyboardDictationLanguage() ?? settings.automaticDictationLanguage
        let recent = settings.recentDictationLanguages
        let removed = settings.removedDictationLanguages
        let supported = settings.supportedDictationLanguages
        let translation = DictationTranslationSettings()
        let target = translation.isEnabled ? translation.targetLanguage : nil
        // Apple's translation (iOS 26+) backs the keyboards' Translate to <language> switch.
        let translationLanguage: String? = if #available(iOS 26.0, *) { translation.targetLanguage } else { nil }
        let translationLanguages = Self.translationLanguages
        let hinglish = HinglishSettings().isEnabled
        let greeklish = GreeklishSettings().isEnabled
        let signature = [
            explicit ?? "", automatic ?? "", recent.joined(separator: ","), removed.joined(separator: ","),
            "\(supported.count)",
            target ?? "", "\(hinglish)", "\(greeklish)",
            translationLanguage ?? "", "\(translationLanguages?.count ?? -1)",
        ].joined(separator: "|")
        guard signature != languageSignature else { return }
        languageSignature = signature
        let choices = settings.keyboardLanguageChoices(hinglish: hinglish, greeklish: greeklish)
        let more = DictationLanguageSwitch.more(choices: choices, preferred: Locale.preferredLanguages, supported: supported)
        if viewModel.dictationLanguage != explicit { viewModel.dictationLanguage = explicit }
        if viewModel.automaticLanguage != automatic { viewModel.automaticLanguage = automatic }
        if viewModel.languageChoices != choices { viewModel.languageChoices = choices }
        if viewModel.moreLanguages != more { viewModel.moreLanguages = more }
        if viewModel.translationTarget != target { viewModel.translationTarget = target }
        if viewModel.translationLanguage != translationLanguage { viewModel.translationLanguage = translationLanguage }
        let targets = translationLanguage.map {
            DictationTranslationSettings.targetChoices(
                current: $0,
                dictation: choices,
                preferred: Locale.preferredLanguages,
                supported: translationLanguages
            )
        } ?? []
        if viewModel.translationTargetChoices != targets { viewModel.translationTargetChoices = targets }
        listTranslationLanguagesIfNeeded()
    }

    /// Apple's translation languages (minimal identifiers), listed once per keyboard process
    /// so the Hex key menu offers only targets Apple can translate into.
    private static var translationLanguages: [String]?
    private static var didListTranslationLanguages = false

    private func listTranslationLanguagesIfNeeded() {
        guard #available(iOS 26.0, *), hasFullAccess, mayRunOptionalWork, !Self.didListTranslationLanguages else { return }
        Self.didListTranslationLanguages = true
        Task { [weak self] in
            Self.translationLanguages = await DictationTranslator.supportedTargets()
            self?.languageSignature = nil
            self?.syncLanguageState()
        }
    }

    /// Asks Apple Speech which languages it has and what Automatic picks, so the switch can
    /// offer them before Hex has listed them.
    private func listDictationLanguagesIfNeeded() {
        guard #available(iOS 26.0, *), hasFullAccess, !Self.didListDictationLanguages else { return }
        Self.didListDictationLanguages = true
        Task { [weak self] in
            _ = await AppleSpeechLanguages.options()
            _ = await AppleSpeechLanguages.automatic()
            self?.languageSignature = nil
            self?.syncLanguageState()
        }
    }

    private func refreshColdStartURL(now: TimeInterval, force: Bool) {
        let bucket = Int(now / 2.0)
        guard force || bucket != lastURLRefreshBucket else { return }
        lastURLRefreshBucket = bucket
        mountSurface()

        let request: KeyboardLaunchRequest
        if bridge.isModelInstalled {
            request = .startRecording(
                jobID: coldStartJobID,
                requestedAt: now,
                style: bridge.style
            )
        } else {
            request = .startSession(requestedAt: now)
        }
        viewModel.coldStartURL = request.url
    }

    private func updateKeyboardTraits() {
        let globe = needsInputModeSwitchKey
        if viewModel.needsGlobeKey != globe {
            viewModel.needsGlobeKey = globe
        }
        // No interface style override: iOS already gives the keyboard the field's appearance
        // (a field that asks for a dark keyboard gets dark traits in light mode, like the system
        // keyboard), while the document proxy's `keyboardAppearance` can go stale.
        let nextReturn = ReturnKeyStyle(textDocumentProxy.returnKeyType)
        if viewModel.returnKey != nextReturn {
            viewModel.returnKey = nextReturn
        }
        let returnOn = ReturnKeyStyle.isEnabled(
            automatically: textDocumentProxy.enablesReturnKeyAutomatically,
            hasText: textDocumentProxy.hasText
        )
        if viewModel.returnKeyEnabled != returnOn {
            viewModel.returnKeyEnabled = returnOn
        }
        // As the full keyboard's emoji key: fields that take any text.
        let takesEmoji: Bool = {
            guard case .keys(.text, _) = KeyboardFieldRouting.route(for: textDocumentProxy.keyboardType) else { return false }
            return textDocumentProxy.keyboardType != .asciiCapable
        }()
        let showsEmoji = FullKeyboardSettings().emoji && takesEmoji
        if viewModel.showsEmojiKey != showsEmoji {
            viewModel.showsEmojiKey = showsEmoji
        }
    }

    /// In-keyboard dictation starts listening as the finger lands on the Hex key, not when the
    /// touch has become a tap (on lifting) or a hold (0.35 s): both keep that capture, and a
    /// slide to the menu or a cancelled touch drops it (`onHexKeyAbandon`). The same checks as
    /// the Hex key's own choice of in-keyboard dictation; a warm Hex session records instead.
    private func hexKeyPressed(at time: TimeInterval) {
        hexKeyPressedAt = time
        guard viewModel.hasFullAccess, !viewModel.dictatesThroughHex, viewModel.onLocalDictateTap != nil,
              viewModel.stagePhase == .idle || viewModel.stagePhase == .failed else { return }
        localDictation.arm(in: view, microphone: bridge.microphone, pressedAt: time)
    }

    /// The touch-down that led to this start, if it was the Hex key's in the last 2 s.
    private func takeHexKeyPress() -> TimeInterval? {
        defer { hexKeyPressedAt = nil }
        guard let at = hexKeyPressedAt, Date().timeIntervalSince1970 - at < 2 else { return nil }
        return at
    }

    private func startWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard viewModel.hasFullAccess, viewModel.currentSnapshot.isAvailable(at: now) else { return }
        // A warm session that came up since touch-down records instead.
        localDictation.discard(reason: "warm session")
        let jobID = UUID().uuidString
        warmStartTimeline = WarmStartTimeline(jobID: jobID, pressedAt: takeHexKeyPress() ?? now, sentAt: now)
        viewModel.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .start, jobID: jobID, sentAt: now),
            now: now,
            animated: true
        )
        bridge.send(.start, jobID: jobID, style: viewModel.style, sentAt: now)
    }

    private func stopWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard let jobID = viewModel.currentSnapshot.jobID ?? viewModel.pendingCommand?.jobID else { return }
        viewModel.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .stop, jobID: jobID, sentAt: now),
            now: now,
            animated: true
        )
        bridge.send(.stop, jobID: jobID, style: viewModel.style, sentAt: now)
    }

    private func cancelWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard let jobID = viewModel.currentSnapshot.jobID ?? viewModel.pendingCommand?.jobID else { return }
        viewModel.setPendingCommand(
            KeyboardViewModel.PendingCommand(kind: .cancel, jobID: jobID, sentAt: now),
            now: now,
            animated: true
        )
        bridge.send(.cancel, jobID: jobID, style: viewModel.style, sentAt: now)
    }
}
