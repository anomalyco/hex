import XCTest
@testable import HEX

/// Automatic, the quick switch and the keyboard take the same Spanish order: the region's own
/// locale where Apple has it, else es-US, else es-MX, else Spain's. Apple maps es-CO to es-CL
/// and es-419 to es-MX, which must not answer a Colombian or Latin American phone.
final class DictationLanguageTests: XCTestCase {
    /// `SpeechTranscriber`'s Spanish on macOS 26: es-CL, es-ES, es-MX and es-US, with es-CO
    /// resolving to es-CL and es-419 (and es-AR, es) to es-MX.
    private func apple(_ wanted: Locale) async -> Locale? {
        let id = wanted.identifier(.bcp47)
        if id == "es-CO" { return Locale(identifier: "es-CL") }
        if ["es-419", "es-AR", "es"].contains(id) { return Locale(identifier: "es-MX") }
        if ["es-CL", "es-ES", "es-MX", "es-US"].contains(id) { return Locale(identifier: id) }
        return nil
    }

    private func pick(
        _ preferred: [String],
        speech: @escaping (Locale) async -> Locale?
    ) async -> String? {
        let language = await SpeechLanguagePicker.pick(
            explicit: nil,
            candidates: SpeechLanguagePicker.automaticCandidates(preferred: preferred, current: "en_US", region: "US"),
            region: "US",
            speech: speech,
            dictation: { _ in nil }
        )
        return language?.identifier
    }

    /// Automatic tries the identifier's own region first, then es-US, then es-MX, then Spain's.
    func testAutomaticTriesTheRegionThenLatinAmericasSpanish() {
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es-CO", "en-US"], current: "en_US", region: "US"),
            ["es-CO", "es-US", "es-MX", "es-ES", "en-US"]
        )
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es-419", "en-US"], current: "en_US", region: "US"),
            ["es-419", "es-US", "es-MX", "es-ES", "en-US"]
        )
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es-CL", "en-US"], current: "en_US", region: "US"),
            ["es-CL", "es-US", "es-MX", "es-ES", "en-US"]
        )
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es-MX", "en-US"], current: "en_US", region: "US"),
            ["es-MX", "es-US", "es-ES", "en-US"]
        )
        XCTAssertEqual(
            SpeechLanguagePicker.automaticCandidates(preferred: ["es-ES", "en-US"], current: "en_US", region: "US"),
            ["es-ES", "en-US"]
        )
    }

    /// A Chilean phone keeps es-CL; a Colombian one gets es-US rather than the es-CL Apple
    /// resolves es-CO to, and es-419 gets es-US rather than es-MX. Only Spain's still gives es-ES.
    func testAutomaticKeepsTheRegionAppleHas() async {
        let chile = await pick(["es-CL", "en-US"], speech: apple)
        XCTAssertEqual(chile, "es-CL", "not the es-US Latin America would give")
        let colombia = await pick(["es-CO", "en-US"], speech: apple)
        XCTAssertEqual(colombia, "es-US", "not the es-CL Apple resolves es-CO to")
        let latinAmerica = await pick(["es-419", "en-US"], speech: apple)
        XCTAssertEqual(latinAmerica, "es-US", "not the es-MX Apple resolves es-419 to")
        let argentina = await pick(["es-AR", "en-US"], speech: apple)
        XCTAssertEqual(argentina, "es-US")

        // Only Spain's here: Spain's Spanish, never English.
        let spainOnly: (Locale) async -> Locale? = { wanted in
            guard wanted.language.languageCode?.identifier == "es" else { return nil }
            return Locale(identifier: "es-ES")
        }
        let spanish = await pick(["es-CO", "en-US"], speech: spainOnly)
        XCTAssertEqual(spanish, "es-ES")
    }

    /// The quick switch takes the same order.
    func testTheSwitchKeepsTheRegionAppleHas() {
        let supported = ["en-US", "es-CL", "es-ES", "es-MX", "es-US"]
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-CL", in: supported, region: "US"), "es-CL")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-CO", in: supported, region: "US"), "es-US")
        XCTAssertEqual(DictationLanguageSwitch.supportedMatch(for: "es-419", in: supported, region: "US"), "es-US")
        XCTAssertEqual(
            DictationLanguageSwitch.supportedMatch(for: "es-CL", in: ["en-US", "es-ES"], region: "US"),
            "es-ES",
            "Spain's where Apple has no Latin American Spanish"
        )
    }

    /// The keyboard path takes the same order.
    func testSpanishDictatesTheRegionAppleHas() {
        let supported = ["en-US", "es-CL", "es-ES", "es-MX", "es-US"]
        XCTAssertEqual(
            KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es-CL"], region: "US"),
            "es-CL"
        )
        XCTAssertEqual(
            KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es-CO"], region: "US"),
            "es-US"
        )
        XCTAssertEqual(
            KeyboardLanguage.spanish.dictationLanguage(supported: supported, preferred: ["es-419"], region: "US"),
            "es-US"
        )
        XCTAssertEqual(
            KeyboardLanguage.spanish.dictationLanguage(supported: ["en-US", "es-ES"], preferred: ["es-CL"], region: "US"),
            "es-ES",
            "Spain's where Apple has no Latin American Spanish"
        )
    }

    /// Before iOS 26 `SFSpeechRecognizer` lists es-419, es-CL, es-CO, es-ES, es-MX and es-US:
    /// the phone's own region wins where it is listed, an unlisted one falls to es-US rather
    /// than the es-ES the recognizer answers, and Spain's only when it is the only Spanish.
    func testOnDeviceAutomaticKeepsTheRegionAppleHas() throws {
        let suite = "DictationLanguageTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let settings = LanguageSettings(defaults: defaults)
        let supported = ["es-419", "es-CL", "es-CO", "es-ES", "es-MX", "es-US"]
        XCTAssertEqual(
            OnDeviceSpeechLanguages.automatic(
                settings: settings,
                preferred: ["es-CO"],
                supported: supported,
                onDevice: { _ in true }
            ),
            "es-CO"
        )
        XCTAssertEqual(
            OnDeviceSpeechLanguages.automatic(
                settings: settings,
                preferred: ["es-419"],
                supported: supported,
                onDevice: { _ in true }
            ),
            "es-419"
        )
        XCTAssertEqual(
            OnDeviceSpeechLanguages.automatic(
                settings: settings,
                preferred: ["es-CL"],
                supported: supported,
                onDevice: { _ in true }
            ),
            "es-CL"
        )
        XCTAssertEqual(
            OnDeviceSpeechLanguages.automatic(
                settings: settings,
                preferred: ["es-AR"],
                supported: ["es-US", "es-ES"],
                onDevice: { _ in true }
            ),
            "es-US",
            "not the es-ES the recognizer answers for an unlisted region"
        )
        XCTAssertEqual(
            OnDeviceSpeechLanguages.automatic(
                settings: settings,
                preferred: ["es-CO"],
                supported: ["es-ES"],
                onDevice: { _ in true }
            ),
            "es-ES",
            "only Spain's: Spanish, never English"
        )
    }
}
