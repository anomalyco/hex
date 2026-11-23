import CoreGraphics

/// One moved edge of a key's touch area (a typing config's zone, `ios/TypingConfig/package/
/// reference/typing-config.md`): `move` letter keycaps outward from Hex's own edge (negative
/// gives a strip to the neighbours across it).
nonisolated struct KeyboardZone: Equatable, Sendable {
    enum Edge: String, Equatable, Sendable, CaseIterable {
        case top, bottom, left, right
    }

    /// The version of Hex's default touch areas a zone is measured from. Bump it whenever
    /// `KeyboardLayout.buildKeys` moves an edge (as #2379 gave the gap above the space bar to
    /// it): a stored zone then no longer means what it was checked as, and readers skip it.
    static let hitModel = 1

    /// The key's id as typing logs name it (`TypingLogger.keyID`): a letter, `space`, …
    var key: String
    var edge: Edge
    var move: CGFloat
}
