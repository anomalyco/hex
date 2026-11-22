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

    /// TestFlight (build 142): Learn how I type listed "Sunda → Saturday" beside real fixes, a
    /// word erased for another. A word erased and retyped teaches only when it was a slip of the
    /// new one: a change of mind isn't recorded, and one a journal already holds isn't counted,
    /// listed or learned.
    func testAWordErasedForAnotherWordTeachesNothing() {
        let slips = [("Ciry", "City"), ("Diaxuss", "Discuss"), ("Im", "I'm"), ("i", "I"), ("teh", "the"),
                     ("thw", "the"), ("hexx", "hex"), ("bonjor", "bonjour"), ("ity", "city")]
        for (typed, final) in slips {
            XCTAssertTrue(TypingLearning.isSlip(typed: typed, final: final), "\(typed) → \(final)")
        }
        let changes = [("Sunda", "Saturday"), ("is", "this"), ("reqe", "request"), ("agent", "subagent"),
                       ("bat", "cat"), ("take", "talk"), ("a", "do")]
        for (typed, final) in changes {
            XCTAssertFalse(TypingLearning.isSlip(typed: typed, final: final), "\(typed) → \(final)")
        }
        XCTAssertTrue(TypingLearning.isSlip(typed: "tu", final: "zu", language: "de"), "t beside z on German's keys")
        XCTAssertFalse(TypingLearning.isSlip(typed: "tu", final: "zu"), "not on English's")

        let journal = TypingJournal(directory: directory, settings: settings)
        for (erased, word) in [("Sunda", "Saturday"), ("Ciry", "City")] {
            journal.willErase(word: erased)
            journal.didFinish(word: word)
        }
        XCTAssertEqual(journal.current.corrections.map { "\($0.typed) → \($0.final)" }, ["Ciry → City"])

        func correction(_ kind: TypingJournalCorrection.Kind, _ typed: String, _ final: String) -> TypingJournalCorrection {
            TypingJournalCorrection(kind: kind, typed: typed, final: final, at: 0)
        }
        let older = [correction(.retyped, "Sunda", "Saturday"), correction(.retyped, "Sunda", "Saturday"),
                     correction(.retyped, "Ciry", "City"), correction(.retyped, "Ciry", "City")]
        XCTAssertEqual(TypingLearning.derive(from: older).typos, ["ciry": "City"], "never applied")
        XCTAssertEqual(older.filter(\.teaches).count, 2, "never counted or listed")
        XCTAssertTrue(correction(.autocorrected, "Sunda", "Saturday").teaches, "only what you erased and retyped")
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

    func testLongWordsScrollByWholeLinesAndDropLinesWellOutOfView() {
        let words = LiveWordsView()
        let line = words.lineHeight
        words.frame = CGRect(x: 0, y: 0, width: 320, height: LiveWordsView.edge + 4 * line)
        words.layoutIfNeeded()
        let sentence = "We need to finish the design review before Friday and send it to the whole team. "
        words.show(text: String(repeating: sentence, count: 8), settled: 0)
        XCTAssertGreaterThan(words.scrollOffset, 0, "the newest line is in view")
        XCTAssertEqual(words.scrollOffset.truncatingRemainder(dividingBy: line), 0, accuracy: 0.01, "whole lines at the top")
        let landed = expectation(description: "lines well above the box dropped")
        DispatchQueue.main.asyncAfter(deadline: .now() + LiveWordsMotion.change + 0.4) { landed.fulfill() }
        wait(for: [landed], timeout: 2)
        XCTAssertEqual(words.scrollOffset, 0, accuracy: 0.01, "at rest no line waits above the box")
        XCTAssertEqual(words.drawnHeight, 4 * line, accuracy: 0.5, "a long dictation draws only the box's lines, the last at its foot")
    }

    /// Ryan: "tap anywhere on the screen and it will stop dictation". A tap on the words finishes
    /// as Done does, with Show what I say alone and with Type as I speak; a slide, a hold, a tap
    /// as the words come in and a second tap do nothing, and ✕ still cancels.
    @MainActor
    func testATapOnTheWordsFinishesTheDictationAsDoneDoes() throws {
        for typing in [false, true] {
            let (view, model) = recordingKeyboard(showsPreview: true, typesIntoField: typing)
            var stops = 0, cancels = 0
            model.onStopTap = { stops += 1 }
            model.onCancelTap = { cancels += 1 }
            let words = try XCTUnwrap(view.subviews.lazy.compactMap { $0 as? LiveDictationPreviewView }.first, "the words in place of the keys")
            let at = CGPoint(x: words.bounds.midX, y: words.bounds.midY)
            let shown = words.shownAt
            words.touchDown(at: at, time: shown + 0.1)
            words.touchUp(at: at, time: shown + 0.15)
            XCTAssertEqual(stops, 0, "a tap as the words come in is the end of the tap that started it")
            words.touchDown(at: at, time: shown + 1)
            words.touchMoved(to: CGPoint(x: at.x + 30, y: at.y))
            words.touchUp(at: CGPoint(x: at.x + 30, y: at.y), time: shown + 1.1)
            XCTAssertEqual(stops, 0, "a thumb sliding across the words")
            words.touchDown(at: at, time: shown + 2)
            words.touchUp(at: at, time: shown + 2 + FullKeyboardEngine.accentHoldDelay)
            XCTAssertEqual(stops, 0, "a thumb resting on the words")
            try XCTUnwrap(control(in: view, labelled: "Cancel dictation")).sendActions(for: .touchUpInside)
            XCTAssertEqual(cancels, 1, "✕ still throws the dictation away")
            XCTAssertEqual(stops, 0)
            let near = CGPoint(x: at.x + 4, y: at.y + 3)
            words.touchDown(at: at, time: shown + 3)
            words.touchMoved(to: near)
            words.touchUp(at: near, time: shown + 3.08)
            XCTAssertEqual(stops, 1, "a tap finishes")
            words.touchDown(at: at, time: shown + 3.2)
            words.touchUp(at: at, time: shown + 3.25)
            XCTAssertFalse(words.accessibilityActivate())
            XCTAssertEqual(stops, 1, "once")
            XCTAssertEqual(cancels, 1)
        }
    }

    /// The two things only a real thumb would show, at their edges. A tap is the keys' own:
    /// it may move up to `FullKeyboardEngine.accentSlop` (10 pt) from where it landed and must
    /// lift before `FullKeyboardEngine.accentHoldDelay` (0.36 s). And none counts that lands
    /// within `LiveDictationPreviewView.settle` (0.4 s: the words' 0.3 s fade in and 0.1 s)
    /// of the words appearing, measured from touch-down on the touches' clock.
    @MainActor
    func testOnlyAKeysTapAfterTheWordsHaveComeInCounts() {
        XCTAssertEqual(FullKeyboardEngine.accentSlop, 10)
        XCTAssertEqual(FullKeyboardEngine.accentHoldDelay, 0.36, accuracy: 0.001)
        XCTAssertEqual(LiveDictationPreviewView.settle, 0.4, accuracy: 0.001)
        XCTAssertEqual(LiveDictationPreviewView.settle, LiveDictationPreviewHost.Motion.wordsIn + 0.1, accuracy: 0.001)
        let slop = FullKeyboardEngine.accentSlop, hold = FullKeyboardEngine.accentHoldDelay, settle = LiveDictationPreviewView.settle
        func finishes(down: TimeInterval, up: TimeInterval, path: [CGPoint] = [], lift: CGPoint? = nil) -> Bool {
            let words = LiveDictationPreviewView(frame: CGRect(x: 0, y: 0, width: 402, height: 220))
            var finished = 0
            words.onFinish = { finished += 1 }
            let at = CGPoint(x: 200, y: 100)
            words.touchDown(at: at, time: words.shownAt + down)
            for step in path { words.touchMoved(to: CGPoint(x: at.x + step.x, y: at.y + step.y)) }
            let end = lift ?? path.last ?? .zero
            words.touchUp(at: CGPoint(x: at.x + end.x, y: at.y + end.y), time: words.shownAt + up)
            return finished == 1
        }
        XCTAssertTrue(finishes(down: 1, up: 1.08), "a tap")
        XCTAssertTrue(finishes(down: 1, up: 1.08, path: [CGPoint(x: slop * 0.6, y: slop * 0.8)]), "a tap that rolls a key's slop")
        XCTAssertFalse(finishes(down: 1, up: 1.08, path: [CGPoint(x: slop + 0.5, y: 0)]), "a touch that moves further is no tap")
        XCTAssertFalse(finishes(down: 1, up: 1.2, path: [CGPoint(x: 0, y: -(slop + 1)), .zero]), "nor one that goes out and comes back")
        XCTAssertFalse(finishes(down: 1, up: 1.08, lift: CGPoint(x: 0, y: slop + 1)), "nor one that lifts further away")
        XCTAssertFalse(finishes(down: 1, up: 1.6, path: (1 ... 32).map { CGPoint(x: CGFloat($0) * 10, y: 0) }), "nor a thumb sliding across the words")
        XCTAssertTrue(finishes(down: 1, up: 1 + hold - 0.01), "lifted just inside a key's hold")
        XCTAssertFalse(finishes(down: 1, up: 1 + hold), "held as long as a key takes to open its accents")
        XCTAssertFalse(finishes(down: 0, up: 0.06), "the end of the tap that started the dictation")
        XCTAssertFalse(finishes(down: settle - 0.01, up: settle + 0.07), "landing just inside the window, even lifting after it")
        XCTAssertTrue(finishes(down: settle + 0.01, up: settle + 0.09), "landing just after it")
    }

    /// VoiceOver reads the words and finishes from them by activating or by Finish dictation;
    /// with the words switched off the keys stay as they are while it records.
    @MainActor
    func testVoiceOverFinishesFromTheWordsAndWithoutThemTheKeysStay() throws {
        let (view, model) = recordingKeyboard(showsPreview: true, typesIntoField: false)
        var stops = 0
        model.onStopTap = { stops += 1 }
        let words = try XCTUnwrap(view.subviews.lazy.compactMap { $0 as? LiveDictationPreviewView }.first)
        XCTAssertEqual(words.accessibilityLabel, LiveDictationPreviewView.placeholder, "still read as the words")
        XCTAssertEqual(words.accessibilityCustomActions?.map(\.name), ["Finish dictation"])
        XCTAssertTrue(words.accessibilityActivate())
        XCTAssertEqual(stops, 1)

        for typing in [false, true] {
            let (keys, _) = recordingKeyboard(showsPreview: false, typesIntoField: typing)
            XCTAssertTrue(keys.showsRecording)
            XCTAssertFalse(keys.subviews.contains { $0 is LiveDictationPreviewView })
            XCTAssertEqual(keys.plane.alpha, 1, "the keys stay and type")
        }
    }

    @MainActor
    private func recordingKeyboard(showsPreview: Bool, typesIntoField: Bool) -> (FullKeyboardView, KeyboardViewModel) {
        let t0 = 10_000.0
        let model = KeyboardViewModel()
        let view = FullKeyboardView(model: model, target: FixtureTextTarget(text: ""))
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        if showsPreview || typesIntoField { model.liveText.begin(showsPreview: showsPreview, typesIntoField: typesIntoField) }
        model.configureFixture(
            snapshot: KeyboardSnapshot(state: .recording, heartbeat: t0, expiresAt: t0 + 900, recordingStartedAt: t0 - 1, audioLevel: 0.4, jobID: "live"),
            now: t0
        )
        RunLoop.main.run(until: Date().addingTimeInterval(0.05))
        return (view, model)
    }

    private func control(in view: UIView, labelled label: String) -> UIControl? {
        for sub in view.subviews {
            if let control = sub as? UIControl, control.accessibilityLabel == label { return control }
            if let found = control(in: sub, labelled: label) { return found }
        }
        return nil
    }

    /// AJ dictates into Slack with Type as I speak and taps Send without stopping. The app
    /// emptying the field while it ends with the words is a send: the sent words are never typed
    /// or taken back again, and a fresh typing types the next words from the first into the
    /// empty field, three sends in a row, so ✓ inserts only the last dictation's text.
    func testAFieldTheAppEmptiedWithTheWordsInItWasSent() throws {
        let field = FixtureTextTarget(text: "")
        var typing = try XCTUnwrap(LiveFieldTyping(target: field))
        for message in ["See you at ten", "Bring the slides", "And the notes"] {
            typing.show(message)
            XCTAssertEqual(field.text, message)
            XCTAssertFalse(typing.wasSent)
            while field.hasText { field.deleteBackward() }
            XCTAssertTrue(typing.wasSent, "the app emptied the field: a send")
            typing.show(message + " please")
            XCTAssertEqual(field.text, "", "the sent dictation types nothing more")
            typing = try XCTUnwrap(LiveFieldTyping(target: field), "the fresh dictation")
            XCTAssertEqual(typing.base, "")
        }
        typing.show("Thanks")
        XCTAssertEqual(field.text, "Thanks", "typed from its first word")
        XCTAssertTrue(typing.takeBack())
        XCTAssertEqual(field.text, "", "✓'s text goes into the empty field, the sent words nowhere")
    }

    /// An app that changes the words without emptying the field (an edit, a mention) didn't
    /// send them: typing stops for that dictation, as before.
    func testAnEditThatLeavesTextIsNotASend() throws {
        let field = FixtureTextTarget(text: "")
        let typing = try XCTUnwrap(LiveFieldTyping(target: field))
        typing.show("Ask the team")
        for _ in 0 ..< 4 { field.deleteBackward() }
        field.insertText("@team")
        XCTAssertFalse(typing.wasSent)
        typing.show("Ask the team about it")
        XCTAssertTrue(typing.stopped)
        XCTAssertEqual(field.text, "Ask the @team", "hands off")
        XCTAssertFalse(typing.wasSent, "stopped: a later send of the edited text isn't this dictation's")
    }

    /// After a send the keyboard forgets Undo for the text that left the field, and Undo after
    /// the next dictation takes back only that one.
    @MainActor
    func testUndoAfterASendReachesOnlyTheNewDictation() throws {
        let field = FixtureTextTarget(text: "")
        let view = FullKeyboardView(model: KeyboardViewModel(), target: field)
        view.frame = CGRect(x: 0, y: 0, width: 402, height: 262)
        view.layoutIfNeeded()
        func undo() -> UIButton? { visibleButton(in: view, titled: "Undo dictation") }
        field.insertText("Earlier words.")
        view.didInsertDictation("Earlier words.", before: "", after: "")
        XCTAssertNotNil(undo())
        while field.hasText { field.deleteBackward() }
        view.hideUndo()
        XCTAssertNil(undo(), "nothing to undo: the text left with the send")
        field.insertText("Next words.")
        view.didInsertDictation("Next words.", before: "", after: "")
        try XCTUnwrap(undo()).sendActions(for: .touchUpInside)
        XCTAssertEqual(field.text, "")
    }

    private func visibleButton(in view: UIView, titled title: String) -> UIButton? {
        for sub in view.subviews where !sub.isHidden && sub.alpha > 0 {
            if let button = sub as? UIButton, button.configuration?.title == title { return button }
            if let found = visibleButton(in: sub, titled: title) { return found }
        }
        return nil
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
