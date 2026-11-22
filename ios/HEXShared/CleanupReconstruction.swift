import Foundation
import HexKeyboardCore

/// Rebuilds a transcript from the cleanup tagger's KEEP/DELETE labels: joins the kept words
/// and repairs punctuation and capitals left next to a deletion. It only removes characters
/// (commas, hesitation ellipses) or changes the case of a first letter; it never adds,
/// reorders, or changes words, names, or numbers.
///
/// The keyboard core's (`hk_dictation_reconstruct`, crates/keyboard/core/src/dictation/
/// reconstruct.rs), a line-for-line port of the tagger's `js/reconstruct.js` with JavaScript's
/// string semantics: a word's "first letter" is its first UTF-16 unit (an emoji or other
/// astral character is never cased), while the rest of a word is checked by code point.
nonisolated enum CleanupReconstruction {
    /// `words` are the transcript's whitespace-separated words; `deletes` has one flag per word.
    static func reconstruct(_ words: [String], deletes: [Bool]) -> String {
        let flags = deletes.map { $0 ? UInt8(1) : 0 }
        return withTexts(words[...]) { texts in
            texts.withUnsafeBufferPointer { texts in
                flags.withUnsafeBufferPointer { flags in
                    DictationCore.text(words.joined(separator: " ")) {
                        hk_dictation_reconstruct(texts.baseAddress, texts.count, flags.baseAddress, flags.count, $0)
                    }
                }
            }
        }
    }

    /// `body` with each word as the core reads it, all valid inside `body`.
    private static func withTexts<R>(_ words: ArraySlice<String>, _ body: ([HkStr]) -> R) -> R {
        var texts: [HkStr] = []
        texts.reserveCapacity(words.count)
        func next(_ rest: ArraySlice<String>) -> R {
            guard let word = rest.first else { return body(texts) }
            return DictationCore.withText(word) { text in
                texts.append(text)
                return next(rest.dropFirst())
            }
        }
        return next(words)
    }
}
