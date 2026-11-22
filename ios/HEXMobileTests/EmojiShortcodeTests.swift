import Foundation
import XCTest
@testable import HEX

/// Emoji shortcodes (Settings › Experiments): the ":name" before the cursor, the emoji the
/// bundled index names, the closing ":" of an exact name, and the cases that must not fire.
@MainActor
final class EmojiShortcodeTests: XCTestCase {
    private static var libraries: [String: EmojiLibrary] = [:]

    private func library(_ language: String = "en") throws -> EmojiLibrary {
        if let cached = Self.libraries[language] { return cached }
        let loaded = try XCTUnwrap(EmojiLibrary.load(language: language, bundle: Bundle(for: FullKeyboardView.self)))
        Self.libraries[language] = loaded
        return loaded
    }

    private func matches(_ text: String?, enabled: Bool = true, limit: Int = 3) throws -> [String] {
        let library = try library()
        return EmojiShortcode.matches(before: text, enabled: enabled, library: library, usage: [:], limit: limit)
            .map { library.entries[$0].emoji }
    }

    // MARK: - Off, and no token

    func testOffFindsNothing() throws {
        XCTAssertEqual(try matches(":pizza", enabled: false), [])
        XCTAssertEqual(try matches("hello :pizza", enabled: false), [])
    }

    func testOneCharacterIsNotATokenAndTwoAre() throws {
        XCTAssertNil(EmojiShortcode.token(before: ":p"))
        XCTAssertEqual(try matches(":p"), [], "a colon and one letter is not a token yet")
        XCTAssertFalse(try matches(":pi").isEmpty, "two characters may match")
        XCTAssertEqual(try matches(":piz").first ?? "", "🍕")
    }

    func testColonInsideAWordIsNotAToken() throws {
        XCTAssertNil(EmojiShortcode.token(before: "abc:pizza"))
        XCTAssertEqual(try matches("abc:pizza"), [])
        XCTAssertEqual(try matches("abc:p"), [])
    }

    func testTokenAfterWhitespaceOrAtTheStart() throws {
        XCTAssertEqual(try matches(":pizza").first ?? "", "🍕")
        XCTAssertEqual(try matches("I want :pizza").first ?? "", "🍕")
        XCTAssertEqual(try matches("I want\n:pizza").first ?? "", "🍕")
        XCTAssertEqual(try matches("I want:pizza"), [])
    }

    func testPizzaFindsPizzaAndCaseDoesNotMatter() throws {
        XCTAssertTrue(try matches(":pizza").contains("🍕"))
        XCTAssertTrue(try matches(":PIZZA").contains("🍕"))
    }

    func testUnknownNameFindsNothing() throws {
        XCTAssertEqual(try matches(":zzzzzz"), [])
        XCTAssertEqual(try matches(":notanemoji"), [])
    }

    func testTheTokenCoversTheWholeName() throws {
        let token = try XCTUnwrap(EmojiShortcode.token(before: "I want :pizza"))
        XCTAssertEqual(token.name, "pizza")
        XCTAssertEqual(token.text, ":pizza")
        XCTAssertEqual(token.text.count, 6)
    }

    // MARK: - Closing colon

    func testClosingColonCompletesAnExactName() throws {
        let library = try library()
        let completion = try XCTUnwrap(EmojiShortcode.completion(before: ":pizza", library: library))
        XCTAssertEqual(completion.token.text, ":pizza")
        XCTAssertEqual(library.entries[completion.entry].emoji, "🍕")
        XCTAssertEqual(EmojiShortcode.completion(before: "I want :pizza", library: library)?.token.text ?? "", ":pizza")
    }

    func testClosingColonNeedsTheWholeName() throws {
        let library = try library()
        XCTAssertNil(EmojiShortcode.completion(before: ":piz", library: library))
        XCTAssertNil(EmojiShortcode.completion(before: "abc:pizza", library: library))
        XCTAssertNil(EmojiShortcode.completion(before: ":zzzzzz", library: library))
    }

    func testCompletionIsExactEvenWhenPrefixMatchesExist() throws {
        let library = try library()
        let completion = try XCTUnwrap(EmojiShortcode.completion(before: ":thumbs", library: library))
        XCTAssertEqual(library.entries[completion.entry].emoji, "👍")
    }

    // MARK: - The keyboard's own path

    func testTheClosingColonTakesTheTokenBackAndTypesTheEmoji() throws {
        let target = FixtureTextTarget()
        target.insertText(":pizza")
        let engine = FullKeyboardEngine(target: target)
        let library = try library()
        engine.emojiShortcode = { before in
            guard let completion = EmojiShortcode.completion(before: before, library: library) else { return nil }
            return EmojiShortcode.Replacement(
                token: completion.token.text,
                emoji: library.entries[completion.entry].emoji
            )
        }
        XCTAssertTrue(engine.typeEmojiShortcode(":"))
        XCTAssertEqual(target.text, "🍕")

        target.insertText(" :piz")
        XCTAssertFalse(engine.typeEmojiShortcode(":"), "a prefix is not a name typed in full")
        XCTAssertEqual(target.text, "🍕 :piz")
    }

    func testTheSettingIsOffByDefaultAndKeepsTheAppGroupKey() throws {
        let suite = "EmojiShortcodeTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = FullKeyboardSettings(defaults: defaults)
        XCTAssertFalse(settings.emojiShortcodes, "off, like every experiment")
        settings.emojiShortcodes = true
        XCTAssertTrue(FullKeyboardSettings(defaults: defaults).emojiShortcodes)
        XCTAssertTrue(defaults.bool(forKey: "experiments.emojiShortcodes"))
    }
}
