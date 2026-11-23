import Foundation
import HexKeyboardCore

/// Dictation's text rules in the keyboard's Rust core (`crates/keyboard`, `hk_dictation_*`,
/// `core/src/dictation`): the bridge `TranscriptFormatter`, `SpokenLists`,
/// `SpokenPunctuation`, `SpokenCleanupRules`, `CleanupReconstruction`, `FrenchSpacing` and
/// `CleanupTagger.words` call. The core gives what the Swift rules gave, byte for byte
/// (`crates/keyboard/core/tests/dictation_parity.rs`); this file only moves text across.
///
/// A language goes as `Locale` reads its identifier (its language code and region), so the
/// core sees what the Swift rules saw. Text goes as UTF-8 with a length, U+0000 included.
nonisolated enum DictationCore {
    /// A call that writes text to its last argument and returns an `HK_DICTATION_*` status.
    typealias TextCall = (UnsafeMutablePointer<HkText>) -> Int32

    /// `body` with `text` as the core reads it, valid only inside `body`.
    static func withText<R>(_ text: String, _ body: (HkStr) -> R) -> R {
        var text = text
        return text.withUTF8 { buffer in
            body(HkStr(bytes: UnsafeRawPointer(buffer.baseAddress)?.assumingMemoryBound(to: CChar.self),
                       length: buffer.count))
        }
    }

    /// `body` with an optional text: NULL for nil.
    static func withOptionalText<R>(_ text: String?, _ body: (UnsafePointer<HkStr>?) -> R) -> R {
        guard let text else { return body(nil) }
        return withText(text) { string in withUnsafePointer(to: string) { body($0) } }
    }

    /// `body` with `identifier` as the core's language: NULL for nil (Swift's "no language").
    static func withLanguage<R>(_ identifier: String?, _ body: (UnsafePointer<HkLanguage>?) -> R) -> R {
        guard let identifier else { return body(nil) }
        let locale = Locale(identifier: identifier)
        let code = locale.language.languageCode?.identifier
        let region = locale.region?.identifier
        return withText(identifier) { identifier in
            withField(code) { code in
                withField(region) { region in
                    withUnsafePointer(to: HkLanguage(identifier: identifier, code: code, region: region)) { body($0) }
                }
            }
        }
    }

    private static func withField<R>(_ text: String?, _ body: (HkStr) -> R) -> R {
        guard let text else { return body(HkStr(bytes: nil, length: 0)) }
        return withText(text, body)
    }

    /// The text a call wrote, released, or nil with its status when it wrote none.
    static func call(_ call: TextCall) -> (text: String?, status: Int32) {
        var out = HkText(bytes: nil, length: 0)
        let status = call(&out)
        defer { hk_text_free(&out) }
        guard status == HK_DICTATION_OK else { return (nil, status) }
        guard let bytes = out.bytes else { return ("", status) }
        let text = String(decoding: UnsafeRawBufferPointer(start: bytes, count: out.length), as: UTF8.self)
        return (text, status)
    }

    /// The text a call wrote. Every input from Swift is valid, so another status is the core
    /// failing (a panic it caught): a debug build stops there, a release build keeps `fallback`.
    static func text(_ fallback: @autoclosure () -> String, function: StaticString = #function, _ call: TextCall)
        -> String {
        let result = Self.call(call)
        if let text = result.text { return text }
        assertionFailure("\(function): the keyboard core's dictation rules failed (status \(result.status))")
        return fallback()
    }

    /// A yes-or-no answer, with `fallback` as `text` does.
    static func flag(_ fallback: Bool, function: StaticString = #function,
                     _ call: (UnsafeMutablePointer<UInt8>) -> Int32) -> Bool {
        var out: UInt8 = 0
        let status = call(&out)
        guard status == HK_DICTATION_OK else {
            assertionFailure("\(function): the keyboard core's dictation rules failed (status \(status))")
            return fallback
        }
        return out != 0
    }

    /// A one-text stage that takes a language.
    static func stage(
        _ text: String,
        language: String?,
        function: StaticString = #function,
        _ stage: (HkStr, UnsafePointer<HkLanguage>?, UnsafeMutablePointer<HkText>) -> Int32
    ) -> String {
        withText(text) { string in
            withLanguage(language) { language in
                Self.text(text, function: function) { stage(string, language, $0) }
            }
        }
    }

    /// `body` with `isWord` as the core's dictionary callback (NULL for nil).
    static func withDictionary<R>(
        _ isWord: ((String) -> Bool)?,
        _ body: (HkIsWord?, UnsafeMutableRawPointer?) -> R
    ) -> R {
        guard let isWord else { return body(nil, nil) }
        return withDictionary(isWord, body)
    }

    /// `body` with `isWord` as the core's dictionary callback, which the core calls only
    /// during the call `body` makes.
    static func withDictionary<R>(
        _ isWord: (String) -> Bool,
        _ body: (HkIsWord?, UnsafeMutableRawPointer?) -> R
    ) -> R {
        withoutActuallyEscaping(isWord) { isWord in
            let dictionary = Dictionary(isWord: isWord)
            return withExtendedLifetime(dictionary) {
                body(Dictionary.callback, Unmanaged.passUnretained(dictionary).toOpaque())
            }
        }
    }

    private final class Dictionary {
        let isWord: (String) -> Bool

        init(isWord: @escaping (String) -> Bool) {
            self.isWord = isWord
        }

        static let callback: HkIsWord = { context, word in
            guard let context else { return 0 }
            let dictionary = Unmanaged<Dictionary>.fromOpaque(context).takeUnretainedValue()
            let text = String(decoding: UnsafeRawBufferPointer(start: word.bytes, count: word.length), as: UTF8.self)
            return dictionary.isWord(text) ? 1 : 0
        }
    }
}
