import Combine
import UIKit

/// The experimental full keyboard: a strip, QWERTY keys with the Hex key right of space,
/// and the recording panel that replaces both while Hex records. Dictation state comes
/// from the same `KeyboardViewModel` the dictation bar uses.
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

    private let plane: KeyboardKeyPlaneView
    private let callout = KeyCalloutView()
    private let hexKey = HexKeyControl()
    private let globeButton = UIButton(type: .custom)
    private let strip = UIView()
    private let undoChip = UIButton(type: .custom)
    private let stripLabel = UILabel()
    private var recordingPanel: HexKeyRecordingPanel?
    private var styleMenu: HexStyleMenuView?
    private var palette = FullKeyboardPalette(isDark: false)
    private var didApplyPalette = false
    private var subscriptions: Set<AnyCancellable> = []
    private var showsRecording = false
    private var lastDictation: String?
    private var undoHideWork: DispatchWorkItem?
    private let haptics = UIImpactFeedbackGenerator(style: .light)

    init(model: KeyboardViewModel, target: KeyboardTextTarget) {
        self.model = model
        engine = FullKeyboardEngine(target: target)
        plane = KeyboardKeyPlaneView(engine: engine)
        super.init(frame: .zero)
        backgroundColor = .clear

        addSubview(strip)
        addSubview(plane)
        addSubview(hexKey)
        addSubview(globeButton)
        addSubview(callout)

        stripLabel.font = .systemFont(ofSize: 14)
        stripLabel.textAlignment = .center
        stripLabel.adjustsFontSizeToFitWidth = true
        stripLabel.minimumScaleFactor = 0.8
        strip.addSubview(stripLabel)

        var chip = UIButton.Configuration.plain()
        chip.image = UIImage(systemName: "arrow.uturn.backward", withConfiguration: UIImage.SymbolConfiguration(pointSize: 13, weight: .medium))
        chip.imagePadding = 6
        chip.title = "Undo dictation"
        chip.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 14, bottom: 0, trailing: 14)
        chip.background.cornerRadius = 17
        undoChip.configuration = chip
        undoChip.isHidden = true
        undoChip.addTarget(self, action: #selector(undoDictation), for: .touchUpInside)
        strip.addSubview(undoChip)

        globeButton.accessibilityLabel = "Next keyboard"
        globeButton.isAccessibilityElement = true

        hexKey.onTap = { [weak self] in self?.hexKeyTapped() }
        hexKey.onLongPress = { [weak self] in self?.showStyleMenu() }

        plane.onKeyDown = { [weak self] _ in self?.keyFeedback() }
        engine.onChange = { [weak self] in self?.engineChanged() }
        engine.onEdit = { [weak self] in self?.hideUndo() }

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
    }

    /// The keyboard came on screen for a (possibly new) field.
    func prepareForField() {
        plane.cancelAllTouches()
        engine.configureForField()
        refreshReturnKey()
        applyAppearance()
        dismissStyleMenu()
    }

    /// Hex inserted a dictation; offer to take it back until the next edit.
    func didInsertDictation(_ text: String) {
        lastDictation = text
        undoChip.isHidden = false
        refreshStrip()
        undoHideWork?.cancel()
        let work = DispatchWorkItem { [weak self] in self?.hideUndo() }
        undoHideWork = work
        DispatchQueue.main.asyncAfter(deadline: .now() + 12, execute: work)
        engine.textDidChange()
    }

    /// Fixtures: show a key held down.
    func debugPress(_ character: String) {
        guard let key = engine.layout.key(.character(character)) else { return }
        engine.touchBegan(id: -1, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
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
        strip.frame = layout.stripFrame
        stripLabel.frame = strip.bounds.insetBy(dx: 16, dy: 0)
        let chipSize = undoChip.sizeThatFits(CGSize(width: strip.bounds.width, height: 34))
        undoChip.frame = CGRect(
            x: (strip.bounds.width - chipSize.width) / 2,
            y: (strip.bounds.height - 34) / 2,
            width: chipSize.width,
            height: 34
        )

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
        guard next != palette || !didApplyPalette else { return }
        didApplyPalette = true
        palette = next
        plane.palette = next
        callout.palette = next
        callout.setNeedsDisplay()
        hexKey.palette = next
        recordingPanel?.palette = next
        styleMenu?.palette = next
        stripLabel.textColor = next.textMuted
        var chip = undoChip.configuration
        chip?.baseForegroundColor = next.textMuted
        chip?.background.backgroundColor = next.chipFill
        undoChip.configuration = chip
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
        case .emergencyCall: "Emergency"
        case .continue: "continue"
        default: "return"
        }
        plane.returnIsAction = switch type {
        case .go, .google, .yahoo, .search, .join, .route, .send, .done, .continue: true
        default: false
        }
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
        }
        refreshStrip()
    }

    private func refreshStrip() {
        if !undoChip.isHidden {
            stripLabel.text = nil
        } else if !model.hasFullAccess {
            stripLabel.text = "Allow Full Access to dictate with Hex"
        } else if model.stagePhase == .failed, let message = model.statusMessage {
            stripLabel.text = message
        } else {
            stripLabel.text = nil
        }
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
            fade(in: [panel], out: [strip, plane, hexKey, globeButton, callout])
        } else if let panel = recordingPanel {
            panel.scope.phase = .hidden
            fade(in: [strip, plane, hexKey, globeButton, callout], out: [panel])
        }
    }

    private func fade(in shown: [UIView], out hidden: [UIView]) {
        shown.forEach { $0.isHidden = false }
        let globeHidden = engine.layout.key(.globe) == nil
        let duration = window == nil ? 0 : 0.18
        UIView.animate(withDuration: duration, delay: 0, options: [.beginFromCurrentState, .allowUserInteraction]) {
            shown.forEach { $0.alpha = 1 }
            hidden.forEach { $0.alpha = 0 }
        } completion: { _ in
            for view in hidden where view.alpha == 0 {
                view.isHidden = true
            }
            if globeHidden { self.globeButton.isHidden = true }
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
        if globeButton.isHidden != (engine.layout.key(.globe) == nil) || hexKey.frame != engine.layout.key(.hex)?.hitFrame {
            setNeedsLayout()
        }
    }

    private func updateCallout() {
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
            haptics.impactOccurred(intensity: 0.6)
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
        if allowsHaptics() { haptics.impactOccurred() }
        plane.cancelAllTouches()
        let menu = HexStyleMenuView()
        menu.palette = palette
        menu.style = model.style
        menu.returnAppID = model.returnAppID
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

    @objc private func undoDictation() {
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
    }

    private func hideUndo() {
        undoHideWork?.cancel()
        undoHideWork = nil
        lastDictation = nil
        if !undoChip.isHidden {
            undoChip.isHidden = true
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
