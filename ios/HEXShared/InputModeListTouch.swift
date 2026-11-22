import UIKit

/// Hands a key's own touches to UIKit's `handleInputModeList(from:with:)` as its real events, as
/// Apple documents: a tap switches to the next keyboard, a hold (0.2 s) or a slide up opens the
/// list, and lifting over the list picks that keyboard. The full keyboard's globe and Code from
/// Messages chip and the dictation bar's globe all go through here.
///
/// Where Apple's keyboard is split (the Duo unfolded and upright), a lift on the key that never
/// left it isn't passed on, so UIKit's hold opens the list instead of switching to the next
/// keyboard. iOS 27.2's switch to the next keyboard can end the app being typed in there
/// (TestFlight build 121, iPhone Duo: `-[UIKeyboardLayoutStar activationIndicatorView]` reads
/// one key past the end of a split keyboard not laid out yet, to show its language); a pick
/// from the list doesn't show that indicator.
@MainActor
final class InputModeListTouch {
    /// This touch went off the key before lifting.
    private var leftKey = false

    /// `alwaysHoldsBackTap`: a tap opens the list everywhere (Code from Messages, where the next
    /// keyboard could be Emoji). Otherwise only where Apple's keyboard may be split: the Duo
    /// unfolded and upright, or its posture not known yet.
    func forward(_ sender: UIView, with event: UIEvent, to controller: UIInputViewController, alwaysHoldsBackTap: Bool = false) {
        if let touch = event.touches(for: sender)?.first {
            let holdsBackTap = alwaysHoldsBackTap || KeyboardPosture.current(in: sender.window)?.splitsKeyboard ?? true
            let onKey = sender.bounds.contains(touch.location(in: sender))
            if touch.phase == .began { leftKey = false }
            if touch.phase == .moved, !onKey { leftKey = true }
            guard Self.passes(phase: touch.phase, onKey: onKey, leftKey: leftKey, appleKeyboardSplits: holdsBackTap) else { return }
        }
        controller.handleInputModeList(from: sender, with: event)
    }

    static func passes(phase: UITouch.Phase, onKey: Bool, leftKey: Bool, appleKeyboardSplits: Bool) -> Bool {
        !(appleKeyboardSplits && phase == .ended && onKey && !leftKey)
    }
}

extension KeyboardPosture {
    /// The posture as `window` shows it now; nil on the Duo before the keyboard has a window,
    /// where Hex takes Apple's keyboard to be split.
    @MainActor static func current(in window: UIWindow?) -> KeyboardPosture? {
        guard FullKeyboardView.isFoldable else { return .phone }
        guard let screen = window?.windowScene?.screen ?? window?.screen else { return nil }
        return current(screenSize: screen.bounds.size, isFoldable: true)
    }
}
