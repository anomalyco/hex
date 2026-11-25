import CoreGraphics
import Foundation

/// Hex's own number pad, for the fields that ask for one (`.numberPad`, `.asciiCapableNumberPad`
/// and `.decimalPad`; iOS never gives a third-party keyboard `.phonePad` or `.namePhonePad`, or a
/// secure field, and brings its own keyboard there). Apple's pad, as a black box (iOS 26.5 and
/// 27.2 simulators, iPhone 17 Pro and SE, 2026-11-23): three columns of digits, 1 2 3 / 4 5 6 /
/// 7 8 9 / blank 0 delete, letters under 2 to 9 as on a phone, 48 pt keycaps on a 54 pt pitch;
/// upright on a 402 pt iPhone its keycaps run 4.67, 126.33, 6, 126.67, 6, 126.33, 6 pt across
/// (the middle column takes the odd pixel), on the 375 pt SE 4.5, 117.5, 6, 117.5, 6, 117.5,
/// 6; turned, 137.33 pt keycaps on a 40 pt pitch, centred, with the letters right of the digit.
/// The decimal pad puts the region's separator (`.` or `,`) in the blank corner; delete and the
/// separator have no keycap; a pressed digit greys and shows no pop-up. Apple's pad has no bar
/// above it; Hex keeps its strip (the Hex key, Paste, Code from Messages), so its pad sits in
/// the letter keys' band: the keyboard keeps its height from a text field to a number field.
nonisolated enum KeyboardNumberPad {
    /// What Apple prints under each digit.
    static func letters(for digit: String) -> String? {
        switch digit {
        case "2": "ABC"
        case "3": "DEF"
        case "4": "GHI"
        case "5": "JKL"
        case "6": "MNO"
        case "7": "PQRS"
        case "8": "TUV"
        case "9": "WXYZ"
        default: nil
        }
    }

    /// The decimal pad's separator: the region's, as Apple's pad follows it (`,` in Germany).
    static func decimalSeparator(_ locale: Locale = .current) -> String {
        locale.decimalSeparator ?? "."
    }

    /// The decimal pad's separator Hex draws; tests set it.
    nonisolated(unsafe) static var separator = decimalSeparator()

    static let gap: CGFloat = 6
    /// Turned, Apple's keycaps are this wide, centred.
    static let landscapeKeyWidth: CGFloat = 412.0 / 3
    /// On the Duo unfolded and upright Apple's pad is its split keyboard's right half: 101.33 pt
    /// keycaps, 7 pt apart, ending 80 pt from the screen's right edge (951 pt, iOS 27.2); folded,
    /// its keycaps are 7 pt apart too, on its letter keys' edges.
    static let splitKeyWidth: CGFloat = 304.0 / 3
    static let splitRightMargin: CGFloat = 80
    static let duoGap: CGFloat = 7
}

nonisolated extension KeyboardLayout {
    /// Whether this layout is Hex's number pad rather than its key rows (never on an iPad, whose
    /// number fields keep the 123 page, as Apple's iPad keyboard does).
    var isNumberPad: Bool { field.isNumberPad && metrics.pad == nil }

    /// The pad's keys: four rows in the letter keys' band (with Settings › Keyboard › Number
    /// row, its band too), Apple's columns, the Hex key in the strip, and on a Home-button
    /// iPhone, where iOS draws no globe of its own, Hex's globe in the blank corner (beside the
    /// separator on the decimal pad).
    static func buildNumberPadKeys(
        width: CGFloat,
        metrics m: KeyboardMetrics,
        orientation: KeyboardOrientation,
        posture: KeyboardPosture,
        phoneSize: KeyboardPhoneSize,
        decimal: Bool,
        showsGlobe: Bool,
        inset: CGFloat,
        hexPlacement: HexKeyPlacement,
        joinsUnfoldedKeyboard: Bool = false
    ) -> [KeyboardKey] {
        let split = posture.splitsKeyboard && orientation == .portrait && !joinsUnfoldedKeyboard
        let gap = posture == .phone ? KeyboardNumberPad.gap : KeyboardNumberPad.duoGap
        let top = m.stripArea + m.keysTop
        let bottom = m.totalHeight - m.keysBottom
        let keyHeight = (bottom - top - 3 * gap) / 4
        // Columns: Apple's upright iPhone pad runs edge to edge; turned, its keycaps are centred;
        // on the Duo split, its right half; elsewhere on the Duo, the letter keys' edges.
        var columns: [(x: CGFloat, w: CGFloat)]
        if split {
            let w = KeyboardNumberPad.splitKeyWidth
            let x0 = width - inset - KeyboardNumberPad.splitRightMargin - 3 * w - 2 * gap
            columns = (0..<3).map { (x0 + CGFloat($0) * (w + gap), w) }
        } else if orientation == .landscape || posture == .unfoldedWide {
            let w = KeyboardNumberPad.landscapeKeyWidth
            let x0 = ((width - 3 * w - 2 * gap) / 2).rounded()
            columns = (0..<3).map { (x0 + CGFloat($0) * (w + gap), w) }
        } else {
            let left = posture == .phone ? (phoneSize == .homeButton ? 4.5 : 14.0 / 3) : inset + m.side
            let right = posture == .phone ? 6 : inset + m.side
            let avail = width - left - right - 2 * gap
            let pixel: CGFloat = phoneSize == .homeButton ? 2 : 3
            let side = (avail / 3 * pixel).rounded(.down) / pixel
            columns = [(left, side), (left + side + gap, avail - 2 * side), (width - right - side, side)]
        }
        let rows: [[KeyKind?]] = [
            ["1", "2", "3"].map { .character($0) },
            ["4", "5", "6"].map { .character($0) },
            ["7", "8", "9"].map { .character($0) },
            [decimal ? .character(KeyboardNumberPad.separator) : nil, .character("0"), .delete],
        ]
        let hitTop = m.stripArea - max(m.stripBottom, stripTouchBand)
        var keys: [KeyboardKey] = []
        for (r, row) in rows.enumerated() {
            let y = top + CGFloat(r) * (keyHeight + gap)
            let y0 = r == 0 ? hitTop : y - gap / 2
            let y1 = r == 3 ? m.totalHeight : y + keyHeight + gap / 2
            for (c, kind) in row.enumerated() {
                let col = columns[c]
                let x0 = c == 0 ? 0 : col.x - gap / 2
                let x1 = c == 2 ? width : col.x + col.w + gap / 2
                let frame = CGRect(x: col.x, y: y, width: col.w, height: keyHeight)
                let hit = CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
                if r == 3, c == 0, showsGlobe {
                    // The blank corner holds the globe; on the decimal pad it shares it.
                    let half = kind == nil ? frame : CGRect(x: frame.minX, y: y, width: (col.w - gap) / 2, height: keyHeight)
                    let halfHit = kind == nil ? hit : CGRect(x: x0, y: y0, width: half.maxX + gap / 2 - x0, height: y1 - y0)
                    keys.append(KeyboardKey(kind: .globe, row: r, frame: half, hitFrame: halfHit, label: "globe"))
                    if let kind {
                        let rest = CGRect(x: half.maxX + gap, y: y, width: frame.maxX - half.maxX - gap, height: keyHeight)
                        keys.append(KeyboardKey(kind: kind, row: r, frame: rest, hitFrame: CGRect(x: halfHit.maxX, y: y0, width: x1 - halfHit.maxX, height: y1 - y0), label: KeyboardNumberPad.separator))
                    }
                    continue
                }
                guard let kind else { continue }
                let label: String = if case .character(let text) = kind { text } else { "delete" }
                keys.append(KeyboardKey(kind: kind, row: r, frame: frame, hitFrame: hit, label: label))
            }
        }
        keys.append(stripHexKey(width: width, metrics: m, inset: inset, placement: hexPlacement == .stripLeading ? .stripLeading : .stripTrailing))
        return keys
    }

    /// The Hex key in the strip, at its leading or trailing end: a bare mark in a target wider
    /// than tall, inset like the suggestion pills; in Apple's bar the same mark, centred in its
    /// cell on the words' line.
    static func stripHexKey(width: CGFloat, metrics m: KeyboardMetrics, inset: CGFloat, placement: HexKeyPlacement) -> KeyboardKey {
        let left = inset + m.side
        let right = width - inset - m.side
        let height = m.appleBar ? KeyboardMetrics.barHexKeyHeight : m.stripPillHeight
        let hexWidth: CGFloat = 44
        let cell = KeyboardMetrics.appleIconCellWidth
        let x = m.appleBar
            ? (placement == .stripLeading ? inset + (cell - hexWidth) / 2 : width - inset - (cell + hexWidth) / 2)
            : (placement == .stripLeading ? left : right - hexWidth)
        let y = m.appleBar ? m.stripTop + m.stripTextCenter - height / 2 : m.stripTop + m.stripInset
        let frame = CGRect(x: x, y: y, width: hexWidth, height: height)
        // In Apple's bar the key takes its whole cell across, up to the slot beside it.
        let target = m.appleBar
            ? CGRect(x: x - (cell - hexWidth) / 2, y: frame.minY - 4, width: cell, height: frame.height + 8)
            : frame.insetBy(dx: -4, dy: -4)
        // Up to the strip's top at least: the room above the pill belongs to the key too.
        let targetTop = min(target.minY, m.stripTop)
        let top = m.stripArea - max(m.stripBottom, stripTouchBand)
        let hitFrame = CGRect(x: target.minX, y: targetTop, width: target.width, height: top - targetTop)
        return KeyboardKey(kind: .hex, row: -1, frame: frame, hitFrame: hitFrame, label: "hex")
    }
}
