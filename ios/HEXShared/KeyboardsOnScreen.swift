import Foundation

/// Hex's keyboards that iOS has on screen at once. It should be one.
///
/// iOS makes a new input view controller each time the keyboard comes up, tells the one before
/// that it disappeared, and frees it. In one case it doesn't (iOS 27.0 and 27.2 simulators,
/// every time): a field that takes focus as its screen appears and has an input accessory
/// view, opened with Hex. There, switching to Apple's keyboard never tells Hex it disappeared
/// and keeps its remote view in the app's view tree; Apple's keyboard is added beside it, and
/// switching back puts a second Hex on top without taking Apple's away (a
/// `UIKeyboardAutomatic` between two remote views in `_UIKBCompatInputView`). Hex's keys are
/// translucent on iOS's glass, so Apple's keys showed through a row above its own (TestFlight
/// build 56, iPhone 17, iOS 27.0.1). Opened with Apple's keyboard, the same field hides and
/// frees Hex as usual, and nothing is left behind.
///
/// A keyboard can't see the app's views, but it can see this: an earlier Hex keyboard that
/// was never told to disappear, still on screen in the same field. That is exactly when
/// Apple's keys may be under the new one, and the only time Hex draws its backdrop
/// (`KeyboardBackdropView`). Otherwise Hex draws nothing behind its keys and is iOS's glass.
nonisolated struct KeyboardsOnScreen: Equatable, Sendable {
    private struct Entry: Equatable, Sendable {
        let keyboard: Int
        /// The field the keyboard is in, nil until UIKit names it.
        var field: UUID?
    }

    /// In the order they appeared.
    private var entries: [Entry] = []

    var count: Int { entries.count }

    /// `keyboard` came on screen in `field`, after every keyboard already there.
    mutating func appeared(_ keyboard: Int, in field: UUID?) {
        entries.removeAll { $0.keyboard == keyboard }
        entries.append(Entry(keyboard: keyboard, field: field))
    }

    /// UIKit named `keyboard`'s field, or it moved to another. Nothing for a keyboard off screen.
    mutating func moved(_ keyboard: Int, to field: UUID?) {
        guard let index = entries.firstIndex(where: { $0.keyboard == keyboard }) else { return }
        entries[index].field = field
    }

    /// iOS said `keyboard` disappeared, or freed it.
    mutating func disappeared(_ keyboard: Int) {
        entries.removeAll { $0.keyboard == keyboard }
    }

    /// Whether an earlier keyboard is still on screen in `keyboard`'s field: iOS kept it, and
    /// with it whatever it showed in between. A field UIKit hasn't named counts as the same
    /// one, so the doubt covers.
    func isOverAnother(_ keyboard: Int) -> Bool {
        guard let index = entries.firstIndex(where: { $0.keyboard == keyboard }) else { return false }
        let field = entries[index].field
        return entries[..<index].contains { $0.field == nil || field == nil || $0.field == field }
    }
}
