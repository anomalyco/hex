import CoreGraphics

/// The docked iPad's own rows: Apple's iPad mini keyboard (iPadOS 27.2 simulator, black box),
/// on every iPad until the 11- and 13-inch get their own. Delete ends row 1 and return row 2,
/// a shift key opens and closes row 3, with Apple's two-symbol keys after the letters, and
/// the bottom row is the globe, 123, the Hex key where Apple's dictation key is, the space bar,
/// 123 (or emoji) and the hide-keyboard key. Key heights and rows come from `KeyboardMetrics.pad`;
/// widths follow Apple's: letter keys as wide as fits the longest row with its delete or return
/// at least 1.236 and 1.24 letters wide (English 54.5 pt, French 53.25 pt on the mini upright),
/// row 2 set in 0.4 of a key pitch (0.2 where it holds as many letters as row 1, as AZERTY's
/// does), the left shift a letter wide (1.061 on AZERTY), and delete, return and the right
/// shift taking what's left.
extension KeyboardLayout {
    /// Apple's row 3 two-symbol keys: the lower one types, the upper one with shift.
    static func padPairs(letters rowThree: [String], layer: KeyboardLayer) -> [(String, String?)] {
        switch layer {
        case .numbers:
            return ["%", "-", "+", "=", "/", ";", ":"].map { ($0, nil) } + [(",", "!"), (".", "?")]
        case .symbols:
            return ["§", "|", "~", "…", "\\", "<", ">", "!", "?"].map { ($0, nil) }
        case .letters:
            switch rowThree.count {
            // English, German, QWERTY and QWERTZ: , and ! then . and ?
            case 7: return rowThree.map { ($0, nil) } + [(",", "!"), (".", "?")]
            // AZERTY: , and ? then . and ! then ' and : (typed with shift: : , iPadOS 27.2).
            case 6: return rowThree.map { ($0, nil) } + [(",", "?"), (".", "!"), ("'", ":")]
            default: return rowThree.map { ($0, nil) }
            }
        }
    }

    static func padRows(layer: KeyboardLayer, language: KeyboardLanguage, letters: KeyboardLetterLayout) -> (top: [String], middle: [String], bottom: [(String, String?)]) {
        let digits = "1234567890".map(String.init)
        switch layer {
        case .letters:
            // The iPhone's French apostrophe key is the iPad's ' and : pair key (`padPairs`).
            let rows = language.letterRows(letters).map { language.isFrench ? $0.filter { $0 != FrenchTyping.key } : $0 }
            return (rows[0], rows[1], padPairs(letters: rows[2], layer: .letters))
        case .numbers:
            return (digits, ["@", "#", "$", "&", "*", "(", ")", "'", "\""], padPairs(letters: [], layer: .numbers))
        case .symbols:
            return (digits, ["€", "£", "¥", "_", "^", "[", "]", "{", "}"], padPairs(letters: [], layer: .symbols))
        }
    }

    /// Apple's minimums for the key that ends a row: delete 1.236 letters, return 1.24.
    static let padDeleteRatio: CGFloat = 1.236
    static let padReturnRatio: CGFloat = 1.24

    /// The letter key's width on a docked iPad for these rows across `innerWidth`.
    static func padLetterWidth(innerWidth: CGFloat, metrics m: KeyboardMetrics, layer: KeyboardLayer, language: KeyboardLanguage, letters: KeyboardLetterLayout) -> CGFloat {
        let rows = padRows(layer: layer, language: language, letters: letters)
        let available = innerWidth - 2 * m.side
        let g = m.gap
        let n1 = CGFloat(rows.top.count), n2 = CGFloat(rows.middle.count), n3 = CGFloat(rows.bottom.count)
        let indent: CGFloat = n2 < n1 ? 0.4 : 0.2
        let shift = padShiftRatio(language: language, letters: letters, layer: layer)
        // Row 1: n1 letters and delete; row 2: the indent, n2 letters and return; row 3: shift,
        // n3 keys and a shift as wide as delete at least.
        let one = (available - n1 * g) / (n1 + padDeleteRatio)
        let two = (available - (n2 + indent) * g) / (n2 + indent + padReturnRatio)
        let three = (available - (n3 + 1) * g) / (shift + n3 + padDeleteRatio)
        // Never wider than ten letters and delete make them (Dvorak's seven-letter row 1).
        let ten = (available - 10 * g) / (10 + padDeleteRatio)
        return min(one, two, three, ten)
    }

    static func padShiftRatio(language: KeyboardLanguage, letters: KeyboardLetterLayout, layer: KeyboardLayer) -> CGFloat {
        layer == .letters && language.offered(letters) == .azerty ? 1.061 : 1
    }

    static func buildPadKeys(
        width: CGFloat,
        metrics m: KeyboardMetrics,
        orientation: KeyboardOrientation,
        layer: KeyboardLayer,
        field: KeyboardFieldKind,
        showsGlobe: Bool,
        showsEmoji: Bool,
        inset: CGFloat,
        language: KeyboardLanguage,
        letters: KeyboardLetterLayout
    ) -> [KeyboardKey] {
        let innerWidth = width - 2 * inset
        let rows = padRows(layer: layer, language: language, letters: letters)
        let lw = padLetterWidth(innerWidth: innerWidth, metrics: m, layer: layer, language: language, letters: letters)
        let g = m.gap, pitch = lw + g
        let left = inset + m.side, right = width - inset - m.side

        func rowY(_ row: Int) -> CGFloat { m.stripArea + m.keysTop + CGFloat(row) * (m.keyHeight + m.rowGap) }
        var drawn: [(kind: KeyKind, row: Int, frame: CGRect, label: String, shifted: String?)] = []
        func add(_ kind: KeyKind, _ row: Int, _ x: CGFloat, _ w: CGFloat, _ label: String, shifted: String? = nil) {
            drawn.append((kind, row, CGRect(x: x, y: rowY(row), width: w, height: m.keyHeight), label, shifted))
        }

        // Row 1: the top row, then delete to the edge.
        var x = left
        for ch in rows.top { add(.character(ch), 0, x, lw, ch); x += pitch }
        add(.delete, 0, x, right - x, "delete")
        // Row 2: set in, then return to the edge.
        x = left + (rows.middle.count < rows.top.count ? 0.4 : 0.2) * pitch
        for ch in rows.middle { add(.character(ch), 1, x, lw, ch); x += pitch }
        add(.returnKey, 1, x, right - x, "return")
        // Row 3: shift (or the page switch), the letters and two-symbol keys, the other shift.
        let shiftW = padShiftRatio(language: language, letters: letters, layer: layer) * lw
        let page: (KeyKind, String) = switch layer {
        case .letters: (.shift, "shift")
        case .numbers: (.layerSwitch(.symbols), "#+=")
        case .symbols: (.layerSwitch(.numbers), "123")
        }
        add(page.0, 2, left, shiftW, page.1)
        x = left + shiftW + g
        for (ch, upper) in rows.bottom { add(.character(ch), 2, x, lw, ch, shifted: upper); x += pitch }
        add(page.0, 2, x, right - x, page.1)

        // Row 4: globe, 123 or ABC, the Hex key (Apple's dictation key), space, the field's
        // extras, 123/ABC or emoji, and hide. Apple's small keys are 1.046 letters wide upright
        // and 1.02 turned, its right two 1.679 and 1.61.
        // The same in every language: from row 1's letter with ten letters and delete.
        let reference = (innerWidth - 2 * m.side - 10 * g) / (10 + padDeleteRatio)
        let landscape = orientation == .landscape
        let small = (landscape ? 1.02 : 1.046) * reference
        let wide = (landscape ? 1.61 : 1.679) * reference
        let pageKey: (KeyKind, String) = layer == .letters ? (.layerSwitch(.numbers), "123") : (.layerSwitch(.letters), "ABC")
        var lead: [(KeyKind, CGFloat, String)] = []
        if showsGlobe { lead.append((.globe, small, "globe")) }
        lead.append((pageKey.0, small, pageKey.1))
        lead.append((.hex, small, "hex"))
        // Apple's docked web search row is its text row, `.` staying on row 3 (iPad mini, iOS 27.2).
        var trail = field == .webSearch ? [] : field.keysRightOfSpace(width: lw)
        trail.append(showsEmoji && layer == .letters ? (.emoji, wide, "emoji") : (pageKey.0, wide, pageKey.1))
        trail.append((.dismiss, wide, "dismiss"))
        x = left
        for (kind, w, label) in lead { add(kind, 3, x, w, label); x += w + g }
        let trailW = trail.reduce(0) { $0 + $1.1 } + CGFloat(trail.count) * g
        add(.space, 3, x, max(lw, right - trailW - x), "space")
        x = right - trailW + g
        for (kind, w, label) in trail { add(kind, 3, x, w, label); x += w + g }

        // Targets: every gap split down the middle, the outer keys to the edges, row 1 up to the
        // strip's band and row 4 to the keyboard's bottom, as the iPhone's (`buildKeys`).
        let top = m.stripArea - max(m.stripBottom, stripTouchBand)
        var keys: [KeyboardKey] = []
        for row in 0..<4 {
            let rowKeys = drawn.filter { $0.row == row }.sorted { $0.frame.minX < $1.frame.minX }
            let y0 = row == 0 ? top : rowY(row) - m.rowGap / 2
            let y1 = row == 3 ? m.totalHeight : rowY(row + 1) - m.rowGap / 2
            for (i, item) in rowKeys.enumerated() {
                let x0 = i == 0 ? 0 : (rowKeys[i - 1].frame.maxX + item.frame.minX) / 2
                let x1 = i == rowKeys.count - 1 ? width : (item.frame.maxX + rowKeys[i + 1].frame.minX) / 2
                var hit = CGRect(x: x0, y: y0, width: x1 - x0, height: y1 - y0)
                if item.kind == .emoji {
                    hit = CGRect(x: item.frame.minX + 6, y: item.frame.minY + 7.5, width: item.frame.width - 12, height: y1 - item.frame.minY - 7.5)
                }
                keys.append(KeyboardKey(kind: item.kind, row: row, frame: item.frame, hitFrame: hit, label: item.label, shifted: item.shifted))
            }
        }
        KeyboardFlicks.add(to: &keys, layer: layer, language: language)
        return keys
    }
}
