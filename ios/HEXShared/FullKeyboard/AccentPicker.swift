import UIKit

/// The options a held key offers after 400 ms: the language's letters (`KeyboardLanguage.letterHolds`),
/// else the 123 and #+= pages' list, the system English keyboard's observed long-press options
/// (iOS 27.2 simulator) unless the language's differ (`KeyboardLanguage.symbolHolds`).
nonisolated enum AccentCatalog {
    private static let symbols: [String: [String]] = [
        "0": ["0", "°"],
        "-": ["-", "–", "—", "•"],
        "/": ["/", "\\"],
        "$": ["$", "₩", "£", "¢", "€", "¥", "₽"],
        "\"": ["\"", "”", "“", "„", "»", "«"],
        ".": [".", "…"],
        "?": ["?", "¿"],
        "!": ["!", "¡"],
        "'": ["'", "’", "‘", "`"],
        "&": ["&", "§"],
        "%": ["%", "‰"],
        "=": ["=", "≠", "≈"],
    ]

    /// The options for a held key, base first; nil when it holds nothing.
    static func options(for base: String, uppercase: Bool, language: KeyboardLanguage = .english) -> [String]? {
        let key = base.lowercased()
        guard let list = language.letterHolds[key] ?? language.symbolHolds[key] ?? symbols[key],
              list.count > 1 else { return nil }
        guard uppercase else { return list }
        return list.map { option in
            // ẞ, Unicode's capital ß; İ for the dotless ı, as the system keyboard shows it in capitals.
            switch option {
            case "ß": "ẞ"
            case "ı": "İ"
            default: option.uppercased()
            }
        }
    }
}

/// Pure inward-clamped geometry for the long-press accent callout so every option fits inside
/// the extension's clipped bounds across 320–440 pt widths.
nonisolated struct AccentMenu: Equatable, Sendable {
    static let cellHeight: CGFloat = 44
    static let padding: CGFloat = 4
    static let verticalGap: CGFloat = 6

    let keyFrame: CGRect
    let bubbleFrame: CGRect
    /// Ordered left-to-right on screen.
    let options: [String]
    let cellFrames: [CGRect]
    var selectedIndex: Int

    var selectedOption: String { options[selectedIndex] }

    static func build(
        for keyFrame: CGRect,
        rawOptions: [String],
        in layoutWidth: CGFloat,
        horizontalInset: CGFloat = 0,
        cellWidth: CGFloat? = nil
    ) -> AccentMenu {
        let minX = max(2, horizontalInset + 4)
        let maxX = max(minX + 60, layoutWidth - horizontalInset - 4)
        let availableWidth = maxX - minX

        // Right-half keys reverse the list so the base letter sits under the thumb on the right
        // and alternates fan inward toward the center of the keyboard.
        let opensLeft = keyFrame.midX > layoutWidth * 0.52
        let ordered = opensLeft ? Array(rawOptions.reversed()) : rawOptions
        let defaultIndex = opensLeft ? max(0, ordered.count - 1) : 0

        let count = CGFloat(max(1, ordered.count))
        let idealCellW = cellWidth ?? min(38, max(28, keyFrame.width))
        let maxCellW = (availableWidth - 2 * padding) / count
        let cellW = min(idealCellW, maxCellW)
        let bubbleW = count * cellW + 2 * padding
        let bubbleH = cellHeight + 2 * padding

        // Anchor the default cell over the pressed key, then clamp inside [minX, maxX - bubbleW].
        let defaultCellCenterOffset = padding + (CGFloat(defaultIndex) + 0.5) * cellW
        var bubbleX = keyFrame.midX - defaultCellCenterOffset
        bubbleX = max(minX, min(maxX - bubbleW, bubbleX))
        let bubbleY = max(1, keyFrame.minY - verticalGap - bubbleH)
        let bubbleFrame = CGRect(x: bubbleX, y: bubbleY, width: bubbleW, height: bubbleH)

        let cells = (0..<ordered.count).map { idx in
            CGRect(
                x: bubbleX + padding + CGFloat(idx) * cellW,
                y: bubbleY + padding,
                width: cellW,
                height: cellHeight
            )
        }
        return AccentMenu(
            keyFrame: keyFrame,
            bubbleFrame: bubbleFrame,
            options: ordered,
            cellFrames: cells,
            selectedIndex: defaultIndex
        )
    }

    /// Maps any horizontal finger location to the nearest cell index.
    func index(for x: CGFloat) -> Int {
        guard !cellFrames.isEmpty else { return 0 }
        if x <= cellFrames[0].minX { return 0 }
        if x >= cellFrames[cellFrames.count - 1].maxX { return cellFrames.count - 1 }
        for (idx, frame) in cellFrames.enumerated() where x <= frame.maxX {
            return idx
        }
        return cellFrames.count - 1
    }
}

/// Floating callout view that renders an active `AccentMenu` over the key plane.
@MainActor
final class AccentCalloutView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { setNeedsDisplay() } } }
    private(set) var menu: AccentMenu?
    /// The options' size: 22 pt for letters; the emoji panel's skin tones are drawn larger.
    var optionFontSize: CGFloat = 22 { didSet { setNeedsDisplay() } }
    /// An option set apart from the rest by a hairline (an emoji's untoned form).
    var separatedOption: String? { didSet { setNeedsDisplay() } }

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        backgroundColor = .clear
        isUserInteractionEnabled = false
        contentMode = .redraw
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { fatalError() }

    func show(_ menu: AccentMenu?) {
        guard menu != self.menu else { return }
        self.menu = menu
        isHidden = menu == nil
        setNeedsDisplay()
    }

    override func draw(_ rect: CGRect) {
        guard let menu, let context = UIGraphicsGetCurrentContext() else { return }
        let pill = UIBezierPath(roundedRect: menu.bubbleFrame, cornerRadius: 12)

        context.saveGState()
        context.setShadow(offset: CGSize(width: 0, height: 3), blur: 10, color: palette.popupShadow.cgColor)
        context.addPath(pill.cgPath)
        context.setFillColor(palette.popupFill.cgColor)
        context.fillPath()
        context.restoreGState()

        let scale = max(1, traitCollection.displayScale)
        context.saveGState()
        context.addPath(pill.cgPath)
        context.setLineWidth(1 / scale)
        context.setStrokeColor(palette.popupBorder.cgColor)
        context.strokePath()
        context.restoreGState()

        if let separatedOption, let idx = menu.options.firstIndex(of: separatedOption), menu.options.count > 1 {
            let cell = menu.cellFrames[idx]
            let x = idx == 0 ? cell.maxX : cell.minX
            context.saveGState()
            context.setFillColor(palette.separator.cgColor)
            context.fill(CGRect(x: x - 0.5 / scale, y: cell.minY + 9, width: 1 / scale, height: cell.height - 18))
            context.restoreGState()
        }
        for (idx, option) in menu.options.enumerated() {
            let cell = menu.cellFrames[idx]
            let isSelected = idx == menu.selectedIndex
            if isSelected {
                context.saveGState()
                let highlight = UIBezierPath(roundedRect: cell.insetBy(dx: 1.5, dy: 2), cornerRadius: 8)
                context.addPath(highlight.cgPath)
                context.setFillColor(palette.actionFill.cgColor)
                context.fillPath()
                context.restoreGState()
            }
            let attributes: [NSAttributedString.Key: Any] = [
                .font: UIFont.systemFont(ofSize: optionFontSize, weight: isSelected ? .medium : .regular),
                .foregroundColor: isSelected ? UIColor.white : palette.textBase,
            ]
            let size = (option as NSString).size(withAttributes: attributes)
            (option as NSString).draw(
                at: CGPoint(x: cell.midX - size.width / 2, y: cell.midY - size.height / 2),
                withAttributes: attributes
            )
        }
    }
}
