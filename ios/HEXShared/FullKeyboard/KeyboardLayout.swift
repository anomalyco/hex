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
    /// Opens the emoji panel (Settings › Keyboard › Emoji).
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

/// Which of Apple's iPhone key planes a screen gets (`KeyboardMetrics.phonePortrait`), by its
/// height in points: Apple's keycaps and rows differ with it (iOS 27.2).
nonisolated enum KeyboardPhoneSize: Equatable, Sendable {
    /// A Home-button iPhone (the SE, 667 pt).
    case homeButton
    /// 844 to 874 pt (the 17e, 16, 17 and 17 Pro), and anything else under 900 pt.
    case regular
    /// 900 pt and taller (the Air, the Plus and Pro Max sizes).
    case large

    static func current(screenSize: CGSize) -> KeyboardPhoneSize {
        let tall = max(screenSize.width, screenSize.height)
        if tall < 700 { return .homeButton }
        return tall < 900 ? .regular : .large
    }
}

/// Where the keyboard is shown. Every iPhone is `.phone`; the iPhone Duo (a foldable) sets
/// third-party keyboards in an inset platter, with iOS's own band above them, and Apple sizes
/// its keys there for each screen.
nonisolated enum KeyboardPosture: Equatable, Sendable {
    case phone
    /// The Duo folded: its 466 × 678 pt cover screen, either way up.
    case cover
    /// The Duo unfolded and held upright: the inner screen is wider than tall (951 × 669 pt).
    case unfoldedWide
    /// The Duo unfolded and turned: the inner screen is taller than wide (669 × 951 pt).
    case unfoldedTall

    /// The Duo's model identifier (the simulator's too).
    static let foldableModels: Set<String> = ["iPhone19,4"]

    /// The posture for a screen of `screenSize` points (as it is turned now), on a foldable or not.
    static func current(screenSize: CGSize, isFoldable: Bool) -> KeyboardPosture {
        guard isFoldable else { return .phone }
        guard min(screenSize.width, screenSize.height) >= 600 else { return .cover }
        return screenSize.width > screenSize.height ? .unfoldedWide : .unfoldedTall
    }

    /// Apple splits its keyboard on the Duo unfolded and held upright (the inner screen wider
    /// than tall); Hex does too, automatically, as there's no setting a keyboard can read.
    var splitsKeyboard: Bool { self == .unfoldedWide }

    /// iOS draws a band of its own above a third-party keyboard here, so the strip needs no gap
    /// under it to sit as far above the keys as Apple's predictive bar does.
    var hasSystemBand: Bool { self != .phone }

    /// How far in from the screen's side Apple's keycaps start on the Duo; nil on an iPhone,
    /// where the keyboard has the whole width. Measured in the iOS 27.2 simulator.
    var appleKeyInset: CGFloat? {
        switch self {
        case .phone: nil
        // Folded: 6.67 pt in, a third of a point outside the 452 pt view iOS gives a third-party
        // keyboard, so Hex's keys run to the view's edges (`KeyboardMetrics.for`).
        case .cover: 6.67
        case .unfoldedWide: 86
        case .unfoldedTall: 19
        }
    }
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
    /// How much shorter row 4's keycaps are than the others: where Apple's row 4 reaches
    /// below the bottom of a third-party keyboard's view, Hex's keeps Apple's top edge.
    var bottomRowTrim: CGFloat = 0
    /// Row 4 on row 1's and row 2's columns, as Apple's is on the Duo folded: 123, the globe and
    /// emoji one key wide on q, w and e, space from r to n, dictation on m, return from l.
    var bottomRowOnLetterColumns = false

    /// The suggestion strip is the part of Apple's predictive bar below the band iOS 26 draws
    /// over every third-party keyboard (16.3 pt on the iPhone 17 Pro, 12.5 pt on the SE, 13.3 pt
    /// on the 17 Pro Max, iOS 27.2; nothing can draw there). Apple's bar is 44 pt from the
    /// panel's edge, then about 8 pt to row 1's keycaps, which start 51.7, 48 and 48.3 pt down:
    /// 35.3, 35.5 and 35 pt below this view's top on those iPhones. So the strip is 27 pt, then
    /// the same 8 pt gap, and row 1 starts 35 pt down. Everything the strip shows (suggestions,
    /// Undo dictation, the Full Access hint, the recording bar, the emoji panel's top row, the
    /// Logging mark) lives in those 27 pt.
    static let stripBarHeight: CGFloat = 27
    static let stripGap: CGFloat = 8
    /// On the iPhone Duo iOS sets the keyboard in a platter with its own band above, and the
    /// strip is Apple's whole 44 pt bar, right on the keys.
    static let insetStripHeight: CGFloat = 44
    /// The Duo folded and upright: iOS's band there is 8.7 pt and Apple's keycaps start 48.7 pt
    /// below the panel's edge (iOS 27.2), so the strip is 32 pt with the iPhone's 8 pt gap.
    static let coverStripHeight: CGFloat = 32

    /// How far the strip's pills (an autocorrection, a pressed suggestion, the Hex key, the
    /// recording bar) sit in from its top and bottom: Apple's pill fills its bar but the
    /// edge band iOS draws, so in the 27 pt strip half a point, in the Duo's 44 pt bar 4 pt.
    var stripInset: CGFloat { (stripHeight - stripPillHeight) / 2 }
    /// The pills' height: the strip less half a point above and below, at most 36 pt (26 pt in
    /// the phone's portrait strip, 36 pt in landscape and in the Duo's 44 pt bar).
    var stripPillHeight: CGFloat { min(stripHeight - 1, 36) }

    /// Apple's keycap corners (iOS 27.2: the same on every iPhone, measured from screenshots
    /// against keys Hex drew at known radii).
    static let keyCornerRadius: CGFloat = 7.9

    /// iPhone portrait key geometry, as Apple's keyboard draws it beside Hex in the same field
    /// (iOS 27.2) on an 844 to 874 pt iPhone (17e, 17, 17 Pro): 43 pt keycaps on a 54 pt row
    /// pitch, row 1 on the same line on screen. Apple's row 4 ends 4 pt below where a
    /// third-party keyboard's view ends, so Hex's row 4 keeps Apple's top edge and is 4 pt
    /// shorter: 35 pt + 201 pt of keys (4 × 43 pt + 3 × 11 pt − 4 pt) = 236 pt. Hex drew iOS 26's
    /// 45 pt keys on a 56 pt pitch with 5 pt under them, 11 to 17 pt above Apple's rows, so
    /// fingers used to Apple's keyboard landed low on Hex's keys.
    static let portrait = KeyboardMetrics(
        side: 6.5, gap: 6, rowGap: 11, keyHeight: 43,
        stripTop: 0, stripHeight: stripBarHeight, stripBottom: stripGap,
        keysTop: 0, keysBottom: 0, cornerRadius: keyCornerRadius, bottomRowTrim: 4
    )

    /// Turned, iOS draws no band above the keyboard (iPhone 17 Pro Max, iOS 27.2): Apple's keycaps
    /// start 50 pt below the panel's edge, so the strip is 42 pt and the same gap.
    static let landscapeStripHeight: CGFloat = 42

    /// The other iPhones' key planes (`KeyboardPhoneSize`), measured the same way: a Home-button
    /// iPhone (SE) has 42 pt keycaps on a 54 pt pitch and 4 pt under row 4; one 900 pt or taller
    /// (Air, Pro Max) 45 pt keycaps on a 56 pt pitch and 7 pt under row 4.
    static func phonePortrait(_ size: KeyboardPhoneSize) -> KeyboardMetrics {
        switch size {
        case .regular:
            return .portrait
        case .homeButton:
            return KeyboardMetrics(
                side: 6.5, gap: 6, rowGap: 12, keyHeight: 42,
                stripTop: 0, stripHeight: stripBarHeight, stripBottom: stripGap,
                keysTop: 0, keysBottom: 4, cornerRadius: keyCornerRadius
            )
        case .large:
            return KeyboardMetrics(
                side: 6.5, gap: 6, rowGap: 11, keyHeight: 45,
                stripTop: 0, stripHeight: stripBarHeight, stripBottom: stripGap,
                keysTop: 0, keysBottom: 7, cornerRadius: keyCornerRadius
            )
        }
    }

    /// iOS 26 landscape key geometry under that strip: 50 pt + 137 pt of keys
    /// (4 × 27.5 pt + 3 × 8 pt + 3 pt) = 187 pt.
    static let landscape = KeyboardMetrics(
        side: 4, gap: 6, rowGap: 8, keyHeight: 27.5,
        stripTop: 0, stripHeight: landscapeStripHeight, stripBottom: stripGap,
        keysTop: 0, keysBottom: 3, cornerRadius: 7.5
    )

    /// `windowInset` is how far iOS set the keyboard in from each side of the screen (the Duo).
    static func `for`(
        _ orientation: KeyboardOrientation,
        innerWidth: CGFloat? = nil,
        posture: KeyboardPosture = .phone,
        windowInset: CGFloat = 0,
        phoneSize: KeyboardPhoneSize = .regular
    ) -> KeyboardMetrics {
        let base: KeyboardMetrics
        if orientation == .landscape {
            base = .landscape
        } else if posture == .phone {
            let phone = phonePortrait(phoneSize)
            // Apple's keys start 6.5 pt in on every iPhone from the SE's 375 pt up.
            let side: CGFloat = (innerWidth ?? 402) >= 370 ? 6.5 : 3.0
            base = side == phone.side ? phone : phone.with(side: side)
        } else {
            base = .portrait
        }
        guard posture != .phone else { return base }

        // The Duo: Apple's row pitch for each screen (56 pt folded, 59.7 pt unfolded and turned,
        // 51.7 pt unfolded upright), keycaps starting where Apple's do, and no gap under the strip.
        var keyHeight = base.keyHeight
        var rowGap = base.rowGap
        if orientation == .portrait {
            switch posture {
            case .unfoldedTall:
                keyHeight = 46
                rowGap = 13.7
            case .unfoldedWide:
                keyHeight = 39
                rowGap = 12.7
            case .cover:
                keyHeight = 45
                rowGap = 11
            case .phone:
                break
            }
        }
        let side = orientation == .portrait
            ? max(0, (posture.appleKeyInset ?? base.side) - windowInset)
            : base.side
        // Measured folded and upright only; unfolded, and folded in landscape, the strip is still
        // Apple's whole bar right on the keys. Folded and upright (iOS 27.2 simulator, beside
        // Apple's keyboard) Apple's row 4 ends 7 pt above the keyboard's bottom and its keycaps
        // round 9.8 pt; Hex drew 5 pt and 7.9 pt, its rows 2 pt low. Unfolded keeps the 5 pt
        // under row 4 it was measured with.
        let coverPortrait = posture == .cover && orientation == .portrait
        return KeyboardMetrics(
            side: side, gap: base.gap, rowGap: rowGap, keyHeight: keyHeight,
            stripTop: base.stripTop,
            stripHeight: coverPortrait ? coverStripHeight : insetStripHeight,
            stripBottom: coverPortrait ? stripGap : 0,
            keysTop: base.keysTop,
            keysBottom: coverPortrait ? 7 : orientation == .portrait ? 5 : base.keysBottom,
            cornerRadius: coverPortrait ? 9.8 : base.cornerRadius,
            bottomRowOnLetterColumns: coverPortrait
        )
    }

    private func with(side: CGFloat) -> KeyboardMetrics {
        KeyboardMetrics(
            side: side, gap: gap, rowGap: rowGap, keyHeight: keyHeight,
            stripTop: stripTop, stripHeight: stripHeight, stripBottom: stripBottom,
            keysTop: keysTop, keysBottom: keysBottom, cornerRadius: cornerRadius,
            bottomRowTrim: bottomRowTrim, bottomRowOnLetterColumns: bottomRowOnLetterColumns
        )
    }

    var stripArea: CGFloat { stripTop + stripHeight + stripBottom }
    /// Row 4's keycap height.
    var bottomRowHeight: CGFloat { keyHeight - bottomRowTrim }
    var keysArea: CGFloat { keysTop + 4 * keyHeight + 3 * rowGap + keysBottom - bottomRowTrim }
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

    /// The keyboard's, for typing logs: `FullKeyboardView` keeps Settings › Keyboard › Hex key
    /// (`FullKeyboardSettings.hexKeyPlacement`) here; layouts take theirs as a parameter. The
    /// snapshot stage's `--hex-placement` can render the others for comparison.
    nonisolated(unsafe) static var current: HexKeyPlacement = .stripTrailing

    var isInStrip: Bool { self == .stripLeading || self == .stripTrailing }
}

nonisolated struct KeyboardLayout: Equatable, Sendable {
    /// Shift, delete, #+= and 123 on row 3 (and the Duo's globe and emoji keys) from the letter
    /// pitch: Apple's are 1.227 pitches less 3 pt on every iPhone (45.5 pt on the 17 Pro's
    /// 39.5 pt pitch, 50.2 on the Pro Max's 43.3, 42 on the SE's 36.8; iOS 27.2). A fixed
    /// 1.34 letters was half a point narrow on the 17 Pro.
    static func modifierWidth(letterWidth: CGFloat, gap: CGFloat) -> CGFloat {
        1.227 * (letterWidth + gap) - 3
    }

    /// Row 3's modifier width in this layout (`modifierWidth(letterWidth:gap:)`).
    var modifierWidth: CGFloat { Self.modifierWidth(letterWidth: letterWidth, gap: metrics.gap) }
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
    /// The letters' arrangement (Settings › Keyboard › Languages › Layout): `language`'s own
    /// unless it offers the one asked for.
    let letters: KeyboardLetterLayout
    let posture: KeyboardPosture
    /// Where the Hex key sits: Settings › Keyboard › Hex key picks the strip's trailing
    /// (Right) or leading (Left) end.
    let hexPlacement: HexKeyPlacement
    let metrics: KeyboardMetrics
    let keys: [KeyboardKey]
    /// The two halves' inner edges while the keyboard is split (the Duo unfolded upright, as
    /// Apple's is there); nil otherwise.
    let split: SplitGeometry?

    init(
        width: CGFloat,
        orientation: KeyboardOrientation = .portrait,
        layer: KeyboardLayer = .letters,
        field: KeyboardFieldKind = .text,
        showsGlobe: Bool = false,
        showsEmoji: Bool = false,
        horizontalInset: CGFloat = 0,
        language: KeyboardLanguage = .english,
        letters: KeyboardLetterLayout? = nil,
        posture: KeyboardPosture = .phone,
        windowInset: CGFloat = 0,
        hexPlacement: HexKeyPlacement = .stripTrailing,
        phoneSize: KeyboardPhoneSize = .regular
    ) {
        self.width = width
        let letters = language.offered(letters)
        self.letters = letters
        self.hexPlacement = hexPlacement
        self.orientation = orientation
        self.layer = layer
        self.field = field
        self.showsGlobe = showsGlobe
        self.showsEmoji = showsEmoji
        self.horizontalInset = max(0, horizontalInset)
        self.language = language
        self.posture = posture
        let innerWidth = width - 2 * self.horizontalInset
        let metrics = KeyboardMetrics.for(
            orientation, innerWidth: innerWidth, posture: posture, windowInset: windowInset, phoneSize: phoneSize
        )
        self.metrics = metrics
        if posture.splitsKeyboard, orientation == .portrait {
            let built = Self.buildSplitKeys(
                width: width,
                metrics: metrics,
                layer: layer,
                field: field,
                showsGlobe: showsGlobe,
                showsEmoji: showsEmoji,
                inset: self.horizontalInset,
                language: language,
                letters: letters,
                hexPlacement: hexPlacement
            )
            self.keys = built.keys
            self.split = built.split
        } else {
            self.keys = Self.buildKeys(
                width: width,
                metrics: metrics,
                layer: layer,
                field: field,
                showsGlobe: showsGlobe,
                showsEmoji: showsEmoji,
                inset: self.horizontalInset,
                language: language,
                letters: letters,
                hexPlacement: hexPlacement
            )
            self.split = nil
        }
    }

    var isSplit: Bool { split != nil }

    /// The Hex key sits at the strip's leading (left) end, so what grows from it or points at
    /// it (the recording bar's ✕ and ✓, its menu, the Full Access hint's Open Hex) mirrors.
    var hexOnLeading: Bool { hexPlacement == .stripLeading }

    static func height(
        for orientation: KeyboardOrientation,
        posture: KeyboardPosture = .phone,
        phoneSize: KeyboardPhoneSize = .regular
    ) -> CGFloat {
        KeyboardMetrics.for(orientation, posture: posture, phoneSize: phoneSize).totalHeight
    }

    var totalHeight: CGFloat { metrics.totalHeight }

    /// Apple's predictive bar here: the strip with the Hex key's room (`stripFrame` leaves it out).
    var stripBarFrame: CGRect {
        var full = CGRect(x: horizontalInset, y: metrics.stripTop, width: width - 2 * horizontalInset, height: metrics.stripHeight)
        if let split {
            if hexOnLeading {
                // Over the left half with the Hex key, for the left thumb, mirroring the right.
                let maxX = split.leftEdge + SplitGeometry.stripLead
                full = CGRect(x: full.minX, y: full.minY, width: maxX - full.minX, height: full.height)
            } else {
                // Over the right half, where Apple's split keyboard puts its predictions.
                let minX = split.rightEdge - SplitGeometry.stripLead
                full = CGRect(x: minX, y: full.minY, width: full.maxX - minX, height: full.height)
            }
        }
        return full
    }

    /// Where the suggestion / undo strip sits.
    var stripFrame: CGRect {
        let full = stripBarFrame
        guard let hex = key(.hex), hex.row < 0 else { return full }
        let reserved = hex.frame.width + metrics.side + metrics.gap
        return hexOnLeading
            ? CGRect(x: full.minX + reserved, y: full.minY, width: full.width - reserved, height: full.height)
            : CGRect(x: full.minX, y: full.minY, width: full.width - reserved, height: full.height)
    }

    /// The three suggestion slots, in keyboard coordinates: thirds of the bar, as Apple's are
    /// (its dividers sit at a third and two thirds of the keyboard), with the Hex key's room
    /// taken from the slot beside it.
    var stripSlotFrames: [CGRect] {
        let bar = stripBarFrame
        let strip = stripFrame
        let third = bar.width / 3
        return (0..<3).map { index in
            let minX = max(strip.minX, bar.minX + CGFloat(index) * third)
            let maxX = min(strip.maxX, bar.minX + CGFloat(index + 1) * third)
            return CGRect(x: minX, y: strip.minY, width: max(0, maxX - minX), height: strip.height)
        }
    }

    var keysAreaFrame: CGRect {
        CGRect(x: 0, y: metrics.stripArea, width: width, height: metrics.keysArea)
    }

    /// Where the strip sits right on the keys (the Duo), its bottom points, below its pills and
    /// chips, belong to row 1's keys: a tap just above a keycap types it rather than picking a
    /// suggestion.
    static let stripTouchBand: CGFloat = 4

    /// How far above its keycaps row 1 takes touches: the gap under the strip, as Apple's keys
    /// reach about 8 pt above their keycaps to the predictive bar, or `stripTouchBand`.
    var rowOneReach: CGFloat { max(metrics.stripBottom, Self.stripTouchBand) }

    /// Where touches go to the keys: the key area plus `rowOneReach`. Hit frames tile it.
    var touchAreaFrame: CGRect {
        CGRect(x: 0, y: metrics.stripArea - rowOneReach, width: width, height: metrics.keysArea + rowOneReach)
    }

    /// A letter key's width on a 10-key row. Shift, delete and the bottom row are sized
    /// from it in every language, as Apple's are.
    var letterWidth: CGFloat {
        if isSplit { return SplitGeometry.keyWidth }
        return Self.letterWidth(innerWidth: width - 2 * horizontalInset, metrics: metrics)
    }

    /// `x` as it would be with the halves pushed together, so a tap on the right half's G and
    /// one on the left half's land on the same key for Spatial autocorrect.
    func unsplitX(_ x: CGFloat) -> CGFloat {
        guard let split, x >= split.middle else { return x }
        return x - split.shift
    }

    /// The width of this page's character keys: `letterWidth`, or narrower where a row
    /// holds 11 keys (Norwegian letters).
    var characterWidth: CGFloat {
        if isSplit { return SplitGeometry.keyWidth }
        if layer == .letters, letters == .dvorak {
            return Self.dvorakLetterWidth(innerWidth: width - 2 * horizontalInset, metrics: metrics)
        }
        return Self.letterWidth(
            innerWidth: width - 2 * horizontalInset,
            metrics: metrics,
            columns: Self.columns(Self.characterRows(for: layer, language: language, letters: letters))
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

    /// Dvorak's letter keys: narrower than a 10-key row's, so shift and nine letters fill row 3
    /// edge to edge, as on Apple's (iOS 27.0 simulator: a 38.3 pt pitch where QWERTY's is 39.5 pt
    /// on a 402 pt iPhone).
    static func dvorakLetterWidth(innerWidth: CGFloat, metrics: KeyboardMetrics) -> CGFloat {
        let modifier = modifierWidth(letterWidth: letterWidth(innerWidth: innerWidth, metrics: metrics), gap: metrics.gap)
        return (innerWidth - 2 * metrics.side - modifier) / 9 - metrics.gap
    }

    /// Keys across the widest of rows 1 and 2, never fewer than 10.
    static func columns(_ rows: [[String]]) -> Int {
        max(10, rows.prefix(2).map(\.count).max() ?? 10)
    }

    static func characterRows(
        for layer: KeyboardLayer,
        language: KeyboardLanguage = .english,
        letters: KeyboardLetterLayout? = nil
    ) -> [[String]] {
        let punctuation = [".", ",", "?", "!", "'"]
        switch layer {
        case .letters:
            return language.letterRows(language.offered(letters))
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
        language: KeyboardLanguage,
        letters: KeyboardLetterLayout,
        hexPlacement placement: HexKeyPlacement
    ) -> [KeyboardKey] {
        let innerWidth = width - 2 * inset
        let rows = characterRows(for: layer, language: language, letters: letters)
        let dvorak = layer == .letters && letters == .dvorak
        // Modifiers and the bottom row keep the 10-key sizes; an 11-key layout narrows only
        // its character keys, and row 3's letters sit centred between shift and delete.
        let referenceW = letterWidth(innerWidth: innerWidth, metrics: m)
        let lw = dvorak
            ? dvorakLetterWidth(innerWidth: innerWidth, metrics: m)
            : letterWidth(innerWidth: innerWidth, metrics: m, columns: columns(rows))
        let modW = modifierWidth(letterWidth: referenceW, gap: m.gap)
        let left = inset + m.side
        let right = width - inset - m.side

        func rowY(_ row: Int) -> CGFloat {
            m.stripArea + m.keysTop + CGFloat(row) * (m.keyHeight + m.rowGap)
        }

        var drawn: [(KeyKind, Int, CGRect, String)] = []

        func add(_ kind: KeyKind, row: Int, x: CGFloat, w: CGFloat, label: String) {
            let h = row == 3 ? m.bottomRowHeight : m.keyHeight
            drawn.append((kind, row, CGRect(x: x, y: rowY(row), width: w, height: h), label))
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

        if dvorak {
            // Apple's Dvorak: row 1's seven letters end at a delete key a pitch and a half wide,
            // row 2's ten start at the edge, and row 3 is shift and nine letters, edge to edge.
            let pitch = lw + m.gap
            let deleteW = 1.5 * pitch - m.gap
            add(.delete, row: 0, x: right - deleteW, w: deleteW, label: "delete")
            addCharacters(rows[0], row: 0, startX: right - deleteW - CGFloat(rows[0].count) * pitch, keyWidth: lw)
            addCharacters(rows[1], row: 1, startX: left, keyWidth: lw)
            add(.shift, row: 2, x: left, w: modW, label: "shift")
            addCharacters(rows[2], row: 2, startX: left + modW + m.gap, keyWidth: lw)
        }

        // Rows 1 and 2.
        for row in 0..<2 where !dvorak {
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
        if !dvorak {
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
        }

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

        if field == .text, m.bottomRowOnLetterColumns {
            // The Duo folded: each key before the space bar one letter wide on row 1's columns,
            // space to where row 2's k column starts, then dictation (Hex has none: space runs on,
            // or a Hex key right of space takes that slot), and return from l's column.
            let pitch = referenceW + m.gap
            let kColumn = left + 7.5 * pitch
            let returnStart = left + 8.5 * pitch
            var lead: [(KeyKind, String)] = [leading[0].0 == .layerSwitch(.numbers) ? (.layerSwitch(.numbers), "123") : (.layerSwitch(.letters), "ABC")]
            if showsGlobe { lead.append((.globe, "globe")) }
            if showsEmoji { lead.append((.emoji, "emoji")) }
            if placement == .leftOfSpace { lead.append((.hex, "hex")) }
            for (i, (kind, label)) in lead.enumerated() {
                add(kind, row: 3, x: left + CGFloat(i) * pitch, w: referenceW, label: label)
            }
            let spaceStart = left + CGFloat(lead.count) * pitch
            var spaceEnd = returnStart - m.gap
            if placement == .rightOfSpace {
                add(.hex, row: 3, x: kColumn, w: returnStart - m.gap - kColumn, label: "hex")
                spaceEnd = kColumn - m.gap
            }
            add(.space, row: 3, x: spaceStart, w: max(lw, spaceEnd - spaceStart), label: "space")
            add(.returnKey, row: 3, x: returnStart, w: right - returnStart, label: "return")
        } else if field == .text, m.keyHeight >= 40 {
            // Apple's portrait bottom row stands on row 3's letter columns: 123 (and emoji, or
            // the globe on a Home-button iPhone) share the width from the edge to z's right edge,
            // the space bar runs from x to n, and return from m to the edge. With no emoji key
            // (Emoji not among the keyboards), Apple's 123 takes the whole first block. Hex's Hex
            // key, left or right of space, comes out of the space bar.
            let pitch = referenceW + m.gap
            let blockEnd = left + 2.5 * pitch - m.gap
            let returnStart = left + 7.5 * pitch
            var block: [(KeyKind, String)] = [leading[0].0 == .layerSwitch(.numbers) ? (.layerSwitch(.numbers), "123") : (.layerSwitch(.letters), "ABC")]
            if showsGlobe { block.append((.globe, "globe")) }
            // Beside the globe, the emoji key takes x's column, where Apple's dictation key is.
            let emojiInColumn = showsEmoji && showsGlobe
            if showsEmoji, !emojiInColumn { block.append((.emoji, "emoji")) }
            let blockW = (blockEnd - left - CGFloat(block.count - 1) * m.gap) / CGFloat(block.count)
            var x = left
            for (kind, label) in block {
                add(kind, row: 3, x: x, w: blockW, label: label)
                x += blockW + m.gap
            }
            if emojiInColumn {
                add(.emoji, row: 3, x: x, w: referenceW, label: "emoji")
                x += referenceW + m.gap
            }
            var spaceEnd = returnStart - m.gap
            if placement == .leftOfSpace {
                add(.hex, row: 3, x: x, w: modW, label: "hex")
                x += modW + m.gap
            }
            if placement == .rightOfSpace {
                add(.hex, row: 3, x: spaceEnd - modW, w: modW, label: "hex")
                spaceEnd -= modW + m.gap
            }
            add(.space, row: 3, x: x, w: max(lw, spaceEnd - x), label: "space")
            add(.returnKey, row: 3, x: returnStart, w: right - returnStart, label: "return")
        } else {
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
        }

        // Touch targets: split every gap down the middle; outer keys reach the edges, and row 1
        // the gap under the strip (or the strip's bottom band).
        let top = m.stripArea - max(m.stripBottom, stripTouchBand)
        let bottom = m.totalHeight
        var keys: [KeyboardKey] = []
        for row in 0..<4 {
            let rowKeys = drawn.filter { $0.1 == row }.sorted { $0.2.minX < $1.2.minX }
            let y0 = row == 0 ? top : rowY(row) - m.rowGap / 2
            let y1 = row == 3 ? bottom : rowY(row + 1) - m.rowGap / 2
            for (i, item) in rowKeys.enumerated() {
                let x0 = i == 0 ? 0 : (rowKeys[i - 1].2.maxX + item.2.minX) / 2
                let x1 = i == rowKeys.count - 1 ? width : (item.2.maxX + rowKeys[i + 1].2.minX) / 2
                var hit = CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
                if item.0 == .emoji, row == 3, m.keyHeight >= 40 {
                    // Apple's emoji key takes taps only well inside its keycap, 6 pt in from each
                    // side and 7.5 pt below its top, and the margins around it type nothing:
                    // a finger aimed at 123 or space that lands near it doesn't switch keyboards.
                    hit = CGRect(x: item.2.minX + 6, y: item.2.minY + 7.5, width: item.2.width - 12, height: y1 - item.2.minY - 7.5)
                }
                keys.append(KeyboardKey(
                    kind: item.0,
                    row: row,
                    frame: item.2,
                    hitFrame: hit,
                    label: item.3
                ))
            }
        }
        if placement.isInStrip {
            // A bare mark in a target wider than tall, inset like the suggestion pills.
            let height = m.stripPillHeight
            let width: CGFloat = 44
            let x = placement == .stripLeading ? left : right - width
            let frame = CGRect(x: x, y: m.stripTop + m.stripInset, width: width, height: height)
            let target = frame.insetBy(dx: -4, dy: -4)
            let hitFrame = CGRect(x: target.minX, y: target.minY, width: target.width, height: top - target.minY)
            keys.append(KeyboardKey(kind: .hex, row: -1, frame: frame, hitFrame: hitFrame, label: "hex"))
        }
        return keys
    }
}

/// Where a split keyboard's halves end. Apple's split keyboard on the Duo unfolded upright
/// (iOS 27.2 simulator, 801 pt keyboard): 46.3 pt keycaps on a 57 pt pitch, shift and delete
/// 63.3 pt, 123 66.7 pt, globe and emoji 40.7 pt, return 66 pt; each half starts at the
/// keyboard's side, the middle (about 220 pt) holds no key, and its predictions sit over the
/// right half.
nonisolated struct SplitGeometry: Equatable, Sendable {
    static let keyWidth: CGFloat = 46.3
    static let gap: CGFloat = 10.7
    static var pitch: CGFloat { keyWidth + gap }
    static let modifierWidth: CGFloat = 63.3
    static let layerSwitchWidth: CGFloat = 66.7
    static let smallKeyWidth: CGFloat = 40.7
    static let returnWidth: CGFloat = 66
    /// How far left of the right half's first key the strip starts (Apple's first prediction).
    static let stripLead: CGFloat = 38.3

    /// The left half's right edge (row 1's last keycap) and the right half's left edge.
    let leftEdge: CGFloat
    let rightEdge: CGFloat

    /// Touches right of this belong to the right half.
    var middle: CGFloat { (leftEdge + rightEdge) / 2 }
    /// How far the right half would move left to sit one gap after the left half.
    var shift: CGFloat { rightEdge - leftEdge - Self.gap }

    /// One row's keys for each half: an even row splits down the middle, an odd one gives its
    /// middle key to both halves, as Apple's English split keeps G and V on both sides.
    static func halves(_ row: [String]) -> (left: [String], right: [String]) {
        let n = row.count
        guard n > 1 else { return (row, row) }
        if n % 2 == 1 {
            return (Array(row[...(n / 2)]), Array(row[(n / 2)...]))
        }
        return (Array(row[..<(n / 2)]), Array(row[(n / 2)...]))
    }
}

extension KeyboardLayout {
    /// The split keyboard: each row's halves at the keyboard's sides with Apple's key sizes,
    /// row 2 in half a key from the side and row 3's letters a key and a half, two space bars,
    /// and touch targets that tile each half but leave the middle to no key.
    static func buildSplitKeys(
        width: CGFloat,
        metrics m: KeyboardMetrics,
        layer: KeyboardLayer,
        field: KeyboardFieldKind,
        showsGlobe: Bool,
        showsEmoji: Bool,
        inset: CGFloat,
        language: KeyboardLanguage,
        letters: KeyboardLetterLayout = .own,
        hexPlacement placement: HexKeyPlacement
    ) -> (keys: [KeyboardKey], split: SplitGeometry) {
        typealias G = SplitGeometry
        // Dvorak splits like the other layouts, delete ending row 3 (Apple's split Dvorak unobserved).
        let rows = characterRows(for: layer, language: language, letters: letters)
        let left = inset + m.side
        let right = width - inset - m.side
        let p = G.pitch

        func rowY(_ row: Int) -> CGFloat {
            m.stripArea + m.keysTop + CGFloat(row) * (m.keyHeight + m.rowGap)
        }
        var drawn: [(KeyKind, Int, CGRect, String, Bool)] = [] // kind, row, frame, label, right half
        func add(_ kind: KeyKind, row: Int, x: CGFloat, w: CGFloat, label: String, rightHalf: Bool) {
            drawn.append((kind, row, CGRect(x: x, y: rowY(row), width: w, height: m.keyHeight), label, rightHalf))
        }
        func addRow(_ chars: [String], row: Int, leftStart: CGFloat, rightEnd: CGFloat) -> (CGFloat, CGFloat) {
            let (l, r) = G.halves(chars)
            for (i, ch) in l.enumerated() {
                add(.character(ch), row: row, x: leftStart + CGFloat(i) * p, w: G.keyWidth, label: ch, rightHalf: false)
            }
            let rStart = rightEnd - CGFloat(r.count) * p + G.gap
            for (i, ch) in r.enumerated() {
                add(.character(ch), row: row, x: rStart + CGFloat(i) * p, w: G.keyWidth, label: ch, rightHalf: true)
            }
            return (leftStart + CGFloat(l.count) * p - G.gap, rStart)
        }

        // Rows 1 and 2: row 2 sits half a key in from each side.
        let (leftEdge, rightEdge) = addRow(rows[0], row: 0, leftStart: left, rightEnd: right)
        _ = addRow(rows[1], row: 1, leftStart: left + p / 2, rightEnd: right - p / 2)

        // Row 3: shift (or the page switch) and delete at the sides, the middle keys a key and a
        // half in from each side.
        switch layer {
        case .letters: add(.shift, row: 2, x: left, w: G.modifierWidth, label: "shift", rightHalf: false)
        case .numbers: add(.layerSwitch(.symbols), row: 2, x: left, w: G.modifierWidth, label: "#+=", rightHalf: false)
        case .symbols: add(.layerSwitch(.numbers), row: 2, x: left, w: G.modifierWidth, label: "123", rightHalf: false)
        }
        _ = addRow(rows[2], row: 2, leftStart: left + 1.5 * p, rightEnd: right - 1.5 * p)
        add(.delete, row: 2, x: right - G.modifierWidth, w: G.modifierWidth, label: "delete", rightHalf: true)

        // Row 4: 123/ABC, globe?, emoji? and a space bar on the left; a space bar, the field's
        // extras, Hex (when it sits in the row) and return on the right.
        var x = left
        let switchKind: KeyKind = layer == .letters ? .layerSwitch(.numbers) : .layerSwitch(.letters)
        add(switchKind, row: 3, x: x, w: G.layerSwitchWidth, label: layer == .letters ? "123" : "ABC", rightHalf: false)
        x += G.layerSwitchWidth + G.gap
        if showsGlobe {
            add(.globe, row: 3, x: x, w: G.smallKeyWidth, label: "globe", rightHalf: false)
            x += G.smallKeyWidth + G.gap
        }
        if showsEmoji {
            add(.emoji, row: 3, x: x, w: G.smallKeyWidth, label: "emoji", rightHalf: false)
            x += G.smallKeyWidth + G.gap
        }
        add(.space, row: 3, x: x, w: max(G.keyWidth, leftEdge - x), label: "space", rightHalf: false)

        var trailing: [(KeyKind, CGFloat, String)] = []
        switch field {
        case .text: break
        case .url:
            trailing.append((.character("."), G.smallKeyWidth, "."))
            trailing.append((.character("/"), G.smallKeyWidth, "/"))
        case .email:
            trailing.append((.character("@"), G.smallKeyWidth, "@"))
            trailing.append((.character("."), G.smallKeyWidth, "."))
        }
        if !placement.isInStrip {
            trailing.append((.hex, G.smallKeyWidth, "hex"))
        }
        trailing.append((.returnKey, G.returnWidth, "return"))
        let trailingWidth = trailing.reduce(0) { $0 + $1.1 + G.gap }
        let spaceRightWidth = max(G.keyWidth, right - rightEdge - trailingWidth)
        add(.space, row: 3, x: rightEdge, w: spaceRightWidth, label: "space", rightHalf: true)
        x = rightEdge + spaceRightWidth + G.gap
        for (kind, w, label) in trailing {
            add(kind, row: 3, x: x, w: w, label: label, rightHalf: true)
            x += w + G.gap
        }

        // Touch targets: each half tiles from its side to half a gap past its inner key; the
        // middle between the halves is no key's.
        let top = m.stripArea - max(m.stripBottom, stripTouchBand)
        var keys: [KeyboardKey] = []
        for row in 0..<4 {
            let y0 = row == 0 ? top : rowY(row) - m.rowGap / 2
            let y1 = row == 3 ? m.totalHeight : rowY(row + 1) - m.rowGap / 2
            for half in [false, true] {
                let items = drawn.filter { $0.1 == row && $0.4 == half }.sorted { $0.2.minX < $1.2.minX }
                for (i, item) in items.enumerated() {
                    let x0 = i == 0 ? (half ? item.2.minX - G.gap / 2 : 0) : (items[i - 1].2.maxX + item.2.minX) / 2
                    let x1 = i == items.count - 1 ? (half ? width : item.2.maxX + G.gap / 2) : (item.2.maxX + items[i + 1].2.minX) / 2
                    keys.append(KeyboardKey(
                        kind: item.0,
                        row: row,
                        frame: item.2,
                        hitFrame: CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0),
                        label: item.3
                    ))
                }
            }
        }
        if placement.isInStrip {
            let height = m.stripPillHeight
            let hexWidth: CGFloat = 44
            let hx = placement == .stripLeading ? left : right - hexWidth
            let frame = CGRect(x: hx, y: m.stripTop + m.stripInset, width: hexWidth, height: height)
            let target = frame.insetBy(dx: -4, dy: -4)
            let hitFrame = CGRect(x: target.minX, y: target.minY, width: target.width, height: top - target.minY)
            keys.append(KeyboardKey(kind: .hex, row: -1, frame: frame, hitFrame: hitFrame, label: "hex"))
        }
        return (keys, SplitGeometry(leftEdge: leftEdge, rightEdge: rightEdge))
    }
}
