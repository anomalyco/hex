import Foundation
import UIKit
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

    // MARK: - What's new

    private func list(_ version: String) -> WhatsNewRelease {
        WhatsNewRelease(version: version, items: [])
    }

    @MainActor
    func testWhatsNewPicksTheNewestListAtOrBelowTheVersion() {
        let lists = [list("1.0.7"), list("1.0.5")]
        XCTAssertNil(WhatsNew.release(forAppVersion: "1.0.4", in: lists))
        XCTAssertEqual(WhatsNew.release(forAppVersion: "1.0.5", in: lists)?.version, "1.0.5")
        // A version without a list of its own keeps the last one, which has shown by then.
        XCTAssertEqual(WhatsNew.release(forAppVersion: "1.0.6", in: lists)?.version, "1.0.5")
        XCTAssertEqual(WhatsNew.release(forAppVersion: "1.0.10", in: lists)?.version, "1.0.7")
        // TestFlight stays at 9.0.0, above every App Store version: the newest list.
        XCTAssertEqual(WhatsNew.release(forAppVersion: "9.0.0", in: lists)?.version, "1.0.7")
        XCTAssertEqual(WhatsNew.release(forAppVersion: "9.0.0")?.version, "1.0.5")
        XCTAssertNil(WhatsNew.release(forAppVersion: "1.0.4"))
    }

    @MainActor
    func testWhatsNewNeverShowsOnAFirstRunAndStaysSeenAfterSetup() {
        XCTAssertNil(WhatsNew.decideAtLaunch(list("1.0.5"), defaults: defaults, firstRun: true))
        XCTAssertEqual(defaults.string(forKey: WhatsNew.seenKey), "1.0.5")
        XCTAssertNil(WhatsNew.decideAtLaunch(list("1.0.5"), defaults: defaults, firstRun: false))
    }

    @MainActor
    func testWhatsNewShowsOnceAfterAnUpdate() {
        // Updated from 1.0.4, which never saved a list.
        XCTAssertEqual(WhatsNew.decideAtLaunch(list("1.0.5"), defaults: defaults, firstRun: false)?.version, "1.0.5")
        WhatsNew.markSeen(list("1.0.5"), defaults: defaults)
        XCTAssertNil(WhatsNew.decideAtLaunch(list("1.0.5"), defaults: defaults, firstRun: false))
        // A later release's list shows once in its turn; a version without one shows nothing.
        XCTAssertEqual(WhatsNew.decideAtLaunch(list("1.0.7"), defaults: defaults, firstRun: false)?.version, "1.0.7")
        XCTAssertNil(WhatsNew.decideAtLaunch(nil, defaults: defaults, firstRun: false))
    }

    @MainActor
    func testWhatsNewWaitsForAPlainOpenOverHome() {
        let due = list("1.0.5")
        XCTAssertEqual(WhatsNew.shows(due, at: WhatsNewMoment()), due)
        XCTAssertNil(WhatsNew.shows(nil, at: WhatsNewMoment()))
        let blocked = [
            WhatsNewMoment(openedByLink: true), // the keyboard's hand-off or Open Hex, a words file, setup
            WhatsNewMoment(shownThisLaunch: true),
            WhatsNewMoment(setupSheet: true),
            WhatsNewMoment(settingsPage: true),
            WhatsNewMoment(presenting: true), // a words file's review, Safari, Send feedback
            WhatsNewMoment(dictationBusy: true), // recording, transcribing, asking for the microphone
            WhatsNewMoment(active: false), // iOS's microphone prompt, the screen locking
        ]
        for moment in blocked {
            XCTAssertNil(WhatsNew.shows(due, at: moment), "\(moment)")
        }
    }

    @MainActor
    func testEveryWhatsNewItemHasItsPicture() {
        for item in WhatsNew.releases.flatMap(\.items) {
            XCTAssertNotNil(UIImage(named: item.imageName), item.picture)
        }
    }
}
