import UIKit

/// Simulator fixtures (`FullKeyboardSnapshotStage`): a key held down, or its accent callout
/// open with an option picked.
extension FullKeyboardView {
    func debugPress(_ character: String) {
        guard let key = engine.layout.key(.character(character)) else { return }
        engine.touchBegan(id: -1, at: CGPoint(x: key.frame.midX, y: key.frame.midY))
    }

    func debugHoldAccent(_ character: String, selecting option: String) {
        debugHoldAccent(character)
        guard let index = engine.activeAccentMenu?.options.firstIndex(of: option) else { return }
        engine.debugSelectAccent(index)
    }

    func debugHoldAccent(_ character: String, selectIndex: Int? = nil) {
        guard let key = engine.layout.key(.character(character)) else { return }
        let center = CGPoint(x: key.frame.midX, y: key.frame.midY)
        engine.touchBegan(id: -2, at: center)
        _ = engine.openAccentMenuIfEligible(forTouch: -2)
        if let selectIndex { engine.debugSelectAccent(selectIndex) }
    }
}
