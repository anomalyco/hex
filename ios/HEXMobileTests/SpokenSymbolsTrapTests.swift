import HexKeyboardCore
import XCTest
@testable import HEX

/// Two addresses said back to back ("a at sign b.com at sign c.com"): the Swift rules before
/// the keyboard core trapped in `spokenSymbols`, and iOS still does, on purpose, in
/// `SpokenCleanupRules.overlappingAddresses(in:)` until the owner decides what it writes.
final class SpokenSymbolsTrapTests: XCTestCase {
    private let backToBack = ["a at sign b.com at sign c.com", "mail john at the rate gmail dot com at the rate yahoo dot com"]

    func testTheCoreReportsBackToBackAddresses() {
        for spoken in backToBack {
            let result = DictationCore.withText(spoken) { text in
                DictationCore.withLanguage("en-US") { language in
                    DictationCore.call { hk_dictation_spoken_symbols(text, language, $0) }
                }
            }
            XCTAssertNil(result.text, spoken)
            XCTAssertEqual(result.status, HK_DICTATION_OVERLAPPING_ADDRESSES, spoken)
        }
        // One address after another, apart, is fine.
        XCTAssertEqual(
            SpokenCleanupRules.spokenSymbols(in: "john at sign gmail.com and mary at sign yahoo.com", language: "en-US"),
            "john@gmail.com and mary@yahoo.com"
        )
    }

    /// Expected to trap, as before the core: XCTest can't survive a trap, so it's skipped.
    /// Once the owner decides, `overlappingAddresses(in:)` returns text and this asserts it.
    func testBackToBackAddressesTrapUntilTheOwnerDecides() throws {
        throw XCTSkip(
            "SpokenCleanupRules.spokenSymbols(in: \"a at sign b.com at sign c.com\", language: \"en-US\") traps "
                + "(preconditionFailure in SpokenCleanupRules.overlappingAddresses(in:)), as the Swift original did; "
                + "the owner hasn't decided what it writes instead."
        )
    }
}
