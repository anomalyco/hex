import Foundation

/// Whether the keyboard is showing because you switched to it from another keyboard, which iOS
/// never says: a keyboard just appears, whether a field opened with it already chosen or you
/// picked it with the globe key. A guess from when the keyboard last hid: it comes back in
/// the same field (UIKit's `documentIdentifier`), and that field's app didn't go to the
/// background in between (opening Hex, going home, another app). So switching away and back
/// in one field counts; a field that opens with Hex's keyboard already chosen is another
/// document, and so is the first switch to Hex in a field another keyboard opened, which this
/// misses. Not yet proven on a device.
nonisolated struct KeyboardSwitchGuess: Equatable {
    /// The field the keyboard was in when it last hid, and when.
    var hiddenDocument: UUID?
    var hiddenAt: TimeInterval?
    /// When the field's app last went to the background.
    var hostLeftAt: TimeInterval?

    /// The background notice and the hide can come in either order as the app leaves.
    static let hostLeaveSlack: TimeInterval = 2

    func isSwitchBack(to document: UUID?) -> Bool {
        guard let document, let hiddenDocument, let hiddenAt, document == hiddenDocument else { return false }
        if let hostLeftAt, hostLeftAt >= hiddenAt - Self.hostLeaveSlack { return false }
        return true
    }
}

/// `KeyboardSwitchGuess` in the keyboard's own defaults, outliving its process.
nonisolated struct KeyboardSwitchGuessStore {
    private enum Key {
        static let hiddenDocument = "keyboard.switch.hiddenDocument"
        static let hiddenAt = "keyboard.switch.hiddenAt"
        static let hostLeftAt = "keyboard.switch.hostLeftAt"
    }

    private let defaults: UserDefaults

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults
    }

    var guess: KeyboardSwitchGuess {
        KeyboardSwitchGuess(
            hiddenDocument: defaults.string(forKey: Key.hiddenDocument).flatMap(UUID.init(uuidString:)),
            hiddenAt: defaults.object(forKey: Key.hiddenAt) as? TimeInterval,
            hostLeftAt: defaults.object(forKey: Key.hostLeftAt) as? TimeInterval
        )
    }

    func keyboardHid(in document: UUID?, at time: TimeInterval) {
        defaults.set(document?.uuidString, forKey: Key.hiddenDocument)
        defaults.set(time, forKey: Key.hiddenAt)
    }

    func hostLeft(at time: TimeInterval) {
        defaults.set(time, forKey: Key.hostLeftAt)
    }
}
