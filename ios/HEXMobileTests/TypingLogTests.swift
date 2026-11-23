import UIKit
import XCTest
@testable import HEX

/// Record typing logs: line format, day files, the skip rule, and a real full keyboard
/// typing through the logger.
@MainActor
final class TypingLogTests: XCTestCase {
    private var directory: URL!

    override func setUp() async throws {
        directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("TypingLogTests-\(UUID().uuidString)", isDirectory: true)
    }

    override func tearDown() async throws {
        try? FileManager.default.removeItem(at: directory)
    }

    private static var utc: Calendar {
        var calendar = Calendar(identifier: .gregorian)
        calendar.timeZone = TimeZone(identifier: "UTC")!
        return calendar
    }

    /// 2026-11-19 12:00:00 UTC.
    private static let noon: TimeInterval = 1_795_089_600

    private func event(_ type: String, at seconds: TimeInterval, seq: Int = 1, fields: [TypingLogField] = [], detail: TypingLogEvent.Detail? = nil) -> TypingLogEvent {
        TypingLogEvent(type: type, wallMS: Int64(seconds * 1000), monoMS: 5_000.25, session: "s1", seq: seq, fields: fields, detail: detail)
    }

    private func lines(_ url: URL) throws -> [[String: Any]] {
        let text = try String(contentsOf: url, encoding: .utf8)
        XCTAssertTrue(text.isEmpty || text.hasSuffix("\n"), "every line ends in a newline")
        return try text.split(separator: "\n").map { line in
            try XCTUnwrap(JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any], String(line))
        }
    }

    // MARK: - Encoder

    func testLineStartsWithTheCommonFieldsAndParses() throws {
        let line = TypingLogEncoder.line(event("touch", at: Self.noon, seq: 7, fields: [
            TypingLogField("x", .pt(12.346)),
            TypingLogField("ts", .ms(1234.5678)),
            TypingLogField("key", .string("a")),
            TypingLogField("under", .null),
            TypingLogField("frame", .rect(CGRect(x: 1, y: 2.5, width: 30, height: 45))),
        ]))
        XCTAssertEqual(
            line,
            #"{"v":1,"t":1795089600000,"mt":5000.25,"type":"touch","session":"s1","seq":7,"x":12.35,"ts":1234.568,"key":"a","under":null,"frame":[1,2.5,30,45]}"# + "\n"
        )
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
        XCTAssertEqual(object["v"] as? Int, TypingLogSchema.version)
        XCTAssertEqual(object["type"] as? String, "touch")
    }

    func testStringsAreEscapedAndNumbersNeverUseExponents() throws {
        let tricky = "a \"quote\" \\ back\nslash\ttab\u{1} é 👍🏽"
        let line = TypingLogEncoder.line(event("edit", at: Self.noon, fields: [
            TypingLogField("inserted", .string(tricky)),
            TypingLogField("tiny", .number(0.0004, places: 3)),
            TypingLogField("small", .number(-0.05, places: 3)),
            TypingLogField("nan", .number(.nan, places: 2)),
            TypingLogField("nested", .object([TypingLogField("list", .strings(["x", "y"]))])),
        ]))
        XCTAssertFalse(line.dropLast().contains("\n"), "one physical line")
        XCTAssertTrue(line.contains(#""tiny":0,"small":-0.05,"nan":null"#), line)
        let object = try XCTUnwrap(JSONSerialization.jsonObject(with: Data(line.utf8)) as? [String: Any])
        XCTAssertEqual(object["inserted"] as? String, tricky)
        XCTAssertEqual((object["nested"] as? [String: Any])?["list"] as? [String], ["x", "y"])
    }

    // MARK: - Day files

    func testEventsGoToTheirLocalDayAndLayoutsAreWrittenInFullOncePerFile() throws {
        let store = TypingLogStore(directory: directory, calendar: Self.utc)
        let detail = TypingLogEvent.Detail(key: "abc") { [TypingLogField("keys", .strings(["q", "w"]))] }
        store.append([
            event("layout", at: Self.noon, seq: 1, fields: [TypingLogField("hash", .string("abc"))], detail: detail),
            event("layout", at: Self.noon + 60, seq: 2, fields: [TypingLogField("hash", .string("abc"))], detail: detail),
            // 00:30 the next day (UTC).
            event("layout", at: Self.noon + 12.5 * 3600, seq: 3, fields: [TypingLogField("hash", .string("abc"))], detail: detail),
        ])
        store.waitUntilWritten()

        let days = TypingLogStore.days(in: directory)
        XCTAssertEqual(days.map(\.day), ["2026-11-20", "2026-11-19"])
        let first = try lines(days[1].url)
        XCTAssertEqual(first.count, 2)
        XCTAssertEqual(first[0]["keys"] as? [String], ["q", "w"])
        XCTAssertNil(first[0]["repeat"])
        XCTAssertNil(first[1]["keys"])
        XCTAssertEqual(first[1]["repeat"] as? Bool, true)
        let second = try lines(days[0].url)
        XCTAssertEqual(second.first?["keys"] as? [String], ["q", "w"], "a new day's file gets the layout in full again")
        XCTAssertEqual(TypingLogStore.countEvents(in: days[1].url), 2)

        // Delete all while the writer holds the file open: the next lines start a new file.
        TypingLogStore.deleteAll(in: directory)
        XCTAssertTrue(TypingLogStore.days(in: directory).isEmpty)
        store.append([event("layout", at: Self.noon + 13 * 3600, seq: 4, fields: [TypingLogField("hash", .string("abc"))], detail: detail)])
        store.waitUntilWritten()
        let fresh = try lines(XCTUnwrap(TypingLogStore.days(in: directory).first).url)
        XCTAssertEqual(fresh.first?["keys"] as? [String], ["q", "w"])
    }

    func testADayStopsAtItsCapWithOneCapLineEvenAcrossProcesses() throws {
        let cap = 4096
        let padding = TypingLogField("pad", .string(String(repeating: "x", count: 200)))
        let store = TypingLogStore(directory: directory, cap: cap, calendar: Self.utc)
        store.append((1...40).map { event("edit", at: Self.noon + Double($0), seq: $0, fields: [padding]) })
        store.waitUntilWritten()

        let url = try XCTUnwrap(TypingLogStore.days(in: directory).first).url
        var parsed = try lines(url)
        XCTAssertEqual(parsed.last?["type"] as? String, "cap_reached")
        XCTAssertEqual(parsed.filter { $0["type"] as? String == "cap_reached" }.count, 1)
        let firstDropped = try XCTUnwrap(parsed.last?["seq"] as? Int)
        XCTAssertEqual(firstDropped, parsed.count, "seq names the first event not written")
        var size = try XCTUnwrap(try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int)
        XCTAssertLessThanOrEqual(size, cap)

        // Another process (a new keyboard) opening the capped file writes nothing more.
        let later = TypingLogStore(directory: directory, cap: cap, calendar: Self.utc)
        later.append([event("edit", at: Self.noon + 100, seq: 1, fields: [TypingLogField("tiny", .int(1))])])
        later.waitUntilWritten()
        parsed = try lines(url)
        XCTAssertEqual(parsed.last?["type"] as? String, "cap_reached")
        size = try XCTUnwrap(try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int)
        XCTAssertLessThanOrEqual(size, cap)

        // The next day starts again.
        later.append([event("edit", at: Self.noon + 86_400, seq: 2)])
        later.waitUntilWritten()
        XCTAssertEqual(TypingLogStore.days(in: directory).count, 2)
    }

    func testPruneKeepsTodayAndThirteenDaysBefore() throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let names = ["2026-11-19", "2026-11-06", "2026-11-05", "2026-10-01"].map { "\($0).ndjson" } + ["notes.txt", "2026-1-1.ndjson"]
        for name in names {
            FileManager.default.createFile(atPath: directory.appendingPathComponent(name).path, contents: Data("{}\n".utf8))
        }
        TypingLogStore.prune(in: directory, now: Date(timeIntervalSince1970: Self.noon), calendar: Self.utc)
        let left = try FileManager.default.contentsOfDirectory(atPath: directory.path).sorted()
        XCTAssertEqual(left, ["2026-1-1.ndjson", "2026-11-06.ndjson", "2026-11-19.ndjson", "notes.txt"])
    }

    // MARK: - What is never logged

    func testPasswordCodeAndCardFieldsAreSkippedEverythingElseIsLogged() {
        for type in [UITextContentType.password, .newPassword, .oneTimeCode, .creditCardNumber, .creditCardSecurityCode, .creditCardExpiration] {
            XCTAssertEqual(TypingLogPrivacy.skipReason(isSecure: false, textContentType: type), "textContentType:\(type.rawValue)")
        }
        XCTAssertEqual(TypingLogPrivacy.skipReason(isSecure: true, textContentType: nil), "secureTextEntry")
        for type in [nil, UITextContentType.username, .emailAddress, .URL, .name, .fullStreetAddress] {
            XCTAssertNil(TypingLogPrivacy.skipReason(isSecure: false, textContentType: type), "\(String(describing: type))")
        }
        var noAutocorrect = TypingLogTraits()
        noAutocorrect.autocorrection = "no"
        XCTAssertNil(noAutocorrect.skipReason, "fields with autocorrection off are still logged")
        XCTAssertNil(TypingLogger.sharedIfEnabled(settings: TypingLogSettings(defaults: nil)), "no logger while the experiment is off")
    }

    func testTypingLogsTurnOffOnceAndKeepWhatTheyRecorded() throws {
        let name = "TypingLogMigration-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: name))
        defer { defaults.removePersistentDomain(forName: name) }
        let store = TypingLogStore(directory: directory)
        let logger = TypingLogger(store: store)
        logger.contextSource = { TypingLogContext(before: "", after: "", hasText: false) }
        logger.beginSession(Self.info(TypingLogTraits()))
        logger.endSession(reason: "disappear")
        store.waitUntilWritten()
        let recorded = TypingLogStore.days(in: directory).count
        XCTAssertGreaterThan(recorded, 0)

        let settings = TypingLogSettings(defaults: defaults)
        settings.isEnabled = true
        settings.turnOffOnce()
        XCTAssertFalse(settings.isEnabled, "the migration turns it off")
        XCTAssertEqual(TypingLogStore.days(in: directory).count, recorded, "the logs already recorded stay")
        settings.isEnabled = true
        settings.turnOffOnce()
        XCTAssertTrue(settings.isEnabled, "turned back on after the migration, it stays on")

        let neverOn = TypingLogSettings(defaults: nil)
        neverOn.turnOffOnce()
        XCTAssertFalse(neverOn.isEnabled)
    }

    func testASkippedFieldWritesOneLineAndNothingElse() throws {
        let store = TypingLogStore(directory: directory)
        let logger = TypingLogger(store: store)
        logger.contextSource = { TypingLogContext(before: "hunter", after: "", hasText: true) }
        var traits = TypingLogTraits()
        traits.textContentType = UITextContentType.password.rawValue
        logger.beginSession(Self.info(traits))
        XCTAssertFalse(logger.isRecording)
        logger.edit("insert", [TypingLogField("inserted", .string("2"))])
        logger.context("space")
        logger.touch(.began, id: 1, sample: TypingLogTouchSample(point: .zero, timestamp: 1), key: nil)
        logger.endSession(reason: "disappear")
        store.waitUntilWritten()

        let parsed = try lines(XCTUnwrap(TypingLogStore.days(in: directory).first).url)
        XCTAssertEqual(parsed.count, 1)
        XCTAssertEqual(parsed[0]["type"] as? String, "skipped")
        XCTAssertEqual(parsed[0]["reason"] as? String, "textContentType:password")
        XCTAssertFalse(try String(contentsOf: TypingLogStore.days(in: directory)[0].url, encoding: .utf8).contains("hunter"))
    }

    // MARK: - Which field

    /// The keyboard's proxy names no document early on (nil, though UIKit declares it
    /// non-optional): reading it gives nil instead of trapping, and a document reads as itself.
    func testDocumentIdentifierReadsNilInsteadOfTrapping() {
        let proxy = DocumentProxyStandIn()
        XCTAssertNil(TypingLogTraits.documentIdentifier(of: proxy))
        let id = UUID()
        proxy.documentIdentifier = id as NSUUID
        XCTAssertEqual(TypingLogTraits.documentIdentifier(of: proxy), id)
        XCTAssertNil(TypingLogTraits.documentIdentifier(of: NSObject()), "an object without one")
    }

    /// A document that wasn't named yet is the field the session is in; another document, or
    /// other traits, is another field.
    func testAnUnnamedDocumentIsTheSameField() {
        let unnamed = TypingLogTraits()
        var named = TypingLogTraits()
        named.document = "A"
        var other = TypingLogTraits()
        other.document = "B"
        XCTAssertFalse(unnamed.isOtherField(than: named))
        XCTAssertFalse(named.isOtherField(than: unnamed))
        XCTAssertFalse(named.isOtherField(than: named))
        XCTAssertTrue(named.isOtherField(than: other))
        var email = named
        email.keyboardType = "emailAddress"
        XCTAssertTrue(unnamed.isOtherField(than: email))
    }

    private final class DocumentProxyStandIn: NSObject {
        @objc var documentIdentifier: NSUUID?
    }

    // MARK: - The real keyboard

    private static func info(_ traits: TypingLogTraits = TypingLogTraits()) -> TypingLogSessionInfo {
        TypingLogSessionInfo(
            trigger: "appear", surface: "full", traits: traits, bounds: CGSize(width: 393, height: 262),
            safeArea: .zero, orientation: "portrait", appearance: "light", device: TypingLogSessionInfo.deviceModel,
            system: UIDevice.current.systemVersion, version: "0.2.0", build: "1", locale: "en_US",
            languages: ["en-US"], keyboardLanguage: "en-US", inputMode: "en-US", fullAccess: true, settings: []
        )
    }

    private final class Checker: TextChecking {
        func isWordValid(_ word: String) -> Bool { true }
        func guesses(for word: String) -> [String] { [] }
        func completions(for prefix: String) -> [String] { [] }
    }

    private struct Rig {
        let view: FullKeyboardView
        let target: KeyboardTextTarget
        let logger: TypingLogger?
        let store: TypingLogStore?
    }

    /// A field that, like the real proxy, shows only the text near the cursor.
    private final class LongField: KeyboardTextTarget {
        var text = ""
        var documentContextBeforeInput: String? { String(text.suffix(200)) }
        var hasText: Bool { !text.isEmpty }
        var autocapitalizationType: UITextAutocapitalizationType? { .sentences }
        var keyboardType: UIKeyboardType? { .default }
        var returnKeyType: UIReturnKeyType? { .default }
        var keyboardAppearance: UIKeyboardAppearance? { .light }
        func insertText(_ inserted: String) { text += inserted }
        func deleteBackward() { if !text.isEmpty { text.removeLast() } }
    }

    private var clock: TimeInterval = 50_000
    private var nextID = 0

    private func makeRig(logging: Bool, directory: URL? = nil, long: Bool = false) -> Rig {
        let target: KeyboardTextTarget
        if long {
            target = LongField()
        } else {
            let fixture = FixtureTextTarget()
            fixture.autocapitalizationType = .sentences
            target = fixture
        }
        let defaults = UserDefaults(suiteName: "TypingLogTests-\(UUID().uuidString)")!
        let autocorrect = AutocorrectEngine(checker: Checker(), learnedWords: LearnedWordsStore(defaults: defaults))
        let view = FullKeyboardView(model: KeyboardViewModel(), target: target, autocorrect: autocorrect)
        // English, whatever dictation language the simulator's App Group holds.
        view.languageOverride = .english
        view.engine.autocorrect.isEnabled = true
        view.engine.clock = { [unowned self] in self.clock }
        view.orientationOverride = .portrait
        view.frame = CGRect(x: 0, y: 0, width: 393, height: view.preferredHeight)
        view.layoutIfNeeded()
        guard logging else { return Rig(view: view, target: target, logger: nil, store: nil) }
        let store = TypingLogStore(directory: directory ?? self.directory)
        let logger = TypingLogger(store: store)
        logger.monoClock = { [unowned self] in self.clock }
        logger.contextSource = { TypingLogContext(before: target.documentContextBeforeInput, after: "", hasText: target.hasText) }
        logger.beginSession(Self.info())
        view.typingLog = logger
        view.typingLogSessionBegan()
        return Rig(view: view, target: target, logger: logger, store: store)
    }

    /// One tap as a finger makes it: down a little off-centre, a 1 pt wobble (throttled
    /// out), a 3 pt drift (written), up.
    private func tap(_ kind: KeyKind, on rig: Rig, accent: String? = nil) {
        guard let key = rig.view.engine.layout.key(kind) else { return XCTFail("no key \(kind)") }
        nextID += 1
        let id = nextID
        var point = CGPoint(x: key.frame.midX + 1.5, y: key.frame.midY + 4)
        func sample() -> TypingLogTouchSample {
            TypingLogTouchSample(point: point, timestamp: clock, majorRadius: 20.85, majorRadiusTolerance: 5.21)
        }
        clock += 0.11
        rig.view.plane.began(id: id, sample: sample())
        clock += 0.004
        point.x += 1
        rig.view.plane.moved(id: id, sample: sample())
        if let accent {
            // What the plane's hold timer does after 0.4 s.
            clock += 0.4
            XCTAssertTrue(rig.view.engine.openAccentMenuIfEligible(forTouch: id))
            let menu = try! XCTUnwrap(rig.view.engine.activeAccentMenu)
            let index = try! XCTUnwrap(menu.options.firstIndex(of: accent))
            point.x = menu.cellFrames[index].midX
        } else {
            clock += 0.012
            point.y -= 3
        }
        rig.view.plane.moved(id: id, sample: sample())
        clock += 0.03
        rig.view.plane.ended(id: id, sample: sample())
    }

    private func type(_ keys: [KeyKind], on rig: Rig) {
        for key in keys { tap(key, on: rig) }
    }

    private func letters(_ text: String) -> [KeyKind] {
        text.map { $0 == " " ? .space : .character(String($0)) }
    }

    /// Replays every edit line on an empty field, the way a Mac-side parser would.
    private func replay(_ lines: [[String: Any]]) -> String {
        var text = ""
        for line in lines where line["type"] as? String == "edit" || line["type"] as? String == "dictation" {
            if let deleted = line["deleted"] as? String {
                text.removeLast(min(deleted.count, text.count))
            } else if let count = line["deletedCount"] as? Int {
                text.removeLast(min(count, text.count))
            }
            if let inserted = line["inserted"] as? String {
                text += inserted
            }
        }
        return text
    }

    func testTypingOnTheRealKeyboardLogsTouchesKeysAndTextThatReplayToTheField() throws {
        let rig = makeRig(logging: true)
        // "dont" autocorrects on space; x is typed and deleted; two spaces make a period;
        // 123 → 7 and space return to letters; return; then a long-press é.
        type(letters("dont go x"), on: rig)
        tap(.delete, on: rig)
        type(letters("yet  "), on: rig)
        tap(.layerSwitch(.numbers), on: rig)
        type([.character("7"), .space, .character("o"), .character("k"), .returnKey, .character("c"), .character("a"), .character("f")], on: rig)
        tap(.character("e"), on: rig, accent: "é")
        let field = try XCTUnwrap(rig.target as? FixtureTextTarget)
        XCTAssertEqual(field.text, "Don't go yet. 7 ok\nCafé")

        rig.logger?.endSession(reason: "disappear")
        rig.store?.waitUntilWritten()
        let url = try XCTUnwrap(TypingLogStore.days(in: directory).first).url
        let parsed = try lines(url)

        // Every line has the common fields, in one session, in order.
        for (index, line) in parsed.enumerated() {
            XCTAssertEqual(line["v"] as? Int, 1)
            XCTAssertNotNil(line["t"] as? Int)
            XCTAssertNotNil(line["mt"] as? Double)
            XCTAssertEqual(line["seq"] as? Int, index + 1)
        }
        XCTAssertEqual(Set(parsed.compactMap { $0["session"] as? String }).count, 1)
        XCTAssertEqual(parsed.first?["type"] as? String, "session_start")
        XCTAssertEqual(parsed.last?["type"] as? String, "session_end")

        // Edits replay to exactly what is in the field, and so does the last context.
        XCTAssertEqual(replay(parsed), field.text)
        let lastContext = try XCTUnwrap(parsed.last { $0["type"] as? String == "context" })
        XCTAssertEqual(lastContext["before"] as? String, field.text)

        // Each key a finger lifted from typed that key.
        let touches = parsed.filter { $0["type"] as? String == "touch" }
        XCTAssertEqual(touches.filter { $0["phase"] as? String == "began" }.count, 25)
        XCTAssertEqual(touches.filter { $0["phase"] as? String == "moved" }.count, 25, "the 1 pt wobble is throttled out")
        XCTAssertEqual(touches.filter { $0["phase"] as? String == "ended" }.count, 25)
        let layouts = Set(parsed.filter { $0["type"] as? String == "layout" }.compactMap { $0["hash"] as? String })
        for touch in touches {
            XCTAssertNotNil(touch["x"])
            XCTAssertEqual(touch["r"] as? Double, 20.85)
            XCTAssertTrue(layouts.contains(touch["layout"] as? String ?? ""), "every touch names a layout written before it")
        }
        var typedKeys: [String] = []
        for (index, line) in parsed.enumerated() where line["op"] as? String == "insert" && line["src"] as? String == "key" {
            let inserted = try XCTUnwrap(line["inserted"] as? String)
            let lift = try XCTUnwrap(parsed[..<index].last { $0["type"] as? String == "touch" && $0["phase"] as? String == "ended" })
            let key = try XCTUnwrap(lift["key"] as? String)
            XCTAssertEqual(key == "space" ? " " : key, inserted.lowercased(), "line \(index)")
            typedKeys.append(key)
        }
        XCTAssertEqual(typedKeys.prefix(4), ["d", "o", "n", "t"])

        let ops = parsed.filter { $0["type"] as? String == "edit" }.compactMap { $0["op"] as? String }
        for op in ["insert", "autocorrect", "delete", "double_space_period", "return"] {
            XCTAssertTrue(ops.contains(op), op)
        }
        let autocorrect = try XCTUnwrap(parsed.first { $0["op"] as? String == "autocorrect" })
        XCTAssertEqual(autocorrect["from"] as? String, "Dont")
        XCTAssertEqual(autocorrect["to"] as? String, "Don't")
        XCTAssertTrue(parsed.contains { $0["action"] as? String == "accent_pick" && $0["chosen"] as? String == "é" })
        XCTAssertTrue(parsed.contains { $0["type"] as? String == "layout" && $0["layer"] as? String == "numbers" })
        XCTAssertTrue(parsed.contains { $0["type"] as? String == "layout" && $0["shift"] as? String == "on" && $0["cause"] as? String == "auto" })

        let fullLayout = try XCTUnwrap(parsed.first { $0["type"] as? String == "layout" })
        let keys = try XCTUnwrap(fullLayout["keys"] as? [[String: Any]])
        let logged = try XCTUnwrap(keys.first { $0["id"] as? String == "q" }?["hitFrame"] as? [Double])
        let q = try XCTUnwrap(rig.view.engine.layout.key(.character("q"))).hitFrame
        for (value, expected) in zip(logged, [q.minX, q.minY, q.width, q.height]) {
            XCTAssertEqual(value, Double(expected), accuracy: 0.006)
        }
        XCTAssertEqual((fullLayout["slots"] as? [Any])?.count, 3)

        let text = try String(contentsOf: url, encoding: .utf8)
        print("TYPING-LOG-SAMPLE \(url.path)")
        print(text, terminator: "")
        print("TYPING-LOG-SAMPLE-END")
    }

    /// Main-thread cost per tap with logging on and off, and bytes per tap (simulator,
    /// Debug build). Printed for the report; asserts only that logging stays cheap.
    func testLoggingOverheadAndSize() throws {
        let words = Array("the quick brown fox jumps over a lazy dog and then types ").map { $0 == " " ? KeyKind.space : .character(String($0)) }
        func run(_ rig: Rig, taps: Int) -> TimeInterval {
            let start = CACurrentMediaTime()
            for index in 0..<taps {
                tap(words[index % words.count], on: rig)
                // The 2 s flush timer, which needs a spinning run loop.
                if index % 300 == 299 {
                    rig.logger?.flush()
                }
            }
            return CACurrentMediaTime() - start
        }
        let taps = 3000
        let off = makeRig(logging: false, long: true)
        let on = makeRig(logging: true, directory: directory.appendingPathComponent("overhead"), long: true)
        _ = run(off, taps: 200)
        _ = run(on, taps: 200)
        let offTime = run(off, taps: taps)
        let onTime = run(on, taps: taps)
        let flushStart = CACurrentMediaTime()
        on.logger?.endSession(reason: "test")
        let flushTime = CACurrentMediaTime() - flushStart
        let writeStart = CACurrentMediaTime()
        on.store?.waitUntilWritten()
        let writeTime = CACurrentMediaTime() - writeStart

        let url = try XCTUnwrap(TypingLogStore.days(in: directory.appendingPathComponent("overhead")).first).url
        let bytes = try XCTUnwrap(try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int)
        let perTapBytes = Double(bytes) / Double(taps + 200)
        let perTapOverhead = (onTime - offTime) / Double(taps) * 1_000_000
        // Encoding cost on the writer queue, measured apart.
        let sample = try String(contentsOf: url, encoding: .utf8).split(separator: "\n").count
        print(String(
            format: "TYPING-LOG-OVERHEAD taps=%d off=%.1fus/tap on=%.1fus/tap overhead=%.1fus/tap endSession=%.2fms writerDrain=%.1fms bytes=%d lines=%d bytesPerTap=%.0f perHourAt200cpm=%.1fMB",
            taps, offTime / Double(taps) * 1e6, onTime / Double(taps) * 1e6, perTapOverhead,
            flushTime * 1000, writeTime * 1000, bytes, sample, perTapBytes, perTapBytes * 12_000 / 1_048_576
        ))
        XCTAssertLessThan(perTapOverhead, 500, "logging adds well under a millisecond per tap")
    }

    // MARK: - Typing configs (the phone's side of a config made from these logs)

    private let typingConfig = Data(#"{"format": "hex.typing-config", "version": 0, "note": "Kept your words.", "words": {"keep": [{"word": "eepy", "evidence": {"times": 3, "sessions": 3}}, {"word": "nightmail", "evidence": {"times": 2, "sessions": 2}}]}}"#.utf8)

    /// A file opened from Files or a share sheet is read once, never past the 64 KB limit, and a
    /// copy iOS put in Documents/Inbox is deleted once read; its name shows without control or
    /// direction-changing characters.
    func testATypingConfigArrivesReadOnceAndNeverPastTheLimit() throws {
        let inbox = directory.appendingPathComponent("Documents/Inbox", isDirectory: true)
        try FileManager.default.createDirectory(at: inbox, withIntermediateDirectories: true)
        let small = inbox.appendingPathComponent("my typing.mhexconfig")
        try typingConfig.write(to: small)
        let arrival = TypingConfigInbox.read(small)
        XCTAssertEqual(arrival.content, .file(typingConfig))
        XCTAssertEqual(arrival.name, "my typing")
        XCTAssertFalse(FileManager.default.fileExists(atPath: small.path), "Inbox copy deleted")

        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let big = directory.appendingPathComponent("logs.mhexconfig")
        try Data(count: TypingConfigLimits.maxBytes + 1).write(to: big)
        XCTAssertEqual(TypingConfigInbox.read(big).content, .tooLarge(bytes: TypingConfigLimits.maxBytes + 1))
        XCTAssertTrue(FileManager.default.fileExists(atPath: big.path), "a file in place is left alone")
        let empty = directory.appendingPathComponent("empty.mhexconfig")
        try Data().write(to: empty)
        XCTAssertEqual(TypingConfigInbox.read(empty).content, .unreadable)

        XCTAssertTrue(TypingConfigInbox.handles(URL(fileURLWithPath: "/a/b.MHEXCONFIG")))
        XCTAssertFalse(TypingConfigInbox.handles(URL(string: "hex-dictation://config")!))
        XCTAssertEqual(TypingConfigInbox.displayName("gift\u{202E}gpj.exe\u{0007}"), "giftgpj.exe")
    }

    /// The history keeps every config unchanged under the SHA-256 of its bytes (the same file
    /// twice is one entry) and never refuses one: past 20 the oldest that is neither active nor
    /// best so far goes, and the review can name it first; deleting the active one leaves Hex
    /// defaults.
    func testTypingConfigHistoryKeysOnTheBytesAndKeepsAppliedOnes() throws {
        let history = TypingConfigHistory(directory: directory)
        let hash = TypingConfigReview(data: typingConfig, keyboard: KeyboardLayout(width: 402)).contentHash
        guard case .added(let first) = history.add(typingConfig, hash: hash, name: "mine") else { return XCTFail("added") }
        XCTAssertEqual(first.id, hash)
        XCTAssertEqual(history.add(typingConfig, hash: hash, name: "again"), .existing(first))
        XCTAssertEqual(history.data(hash), typingConfig, "kept unchanged")
        history.setActive(hash, partsOff: ["words.keep[1]"])
        XCTAssertEqual(TypingConfigHistory(directory: directory).active?.partsOff, ["words.keep[1]"], "survives a relaunch")

        for n in 1..<TypingConfigHistory.limit {
            _ = history.add(Data("\(n)".utf8), hash: "h\(n)", name: "n\(n)", now: Date(timeIntervalSince1970: Double(n)))
        }
        XCTAssertEqual(history.entries.count, TypingConfigHistory.limit)
        history.setActive(hash)
        XCTAssertEqual(history.removedByAdding("new")?.id, "h1", "named before it goes")
        XCTAssertNil(history.removedByAdding("h2"), "one already kept removes nothing")
        _ = history.add(Data("new".utf8), hash: "new", name: "new")
        XCTAssertNil(history.entry("h1"), "the oldest went")
        XCTAssertNotNil(history.entry(hash), "the active one stays, though it is the oldest")
        XCTAssertEqual(history.entries.count, TypingConfigHistory.limit)

        history.setActive(hash)
        history.delete(hash)
        XCTAssertNil(history.activeID)
        XCTAssertEqual(history.shareableCopy("new")?.lastPathComponent, "new.mhexconfig")
        history.deleteAll()
        XCTAssertTrue(history.entries.isEmpty)
    }

    /// Apply writes active.json with only the parts left on and bumps the generation; going back
    /// to it compiles the kept file again with the same parts off; Hex defaults removes it.
    func testApplyingAConfigWritesOnlyItsSwitchedOnPartsAndGoingBackDeletesNothing() throws {
        let suite = "TypingLogTests-\(UUID().uuidString)"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defer { defaults.removePersistentDomain(forName: suite) }
        let store = TypingConfigStore(settings: TypingConfigSettings(defaults: defaults), directory: directory)
        let history = TypingConfigHistory(directory: directory)
        let keyboard = KeyboardLayout(width: 402)
        let review = TypingConfigReview(data: typingConfig, keyboard: keyboard)
        guard case .added(let entry) = history.add(typingConfig, hash: review.contentHash, name: "mine") else { return XCTFail("added") }

        try TypingConfigActivation.apply(entry, review: review, partsOff: ["words.keep[1]"], history: history, store: store)
        let active = try XCTUnwrap(store.activeURL.flatMap { try? Data(contentsOf: $0) })
        let written = try JSONDecoder().decode(TypingConfigFile.self, from: active)
        XCTAssertEqual(written.words?.keep?.compactMap(\.value?.word), ["eepy"])
        XCTAssertNil(written.note, "no note in what the keyboard reads")
        XCTAssertEqual(TypingConfigSettings(defaults: defaults).generation, 1)
        XCTAssertEqual(history.activeID, entry.id)

        TypingConfigActivation.useDefaults(history: history, store: store)
        XCTAssertFalse(FileManager.default.fileExists(atPath: try XCTUnwrap(store.activeURL).path))
        XCTAssertNil(history.activeID)
        XCTAssertNotNil(history.entry(entry.id), "Hex defaults deletes nothing")

        let kept = try XCTUnwrap(history.entry(entry.id))
        let again = TypingConfigReview(data: try XCTUnwrap(history.data(kept.id)), keyboard: keyboard)
        try TypingConfigActivation.apply(kept, review: again, partsOff: Set(kept.partsOff), history: history, store: store)
        let back = try JSONDecoder().decode(TypingConfigFile.self, from: try Data(contentsOf: try XCTUnwrap(store.activeURL)))
        XCTAssertEqual(back.words?.keep?.compactMap(\.value?.word), ["eepy"], "the parts it had off stay off")

        // Hex's own dictation takes a config's dictation fixes, never for a translated dictation.
        let off = TypingConfigSettings(defaults: defaults)
        XCTAssertEqual(TypingConfigDictation.corrections(translating: false, settings: off), [], "experiment off")
        off.isEnabled = true
        XCTAssertEqual(TypingConfigDictation.corrections(translating: true, settings: off), [], "translated")
    }

}
