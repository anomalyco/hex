import UIKit

/// The keyboard's root view. `UIDevice.playInputClick()` plays the key click only while the
/// visible input view adopts `UIInputViewAudioFeedback` and enables it (UIKit's docs for
/// both): a subview that adopts it, as the full keyboard's did, plays nothing. The click then
/// follows Settings › Sounds & Haptics › Keyboard Feedback › Sound, as the system keyboard's
/// does, and like any keyboard sound needs Allow Full Access (Apple's custom keyboard guide).
final class KeyboardInputView: UIInputView, UIInputViewAudioFeedback {
    var enableInputClicksWhenVisible: Bool { true }

    private struct WeakView {
        weak var view: UIView?
    }

    private var mountedByHex: [WeakView] = []

    /// Records a subview Hex itself mounted (`surface`, `backdrop`, `barEmojiPanel`) so
    /// `unmountHexViews()` removes only those exact views by reference—never any view UIKit
    /// adds inside `UIInputView`.
    func noteHexSubview(_ subview: UIView) {
        mountedByHex.removeAll { $0.view == nil || $0.view === subview }
        mountedByHex.append(WeakView(view: subview))
    }

    func unmountHexViews() {
        for entry in mountedByHex {
            entry.view?.removeFromSuperview()
        }
        mountedByHex.removeAll()
    }
}
