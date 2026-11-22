import XCTest
@testable import HEX

/// Apple's speech assets, with the calls swapped out: callers joined on one reservation count
/// it once, and a hung reservation or release must not hold every later one.
@available(iOS 26.0, *)
final class AppleSpeechAssetReservationsTests: XCTestCase {
    private func waitFor(
        _ expected: AppleSpeechAssetReservations.DebugState?,
        of reservations: AppleSpeechAssetReservations,
        locale: Locale
    ) async throws {
        for _ in 0..<200 {
            if await reservations.debugState(locale) == expected { return }
            try await Task.sleep(for: .milliseconds(5))
        }
        XCTFail("the reservations never became \(String(describing: expected))")
    }

    func testTwoCallersJoinedOnOneReservationReleaseItOnce() async throws {
        let locale = Locale(identifier: "en-US")
        let gate = AsyncGate()
        let releases = CallCounter()
        let reservations = AppleSpeechAssetReservations(
            reserver: { _ in await gate.wait() },
            releaser: { _ in _ = releases.next() }
        )
        let first = Task { try await reservations.acquire(locale, limit: .seconds(2)) }
        try await waitFor(.acquiring(1), of: reservations, locale: locale)
        let second = Task { try await reservations.acquire(locale, limit: .seconds(2)) }
        try await waitFor(.acquiring(2), of: reservations, locale: locale)

        gate.open()
        try await first.value
        try await second.value
        let settled = await reservations.debugState(locale)
        XCTAssertEqual(settled, .reserved(2), "one reservation, counted once per caller")

        await reservations.release(locale)
        XCTAssertEqual(releases.count, 0, "the first release only drops a reference")
        await reservations.release(locale)
        XCTAssertEqual(releases.count, 1, "the last release asks Apple once")
    }

    func testAnAcquireThatNeverAnswersIsForgottenSoTheNextOneAsksAgain() async throws {
        let locale = Locale(identifier: "en-US")
        let calls = CallCounter()
        let reservations = AppleSpeechAssetReservations(
            reserver: { _ in
                if calls.next() == 1 {
                    try? await Task.sleep(for: .seconds(60))
                }
            },
            releaser: { _ in }
        )
        do {
            try await reservations.acquire(locale, limit: .milliseconds(150))
            XCTFail("the first call never answers")
        } catch {
            XCTAssertTrue(error is SpeechModelUnresponsive, "\(error)")
        }

        // The hung call was forgotten, so this starts a fresh one instead of joining it.
        try await reservations.acquire(locale, limit: .milliseconds(150))
        XCTAssertEqual(calls.count, 2)
        await reservations.release(locale)
    }

    func testAnAcquireThatGivesUpOnAHungReleaseForgetsIt() async throws {
        let locale = Locale(identifier: "en-US")
        let releaseGate = AsyncGate()
        let reservations = AppleSpeechAssetReservations(
            reserver: { _ in },
            releaser: { _ in await releaseGate.wait() }
        )
        try await reservations.acquire(locale, limit: .milliseconds(150))
        let releasing = Task { await reservations.release(locale) }
        try await waitFor(.releasing, of: reservations, locale: locale)

        // The acquire waits on the hung release, gives up, forgets it, and reserves afresh.
        let clock = ContinuousClock()
        let started = clock.now
        try await reservations.acquire(locale, limit: .milliseconds(150))
        XCTAssertLessThan(clock.now - started, .seconds(2))

        releaseGate.open()
        await releasing.value
        await reservations.release(locale)
    }
}

/// How many times the fake reservation or release was asked for.
private final class CallCounter: @unchecked Sendable {
    private let lock = NSLock()
    private var calls = 0

    var count: Int { lock.withLock { calls } }

    /// The 1-based number of this call.
    func next() -> Int {
        lock.withLock {
            calls += 1
            return calls
        }
    }
}

/// A call that hasn't finished until the test opens it.
private final class AsyncGate: @unchecked Sendable {
    private let lock = NSLock()
    private var opened = false

    func open() {
        lock.withLock { opened = true }
    }

    func wait() async {
        while !lock.withLock({ opened }) {
            try? await Task.sleep(for: .milliseconds(10))
        }
    }
}
