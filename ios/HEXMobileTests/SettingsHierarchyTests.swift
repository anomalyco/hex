import Foundation
import XCTest
@testable import HEX

final class SettingsHierarchyTests: XCTestCase {
    private var suiteName = ""
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "hex.tests.settings.(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    func testLiveDictationModeMapsCleanlyToExistingSettingsKeys() {
        let settings = LiveDictationSettings(defaults: defaults)
        // Default: preview on, typing off -> .preview
        XCTAssertEqual(LiveDictationMode.read(from: settings), .preview)

        LiveDictationMode.off.write(to: settings)
        XCTAssertFalse(settings.showsPreview)
        XCTAssertFalse(settings.typesIntoField)
        XCTAssertFalse(settings.wantsLiveWords)
        XCTAssertEqual(LiveDictationMode.read(from: settings), .off)

        LiveDictationMode.typeIntoField.write(to: settings)
        XCTAssertFalse(settings.showsPreview)
        XCTAssertTrue(settings.typesIntoField)
        XCTAssertTrue(settings.wantsLiveWords)
        XCTAssertEqual(LiveDictationMode.read(from: settings), .typeIntoField)

        LiveDictationMode.preview.write(to: settings)
        XCTAssertTrue(settings.showsPreview)
        XCTAssertFalse(settings.typesIntoField)
        XCTAssertTrue(settings.wantsLiveWords)
        XCTAssertEqual(LiveDictationMode.read(from: settings), .preview)
    }

    func testLegacyBothEnabledReadsAsTypeIntoField() {
        let settings = LiveDictationSettings(defaults: defaults)
        settings.showsPreview = true
        settings.typesIntoField = true
        XCTAssertEqual(LiveDictationMode.read(from: settings), .typeIntoField)
    }
}
