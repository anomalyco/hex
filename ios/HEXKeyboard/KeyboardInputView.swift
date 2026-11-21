import UIKit

/// The keyboard's root view. `UIDevice.playInputClick()` plays the key click only while the
/// visible input view adopts `UIInputViewAudioFeedback` and enables it (UIKit's docs for
/// both): a subview that adopts it, as the full keyboard's did, plays nothing. The click then
/// follows Settings › Sounds & Haptics › Keyboard Feedback › Sound, as the system keyboard's
/// does, and like any keyboard sound needs Allow Full Access (Apple's custom keyboard guide).
final class KeyboardInputView: UIInputView, UIInputViewAudioFeedback {
    var enableInputClicksWhenVisible: Bool { true }
}
