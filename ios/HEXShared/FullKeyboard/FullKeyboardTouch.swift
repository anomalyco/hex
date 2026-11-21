import QuartzCore
import UIKit

/// The full keyboard's touches, as data: one finger's key, where it started and where it is,
/// and the state that makes a press an accent picker, a slide, a trackpad or a language
/// swipe. Kept beside the engine, like `SwipedWord`, so the engine's own file stays readable.
@MainActor
extension FullKeyboardEngine {
    /// One finger on the keys.
    struct ActiveTouch: Equatable {
        var key: KeyboardKey
        var startPoint: CGPoint
        var point: CGPoint
        /// A character that already went in because another key went down after it.
        var committed: Bool
        /// The page a 123/ABC press came from; releasing over a character returns there.
        var returnLayer: KeyboardLayer?
        /// A 123, #+= or shift press slid onto this character key: it shows the key's pop-up,
        /// and letting go types it (`slideTarget`), as on Apple's keyboard.
        var slideKey: KeyboardKey?
        /// True once such a press slid onto a character key, so letting go back on the page
        /// key is a slide taken back rather than a tap.
        var hasSlid = false
        /// True once a spacebar press is held `trackpadHoldDelay` (`holdSpace`).
        var isTrackpad = false
        /// True once a spacebar press slides before its hold (`holdSpace`) while it switches
        /// language: letting go far enough along switches instead of typing a space.
        var isLanguageSwipe = false
        /// The trackpad finger's last few moves, for its speed (`TrackpadCursor.speed(along:)`).
        var trackpadPath: [TrackpadCursor.Sample] = []
        /// The text the trackpad moves the cursor through (`TrackpadCursor.State`).
        var trackpad: TrackpadCursor.State?
        /// UTF-16 units the trackpad has moved the cursor, for Record typing logs.
        var trackpadOffset = 0
        /// True when the likelier next letter claimed this touch from the key under it; a
        /// small move then keeps it rather than sliding back (`letterReach`).
        var claimed = false
        /// Where the finger went down, before a strip touch moved onto row 1 (Adaptive touch).
        var downPoint: CGPoint = .zero
    }

    /// A press that types the character key it slides onto: 123, ABC and #+= (with a page to
    /// go back to), and shift.
    static func slidesToType(_ touch: ActiveTouch) -> Bool {
        touch.returnLayer != nil || touch.key.kind == .shift
    }

    /// A language swipe's direction, or nil when it was too short or more up than along.
    static func languageSwipeStep(from start: CGPoint, to end: CGPoint) -> Int? {
        let dx = end.x - start.x
        guard abs(dx) >= languageSwipeDistance, abs(dx) >= abs(end.y - start.y) else { return nil }
        return dx < 0 ? 1 : -1
    }

    /// How far `point` is from `rect`: zero inside it.
    static func distance(from point: CGPoint, to rect: CGRect) -> CGFloat {
        let dx = max(rect.minX - point.x, 0, point.x - rect.maxX)
        let dy = max(rect.minY - point.y, 0, point.y - rect.maxY)
        return (dx * dx + dy * dy).squareRoot()
    }
}
