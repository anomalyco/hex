import XCTest
@testable import HEX

/// Two addresses said back to back, the second's local part the first's domain: the first is
/// written and the second left as said (the owner's decided change; the Swift rules before the
/// keyboard core trapped on these).
final class SpokenSymbolsBackToBackTests: XCTestCase {
    func testBackToBackAddressesKeepTheFirst() {
        XCTAssertEqual(
            SpokenCleanupRules.spokenSymbols(in: "a at sign b.com at sign c.com", language: "en-US"),
            "a@b.com at sign c.com"
        )
        XCTAssertEqual(
            SpokenCleanupRules.spokenSymbols(in: "mail john at the rate gmail dot com at the rate yahoo dot com",
                                             language: "en-US"),
            "mail john@gmail.com at the rate yahoo dot com"
        )
        // One address after another, apart, as before.
        XCTAssertEqual(
            SpokenCleanupRules.spokenSymbols(in: "john at sign gmail.com and mary at sign yahoo.com", language: "en-US"),
            "john@gmail.com and mary@yahoo.com"
        )
    }
}
