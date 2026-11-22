import CoreGraphics

/// Settings › Experiments › Comma and period keys: the keys one space bar gives up at its ends.
nonisolated extension KeyboardLayout {
    /// `space` with a letter-wide key taken out of its leading (a comma) or trailing (a period)
    /// end, and that key. nil where the bar would not still hold a letter-wide space after it.
    static func spaceBar(
        _ space: CGRect,
        takingPunctuation trailing: Bool,
        keyWidth: CGFloat,
        gap: CGFloat
    ) -> (space: CGRect, key: CGRect)? {
        guard space.width >= 2 * keyWidth + gap else { return nil }
        let remaining = space.width - keyWidth - gap
        if trailing {
            return (
                CGRect(x: space.minX, y: space.minY, width: remaining, height: space.height),
                CGRect(x: space.maxX - keyWidth, y: space.minY, width: keyWidth, height: space.height)
            )
        }
        return (
            CGRect(x: space.minX + keyWidth + gap, y: space.minY, width: remaining, height: space.height),
            CGRect(x: space.minX, y: space.minY, width: keyWidth, height: space.height)
        )
    }

    /// The joined keyboard's one space bar: a comma out of its leading end and a period out of
    /// its trailing one, where it still holds a letter-wide space after both.
    static func addCommaPeriodKeys(
        to drawn: inout [(kind: KeyKind, row: Int, frame: CGRect, label: String, shifted: String?)],
        keyWidth: CGFloat,
        gap: CGFloat
    ) {
        guard let i = drawn.firstIndex(where: { $0.kind == .space && $0.row == 3 }),
              let period = spaceBar(drawn[i].frame, takingPunctuation: true, keyWidth: keyWidth, gap: gap),
              let comma = spaceBar(period.space, takingPunctuation: false, keyWidth: keyWidth, gap: gap)
        else { return }
        drawn[i] = (.space, 3, comma.space, "space", nil)
        drawn.append((.character(","), 3, comma.key, ",", nil))
        drawn.append((.character("."), 3, period.key, ".", nil))
    }

    /// The split keyboard's two bars: a comma out of the left half's leading end and a period
    /// out of the right half's trailing one, where each still holds a letter-wide space after.
    static func addSplitCommaPeriodKeys(
        to drawn: inout [(kind: KeyKind, row: Int, frame: CGRect, label: String, rightHalf: Bool, shifted: String?)],
        keyWidth: CGFloat,
        gap: CGFloat
    ) {
        for i in drawn.indices where drawn[i].kind == .space && drawn[i].row == 3 {
            let trailing = drawn[i].rightHalf
            guard let carved = spaceBar(drawn[i].frame, takingPunctuation: trailing, keyWidth: keyWidth, gap: gap)
            else { continue }
            let mark = trailing ? "." : ","
            drawn[i] = (.space, 3, carved.space, "space", trailing, nil)
            drawn.append((.character(mark), 3, carved.key, mark, trailing, nil))
        }
    }
}
