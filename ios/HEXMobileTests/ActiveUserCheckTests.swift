import XCTest
@testable import HEX

final class ActiveUserCheckTests: XCTestCase {
    func testRequestsTheChannelManifestWithTheProtocolUserAgent() {
        let request = ActiveUserCheck(channel: "beta", version: "0.2.0-57").request
        XCTAssertEqual(request.url?.absoluteString, "https://anoma.ly/release/hex-ios/ios/beta.json")
        XCTAssertEqual(request.httpMethod, "GET")
        XCTAssertEqual(request.value(forHTTPHeaderField: "User-Agent"), "hex-ios/beta/0.2.0-57/ios")
        XCTAssertEqual(request.allHTTPHeaderFields?.keys.sorted(), ["User-Agent"])
    }

    func testChecksAtMostOnceAnHour() {
        XCTAssertTrue(ActiveUserCheck.isDue(lastCheck: nil, now: 10_000))
        XCTAssertFalse(ActiveUserCheck.isDue(lastCheck: 10_000, now: 10_000 + 59 * 60))
        XCTAssertTrue(ActiveUserCheck.isDue(lastCheck: 10_000, now: 10_000 + 60 * 60))
        // A clock set back doesn't silence the check for the gap.
        XCTAssertTrue(ActiveUserCheck.isDue(lastCheck: 10_000, now: 5_000))
    }
}
