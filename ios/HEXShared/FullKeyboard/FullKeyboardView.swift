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

    private let stripView = KeyboardStripView()
    private let plane: KeyboardKeyPlaneView
    private let callout = KeyCalloutView()
    private let accentCallout = AccentCalloutView()
    private let hexKey = HexKeyControl()
    private let globeButton = UIButton(type: .custom)
    private var recordingPanel: HexKeyRecordingPanel?
    private var styleMenu: HexStyleMenuView?
    private var palette = FullKeyboardPalette(isDark: false)
    private var didApplyPalette = false
    private var subscriptions: Set<AnyCancellable> = []
    private var showsRecording = false
    private var lastDictation: String?
    private var undoHideWork: DispatchWorkItem?
    private let haptics = UIImpactFeedbackGenerator(style: .light)
    private let selectionHaptics = UISelectionFeedbackGenerator()

    /// Exposed so unit tests can verify lazy allocation and teardown of the Metal recording panel.
    var hasAllocatedRecordingPanel: Bool { recordingPanel != nil }

    init(model: KeyboardViewModel, target: KeyboardTextTarget, autocorrect: AutocorrectEngine? = nil) {
        self.model = model
        engine = FullKeyboardEngine(target: target, autocorrect: autocorrect)
        plane = KeyboardKeyPlaneView(engine: engine)
        super.init(frame: .zero)
        backgroundColor = .clear

        addSubview(stripView)
        addSubview(plane)
        addSubview(hexKey)
        addSubview(globeButton)
        addSubview(callout)
        addSubview(accentCallout)
        accentCallout.isHidden = true

        stripView.onSelectSuggestion = { [weak self] suggestion in
            guard let self else { return }
            self.keyFeedback()
            self.engine.acceptSuggestion(suggestion)
        }
        stripView.onUndoDictation = { [weak self] in self?.undoDictation() }

        globeButton.accessibilityLabel = "Next keyboard"
        globeButton.isAccessibilityElement = true

        hexKey.onTap = { [weak self] in self?.hexKeyTapped() }
        hexKey.onLongPress = { [weak self] in self?.showStyleMenu() }
        hexKey.onLongPressDrag = { [weak self] point in self?.styleMenu?.updateDrag(at: point) }
        hexKey.onLongPressEnd = { [weak self] point in self?.styleMenu?.finishDrag(at: point) }

        plane.onKeyDown = { [weak self] _ in self?.keyFeedback() }
        plane.onAccentMenuOpened = { [weak self] in self?.selectionFeedback() }
        engine.onSelectionStep = { [weak self] in self?.selectionFeedback() }
        engine.onChange = { [weak self] in self?.engineChanged() }
        engine.onEdit = { [weak self] in
            self?.hideUndo()
            self?.refreshReturnKey()
            self?.refreshStrip()
        }

        model.objectWillChange
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.refreshFromModel() }
            .store(in: &subscriptions)
        model.clock.$recordingText
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in self?.refreshRecordingPanel() }
            .store(in: &subscriptions)

        registerForTraitChanges([UITraitUserInterfaceStyle.self, UITraitVerticalSizeClass.self]) { (view: FullKeyboardView, _) in
            view.applyAppearance()
            view.setNeedsLayout()
        }

        engine.autocorrect.isEnabled = FullKeyboardSettings().autocorrect
        engine.configureForField()
        applyAppearance()
        refreshFromModel()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    // MARK: - Size

    static func orientation(for traits: UITraitCollection) -> KeyboardOrientation {
        traits.verticalSizeClass == .compact ? .landscape : .portrait
    }

    var orientation: KeyboardOrientation {
        orientationOverride ?? Self.orientation(for: traitCollection)
    }

    var preferredHeight: CGFloat {
        KeyboardLayout.height(for: orientation)
    }

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

    func syncSettings() {
        let wantsAutocorrect = FullKeyboardSettings().autocorrect
        if engine.autocorrect.isEnabled != wantsAutocorrect {
            engine.autocorrect.isEnabled = wantsAutocorrect
            engine.refreshSuggestions()
        }
    }

    /// The field or its contents changed outside these keys.
    func textDidChange() {
        applyAppearance()
        let field = engine.field
        let route = KeyboardFieldRouting.route(for: engine.target.keyboardType)
        let routeField: KeyboardFieldKind = if case .keys(let kind, _) = route { kind } else { .text }
        if routeField != field {
            plane.cancelAllTouches()
            engine.configureForField()
        } else {
            engine.textDidChange()
        }
        refreshReturnKey()
        refreshStrip()
    }

    /// The keyboard came on screen for a (possibly new) field.
    func prepareForField() {
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
        engine.orientation = orientation
        engine.horizontalInset = isLandscape ? max(safeAreaInsets.left, safeAreaInsets.right) : 0
        engine.width = bounds.width
        let layout = engine.layout

        plane.frame = bounds
        callout.frame = bounds
        accentCallout.frame = bounds
        stripView.frame = layout.stripFrame

        hexKey.cornerRadius = layout.metrics.cornerRadius
        if let hex = layout.key(.hex) {
            hexKey.frame = hex.hitFrame
            hexKey.keycapFrame = hex.frame.offsetBy(dx: -hex.hitFrame.minX, dy: -hex.hitFrame.minY)
        }
        if let globe = layout.key(.globe) {
            globeButton.isHidden = false
            globeButton.frame = globe.hitFrame
        } else {
            globeButton.isHidden = true
        }

        if let panel = recordingPanel {
            panel.frame = bounds
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
            ?? FullKeyboardPalette.isDark(appearance: engine.target.keyboardAppearance, traits: traitCollection)
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
    }

    private func refreshReturnKey() {
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
        if let menu = styleMenu {
            menu.style = model.style
            menu.returnAppID = model.returnAppID
            menu.quickReturnApps = model.quickReturnApps
        }
        refreshStrip()
    }

    private func refreshStrip() {
        let status: String? = if !model.hasFullAccess {
            "Allow Full Access to dictate with Hex"
        } else if model.stagePhase == .failed {
            model.statusMessage
        } else {
            nil
        }
        stripView.update(
            suggestions: engine.suggestions,
            showsUndo: lastDictation != nil,
            statusText: status
        )
    }

    private func refreshRecordingPanel() {
        guard let panel = recordingPanel, showsRecording else { return }
        panel.update(
            phase: model.stagePhase == .transcribing ? .processing : .recording,
            clock: model.clock.recordingText,
            style: model.style
        )
    }

    private func setRecording(_ recording: Bool) {
        showsRecording = recording
        dismissStyleMenu()
        if recording {
            plane.cancelAllTouches()
            hideUndo()
            let panel = recordingPanel ?? makeRecordingPanel()
            panel.palette = palette
            panel.frame = bounds
            refreshRecordingPanel()
            panel.isHidden = false
            fade(in: [panel], out: [stripView, plane, hexKey, globeButton, callout, accentCallout])
        } else if let panel = recordingPanel {
            panel.scope.phase = .hidden
            fade(in: [stripView, plane, hexKey, globeButton, callout], out: [panel]) { [weak self, weak panel] in
                guard let self, !self.showsRecording, let panel, panel === self.recordingPanel else { return }
                panel.tearDown()
                panel.removeFromSuperview()
                self.recordingPanel = nil
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
            for view in hidden where view.alpha == 0 {
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
        panel.onDone = { [weak self] in self?.model.onStopTap?() }
        panel.onCancel = { [weak self] in self?.model.onCancelTap?() }
        panel.onCycleStyle = { [weak self] in
            guard let self else { return }
            self.model.onStyleSelected?(self.model.style.next)
        }
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
        if let key = engine.previewKey, let balloon = layout.balloon(for: key) {
            callout.show(balloon, label: engine.displayLabel(for: key))
        } else {
            callout.show(nil, label: "")
        }
    }

    // MARK: - Actions

    private func keyFeedback() {
        UIDevice.current.playInputClick()
        if allowsHaptics() {
            haptics.impactOccurred(intensity: 0.65)
            haptics.prepare()
        }
    }

    private func selectionFeedback() {
        if allowsHaptics() {
            selectionHaptics.selectionChanged()
            selectionHaptics.prepare()
        }
    }

    private func hexKeyTapped() {
        keyFeedback()
        dismissStyleMenu()
        if hexKey.isWarm {
            model.onDictateTap?()
        } else {
            model.onOpenURL?(model.coldStartURL)
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
        menu.returnAppID = model.returnAppID
        menu.quickReturnApps = model.quickReturnApps
        menu.onHoverChanged = { [weak self] in self?.selectionFeedback() }
        menu.onStyle = { [weak self] style in
            self?.model.onStyleSelected?(style)
            self?.dismissStyleMenu()
        }
        menu.onReturnApp = { [weak self] appID in
            self?.model.onReturnAppSelected?(appID)
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

    /// Fixtures: open the menu without a long press.
    func debugShowStyleMenu() {
        showStyleMenu()
    }

    private func undoDictation() {
        guard let text = lastDictation else { return }
        let before = engine.target.documentContextBeforeInput ?? ""
        // Only take back text that is still right before the cursor.
        if before.hasSuffix(text) || (!before.isEmpty && before.count < text.count && text.hasSuffix(before)) {
            for _ in 0..<text.count {
                engine.target.deleteBackward()
            }
        }
        hideUndo()
        engine.textDidChange()
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
