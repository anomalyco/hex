import Foundation

extension DictationController {
    /// Keep mic warm's default follows the keyboard (`KeyboardBridge.warmDuration`): read again
    /// as Hex comes back and as the keyboard opens it. Where it becomes Off, an idle warm
    /// session ends.
    func refreshWarmDuration() {
        guard !OnboardingFixture.isSnapshot else { return }
        let current = keyboardBridge.warmDuration
        guard current != warmDuration else { return }
        warmDuration = current
        if current == .off, keyboardSessionEnabled, phase == .ready {
            endWarmSession()
        }
    }

    /// Settings › Dictation › Keep mic warm, unless it would say Off where the keyboard
    /// dictates by itself: a duration in effect always shows, so it can be turned off.
    var showsKeepMicWarm: Bool {
        warmDuration != .off || !keyboardBridge.keyboardDictatesItself()
    }
}
