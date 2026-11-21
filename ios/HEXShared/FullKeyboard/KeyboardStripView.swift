import UIKit

/// The 3-slot top bar above the keys, drawn as Apple's iOS 26 predictive bar is (measured in
/// the iOS 27.2 simulator): SF 17 pt regular in muted ink, slots a third of the keyboard wide,
/// 1 × 24 pt dividers ending on the thirds, and a pending autocorrection as a full-slot
/// capsule with the divider beside it hidden. iOS's band above the keyboard takes the top of
/// Apple's bar, so everything here is centred in the 27 pt below it. The centred
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
    /// How far the pills sit in from the strip's top and bottom (`KeyboardMetrics.stripInset`).
    var pillInset: CGFloat = 0.5 {
        didSet {
            guard pillInset != oldValue else { return }
            for button in slotButtons { button.pillInset = pillInset }
            setNeedsLayout()
        }
    }
    var onSelectSuggestion: ((KeyboardSuggestion) -> Void)?
    var onUndoDictation: (() -> Void)?
    var onPaste: (() -> Void)?
    var onNextKeyboard: (() -> Void)?
    /// The Full Access hint's Open Hex was tapped (it opens Hex's setup).
    var onOpenHexFromHint: (() -> Void)?

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
    /// The Hex key sits left of the strip (Settings › Keyboard › Hex key): the Full Access
    /// hint's Open Hex leads, beside the key, and the Logging mark moves to the far corner.
    var hexOnLeading = false {
        didSet {
            guard hexOnLeading != oldValue else { return }
            fullAccessHintView?.chipLeads = hexOnLeading
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
            button.isHidden = true
            addSubview(button)
        }
        applyPalette()
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    /// `fullAccessHint`, while true, replaces everything else in the strip with what
    /// dictation needs (`FullAccessHintView`).
    func update(
        suggestions: [KeyboardSuggestion],
        showsUndo: Bool,
        statusText: String?,
        clipboard: KeyboardClipboardOffer? = nil,
        fullAccessHint: Bool = false
    ) {
        let changed = suggestions != currentSuggestions || showsUndo != self.showsUndo
            || statusText != self.statusText || clipboard != self.clipboard
            || fullAccessHint != showsFullAccessHint
        guard changed else { return }
        currentSuggestions = suggestions
        self.showsUndo = showsUndo
        self.statusText = statusText
        self.clipboard = clipboard
        showsFullAccessHint = fullAccessHint

        if fullAccessHint {
            (fullAccessHintView ?? makeFullAccessHintView()).isHidden = false
        } else {
            fullAccessHintView?.isHidden = true
        }
        let showsHint = fullAccessHint
        undoChip.isHidden = !showsUndo || showsHint
        let showsClipboard = showsClipboard
        pasteControl.isHidden = !showsClipboard || clipboard == .code(canPaste: false)
        nextKeyboardChip.isHidden = !showsClipboard || clipboard == .paste
        if showsHint || showsUndo || showsClipboard {
            statusLabel.isHidden = true
            slotButtons.forEach { $0.isHidden = true }
        } else if let statusText, suggestions.isEmpty {
            statusLabel.text = statusText
            statusLabel.isHidden = false
            slotButtons.forEach { $0.isHidden = true }
        } else {
            statusLabel.isHidden = true
            let slots = Self.slots(for: suggestions)
            for (idx, button) in slotButtons.enumerated() {
                if let item = slots.firstIndex(of: idx) {
                    button.configure(with: suggestions[item], palette: palette)
                    button.isHidden = false
                } else {
                    button.isHidden = true
                }
            }
        }
        setNeedsLayout()
        setNeedsDisplay()
    }

    /// Which slot shows each suggestion: in order, except a lone emoji, swiped word or reading
    /// of one, which takes the centre.
    static func slots(for suggestions: [KeyboardSuggestion]) -> [Int] {
        if suggestions.count == 1, suggestions[0].kind != .keepLiteral, suggestions[0].kind != .secondary { return [1] }
        return Array(suggestions.indices.prefix(3))
    }

    /// Clipboard chips replace the empty strip; suggestions and status win.
    private var showsClipboard: Bool {
        guard let clipboard, !showsUndo, statusText == nil, !showsFullAccessHint else { return false }
        if case .code = clipboard { return true }
        return currentSuggestions.isEmpty
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
        loggingMark?.textColor = palette.textMuted
        for (item, slot) in Self.slots(for: currentSuggestions).enumerated() {
            slotButtons[slot].configure(with: currentSuggestions[item], palette: palette)
        }
        setNeedsDisplay()
    }

    private var contentCenterY: CGFloat {
        bounds.height * 0.5
    }

    override func layoutSubviews() {
        super.layoutSubviews()
        let midY = contentCenterY
        statusLabel.frame = CGRect(x: 16, y: max(0, midY - 12), width: max(0, bounds.width - 32), height: 24)
        let chipH = min(34, max(24, bounds.height - 2 * pillInset))
        let chipSize = undoChip.sizeThatFits(CGSize(width: bounds.width, height: chipH))
        undoChip.frame = CGRect(
            x: (bounds.width - chipSize.width) / 2,
            y: max(0, midY - chipH / 2),
            width: chipSize.width,
            height: chipH
        )
        let pasteW = pasteControl.sizeThatFits(CGSize(width: bounds.width, height: chipH)).width
        let nextSize = nextKeyboardChip.sizeThatFits(CGSize(width: bounds.width, height: chipH))
        let pair = !pasteControl.isHidden && !nextKeyboardChip.isHidden
        let gap: CGFloat = 8
        let total = pair ? pasteW + gap + nextSize.width : (pasteControl.isHidden ? nextSize.width : pasteW)
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
        if let loggingMark, showsLoggingMark {
            // Above the outer slot's text, and in from the keyboard's rounded top corner.
            let size = loggingMark.sizeThatFits(bounds.size)
            let markX = hexOnLeading ? bounds.width - 18 - size.width : 18
            loggingMark.frame = CGRect(x: markX, y: 2, width: size.width, height: size.height)
            bringSubviewToFront(loggingMark)
            hintFrame = hexOnLeading
                ? CGRect(x: 12, y: 0, width: max(0, loggingMark.frame.minX - 8 - 12), height: bounds.height)
                : CGRect(x: loggingMark.frame.maxX + 8, y: 0, width: max(0, bounds.width - loggingMark.frame.maxX - 20), height: bounds.height)
        }
        fullAccessHintView?.frame = hintFrame
        fullAccessHintView?.chipHeight = chipH
        let slots = currentSlotFrames
        for (idx, button) in slotButtons.enumerated() {
            button.inSystemPlatter = inSystemPlatter
            button.frame = slots[idx]
        }
    }

    private var currentSlotFrames: [CGRect] {
        if let slotFrames, slotFrames.count == 3 { return slotFrames }
        let slotW = bounds.width / 3
        return (0..<3).map { CGRect(x: CGFloat($0) * slotW, y: 0, width: slotW, height: bounds.height) }
    }

    override func draw(_ rect: CGRect) {
        guard !showsUndo, !showsClipboard, !showsFullAccessHint, statusLabel.isHidden, bounds.width > 60 else { return }
        // Hide the divider adjacent to a highlighted primary pill, matching Apple's strip.
        let primaryHighlighted = currentSuggestions.count > 1 && currentSuggestions[1].willAutoReplace
        guard !primaryHighlighted else { return }

        // Apple's dividers: 1 pt wide, ending on the slot boundary, 24 pt tall on the text's centre.
        let lineH: CGFloat = min(24, bounds.height - 2)
        let y = contentCenterY - lineH / 2
        palette.stripDivider.setFill()
        for slot in currentSlotFrames.prefix(2) where slot.maxX < bounds.width - 1 {
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

    private final class SlotButton: UIControl {
        var inSystemPlatter = true { didSet { if inSystemPlatter != oldValue { setNeedsDisplay() } } }
        var pillInset: CGFloat = 0.5 { didSet { if pillInset != oldValue { setNeedsDisplay() } } }
        private var suggestion: KeyboardSuggestion?
        private var palette = FullKeyboardPalette(isDark: false)

        override init(frame: CGRect) {
            super.init(frame: frame)
            isOpaque = false
            backgroundColor = .clear
            contentMode = .redraw
            isAccessibilityElement = true
            accessibilityTraits = .button
            addTarget(self, action: #selector(redraw), for: [.touchDown, .touchDragEnter, .touchDragExit, .touchCancel, .touchUpInside, .touchUpOutside])
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) { fatalError() }

        override var isHighlighted: Bool { didSet { if isHighlighted != oldValue { setNeedsDisplay() } } }

        func configure(with suggestion: KeyboardSuggestion, palette: FullKeyboardPalette) {
            self.suggestion = suggestion
            self.palette = palette
            accessibilityLabel = suggestion.displayText
            setNeedsDisplay()
        }

        @objc private func redraw() { setNeedsDisplay() }

        override func draw(_ rect: CGRect) {
            guard let suggestion else { return }
            let midY = bounds.height * 0.5
            // Like Apple's, the highlight is a capsule as wide as its slot.
            let pillH = max(22, bounds.height - 2 * pillInset)
            let pillInset = CGRect(
                x: 0,
                y: max(0.5, midY - pillH / 2),
                width: bounds.width,
                height: pillH
            )
            let pilled = isHighlighted || suggestion.willAutoReplace
            if pilled {
                (isHighlighted ? palette.stripPillPressedFill : palette.stripPillFill).setFill()
                UIBezierPath(roundedRect: pillInset, cornerRadius: pillH / 2).fill()
            }

            // Apple's words are all SF 17 pt regular, the one in the pill included.
            let font = UIFont.systemFont(ofSize: suggestion.kind == .emoji ? 22 : 17, weight: .regular)
            let ink = pilled ? palette.stripPillText : palette.stripText
            let attributes: [NSAttributedString.Key: Any] = [
                .font: font,
                .foregroundColor: ink,
            ]
            let maxTextW = max(20, pillInset.width - 12)
            var drawString = suggestion.displayText
            var size = (drawString as NSString).size(withAttributes: attributes)
            if size.width > maxTextW {
                let smallAttrs: [NSAttributedString.Key: Any] = [
                    .font: UIFont.systemFont(ofSize: 15, weight: .regular),
                    .foregroundColor: ink,
                ]
                size = (drawString as NSString).size(withAttributes: smallAttrs)
                let drawRect = CGRect(
                    x: bounds.midX - min(maxTextW, size.width) / 2,
                    y: midY - size.height / 2,
                    width: min(maxTextW, size.width),
                    height: size.height
                )
                let style = NSMutableParagraphStyle()
                style.lineBreakMode = .byTruncatingTail
                style.alignment = .center
                var truncatedAttrs = smallAttrs
                truncatedAttrs[.paragraphStyle] = style
                (drawString as NSString).draw(in: drawRect, withAttributes: truncatedAttrs)
                return
            }
            drawString = suggestion.displayText
            (drawString as NSString).draw(
                at: CGPoint(x: bounds.midX - size.width / 2, y: midY - size.height / 2),
                withAttributes: attributes
            )
        }
    }
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
