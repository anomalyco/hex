import UIKit
import XCTest
@testable import HEX

/// Experiment › Learn each language separately: what the keys and dictation learn, and the
/// words they keep, stay with the language they were typed or dictated in; off, everything is
/// the one shared list and learning it was before.
@MainActor
final class LanguageLearningTests: XCTestCase {
    private var directory: URL!
    private var suiteName: String!
    private var defaults: UserDefaults!
    private var settings: TypingJournalSettings!

    override func setUp() async throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("language-learning-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        suiteName = "LanguageLearningTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
        settings = TypingJournalSettings(defaults: defaults)
        settings.isEnabled = true
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: directory)
        defaults.removePersistentDomain(forName: suiteName)
    }

    private func fix(_ heard: String, _ meant: String) -> DictationEditResult {
        DictationEditResult(
            outcome: .edited,
            pairs: [DictationEditPair(kind: .substituted, heard: heard, inserted: heard, meant: meant)],
            wordCount: 5,
            changedWordCount: 1
        )
    }

    func testTypingFixesAndRejectionsStayInTheirLanguage() {
        XCTAssertFalse(settings.learnsPerLanguage, "off until the experiment is turned on")
        settings.learnsPerLanguage = true
        XCTAssertTrue(defaults.bool(forKey: "experiments.learnPerLanguage"))
        let journal = TypingJournal(directory: directory, settings: settings)

        journal.record(.retyped, typed: "wrod", final: "word", language: "en")
        journal.record(.retyped, typed: "wrod", final: "word", language: "en")
        journal.record(.retyped, typed: "bonjor", final: "bonjour", language: "fr")
        journal.record(.retyped, typed: "bonjor", final: "bonjour", language: "fr")
        journal.record(.retyped, typed: "tihs", final: "this")
        journal.record(.retyped, typed: "tihs", final: "this")
        journal.record(.undone, typed: "form", final: "from", language: "en")
        journal.record(.keptTyped, typed: "form", final: "from", language: "en")
        journal.record(.undone, typed: "chatte", final: "chat", language: "fr")
        journal.record(.keptTyped, typed: "chatte", final: "chat", language: "fr")
        journal.record(.undone, typed: "adn", final: "and")
        journal.record(.keptTyped, typed: "adn", final: "and")

        XCTAssertEqual(journal.typingLearning(for: .english).typos, ["wrod": "word", "tihs": "this"])
        XCTAssertEqual(journal.typingLearning(for: .english).rejected, ["from", "and"])
        XCTAssertEqual(journal.typingLearning(for: .french).typos, ["bonjor": "bonjour", "tihs": "this"])
        XCTAssertEqual(journal.typingLearning(for: .french).rejected, ["chat", "and"])
        XCTAssertEqual(journal.typingLearning(for: .german).typos, ["tihs": "this"], "never-tagged fixes count everywhere")
        XCTAssertEqual(journal.typingLearning(for: .german).rejected, ["and"])

        let stored = TypingJournal.load(from: directory.appendingPathComponent(TypingJournal.fileName))
        XCTAssertEqual(stored.corrections.first?.language, "en")
        XCTAssertEqual(
            stored.corrections.map(\.language).compactMap { $0 },
            ["en", "en", "fr", "fr", "en", "en", "fr", "fr"]
        )

        settings.learnsPerLanguage = false
        let blended = journal.typingLearning(for: .german)
        XCTAssertEqual(blended, journal.typingLearning, "off, every language has the one shared learning")
        XCTAssertEqual(blended.typos.keys.sorted(), ["bonjor", "tihs", "wrod"])
        XCTAssertEqual(blended.rejected, ["and", "chat", "from"])
    }

    func testDictationFixesStayInTheLanguageTheyWereDictatedIn() {
        settings.learnsPerLanguage = true
        let journal = TypingJournal(directory: directory, settings: settings)

        journal.recordDictation(fix("cloud", "Claude"), language: "en-US")
        journal.recordDictation(fix("cloud", "Claude"), language: "en-US")
        journal.recordDictation(fix("merci", "Merci!"), language: "fr-FR")
        journal.recordDictation(fix("merci", "Merci!"), language: "fr-FR")
        journal.recordDictation(fix("tihs", "this"))
        journal.recordDictation(fix("tihs", "this"))

        XCTAssertEqual(
            journal.dictationCorrections(language: "en-GB"),
            [DictationCorrection(heard: "cloud", meant: "Claude"), DictationCorrection(heard: "tihs", meant: "this")],
            "an English region keeps the other's fixes, and a fix never tagged counts too"
        )
        XCTAssertEqual(
            journal.dictationCorrections(language: "fr"),
            [DictationCorrection(heard: "merci", meant: "Merci!"), DictationCorrection(heard: "tihs", meant: "this")]
        )
        XCTAssertEqual(
            TypingJournal.storedDictationCorrections(settings: settings, directory: directory, language: "en-US"),
            [DictationCorrection(heard: "cloud", meant: "Claude"), DictationCorrection(heard: "tihs", meant: "this")]
        )

        settings.learnsPerLanguage = false
        let blended = journal.dictationCorrections(language: "de")
        XCTAssertEqual(Set(blended.map(\.heard)), ["cloud", "merci", "tihs"], "off, every language gets every fix")
    }

    func testAFixLearnedInTwoLanguagesCountsForBoth() {
        settings.learnsPerLanguage = true
        let journal = TypingJournal(directory: directory, settings: settings)

        journal.recordDictation(fix("cloud", "Claude"), language: "en")
        journal.recordDictation(fix("cloud", "Claude"), language: "fr")
        XCTAssertNil(
            TypingJournal.load(from: directory.appendingPathComponent(TypingJournal.fileName)).dictationFixes.first?.language,
            "made in two languages: it belongs to every one of them"
        )

        journal.recordDictation(fix("cloud", "Claude"), language: "en")
        let expected = [DictationCorrection(heard: "cloud", meant: "Claude")]
        XCTAssertEqual(journal.dictationCorrections(language: "en"), expected)
        XCTAssertEqual(journal.dictationCorrections(language: "fr"), expected)
        XCTAssertEqual(journal.dictationCorrections(language: "de"), expected, "a fix never tagged counts for a language it never saw")
    }

    func testLearnedWordsKeepOneListPerLanguageSeededFromTheSharedOne() {
        let shared = LearnedWordsStore(defaults: defaults)
        shared.learn("teh")
        shared.learn("bonjour")

        let english = shared.scoped(to: "en")
        XCTAssertEqual(english.words, ["teh", "bonjour"], "a language starts with the shared words")
        english.learn("the")
        XCTAssertEqual(LearnedWordsStore(defaults: defaults, language: "en").words, ["teh", "bonjour", "the"])

        let french = shared.scoped(to: "fr")
        XCTAssertEqual(french.words, ["teh", "bonjour"])
        shared.learn("merci")
        XCTAssertFalse(french.contains("merci"), "a language's list is made once, from the shared words of then")
        XCTAssertEqual(shared.words, ["teh", "bonjour", "merci"])
    }

    func testThePartnerKeepsItsOwnWordsAndLearning() {
        let engine = AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults))
        let available = ["en_US", "fr_FR"]
        engine.setLanguage(.english, available: available)
        engine.learnsPerLanguage = true
        settings.learnsPerLanguage = true
        let journal = TypingJournal(directory: directory, settings: settings)
        journal.record(.retyped, typed: "wrod", final: "word", language: "en")
        journal.record(.retyped, typed: "wrod", final: "word", language: "en")
        journal.record(.retyped, typed: "bonjor", final: "bonjour", language: "fr")
        journal.record(.retyped, typed: "bonjor", final: "bonjour", language: "fr")
        engine.learnedWords.learn("the")
        engine.setPartner(.french, available: available)

        let keyboard = FullKeyboardEngine(target: FixtureTextTarget(), autocorrect: engine)
        keyboard.journal = journal

        XCTAssertEqual(engine.learning.typos, ["wrod": "word"])
        XCTAssertEqual(engine.partner?.learning.typos, ["bonjor": "bonjour"], "the partner reads its own language's fixes")
        XCTAssertTrue(engine.learnedWords.contains("the"))
        XCTAssertFalse(engine.partner?.learnedWords.contains("the") ?? true, "the other language keeps its own words")
        engine.partner?.learnedWords.learn("bonjour")
        XCTAssertTrue(engine.partner?.learnedWords.contains("bonjour") ?? false)
        XCTAssertFalse(engine.learnedWords.contains("bonjour"))
    }

    func testDeleteErasesEveryLanguagesWords() {
        let shared = LearnedWordsStore(defaults: defaults)
        shared.learn("teh")
        let english = shared.scoped(to: "en")
        english.learn("the")
        let french = shared.scoped(to: "fr")
        french.learn("bonjour")
        XCTAssertEqual(
            Set(defaults.dictionaryRepresentation().keys.filter { $0.hasPrefix("keyboard.learnedWords") }),
            ["keyboard.learnedWords", "keyboard.learnedWords.en", "keyboard.learnedWords.fr"]
        )

        TypingJournal.delete(in: directory, settings: settings)

        XCTAssertTrue(shared.words.isEmpty)
        XCTAssertTrue(english.words.isEmpty, "a list another process still holds drops it too")
        XCTAssertTrue(french.words.isEmpty)
        XCTAssertTrue(
            defaults.dictionaryRepresentation().keys.filter { $0.hasPrefix("keyboard.learnedWords") }.isEmpty,
            "the shared list and every language's are erased"
        )
    }

    func testAJournalWrittenBeforeLanguagesStillLoads() throws {
        let url = directory.appendingPathComponent(TypingJournal.fileName)
        try Data(#"{"keys":{},"corrections":[{"kind":"picked","typed":"teh","final":"the","at":1}],"dictationFixes":[{"kind":"substituted","heard":"cloud","inserted":"cloud","meant":"Claude","count":2,"contradictions":0,"lastAt":1}]}"#.utf8)
            .write(to: url)

        let snapshot = TypingJournal.load(from: url)
        XCTAssertEqual(snapshot.corrections.count, 1)
        XCTAssertNil(snapshot.corrections.first?.language)
        XCTAssertNil(snapshot.dictationFixes.first?.language)

        settings.learnsPerLanguage = true
        XCTAssertEqual(
            TypingJournal.storedDictationCorrections(settings: settings, directory: directory, language: "en-US"),
            [DictationCorrection(heard: "cloud", meant: "Claude")],
            "an untagged fix counts for every language"
        )
        XCTAssertEqual(TypingJournal(directory: directory, settings: settings).typingLearning(for: .english).typos, [:])
    }
}
