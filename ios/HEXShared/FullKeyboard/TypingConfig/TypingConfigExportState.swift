import CoreGraphics

/// What an export's `state/phone.json` says about the keyboard a config would be for
/// (`ios/TypingConfig/package/reference/state.md`, `layout`): its context, the letter
/// keycap's size, the keys' touch area and every key's keycap and touch target. The package's
/// scripts map logged taps onto these keys and compile zones against them as Hex does.
nonisolated enum TypingConfigExportState {
    /// `formats`: the typing-config version this Hex reads and its touch areas' version.
    static var formats: [String: Any] {
        ["typingConfig": TypingConfigLimits.version, "hitModel": KeyboardZone.hitModel, "logs": 1]
    }

    /// `layout` for `state/phone.json`, from a letters layout.
    static func layout(_ layout: KeyboardLayout) -> [String: Any] {
        func rect(_ r: CGRect) -> [Double] { [r.minX, r.minY, r.width, r.height].map { (Double($0) * 100).rounded() / 100 } }
        let c = TypingConfigFile.Context(layout: layout)
        let context: [String: Any] = [
            "width": c.width, "orientation": c.orientation, "posture": c.posture, "language": c.language,
            "letters": c.letters, "split": c.split, "hexPlacement": c.hexPlacement, "numberRow": c.numberRow,
            "keyHeight": c.keyHeight, "rowPitch": c.rowPitch, "letterWidth": c.letterWidth, "hitModel": c.hitModel,
        ]
        let keys: [[String: Any]] = layout.keys.map { key in
            var k: [String: Any] = ["id": TypingLogger.keyID(key.kind), "row": key.row, "frame": rect(key.frame), "hitFrame": rect(key.hitFrame)]
            if let gap = key.gapHitFrame { k["gapHitFrame"] = rect(gap) }
            return k
        }
        return [
            "context": context, "letterWidth": Double(layout.letterWidth), "keyHeight": Double(layout.metrics.keyHeight),
            "gap": Double(layout.metrics.gap), "rowGap": Double(layout.metrics.rowGap),
            "touchArea": rect(layout.touchAreaFrame), "keys": keys,
        ]
    }
}
