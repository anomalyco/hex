import UIKit

/// Delete after an autocorrection, as on Apple's keyboard (black box, iOS 27.2): delete takes
/// only the space or mark after the corrected words, and Apple's text view then shows what you
/// typed in a bubble with an undo arrow. A keyboard can't draw in the text, so the strip's left
/// slot offers it instead, highlighted with the same arrow (`KeyboardSuggestion.Kind.revert`);
/// tapping it puts back what you typed, without the space, and space after that keeps it. The
/// offer comes back whenever the cursor is at the end of the latest autocorrection again
/// (deleting back to it), as Apple's bubble does, and goes once a newer one replaces it.
@MainActor
extension FullKeyboardEngine {
    /// `suggestions` with the revert in the left slot, in place of the word as it reads (which
    /// space keeps anyway), while the cursor is right after the latest autocorrection.
    func offeringRevert(_ suggestions: [KeyboardSuggestion]) -> [KeyboardSuggestion] {
        // A held space (`spacePending`) stands for the one delete takes first.
        guard !spacePending, let correction = autocorrect.revertibleCorrection(before: textBefore) else {
            return suggestions
        }
        let revert = KeyboardSuggestion(displayText: correction.original, replacement: correction.original, kind: .revert)
        let others = suggestions.filter {
            $0.kind != .keepLiteral && $0.replacement.caseInsensitiveCompare(correction.original) != .orderedSame
        }
        return [revert] + others.prefix(2)
    }
}

extension AutocorrectEngine {
    /// The latest autocorrection, which the strip offers to revert while the cursor is right
    /// after it, as Apple's keyboard offers its bubble (black box, iOS 27.2): delete takes only
    /// the space or mark after it, and the cursor may come back to it later.
    struct AppliedCorrection: Equatable {
        /// One word the revert puts back as typed (`from`), and what autocorrect made it.
        struct Change: Equatable {
            let from: String
            let to: String
            /// The language the fix was made in, for what the keys learn from reverting it.
            var language: KeyboardLanguage? = nil
        }

        /// What you typed ("teh", "well see", "ofthe").
        let original: String
        /// What autocorrect put in its place, without the space or mark after it.
        let corrected: String
        /// The words in `original` that autocorrect changed; the revert keeps each as typed.
        let changes: [Change]
        /// The text before the cursor right after `corrected`.
        let anchor: String

        /// How much of the text before the cursor must read as `anchor`: hosts give a window of
        /// it, which may start elsewhere once the text is long.
        static let anchorTail = 64

        /// Whether the cursor is right after `corrected`, with the same text before it.
        func isAtEnd(of before: String?) -> Bool {
            guard let before, before.hasSuffix(corrected) else { return false }
            guard anchor.count >= Self.anchorTail, before.count >= Self.anchorTail else { return before == anchor }
            return before.suffix(Self.anchorTail) == anchor.suffix(Self.anchorTail)
        }
    }
}
