import CoreGraphics
import Foundation

/// The full keyboard's Hex key as a pure state machine, so a stray tap while typing
/// does nothing. A still hold of `holdDuration` fires the hold action (dictate, or open
/// Hex); moving more than `slop` before that opens the style menu, and the release
/// picks the row under the finger. Once the hold fires, moves and the release are ignored.
nonisolated struct HexKeyGesture: Equatable, Sendable {
    static let holdDuration: TimeInterval = 0.35
    static let slop: CGFloat = 10

    enum State: Equatable, Sendable {
        case idle
        case pressing
        case menu
        case fired
    }

    enum Output: Equatable, Sendable {
        case ignored
        case hold
        case openMenu
        case dragMenu
        case releaseMenu
    }

    private(set) var state: State = .idle
    private var origin: CGPoint = .zero

    mutating func touchDown(at point: CGPoint) {
        state = .pressing
        origin = point
    }

    mutating func touchMoved(to point: CGPoint) -> Output {
        switch state {
        case .pressing:
            guard hypot(point.x - origin.x, point.y - origin.y) > Self.slop else { return .ignored }
            state = .menu
            return .openMenu
        case .menu:
            return .dragMenu
        case .idle, .fired:
            return .ignored
        }
    }

    /// The hold timer started at touch-down ran out.
    mutating func holdElapsed() -> Output {
        guard state == .pressing else { return .ignored }
        state = .fired
        return .hold
    }

    mutating func touchUp() -> Output {
        let output: Output = state == .menu ? .releaseMenu : .ignored
        state = .idle
        return output
    }

    mutating func cancel() {
        state = .idle
    }
}
