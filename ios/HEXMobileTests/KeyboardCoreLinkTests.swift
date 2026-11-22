import HexKeyboardCore
import XCTest
@testable import HEX

/// The keyboard's Rust core (`crates/keyboard`) links into the app and opens the model the
/// keyboard ships.
@MainActor
final class KeyboardCoreLinkTests: XCTestCase {
    func testTheCoreIsTheHeadersVersion() {
        XCTAssertEqual(hk_abi_version(), UInt32(HK_ABI_VERSION))
    }

    func testTheCoreOpensTheBundledEnglishModel() throws {
        let model = try XCTUnwrap(TypingDecoderModel.bundled("en"), "the English model is bundled")
        let core = try XCTUnwrap(KeyboardCoreModel(url: model.url))
        XCTAssertEqual(core.wordCount, 86_133)
        XCTAssertEqual(model.core?.wordCount, model.wordCount)
        XCTAssertTrue(model.core === model.core, "opened once")
    }

    func testTheCoreOpensNothingWhereThereIsNoModel() {
        XCTAssertNil(KeyboardCoreModel(url: URL(fileURLWithPath: "/nonexistent/en.hextm")))
    }
}
