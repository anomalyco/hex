import UIKit

/// The one edit notification: kept beside the engine like the trackpad's own code so
/// `FullKeyboardEngine.swift` stays readable.
@MainActor
extension FullKeyboardEngine {
    func didEdit() {
        suggestionSpaceBefore = nil
        // A key's own edit means a pending Dictate no longer has the field it picked.
        trackpadSelectionState.pendingDictation = nil
        refreshAutoCapitalization()
        refreshSuggestions()
        onEdit?()
    }
}
