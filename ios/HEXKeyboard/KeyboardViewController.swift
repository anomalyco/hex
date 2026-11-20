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
    private var lastFullAccessNote: TimeInterval = 0
    private var surface: UIView?
    private var fullKeyboard: FullKeyboardView?
    private var heightConstraint: NSLayoutConstraint?
    private var pollInterval: TimeInterval = 0
    private var didHandOffCurrentField = false
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
    private var isShown = false
    /// The dictation bar's key haptic; the full keyboard has its own generator.
    private lazy var barHaptics = UIImpactFeedbackGenerator(style: .light)

    /// Haptics need Full Access, and Settings › Keyboard › Haptics covers both keyboards.
    private var allowsKeyHaptics: Bool {
        hasFullAccess && FullKeyboardSettings().haptics
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        configureView()
        syncFromBridge(animated: false)
    }

    override func viewWillAppear(_ animated: Bool) {
        super.viewWillAppear(animated)
        coldStartJobID = UUID().uuidString
        didHandOffCurrentField = false
        // The keyboard may come back for another field: settle on where the dictation was last seen.
        dictationEdits.finish(nil)
        mountSurface()
        fullKeyboard?.prepareForField()
        isShown = true
        beginTypingLogSession(trigger: "appear")
        handOffNumericPadIfNeeded()
        syncFromBridge(animated: false)
        schedulePoll(force: true)
    }

    override func viewDidAppear(_ animated: Bool) {
        super.viewDidAppear(animated)
        warmFullKeyboardLexiconIfNeeded()
        listDictationLanguagesIfNeeded()
    }

    override func viewWillDisappear(_ animated: Bool) {
        super.viewWillDisappear(animated)
        if dictationEdits.isTracking { dictationEdits.finish(editContext) }
        TypingJournal.sharedIfEnabled()?.save()
        isShown = false
        endTypingLogSession(reason: "disappear", settleDictation: editContext)
        pollTimer?.invalidate()
        pollTimer = nil
    }

    override func didReceiveMemoryWarning() {
        super.didReceiveMemoryWarning()
        fullKeyboard?.handleMemoryWarning()
        typingLog?.flush()
    }

    override func viewWillLayoutSubviews() {
        super.viewWillLayoutSubviews()
        if let fullKeyboard, heightConstraint?.constant != fullKeyboard.preferredHeight {
            heightConstraint?.constant = fullKeyboard.preferredHeight
            preferredContentSize.height = fullKeyboard.preferredHeight
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
        updateKeyboardTraits()
        followTypingLogField()
        fullKeyboard?.textDidChange()
        observeDictationEdits(fromKeys: false)
        noteHostTextChange()
        handOffNumericPadIfNeeded()
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
        let traits = TypingLogTraits(textDocumentProxy, document: textDocumentProxy.documentIdentifier)
        typingLogTraits = traits
        log.beginSession(typingLogSessionInfo(trigger: trigger, traits: traits))
        attachTypingLog()
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
        let traits = TypingLogTraits(textDocumentProxy, document: textDocumentProxy.documentIdentifier)
        guard traits.signature != typingLogTraits?.signature else { return }
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
        guard fullKeyboard != nil else { return }
        if KeyboardFieldRouting.route(for: textDocumentProxy.keyboardType) == .handOff {
            guard !didHandOffCurrentField else { return }
            didHandOffCurrentField = true
            if needsInputModeSwitchKey {
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

    private func configureView() {
        view.backgroundColor = .clear

        viewModel.inputViewController = self
        dictationEdits.onResult = { result in
            TypingJournal.sharedIfEnabled()?.recordDictation(result)
        }
        logDictationEdits.onResult = { [weak self] result in
            self?.typingLog?.dictationOutcome(result)
        }
        localDictation.corrections = {
            TypingJournal.sharedIfEnabled()?.dictationCorrections() ?? []
        }
        viewModel.onDictateTap = { [weak self] in self?.startWarmDictation() }
        viewModel.onKeyFeedback = { [weak self] in
            guard let self, self.allowsKeyHaptics else { return }
            self.barHaptics.impactOccurred(intensity: 0.65)
            self.barHaptics.prepare()
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
        syncLocalDictationAvailability()
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
        viewModel.onPasteTranslationSelected = { [weak self] translated in
            DictationTranslationSettings().pastesTranslation = translated
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

        let heightConstraint = view.heightAnchor.constraint(equalToConstant: 140)
        heightConstraint.priority = UILayoutPriority(999)
        heightConstraint.isActive = true
        self.heightConstraint = heightConstraint
        mountSurface()
    }

    /// The dictation bar, or the full keyboard when its experiment is on (iPhone only).
    /// Only switches while idle.
    private func mountSurface() {
        let wantsFull = FullKeyboardSettings().isEnabled && UIDevice.current.userInterfaceIdiom == .phone
        guard surface == nil || wantsFull != (fullKeyboard != nil) else { return }
        guard surface == nil || viewModel.stagePhase == .idle || viewModel.stagePhase == .failed else { return }

        children.forEach { child in
            child.willMove(toParent: nil)
            child.removeFromParent()
        }
        surface?.removeFromSuperview()
        fullKeyboard = nil
        didWarmLexicon = false

        let mounted: UIView
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
            heightConstraint?.constant = keyboard.preferredHeight
        } else {
            let hosting = UIHostingController(
                rootView: KeyboardSurfaceView(model: viewModel, defersDotScopeUntilActive: true)
            )
            hosting.view.backgroundColor = .clear
            addChild(hosting)
            hosting.didMove(toParent: self)
            mounted = hosting.view
            heightConstraint?.constant = 140
        }
        preferredContentSize.height = heightConstraint?.constant ?? 140
        mounted.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(mounted)
        NSLayoutConstraint.activate([
            mounted.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            mounted.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            mounted.topAnchor.constraint(equalTo: view.topAnchor),
            mounted.bottomAnchor.constraint(equalTo: view.bottomAnchor),
        ])
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

    private func syncFromBridge(animated: Bool) {
        let now = Date().timeIntervalSince1970
        updateKeyboardTraits()
        fullKeyboard?.syncSettings()
        syncTypingLogEnabled()
        syncLocalDictationAvailability()

        #if targetEnvironment(simulator)
        let simSnap = bridge.snapshot()
        let effectiveFullAccess = hasFullAccess || simSnap.isAvailable(at: now)
        #else
        let effectiveFullAccess = hasFullAccess
        #endif

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

        if now - lastFullAccessNote >= 5 {
            lastFullAccessNote = now
            bridge.fullAccessSeenAt = now
        }

        if dictationEdits.isTracking {
            dictationEdits.tick { editContext }
        }
        if logDictationEdits.isTracking {
            logDictationEdits.tick { editContext }
        }

        let localSnapshot = localDictation.snapshot(now: now)
        let snap = localSnapshot ?? bridge.snapshot()
        var insertedResult = false

        if snap.hasDeliverableResult(at: now),
           let resultID = snap.resultID,
           let rawTranscript = snap.transcript,
           bridge.lastInsertedResultID != resultID {
            let context = editContext
            let insertion = TranscriptFormatter.insertionText(
                for: rawTranscript,
                beforeContext: context.before,
                afterContext: context.after,
                isWord: dictationWordCheck()
            )
            if learnsFromDictation {
                dictationEdits.willInsert(heard: snap.heard, inserted: insertion, context: context)
            } else {
                dictationEdits.finish(context)
            }
            if let log = typingLog, log.isRecording {
                // A translation can't be compared with what was heard.
                let heard = snap.stages?.translated == nil ? (snap.heard ?? snap.stages?.raw) : nil
                logDictationEdits.willInsert(heard: heard, inserted: insertion, context: context)
            } else if logDictationEdits.isTracking {
                logDictationEdits.finish(context)
            }
            textDocumentProxy.insertText(insertion)
            fullKeyboard?.didInsertDictation(insertion)
            typingLog?.dictation(
                source: localSnapshot == nil ? "app" : "keyboard",
                resultID: resultID,
                style: bridge.style.rawValue,
                stages: snap.stages,
                delivered: rawTranscript,
                inserted: insertion
            )
            observeDictationEdits(fromKeys: true)
            bridge.lastInsertedResultID = resultID
            localDictation.markDelivered(resultID: resultID)
            insertedResult = true
            coldStartJobID = UUID().uuidString
        }

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

        refreshColdStartURL(now: now, force: false)
        schedulePoll(force: false)
    }

    // MARK: - In-keyboard dictation

    /// Whether the Dictate and Hex keys dictate in the keyboard; nil until first decided.
    private var localDictationAvailable: Bool?
    private var lastAvailabilityNote: TimeInterval = 0

    /// In-keyboard dictation (iOS 26) needs memory for its capture web view and recording
    /// screen, and iOS ends a keyboard that passes its limit (older iPhones, many apps open).
    /// While the keyboard is short of it, `onLocalDictateTap` is nil, so Dictate and the Hex
    /// key take the cold start that opens Hex, as before iOS 26 (a hold stays a tap there).
    private func syncLocalDictationAvailability() {
        guard KeyboardLocalDictation.isSupported else { return }
        let available = localDictation.isActive || localDictation.hasMemoryToStart
        guard available != localDictationAvailable else { return }
        // Noted at most every 5 s, as the footprint can hover at the line.
        let now = Date().timeIntervalSince1970
        if localDictationAvailable != nil || !available, now - lastAvailabilityNote >= 5 {
            lastAvailabilityNote = now
            let change = available ? "memory back, dictating in the keyboard" : "short of memory, Dictate opens Hex"
            KeyboardProbeLog.shared.write("\(change) \(KeyboardProbeLog.memory())")
        }
        localDictationAvailable = available
        viewModel.onLocalDictateTap = available ? { [weak self] in
            guard let self else { return }
            self.localDictation.start(in: self.view, allowBluetoothMic: self.bridge.allowBluetoothMic)
        } : nil
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
        let automatic = settings.automaticDictationLanguage
        let recent = settings.recentDictationLanguages
        let supported = settings.supportedDictationLanguages
        let translation = DictationTranslationSettings()
        let target = translation.isEnabled ? translation.targetLanguage : nil
        let pastesTranslation = translation.pastesTranslation
        let signature = [
            explicit ?? "", automatic ?? "", recent.joined(separator: ","), "\(supported.count)",
            target ?? "", "\(pastesTranslation)",
        ].joined(separator: "|")
        guard signature != languageSignature else { return }
        languageSignature = signature
        let choices = DictationLanguageSwitch.choices(
            current: explicit,
            recent: recent,
            preferred: Locale.preferredLanguages,
            supported: supported
        )
        if viewModel.dictationLanguage != explicit { viewModel.dictationLanguage = explicit }
        if viewModel.automaticLanguage != automatic { viewModel.automaticLanguage = automatic }
        if viewModel.languageChoices != choices { viewModel.languageChoices = choices }
        if viewModel.translationTarget != target { viewModel.translationTarget = target }
        if viewModel.pastesTranslation != pastesTranslation { viewModel.pastesTranslation = pastesTranslation }
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
        let nextReturn: String = switch textDocumentProxy.returnKeyType {
        case .send: "Send"
        case .google, .yahoo, .search: "Search"
        case .go: "Go"
        case .done: "Done"
        case .join: "Join"
        case .next: "Next"
        case .route: "Route"
        case .continue: "Continue"
        case .emergencyCall: "SOS"
        default: "Return"
        }
        if viewModel.returnKeyLabel != nextReturn {
            viewModel.returnKeyLabel = nextReturn
        }
    }

    private func startWarmDictation() {
        let now = Date().timeIntervalSince1970
        guard viewModel.hasFullAccess, viewModel.currentSnapshot.isAvailable(at: now) else { return }
        let jobID = UUID().uuidString
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
