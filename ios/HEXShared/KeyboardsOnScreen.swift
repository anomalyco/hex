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
        /// A newly loaded keyboard this buried one reappeared underneath when its host reshowed
        /// with a fresh `documentIdentifier`.
        var under: Int?
    }

    /// In the order they appeared.
    private var entries: [Entry] = []

    var count: Int { entries.count }

    /// `keyboard` came on screen in `field`, after every keyboard already there.
    mutating func appeared(_ keyboard: Int, in field: UUID?) {
        entries.removeAll { $0.keyboard == keyboard }
        entries.append(Entry(keyboard: keyboard, field: field))
    }

    /// Registers `keyboard` as `viewWillAppear` runs, keeping an already-superseded keyboard in
    /// place (or marking a previously superseded one as sitting under `preparingKeyboard`, which
    /// UIKit loads before calling `viewWillAppear` on buried controllers during a reshow).
    /// Returns true when `keyboard` is superseded and should skip mounting.
    mutating func willAppear(_ keyboard: Int, in field: UUID?, under preparingKeyboard: Int? = nil) -> Bool {
        if let index = entries.firstIndex(where: { $0.keyboard == keyboard }) {
            if let field { entries[index].field = field }
            if let preparingKeyboard, preparingKeyboard != keyboard { entries[index].under = preparingKeyboard }
            if isSuperseded(keyboard) || preparingKeyboard != nil { return true }
        } else if let preparingKeyboard, preparingKeyboard != keyboard {
            entries.append(Entry(keyboard: keyboard, field: field, under: preparingKeyboard))
            return true
        }
        appeared(keyboard, in: field)
        return false
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

    /// Whether a later keyboard is currently on screen in `keyboard`'s own named field: `keyboard`
    /// sits underneath it and can retire its surface until no later keyboard remains in that field.
    func isSuperseded(_ keyboard: Int) -> Bool {
        guard let index = entries.firstIndex(where: { $0.keyboard == keyboard }) else { return false }
        let entry = entries[index]
        return entries[(index + 1)...].contains { later in
            (entry.field != nil && later.field == entry.field) || later.keyboard == entry.under
        }
    }
}

/// Single-owner bookkeeping for the keyboard process's shared capture web view. Only the
/// current owner—or any caller while the capture is parked with no owner—may park, evict on a
/// memory warning, or tear down the shared web view; a former owner that still held a pointer
/// drops only its own reference so an active recording on a newer keyboard is never disturbed.
nonisolated struct KeyboardCaptureOwnership: Equatable, Sendable {
    private(set) var hasSharedCapture = false
    private(set) var owner: Int?
    private(set) var isCapturing = false

    /// Whether `requester` may attach or start the shared capture. False while another keyboard
    /// is actively recording with it.
    func canAcquire(by requester: Int) -> Bool {
        !isCapturing || owner == requester
    }

    /// `requester` attached the shared capture while idle (creating it if needed).
    mutating func acquireIdle(by requester: Int) {
        hasSharedCapture = true
        owner = requester
        isCapturing = false
    }

    /// `requester` armed or started microphone capture on the shared web view.
    mutating func beginCapture(by requester: Int) {
        hasSharedCapture = true
        owner = requester
        isCapturing = true
    }

    /// `requester`'s capture stopped, failed or was discarded while its keyboard stays on screen.
    mutating func endCapture(by requester: Int) {
        guard owner == requester else { return }
        isCapturing = false
    }

    /// `requester`'s keyboard hid: true when `requester` owned the shared capture (or nobody did),
    /// so the caller detaches and parks it; false when another keyboard now owns it.
    mutating func park(by requester: Int) -> Bool {
        guard hasSharedCapture, owner == nil || owner == requester else { return false }
        owner = nil
        isCapturing = false
        return true
    }

    /// `requester` deallocated: true when `requester` still owned the shared capture (or nobody
    /// did), so the caller stops, detaches and parks it; false when another keyboard owns it.
    mutating func ownerDidDeinit(_ requester: Int) -> Bool {
        guard hasSharedCapture, owner == nil || owner == requester else { return false }
        owner = nil
        isCapturing = false
        return true
    }

    /// `requester` received a memory warning while idle: true only when the shared capture is not
    /// capturing and either nobody owns it or `requester` itself owns it.
    mutating func evictForMemoryWarning(by requester: Int) -> Bool {
        guard hasSharedCapture, !isCapturing, owner == nil || owner == requester else { return false }
        hasSharedCapture = false
        owner = nil
        return true
    }

    /// WebKit's content process terminated under `requester` (or `nil` while parked).
    mutating func dropTerminated(by requester: Int?) -> Bool {
        guard hasSharedCapture, requester == nil || owner == nil || owner == requester else { return false }
        hasSharedCapture = false
        owner = nil
        isCapturing = false
        return true
    }
}
