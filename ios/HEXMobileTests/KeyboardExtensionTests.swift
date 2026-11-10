import SwiftUI
import XCTest
@testable import HEX

/// The keyboard is a separate process this app embeds, and no test read what iOS
/// reads when it launches it: the built `HEXKeyboard.appex` bundle and its
/// `NSExtension` contract. A wrong one makes the keyboard appear and immediately
/// switch back to the system one, which looks like a crash and leaves nothing in
/// the app's own tests. The surfaces below are the same ones the extension mounts
/// in `KeyboardViewController.viewDidLoad`, so a trap there fails here.
@MainActor
final class KeyboardExtensionTests: XCTestCase {
    private var appexURL: URL {
        Bundle.main.bundleURL.appendingPathComponent("PlugIns/HEXKeyboard.appex")
    }

    private func extensionInfo() throws -> [String: Any] {
        XCTAssertTrue(
            FileManager.default.fileExists(atPath: appexURL.path),
            "HEXKeyboard.appex is not embedded in the app"
        )
        let data = try Data(contentsOf: appexURL.appendingPathComponent("Info.plist"))
        let plist = try PropertyListSerialization.propertyList(from: data, format: nil)
        return try XCTUnwrap(plist as? [String: Any])
    }

    func testExtensionDeclaresTheKeyboardService() throws {
        let info = try extensionInfo()
        XCTAssertEqual(info["CFBundlePackageType"] as? String, "XPC!")
        XCTAssertEqual(info["CFBundleIdentifier"] as? String, "com.kitlangton.hex.mobile.keyboard")

        let extensionKeys = try XCTUnwrap(info["NSExtension"] as? [String: Any])
        XCTAssertEqual(
            extensionKeys["NSExtensionPointIdentifier"] as? String,
            "com.apple.keyboard-service"
        )

        let attributes = try XCTUnwrap(extensionKeys["NSExtensionAttributes"] as? [String: Any])
        XCTAssertEqual(attributes["IsASCIICapable"] as? Bool, true)
        XCTAssertEqual(attributes["PrimaryLanguage"] as? String, "en-US")
        XCTAssertEqual(
            attributes["RequestsOpenAccess"] as? Bool,
            true,
            "Hex's keyboard reads the App Group, so it needs Full Access"
        )
    }

    /// iOS instantiates this class by name; an unexpanded build setting or a
    /// renamed class leaves the extension unable to launch at all.
    func testExtensionPrincipalClassIsTheBuiltController() throws {
        let info = try extensionInfo()
        let extensionKeys = try XCTUnwrap(info["NSExtension"] as? [String: Any])
        let principal = try XCTUnwrap(extensionKeys["NSExtensionPrincipalClass"] as? String)

        XCTAssertFalse(principal.contains("$("), "unexpanded build setting in NSExtensionPrincipalClass")
        XCTAssertEqual(principal, "HEXKeyboard.KeyboardViewController")

        let executable = try XCTUnwrap(info["CFBundleExecutable"] as? String)
        XCTAssertTrue(
            FileManager.default.fileExists(
                atPath: appexURL.appendingPathComponent(executable).path
            ),
            "the extension's executable is missing from its bundle"
        )
    }

    /// iOS refuses to launch an extension whose version differs from its app's.
    func testExtensionVersionMatchesTheApp() throws {
        let extensionInfo = try extensionInfo()
        let appInfo = try XCTUnwrap(Bundle.main.infoDictionary)

        XCTAssertEqual(
            extensionInfo["CFBundleShortVersionString"] as? String,
            appInfo["CFBundleShortVersionString"] as? String
        )
        XCTAssertEqual(
            extensionInfo["CFBundleVersion"] as? String,
            appInfo["CFBundleVersion"] as? String
        )
    }

    /// The dictation bar the extension mounts when the full-keyboard experiment is off.
    func testDictationBarMountsInAKeyboardSizedWindow() {
        let model = KeyboardViewModel()
        let hosting = UIHostingController(rootView: KeyboardSurfaceView(model: model))
        hosting.view.backgroundColor = .clear

        let window = UIWindow(frame: CGRect(x: 0, y: 0, width: 390, height: 140))
        window.rootViewController = hosting
        window.isHidden = false
        hosting.view.frame = window.bounds
        hosting.view.layoutIfNeeded()

        XCTAssertFalse(hosting.view.subviews.isEmpty)
        XCTAssertEqual(hosting.view.bounds.height, 140, accuracy: 0.5)
    }

    /// The full keyboard the extension mounts when the experiment is on, at the
    /// height the extension pins on its input view.
    func testFullKeyboardMountsAtItsReportedHeight() {
        let model = KeyboardViewModel()
        let view = FullKeyboardView(model: model, target: FixtureTextTarget())
        view.frame = CGRect(x: 0, y: 0, width: 390, height: view.preferredHeight)
        view.prepareForField()
        view.layoutIfNeeded()

        XCTAssertEqual(view.preferredHeight, KeyboardLayout.height(for: .portrait))
        XCTAssertGreaterThan(view.preferredHeight, 140)
        XCTAssertFalse(view.engine.layout.keys.isEmpty)
    }
}
