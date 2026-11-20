import CoreGraphics
import Foundation

/// The full keyboard's Hex key as a pure state machine, also the dictation bar's push to
/// talk. A tap dictates (Done finishes it); a still hold of `holdDuration` dictates while
/// held, and releasing finishes it (push to talk). Moving more than `slop` before either
/// opens the style menu, and the release picks the row under the finger. Once the hold
/// fires, moves are ignored.
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
        case tap
        case hold
        case openMenu
        case dragMenu
        case releaseMenu
        /// The finger lifted after a hold: finish the dictation it started.
        case release
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
        let output: Output = switch state {
        case .menu: .releaseMenu
        case .pressing: .tap
        case .fired: .release
        case .idle: .ignored
        }
        state = .idle
        return output
    }

    mutating func cancel() {
        state = .idle
    }
}
