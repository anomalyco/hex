import UIKit

/// The 3-slot top bar above the keys, matching Apple's iOS 26 suggestion strip:
/// two 1 pt vertical hairlines at 1/3 and 2/3 width when idle or suggesting, replaced by
/// the centered `Undo dictation` chip or a status message when needed.
@MainActor
final class KeyboardStripView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { applyPalette() } } }
    /// Hosted extensions sit inside iOS 26's +17 pt Liquid Glass crown above `bounds.minY`,
    /// so nudge the strip's visual centre slightly upward within `bounds` to align with Apple's bar.
    var inSystemPlatter = true {
        didSet {
            guard inSystemPlatter != oldValue else { return }
            for button in slotButtons { button.inSystemPlatter = inSystemPlatter }
            setNeedsLayout()
            setNeedsDisplay()
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
    /// Record typing logs is recording: a small mark in the strip's leading corner.
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
    /// extension (TestFlight build 4), so this reads the clipboard on tap instead.
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

    /// Which slot shows each suggestion: in order, except a lone emoji, which takes the centre.
    static func slots(for suggestions: [KeyboardSuggestion]) -> [Int] {
        if suggestions.count == 1, suggestions[0].kind == .emoji { return [1] }
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
        let chipH = min(34, max(24, bounds.height - 10))
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
            // Above the first slot's text, and in from the keyboard's rounded top corner.
            let size = loggingMark.sizeThatFits(bounds.size)
            loggingMark.frame = CGRect(x: 18, y: 2, width: size.width, height: size.height)
            bringSubviewToFront(loggingMark)
            hintFrame = CGRect(x: loggingMark.frame.maxX + 8, y: 0, width: max(0, bounds.width - loggingMark.frame.maxX - 20), height: bounds.height)
        }
        fullAccessHintView?.frame = hintFrame
        fullAccessHintView?.chipHeight = chipH
        let slotW = bounds.width / 3
        for (idx, button) in slotButtons.enumerated() {
            button.inSystemPlatter = inSystemPlatter
            button.frame = CGRect(x: CGFloat(idx) * slotW, y: 0, width: slotW, height: bounds.height)
        }
    }

    override func draw(_ rect: CGRect) {
        guard !showsUndo, !showsClipboard, !showsFullAccessHint, statusLabel.isHidden, bounds.width > 60 else { return }
        // Hide the divider adjacent to a highlighted primary pill, matching Apple's strip.
        let primaryHighlighted = currentSuggestions.count > 1 && currentSuggestions[1].willAutoReplace
        guard !primaryHighlighted else { return }

        let scale = max(1, traitCollection.displayScale)
        let hairline = 1 / scale
        let lineH: CGFloat = min(22, bounds.height * 0.66)
        let y = max(1, contentCenterY - lineH / 2)
        let slotW = bounds.width / 3

        palette.separator.setFill()
        UIRectFill(CGRect(x: (slotW * 1).rounded() - hairline / 2, y: y, width: hairline, height: lineH))
        UIRectFill(CGRect(x: (slotW * 2).rounded() - hairline / 2, y: y, width: hairline, height: lineH))
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
            // Like Apple's strip, the highlight fills its slot, 4 pt in from each edge.
            let pillH = max(22, bounds.height - 8)
            let pillInset = CGRect(
                x: 4,
                y: max(0.5, midY - pillH / 2),
                width: max(0, bounds.width - 8),
                height: pillH
            )
            if isHighlighted {
                palette.keyPressedFill.setFill()
                UIBezierPath(roundedRect: pillInset, cornerRadius: 10).fill()
            } else if suggestion.willAutoReplace {
                palette.selectFill.setFill()
                UIBezierPath(roundedRect: pillInset, cornerRadius: 10).fill()
            }

            let isLiteral = suggestion.kind == .keepLiteral
            let font = UIFont.systemFont(
                ofSize: suggestion.kind == .emoji ? 24 : 17,
                weight: suggestion.willAutoReplace ? .medium : .regular
            )
            let ink = isLiteral && !suggestion.willAutoReplace ? palette.textBase.withAlphaComponent(0.88) : palette.textBase
            let attributes: [NSAttributedString.Key: Any] = [
                .font: font,
                .foregroundColor: ink,
            ]
            let maxTextW = max(20, pillInset.width - 12)
            var drawString = suggestion.displayText
            var size = (drawString as NSString).size(withAttributes: attributes)
            if size.width > maxTextW {
                let smallAttrs: [NSAttributedString.Key: Any] = [
                    .font: UIFont.systemFont(ofSize: 15, weight: suggestion.willAutoReplace ? .medium : .regular),
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
/// turn it on, and an Open Hex chip, the only part that opens Hex.
@MainActor
final class FullAccessHintView: UIView {
    static let title = "Allow Full Access to dictate"
    static let path = "Settings › Apps › Hex › Keyboards"

    var palette = FullKeyboardPalette(isDark: false) { didSet { applyPalette() } }
    var chipHeight: CGFloat = 30 { didSet { setNeedsLayout() } }
    var onOpenHex: (() -> Void)?

    private let titleLabel = UILabel()
    private let pathLabel = UILabel()
    private let openButton = UIButton(type: .custom)

    override init(frame: CGRect) {
        super.init(frame: frame)
        backgroundColor = .clear
        titleLabel.text = Self.title
        titleLabel.font = .systemFont(ofSize: 13.5, weight: .medium)
        pathLabel.text = Self.path
        pathLabel.font = .systemFont(ofSize: 12)
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
            x: bounds.width - buttonWidth,
            y: (bounds.height - chipHeight) / 2,
            width: buttonWidth,
            height: chipHeight
        )
        let textWidth = max(0, openButton.frame.minX - 10)
        let lineHeight: CGFloat = 16
        let top = (bounds.height - 2 * lineHeight - 1) / 2
        titleLabel.frame = CGRect(x: 0, y: top, width: textWidth, height: lineHeight)
        pathLabel.frame = CGRect(x: 0, y: top + lineHeight + 1, width: textWidth, height: lineHeight)
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
