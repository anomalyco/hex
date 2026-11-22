import CoreGraphics
import XCTest
@testable import HEX

/// Settings › Experiments › Arabic keyboard: the iPhone and iPad Arabic layouts read from
/// Apple's own keyboards on the iOS 26.5 simulator, their long presses and harakat, the Arabic
/// punctuation and digits, and the switch that keeps العربية out of Settings › Keyboard ›
/// Languages and Automatic while it is off.
@MainActor
final class ArabicKeyboardTests: XCTestCase {
    private var suiteName: String!
    private var defaults: UserDefaults!

    override func setUp() async throws {
        suiteName = "ArabicKeyboardTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    private func row(_ layout: KeyboardLayout, _ index: Int) -> String {
        layout.keys.filter { $0.row == index }.compactMap(\.character).joined()
    }

    /// Apple's iPhone Arabic keyboard (iOS 26.5 simulator, AppleKeyboards ar@sw=Arabic): two
    /// eleven-key rows, then nine keys and a gap before delete, and no shift key.
    func testLetterRowsMatchApplesArabicKeyboard() {
        XCTAssertEqual(
            KeyboardLanguage.arabic.letterRows,
            ["ضصثقفغعهخحج", "شسيبلاتنمكة", "ءظطذدزروى"].map { $0.map(String.init) }
        )

        let layout = KeyboardLayout(width: 402, language: .arabic)
        XCTAssertEqual(row(layout, 0), "ضصثقفغعهخحج")
        XCTAssertEqual(row(layout, 1), "شسيبلاتنمكة")
        XCTAssertEqual(row(layout, 2), "ءظطذدزروى")
        XCTAssertNil(layout.key(.shift), "Arabic writes without case, so Apple's keyboard has no shift")
        let last = layout.key(.character("ى"))!.frame
        let delete = layout.key(.delete)!.frame
        XCTAssertGreaterThan(delete.minX - last.maxX, layout.metrics.gap, "Apple leaves the shift's room before delete")
        XCTAssertEqual(delete.maxX, layout.width - layout.metrics.side, accuracy: 0.001)
        XCTAssertEqual(layout.key(.character("ض"))!.frame.width, layout.key(.character("ء"))!.frame.width, accuracy: 0.001)
    }

    /// Every key Apple's callouts show, base first: ا → أ إ آ, ي and ى → ئ, و → ؤ, ه → ة,
    /// ء → أ إ ؤ ئ, and ل → لا. The harakat key holds the eleven marks.
    func testLongPressVariantsMatchApplesArabicKeyboard() {
        func options(_ key: String) -> [String] {
            AccentCatalog.options(for: key, uppercase: false, language: .arabic) ?? []
        }
        XCTAssertEqual(options("ا"), ["ا", "أ", "إ", "آ"])
        XCTAssertEqual(options("ي"), ["ي", "ئ"])
        XCTAssertEqual(options("ى"), ["ى", "ئ"])
        XCTAssertEqual(options("و"), ["و", "ؤ"])
        XCTAssertEqual(options("ه"), ["ه", "ة"])
        XCTAssertEqual(options("ء"), ["ء", "أ", "إ", "ؤ", "ئ"])
        XCTAssertEqual(options("ل"), ["ل", "لا"])
        XCTAssertNil(AccentCatalog.options(for: "ض", uppercase: false, language: .arabic), "a letter with no variant holds nothing")
        XCTAssertEqual(
            AccentCatalog.options(for: "ا", uppercase: false, language: .arabic),
            AccentCatalog.options(for: "ا", uppercase: true, language: .arabic),
            "no capitals to change them"
        )
    }

    /// Apple's harakat key sits between space and return: tapping it types fathatan, and its
    /// callout offers every mark, drawn on a dotted circle as Apple draws them.
    func testHarakatKeyAndCallout() {
        let layout = KeyboardLayout(width: 402, language: .arabic)
        let key = layout.key(.character("ً"))
        XCTAssertNotNil(key)
        XCTAssertEqual(key?.character, "ً")
        XCTAssertGreaterThan(key!.frame.minX, layout.key(.space)!.frame.minX, "right of space")
        XCTAssertLessThan(key!.frame.maxX, layout.key(.returnKey)!.frame.minX, "left of return")
        XCTAssertNil(KeyboardLayout(width: 402, layer: .numbers, language: .arabic).key(.character("ً")), "letters page only")

        let marks = AccentCatalog.options(for: "ً", uppercase: false, language: .arabic) ?? []
        XCTAssertEqual(marks.count, 11)
        for mark in ["ً", "ٌ", "ٍ", "َ", "ُ", "ِ", "ّ", "ْ", "ٓ", "ٰ", "ٖ"] {
            XCTAssertTrue(marks.contains(mark), mark)
        }
        XCTAssertEqual(KeyboardLanguage.calloutDisplay("ّ"), "◌ّ")
        XCTAssertEqual(KeyboardLanguage.calloutDisplay("a"), "a")
    }

    /// The final characters type through the engine: ء and ى are keys of their own, and the
    /// harakat key types a mark.
    func testArabicKeysTypeThroughTheEngine() {
        let target = FixtureTextTarget()
        target.autocapitalizationType = .none
        let engine = FullKeyboardEngine(target: target)
        engine.language = .arabic
        engine.width = 402
        engine.configureForField()
        for character in ["ء", "ى", "ً", "ا"] {
            let key = engine.layout.key(.character(character))!
            let point = CGPoint(x: key.frame.midX, y: key.frame.midY)
            engine.touchBegan(id: 1, at: point)
            engine.touchEnded(id: 1, at: point)
        }
        XCTAssertEqual(target.text, "ءىًا")
    }

    /// Apple's Arabic 123 page: Eastern Arabic digits, ، ؟ ! ’ on row 3, ؛ on row 2, and ١٢٣ /
    /// أبج as the page keys. The #+= page keeps Latin , and ? and still ends in ’.
    func testArabicDigitsAndPunctuationPages() {
        XCTAssertEqual(KeyboardLanguage.arabic.digits, "١٢٣٤٥٦٧٨٩٠")
        XCTAssertEqual(KeyboardLanguage.arabic.numbersKeyLabel, "١٢٣")
        XCTAssertEqual(KeyboardLanguage.arabic.lettersKeyLabel, "أبج")

        let letters = KeyboardLayout(width: 402, language: .arabic)
        XCTAssertEqual(letters.key(.layerSwitch(.numbers))?.label, "١٢٣")

        let numbers = KeyboardLayout(width: 402, layer: .numbers, language: .arabic)
        XCTAssertEqual(row(numbers, 0), "١٢٣٤٥٦٧٨٩٠")
        XCTAssertEqual(row(numbers, 2), ".،؟!’")
        XCTAssertTrue(row(numbers, 1).contains("؛"))
        XCTAssertTrue(row(numbers, 1).contains("”"))
        XCTAssertEqual(numbers.key(.layerSwitch(.letters))?.label, "أبج")

        let symbols = KeyboardLayout(width: 402, layer: .symbols, language: .arabic)
        XCTAssertEqual(row(symbols, 2), ".,?!’")
        XCTAssertEqual(symbols.key(.layerSwitch(.numbers))?.label, "١٢٣")

        XCTAssertEqual(AccentCatalog.options(for: "؟", uppercase: false, language: .arabic), ["؟", "?"])
        XCTAssertEqual(AccentCatalog.options(for: "،", uppercase: false, language: .arabic), ["،", ","])
    }

    /// The docked iPad's 123 and #+= rows carry Apple's Arabic marks, and its letters row ends
    /// in the Arabic comma.
    func testIPadArabicPagesUseArabicPunctuation() {
        let numbers = KeyboardLayout(width: 744, layer: .numbers, language: .arabic, pad: .docked(.mini))
        XCTAssertEqual(row(numbers, 1), "!@#$%^&*()؟")
        XCTAssertEqual(row(numbers, 2), ".|/=+…:؛،")
        XCTAssertNotNil(numbers.key(.returnKey), "the 123 page's return ends row 3")

        let symbols = KeyboardLayout(width: 744, layer: .symbols, language: .arabic, pad: .docked(.mini))
        XCTAssertEqual(row(symbols, 2), "%•\\,’’”;,")

        let letters = KeyboardLayout(width: 744, language: .arabic, pad: .docked(.mini))
        XCTAssertNotNil(letters.key(.character("،")))
        XCTAssertEqual(letters.key(.character("،"))?.label, "،")
    }

    /// Settings › Experiments › Comma and period keys uses the Arabic comma.
    func testCommaAndPeriodExperimentUsesTheArabicComma() {
        let layout = KeyboardLayout(width: 402, language: .arabic, commaPeriodKeys: true)
        XCTAssertNotNil(layout.key(.character("،")))
        XCTAssertNotNil(layout.key(.character(".")))
        XCTAssertNil(layout.key(.character(",")), "not the Latin comma")
    }

    func testArabicNamesItsInputModeAndHasNoDictionary() {
        XCTAssertEqual(KeyboardLanguage.arabic.displayName, "العربية")
        XCTAssertEqual(KeyboardLanguage.arabic.shortName, "العربية")
        XCTAssertEqual(KeyboardLanguage.arabic.inputModeLanguage, "ar-SA")
        XCTAssertEqual(KeyboardLanguage.arabic.baseLanguage, "ar")
        XCTAssertFalse(KeyboardLanguage.arabic.isLatin)
        XCTAssertFalse(KeyboardLanguage.arabic.hasLetterCase)
        XCTAssertFalse(KeyboardLanguage.arabic.showsShift)
        XCTAssertTrue(KeyboardLanguage.arabic.spellCheckCandidates.isEmpty)
        XCTAssertTrue(KeyboardLanguage.arabic.letterHolds.keys.contains("ء"))
        XCTAssertEqual(KeyboardLanguage.arabic.letterLayouts, [.own])
    }

    /// Off, العربية is not offered, Automatic falls back to English for an Arabic dictation
    /// language, and a stored ar is ignored, exactly as Korean's switch does.
    func testArabicIsGatedByItsExperiment() {
        XCTAssertEqual(ArabicKeyboardExperiment.key, "experiments.arabicKeyboard")
        let settings = LanguageSettings(defaults: defaults)
        settings.keyboardLanguages = [.english, .arabic]
        settings.currentKeyboardLanguage = .arabic
        defaults.set(false, forKey: ArabicKeyboardExperiment.key)

        XCTAssertEqual(settings.keyboardLanguages, [.english], "a stored ar is ignored while off")
        XCTAssertNil(settings.currentKeyboardLanguage, "a stored ar is ignored while off")
        XCTAssertNil(KeyboardLanguage.matching("ar-SA", experiments: .none))
        XCTAssertFalse(KeyboardLanguage.selectable(experiments: .none).contains(.arabic))
        XCTAssertEqual(
            KeyboardLanguage.automatic(
                dictationLanguage: "ar-SA",
                inputModes: [],
                preferredLanguages: [],
                experiments: .none
            ),
            .english,
            "Automatic falls back to English for Arabic while the experiment is off"
        )
        XCTAssertFalse(FullKeyboardSettings(defaults: defaults).arabicKeyboard)

        defaults.set(true, forKey: ArabicKeyboardExperiment.key)

        XCTAssertEqual(settings.keyboardLanguages, [.english, .arabic])
        XCTAssertEqual(settings.currentKeyboardLanguage, .arabic)
        XCTAssertEqual(KeyboardLanguage.matching("ar-SA", experiments: .init(arabic: true)), .arabic)
        XCTAssertTrue(KeyboardLanguage.selectable(experiments: .init(arabic: true)).contains(.arabic))
        XCTAssertEqual(
            KeyboardLanguage.automatic(
                dictationLanguage: "ar-SA",
                inputModes: [],
                preferredLanguages: [],
                experiments: .init(arabic: true)
            ),
            .arabic
        )
    }

    /// The switch turns the language on for the stored settings too, and the two experiments
    /// do not turn each other on.
    func testEachExperimentOnlyEnablesItsOwnLanguage() {
        let settings = LanguageSettings(defaults: defaults)
        defaults.set(false, forKey: KoreanKeyboardExperiment.key)
        defaults.set(false, forKey: ArabicKeyboardExperiment.key)
        settings.keyboardLanguages = [.english, .korean, .arabic]
        XCTAssertEqual(settings.keyboardLanguages, [.english])

        defaults.set(false, forKey: KoreanKeyboardExperiment.key)
        defaults.set(true, forKey: ArabicKeyboardExperiment.key)
        XCTAssertEqual(settings.keyboardLanguages, [.english, .arabic])
        XCTAssertFalse(KeyboardLanguage.selectable(experiments: .init(arabic: true)).contains(.korean))

        defaults.set(true, forKey: KoreanKeyboardExperiment.key)
        defaults.set(false, forKey: ArabicKeyboardExperiment.key)
        XCTAssertEqual(settings.keyboardLanguages, [.english, .korean])
        XCTAssertFalse(KeyboardLanguage.selectable(experiments: .init(korean: true)).contains(.arabic))
    }
}
