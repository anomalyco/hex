import CoreGraphics

nonisolated enum KeyboardLayer: Equatable, Hashable, Sendable {
    case letters
    case numbers
    case symbols
}

/// The kind of text field, as far as the key rows care.
nonisolated enum KeyboardFieldKind: Equatable, Sendable {
    case text
    /// Adds `.` and `/` right of space.
    case url
    /// Adds `@` and `.` right of space, matching Apple's iOS 26 email layout.
    case email
}

nonisolated enum KeyboardOrientation: Equatable, Sendable {
    case portrait
    case landscape
}

nonisolated enum KeyKind: Equatable, Hashable, Sendable {
    /// Inserts its text (letters are stored lowercase; shift decides the case).
    case character(String)
    case shift
    case layerSwitch(KeyboardLayer)
    case delete
    case space
    case returnKey
    case hex
    case globe
    /// Opens the emoji panel (Experiments › Emoji).
    case emoji
}

nonisolated struct KeyboardKey: Equatable, Sendable {
    let kind: KeyKind
    let row: Int
    /// The drawn keycap.
    let frame: CGRect
    /// The touch target. Hit frames tile the whole key area with no gaps.
    var hitFrame: CGRect
    let label: String

    var character: String? {
        if case .character(let text) = kind { return text }
        return nil
    }
}

/// The pop-up balloon over a pressed key: a wider bubble joined to the key by a neck.
nonisolated struct KeyBalloon: Equatable, Sendable {
    static let bubbleHeight: CGFloat = 46
    static let neck: CGFloat = 8
    static let extraWidth: CGFloat = 20

    let keyFrame: CGRect
    let bubble: CGRect

    /// Bubble centre minus key centre; nonzero when an edge key's bubble is pushed inward.
    var offsetX: CGFloat { bubble.midX - keyFrame.midX }
    /// The balloon's whole outline, bubble through key.
    var bounds: CGRect { bubble.union(keyFrame) }
}

nonisolated struct KeyboardMetrics: Equatable, Sendable {
    let side: CGFloat
    let gap: CGFloat
    let rowGap: CGFloat
    let keyHeight: CGFloat
    let stripTop: CGFloat
    let stripHeight: CGFloat
    let stripBottom: CGFloat
    let keysTop: CGFloat
    let keysBottom: CGFloat
    let cornerRadius: CGFloat

    /// iOS 26 key geometry with a 44 pt suggestion strip: 44 pt + 218 pt of keys
    /// (4 × 45 pt + 3 × 11 pt + 5 pt) = 262 pt. The 35 pt strip assumed iOS 26's +17 pt
    /// Liquid Glass crown above the extension; iOS 27 adds none, which left suggestions
    /// cramped against the keyboard's top edge (TestFlight build 4).
    static let portrait = KeyboardMetrics(
        side: 6.5, gap: 6, rowGap: 11, keyHeight: 45,
        stripTop: 0, stripHeight: 44, stripBottom: 0,
        keysTop: 0, keysBottom: 5, cornerRadius: 8.5
    )

    /// Matches iOS 26 landscape keyboard geometry inside the system's +10 pt Liquid Glass crown:
    /// 39 pt strip area + 137 pt of keys (4 × 27.5 pt + 3 × 8 pt + 3 pt) = 176 pt extension height.
    static let landscape = KeyboardMetrics(
        side: 4, gap: 6, rowGap: 8, keyHeight: 27.5,
        stripTop: 0, stripHeight: 39, stripBottom: 0,
        keysTop: 0, keysBottom: 3, cornerRadius: 7.5
    )

    static func `for`(_ orientation: KeyboardOrientation, innerWidth: CGFloat? = nil) -> KeyboardMetrics {
        guard orientation == .portrait else { return .landscape }
        guard let innerWidth else { return .portrait }
        let side: CGFloat = if innerWidth >= 390 {
            6.5
        } else if innerWidth >= 370 {
            4.5
        } else {
            3.0
        }
        if side == KeyboardMetrics.portrait.side { return .portrait }
        return KeyboardMetrics(
            side: side,
            gap: KeyboardMetrics.portrait.gap,
            rowGap: KeyboardMetrics.portrait.rowGap,
            keyHeight: KeyboardMetrics.portrait.keyHeight,
            stripTop: KeyboardMetrics.portrait.stripTop,
            stripHeight: KeyboardMetrics.portrait.stripHeight,
            stripBottom: KeyboardMetrics.portrait.stripBottom,
            keysTop: KeyboardMetrics.portrait.keysTop,
            keysBottom: KeyboardMetrics.portrait.keysBottom,
            cornerRadius: KeyboardMetrics.portrait.cornerRadius
        )
    }

    var stripArea: CGFloat { stripTop + stripHeight + stripBottom }
    var keysArea: CGFloat { keysTop + 4 * keyHeight + 3 * rowGap + keysBottom }
    var totalHeight: CGFloat { stripArea + keysArea }
}

/// Pure geometry of the full keyboard: every key's frame and touch target for one width,
/// layer and field kind. Coordinates are in the keyboard view, strip included.
/// Where the full keyboard's Hex key sits. The strip's trailing edge keeps a full-width
/// space bar and gives the recording animation a starting point.
nonisolated enum HexKeyPlacement: String, CaseIterable, Sendable {
    case rightOfSpace
    case leftOfSpace
    case stripLeading
    case stripTrailing

    /// The snapshot stage's `--hex-placement` can render the others for comparison.
    nonisolated(unsafe) static var current: HexKeyPlacement = .stripTrailing

    var isInStrip: Bool { self == .stripLeading || self == .stripTrailing }
}

nonisolated struct KeyboardLayout: Equatable, Sendable {
    /// Matches Apple's 50 pt shift/delete beside 37.3 pt letters on 440 pt Pro Max (`50 / 37.3 = 1.34`).
    static let modifierWidthRatio: CGFloat = 1.34
    static let bottomSwitchWidthRatio: CGFloat = 1.56
    static let returnWidthRatio: CGFloat = 2.40
    static let compactReturnWidthRatio: CGFloat = 2.18

    let width: CGFloat
    let orientation: KeyboardOrientation
    let layer: KeyboardLayer
    let field: KeyboardFieldKind
    let showsGlobe: Bool
    let showsEmoji: Bool
    let horizontalInset: CGFloat
    let language: KeyboardLanguage
    let metrics: KeyboardMetrics
    let keys: [KeyboardKey]

    init(
        width: CGFloat,
        orientation: KeyboardOrientation = .portrait,
        layer: KeyboardLayer = .letters,
        field: KeyboardFieldKind = .text,
        showsGlobe: Bool = false,
        showsEmoji: Bool = false,
        horizontalInset: CGFloat = 0,
        language: KeyboardLanguage = .english
    ) {
        self.width = width
        self.orientation = orientation
        self.layer = layer
        self.field = field
        self.showsGlobe = showsGlobe
        self.showsEmoji = showsEmoji
        self.horizontalInset = max(0, horizontalInset)
        self.language = language
        let innerWidth = width - 2 * self.horizontalInset
        let metrics = KeyboardMetrics.for(orientation, innerWidth: innerWidth)
        self.metrics = metrics
        self.keys = Self.buildKeys(
            width: width,
            metrics: metrics,
            layer: layer,
            field: field,
            showsGlobe: showsGlobe,
            showsEmoji: showsEmoji,
            inset: self.horizontalInset,
            language: language
        )
    }

    static func height(for orientation: KeyboardOrientation) -> CGFloat {
        KeyboardMetrics.for(orientation).totalHeight
    }

    var totalHeight: CGFloat { metrics.totalHeight }

    /// Where the suggestion / undo strip sits.
    var stripFrame: CGRect {
        let full = CGRect(x: horizontalInset, y: metrics.stripTop, width: width - 2 * horizontalInset, height: metrics.stripHeight)
        guard let hex = key(.hex), hex.row < 0 else { return full }
        let reserved = hex.frame.width + metrics.side + metrics.gap
        return HexKeyPlacement.current == .stripLeading
            ? CGRect(x: full.minX + reserved, y: full.minY, width: full.width - reserved, height: full.height)
            : CGRect(x: full.minX, y: full.minY, width: full.width - reserved, height: full.height)
    }

    var keysAreaFrame: CGRect {
        CGRect(x: 0, y: metrics.stripArea, width: width, height: metrics.keysArea)
    }

    /// The strip's bottom points, below its pills and chips, that belong to row 1's keys: a
    /// tap just above a keycap types it rather than picking a suggestion. Apple's keys reach
    /// about 8 pt above their keycaps, to the predictive bar.
    static let stripTouchBand: CGFloat = 4

    /// Where touches go to the keys: the key area plus `stripTouchBand`. Hit frames tile it.
    var touchAreaFrame: CGRect {
        CGRect(x: 0, y: metrics.stripArea - Self.stripTouchBand, width: width, height: metrics.keysArea + Self.stripTouchBand)
    }

    /// A letter key's width on a 10-key row. Shift, delete and the bottom row are sized
    /// from it in every language, as Apple's are.
    var letterWidth: CGFloat {
        Self.letterWidth(innerWidth: width - 2 * horizontalInset, metrics: metrics)
    }

    /// The width of this page's character keys: `letterWidth`, or narrower where a row
    /// holds 11 keys (Norwegian letters).
    var characterWidth: CGFloat {
        Self.letterWidth(
            innerWidth: width - 2 * horizontalInset,
            metrics: metrics,
            columns: Self.columns(Self.characterRows(for: layer, language: language))
        )
    }

    var showsBalloons: Bool { orientation == .portrait }

    func key(_ kind: KeyKind) -> KeyboardKey? {
        keys.first { $0.kind == kind }
    }

    func hitTest(_ point: CGPoint) -> KeyboardKey? {
        keys.first { $0.hitFrame.contains(point) }
    }

    /// The balloon for a character key, kept inside the keyboard's bounds. `nil` for keys
    /// without one (space, modifiers) and in landscape, where there is no room above row 1.
    func balloon(for key: KeyboardKey) -> KeyBalloon? {
        guard showsBalloons, key.character != nil else { return nil }
        let bubbleWidth = key.frame.width + KeyBalloon.extraWidth
        let bubbleY = max(0, key.frame.minY - KeyBalloon.neck - KeyBalloon.bubbleHeight)
        let minX = max(0, horizontalInset + 2)
        let maxX = max(minX, width - horizontalInset - 2 - bubbleWidth)
        var x = key.frame.midX - bubbleWidth / 2
        x = max(minX, min(maxX, x))
        let bubble = CGRect(
            x: x,
            y: bubbleY,
            width: bubbleWidth,
            height: KeyBalloon.bubbleHeight
        )
        return KeyBalloon(keyFrame: key.frame, bubble: bubble)
    }

    // MARK: - Building

    static func letterWidth(innerWidth: CGFloat, metrics: KeyboardMetrics, columns: Int = 10) -> CGFloat {
        (innerWidth - 2 * metrics.side - CGFloat(columns - 1) * metrics.gap) / CGFloat(columns)
    }

    /// Keys across the widest of rows 1 and 2, never fewer than 10.
    static func columns(_ rows: [[String]]) -> Int {
        max(10, rows.prefix(2).map(\.count).max() ?? 10)
    }

    static func characterRows(for layer: KeyboardLayer, language: KeyboardLanguage = .english) -> [[String]] {
        let punctuation = [".", ",", "?", "!", "'"]
        switch layer {
        case .letters:
            return language.letterRows
        case .numbers:
            return [
                "1234567890".map(String.init),
                ["-", "/", ":", ";", "(", ")", "$", "&", "@", "\""],
                punctuation,
            ]
        case .symbols:
            return [
                ["[", "]", "{", "}", "#", "%", "^", "*", "+", "="],
                ["_", "\\", "|", "~", "<", ">", "€", "£", "¥", "•"],
                punctuation,
            ]
        }
    }

    private static func buildKeys(
        width: CGFloat,
        metrics m: KeyboardMetrics,
        layer: KeyboardLayer,
        field: KeyboardFieldKind,
        showsGlobe: Bool,
        showsEmoji: Bool,
        inset: CGFloat,
        language: KeyboardLanguage
    ) -> [KeyboardKey] {
        let innerWidth = width - 2 * inset
        let rows = characterRows(for: layer, language: language)
        // Modifiers and the bottom row keep the 10-key sizes; an 11-key layout narrows only
        // its character keys, and row 3's letters sit centred between shift and delete.
        let referenceW = letterWidth(innerWidth: innerWidth, metrics: m)
        let lw = letterWidth(innerWidth: innerWidth, metrics: m, columns: columns(rows))
        let modW = referenceW * modifierWidthRatio
        let left = inset + m.side
        let right = width - inset - m.side

        func rowY(_ row: Int) -> CGFloat {
            m.stripArea + m.keysTop + CGFloat(row) * (m.keyHeight + m.rowGap)
        }

        var drawn: [(KeyKind, Int, CGRect, String)] = []

        func add(_ kind: KeyKind, row: Int, x: CGFloat, w: CGFloat, label: String) {
            drawn.append((kind, row, CGRect(x: x, y: rowY(row), width: w, height: m.keyHeight), label))
        }

        func addCharacters(_ chars: [String], row: Int, startX: CGFloat, keyWidth: CGFloat) {
            var x = startX
            for ch in chars {
                add(.character(ch), row: row, x: x, w: keyWidth, label: ch)
                x += keyWidth + m.gap
            }
        }

        func centredStart(count: Int, keyWidth: CGFloat) -> CGFloat {
            let total = CGFloat(count) * keyWidth + CGFloat(count - 1) * m.gap
            return inset + (innerWidth - total) / 2
        }

        // Rows 1 and 2.
        for row in 0..<2 {
            let chars = rows[row]
            addCharacters(chars, row: row, startX: centredStart(count: chars.count, keyWidth: lw), keyWidth: lw)
        }

        // Row 3: shift (or the symbol-page switch), the middle keys, delete. A letter row too
        // long for full-size shift and delete (Russian's nine) narrows them to fit instead, as
        // Apple's Russian keyboard does; every other layout keeps them at `modW`.
        let middle = rows[2]
        let row3ModW = layer == .letters
            ? min(modW, (innerWidth - 2 * m.side - CGFloat(middle.count) * lw - CGFloat(middle.count + 1) * m.gap) / 2)
            : modW
        switch layer {
        case .letters:
            add(.shift, row: 2, x: left, w: row3ModW, label: "shift")
        case .numbers:
            add(.layerSwitch(.symbols), row: 2, x: left, w: modW, label: "#+=")
        case .symbols:
            add(.layerSwitch(.numbers), row: 2, x: left, w: modW, label: "123")
        }
        if layer == .letters {
            addCharacters(middle, row: 2, startX: centredStart(count: middle.count, keyWidth: lw), keyWidth: lw)
        } else {
            // Apple's punctuation row: five wide keys aligned to the s..k columns between modifiers.
            let edgeGap = 1.5 * (lw + m.gap) - modW
            let count = CGFloat(middle.count)
            let middleW = (innerWidth - 2 * m.side - 2 * modW - 2 * edgeGap - (count - 1) * m.gap) / count
            addCharacters(middle, row: 2, startX: left + modW + edgeGap, keyWidth: middleW)
        }
        add(.delete, row: 2, x: right - row3ModW, w: row3ModW, label: "delete")

        // Row 4: 123/ABC, globe?, emoji?, space, field extras (@ . / . /), Hex, return.
        let isCompactBottomRow = showsGlobe || showsEmoji || field != .text
        let switchW = isCompactBottomRow ? modW : referenceW * bottomSwitchWidthRatio
        let returnW = isCompactBottomRow ? (referenceW * compactReturnWidthRatio) : (referenceW * returnWidthRatio + m.gap)
        var leading: [(KeyKind, CGFloat, String)] = [
            layer == .letters
                ? (.layerSwitch(.numbers), switchW, "123")
                : (.layerSwitch(.letters), switchW, "ABC"),
        ]
        if showsGlobe {
            leading.append((.globe, modW, "globe"))
        }
        if showsEmoji {
            leading.append((.emoji, modW, "emoji"))
        }
        let placement = HexKeyPlacement.current
        if placement == .leftOfSpace {
            leading.append((.hex, modW, "hex"))
        }
        var trailing: [(KeyKind, CGFloat, String)] = []
        switch field {
        case .text:
            break
        case .url:
            trailing.append((.character("."), referenceW, "."))
            trailing.append((.character("/"), referenceW, "/"))
        case .email:
            trailing.append((.character("@"), referenceW, "@"))
            trailing.append((.character("."), referenceW, "."))
        }
        if placement == .rightOfSpace {
            trailing.append((.hex, modW, "hex"))
        }
        trailing.append((.returnKey, returnW, "return"))

        let fixed = (leading + trailing).reduce(0) { $0 + $1.1 }
        let itemCount = CGFloat(leading.count + trailing.count + 1)
        let spaceW = max(lw, (right - left) - fixed - (itemCount - 1) * m.gap)
        var x = left
        for (kind, w, label) in leading {
            add(kind, row: 3, x: x, w: w, label: label)
            x += w + m.gap
        }
        add(.space, row: 3, x: x, w: spaceW, label: "space")
        x += spaceW + m.gap
        for (kind, w, label) in trailing {
            add(kind, row: 3, x: x, w: w, label: label)
            x += w + m.gap
        }

        // Touch targets: split every gap down the middle; outer keys reach the edges, and row 1
        // the strip's bottom band.
        let top = m.stripArea - stripTouchBand
        let bottom = m.totalHeight
        var keys: [KeyboardKey] = []
        for row in 0..<4 {
            let rowKeys = drawn.filter { $0.1 == row }.sorted { $0.2.minX < $1.2.minX }
            let y0 = row == 0 ? top : rowY(row) - m.rowGap / 2
            let y1 = row == 3 ? bottom : rowY(row + 1) - m.rowGap / 2
            for (i, item) in rowKeys.enumerated() {
                let x0 = i == 0 ? 0 : (rowKeys[i - 1].2.maxX + item.2.minX) / 2
                let x1 = i == rowKeys.count - 1 ? width : (item.2.maxX + rowKeys[i + 1].2.minX) / 2
                keys.append(KeyboardKey(
                    kind: item.0,
                    row: row,
                    frame: item.2,
                    hitFrame: CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0),
                    label: item.3
                ))
            }
        }
        if placement.isInStrip {
            // A bare mark in a target wider than tall, inset like the suggestion pills.
            let height = m.stripHeight - 8
            let width: CGFloat = 44
            let x = placement == .stripLeading ? left : right - width
            let frame = CGRect(x: x, y: m.stripTop + 4, width: width, height: height)
            let target = frame.insetBy(dx: -4, dy: -4)
            let hitFrame = CGRect(x: target.minX, y: target.minY, width: target.width, height: top - target.minY)
            keys.append(KeyboardKey(kind: .hex, row: -1, frame: frame, hitFrame: hitFrame, label: "hex"))
        }
        return keys
    }
}