import UIKit
import XCTest
@testable import HEX

/// The pure Dubeolsik composer: no keyboard, no UIKit text field.
final class HangulComposerTests: XCTestCase {
    private func composer(typing jamo: [String]) -> HangulComposer {
        var composer = HangulComposer()
        for jamo in jamo { composer.type(jamo) }
        return composer
    }

    /// 한: ㅎ shows as itself, ㅏ makes 하, ㄴ makes 한, and delete steps back through all three.
    func testConsonantVowelAndFinalComposeThenBackspace() {
        var composer = HangulComposer()
        XCTAssertEqual(composer.type("ㅎ"), "ㅎ")
        XCTAssertEqual(composer.type("ㅏ"), "하")
        XCTAssertEqual(composer.type("ㄴ"), "한")
        XCTAssertEqual(composer.backspace(), "하")
        XCTAssertEqual(composer.backspace(), "ㅎ")
        XCTAssertEqual(composer.backspace(), "")
        XCTAssertNil(composer.backspace(), "an empty run has nothing to take back")
        XCTAssertTrue(composer.isEmpty)
    }

    /// ㅗ and ㅏ make the compound vowel ㅘ, which delete splits one jamo at a time.
    func testCompoundVowelComposesAndSplits() {
        var composer = HangulComposer()
        XCTAssertEqual(composer.type("ㄱ"), "ㄱ")
        XCTAssertEqual(composer.type("ㅗ"), "고")
        XCTAssertEqual(composer.type("ㅏ"), "과")
        XCTAssertEqual(composer.backspace(), "고")
        XCTAssertEqual(composer.backspace(), "ㄱ")
        XCTAssertEqual(composer.backspace(), "")
    }

    /// ㄹ then ㄱ make the compound final ㄺ, which delete splits to ㄹ and then removes.
    func testCompoundFinalComposesAndSplits() {
        var composer = HangulComposer()
        XCTAssertEqual(composer.type("ㄷ"), "ㄷ")
        XCTAssertEqual(composer.type("ㅏ"), "다")
        XCTAssertEqual(composer.type("ㄹ"), "달")
        XCTAssertEqual(composer.type("ㄱ"), "닭")
        XCTAssertEqual(composer.backspace(), "달")
        XCTAssertEqual(composer.backspace(), "다")
        XCTAssertEqual(composer.backspace(), "ㄷ")
    }

    /// A vowel after a final moves the final to start the next syllable: 한 then ㅏ is 하나, and
    /// 닭 then ㅗ leaves ㄹ behind and starts 고 (달고).
    func testVowelAfterAFinalMovesItToTheNextSyllable() {
        XCTAssertEqual(composer(typing: ["ㅎ", "ㅏ", "ㄴ", "ㅏ"]).text, "하나")
        XCTAssertEqual(composer(typing: ["ㄷ", "ㅏ", "ㄹ", "ㄱ", "ㅗ"]).text, "달고")
    }

    /// ㄸ, ㅃ and ㅉ are initials only; typed after a syllable they start a new one.
    func testConsonantsThatCannotBeFinalsStartANewSyllable() {
        XCTAssertEqual(composer(typing: ["ㄷ", "ㅏ", "ㄸ"]).text, "다ㄸ")
        XCTAssertEqual(composer(typing: ["ㄷ", "ㅏ", "ㄸ", "ㅏ"]).text, "다따")
        XCTAssertEqual(composer(typing: ["ㅂ", "ㅏ", "ㅃ", "ㅏ"]).text, "바빠")
        XCTAssertEqual(composer(typing: ["ㅈ", "ㅏ", "ㅉ", "ㅏ"]).text, "자짜")
    }

    /// A lone consonant or vowel is shown as its compatibility jamo.
    func testLoneJamoShowAsCompatibilityJamo() {
        XCTAssertEqual(composer(typing: ["ㅎ"]).text, "ㅎ")
        XCTAssertEqual(composer(typing: ["ㅏ"]).text, "ㅏ")
        XCTAssertEqual(composer(typing: ["ㅗ", "ㅏ"]).text, "ㅘ")
        XCTAssertEqual(composer(typing: ["ㄲ"]).text, "ㄲ")
    }

    /// Shift's doubles: the five doubled consonants and ㅐㅔ's ㅒㅖ, and nothing else.
    func testShiftedJamo() {
        let shifted = [
            "ㅂ": "ㅃ", "ㅈ": "ㅉ", "ㄷ": "ㄸ", "ㄱ": "ㄲ", "ㅅ": "ㅆ",
            "ㅐ": "ㅒ", "ㅔ": "ㅖ",
        ]
        for (plain, double) in shifted {
            XCTAssertEqual(HangulComposer.shifted(plain), double)
        }
        for unchanged in ["ㅁ", "ㄴ", "ㅇ", "ㄹ", "ㅎ", "ㅗ", "ㅓ", "ㅏ", "ㅣ", "ㅛ"] {
            XCTAssertEqual(HangulComposer.shifted(unchanged), unchanged)
        }
        XCTAssertEqual(composer(typing: ["ㄲ", "ㅏ"]).text, "까")
        XCTAssertEqual(composer(typing: ["ㅆ", "ㅏ"]).text, "싸")
        XCTAssertEqual(composer(typing: ["ㄱ", "ㅒ"]).text, "걔")
    }

    /// A compound final is made from a final and the next consonant (ㄻ, ㄳ), and a vowel after
    /// it leaves its first half and takes the second.
    func testCompoundFinals() {
        XCTAssertEqual(composer(typing: ["ㅁ", "ㅏ", "ㄹ", "ㅁ"]).text, "맒")
        XCTAssertEqual(composer(typing: ["ㅅ", "ㅏ", "ㄱ", "ㅅ"]).text, "삯")
        XCTAssertEqual(composer(typing: ["ㅁ", "ㅏ", "ㄹ", "ㅁ", "ㅏ"]).text, "말마")
    }

    /// A reset forgets the run, as a commit does after space, return or punctuation.
    func testResetStartsANewRun() {
        var composer = HangulComposer()
        composer.type("ㅎ")
        composer.type("ㅏ")
        composer.reset()
        XCTAssertTrue(composer.isEmpty)
        XCTAssertEqual(composer.text, "")
        XCTAssertEqual(composer.type("ㄴ"), "ㄴ")
    }
}

/// The keyboard typing through the composer: the field is updated through `deleteBackward` and
/// `insertText`, and space, delete and pages commit.
@MainActor
final class HangulKeyboardTests: XCTestCase {
    private var target: FixtureTextTarget!
    private var engine: FullKeyboardEngine!
    private var nextID = 1

    override func setUp() async throws {
        target = FixtureTextTarget()
        target.autocapitalizationType = UITextAutocapitalizationType.none
        engine = FullKeyboardEngine(target: target)
        engine.width = 393
        engine.korean.isEnabled = true
        engine.language = .korean
        engine.configureForField()
    }

    private func tap(_ kind: KeyKind) {
        guard let key = engine.layout.key(kind) else { return XCTFail("no \(kind)") }
        let point = CGPoint(x: key.frame.midX, y: key.frame.midY)
        engine.touchBegan(id: nextID, at: point)
        engine.touchEnded(id: nextID, at: point)
        nextID += 1
    }

    private func tap(_ character: String) {
        tap(.character(character))
    }

    /// ㅎ ㅏ ㄴ shows 한 in the host, and delete steps it back to 하 without the field seeing a
    /// whole syllable removed.
    func testKeysComposeAndDeleteStepsBack() {
        tap("ㅎ")
        XCTAssertEqual(target.text, "ㅎ")
        tap("ㅏ")
        XCTAssertEqual(target.text, "하")
        tap("ㄴ")
        XCTAssertEqual(target.text, "한")
        engine.deleteBackward()
        XCTAssertEqual(target.text, "하")
        engine.deleteBackward()
        XCTAssertEqual(target.text, "ㅎ")
        engine.deleteBackward()
        XCTAssertEqual(target.text, "")
        engine.deleteBackward()
        XCTAssertEqual(target.text, "", "delete with nothing composing does nothing in an empty field")
    }

    /// Shift is one-shot for Hangul: ㄲ gives 까, then shift is off again and ㅏ types a plain
    /// vowel, never an uppercased one.
    func testShiftIsOneShot() {
        tap(.shift)
        tap("ㄱ")
        XCTAssertEqual(target.text, "ㄲ")
        XCTAssertEqual(engine.shift.state, .off)
        tap("ㅏ")
        XCTAssertEqual(target.text, "까")
    }

    /// Space commits the run: space types as usual, and the next jamo starts a new syllable.
    func testSpaceCommits() {
        tap("ㅎ")
        tap("ㅏ")
        tap("ㄴ")
        tap(.space)
        XCTAssertEqual(target.text, "한 ")
        tap("ㅏ")
        XCTAssertEqual(target.text, "한 ㅏ", "a lone vowel after the space, not one syllable with 한")
        engine.deleteBackward()
        XCTAssertEqual(target.text, "한 ", "delete takes the composing jamo, not the space")
    }

    /// A page switch commits; punctuation then types as usual.
    func testPageSwitchCommits() {
        tap("ㅎ")
        tap("ㅏ")
        tap(.layerSwitch(.numbers))
        tap(".")
        XCTAssertEqual(target.text, "하.")
        tap(.layerSwitch(.letters))
        XCTAssertEqual(engine.layout.layer, .letters)
    }

    /// Auto-capitalization doesn't shift Hangul: at a sentence's start ㅂ types ㅂ, not ㅃ.
    func testAutoCapitalizationDoesNotShiftHangul() {
        target.autocapitalizationType = UITextAutocapitalizationType.sentences
        engine.configureForField()
        tap("ㅂ")
        XCTAssertEqual(target.text, "ㅂ")
        XCTAssertFalse(engine.isUppercase)
    }
}
