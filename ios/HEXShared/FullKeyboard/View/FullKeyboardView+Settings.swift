import UIKit

/// The keyboard's key layout settings (Settings › Keyboard and Settings › Labs),
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
    /// for the new height. Settings › Keyboard › Key flicks (iPad) is read here too.
    func syncNumberRow() {
        let flicks = FullKeyboardSettings().keyFlicks
        if KeyboardFlicks.isEnabled != flicks {
            KeyboardFlicks.isEnabled = flicks
            plane.setNeedsDisplay()
        }
        let shows = FullKeyboardSettings().numberRow
        guard engine.showsNumberRow != shows else { return }
        plane.cancelAllTouches()
        dismissStyleMenu()
        engine.showsNumberRow = shows
        setNeedsLayout()
        onPreferredHeightChange?()
    }

    /// Settings › Labs › Join the keyboard when unfolded: the Duo unfolded upright draws
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

    /// Settings › Labs › Dvorak punctuation keys: English's Dvorak letters gain comma,
    /// period and semicolon keys. No row changes height, so the host is asked for nothing.
    func syncDvorakPunctuation() {
        let on = FullKeyboardSettings().dvorakPunctuation
        guard engine.dvorakPunctuation != on else { return }
        plane.cancelAllTouches()
        dismissStyleMenu()
        engine.dvorakPunctuation = on
        setNeedsLayout()
    }

    /// Settings › Labs › Comma and period keys: a comma and a period join the space bar
    /// on an iPhone's letters page in a text field. No row changes height, so the host is asked
    /// for nothing.
    func syncCommaPeriodKeys() {
        let on = FullKeyboardSettings().commaPeriodKeys
        guard engine.commaPeriodKeys != on else { return }
        plane.cancelAllTouches()
        dismissStyleMenu()
        engine.commaPeriodKeys = on
        setNeedsLayout()
    }
}
