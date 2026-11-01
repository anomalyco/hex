import FoundationModels
import XCTest
@testable import HEX

final class TranscriptCleanupGuardTests: XCTestCase {
    private let formatted = "I know Pi's website has the agent change. Its own website, except in a very ugly way, so maybe there's some shared idea there, but this would be funnier and cooler looking, so."
    private let meant = "I know Pi's website has the agent change its own website, except in a very ugly way, so maybe there's some shared idea there, but this would be funnier and cooler looking, so"

    func testAcceptsPunctuationAndCaseChanges() {
        XCTAssertTrue(TranscriptCleanupGuard.keepsWords(original: formatted, cleaned: meant))
    }

    func testTreatsCurlyAndStraightApostrophesAlike() {
        XCTAssertTrue(TranscriptCleanupGuard.keepsWords(original: "There's Pi's site.", cleaned: "There’s Pi’s site"))
    }

    func testRejectsChangedWords() {
        XCTAssertFalse(
            TranscriptCleanupGuard.keepsWords(
                original: formatted,
                cleaned: meant.replacingOccurrences(of: "funnier", with: "funnier, nicer")
            )
        )
        XCTAssertFalse(TranscriptCleanupGuard.keepsWords(original: "See you at ten.", cleaned: "See you at 10."))
        XCTAssertFalse(TranscriptCleanupGuard.keepsWords(original: "See you at ten.", cleaned: "you see at ten"))
    }

    func testRejectsEmptyText() {
        XCTAssertFalse(TranscriptCleanupGuard.keepsWords(original: "", cleaned: ""))
    }
}

@MainActor
final class AppleIntelligenceCleanupLiveTests: XCTestCase {
    /// Measures the real model where the simulator has one; skipped elsewhere.
    func testLiveCleanupLatency() async throws {
        let cleaner = AppleIntelligenceCleaner()
        guard cleaner.isAvailable, #available(iOS 26.0, *) else {
            throw XCTSkip("Apple Intelligence is not available on this device")
        }
        let input = TranscriptFormatter.format(
            "I know Pi's website has the agent change. Its own website, except in a very ugly way, so maybe there's some... Shared idea there, but this would be funnier and cooler looking, so...",
            style: .polished
        )

        for attempt in 1...3 {
            cleaner.prewarm()
            try await Task.sleep(for: .milliseconds(500))
            let clock = ContinuousClock()
            var result: String?
            let elapsed = await clock.measure {
                result = await cleaner.clean(input)
            }
            print("CLEANUP_LIVE attempt=\(attempt) elapsed=\(elapsed) accepted=\(result != nil) result=\(result ?? "-")")
        }

        let session = LanguageModelSession(model: .default, instructions: AppleIntelligenceCleaner.instructions)
        session.prewarm()
        try await Task.sleep(for: .milliseconds(500))
        let clock = ContinuousClock()
        var output = ""
        let unbounded = try await clock.measure {
            output = try await session.respond(to: input, options: GenerationOptions(sampling: .greedy)).content
        }
        print("CLEANUP_UNBOUNDED elapsed=\(unbounded) output=\(output)")
    }
}
