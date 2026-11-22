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
        configure()
    }

    /// A field with `text` before the cursor (at its end) and the Korean keyboard on it.
    private func configure(text: String = "") {
        configure(FixtureTextTarget(text: text))
    }

    private func configure(_ field: FixtureTextTarget) {
        target = field
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

    /// A cursor move to text whose end reads like the run commits it: the jamo starts a new
    /// run where the cursor is now, and the old run stays as it was.
    func testCursorMoveWithTheSameLastSyllableStartsANewRun() {
        configure(text: "같이")
        tap("ㅇ")
        tap("ㅣ")
        XCTAssertEqual(target.text, "같이이")
        engine.target.adjustTextPosition(byCharacterOffset: -1)
        tap("ㄴ")
        XCTAssertEqual(target.text, "같이ㄴ이", "the run at the old cursor is untouched")
    }

    /// Delete after such a move takes the character at the cursor, never steps the old run
    /// back (which would turn its 이 into ㅇ).
    func testCursorMoveCommitsBeforeDelete() {
        configure(text: "같이")
        tap("ㅇ")
        tap("ㅣ")
        XCTAssertEqual(target.text, "같이이")
        engine.target.adjustTextPosition(byCharacterOffset: -1)
        engine.deleteBackward()
        XCTAssertEqual(target.text, "같이", "delete takes the character before the cursor, not the old run")
    }

    /// A selection commits the run too: delete then acts on the host, not on the run.
    func testSelectionCommitsBeforeDelete() {
        tap("ㅇ")
        tap("ㅣ")
        XCTAssertEqual(target.text, "이")
        target.selectedText = "이"
        engine.deleteBackward()
        XCTAssertEqual(target.text, "", "the host's delete, not deleteHangulBackward's stepping")
    }

    /// A host with no text after the cursor still tells two lines ending alike apart from the
    /// tail before the run: delete at the second line's end takes its last character, not the
    /// run at the first line's end.
    func testANilAfterTextHostStillCommitsOnACursorMove() {
        let field = FixtureTextTarget(text: "같이\n같이")
        field.hidesAfterText = true
        configure(field)
        engine.target.adjustTextPosition(byCharacterOffset: -3)
        tap("ㅇ")
        tap("ㅣ")
        XCTAssertEqual(target.text, "같이이\n같이")
        engine.target.adjustTextPosition(byCharacterOffset: 3)
        engine.deleteBackward()
        XCTAssertEqual(target.text, "같이이\n같", "the first line stays; the second loses its last character")
    }

    /// The same host still composes normally: the tail of an empty field is empty.
    func testANilAfterTextHostComposes() {
        let field = FixtureTextTarget()
        field.hidesAfterText = true
        configure(field)
        tap("ㅎ")
        tap("ㅏ")
        tap("ㄴ")
        XCTAssertEqual(target.text, "한")
        engine.deleteBackward()
        XCTAssertEqual(target.text, "하")
    }

    /// A lagging host reports the text from before its last insert (after the space, without
    /// it): the engine's own record keeps the space, so ㅎ ㅏ makes one syllable.
    func testALaggingHostKeepsTheSpaceBeforeTheNextJamo() {
        let field = LaggingField()
        field.lag = .behind
        configure(field)
        engine.clock = { 100 }
        tap("ㄱ")
        tap("ㅏ")
        tap(.space)
        tap("ㅎ")
        tap("ㅏ")
        XCTAssertEqual(target.text, "가 하")
    }

    /// A delete key's jitter is not a word delete: the run survives it, so the next delete
    /// steps back one jamo rather than removing the syllable.
    func testDeleteKeyJitterDoesNotCommitTheRun() {
        tap("ㅎ")
        tap("ㅏ")
        tap("ㄴ")
        XCTAssertEqual(target.text, "한")
        let plane = KeyboardKeyPlaneView(engine: engine)
        guard let delete = engine.layout.key(.delete) else { return XCTFail("no delete key") }
        let start = CGPoint(x: delete.frame.midX, y: delete.frame.midY)
        plane.began(id: 50, sample: TypingLogTouchSample(point: start, timestamp: 0))
        XCTAssertEqual(target.text, "하", "the press steps the run back")
        let jitter = TypingLogTouchSample(point: CGPoint(x: start.x - 1, y: start.y), timestamp: 0.01)
        plane.moved(id: 50, sample: jitter)
        plane.ended(id: 50, sample: TypingLogTouchSample(point: jitter.point, timestamp: 0.02))
        tap(.delete)
        XCTAssertEqual(target.text, "ㅎ", "the run survived the jitter")
    }

    /// A jamo ends the space pair: space, a jamo, delete, space types two spaces, not ". ".
    func testAJamoEndsTheDoubleSpacePair() {
        engine.clock = { 100 }
        tap("ㄱ")
        tap("ㅏ")
        XCTAssertEqual(target.text, "가")
        tap(.space)
        XCTAssertEqual(target.text, "가 ")
        tap("ㅎ")
        engine.deleteBackward()
        XCTAssertEqual(target.text, "가 ")
        tap(.space)
        XCTAssertEqual(target.text, "가  ", "not '가. '")
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

/// Settings › Experiments › Korean keyboard's language gating, with its flag in a test's own
/// defaults and the pure helpers taking it directly.
final class KoreanKeyboardGatingTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "KoreanKeyboardGatingTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    func testKoreanIsIgnoredWhileTheExperimentIsOff() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .korean]
        settings.currentKeyboardLanguage = .korean
        defaults.set(false, forKey: KoreanKeyboardExperiment.key)

        XCTAssertEqual(settings.keyboardLanguages, [.english], "a stored ko is ignored")
        XCTAssertNil(settings.currentKeyboardLanguage, "a stored ko is ignored")
        XCTAssertNil(KeyboardLanguage.matching("ko-KR", koreanEnabled: false))
        XCTAssertFalse(KeyboardLanguage.selectable(koreanEnabled: false).contains(.korean))
    }

    func testKoreanAppearsWhileTheExperimentIsOn() {
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .korean]
        settings.currentKeyboardLanguage = .korean
        defaults.set(true, forKey: KoreanKeyboardExperiment.key)

        XCTAssertEqual(settings.keyboardLanguages, [.english, .korean])
        XCTAssertEqual(settings.currentKeyboardLanguage, KeyboardLanguage.korean)
        XCTAssertEqual(KeyboardLanguage.matching("ko-KR", koreanEnabled: true), KeyboardLanguage.korean)
        XCTAssertTrue(KeyboardLanguage.selectable(koreanEnabled: true).contains(.korean))
    }
}
