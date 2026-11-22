import UIKit
import XCTest
@testable import HEX

/// The typing quality gate's replay on the iOS engine: the Android benchmark's held-out inputs
/// (`android/benchmark/typing/held-out`, #2374) typed through `FullKeyboardEngine` with the
/// language's bundled model, as `android/benchmark/typing/quality-gate.py` types them through
/// Android's. Opt-in: it skips unless `HEX_TYPING_GATE` names a folder `scripts/typing-gate.py
/// prepare` wrote (xcodebuild passes it as `TEST_RUNNER_HEX_TYPING_GATE`), with
/// `HEX_TYPING_GATE_LANGUAGE` the one language to type; `scripts/typing-gate.py run` does both and
/// scores the outputs it writes there (`output-<lang>.json`). German reads its model as with its
/// experiment on; a language with no bundled model types as the keyboard does without one.
@MainActor
final class TypingGateReplayTests: XCTestCase {
    private struct Input: Decodable {
        struct Item: Decodable {
            let id: String
            let language: String
            let taps: [[Double]]
            let want: String
        }
        let width: Double
        let geometry: [String: [String: [Double]]]
        let items: [Item]
    }

    func testReplaysTheHeldOutGate() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let folder = environment["HEX_TYPING_GATE"], let code = environment["HEX_TYPING_GATE_LANGUAGE"] else {
            throw XCTSkip("set HEX_TYPING_GATE and HEX_TYPING_GATE_LANGUAGE (scripts/typing-gate.py run)")
        }
        let input = try JSONDecoder().decode(Input.self, from: Data(contentsOf: URL(fileURLWithPath: folder + "/input.json")))
        let language = try XCTUnwrap(KeyboardLanguage(rawValue: code), code)
        let geo = try XCTUnwrap(input.geometry[code], "no geometry for \(code)")
        // A language without a bundled model (Bokmål) types as the keyboard does today: no decoder.
        let model = TypingDecoderModel.bundled(code)
        let checker = SystemTextChecker()
        checker.warmUp()
        var outputs: [String: String] = [:]
        for item in input.items where item.language == code {
            outputs[item.id] = try type(item, language: language, model: model, checker: checker, geo: geo, width: input.width)
        }
        let json = try JSONSerialization.data(withJSONObject: outputs, options: [.sortedKeys])
        try json.write(to: URL(fileURLWithPath: folder + "/output-\(code).json"))
    }

    private func type(
        _ item: Input.Item, language: KeyboardLanguage, model: TypingDecoderModel?, checker: SystemTextChecker,
        geo: [String: [Double]], width: Double
    ) throws -> String {
        let target = FixtureTextTarget(text: "")
        target.autocapitalizationType = .sentences
        let suite = "TypingGateReplayTests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: suite)!
        defer { defaults.removePersistentDomain(forName: suite) }
        let autocorrect = AutocorrectEngine(checker: checker, learnedWords: LearnedWordsStore(defaults: defaults))
        autocorrect.decoder = model.map { TypingDecoder(model: $0) }
        let engine = FullKeyboardEngine(target: target, autocorrect: autocorrect)
        engine.width = CGFloat(width)
        engine.letterOdds = model
        engine.language = language
        var now: TimeInterval = 1000
        engine.clock = { now }
        engine.configureForField()

        func press(_ point: CGPoint, id: Int) {
            engine.touchBegan(id: id, at: point)
            engine.touchEnded(id: id, at: point)
            now += 0.2
        }
        func centre(_ kind: KeyKind) throws -> CGPoint {
            let frame = try XCTUnwrap(engine.layout.key(kind), "no \(kind) key in \(item.id)").frame
            return CGPoint(x: frame.midX, y: frame.midY)
        }
        func center(_ f: [Double]) -> CGPoint { CGPoint(x: f[0] + f[2] / 2, y: f[1] + f[3] / 2) }
        // Pitches from two keys of the top row and the first of the second (AZERTY's q, w and a
        // sit on other rows than QWERTY's).
        let rows = language.letterRows
        let (k0, k1, k2) = (rows[0][0], rows[0][1], rows[1][0])
        let pitchX = (try centre(.character(k1)).x - centre(.character(k0)).x) / (center(geo[k1]!).x - center(geo[k0]!).x)
        let pitchY = (try centre(.character(k2)).y - centre(.character(k0)).y) / (center(geo[k2]!).y - center(geo[k0]!).y)
        // French's apostrophe key types ' right after a letter (#2354, `FrenchTyping`).
        let apostrophe = engine.layout.key(.character("'")) != nil ? "'"
            : engine.layout.key(.character(FrenchTyping.key)) != nil ? FrenchTyping.key : nil
        func takes(_ key: KeyboardKey) -> Bool {
            key.kind == .space || key.character.map { $0 == "'" || $0 == FrenchTyping.key || ($0.count == 1 && $0.first!.isLetter) } == true
        }
        let planned = Array(item.want)
        XCTAssertEqual(planned.count, item.taps.count, item.id)
        for (i, tap) in item.taps.enumerated() {
            let ch = planned[i]
            let lower = String(ch).lowercased()
            if ch.isLetter, engine.layout.layer == .letters, engine.isUppercase != ch.isUppercase {
                press(try centre(.shift), id: 30_000 + i)
            }
            if tap == [-1, -1] {
                // A letter without a key of its own, picked from its key's long-press menu.
                let base = String(lower.decomposedStringWithCanonicalMapping.unicodeScalars.first.map(Character.init) ?? Character(lower))
                let point = try centre(.character(lower == "ß" ? "s" : base))
                engine.touchBegan(id: i, at: point)
                engine.openAccentMenuIfEligible(forTouch: i)
                let menu = try XCTUnwrap(engine.activeAccentMenu, "no accent menu for \(ch) in \(item.id)")
                let index = try XCTUnwrap(menu.options.firstIndex { $0.lowercased() == lower }, "\(ch) not in \(menu.options)")
                let cell = menu.cellFrames[index]
                engine.touchEnded(id: i, at: CGPoint(x: cell.midX, y: cell.midY))
                now += 0.2
                continue
            }
            if ch == "'", apostrophe == nil {
                // No apostrophe key among the letters: the 123 page's, then back to the letters.
                press(try centre(.layerSwitch(.numbers)), id: 10_000 + i)
                press(try centre(.character("'")), id: i)
                if engine.layout.layer != .letters, i + 1 < planned.count, planned[i + 1] != " " {
                    press(try centre(.layerSwitch(.letters)), id: 20_000 + i)
                }
                continue
            }
            let space = ch == " "
            let name = space ? "space" : lower
            let kind: KeyKind = space ? .space : .character(ch == "'" ? apostrophe! : lower)
            let dst = try XCTUnwrap(engine.layout.key(kind), "no \(name) key in \(item.id)").frame
            var p = CGPoint(x: dst.midX, y: dst.midY)
            if let f = geo[name] {
                let src = center(f)
                p = CGPoint(x: dst.midX + (tap[0] - src.x) * (space ? dst.width / f[2] : pitchX),
                            y: dst.midY + (tap[1] - src.y) * pitchY)
            }
            if engine.layout.hitTest(p).map(takes) != true {
                // The synthetic rows are taller than Hex's: a touch outside Hex's keys, or on 123,
                // shift, delete or the globe, goes to the nearest letter or space bar's edge.
                let nearest = engine.layout.keys.filter(takes).min { a, b in
                    func d(_ f: CGRect) -> CGFloat { hypot(max(f.minX - p.x, 0, p.x - f.maxX), max(f.minY - p.y, 0, p.y - f.maxY)) }
                    return d(a.hitFrame) < d(b.hitFrame)
                }!.hitFrame
                p = CGPoint(x: min(max(p.x, nearest.minX + 0.5), nearest.maxX - 0.5),
                            y: min(max(p.y, nearest.minY + 0.5), nearest.maxY - 0.5))
            }
            press(p, id: i)
        }
        return target.text
    }
}
