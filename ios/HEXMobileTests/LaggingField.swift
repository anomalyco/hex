import UIKit
@testable import HEX

/// A host whose proxy lags `insertText`: `documentContextBeforeInput` keeps returning what it
/// had before the last insert (`.behind`), nothing (`.nothing`) or an empty string
/// (`.empty`), as web views and some composers do, while the text takes every insert.
@MainActor
final class LaggingField: FixtureTextTarget {
    enum Lag: CaseIterable {
        case behind, nothing, empty
    }

    var lag: Lag = .behind
    private var reported: String?

    override var documentContextBeforeInput: String? {
        switch lag {
        case .behind: return reported
        case .nothing: return nil
        case .empty: return ""
        }
    }

    override func insertText(_ inserted: String) {
        // What the host reports before this insert is what it keeps reporting after it,
        // except that one that is a single insert behind reports the text before this one.
        reported = lag == .behind ? super.documentContextBeforeInput : documentContextBeforeInput
        super.insertText(inserted)
    }
}
