import Foundation

/// Preparation never answered: a Speech call that never returns, rather than a download that
/// stopped. The failure offers Try again, so the screen never stays on "Checking the speech
/// model" forever.
nonisolated struct SpeechModelUnresponsive: HexDescribedError {
    var errorDescription: String? {
        String(localized: "The speech model isn't ready. Try again.")
    }
}

/// Runs one Apple step under `StallWatch`'s deadline, which doesn't count time iOS has Hex
/// suspended, and reports it as `SpeechModelUnresponsive` instead of a stopped download. The
/// step reports no progress of its own, so the whole wait is bounded.
nonisolated enum SpeechModelDeadline {
    static func run<T: Sendable>(
        _ limit: Duration,
        operation: @escaping @Sendable () async throws -> T
    ) async throws -> T {
        do {
            return try await StallWatch.run(
                limit: limit,
                progress: { () -> Double? in 0 },
                operation: operation
            )
        } catch is StallWatch.Stalled {
            throw SpeechModelUnresponsive()
        }
    }
}
