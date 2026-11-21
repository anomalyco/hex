import UIKit

/// The 3-slot top bar above the keys, drawn as Apple's iOS 26 predictive bar is (measured in
/// the iOS 27.2 simulator): SF 17 pt regular in muted ink, slots a third of the keyboard wide,
/// 1 × 24 pt dividers ending on the thirds, and a pending autocorrection as a full-slot
/// capsule with the divider beside it hidden. iOS's band above the keyboard takes the top of
/// Apple's bar, so the pills fill the 27 pt below it and the words and dividers centre in it
/// (`KeyboardMetrics.stripTextCenter`). The centred
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
            setNeedsDisplay()
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
            setNeedsDisplay()
        }
    }
    /// A suggestion's chip and the dividers' height (`KeyboardMetrics.stripChipHeight`).
    var chipHeight: CGFloat? {
        didSet {
            guard chipHeight != oldValue else { return }
            for button in slotButtons { button.chipHeight = chipHeight }
            setNeedsLayout()
            setNeedsDisplay()
        }
    }
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
    /// The Full Access hint's Open Hex was tapped (it opens Hex's setup).
    var onOpenHexFromHint: (() -> Void)?
    /// The notice's chip was tapped (`KeyboardStripNotice`).
    var onOpenNotice: (() -> Void)?

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
    /// hint's Open Hex leads, beside the key, and the Logging mark moves to the far corner.
    var hexOnLeading = false {
        didSet {
            guard hexOnLeading != oldValue else { return }
            fullAccessHintView?.chipLeads = hexOnLeading
            noticeView?.chipLeads = hexOnLeading
            setNeedsLayout()
        }
    }
    /// Record typing logs is recording: a small mark in the strip's corner away from the Hex key.
    var showsLoggingMark = false {
        didSet {
            guard showsLoggingMark != oldValue else { return }
            if showsLoggingMark, loggingMark == nil {
                let mark = TypingLogMarkView()
                mark.textColor = palette.textMuted
                addSubview(mark)
                loggingMark = mark
            }
            loggingMark?.isHidden = !showsLoggingMark
            setNeedsLayout()
        }
    }
    private var loggingMark: TypingLogMarkView?
    /// A plain chip: `UIPasteControl` renders as an empty blue box inside a keyboard
    /// extension (TestFlight build 4), so this reads the clipboard on tap instead, once, which
    /// iOS asks about until Paste from Other Apps is Allow (`KeyboardClipboardTracker`).
    private let pasteControl = UIButton(type: .custom)
    private let nextKeyboardChip = UIButton(type: .custom)

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

        var chip = UIButton.Configuration.plain()
        chip.image = UIImage(
            systemName: "arrow.uturn.backward",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 12.5, weight: .medium)
        )
        chip.imagePadding = 6
        chip.title = "Undo dictation"
        chip.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 13, bottom: 0, trailing: 13)
        chip.background.cornerRadius = 15
        undoChip.configuration = chip
        undoChip.isHidden = true
        undoChip.addTarget(self, action: #selector(handleUndo), for: .touchUpInside)
        var paste = UIButton.Configuration.plain()
        paste.image = UIImage(
            systemName: "doc.on.clipboard",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 12.5, weight: .medium)
        )
        paste.imagePadding = 6
        paste.title = "Paste"
        paste.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 13, bottom: 0, trailing: 13)
        paste.background.cornerRadius = 15
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
        next.title = "Code from Messages"
        next.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 13, bottom: 0, trailing: 13)
        next.background.cornerRadius = 15
        nextKeyboardChip.configuration = next
        nextKeyboardChip.accessibilityHint = "Switches to the next keyboard, where iOS offers codes from Messages and Mail"
        nextKeyboardChip.isHidden = true
        nextKeyboardChip.addTarget(self, action: #selector(handleNextKeyboard), for: .touchUpInside)
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
        let pasteTitle = clipboard == .dictation ? "Paste dictation" : "Paste"
        if paste?.title != pasteTitle {
            paste?.title = pasteTitle
            pasteControl.configuration = paste
        }
        show(pasteControl, showsPasteChip)
        show(nextKeyboardChip, showsNextKeyboardChip)
        // Words swap letter by letter (and leave the same way when a chip or status takes the
        // strip); the strip being shown for the first time just draws them.
        let animated = window != nil
        if showsHint || showsUndo || showsClipboard {
            statusLabel.isHidden = true
            slotButtons.forEach { $0.configure(with: nil, palette: palette, animated: animated) }
        } else if let statusText, suggestions.isEmpty {
            statusLabel.text = statusText
            statusLabel.isHidden = false
            slotButtons.forEach { $0.configure(with: nil, palette: palette, animated: false) }
        } else {
            statusLabel.isHidden = true
            let slots = Self.slots(for: suggestions)
            for (idx, button) in slotButtons.enumerated() {
                let item = slots.firstIndex(of: idx).map { suggestions[$0] }
                button.configure(with: item, palette: palette, animated: animated)
            }
        }
        setNeedsLayout()
        setNeedsDisplay()
    }

    /// Shows or hides a chip, fading it over Apple's 0.2 s word swap (at once with Reduce Motion).
    private func show(_ view: UIView, _ shown: Bool) {
        let visible = !view.isHidden && view.alpha > 0
        guard shown != visible || (shown && view.alpha < 1) else { return }
        guard window != nil, !UIAccessibility.isReduceMotionEnabled else {
            view.layer.removeAllAnimations()
            view.alpha = 1
            view.isHidden = !shown
            return
        }
        if shown {
            if view.isHidden { view.alpha = 0 }
            view.isHidden = false
        }
        UIView.animate(withDuration: 0.2, delay: 0, options: [.beginFromCurrentState, .allowUserInteraction, .curveEaseOut]) {
            view.alpha = shown ? 1 : 0
        } completion: { _ in
            guard view.alpha == 0 else { return }
            view.isHidden = true
            view.alpha = 1
        }
    }

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
        showsClipboard && clipboard != .code(canPaste: false)
    }

    private var showsNextKeyboardChip: Bool {
        guard showsClipboard, case .code = clipboard else { return false }
        return true
    }

    @objc private func handlePaste() {
        onPaste?()
    }

    @objc private func handleNextKeyboard() {
        onNextKeyboard?()
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
        paste?.baseForegroundColor = palette.textBase
        paste?.background.backgroundColor = palette.chipFill
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
        loggingMark?.textColor = palette.textMuted
        for (item, slot) in Self.slots(for: currentSuggestions).enumerated() {
            slotButtons[slot].configure(with: currentSuggestions[item], palette: palette, animated: false)
        }
        setNeedsDisplay()
    }

    private var contentCenterY: CGFloat {
        textCenterY ?? bounds.height * 0.5
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let midY = contentCenterY
        statusLabel.frame = CGRect(x: 16, y: max(0, midY - 12), width: max(0, bounds.width - 32), height: 24)
        let chipH = min(34, max(18, min(bounds.height - 2 * pillInset, chipHeight ?? .infinity)))
        let chipSize = undoChip.sizeThatFits(CGSize(width: bounds.width, height: chipH))
        undoChip.frame = CGRect(
            x: (bounds.width - chipSize.width) / 2,
            y: max(0, midY - chipH / 2),
            width: chipSize.width,
            height: chipH
        )
        let pasteW = pasteControl.sizeThatFits(CGSize(width: bounds.width, height: chipH)).width
        let nextSize = nextKeyboardChip.sizeThatFits(CGSize(width: bounds.width, height: chipH))
        // Laid out for what is shown, not what is still fading out.
        let pasteShown = showsPasteChip
        let nextShown = showsNextKeyboardChip
        let pair = pasteShown && nextShown
        let gap: CGFloat = 8
        let total = pair ? pasteW + gap + nextSize.width : (pasteShown ? pasteW : nextSize.width)
        let startX = (bounds.width - total) / 2
        let chipY = max(0, midY - chipH / 2)
        pasteControl.frame = CGRect(x: startX, y: chipY, width: pasteW, height: chipH)
        nextKeyboardChip.frame = CGRect(
            x: pair ? startX + pasteW + gap : startX,
            y: chipY,
            width: nextSize.width,
            height: chipH
        )
        var hintFrame = bounds.insetBy(dx: 12, dy: 0)
        var markRoom: CGFloat = 0
        if let loggingMark, showsLoggingMark {
            // On the text line at the outer slot's end, in from the keyboard's rounded corner;
            // that slot centres its word in the rest, so a long word never runs under it.
            let size = loggingMark.sizeThatFits(bounds.size)
            let markX = hexOnLeading ? bounds.width - 12 - size.width : 12
            loggingMark.frame = CGRect(x: markX, y: (midY - size.height / 2).rounded(), width: size.width, height: size.height)
            markRoom = 12 + size.width + 4
            bringSubviewToFront(loggingMark)
            hintFrame = hexOnLeading
                ? CGRect(x: 12, y: 0, width: max(0, loggingMark.frame.minX - 8 - 12), height: bounds.height)
                : CGRect(x: loggingMark.frame.maxX + 8, y: 0, width: max(0, bounds.width - loggingMark.frame.maxX - 20), height: bounds.height)
        }
        fullAccessHintView?.frame = hintFrame
        fullAccessHintView?.chipHeight = chipH
        noticeView?.frame = hintFrame
        noticeView?.chipHeight = chipH
        let slots = currentSlotFrames
        for (idx, button) in slotButtons.enumerated() {
            button.inSystemPlatter = inSystemPlatter
            button.frame = slots[idx]
            button.leadingRoom = !hexOnLeading && idx == 0 ? markRoom : 0
            button.trailingRoom = hexOnLeading && idx == 2 ? markRoom : 0
        }
    }

    private var currentSlotFrames: [CGRect] {
        if let slotFrames, slotFrames.count == 3 { return slotFrames }
        let slotW = bounds.width / 3
        return (0..<3).map { CGRect(x: CGFloat($0) * slotW, y: 0, width: slotW, height: bounds.height) }
    }

    override func draw(_ rect: CGRect) {
        guard !showsUndo, !showsClipboard, !showsFullAccessHint, notice == nil, statusLabel.isHidden, bounds.width > 60 else { return }
        // A chip sits inside its slot, so the dividers stay beside it, but for the one beside a
        // revert, which Apple's bar hides.
        let reverting = currentSuggestions.first?.kind == .revert

        // Apple's dividers: 1 pt wide, ending on the slot boundary, centred on the words' line, and
        // no taller than a chip (24 pt where there's room).
        let lineH: CGFloat = min(24, bounds.height - 2, chipHeight ?? .infinity, 2 * contentCenterY)
        let y = contentCenterY - lineH / 2
        palette.stripDivider.setFill()
        // Only between two filled slots: nothing to show, no dividers (as Apple's bar in a
        // field it offers nothing for).
        let filled = Set(Self.slots(for: currentSuggestions))
        for (index, slot) in currentSlotFrames.prefix(2).enumerated()
        where slot.maxX < bounds.width - 1 && filled.contains(index) && filled.contains(index + 1) && !(reverting && index == 0) {
            UIRectFill(CGRect(x: slot.maxX - 1, y: y, width: 1, height: lineH))
        }
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

    /// One suggestion slot. A word that changes swaps letter by letter, as Apple's bar does
    /// (iOS 27.2, recorded at 60 fps): the letters both words share stay, the old word's other
    /// letters fade from its end and the new word's from its start, each in 0.1 s, staggered
    /// over 0.2 s. The capsule and dividers change at once. Reduce Motion swaps at once.
    private final class SlotButton: UIControl {
        var inSystemPlatter = true { didSet { if inSystemPlatter != oldValue { setNeedsDisplay() } } }
        var pillInset: CGFloat = 0.5 { didSet { if pillInset != oldValue { setNeedsDisplay() } } }
        var textCenterY: CGFloat? { didSet { if textCenterY != oldValue { setNeedsDisplay() } } }
        var chipHeight: CGFloat? { didSet { if chipHeight != oldValue { setNeedsDisplay() } } }
        /// Room the Logging mark takes at the slot's outer end: the word centres in the rest.
        var leadingRoom: CGFloat = 0 { didSet { if leadingRoom != oldValue { setNeedsDisplay() } } }
        var trailingRoom: CGFloat = 0 { didSet { if trailingRoom != oldValue { setNeedsDisplay() } } }
        private var suggestion: KeyboardSuggestion?
        private var palette = FullKeyboardPalette(isDark: false)
        /// The word leaving while its letters fade, and when the swap began.
        private var outgoing: KeyboardSuggestion?
        private var swapStart: CFTimeInterval = 0
        private var displayLink: CADisplayLink?

        static let letterFade: CFTimeInterval = 0.1
        static let swapLength: CFTimeInterval = 0.2
        static let maxStagger: CFTimeInterval = 0.025

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

        /// nil empties the slot (its word fades out when animated).
        func configure(with suggestion: KeyboardSuggestion?, palette: FullKeyboardPalette, animated: Bool) {
            let old = self.suggestion
            self.suggestion = suggestion
            self.palette = palette
            isAccessibilityElement = suggestion != nil
            isUserInteractionEnabled = suggestion != nil
            accessibilityLabel = suggestion?.displayText
            accessibilityHint = suggestion?.kind == .revert ? "Undoes the autocorrection" : nil
            if old?.displayText != suggestion?.displayText {
                if animated, window != nil, !UIAccessibility.isReduceMotionEnabled {
                    outgoing = old
                    swapStart = CACurrentMediaTime()
                    startTicking()
                } else {
                    endSwap()
                }
            }
            setNeedsDisplay()
        }

        override func didMoveToWindow() {
            super.didMoveToWindow()
            if window == nil { endSwap() }
        }

        private func startTicking() {
            guard displayLink == nil else { return }
            let link = CADisplayLink(target: self, selector: #selector(tick))
            link.add(to: .main, forMode: .common)
            displayLink = link
        }

        private func endSwap() {
            outgoing = nil
            displayLink?.invalidate()
            displayLink = nil
        }

        @objc private func tick() {
            if CACurrentMediaTime() - swapStart >= Self.swapLength { endSwap() }
            setNeedsDisplay()
        }

        @objc private func redraw() { setNeedsDisplay() }

        private struct Word {
            let text: String
            let attributes: [NSAttributedString.Key: Any]
            let origin: CGPoint
            /// Drawn in a box with a truncating tail (too long for the slot): never swapped by letter.
            let truncatedRect: CGRect?
        }

        private func layout(_ suggestion: KeyboardSuggestion, ink: UIColor) -> Word {
            let midY = textCenterY ?? bounds.height * 0.5
            // Apple's words are all SF 17 pt regular, the one in the capsule included.
            let font = UIFont.systemFont(ofSize: suggestion.kind == .emoji ? 22 : 17, weight: .regular)
            let attributes: [NSAttributedString.Key: Any] = [.font: font, .foregroundColor: ink]
            // The word centres in the slot, less any room the Logging mark takes; a revert's
            // with its arrow after it.
            let arrowRoom = suggestion.kind == .revert ? Self.revertArrowSize + Self.revertArrowGap : 0
            let textMidX = (leadingRoom + bounds.width - trailingRoom - arrowRoom) / 2
            let maxTextW = max(20, min(bounds.width - 12, bounds.width - leadingRoom - trailingRoom - 8) - arrowRoom)
            let text = suggestion.displayText
            let size = (text as NSString).size(withAttributes: attributes)
            guard size.width > maxTextW else {
                let origin = CGPoint(x: textMidX - size.width / 2, y: midY - size.height / 2)
                return Word(text: text, attributes: attributes, origin: origin, truncatedRect: nil)
            }
            let style = NSMutableParagraphStyle()
            style.lineBreakMode = .byTruncatingTail
            style.alignment = .center
            let small: [NSAttributedString.Key: Any] = [
                .font: UIFont.systemFont(ofSize: 15, weight: .regular), .foregroundColor: ink, .paragraphStyle: style,
            ]
            let smallSize = (text as NSString).size(withAttributes: small)
            let width = min(maxTextW, smallSize.width)
            let box = CGRect(x: textMidX - width / 2, y: midY - smallSize.height / 2, width: width, height: smallSize.height)
            return Word(text: text, attributes: small, origin: box.origin, truncatedRect: box)
        }

        /// Draws the word with each letter's opacity from alpha (1 is the ink's own).
        private func draw(_ word: Word, alpha: (Int) -> CGFloat) {
            let ink = word.attributes[.foregroundColor] as? UIColor ?? .label
            if let box = word.truncatedRect {
                var attributes = word.attributes
                attributes[.foregroundColor] = ink.withAlphaComponent(ink.cgColor.alpha * max(0, alpha(0)))
                (word.text as NSString).draw(in: box, withAttributes: attributes)
                return
            }
            let string = NSMutableAttributedString(string: word.text, attributes: word.attributes)
            var offset = 0
            for (index, character) in word.text.enumerated() {
                let length = String(character).utf16.count
                let a = alpha(index)
                if a < 1 {
                    let faded = ink.withAlphaComponent(ink.cgColor.alpha * max(0, a))
                    string.addAttribute(.foregroundColor, value: faded, range: NSRange(location: offset, length: length))
                }
                offset += length
            }
            string.draw(at: word.origin)
        }

        private static func progress(_ t: CFTimeInterval, start: CFTimeInterval) -> CGFloat {
            let x = min(1, max(0, (t - start) / letterFade))
            return CGFloat(x * x * (3 - 2 * x))
        }

        static let chipPadding: CGFloat = 12
        static let revertArrowSize: CGFloat = 13
        static let revertArrowGap: CGFloat = 5

        /// Apple's revert bubble's undo arrow, after the word.
        private func drawRevertArrow(after word: Word, ink: UIColor) {
            let config = UIImage.SymbolConfiguration(pointSize: Self.revertArrowSize, weight: .medium)
            guard let arrow = UIImage(systemName: "arrow.uturn.backward", withConfiguration: config)?
                .withTintColor(ink, renderingMode: .alwaysOriginal) else { return }
            let textWidth = word.truncatedRect?.width ?? (word.text as NSString).size(withAttributes: word.attributes).width
            let x = word.origin.x + textWidth + Self.revertArrowGap
            arrow.draw(at: CGPoint(x: x, y: (textCenterY ?? bounds.height * 0.5) - arrow.size.height / 2))
        }

        override func draw(_ rect: CGRect) {
            let pilled = suggestion != nil && (isHighlighted || suggestion?.willAutoReplace == true || suggestion?.kind == .revert)
            let ink = pilled ? palette.stripPillText : palette.stripText
            let incoming = suggestion.map { layout($0, ink: ink) }
            if pilled, let incoming {
                // A chip around the word (and a revert's arrow), even above and below the words'
                // line: Apple's slot-wide 38 pt pill reaches into the band iOS draws over a
                // third-party keyboard, and cut short there it read as squashed.
                let textW = incoming.truncatedRect?.width ?? (incoming.text as NSString).size(withAttributes: incoming.attributes).width
                let arrowRoom = suggestion?.kind == .revert ? Self.revertArrowSize + Self.revertArrowGap : 0
                let chipH = chipHeight ?? max(22, bounds.height - 2 * pillInset)
                let chipW = min(bounds.width, textW + arrowRoom + 2 * Self.chipPadding)
                let midY = textCenterY ?? bounds.height * 0.5
                let chip = CGRect(
                    x: max(0, min(bounds.width - chipW, incoming.origin.x - (chipW - textW - arrowRoom) / 2)),
                    y: midY - chipH / 2,
                    width: chipW,
                    height: chipH
                )
                (isHighlighted ? palette.stripPillPressedFill : palette.stripPillFill).setFill()
                UIBezierPath(roundedRect: chip, cornerRadius: chipH / 2).fill()
            }
            if let incoming, suggestion?.kind == .revert { drawRevertArrow(after: incoming, ink: ink) }
            guard var leaving = outgoing.map({ layout($0, ink: ink) }) else {
                if let incoming { draw(incoming) { _ in 1 } }
                return
            }
            let t = CACurrentMediaTime() - swapStart
            let oldLetters = Array(leaving.text)
            let newLetters = Array(incoming?.text ?? "")
            let byLetter = leaving.truncatedRect == nil && incoming?.truncatedRect == nil
            // Letters both words start with stay put (drawn once, in the new word).
            var shared = 0
            if byLetter, incoming != nil {
                while shared < min(oldLetters.count, newLetters.count), oldLetters[shared] == newLetters[shared] {
                    shared += 1
                }
            }
            if shared > 0, let incoming {
                // The old ending continues from the shared letters, so it never jumps aside.
                leaving = Word(text: leaving.text, attributes: leaving.attributes, origin: incoming.origin, truncatedRect: nil)
            }
            let changing = max(oldLetters.count, newLetters.count) - shared
            let stagger = changing > 1 ? min(Self.maxStagger, (Self.swapLength - Self.letterFade) / Double(changing - 1)) : 0
            draw(leaving) { index in
                guard index >= shared else { return 0 }
                guard byLetter else { return 1 - Self.progress(t, start: 0) }
                return 1 - Self.progress(t, start: Double(oldLetters.count - 1 - index) * stagger)
            }
            if let incoming {
                draw(incoming) { index in
                    guard index >= shared else { return 1 }
                    guard byLetter else { return Self.progress(t, start: 0) }
                    return Self.progress(t, start: Double(index - shared) * stagger)
                }
            }
        }
    }
}

/// A two-line message with a chip that replaces the strip for a few seconds, in the Full Access
/// hint's shape: "Couldn't translate, pasted as spoken" over its fix, and Open Hex.
struct KeyboardStripNotice: Equatable {
    let title: String
    let detail: String
    let button: String
}

/// What the Hex key shows without Full Access: dictation needs Allow Full Access, where to
/// turn it on, and an Open Hex chip, the only part that opens Hex, on the Hex key's side.
@MainActor
final class FullAccessHintView: UIView {
    static let title = "Allow Full Access to dictate"
    static let path = "Settings › Apps › Hex › Keyboards"

    var palette = FullKeyboardPalette(isDark: false) { didSet { applyPalette() } }
    var chipHeight: CGFloat = 30 { didSet { setNeedsLayout() } }
    /// The Hex key is on the left: Open Hex sits at the leading end, the text after it.
    var chipLeads = false { didSet { if chipLeads != oldValue { setNeedsLayout() } } }
    var onOpenHex: (() -> Void)?

    private let titleLabel = UILabel()
    private let pathLabel = UILabel()
    private let openButton = UIButton(type: .custom)

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        titleLabel.text = Self.title
        titleLabel.font = .systemFont(ofSize: 13, weight: .medium)
        pathLabel.text = Self.path
        pathLabel.font = .systemFont(ofSize: 11.5)
        pathLabel.accessibilityLabel = "Settings, Apps, Hex, Keyboards"
        for label in [titleLabel, pathLabel] {
            label.adjustsFontSizeToFitWidth = true
            label.minimumScaleFactor = 0.8
            label.lineBreakMode = .byTruncatingTail
            addSubview(label)
        }
        var open = UIButton.Configuration.plain()
        open.title = "Open Hex"
        open.contentInsets = NSDirectionalEdgeInsets(top: 0, leading: 13, bottom: 0, trailing: 13)
        open.background.cornerRadius = 15
        openButton.configuration = open
        openButton.accessibilityHint = "Opens Hex's setup"
        openButton.addTarget(self, action: #selector(handleOpen), for: .touchUpInside)
        addSubview(openButton)
        applyPalette()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    @objc private func handleOpen() {
        onOpenHex?()
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
        setNeedsLayout()
    }

    private func applyPalette() {
        titleLabel.textColor = palette.textBase
        pathLabel.textColor = palette.textMuted
        var open = openButton.configuration
        open?.baseForegroundColor = palette.textBase
        open?.background.backgroundColor = palette.chipFill
        openButton.configuration = open
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
        let textX = chipLeads ? openButton.frame.maxX + 10 : 0
        let textWidth = chipLeads ? max(0, bounds.width - textX) : max(0, openButton.frame.minX - 10)
        // Two lines in the strip's 27 pt.
        let lineHeight: CGFloat = min(16, (bounds.height + 1) / 2)
        let top = (bounds.height - 2 * lineHeight) / 2
        titleLabel.frame = CGRect(x: textX, y: top, width: textWidth, height: lineHeight)
        pathLabel.frame = CGRect(x: textX, y: top + lineHeight, width: textWidth, height: lineHeight)
    }
}

/// Record typing logs is recording this field: a dot and "Logging", flat and small, in the
/// strip's corner and in the recording bar, so logging is never hidden.
@MainActor
final class TypingLogMarkView: UIView {
    static let text = "Logging"
    static let accessibilityText = "Typing logs are recording"

    /// The dot and the word share the strip's muted ink: an orange dot would read as iOS's
    /// microphone indicator.
    var textColor: UIColor = .secondaryLabel {
        didSet {
            label.textColor = textColor
            dot.backgroundColor = textColor
        }
    }

    private let dot = UIView()
    private let label = UILabel()
    private static let dotSize: CGFloat = 5
    private static let gap: CGFloat = 4

    override init(frame: CGRect) {
        super.init(frame: frame)
        isUserInteractionEnabled = false
        isAccessibilityElement = true
        accessibilityLabel = Self.accessibilityText
        dot.backgroundColor = textColor
        dot.layer.cornerRadius = Self.dotSize / 2
        label.text = Self.text
        label.font = .systemFont(ofSize: 10, weight: .medium)
        label.textColor = textColor
        addSubview(dot)
        addSubview(label)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    override func sizeThatFits(_ size: CGSize) -> CGSize {
        let text = label.sizeThatFits(size)
        return CGSize(width: Self.dotSize + Self.gap + ceil(text.width), height: max(Self.dotSize, ceil(text.height)))
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        dot.frame = CGRect(x: 0, y: (bounds.height - Self.dotSize) / 2, width: Self.dotSize, height: Self.dotSize)
        label.frame = CGRect(x: Self.dotSize + Self.gap, y: 0, width: max(0, bounds.width - Self.dotSize - Self.gap), height: bounds.height)
    }
}
