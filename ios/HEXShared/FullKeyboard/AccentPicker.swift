import UIKit

/// The options a held key offers after 400 ms: the language's letters on its layout
/// (`KeyboardLanguage.letterHolds(on:)`),
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
    static func options(
        for base: String,
        uppercase: Bool,
        language: KeyboardLanguage = .english,
        letters: KeyboardLetterLayout? = nil
    ) -> [String]? {
        let key = base.lowercased()
        guard var list = language.letterHolds(on: language.offered(letters))[key] ?? language.symbolHolds[key] ?? symbols[key] else {
            return nil
        }
        if uppercase, language == .greek {
            list.removeAll(where: greekWithoutCapitals.contains)
        }
        guard list.count > 1 else { return nil }
        return uppercase ? list.map(upper) : list
    }

    /// Greek capitals: ς is Σ itself and ΐ ΰ have no capital of their own, so the system
    /// keyboard's held Σ offers nothing, and Ι and Υ only Ί Ϊ and Ύ Ϋ.
    private static let greekWithoutCapitals: Set<String> = ["ς", "ΐ", "ΰ"]

    // ẞ, Unicode's capital ß; İ for the dotless ı, as the system keyboard shows it in capitals.
    private static func upper(_ option: String) -> String {
        switch option {
        case "ß": "ẞ"
        case "ı": "İ"
        default: option.uppercased()
        }
    }

    /// Where the system keyboard puts each option in its callout, left to right as screenshots
    /// of every key held show them (iOS 27.2 simulator; `|` starts a second row below), for the
    /// languages checked on screen. The callout opens with the key's own under the finger, where
    /// Apple's is. Used only while it lists the same options as `options(for:)`.
    private static let englishOrder: [String: String] = [
        "e": "ë é e è ê ě ẽ ē ė ę",
        "u": "ų ů ű ũ ü ú ù u û ǔ ū",
        "i": "į ı ī ĩ ï í ì i î ǐ",
        "o": "ő ō õ ø œ ǒ ö ó ò o ô",
        "a": "æ ã å ā ă ą | a à á â ä ǎ",
        "y": "ÿ ŷ ý y",
        "w": "w ŵ",
        "r": "r ř",
        "t": "t ț ť þ",
        "s": "s ß ş ș ś š",
        "d": "d ď ð",
        "g": "g ğ ġ",
        "h": "h ħ",
        "k": "ķ k",
        "l": "ľ ļ ł l",
        "z": "z ź ž ż",
        "c": "c ç ć č ċ",
        "n": "ň ņ ń ñ n",
        "0": "° 0",
        "-": "- – — •",
        "/": "/ \\",
        "$": "₽ ¥ € $ ¢ £ ₩",
        "&": "& §",
        "\"": "« » „ “ ” \"",
        ".": ". …",
        "?": "? ¿",
        "!": "! ¡",
        "'": "` ‘ ’ '",
        "%": "% ‰",
        "=": "≈ ≠ =",
    ]

    /// The Spanish keyboard's (letters missing hold nothing there), and English's symbols.
    private static let spanishOrder: [String: String] = englishOrder.filter { $0.key.first?.isLetter != true }.merging([
        "a": "a á à ä â ã å ą æ ā ª",
        "e": "e é è ë ê ę ė ē",
        "i": "ī į î ì ï í i",
        "o": "º ō œ ø õ ô ò ó o ö",
        "u": "ū û ù ü ú u",
        "s": "s š",
        "d": "d đ",
        "l": "l·l l",
        "c": "c ç ć č",
        "n": "ń n",
        "1": "1 1º 1ª", "2": "2 2º 2ª", "3": "3 3º 3ª", "4": "4 4º 4ª", "5": "5 5º 5ª",
        "6": "6 6º 6ª", "7": "7 7º 7ª", "8": "8 8º 8ª", "9": "9ª 9 9º", "0": "0 0ª ° 0º",
    ]) { _, spanish in spanish }

    /// The Greek keyboard's: each held letter opens from the key's own on the left, and its
    /// held " differs from English's; every other symbol's is English's.
    private static let greekOrder: [String: String] = englishOrder.filter { $0.key.first?.isLetter != true }.merging([
        "ε": "ε έ", "υ": "υ ύ ϋ ΰ", "ι": "ι ί ϊ ΐ", "ο": "ο ό", "α": "α ά", "σ": "σ ς",
        "η": "η ή", "κ": "κ ϗ", "ω": "ω ώ", "\"": "„ “ ” « » \"",
    ]) { _, greek in greek }

    /// The Czech keyboard's (iOS 27.2 simulator, QWERTZ – Czech/Slovak), one row each; its
    /// symbols are English's.
    private static let czechOrder: [String: String] = englishOrder.filter { $0.key.first?.isLetter != true }.merging([
        "e": "ě e é ê è ę ë ē ė", "r": "r ř ŕ", "t": "t ť", "z": "z ž ź ż", "u": "ū ù ü û ů ú u",
        "i": "ī į ì ï î í i", "o": "ō ø œ õ ò ö ô ó o", "a": "a á à ą â ä æ ã å ā", "s": "s š ß ś",
        "d": "d ď", "l": "ĺ ľ ł l", "y": "y ý ÿ", "c": "c č ç ć", "n": "ń ñ ň n", "\u{02C7}": "\u{02C7} \u{02DA}",
    ]) { _, czech in czech }

    private static func displayOrder(for language: KeyboardLanguage) -> [String: String] {
        switch language {
        case .english: englishOrder
        case .spanish: spanishOrder
        case .greek: greekOrder
        case .czech: czechOrder
        default: [:]
        }
    }

    /// The options in the system keyboard's arrangement, when it's known for this key and
    /// language; nil opens the list from the key toward the keyboard's middle.
    static func arrangement(
        for base: String,
        uppercase: Bool,
        language: KeyboardLanguage = .english,
        letters: KeyboardLetterLayout? = nil
    ) -> AccentArrangement? {
        let key = base.lowercased()
        guard let order = displayOrder(for: language)[key],
              let options = options(for: key, uppercase: false, language: language, letters: letters) else { return nil }
        var rows = order.split(separator: "|").map { $0.split(separator: " ").map(String.init) }
        let all = rows.flatMap { $0 }
        guard all.count == options.count, Set(all) == Set(options) else { return nil }
        if uppercase, language == .greek {
            rows = rows.map { $0.filter { !greekWithoutCapitals.contains($0) } }.filter { !$0.isEmpty }
            guard rows.joined().count > 1 else { return nil }
        }
        guard let baseRow = rows.firstIndex(where: { $0.contains(key) }),
              let baseColumn = rows[baseRow].firstIndex(of: key) else { return nil }
        let shown = uppercase ? rows.map { $0.map(upper) } : rows
        return AccentArrangement(rows: shown, baseRow: baseRow, baseColumn: baseColumn)
    }
}


/// A callout's options in rows (top first), and which one is the key's own.
nonisolated struct AccentArrangement: Equatable, Sendable {
    var rows: [[String]]
    var baseRow: Int
    var baseColumn: Int
}

/// The long-press accent callout's geometry, as Apple's measures beside Hex's (iOS 27.2
/// simulators): a pill of cells 4 pt wider than the held key (36.3 pt on a 390 pt iPhone's
/// letters, 37.5 on 402, 51.7 on the 390's wide . , ? ! ' keys), 47 pt tall, 10.5 pt in from its sides and 10.35 pt from its top and
/// bottom, 3 pt above the key; the key's own option sits over the key, the pill pushed in to
/// stay 6.67 pt from the sides, cells narrowed if it can't fit. Row 1's has no room above it in
/// a third-party keyboard (Apple's rises into the app): it starts at the keyboard's top instead.
nonisolated struct AccentMenu: Equatable, Sendable {
    static let cellHeight: CGFloat = 47
    static let padding: CGFloat = 10.5
    static let verticalPadding: CGFloat = 10.35
    static let verticalGap: CGFloat = 3
    static let cellGrowth: CGFloat = 4
    static let sideInset: CGFloat = 6.67
    static let cornerRadius: CGFloat = 20.8
    static let highlightInset: CGFloat = 1.15
    static let highlightRadius: CGFloat = 6.3
    /// The skin-tone picker's geometry (`buildFixed`).
    static let fixedCellHeight: CGFloat = 44
    static let fixedPadding: CGFloat = 4
    static let fixedGap: CGFloat = 6

    let keyFrame: CGRect
    let bubbleFrame: CGRect
    /// Every option, row by row, left to right.
    let options: [String]
    let cellFrames: [CGRect]
    var selectedIndex: Int
    /// Laid out by `buildFixed`, and drawn the skin-tone picker's way.
    var fixedCells = false

    var selectedOption: String { options[selectedIndex] }

    /// A list with the key's own option first (a language's): it opens toward the middle of the
    /// keyboard, the key's own nearest the key.
    static func build(
        for keyFrame: CGRect,
        rawOptions: [String],
        in layoutWidth: CGFloat,
        horizontalInset: CGFloat = 0,
        cellWidth: CGFloat? = nil
    ) -> AccentMenu {
        if let cellWidth {
            return buildFixed(for: keyFrame, rawOptions: rawOptions, in: layoutWidth, horizontalInset: horizontalInset, cellWidth: cellWidth)
        }
        let opensLeft = keyFrame.midX > layoutWidth * 0.52
        let ordered = opensLeft ? Array(rawOptions.reversed()) : rawOptions
        return build(
            for: keyFrame,
            arrangement: AccentArrangement(rows: [ordered], baseRow: 0, baseColumn: opensLeft ? ordered.count - 1 : 0),
            in: layoutWidth,
            horizontalInset: horizontalInset
        )
    }

    /// `cellWidth` and `cellHeight` replace the held key's width and 47 pt (an iPad's, docked).
    static func build(
        for keyFrame: CGRect,
        arrangement: AccentArrangement,
        in layoutWidth: CGFloat,
        horizontalInset: CGFloat = 0,
        cellWidth: CGFloat? = nil,
        cellHeight: CGFloat = AccentMenu.cellHeight
    ) -> AccentMenu {
        let minX = horizontalInset + min(sideInset, max(1, keyFrame.minX - horizontalInset))
        let maxX = max(minX + 60, layoutWidth - horizontalInset - min(sideInset, max(1, layoutWidth - horizontalInset - keyFrame.maxX)))
        let columns = CGFloat(max(1, arrangement.rows.map(\.count).max() ?? 1))
        let rowCount = CGFloat(max(1, arrangement.rows.count))
        let fullCell = cellWidth ?? keyFrame.width + cellGrowth
        let cellW = min(fullCell, (maxX - minX - 2 * padding) / columns)
        let bubbleW = columns * cellW + 2 * padding
        let bubbleH = rowCount * cellHeight + 2 * verticalPadding

        let baseOffset = padding + (CGFloat(arrangement.baseColumn) + 0.5) * cellW
        var bubbleX = keyFrame.midX - baseOffset
        bubbleX = max(minX, min(maxX - bubbleW, bubbleX))
        let bubbleY = max(0, keyFrame.minY - verticalGap - bubbleH)
        let bubbleFrame = CGRect(x: bubbleX, y: bubbleY, width: bubbleW, height: bubbleH)

        var options: [String] = []
        var cells: [CGRect] = []
        var selected = 0
        for (r, row) in arrangement.rows.enumerated() {
            for (c, option) in row.enumerated() {
                if r == arrangement.baseRow, c == arrangement.baseColumn { selected = options.count }
                options.append(option)
                cells.append(CGRect(
                    x: bubbleX + padding + CGFloat(c) * cellW,
                    y: bubbleY + verticalPadding + CGFloat(r) * cellHeight,
                    width: cellW,
                    height: cellHeight
                ))
            }
        }
        return AccentMenu(keyFrame: keyFrame, bubbleFrame: bubbleFrame, options: options, cellFrames: cells, selectedIndex: selected)
    }

    /// Apple's docked iPad keyboard shows a held key's accents in cells about half a key wide
    /// (30 to 46.5 pt, `KeyboardMetrics.accentCellWidth`), 53 pt tall, and in two rows once there
    /// are more than five (e's nine: four over five, iPadOS 27.2). Here the row with the key's own
    /// option, nearest the key, is the bottom one and takes the larger half.
    static let padCellHeight: CGFloat = 53

    static func padArrangement(_ options: [String], opensLeft: Bool) -> AccentArrangement {
        let ordered = opensLeft ? Array(options.reversed()) : options
        guard options.count > 5 else {
            return AccentArrangement(rows: [ordered], baseRow: 0, baseColumn: opensLeft ? ordered.count - 1 : 0)
        }
        let near = (options.count + 1) / 2
        let bottom = opensLeft ? Array(ordered.suffix(near)) : Array(ordered.prefix(near))
        let top = opensLeft ? Array(ordered.prefix(options.count - near)) : Array(ordered.suffix(options.count - near))
        return AccentArrangement(rows: [top, bottom], baseRow: 1, baseColumn: opensLeft ? bottom.count - 1 : 0)
    }

    /// The emoji panel's skin-tone picker, cells of its own width: its layout as it was.
    private static func buildFixed(
        for keyFrame: CGRect,
        rawOptions: [String],
        in layoutWidth: CGFloat,
        horizontalInset: CGFloat,
        cellWidth: CGFloat
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
        let idealCellW = cellWidth
        let maxCellW = (availableWidth - 2 * fixedPadding) / count
        let cellW = min(idealCellW, maxCellW)
        let bubbleW = count * cellW + 2 * fixedPadding
        let bubbleH = fixedCellHeight + 2 * fixedPadding

        // Anchor the default cell over the pressed key, then clamp inside [minX, maxX - bubbleW].
        let defaultCellCenterOffset = fixedPadding + (CGFloat(defaultIndex) + 0.5) * cellW
        var bubbleX = keyFrame.midX - defaultCellCenterOffset
        bubbleX = max(minX, min(maxX - bubbleW, bubbleX))
        let bubbleY = max(1, keyFrame.minY - fixedGap - bubbleH)
        let bubbleFrame = CGRect(x: bubbleX, y: bubbleY, width: bubbleW, height: bubbleH)

        let cells = (0..<ordered.count).map { idx in
            CGRect(
                x: bubbleX + fixedPadding + CGFloat(idx) * cellW,
                y: bubbleY + fixedPadding,
                width: cellW,
                height: fixedCellHeight
            )
        }
        return AccentMenu(
            keyFrame: keyFrame,
            bubbleFrame: bubbleFrame,
            options: ordered,
            cellFrames: cells,
            selectedIndex: defaultIndex,
            fixedCells: true
        )
    }

    /// The cell under a finger at `point`: its row by height (above or below the callout, the
    /// nearest row), then the nearest cell in that row.
    func index(for point: CGPoint) -> Int {
        guard !cellFrames.isEmpty else { return 0 }
        let rowTops = Array(Set(cellFrames.map(\.minY))).sorted()
        var rowY = rowTops[0]
        for top in rowTops where point.y >= top { rowY = top }
        let row = cellFrames.indices.filter { cellFrames[$0].minY == rowY }
        if point.x <= cellFrames[row[0]].minX { return row[0] }
        for idx in row where point.x <= cellFrames[idx].maxX { return idx }
        return row[row.count - 1]
    }

    /// The nearest cell along a one-row callout (or the base's row) for `x`.
    func index(for x: CGFloat) -> Int {
        index(for: CGPoint(x: x, y: cellFrames.indices.contains(selectedIndex) ? cellFrames[selectedIndex].midY : 0))
    }
}

/// Floating callout view that renders an active `AccentMenu` over the key plane.
@MainActor
final class AccentCalloutView: UIView {
    var palette = FullKeyboardPalette(isDark: false) { didSet { if palette != oldValue { setNeedsDisplay() } } }
    private(set) var menu: AccentMenu?
    /// The skin-tone picker's options' size (fixed cells).
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

    /// Apple's options are 21.5 pt, selected or not.
    static let font = UIFont.systemFont(ofSize: 21.5, weight: .regular)

    override func draw(_ rect: CGRect) {
        guard let menu, let context = UIGraphicsGetCurrentContext() else { return }
        if menu.fixedCells { return drawFixed(menu, in: context) }
        let pill = UIBezierPath(roundedRect: menu.bubbleFrame, cornerRadius: AccentMenu.cornerRadius)

        context.saveGState()
        context.setShadow(offset: CGSize(width: 0, height: 1), blur: 6, color: palette.bubbleShadow.cgColor)
        context.addPath(pill.cgPath)
        context.setFillColor(palette.bubbleFill.cgColor)
        context.fillPath()
        context.restoreGState()
        // The system cutout's lightening gradient only makes sense on the system look;
        // a theme paints its own flat bubble.
        if palette.theme == .automatic, palette.isDark {
            // Apple's dark callout lightens toward its bottom, as its key pop-up does.
            context.saveGState()
            context.addPath(pill.cgPath)
            context.clip()
            let colors = [palette.bubbleFill.cgColor, UIColor(white: 0.33, alpha: 1).cgColor] as CFArray
            if let gradient = CGGradient(colorsSpace: CGColorSpaceCreateDeviceRGB(), colors: colors, locations: [0, 1]) {
                context.drawLinearGradient(
                    gradient,
                    start: CGPoint(x: 0, y: menu.bubbleFrame.minY),
                    end: CGPoint(x: 0, y: menu.bubbleFrame.maxY),
                    options: []
                )
            }
            context.restoreGState()
        }

        for (idx, option) in menu.options.enumerated() {
            let cell = menu.cellFrames[idx]
            if idx == menu.selectedIndex {
                let highlight = UIBezierPath(
                    roundedRect: cell.insetBy(dx: AccentMenu.highlightInset, dy: 0),
                    cornerRadius: AccentMenu.highlightRadius
                )
                context.saveGState()
                context.addPath(highlight.cgPath)
                context.setFillColor(palette.accentSelectFill.cgColor)
                context.fillPath()
                context.restoreGState()
            }
            let attributes: [NSAttributedString.Key: Any] = [.font: Self.font, .foregroundColor: palette.textBase]
            let size = (option as NSString).size(withAttributes: attributes)
            (option as NSString).draw(
                at: CGPoint(x: cell.midX - size.width / 2, y: cell.midY - size.height / 2),
                withAttributes: attributes
            )
        }
    }

    /// The skin-tone picker, drawn as it was.
    private func drawFixed(_ menu: AccentMenu, in context: CGContext) {
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
