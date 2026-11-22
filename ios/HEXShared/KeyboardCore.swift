import Foundation
import HexKeyboardCore

/// A typing model opened by the keyboard's Rust core (`crates/keyboard`, `hk_model_*`): its
/// own read-only map of the file. It never changes once open, so any thread may use it.
nonisolated final class KeyboardCoreModel: @unchecked Sendable {
    /// The core's handle, for the other wrappers of its interface.
    let pointer: OpaquePointer

    init?(url: URL) {
        guard let pointer = hk_model_open(url.path) else { return nil }
        self.pointer = pointer
    }

    deinit {
        hk_model_free(pointer)
    }

    var wordCount: Int { Int(hk_model_word_count(pointer)) }
}
