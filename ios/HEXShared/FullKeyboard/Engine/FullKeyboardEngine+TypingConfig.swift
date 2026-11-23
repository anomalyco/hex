import CoreGraphics

/// Settings › Experiments › Typing configs: what the active config (`typingConfig`) changes in
/// the keys (`ios/TypingConfig/package/reference/typing-config.md`, "Who wins"). Its words go to
/// autocorrect on top of Learn how I type's, in memory only; its zones move what a tap presses
/// on the letters page; its aim replaces the decoder's centre for the letters it names. With no
/// config every one of these is a no-op.
@MainActor
extension FullKeyboardEngine {
    func typingConfigChanged() {
        rebuildLayout(cause: "typing_config")
        syncLearning()
        syncAdaptiveField()
    }

    /// `layout` with the active config's zones, when it has a key block for these keys.
    func zoned(_ layout: KeyboardLayout) -> KeyboardLayout {
        TypingConfigActive.record(layout)
        guard let config = typingConfig, let zones = config.keys(on: layout)?.zones, !zones.isEmpty else { return layout }
        return layout.applying(config.zoneMap(zones, on: layout))
    }

    /// The active config's aim for these keys, in points from each letter's keycap centre.
    func typingConfigAim() -> [Character: CGVector] {
        guard let aim = typingConfig?.keys(on: layout)?.aim, !aim.isEmpty else { return [:] }
        var out: [Character: CGVector] = [:]
        for (letter, aim) in aim {
            guard let key = layout.keys.first(where: { $0.character == String(letter) }) else { continue }
            out[letter] = CGVector(dx: aim.dx * key.frame.width, dy: aim.dy * key.frame.height)
        }
        return out
    }

    /// `learning` with the active config's words.
    func withTypingConfig(_ learning: TypingLearning) -> TypingLearning {
        typingConfig.map { learning.adding($0.words) } ?? learning
    }

    /// How far `point` is from `key`'s touch area, zones included, for the next-letter odds.
    func touchDistance(from point: CGPoint, to key: KeyboardKey) -> CGFloat {
        key.zoneRects?.map { Self.distance(from: point, to: $0) }.min() ?? Self.distance(from: point, to: key.hitFrame)
    }
}
