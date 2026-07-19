import XCTest

final class KeyboardBridgeTests: XCTestCase {
    func testSnapshotAvailabilityRequiresFreshHeartbeatAndActiveSession() {
        let now = 1_000.0
        let available = snapshot(heartbeat: now - 2.9, expiresAt: now + 1)
        let stale = snapshot(heartbeat: now - 3, expiresAt: now + 1)
        let expired = snapshot(heartbeat: now, expiresAt: now)

        XCTAssertTrue(available.isAvailable(at: now))
        XCTAssertFalse(stale.isAvailable(at: now))
        XCTAssertFalse(expired.isAvailable(at: now))
    }

    func testKeyboardMessagesRoundTripThroughJSON() throws {
        let command = KeyboardCommand(id: "command", kind: .stop, jobID: "job")
        let snapshot = snapshot(heartbeat: 10, expiresAt: 20)
        let encoder = JSONEncoder()
        let decoder = JSONDecoder()

        XCTAssertEqual(
            try decoder.decode(KeyboardCommand.self, from: encoder.encode(command)),
            command
        )
        XCTAssertEqual(
            try decoder.decode(KeyboardSnapshot.self, from: encoder.encode(snapshot)),
            snapshot
        )
    }

    private func snapshot(heartbeat: TimeInterval, expiresAt: TimeInterval) -> KeyboardSnapshot {
        KeyboardSnapshot(
            state: .ready,
            heartbeat: heartbeat,
            expiresAt: expiresAt,
            jobID: "job",
            resultID: "result",
            transcript: "hello",
            message: nil
        )
    }
}
