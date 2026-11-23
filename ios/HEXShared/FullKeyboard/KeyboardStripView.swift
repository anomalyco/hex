import UIKit

/// The 3-slot top bar above the keys, drawn as Apple's iOS 26 predictive bar is (measured in
/// the iOS 27.2 simulator): SF 17 pt regular in muted ink, slots a third of the keyboard wide,
/// 1 × 24 pt dividers ending on the thirds, and a pending autocorrection as a full-slot
/// capsule with the dividers beside it hidden. On an iPhone the strip is Apple's whole 44 pt
/// bar (`KeyboardMetrics.appleBar`): words, dividers and the cell-wide 38 pt bubble on Apple's
/// line, 25 pt below the keyboard's top edge (`KeyboardMetrics.stripTextCenter`). Before it was
/// measured (iOS 18) it is the bar from its pill's top down (38 pt), with a small chip around a
/// word instead of the capsule. The centred
/// `Undo dictation` chip or a status message replaces the slots when needed.
@MainActor
final class KeyboardStripView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { applyPalette() } } }
    /// Hosted in iOS's platter (not a snapshot scene's own backdrop).
    var inSystemPlatter = true {
        didSet {
            guard inSystemPlatter != oldValue else { return }
            for button in slotButtons { button.inSystemPlatter = inSystemPlatter }
            setNeedsLayout()
        }
    }
    /// Each slot's frame in this view, from `KeyboardLayout.stripSlotFrames`; thirds of the
    /// view until the keyboard lays it out.
    var slotFrames: [CGRect]? { didSet { if slotFrames != oldValue { setNeedsLayout(); setNeedsDisplay() } } }
    /// Where the words and dividers centre, from the strip's top (`KeyboardMetrics.stripTextCenter`).
    var textCenterY: CGFloat? {
        didSet {
            guard textCenterY != oldValue else { return }
            for button in slotButtons { button.textCenterY = textCenterY }
            setNeedsLayout()
        }
    }
    /// A suggestion's chip and the dividers' height (`KeyboardMetrics.stripChipHeight`).
    var chipHeight: CGFloat? {
        didSet {
            guard chipHeight != oldValue else { return }
            for button in slotButtons { button.chipHeight = chipHeight }
            setNeedsLayout()
        }
    }
    /// The strip is Apple's bar (`KeyboardMetrics.appleBar`): a pending word gets Apple's bubble,
    /// its cell's width, and the dividers beside it hide, as Apple's do.
    var appleBar = false {
        didSet {
            guard appleBar != oldValue else { return }
            for button in slotButtons { button.fillsCell = appleBar }
            setNeedsLayout()
        }
    }
    /// The Hex key has a cell of its own at the strip's end (Apple's bar): a divider stands
    /// between it and the slot beside it, as between Apple's icon and its first cell.
    var dividerBesideHex = false { didSet { if dividerBesideHex != oldValue { setNeedsLayout() } } }
    /// How far the pills sit in from the strip's top and bottom (`KeyboardMetrics.stripInset`).
    var pillInset: CGFloat = 0.5 {
        didSet {
            guard pillInset != oldValue else { return }
            for button in slotButtons { button.pillInset = pillInset }
            setNeedsLayout()
        }
    }
    var onSelectSuggestion: ((KeyboardSuggestion) -> Void)?
    /// A slot was held (half a second): the word you typed offers Add to vocabulary.
    var onHoldSuggestion: ((KeyboardSuggestion) -> Void)?
    var onUndoDictation: (() -> Void)?
    var onPaste: (() -> Void)?
    var onNextKeyboard: (() -> Void)?
    /// Code from Messages' touches where it opens iOS's keyboard list (`.code(opensList: true)`).
    var onKeyboardListTouch: ((UIButton, UIEvent) -> Void)?
    /// The Full Access hint's Open Hex was tapped (it opens Hex's setup).
    var onOpenHexFromHint: (() -> Void)?
    /// The notice's chip was tapped (`KeyboardStripNotice`).
    var onOpenNotice: (() -> Void)?
    /// The notice's ✕ was tapped (a `dismissible` notice).
    var onDismissNotice: (() -> Void)?

    private let statusLabel = UILabel()
    private let undoChip = UIButton(type: .custom)
    private let slotButtons: [SlotButton] = [SlotButton(), SlotButton(), SlotButton()]
    private var currentSuggestions: [KeyboardSuggestion] = []
    private var showsUndo = false
    private var statusText: String?
    private var clipboard: KeyboardClipboardOffer?
    private var showsFullAccessHint = false
    /// Built the first time the Hex key is tapped without Full Access.
    private var fullAccessHintView: FullAccessHintView?
    private var notice: KeyboardStripNotice?
    /// Built for the first notice, in the Full Access hint's shape.
    private var noticeView: FullAccessHintView?
    /// The Hex key sits left of the strip (Settings › Keyboard › Hex key): the Full Access
    /// hint's Open Hex leads, beside the key.
    var hexOnLeading = false {
        didSet {
            guard hexOnLeading != oldValue else { return }
            fullAccessHintView?.chipLeads = hexOnLeading
            noticeView?.chipLeads = hexOnLeading
            setNeedsLayout()
        }
    }
    /// Where the bar's middle slot centres, from this view's leading edge: the keyboard's middle,
    /// not this view's, which leaves out the Hex key. Paste and Next keyboard centre on it.
    var barMidX: CGFloat? { didSet { if barMidX != oldValue { setNeedsLayout() } } }
    /// The bar around this view, in its coordinates: from the keyboard's top to the bar's bottom
    /// and across it up to the Hex key's cell. While Paste (or Paste dictation) is the only chip,
    /// a tap anywhere on it pastes; beside Code from Messages each chip keeps its own frame. The
    /// Hex key and row 1's reach, laid over it, keep theirs.
    var barTouchFrame: CGRect? { didSet { if barTouchFrame != oldValue { setNeedsLayout() } } }
    /// Plain text on the words' line, as a suggestion is: `UIPasteControl` renders as an empty
    /// blue box inside a keyboard extension (TestFlight build 4), so this reads the clipboard on
    /// tap instead, once, which iOS asks about until Paste from Other Apps is Allow
    /// (`KeyboardClipboardTracker`).
    private let pasteControl = StripChip()
    private let nextKeyboardChip = UIButton(type: .custom)
    /// Between slots 0 and 1, slots 1 and 2, and beside the Hex key.
    private let dividers: [CALayer] = [CALayer(), CALayer(), CALayer()]

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        contentMode = .redraw

        statusLabel.font = .systemFont(ofSize: 13.5)
        statusLabel.textAlignment = .center
        statusLabel.adjustsFontSizeToFitWidth = true
        statusLabel.minimumScaleFactor = 0.8
        statusLabel.isHidden = true
        addSubview(statusLabel)
        for divider in dividers {
            divider.opacity = 0
            layer.addSublayer(divider)
        }

        var chip = UIButton.Configuration.plain()
        chip.image = UIImage(
            systemName: "arrow.uturn.backward",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 12.5, weight: .medium)
        )
        chip.imagePadding = 6
        chip.title = String(localized: "Undo dictation")
        chip.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 13, bottom: 0, trailing: 13)
        chip.background.cornerRadius = 15
        undoChip.configuration = chip
        undoChip.isHidden = true
        undoChip.addTarget(self, action: #selector(handleUndo), for: .touchUpInside)
        // Apple's bar words: SF 17 pt regular in the bar's ink, no chip.
        var paste = UIButton.Configuration.plain()
        paste.image = UIImage(
            systemName: "doc.on.clipboard",
            withConfiguration: UIImage.SymbolConfiguration(font: Self.pasteFont, scale: .small)
        )
        paste.imagePadding = 5
        paste.title = String(localized: "Paste")
        paste.titleTextAttributesTransformer = UIConfigurationTextAttributesTransformer { attributes in
            var attributes = attributes
            attributes.font = Self.pasteFont
            return attributes
        }
        paste.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 8, bottom: 0, trailing: 8)
        paste.background.backgroundColor = .clear
        pasteControl.configuration = paste
        pasteControl.isHidden = true
        pasteControl.addTarget(self, action: #selector(handlePaste), for: .touchUpInside)
        addSubview(pasteControl)
        addSubview(undoChip)

        var next = UIButton.Configuration.plain()
        next.image = UIImage(
            systemName: "keyboard",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 12.5, weight: .medium)
        )
        next.imagePadding = 6
        next.title = String(localized: "Code from Messages")
        next.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 13, bottom: 0, trailing: 13)
        next.background.cornerRadius = 15
        nextKeyboardChip.configuration = next
        nextKeyboardChip.isHidden = true
        nextKeyboardChip.addTarget(self, action: #selector(handleNextKeyboard), for: .touchUpInside)
        nextKeyboardChip.addTarget(self, action: #selector(keyboardListTouch(_:with:)), for: .allTouchEvents)
        addSubview(nextKeyboardChip)

        for (index, button) in slotButtons.enumerated() {
            button.tag = index
            button.addTarget(self, action: #selector(handleSlotTap(_:)), for: .touchUpInside)
            button.addGestureRecognizer(UILongPressGestureRecognizer(target: self, action: #selector(handleSlotHold(_:))))
            addSubview(button)
        }
        applyPalette()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    private static let pasteFont = UIFont.systemFont(ofSize: 17, weight: .regular)

    /// While the recording bar covers the strip (and as it hands the strip back), changes just
    /// show: the edge running across the bar is the motion, and words rolling behind it would
    /// read as the old state coming back first.
    var holdsStill = false

    private var animates: Bool { window != nil && !holdsStill }

    /// `fullAccessHint`, while true, replaces everything else in the strip with what
    /// dictation needs (`FullAccessHintView`); `notice` does the same after it.
    func update(
        suggestions: [KeyboardSuggestion],
        showsUndo: Bool,
        statusText: String?,
        clipboard: KeyboardClipboardOffer? = nil,
        fullAccessHint: Bool = false,
        notice: KeyboardStripNotice? = nil
    ) {
        let changed = suggestions != currentSuggestions || showsUndo != self.showsUndo
            || statusText != self.statusText || clipboard != self.clipboard
            || fullAccessHint != showsFullAccessHint || notice != self.notice
        guard changed else { return }
        currentSuggestions = suggestions
        self.showsUndo = showsUndo
        self.statusText = statusText
        self.clipboard = clipboard
        showsFullAccessHint = fullAccessHint
        self.notice = notice

        if fullAccessHint, fullAccessHintView == nil { _ = makeFullAccessHintView() }
        if let hint = fullAccessHintView { show(hint, fullAccessHint) }
        if let notice, noticeView == nil { _ = makeNoticeView() }
        if let notice { noticeView?.show(notice) }
        if let noticeView { show(noticeView, notice != nil && !fullAccessHint) }
        let showsHint = fullAccessHint || notice != nil
        show(undoChip, showsUndo && !showsHint)
        let showsClipboard = showsClipboard
        var paste = pasteControl.configuration
        let pasteTitle = clipboard == .dictation ? String(localized: "Paste dictation") : String(localized: "Paste")
        if paste?.title != pasteTitle {
            paste?.title = pasteTitle
            pasteControl.configuration = paste
        }
        show(pasteControl, showsPasteChip)
        show(nextKeyboardChip, showsNextKeyboardChip)
        nextKeyboardChip.accessibilityHint = chipOpensList
            ? String(localized: "Shows your keyboards: pick Apple's, where iOS offers codes from Messages and Mail")
            : String(localized: "Switches to the next keyboard, where iOS offers codes from Messages and Mail")
        // Words swap letter by letter (and leave the same way when a chip or status takes the
        // strip); the strip being shown for the first time just draws them.
        let animated = animates
        if showsHint || showsUndo || showsClipboard {
            show(statusLabel, false)
            slotButtons.forEach { $0.configure(with: nil, palette: palette, animated: animated) }
        } else if let statusText, suggestions.isEmpty {
            statusLabel.text = statusText
            show(statusLabel, true)
            slotButtons.forEach { $0.configure(with: nil, palette: palette, animated: animated) }
        } else {
            show(statusLabel, false)
            let slots = Self.slots(for: suggestions)
            for (idx, button) in slotButtons.enumerated() {
                let item = slots.firstIndex(of: idx).map { suggestions[$0] }
                button.configure(with: item, palette: palette, animated: animated)
            }
        }
        setNeedsLayout()
        updateDividers(animated: animated)
    }

    /// Shows or hides a chip in place, on the same frame the words roll out or in: it grows
    /// from 0.96 as it fades in on the settle spring and goes back to 0.96 a fifth quicker. At
    /// first paint it just shows; with Reduce Motion it only fades.
    private func show(_ view: UIView, _ shown: Bool) {
        let target: CGFloat = shown ? 1 : 0
        guard view.isHidden == shown || view.alpha != target else { return }
        guard animates else {
            view.layer.removeAllAnimations()
            view.alpha = target
            view.transform = .identity
            view.isHidden = !shown
            return
        }
        let moves = !StripMotion.reduced
        let small = CGAffineTransform(scaleX: Self.chipScale, y: Self.chipScale)
        let fresh = shown && view.isHidden
        if fresh {
            view.alpha = 0
            view.transform = moves ? small : .identity
            view.isHidden = false
        }
        let duration = moves ? (shown ? StripMotion.settle : StripMotion.settleExit) : StripMotion.reducedFade
        StripMotion.animate(duration, fresh: fresh, {
            view.alpha = target
            view.transform = shown || !moves ? .identity : small
        }, completion: { _ in
            guard view.alpha == 0 else { return }
            view.isHidden = true
            view.transform = .identity
        })
    }

    /// How small a chip is as it arrives and leaves (Kit's motion: in-place enters from 0.96).
    private static let chipScale: CGFloat = 0.96

    /// Which slot shows each suggestion: in order, except a lone emoji, swiped word or reading
    /// of one, which takes the centre. A revert keeps the left slot.
    static func slots(for suggestions: [KeyboardSuggestion]) -> [Int] {
        if suggestions.count == 1, ![.keepLiteral, .secondary, .revert].contains(suggestions[0].kind) { return [1] }
        return Array(suggestions.indices.prefix(3))
    }

    /// Clipboard chips replace the empty strip; suggestions and status win.
    private var showsClipboard: Bool {
        guard let clipboard, !showsUndo, statusText == nil, !showsFullAccessHint, notice == nil else { return false }
        if case .code = clipboard { return true }
        return currentSuggestions.isEmpty
    }

    private var showsPasteChip: Bool {
        if case .code(canPaste: false, _)? = clipboard { return false }
        return showsClipboard
    }

    private var chipOpensList: Bool {
        if case .code(_, opensList: true)? = clipboard { return true }
        return false
    }

    private var showsNextKeyboardChip: Bool {
        guard showsClipboard, case .code = clipboard else { return false }
        return true
    }

    @objc private func handlePaste() {
        onPaste?()
    }

    @objc private func handleNextKeyboard() {
        guard !chipOpensList else { return }
        onNextKeyboard?()
    }

    @objc private func keyboardListTouch(_ sender: UIButton, with event: UIEvent) {
        guard chipOpensList else { return }
        onKeyboardListTouch?(sender, event)
    }

    private func makeFullAccessHintView() -> FullAccessHintView {
        let hint = FullAccessHintView()
        hint.chipLeads = hexOnLeading
        hint.onOpenHex = { [weak self] in self?.onOpenHexFromHint?() }
        hint.isHidden = true
        addSubview(hint)
        fullAccessHintView = hint
        applyPalette()
        setNeedsLayout()
        return hint
    }

    private func makeNoticeView() -> FullAccessHintView {
        let view = FullAccessHintView()
        view.chipLeads = hexOnLeading
        view.onOpenHex = { [weak self] in self?.onOpenNotice?() }
        view.onDismiss = { [weak self] in self?.onDismissNotice?() }
        view.isHidden = true
        addSubview(view)
        noticeView = view
        applyPalette()
        setNeedsLayout()
        return view
    }

    private func applyPalette() {
        statusLabel.textColor = palette.textMuted
        var paste = pasteControl.configuration
        paste?.baseForegroundColor = palette.stripText
        pasteControl.configuration = paste
        var next = nextKeyboardChip.configuration
        next?.baseForegroundColor = palette.textMuted
        next?.background.backgroundColor = palette.chipFill
        nextKeyboardChip.configuration = next
        var chip = undoChip.configuration
        chip?.baseForegroundColor = palette.textMuted
        chip?.background.backgroundColor = palette.chipFill
        undoChip.configuration = chip
        fullAccessHintView?.palette = palette
        noticeView?.palette = palette
        for (item, slot) in Self.slots(for: currentSuggestions).enumerated() {
            slotButtons[slot].configure(with: currentSuggestions[item], palette: palette, animated: false)
        }
        updateDividers(animated: false)
    }

    private var contentCenterY: CGFloat {
        textCenterY ?? bounds.height * 0.5
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let midY = contentCenterY
        place(statusLabel, CGRect(x: 16, y: max(0, midY - 12), width: max(0, bounds.width - 32), height: 24))
        let chipH = min(34, max(18, min(bounds.height - 2 * pillInset, chipHeight ?? .infinity)))
        let chipSize = undoChip.sizeThatFits(CGSize(width: bounds.width, height: chipH))
        place(undoChip, CGRect(
            x: (bounds.width - chipSize.width) / 2,
            y: max(0, midY - chipH / 2),
            width: chipSize.width,
            height: chipH
        ))
        let pasteW = pasteControl.sizeThatFits(CGSize(width: bounds.width, height: chipH)).width
        let nextSize = nextKeyboardChip.sizeThatFits(CGSize(width: bounds.width, height: chipH))
        // Laid out for what is shown, not what is still fading out.
        let pasteShown = showsPasteChip
        let nextShown = showsNextKeyboardChip
        let pair = pasteShown && nextShown
        let gap: CGFloat = 8
        let total = pair ? pasteW + gap + nextSize.width : (pasteShown ? pasteW : nextSize.width)
        // On the keyboard's middle, as the middle slot's word, kept inside the strip.
        let midX = barMidX ?? bounds.width / 2
        let startX = max(0, min(bounds.width - total, midX - total / 2))
        let chipY = max(0, midY - chipH / 2)
        place(pasteControl, CGRect(x: startX, y: chipY, width: pasteW, height: chipH))
        place(nextKeyboardChip, CGRect(
            x: pair ? startX + pasteW + gap : startX,
            y: chipY,
            width: nextSize.width,
            height: chipH
        ))
        pasteControl.touchArea = pasteShown && !nextShown ? barTouchFrame ?? bounds : nil
        // The hint's two lines and chip centre on the words' line, as everything in the strip.
        let hintHalf = min(midY, bounds.height - midY)
        let hintY = midY - hintHalf
        let hintFrame = CGRect(x: 12, y: hintY, width: max(0, bounds.width - 24), height: 2 * hintHalf)
        if let fullAccessHintView { place(fullAccessHintView, hintFrame) }
        fullAccessHintView?.chipHeight = chipH
        if let noticeView { place(noticeView, hintFrame) }
        noticeView?.chipHeight = chipH
        let slots = currentSlotFrames
        for (idx, button) in slotButtons.enumerated() {
            button.inSystemPlatter = inSystemPlatter
            button.frame = slots[idx]
        }
        updateDividers(animated: false)
    }

    /// Bounds and centre, not a frame: the chips may be mid-scale as they come and go.
    private func place(_ view: UIView, _ rect: CGRect) {
        view.bounds = CGRect(origin: .zero, size: rect.size)
        view.center = CGPoint(x: rect.midX, y: rect.midY)
    }

    override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
        if pasteControl.touchArea != nil, let barTouchFrame, barTouchFrame.contains(point) { return true }
        return super.point(inside: point, with: event)
    }

    private var currentSlotFrames: [CGRect] {
        if let slotFrames, slotFrames.count == 3 { return slotFrames }
        let slotW = bounds.width / 3
        return (0..<3).map { CGRect(x: CGFloat($0) * slotW, y: 0, width: slotW, height: bounds.height) }
    }

    /// Apple's dividers: 1 pt wide, ending on the slot boundary, centred on the words' line, no
    /// taller than a chip (24 pt where there's room), and only between two filled slots (nothing
    /// to show, no dividers, as Apple's bar in a field it offers nothing for). Apple's bar hides
    /// both beside its bubble, and the one beside a revert. The last stands beside the Hex key.
    private var dividerFrames: [CGRect?] {
        let none: [CGRect?] = [nil, nil, nil]
        guard !showsUndo, !showsClipboard, !showsFullAccessHint, notice == nil, statusText == nil || !currentSuggestions.isEmpty,
              bounds.width > 60 else { return none }
        let reverting = currentSuggestions.first?.kind == .revert
        let lineH: CGFloat = min(24, bounds.height - 2, chipHeight ?? .infinity, 2 * contentCenterY)
        let y = contentCenterY - lineH / 2
        let slots = Self.slots(for: currentSuggestions)
        let filled = Set(slots)
        let bubbled: Set<Int> = appleBar ? Set(zip(currentSuggestions, slots).filter { $0.0.willAutoReplace || $0.0.kind == .revert }.map(\.1)) : []
        var frames = none
        for (index, slot) in currentSlotFrames.prefix(2).enumerated()
        where slot.maxX < bounds.width - 1 && filled.contains(index) && filled.contains(index + 1) && !(reverting && index == 0)
            && !bubbled.contains(index) && !bubbled.contains(index + 1) {
            frames[index] = CGRect(x: slot.maxX - 1, y: y, width: 1, height: lineH)
        }
        let besideHex = hexOnLeading ? 0 : 2
        if dividerBesideHex, filled.contains(besideHex), !bubbled.contains(besideHex) {
            frames[2] = CGRect(x: hexOnLeading ? 0 : bounds.width - 1, y: y, width: 1, height: lineH)
        }
        return frames
    }

    /// Each divider fades in and out with the words beside it (at once at first paint).
    private func updateDividers(animated: Bool) {
        let frames = dividerFrames
        let fades = animated && animates
        for (layer, frame) in zip(dividers, frames) {
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            if let frame { layer.frame = frame }
            layer.backgroundColor = palette.stripDivider.cgColor
            CATransaction.commit()
            let target: Float = frame == nil ? 0 : 1
            guard layer.opacity != target else { continue }
            if fades {
                let fade = CABasicAnimation(keyPath: "opacity")
                fade.fromValue = (layer.presentation() ?? layer).opacity
                fade.toValue = target
                fade.duration = StripMotion.reduced ? StripMotion.reducedFade : (target == 1 ? SlotButton.letterIn : SlotButton.letterOut)
                fade.timingFunction = CAMediaTimingFunction(name: .easeOut)
                layer.add(fade, forKey: "opacity")
            }
            CATransaction.begin()
            CATransaction.setDisableActions(true)
            layer.opacity = target
            CATransaction.commit()
        }
    }

    /// What the bar shows this frame, for `--motion-trace`.
    func debugMotionSample() -> [String: Double] {
        func alpha(_ view: UIView) -> Double { view.isHidden ? 0 : Double((view.layer.presentation() ?? view.layer).opacity) }
        var s: [String: Double] = [
            "undo": alpha(undoChip),
            "undoScale": Double((undoChip.layer.presentation() ?? undoChip.layer).transform.m11),
        ]
        for (i, button) in slotButtons.enumerated() {
            let (progress, x) = button.debugSample()
            s["slot\(i)"] = progress
            s["slot\(i)X"] = x
        }
        return s
    }

    @objc private func handleUndo() {
        onUndoDictation?()
    }

    @objc private func handleSlotTap(_ sender: SlotButton) {
        guard let item = Self.slots(for: currentSuggestions).firstIndex(of: sender.tag) else { return }
        onSelectSuggestion?(currentSuggestions[item])
    }

    @objc private func handleSlotHold(_ recognizer: UILongPressGestureRecognizer) {
        guard recognizer.state == .began, let slot = recognizer.view?.tag,
              let item = Self.slots(for: currentSuggestions).firstIndex(of: slot) else { return }
        onHoldSuggestion?(currentSuggestions[item])
    }

    /// A chip whose touch can reach past its frame (`touchArea`, in the strip's coordinates): it
    /// lights wherever there the finger lands, follows it in and out, and acts only if it lifts
    /// inside, so a finger slid off the bar cancels. Without one, a plain button.
    private final class StripChip: UIButton {
        var touchArea: CGRect?
        private var trackedTouch: UITouch?

        private func inArea(_ touch: UITouch) -> Bool {
            guard let touchArea, let superview else { return false }
            return touchArea.contains(touch.location(in: superview))
        }

        override func point(inside point: CGPoint, with event: UIEvent?) -> Bool {
            guard let touchArea, let superview else { return super.point(inside: point, with: event) }
            return touchArea.contains(convert(point, to: superview))
        }

        override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
            guard touchArea != nil, trackedTouch == nil, let touch = touches.first else {
                super.touchesBegan(touches, with: event)
                return
            }
            trackedTouch = touch
            isHighlighted = true
            sendActions(for: .touchDown)
        }

        override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
            guard let touch = trackedTouch, touches.contains(touch) else {
                super.touchesMoved(touches, with: event)
                return
            }
            let inside = inArea(touch)
            guard inside != isHighlighted else { return }
            isHighlighted = inside
            sendActions(for: inside ? .touchDragEnter : .touchDragExit)
        }

        override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
            guard let touch = trackedTouch, touches.contains(touch) else {
                super.touchesEnded(touches, with: event)
                return
            }
            trackedTouch = nil
            let inside = inArea(touch)
            isHighlighted = false
            sendActions(for: inside ? .touchUpInside : .touchUpOutside)
        }

        override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
            guard let touch = trackedTouch, touches.contains(touch) else {
                super.touchesCancelled(touches, with: event)
                return
            }
            trackedTouch = nil
            isHighlighted = false
            sendActions(for: .touchCancel)
        }
    }

    /// One suggestion slot. A word that changes rolls: the letters both words start with stay
    /// and slide with the word as it recentres (width leads), the old word's other letters rise
    /// and fade from its end, and the new word's rise in from below from its start, staggered.
    /// A word replaced outright leaves where it stood as the new one rolls in, on the same frame.
    /// A new word mid-roll takes over from wherever every letter is. The bubble fades in and out
    /// (a press lights it at once). Reduce Motion crossfades whole words in place.
    private final class SlotButton: UIControl {
        var inSystemPlatter = true { didSet { if inSystemPlatter != oldValue { setNeedsDisplay() } } }
        var pillInset: CGFloat = 0.5 { didSet { if pillInset != oldValue { setNeedsDisplay() } } }
        var textCenterY: CGFloat? { didSet { if textCenterY != oldValue { settle() } } }
        var chipHeight: CGFloat? { didSet { if chipHeight != oldValue { setNeedsDisplay() } } }
        /// Apple's bar: the bubble fills the slot across, from `pillInset` down `chipHeight`.
        var fillsCell = false { didSet { if fillsCell != oldValue { setNeedsDisplay() } } }
        private var suggestion: KeyboardSuggestion?
        private var palette = FullKeyboardPalette(isDark: false)
        private var glyphs: [Glyph] = []
        private var wordX = Glide.at(0)
        private var pill = Tween.at(0)
        /// Where the bubble was last drawn, so it can fade out after its word has gone.
        private var lastPill: (rect: CGRect, arrowAt: CGPoint?)?
        private var settledSize: CGSize = .zero
        private var displayLink: CADisplayLink?

        /// Letters arriving and leaving (a fifth quicker), the most one letter waits on the one
        /// before, how far they roll, and the bubble's fades.
        static let letterIn: CFTimeInterval = 0.25
        static let letterOut: CFTimeInterval = 0.2
        static let maxStagger: CFTimeInterval = 0.025
        static let staggerSpan: CFTimeInterval = 0.08
        static let rise: CGFloat = 6
        static let pillIn: CFTimeInterval = 0.15
        static let pillOut: CFTimeInterval = 0.12

        override init(frame: CGRect) {
            super.init(frame: frame)
            isOpaque = false
            backgroundColor = .clear
            contentMode = .redraw
            isAccessibilityElement = false
            isUserInteractionEnabled = false
            accessibilityTraits = .button
            addTarget(self, action: #selector(redraw), for: [.touchDown, .touchDragEnter, .touchDragExit, .touchCancel, .touchUpInside, .touchUpOutside])
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError() }

        override var isHighlighted: Bool { didSet { if isHighlighted != oldValue { setNeedsDisplay() } } }

        /// Scene time, stretched by `--motion-slow`.
        private var clock: CFTimeInterval { CACurrentMediaTime() / MotionDebug.slow }

        /// nil empties the slot (its word rolls out when animated).
        func configure(with suggestion: KeyboardSuggestion?, palette: FullKeyboardPalette, animated: Bool) {
            let old = self.suggestion
            self.suggestion = suggestion
            self.palette = palette
            isAccessibilityElement = suggestion != nil
            isUserInteractionEnabled = suggestion != nil
            accessibilityLabel = suggestion?.displayText
            accessibilityHint = suggestion?.kind == .revert ? String(localized: "Undoes the autocorrection") : nil
            defer { setNeedsDisplay() }
            guard animated, window != nil else { return settle() }
            let t = clock
            let reduced = StripMotion.reduced
            let bubbled = suggestion.map(Self.bubbled) ?? false
            if bubbled != (old.map(Self.bubbled) ?? false) || (suggestion == nil && old != nil) {
                pill.retarget(to: bubbled ? 1 : 0, at: t, length: bubbled ? Self.pillIn : Self.pillOut)
            }
            guard old?.displayText != suggestion?.displayText || old?.kind == .emoji && suggestion?.kind != .emoji else { return startTicking() }
            let target = suggestion.map(layout)
            let current = glyphs.filter { !$0.leaving }
            // Letters both words start with stay, unless a bubble came or went (its ink changes).
            var shared = 0
            if !reduced, let target, target.box == nil, current.allSatisfy({ $0.box == nil }), current.first?.font == target.font {
                let new = Array(target.text).map(String.init)
                while shared < min(current.count, new.count), current[shared].text == new[shared] { shared += 1 }
            }
            let x = wordX.value(t)
            if shared == 0 {
                // Replaced outright: everything shown leaves where it stands.
                for i in glyphs.indices {
                    if glyphs[i].fixedX == nil { glyphs[i].fixedX = x + glyphs[i].offset }
                }
                wordX = .at(target?.origin.x ?? x)
            } else if let target {
                wordX.retarget(to: target.origin.x, at: t)
            }
            let leavingIDs = Set(current.dropFirst(shared).map(\.id))
            let leaveCount = leavingIDs.count
            let leaveStagger = reduced ? 0 : Self.stagger(leaveCount)
            var order = 0
            for i in glyphs.indices.reversed() where leavingIDs.contains(glyphs[i].id) {
                glyphs[i].leaving = true
                glyphs[i].tween.retarget(
                    to: 0, y: reduced ? 0 : -Self.rise, at: t,
                    delay: Double(order) * leaveStagger, length: reduced ? StripMotion.reducedFade : Self.letterOut
                )
                order += 1
            }
            if let target {
                let entering = target.glyphs.dropFirst(shared)
                let stagger = reduced ? 0 : Self.stagger(entering.count)
                for (n, var glyph) in entering.enumerated() {
                    glyph.tween = Tween(
                        a0: 0, a1: 1, y0: reduced ? 0 : Self.rise, y1: 0,
                        start: t + Double(n) * stagger, length: reduced ? StripMotion.reducedFade : Self.letterIn
                    )
                    glyphs.append(glyph)
                }
            }
            startTicking()
        }

        private static func bubbled(_ suggestion: KeyboardSuggestion) -> Bool {
            suggestion.willAutoReplace || suggestion.kind == .revert
        }

        private static func stagger(_ count: Int) -> CFTimeInterval {
            count > 1 ? min(maxStagger, staggerSpan / Double(count - 1)) : 0
        }

        /// Shows the word as it is, with nothing in flight (first paint, layout, Reduce Motion off-window).
        private func settle() {
            let target = suggestion.map(layout)
            glyphs = target?.glyphs ?? []
            wordX = .at(target?.origin.x ?? 0)
            pill = .at(suggestion.map(Self.bubbled) == true ? 1 : 0)
            settledSize = bounds.size
            stopTicking()
            setNeedsDisplay()
        }

        override func layoutSubviews() {
            super.layoutSubviews()
            if bounds.size != settledSize { settle() }
        }

        override func didMoveToWindow() {
            super.didMoveToWindow()
            if window == nil { settle() }
        }

        private func startTicking() {
            guard displayLink == nil else { return }
            let link = CADisplayLink(target: self, selector: #selector(tick))
            link.add(to: .main, forMode: .common)
            displayLink = link
        }

        private func stopTicking() {
            displayLink?.invalidate()
            displayLink = nil
        }

        @objc private func tick() {
            let t = clock
            glyphs.removeAll { $0.leaving && $0.tween.done(t) }
            if glyphs.allSatisfy({ $0.tween.done(t) }), wordX.settled(t), pill.done(t) {
                wordX = .at(wordX.target)
                stopTicking()
            }
            setNeedsDisplay()
        }

        @objc private func redraw() { setNeedsDisplay() }

        /// For `--motion-trace`: how settled the slot is (1 at rest) and where its word starts.
        func debugSample() -> (Double, Double) {
            let t = clock
            let busy = glyphs.contains { !$0.tween.done(t) } || !wordX.settled(t) || !pill.done(t)
            let x = suggestion == nil ? -1 : Double(wordX.value(t) + frame.minX)
            return (busy ? 0 : 1, x)
        }

        private struct Glyph {
            let id = UUID()
            let text: String
            let font: UIFont
            /// From the word's start (riding with it as it slides).
            let offset: CGFloat
            let y: CGFloat
            /// Too long for the slot: the whole word in a box with a truncating tail.
            let box: CGSize?
            /// A replaced word's letters stay where they were as they leave.
            var fixedX: CGFloat?
            var leaving = false
            var tween = Tween.at(1)
        }

        private struct Word {
            let text: String
            let font: UIFont
            let origin: CGPoint
            let width: CGFloat
            let box: CGSize?
            let glyphs: [Glyph]
        }

        private func layout(_ suggestion: KeyboardSuggestion) -> Word {
            let midY = textCenterY ?? bounds.height * 0.5
            // Apple's words are all SF 17 pt regular, the one in the capsule included.
            let font = UIFont.systemFont(ofSize: suggestion.kind == .emoji ? 22 : 17, weight: .regular)
            // The word centres in the slot; a revert's with its arrow after it.
            let arrowRoom = suggestion.kind == .revert ? Self.revertArrowSize + Self.revertArrowGap : 0
            let textMidX = (bounds.width - arrowRoom) / 2
            let maxTextW = max(20, bounds.width - 12 - arrowRoom)
            let text = suggestion.displayText
            let size = (text as NSString).size(withAttributes: [.font: font])
            guard size.width > maxTextW else {
                let origin = CGPoint(x: textMidX - size.width / 2, y: midY - size.height / 2)
                var glyphs: [Glyph] = []
                var prefix = ""
                for character in text {
                    let offset = (prefix as NSString).size(withAttributes: [.font: font]).width
                    glyphs.append(Glyph(text: String(character), font: font, offset: offset, y: origin.y, box: nil))
                    prefix.append(character)
                }
                return Word(text: text, font: font, origin: origin, width: size.width, box: nil, glyphs: glyphs)
            }
            let small = UIFont.systemFont(ofSize: 15, weight: .regular)
            let smallSize = (text as NSString).size(withAttributes: [.font: small])
            let width = min(maxTextW, smallSize.width)
            let origin = CGPoint(x: textMidX - width / 2, y: midY - smallSize.height / 2)
            let box = CGSize(width: width, height: smallSize.height)
            let glyph = Glyph(text: text, font: small, offset: 0, y: origin.y, box: box)
            return Word(text: text, font: small, origin: origin, width: width, box: box, glyphs: [glyph])
        }

        static let chipPadding: CGFloat = 12
        static let revertArrowSize: CGFloat = 13
        static let revertArrowGap: CGFloat = 5

        override func draw(_ rect: CGRect) {
            let t = clock
            let bubble = isHighlighted && suggestion != nil ? 1 : pill.value(t).alpha
            let ink = Self.mix(palette.stripText, palette.stripPillText, bubble)
            let x = wordX.value(t)
            if let suggestion, Self.bubbled(suggestion) || isHighlighted {
                // A chip around the word (and a revert's arrow), even above and below the words'
                // line: Apple's slot-wide 38 pt pill reaches into the band iOS draws over a
                // third-party keyboard, and cut short there it read as squashed.
                let word = layout(suggestion)
                let arrowRoom = suggestion.kind == .revert ? Self.revertArrowSize + Self.revertArrowGap : 0
                let chipH = chipHeight ?? max(22, bounds.height - 2 * pillInset)
                let chipW = min(bounds.width, word.width + arrowRoom + 2 * Self.chipPadding)
                let midY = textCenterY ?? bounds.height * 0.5
                // Apple's bubble (iOS 26.5 and 27.2): the whole cell across, a capsule on the line.
                let chip = fillsCell ? CGRect(x: 0, y: midY - chipH / 2, width: bounds.width, height: chipH) : CGRect(
                    x: max(0, min(bounds.width - chipW, x - (chipW - word.width - arrowRoom) / 2)),
                    y: midY - chipH / 2,
                    width: chipW,
                    height: chipH
                )
                let arrowAt = suggestion.kind == .revert ? CGPoint(x: x + word.width + Self.revertArrowGap, y: midY) : nil
                lastPill = (chip, arrowAt)
            }
            if bubble > 0.001, let (chip, arrowAt) = lastPill {
                let fill = isHighlighted ? palette.stripPillPressedFill : palette.stripPillFill
                fill.withAlphaComponent(fill.cgColor.alpha * bubble).setFill()
                UIBezierPath(roundedRect: chip, cornerRadius: chip.height / 2).fill()
                if let arrowAt { drawRevertArrow(at: arrowAt, ink: ink.withAlphaComponent(ink.cgColor.alpha * bubble)) }
            }
            for glyph in glyphs {
                let (a, dy) = glyph.tween.value(t)
                guard a > 0.001 else { continue }
                let attributes: [NSAttributedString.Key: Any] = [
                    .font: glyph.font, .foregroundColor: ink.withAlphaComponent(ink.cgColor.alpha * a),
                ]
                let gx = glyph.fixedX ?? (x + glyph.offset)
                if let box = glyph.box {
                    let style = NSMutableParagraphStyle()
                    style.lineBreakMode = .byTruncatingTail
                    style.alignment = .center
                    var boxed = attributes
                    boxed[.paragraphStyle] = style
                    (glyph.text as NSString).draw(in: CGRect(x: gx, y: glyph.y + dy, width: box.width, height: box.height), withAttributes: boxed)
                } else {
                    (glyph.text as NSString).draw(at: CGPoint(x: gx, y: glyph.y + dy), withAttributes: attributes)
                }
            }
        }

        /// Apple's revert bubble's undo arrow, after the word.
        private func drawRevertArrow(at point: CGPoint, ink: UIColor) {
            let config = UIImage.SymbolConfiguration(pointSize: Self.revertArrowSize, weight: .medium)
            guard let arrow = UIImage(systemName: "arrow.uturn.backward", withConfiguration: config)?
                .withTintColor(ink, renderingMode: .alwaysOriginal) else { return }
            arrow.draw(at: CGPoint(x: point.x, y: point.y - arrow.size.height / 2))
        }

        private static func mix(_ a: UIColor, _ b: UIColor, _ f: CGFloat) -> UIColor {
            if f <= 0 { return a }
            if f >= 1 { return b }
            var (r1, g1, b1, a1): (CGFloat, CGFloat, CGFloat, CGFloat) = (0, 0, 0, 0)
            var (r2, g2, b2, a2): (CGFloat, CGFloat, CGFloat, CGFloat) = (0, 0, 0, 0)
            a.getRed(&r1, green: &g1, blue: &b1, alpha: &a1)
            b.getRed(&r2, green: &g2, blue: &b2, alpha: &a2)
            return UIColor(red: r1 + (r2 - r1) * f, green: g1 + (g2 - g1) * f, blue: b1 + (b2 - b1) * f, alpha: a1 + (a2 - a1) * f)
        }
    }
}

/// A zero-bounce spring's curve (Kit's motion tokens), from one value to another, that can start
/// late and be retargeted from wherever it is.
private struct Tween {
    var a0: CGFloat
    var a1: CGFloat
    var y0: CGFloat = 0
    var y1: CGFloat = 0
    var start: CFTimeInterval = 0
    var length: CFTimeInterval = 0

    static func at(_ alpha: CGFloat) -> Tween { Tween(a0: alpha, a1: alpha) }

    private static let tail = 1 - (1 + 2 * CGFloat.pi) * exp(-2 * CGFloat.pi)

    private func progress(_ t: CFTimeInterval) -> CGFloat {
        guard length > 0 else { return 1 }
        let x = CGFloat((t - start) / length)
        if x <= 0 { return 0 }
        if x >= 1 { return 1 }
        let w = 2 * CGFloat.pi * x
        return (1 - (1 + w) * exp(-w)) / Self.tail
    }

    func value(_ t: CFTimeInterval) -> (alpha: CGFloat, y: CGFloat) {
        let p = progress(t)
        return (a0 + (a1 - a0) * p, y0 + (y1 - y0) * p)
    }

    func done(_ t: CFTimeInterval) -> Bool { t >= start + length }

    mutating func retarget(to alpha: CGFloat, y: CGFloat = 0, at t: CFTimeInterval, delay: CFTimeInterval = 0, length: CFTimeInterval) {
        let now = value(t)
        self = Tween(a0: now.alpha, a1: alpha, y0: now.y, y1: y, start: t + delay, length: length)
    }
}

/// A critically damped spring on one coordinate (settle, 0.3 s) that keeps its velocity when
/// it is sent somewhere new.
private struct Glide {
    var target: CGFloat
    var offset: CGFloat = 0
    var velocity0: CGFloat = 0
    var start: CFTimeInterval = 0

    private static let omega = 2 * CGFloat.pi / CGFloat(StripMotion.settle)

    static func at(_ x: CGFloat) -> Glide { Glide(target: x) }

    func value(_ t: CFTimeInterval) -> CGFloat {
        let s = CGFloat(max(0, t - start))
        let w = Self.omega
        return target + (offset + (velocity0 + w * offset) * s) * exp(-w * s)
    }

    func velocity(_ t: CFTimeInterval) -> CGFloat {
        let s = CGFloat(max(0, t - start))
        let w = Self.omega
        let b = velocity0 + w * offset
        return (b - w * (offset + b * s)) * exp(-w * s)
    }

    func settled(_ t: CFTimeInterval) -> Bool { abs(value(t) - target) < 0.05 && abs(velocity(t)) < 1 }

    mutating func retarget(to x: CGFloat, at t: CFTimeInterval) {
        let p = value(t)
        let v = velocity(t)
        self = Glide(target: x, offset: p - x, velocity0: v, start: t)
    }
}

/// A two-line message with a chip that replaces the strip for a few seconds, in the Full Access
/// hint's shape: "Couldn't translate, pasted as spoken" over its fix, and Open Hex.
struct KeyboardStripNotice: Equatable {
    let title: String
    let detail: String
    let button: String
    /// An ✕ at the end away from the chip that puts the notice away for good (`onDismissNotice`).
    var dismissible = false
}

/// What the Hex key shows without Full Access: dictation needs Allow Full Access, where to
/// turn it on, and an Open Hex chip, the only part that opens Hex, on the Hex key's side.
@MainActor
final class FullAccessHintView: UIView {
    static let title = String(localized: "Allow Full Access to dictate")
    static let path = String(localized: "Settings › Apps › Hex › Keyboards")
    static let spokenPath = String(localized: "Settings, Apps, Hex, Keyboards")

    var palette = FullKeyboardPalette(isDark: false) { didSet { applyPalette() } }
    var chipHeight: CGFloat = 30 { didSet { setNeedsLayout() } }
    /// The Hex key is on the left: Open Hex sits at the leading end, the text after it.
    var chipLeads = false { didSet { if chipLeads != oldValue { setNeedsLayout() } } }
    var onOpenHex: (() -> Void)?
    var onDismiss: (() -> Void)?

    private let titleLabel = UILabel()
    private let pathLabel = UILabel()
    private let openButton = UIButton(type: .custom)
    /// A dismissible notice's ✕, drawn as Home's cards draw theirs, its touch area the notice's
    /// height and `dismissWidth` wide at the end away from the chip, so a miss lands on the words.
    private let dismissButton = UIButton(type: .custom)
    private static let dismissWidth: CGFloat = 34

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        titleLabel.text = Self.title
        titleLabel.font = .systemFont(ofSize: 13, weight: .medium)
        pathLabel.text = Self.path
        pathLabel.font = .systemFont(ofSize: 11.5)
        pathLabel.accessibilityLabel = Self.spokenPath
        for label in [titleLabel, pathLabel] {
            label.adjustsFontSizeToFitWidth = true
            label.minimumScaleFactor = 0.8
            label.lineBreakMode = .byTruncatingTail
            addSubview(label)
        }
        var open = UIButton.Configuration.plain()
        open.title = String(localized: "Open Hex")
        open.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 13, bottom: 0, trailing: 13)
        open.background.cornerRadius = 15
        openButton.configuration = open
        openButton.accessibilityHint = String(localized: "Opens Hex's setup")
        openButton.addTarget(self, action: #selector(handleOpen), for: .touchUpInside)
        addSubview(openButton)
        dismissButton.setImage(UIImage(systemName: "xmark", withConfiguration: UIImage.SymbolConfiguration(pointSize: 11, weight: .semibold)), for: .normal)
        dismissButton.accessibilityLabel = String(localized: "Dismiss")
        dismissButton.isHidden = true
        dismissButton.addTarget(self, action: #selector(handleDismiss), for: .touchUpInside)
        addSubview(dismissButton)
        applyPalette()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    @objc private func handleOpen() {
        onOpenHex?()
    }

    @objc private func handleDismiss() {
        onDismiss?()
    }

    /// Shows `notice` in place of the Full Access words.
    func show(_ notice: KeyboardStripNotice) {
        titleLabel.text = notice.title
        pathLabel.text = notice.detail
        pathLabel.accessibilityLabel = nil
        var open = openButton.configuration
        open?.title = notice.button
        openButton.configuration = open
        openButton.accessibilityHint = nil
        dismissButton.isHidden = !notice.dismissible
        setNeedsLayout()
    }

    private func applyPalette() {
        titleLabel.textColor = palette.textBase
        pathLabel.textColor = palette.textMuted
        var open = openButton.configuration
        open?.baseForegroundColor = palette.textBase
        open?.background.backgroundColor = palette.chipFill
        openButton.configuration = open
        dismissButton.tintColor = palette.textMuted
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let buttonWidth = openButton.sizeThatFits(CGSize(width: bounds.width, height: chipHeight)).width
        openButton.frame = CGRect(
            x: chipLeads ? 0 : bounds.width - buttonWidth,
            y: (bounds.height - chipHeight) / 2,
            width: buttonWidth,
            height: chipHeight
        )
        let dismiss = dismissButton.isHidden ? 0 : Self.dismissWidth
        dismissButton.frame = CGRect(x: chipLeads ? bounds.width - dismiss : 0, y: 0, width: dismiss, height: bounds.height)
        let textX = chipLeads ? openButton.frame.maxX + 10 : dismiss
        let textWidth = chipLeads ? max(0, bounds.width - textX - dismiss) : max(0, openButton.frame.minX - 10 - textX)
        // Two lines, even above and below the strip's words' line.
        let lineHeight: CGFloat = min(16, (bounds.height + 1) / 2)
        let top = (bounds.height - 2 * lineHeight) / 2
        titleLabel.frame = CGRect(x: textX, y: top, width: textWidth, height: lineHeight)
        pathLabel.frame = CGRect(x: textX, y: top + lineHeight, width: textWidth, height: lineHeight)
    }
}

/// The strip's geometry: Apple's prediction bar on an iPhone (`appleBar`), the 38 pt strip before
/// it was measured, and the Duo's and iPad's bars.
nonisolated extension KeyboardMetrics {
    /// The suggestion strip is Apple's predictive bar from the top of its suggestion pill down.
    /// Apple's bar is 44 pt from its panel's edge, the pill around a suggested word starts 6 pt
    /// down, and row 1's keycaps start about 8 pt under the bar (51.7 pt down, iPhone 17 Pro,
    /// iOS 27.2). So the strip is 38 pt, then the same 8 pt gap, and row 1 starts 46 pt down.
    /// iOS 26 and later draw about 17 pt of their own panel above a third-party keyboard in some
    /// presentations and nothing in others (Hex, Gboard and SwiftKey alike on an iPhone 17 Pro,
    /// iOS 27.2: Notes has it, a Safari form's first focus doesn't), and a keyboard can't tell
    /// which, so the strip carries its own room above the words. It was 27 pt, the part of
    /// Apple's bar below that band: without the band the words sat 9 pt under the keyboard's top
    /// edge and a chip touched it. Everything the strip shows (suggestions, Undo dictation, the
    /// Full Access hint, the recording bar, the emoji panel's top row) lives in those 38 pt,
    /// centred on the words' line (`stripTextCenter`).
    static let stripBarHeight: CGFloat = 38
    static let stripGap: CGFloat = 8
    /// On the iPhone Duo iOS sets the keyboard in a platter with its own band above, and the
    /// strip is Apple's whole 44 pt bar, right on the keys.
    static let insetStripHeight: CGFloat = 44
    /// The Duo folded and upright: iOS's band there is 8.7 pt and Apple's keycaps start 48.7 pt
    /// below the panel's edge (iOS 27.2), so the strip is 32 pt with the iPhone's 8 pt gap.
    static let coverStripHeight: CGFloat = 32

    /// How far below the strip's top its pills (the Hex key, the recording bar) start: they
    /// centre on the words' line, 6 pt down in the phone's portrait strip, in the Duo's 44 pt
    /// bar 4 pt.
    var stripInset: CGFloat { stripTextCenter - stripPillHeight / 2 }
    /// Apple's pill starts 6 pt below its panel's edge (iOS 27.2, iPhone 17 Pro); Hex's pills
    /// start as far below the keyboard's own top edge, which is the panel's edge whenever iOS
    /// draws nothing above the keyboard.
    static let barPillTop: CGFloat = 6
    /// The pills' height. In the phone's portrait strip, even above and below the words' line
    /// from `barPillTop` down (28 pt), inside the strip. Elsewhere it's the strip less half a
    /// point above and below, at most 36 pt.
    var stripPillHeight: CGFloat {
        if appleBar { return min(Self.appleBubbleHeight, 2 * min(stripTextCenter, stripHeight - stripTextCenter)) }
        guard stripHeight == Self.stripBarHeight else { return min(stripHeight - 1, 36) }
        return 2 * min(stripTextCenter - Self.barPillTop, stripHeight - stripTextCenter)
    }

    /// Where the strip's words, dividers, chips and Hex mark centre. Apple's words sit midway
    /// between the panel's edge and row 1's keycaps (25.3 pt down, keycaps at 52 pt, iPhone 17
    /// Pro, iOS 27.2), so in the phone's portrait strip they're 26 pt above row 1, 20 pt below
    /// the keyboard's top edge. Elsewhere the strip is Apple's bar and they centre in it.
    static let barTextAboveKeys: CGFloat = 26
    var stripTextCenter: CGFloat {
        if appleBar { return stripHeight - Self.appleBarTextAboveBottom }
        return stripHeight == Self.stripBarHeight ? stripHeight + stripBottom - Self.barTextAboveKeys : stripHeight / 2
    }
    /// A suggestion's chip (an autocorrection, a pressed word, a revert) and the dividers: in
    /// the phone's portrait strip a small chip around the word, 18 pt, even above and below the
    /// words' line (not Apple's slot-wide 38 pt pill). Elsewhere the pill height.
    static let barChipHeight: CGFloat = 18
    var stripChipHeight: CGFloat {
        !appleBar && stripHeight == Self.stripBarHeight ? Self.barChipHeight : stripPillHeight
    }

    /// Apple's prediction bar, measured beside Hex on iOS 26.5 and 27.2 (iPhone 17 Pro, Air, 17 Pro
    /// Max and SE, upright and turned, light and dark, black box): 44 pt from the keyboard's top
    /// edge on every iPhone, row 1's keycaps 8 pt under it upright and 6 pt turned. Its words, its
    /// dividers and the bubble behind a pending autocorrection all centre 25 pt down (19 pt above
    /// the bar's bottom); the bubble fills its cell from 6 pt down to the bar's bottom, a 38 pt
    /// capsule, and the dividers are 1 × 24 pt, hidden beside the bubble.
    static let appleBarHeight: CGFloat = 44
    static let appleBarTurnedGap: CGFloat = 6
    static let appleBarTextAboveBottom: CGFloat = 19
    static let appleBubbleHeight: CGFloat = 38
    /// Apple's bar leads with a 59 pt cell for its writing tools icon, then a divider, then three
    /// equal cells (iPhone 17 Pro Max, iOS 27.2.1, a TestFlight tester's screenshot; the simulator
    /// shows no icon). Hex's key takes that cell at its own end of the bar.
    static let appleIconCellWidth: CGFloat = 59
    /// Hex's key in Apple's bar keeps the 44 × 28 pt mark it had in the 38 pt strip.
    static let barHexKeyHeight: CGFloat = 28
    /// Apple's bar was measured on iOS 26 and 27 (`KeyboardLanguage.systemMajorVersion`); iOS 18
    /// is unmeasured (no simulator runtime) and keeps the 38 pt strip it always had.
    static var appleBarIsMeasured: Bool { KeyboardLanguage.systemMajorVersion >= 26 }

    /// These metrics with Apple's prediction bar for a strip: 44 pt, then 8 pt to row 1 upright or
    /// 6 pt turned. Upright the keyboard grows 6 pt upward (rows stay put) and its top edge is
    /// Apple's; turned it was already Apple's height (42 + 8 pt), and only the strip changes.
    func withAppleBar(turned: Bool) -> KeyboardMetrics {
        var m = rebuilt(stripHeight: Self.appleBarHeight, stripBottom: turned ? Self.appleBarTurnedGap : Self.stripGap)
        m.appleBar = true
        return m
    }
    /// The tallest band of the strip that is even above and below the words' line, from the
    /// strip's top: what fills the strip (the Full Access hint, the emoji panel's top row, the
    /// emoji search field) centres in it, so it sits on the line too. The whole strip where the
    /// words centre in it.
    var stripLineBand: (top: CGFloat, height: CGFloat) {
        let half = min(stripTextCenter, stripHeight - stripTextCenter)
        return (stripTextCenter - half, 2 * half)
    }
}
