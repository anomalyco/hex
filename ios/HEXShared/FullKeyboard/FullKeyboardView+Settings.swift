import UIKit

/// The keyboard's key layout settings (Settings › Keyboard and Settings › Experiments),
/// applied on every sync: where the Hex key sits, the number row, and whether the Duo unfolded
/// draws one keyboard. Switches that only change typing behaviour stay in
/// `FullKeyboardView.syncSettings`.
@MainActor
extension FullKeyboardView {
    /// Settings › Keyboard › Hex key: Right (the strip's trailing end) or Left (its leading
    /// end), kept in `HexKeyPlacement.current` too for typing logs.
    func syncHexPlacement() {
        let placement = hexPlacementOverride ?? FullKeyboardSettings().hexKeyPlacement
        HexKeyPlacement.current = placement
        guard engine.hexPlacement != placement else { return }
        plane.cancelAllTouches()
        dismissStyleMenu()
        engine.hexPlacement = placement
        setNeedsLayout()
    }

    /// Settings › Keyboard › Number row: the keyboard grows by the row, so the host is asked
    /// for the new height.
    func syncNumberRow() {
        let shows = FullKeyboardSettings().numberRow
        guard engine.showsNumberRow != shows else { return }
        plane.cancelAllTouches()
        dismissStyleMenu()
        engine.showsNumberRow = shows
        setNeedsLayout()
        onPreferredHeightChange?()
    }

    /// Settings › Experiments › Join the keyboard when unfolded: the Duo unfolded upright draws
    /// one keyboard rather than the split one. The rows keep their heights, so the host is asked
    /// for nothing.
    func syncJoinsUnfoldedKeyboard() {
        let joins = FullKeyboardSettings().joinsKeyboardWhenUnfolded
        guard engine.joinsUnfoldedKeyboard != joins else { return }
        plane.cancelAllTouches()
        dismissStyleMenu()
        engine.joinsUnfoldedKeyboard = joins
        setNeedsLayout()
    }
}
