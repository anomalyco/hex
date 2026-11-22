import UIKit
import XCTest
@testable import HEX

/// Learn how I type, dictation side: locating the dictation, diffing it, classifying the
/// outcome, the learning threshold, and applying fixes with word boundaries.
final class DictationEditInferenceTests: XCTestCase {
    private func span(_ text: String, start: Bool = true, end: Bool = true) -> DictationSpan {
        DictationSpan(text: text, startIsCertain: start, endIsCertain: end)
    }

    // MARK: Span location

    func testLocatesTheDictationBetweenItsAnchorsAfterAnEdit() throws {
        let insertion = DictationInsertion(inserted: " see you at cloud nine.", before: "Hi Sam,", after: " Thanks")
        let found = try XCTUnwrap(DictationSpanLocator.locate(
            insertion, before: "Hi Sam, see you at Claude nine.", after: " Thanks"
        ))
        XCTAssertEqual(found.text, " see you at Claude nine.")
        XCTAssertTrue(found.startIsCertain)
        XCTAssertTrue(found.endIsCertain)

        // The cursor moved into the dictation: the text around it is the same.
        let moved = try XCTUnwrap(DictationSpanLocator.locate(
            insertion, before: "Hi Sam, see you at Cla", after: "ude nine. Thanks"
        ))
        XCTAssertEqual(moved.text, " see you at Claude nine.")
    }

    func testMissingContextOrAnotherFieldIsUnavailable() {
        let insertion = DictationInsertion(inserted: "See you soon.", before: "Hi Sam, ", after: nil)
        XCTAssertNil(DictationSpanLocator.locate(insertion, before: nil, after: nil))
        XCTAssertNil(DictationSpanLocator.locate(insertion, before: "Search the web", after: ""))
    }

    func testADictationThatBeganTheFieldHasAnUncertainStartOnceTextPrecedesTheCursor() throws {
        let insertion = DictationInsertion(inserted: "Hello world", before: "", after: "")
        let atStart = try XCTUnwrap(DictationSpanLocator.locate(insertion, before: "", after: "Hello world"))
        XCTAssertTrue(atStart.startIsCertain)
        let later = try XCTUnwrap(DictationSpanLocator.locate(insertion, before: "Hello world", after: ""))
        XCTAssertFalse(later.startIsCertain)
        XCTAssertTrue(later.endIsCertain)
    }

    func testRepeatedAnchorsPickTheGapClosestToTheDictation() throws {
        let insertion = DictationInsertion(inserted: "two", before: "ok ", after: " ok")
        let found = try XCTUnwrap(DictationSpanLocator.locate(insertion, before: "ok seventeen ok two", after: " ok"))
        XCTAssertEqual(found.text, "two")
    }

    // MARK: Word diff

    func testWordDiffFindsSubstitutionsInsertionsAndDeletions() {
        XCTAssertEqual(DictationWordDiff.hunks(["a", "b", "c"], ["a", "b", "c"]), [])
        XCTAssertEqual(
            DictationWordDiff.hunks(["a", "b", "c", "d"], ["a", "x", "c", "d", "e"]),
            [.init(old: 1..<2, new: 1..<2), .init(old: 4..<4, new: 4..<5)]
        )
        XCTAssertEqual(
            DictationWordDiff.hunks(["a", "b", "c"], ["a", "c"]),
            [.init(old: 1..<2, new: 1..<1)]
        )
    }

    // MARK: Outcomes

    func testUnchangedOrOnlyContinuedIsKept() throws {
        let kept = try XCTUnwrap(DictationEditClassifier.classify(
            heard: "see you at noon", inserted: "See you at noon.", final: span("See you at noon.")
        ))
        XCTAssertEqual(kept.outcome, .kept)
        let continued = try XCTUnwrap(DictationEditClassifier.classify(
            heard: nil, inserted: "See you at noon.", final: span("See you at noon. Bring snacks")
        ))
        XCTAssertEqual(continued.outcome, .kept, "new writing after the dictation isn't a fix")
        XCTAssertTrue(continued.pairs.isEmpty)
    }

    func testAFixedWordBecomesAPairWithAWordOfContextEachSide() throws {
        let result = try XCTUnwrap(DictationEditClassifier.classify(
            heard: "see you at cloud nine",
            inserted: "See you at cloud nine.",
            final: span("See you at Claude nine.")
        ))
        XCTAssertEqual(result.outcome, .edited)
        XCTAssertEqual(result.pairs, [DictationEditPair(
            kind: .substituted, heard: "cloud", inserted: "cloud", meant: "Claude", before: "at", after: "nine"
        )])
        XCTAssertEqual(result.keptHeard, [["see", "you", "at"], ["nine"]])
    }

    func testReplacingTheLastWordAndTypingOnLearnsOnlyTheFix() throws {
        let result = try XCTUnwrap(DictationEditClassifier.classify(
            heard: nil, inserted: "meet at cloud", final: span("meet at Claude and then lunch")
        ))
        XCTAssertEqual(result.outcome, .edited)
        XCTAssertEqual(result.pairs.map(\.meant), ["Claude"])
    }

    func testASingleWordDictationCanBeFixed() throws {
        let result = try XCTUnwrap(DictationEditClassifier.classify(heard: "cloud", inserted: "cloud", final: span("Claude")))
        XCTAssertEqual(result.outcome, .edited)
        XCTAssertEqual(result.pairs.first?.meant, "Claude")
    }

    func testChangingMoreThanHalfIsARewriteWithoutPairs() throws {
        let result = try XCTUnwrap(DictationEditClassifier.classify(
            heard: nil, inserted: "I will be there at noon", final: span("Sorry I can't make it today")
        ))
        XCTAssertEqual(result.outcome, .rewritten)
        XCTAssertTrue(result.pairs.isEmpty)
    }

    func testDifferentWordsAreAnEditButNotAPair() throws {
        let result = try XCTUnwrap(DictationEditClassifier.classify(
            heard: nil, inserted: "meet on Tuesday at noon", final: span("meet on Friday at noon")
        ))
        XCTAssertEqual(result.outcome, .edited)
        XCTAssertTrue(result.pairs.isEmpty, "a change of mind, not a misheard word")
    }

    func testASentenceStartCapitalIsNotPartOfTheSpelling() throws {
        let result = try XCTUnwrap(DictationEditClassifier.classify(
            heard: "their car is here", inserted: "Their car is here.", final: span("There car is here.")
        ))
        XCTAssertEqual(result.pairs.first?.heard, "their")
        XCTAssertEqual(result.pairs.first?.meant, "there")
    }

    func testReaddingAWordCleanupDroppedIsRecordedAsRestored() throws {
        let result = try XCTUnwrap(DictationEditClassifier.classify(
            heard: "I think um we should basically go",
            inserted: "I think we should go.",
            final: span("I think we should basically go.")
        ))
        XCTAssertEqual(result.pairs.map(\.kind), [.restored])
        XCTAssertEqual(result.pairs.first?.meant, "basically")
    }

    func testDeletingAFillerHexLeftIsRecordedSeparately() throws {
        let result = try XCTUnwrap(DictationEditClassifier.classify(
            heard: nil, inserted: "I like think we should go", final: span("I think we should go")
        ))
        XCTAssertEqual(result.pairs.map(\.kind), [.fillerRemoved])
        XCTAssertEqual(result.pairs.first?.inserted, "like")
    }

    func testAContextCutShortIsNotADeletionOrAFix() throws {
        let truncated = try XCTUnwrap(DictationEditClassifier.classify(
            heard: nil, inserted: "hello world how are you", final: span("world how are you", start: false)
        ))
        XCTAssertEqual(truncated.outcome, .kept)
        let midWord = try XCTUnwrap(DictationEditClassifier.classify(
            heard: nil, inserted: "hello world how are you", final: span("llo world how are you", start: false)
        ))
        XCTAssertEqual(midWord.outcome, .kept)
        let end = try XCTUnwrap(DictationEditClassifier.classify(
            heard: nil, inserted: "hello world how are you", final: span("hello world how a", end: false)
        ))
        XCTAssertEqual(end.outcome, .kept)
    }

    func testAnEmptySpanIsDeletedOrUndone() throws {
        XCTAssertEqual(DictationEditClassifier.classify(heard: nil, inserted: "Hi there", final: span(" "))?.outcome, .deleted)
        XCTAssertEqual(DictationEditClassifier.classify(heard: nil, inserted: "Hi there", final: span(""), undone: true)?.outcome, .undone)
    }

    func testDictatingAgainPairsTheWordsThatChanged() throws {
        let same = try XCTUnwrap(DictationEditClassifier.redictation(
            previousHeard: "meet me at cloud nine",
            previousInserted: "Meet me at cloud nine.",
            nextInserted: "Meet me at Claude nine."
        ))
        XCTAssertEqual(same.outcome, .redictated)
        XCTAssertEqual(same.pairs.map(\.heard), ["cloud"])
        XCTAssertEqual(same.pairs.map(\.meant), ["Claude"])

        let different = try XCTUnwrap(DictationEditClassifier.redictation(
            previousHeard: nil, previousInserted: "Meet me at noon.", nextInserted: "Actually let's skip lunch today."
        ))
        XCTAssertEqual(different.outcome, .redictated)
        XCTAssertTrue(different.pairs.isEmpty)
    }

    // MARK: Applying fixes

    func testFixesApplyToWholeWordsOnlyAndFollowCase() {
        let fixes = [DictationCorrection(heard: "cloud", meant: "Claude"), DictationCorrection(heard: "their", meant: "there")]
        XCTAssertEqual(
            DictationCorrections.apply(fixes, to: "cloud is great. Cloud rocks, cloudy day, cloud-based, CLOUD"),
            "Claude is great. Claude rocks, cloudy day, cloud-based, CLAUDE"
        )
        XCTAssertEqual(
            DictationCorrections.apply(fixes, to: "Their car and their dog, THEIR cat"),
            "There car and there dog, THERE cat"
        )
        XCTAssertEqual(DictationCorrections.apply(fixes, to: "cloud's answer"), "Claude's answer")
    }

    func testFixesNeverTouchAddressesPathsOrTags() {
        let fixes = [DictationCorrection(heard: "cloud", meant: "Claude")]
        let text = "cloud.com /cloud/ @cloud #cloud cloud_name"
        XCTAssertEqual(DictationCorrections.apply(fixes, to: text), text)
        XCTAssertEqual(DictationCorrections.apply(fixes, to: "I love cloud."), "I love Claude.")
    }

    func testPhraseFixesNeedPlainSpacesBetweenWords() {
        let fixes = [DictationCorrection(heard: "open code", meant: "OpenCode")]
        XCTAssertEqual(
            DictationCorrections.apply(fixes, to: "i use open code daily, not open, code"),
            "i use OpenCode daily, not open, code"
        )
    }

    func testRecognitionHintsPutCustomVocabularyFirstWithoutDuplicates() {
        let hints = DictationCorrections.recognitionHints(
            vocabulary: ["OpenCode", "Claude"],
            corrections: [DictationCorrection(heard: "cloud", meant: "Claude"), DictationCorrection(heard: "hacks", meant: "Hex")]
        )
        XCTAssertEqual(hints, ["OpenCode", "Claude", "Hex"])
        XCTAssertEqual(DictationCorrections.spellings([
            DictationCorrection(heard: "cloud", meant: "Claude"), DictationCorrection(heard: "their", meant: "there"),
        ]), ["Claude"])
    }

    // MARK: Threshold rule

    private func record(
        _ heard: String, _ meant: String, count: Int = 1, contradictions: Int = 0,
        kind: DictationEditPair.Kind = .substituted
    ) -> TypingJournalDictationFix {
        TypingJournalDictationFix(
            kind: kind, heard: heard, inserted: heard, meant: meant, count: count,
            contradictions: contradictions, before: nil, after: nil, lastAt: 0
        )
    }

    func testAFixIsLearnedOnlyAfterTwoSightingsAndNoContradiction() {
        XCTAssertTrue(DictationLearningRule.learned(from: [record("cloud", "Claude")]).isEmpty)
        let twice = DictationLearningRule.learned(from: [record("cloud", "Claude"), record("Cloud", "Claude", kind: .redictated)])
        XCTAssertEqual(twice.map(\.correction), [DictationCorrection(heard: "cloud", meant: "Claude")])
        XCTAssertEqual(twice.first?.count, 2)
        XCTAssertTrue(DictationLearningRule.learned(from: [record("cloud", "Claude", count: 5, contradictions: 1)]).isEmpty)
        XCTAssertTrue(DictationLearningRule.learned(from: [record("cloud", "Claude", count: 3), record("cloud", "crowd")]).isEmpty,
                      "heard words fixed two ways are ambiguous")
        XCTAssertTrue(DictationLearningRule.learned(from: [record("like", "", count: 4, kind: .fillerRemoved)]).isEmpty)
    }

    func testACapitalYouUsedIsKeptAsTheSpelling() {
        let learned = DictationLearningRule.learned(from: [record("cloud", "claude"), record("cloud", "Claude")])
        XCTAssertEqual(learned.map(\.meant), ["Claude"])
    }

    func testMergingCountsPairsAndContradictsFixesYouKeptOrTookBack() {
        let fix = DictationEditPair(kind: .substituted, heard: "cloud", inserted: "cloud", meant: "Claude", before: "at", after: "nine")
        let edit = DictationEditResult(outcome: .edited, pairs: [fix], wordCount: 5, changedWordCount: 1)
        var records = TypingJournalDictationFix.merging(edit, into: [], at: 1)
        records = TypingJournalDictationFix.merging(edit, into: records, at: 2)
        XCTAssertEqual(records.count, 1)
        XCTAssertEqual(records.first?.count, 2)
        XCTAssertEqual(DictationLearningRule.learned(from: records).count, 1)

        // Hex applied it and you changed it back: never applied again.
        let undoneFix = DictationEditPair(kind: .substituted, heard: "cloud", inserted: "Claude", meant: "cloud")
        let reverted = TypingJournalDictationFix.merging(
            DictationEditResult(outcome: .edited, pairs: [undoneFix], wordCount: 4), into: records, at: 3
        )
        XCTAssertEqual(reverted.first?.contradictions, 1)
        XCTAssertTrue(DictationLearningRule.learned(from: reverted).isEmpty)

        // You corrected a dictation and left "cloud" alone in it.
        let kept = TypingJournalDictationFix.merging(
            DictationEditResult(outcome: .edited, wordCount: 6, keptHeard: [["the", "cloud", "is", "grey"]]),
            into: records, at: 4
        )
        XCTAssertEqual(kept.first?.contradictions, 1)
    }
}

@MainActor
final class DictationEditTrackerTests: XCTestCase {
    private var now: TimeInterval = 1_000
    private var results: [DictationEditResult] = []
    private var tracker: DictationEditTracker!

    override func setUp() async throws {
        results = []
        tracker = DictationEditTracker(now: { [unowned self] in self.now })
        tracker.onResult = { [unowned self] in self.results.append($0) }
    }

    private func context(_ before: String?, _ after: String? = "") -> DictationEditTracker.Context {
        DictationEditTracker.Context(before: before, after: after, hasText: !((before ?? "") + (after ?? "")).isEmpty)
    }

    func testASentDictationIsKept() {
        tracker.willInsert(heard: "see you soon", inserted: "See you soon.", context: context(""))
        tracker.observe(context(""), fromKeys: true) // the host hasn't caught up with the insertion yet
        XCTAssertTrue(tracker.isTracking)
        tracker.observe(context("See you soon."), fromKeys: true)
        tracker.observe(context(""), fromKeys: false) // the host sent it
        XCTAssertEqual(results.map(\.outcome), [.kept])
    }

    func testAnEditIsReportedWhenTheWindowEnds() {
        tracker.willInsert(heard: "meet at cloud nine", inserted: " Meet at cloud nine.", context: context("Hi,"))
        tracker.observe(context("Hi, Meet at cloud nine."), fromKeys: true)
        now += 10
        tracker.observe(context("Hi, Meet at Claude", " nine."), fromKeys: true)
        tracker.tick { context("Hi, Meet at Claude", " nine.") }
        XCTAssertTrue(results.isEmpty, "still inside the edit window")
        now += 51
        tracker.tick { context("Hi, Meet at Claude", " nine.") }
        XCTAssertEqual(results.map(\.outcome), [.edited])
        XCTAssertEqual(results.first?.pairs.map(\.meant), ["Claude"])
    }

    func testDeletingAndDictatingAgainIsARedictation() {
        tracker.willInsert(heard: "meet at cloud nine", inserted: "Meet at cloud nine.", context: context(""))
        tracker.observe(context("Meet at cloud nine."), fromKeys: true)
        tracker.observe(context(""), fromKeys: true) // Hex's delete key emptied it
        now += 5
        tracker.willInsert(heard: "meet at claude nine", inserted: "Meet at Claude nine.", context: context(""))
        XCTAssertEqual(results.map(\.outcome), [.redictated])
        XCTAssertEqual(results.first?.pairs.map(\.meant), ["Claude"])
    }

    func testADeletionWithoutADictationAfterItIsDeleted() {
        tracker.willInsert(heard: nil, inserted: "Meet at noon.", context: context(""))
        tracker.observe(context("Meet at noon."), fromKeys: true)
        tracker.observe(context(""), fromKeys: true)
        now += 10
        tracker.tick { context("") }
        XCTAssertTrue(results.isEmpty, "a dictation may still follow")
        now += 11
        tracker.tick { context("") }
        XCTAssertEqual(results.map(\.outcome), [.deleted])
    }

    func testUndoDictationIsUndone() {
        tracker.willInsert(heard: nil, inserted: "Meet at noon.", context: context(""))
        tracker.observe(context("Meet at noon."), fromKeys: true)
        tracker.didUndo()
        tracker.finish(context(""))
        XCTAssertEqual(results.map(\.outcome), [.undone])
    }

    func testNothingIsReportedWhenTheDictationWasNeverSeen() {
        tracker.willInsert(heard: nil, inserted: "Meet at noon.", context: context(nil, nil))
        tracker.observe(context(nil, nil), fromKeys: false)
        now += 61
        tracker.tick { context(nil, nil) }
        tracker.finish(nil)
        XCTAssertTrue(results.isEmpty)
        XCTAssertFalse(tracker.isTracking)
    }
}

/// The journal side: storing dictation results, the learned fixes the app reads, switched-off
/// fixes, Delete, and what the keys learn from typing corrections.
@MainActor
final class TypingJournalLearningTests: XCTestCase {
    private var directory: URL!
    private var suiteName: String!
    private var defaults: UserDefaults!
    private var settings: TypingJournalSettings!

    override func setUp() async throws {
        directory = FileManager.default.temporaryDirectory.appendingPathComponent("journal-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        suiteName = "TypingJournalLearningTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
        settings = TypingJournalSettings(defaults: defaults)
        settings.isEnabled = true
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: directory)
        defaults.removePersistentDomain(forName: suiteName)
    }

    private func fixResult() -> DictationEditResult {
        DictationEditResult(
            outcome: .edited,
            pairs: [DictationEditPair(kind: .substituted, heard: "cloud", inserted: "cloud", meant: "Claude")],
            wordCount: 5,
            changedWordCount: 1
        )
    }

    func testLearnHowITypeIsOnUntilTurnedOff() {
        let name = "TypingJournalDefault-\(UUID().uuidString)"
        let fresh = UserDefaults(suiteName: name)!
        defer { fresh.removePersistentDomain(forName: name) }
        let untouched = TypingJournalSettings(defaults: fresh)
        XCTAssertTrue(untouched.isEnabled, "an iPhone that never touched the switch learns")
        XCTAssertNotNil(TypingJournal.sharedIfEnabled(settings: untouched))
        untouched.isEnabled = false
        XCTAssertFalse(TypingJournalSettings(defaults: fresh).isEnabled, "a switch turned off stays off")
        XCTAssertNil(TypingJournal.sharedIfEnabled(settings: untouched))
        XCTAssertTrue(TypingJournal.storedDictationCorrections(settings: untouched, directory: directory).isEmpty)
        XCTAssertFalse(TypingJournalSettings(defaults: nil).isEnabled, "nowhere to learn without the App Group")
    }

    func testLearnedFixesReachTheAppAndCanBeSwitchedOffOrDeleted() {
        let journal = TypingJournal(directory: directory, settings: settings)
        journal.recordDictation(fixResult())
        XCTAssertTrue(TypingJournal.storedDictationCorrections(settings: settings, directory: directory).isEmpty)
        journal.recordDictation(fixResult())
        journal.recordDictation(DictationEditResult(outcome: .kept, wordCount: 3))

        let expected = [DictationCorrection(heard: "cloud", meant: "Claude")]
        XCTAssertEqual(TypingJournal.storedDictationCorrections(settings: settings, directory: directory), expected)
        XCTAssertEqual(journal.dictationCorrections(), expected)
        let stored = TypingJournal.load(from: directory.appendingPathComponent(TypingJournal.fileName))
        XCTAssertEqual(stored.count(of: .edited), 2)
        XCTAssertEqual(stored.count(of: .kept), 1)

        let id = stored.learnedDictationFixes[0].id
        settings.disabledDictationFixes = [id]
        XCTAssertTrue(TypingJournal.storedDictationCorrections(settings: settings, directory: directory).isEmpty)
        settings.disabledDictationFixes = []

        settings.isEnabled = false
        XCTAssertTrue(TypingJournal.storedDictationCorrections(settings: settings, directory: directory).isEmpty, "off applies nothing")
        settings.isEnabled = true

        TypingJournal.delete(in: directory, settings: settings)
        XCTAssertTrue(TypingJournal.load(from: directory.appendingPathComponent(TypingJournal.fileName)).isEmpty)
        XCTAssertTrue(journal.dictationCorrections().isEmpty, "a journal still in memory forgets too")
        journal.save()
        XCTAssertTrue(TypingJournal.load(from: directory.appendingPathComponent(TypingJournal.fileName)).isEmpty,
                      "and never writes the old data back")
    }

    func testAJournalPicksUpWhatAnotherProcessWrote() {
        let keyboard = TypingJournal(directory: directory, settings: settings)
        let app = TypingJournal(directory: directory, settings: settings)
        keyboard.recordDictation(fixResult())
        keyboard.recordDictation(fixResult())
        app.record(.retyped, typed: "teh", final: "the")
        let stored = TypingJournal.load(from: directory.appendingPathComponent(TypingJournal.fileName))
        XCTAssertEqual(stored.dictationFixes.first?.count, 2, "the app's write keeps the keyboard's dictation fixes")
        XCTAssertEqual(stored.corrections.count, 1)
    }

    func testAJournalWrittenBeforeDictationLearningStillLoads() throws {
        let url = directory.appendingPathComponent(TypingJournal.fileName)
        try Data(#"{"keys":{"a":{"count":3,"dx":0.3,"dy":0}},"corrections":[{"kind":"picked","typed":"teh","final":"the","at":1}]}"#.utf8)
            .write(to: url)
        let snapshot = TypingJournal.load(from: url)
        XCTAssertEqual(snapshot.tapCount, 3)
        XCTAssertEqual(snapshot.corrections.count, 1)
        XCTAssertTrue(snapshot.dictationFixes.isEmpty)
    }

    func testTypingLearningNeedsTwoMatchingFixesAndTwoRejections() {
        func correction(_ kind: TypingJournalCorrection.Kind, _ typed: String, _ final: String) -> TypingJournalCorrection {
            TypingJournalCorrection(kind: kind, typed: typed, final: final, at: 0)
        }
        let learning = TypingLearning.derive(from: [
            correction(.retyped, "thw", "the"), correction(.picked, "Thw", "The"),
            correction(.retyped, "hel", "hello"), correction(.retyped, "hel", "hello"),
            correction(.retyped, "wrod", "word"),
            correction(.retyped, "form", "from"), correction(.retyped, "form", "from"), correction(.retyped, "form", "fork"),
            correction(.undone, "kid", "kit"), correction(.keptTyped, "kid", "kit"),
            correction(.undone, "Bob", "bpb"),
        ])
        XCTAssertEqual(learning.typos, ["thw": "the"], "completions, single fixes, and two-way typos aren't learned")
        XCTAssertEqual(learning.rejected, ["kit"])
    }

    func testTheKeysUseTheJournalsLearning() {
        let target = FixtureTextTarget()
        target.autocapitalizationType = UITextAutocapitalizationType.none
        let wordsDefaults = UserDefaults(suiteName: suiteName + "-words")!
        defer { wordsDefaults.removePersistentDomain(forName: suiteName + "-words") }
        let engine = FullKeyboardEngine(
            target: target,
            autocorrect: AutocorrectEngine(checker: SystemTextChecker(), learnedWords: LearnedWordsStore(defaults: wordsDefaults))
        )
        engine.width = 393
        engine.configureForField()
        let journal = TypingJournal(directory: directory, settings: settings)
        journal.record(.undone, typed: "the", final: "teh")
        journal.record(.keptTyped, typed: "the", final: "teh")
        journal.record(.retyped, typed: "hexx", final: "hex")
        journal.record(.retyped, typed: "hexx", final: "hex")
        engine.journal = journal

        func type(_ text: String) {
            for character in text {
                let kind: KeyKind = character == " " ? .space : .character(String(character))
                let key = engine.layout.key(kind)!
                let center = CGPoint(x: key.frame.midX, y: key.frame.midY)
                engine.touchBegan(id: 1, at: center)
                engine.touchEnded(id: 1, at: center)
            }
        }
        type("teh ")
        XCTAssertEqual(target.text, "teh ", "an autocorrection you turned down twice never applies again")
        type("hexx ")
        XCTAssertEqual(target.text, "teh hex ", "a typo you fixed the same way twice is fixed on space")

        engine.journal = nil
        type("teh ")
        XCTAssertEqual(target.text, "teh hex the ", "nothing learned applies while Learn how I type is off")
    }
}

/// Show what I say and Type as I speak: the live words as the recogniser sends them, and the
/// field kept exactly as it would be without them.
@MainActor
final class LiveDictationTests: XCTestCase {
    func testVolatileResultsReplaceTheTailAndFinalOnesSettle() {
        var words = LiveTranscriptAccumulator()
        words.apply("Hey", isFinal: false)
        XCTAssertEqual(words.apply("Hey, I am running", isFinal: false), LiveTranscript(settled: "", tentative: "Hey, I am running"))
        XCTAssertEqual(words.apply("Hey, I am running late.", isFinal: true), LiveTranscript(settled: "Hey, I am running late.", tentative: ""))
        let next = words.apply(" Can you", isFinal: false)
        XCTAssertEqual(next.tentative, "Can you")
        XCTAssertEqual(next.text, "Hey, I am running late. Can you", "one space between the settled words and the tail")
        words.apply(" Can you grab a table?", isFinal: true)
        XCTAssertEqual(words.transcript.settled, "Hey, I am running late. Can you grab a table?")
        XCTAssertTrue(words.transcript.tentative.isEmpty)
    }

    func testChineseAndMarksJoinWithoutASpace() {
        XCTAssertEqual(LiveTranscript(settled: "我到了。", tentative: "你在哪里").text, "我到了。你在哪里")
        XCTAssertEqual(LiveTranscript(settled: "Ten minutes late", tentative: ".").text, "Ten minutes late.")
        XCTAssertTrue(LiveTranscript(settled: " ", tentative: "").isEmpty)
    }

    func testTheLongestDictationShowsOnlyItsEndFromAWordsStart() {
        let short = LiveTranscript(settled: "Hey, I am late.", tentative: "Can you")
        XCTAssertTrue(LiveDictationPreviewView.tail(of: short, characters: 200) == ("Hey, I am late.", " Can you"))
        let long = LiveTranscript(settled: "one two three four five six", tentative: "seven eight")
        let end = LiveDictationPreviewView.tail(of: long, characters: 20)
        XCTAssertEqual(end.settled, "five six", "no word shows cut")
        XCTAssertEqual(end.tentative, " seven eight")
        let tentativeOnly = LiveDictationPreviewView.tail(of: LiveTranscript(settled: "", tentative: "alpha beta gamma delta"), characters: 12)
        XCTAssertEqual(tentativeOnly.settled, "")
        XCTAssertEqual(tentativeOnly.tentative, "gamma delta")
    }

    func testTheSwitchesDefaultToShowingButNotTyping() throws {
        let suite = "anomaly.hex.tests.live-dictation"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defaults.removePersistentDomain(forName: suite)
        let settings = LiveDictationSettings(defaults: defaults)
        XCTAssertTrue(settings.showsPreview)
        XCTAssertFalse(settings.typesIntoField)
        settings.showsPreview = false
        XCTAssertFalse(settings.wantsLiveWords)
        settings.typesIntoField = true
        XCTAssertTrue(LiveDictationSettings(defaults: defaults).wantsLiveWords)
        defaults.removePersistentDomain(forName: suite)
    }

    func testTypesTheWordsRevisesOnlyTheTailAndHandsTheFieldBack() throws {
        let field = FixtureTextTarget(text: "Running 10 min late,")
        let typing = try XCTUnwrap(LiveFieldTyping(target: field))
        typing.show("I am running")
        XCTAssertEqual(field.text, "Running 10 min late, I am running", "a space before the words, as a dictation gets")
        typing.show("I am running about 730.")
        typing.show("I am running about 7.30. Can")
        XCTAssertEqual(field.text, "Running 10 min late, I am running about 7.30. Can")
        XCTAssertEqual(typing.typed, " I am running about 7.30. Can")
        XCTAssertTrue(typing.takeBack())
        XCTAssertEqual(field.text, "Running 10 min late,", "the dictation's own text goes in as without Type as I speak")
        XCTAssertEqual(typing.base, "Running 10 min late,")
    }

    func testStopsAndLeavesTheFieldOnceItNoLongerEndsWithTheWords() throws {
        let field = FixtureTextTarget(text: "")
        let typing = try XCTUnwrap(LiveFieldTyping(target: field))
        typing.show("See you at")
        field.insertText("!")
        typing.show("See you at ten")
        XCTAssertTrue(typing.stopped)
        XCTAssertEqual(field.text, "See you at!")
        XCTAssertFalse(typing.takeBack(), "never deletes what someone else typed")
        XCTAssertEqual(field.text, "See you at!")

        let moved = FixtureTextTarget(text: "Hi ")
        let second = try XCTUnwrap(LiveFieldTyping(target: moved))
        second.show("there")
        moved.adjustTextPosition(byCharacterOffset: -2)
        XCTAssertFalse(second.takeBack(), "the cursor moved: the words stay")
        XCTAssertEqual(moved.text, "Hi there")
    }

    func testWordsLeftByAHiddenKeyboardAreReplacedOnlyWhereTheFieldStillEndsWithThem() throws {
        let field = FixtureTextTarget(text: "Note: Hey, I'm running")
        let resumed = try XCTUnwrap(LiveFieldTyping(resuming: " Hey, I'm running", in: field))
        XCTAssertEqual(resumed.base, "Note:")
        XCTAssertTrue(resumed.takeBack())
        XCTAssertEqual(field.text, "Note:", "the held text then goes in as if they were never typed")

        XCTAssertNil(LiveFieldTyping(resuming: " Hey, I'm running", in: FixtureTextTarget(text: "Hey, I'm running late")))
        let quoted = FixtureTextTarget(text: "I\u{2019}m late")
        XCTAssertNotNil(LiveFieldTyping(resuming: "I'm late", in: quoted), "a host's smart apostrophe is still the word typed")
    }

    func testAHostThatReportsNothingIsNeverRevised() throws {
        let field = LaggingField(text: "Hello")
        field.lag = .nothing
        XCTAssertNil(LiveFieldTyping(target: field), "a field with text whose app reports none can't be checked")

        let empty = LaggingField(text: "")
        empty.lag = .nothing
        let typing = try XCTUnwrap(LiveFieldTyping(target: empty))
        typing.show("One")
        typing.show("One two")
        XCTAssertTrue(typing.stopped)
        XCTAssertEqual(empty.text, "One", "stops instead of deleting what it can't see")
    }

    func testNeverTypesInPrivateFieldsOrOverASelection() {
        XCTAssertFalse(LiveFieldTyping.mayType(secure: true, content: nil, selected: nil))
        XCTAssertFalse(LiveFieldTyping.mayType(secure: nil, content: .password, selected: nil))
        XCTAssertFalse(LiveFieldTyping.mayType(secure: nil, content: .oneTimeCode, selected: nil))
        XCTAssertFalse(LiveFieldTyping.mayType(secure: nil, content: nil, selected: "late"))
        XCTAssertTrue(LiveFieldTyping.mayType(secure: false, content: .emailAddress, selected: ""))
    }
}
