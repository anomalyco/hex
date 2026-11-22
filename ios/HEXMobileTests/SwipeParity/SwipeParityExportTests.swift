import CoreGraphics
import CryptoKit
import XCTest
@testable import HEX

/// Writes the swipe parity fixture the Rust port is checked against
/// (`crates/keyboard/core/tests/swipe_parity.rs`): `SwipeParity`'s cases with what the Swift
/// oracle (`SwipeDecoderOracle`) reads for each. Skipped unless asked for:
///
///     xcodebuild test … -only-testing:HEXMobileTests/SwipeParityExportTests \
///       TEST_RUNNER_HEX_SWIPE_PARITY_OUT=/tmp/swipe-parity.jsonl TEST_RUNNER_HEX_SWIPE_PARITY_COUNT=1000
///
/// (`TEST_RUNNER_HEX_SWIPE_PARITY_TIMING=1` adds each read's time in nanoseconds.)
///
/// JSON Lines: a header (generator version, the oracle's source commit, the models' SHA-256,
/// the parameters), each context and layout, then one case per line. Numbers that must come
/// through exactly are strings: a key's centre, a split layout's edges or a base time as
/// Swift's shortest round-trip decimal, a cost as its IEEE 754 bits in 16 hex digits. The
/// touches (as whole quarter points, x then y) and their times (as whole ticks,
/// `SwipeParity.Case.times`) are packed: each number after the first as its step from the
/// one before, zigzag-encoded (0, -1, 1, -2… as 0, 1, 2, 3…), in LEB128 bytes, in base64. The path
/// the decoder read is the touches joined as `KeyboardLayout.joinedForSwipe` joins them (on
/// the split layout, by its keys line's `join`: left edge, right edge, gap), and `inputs`
/// hashes it and the times bit for bit.
@MainActor
final class SwipeParityExportTests: XCTestCase {
    /// The dev commit `SwipeDecoderOracle.swift` was copied from.
    static let oracleCommit = "b719d45e5361ae04e220ec8b6822a40ea0dc1cd4"

    func testExportSwipeParityFixture() throws {
        let env = ProcessInfo.processInfo.environment
        guard let path = env["HEX_SWIPE_PARITY_OUT"], !path.isEmpty else {
            throw XCTSkip("set TEST_RUNNER_HEX_SWIPE_PARITY_OUT to write the fixture")
        }
        let count = Int(env["HEX_SWIPE_PARITY_COUNT"] ?? "") ?? 1000
        let timing = env["HEX_SWIPE_PARITY_TIMING"] != nil
        XCTAssertTrue(FileManager.default.createFile(atPath: path, contents: nil), path)
        let file = try XCTUnwrap(FileHandle(forWritingAtPath: path))
        defer { try? file.close() }
        var out = JSONLines(file: file)

        var setups: [SwipeParity.Setup] = []
        for language in SwipeParity.languages {
            let contexts = setups.reduce(0) { $0 + $1.contexts.count }, layouts = setups.reduce(0) { $0 + $1.layouts.count }
            setups.append(try SwipeParity.Setup(language, firstContext: contexts, firstLayout: layouts))
        }
        var models: [String] = []
        for language in SwipeParity.languages {
            let url = try XCTUnwrap(Self.modelURL(language.rawValue))
            let digest = SHA256.hash(data: try Data(contentsOf: url)).map { String(format: "%02x", $0) }.joined()
            models.append("\(JSONLines.string(language.rawValue)):\(JSONLines.string(digest))")
        }
        let parameters = Mirror(reflecting: SwipeDecoderOracle.Parameters()).children.map {
            "\(JSONLines.string($0.label ?? "")):\(JSONLines.string("\($0.value)"))"
        }
        out.line(
            #"{"kind":"header","generator":\#(SwipeParity.generatorVersion),"oracle":\#(JSONLines.string(Self.oracleCommit)),"#
                + #""count":\#(count),"models":{\#(models.joined(separator: ","))},"parameters":{\#(parameters.joined(separator: ","))}}"#
        )
        for setup in setups {
            for context in setup.contexts {
                let extra = Self.extraWords(context).map { "[\(JSONLines.string($0.text)),\(JSONLines.bits($0.cost)),\($0.contraction)]" }
                out.line(
                    #"{"kind":"context","id":\#(context.id),"language":\#(JSONLines.string(context.language.rawValue)),"#
                        + #""swearing":\#(context.allowSwearing),"blocked":[\#(context.blocked.map(JSONLines.string).joined(separator: ","))],"#
                        + #""extra":[\#(extra.joined(separator: ","))]}"#
                )
            }
            for layout in setup.layouts {
                let join = layout.layout.split.map {
                    "[\(JSONLines.number($0.leftEdge)),\(JSONLines.number($0.rightEdge)),\(JSONLines.number(SplitGeometry.gap))]"
                } ?? "null"
                let centres = layout.keys.centres.sorted { $0.key < $1.key }.map {
                    "[\(JSONLines.string(String($0.key))),\(JSONLines.number($0.value.x)),\(JSONLines.number($0.value.y))]"
                }
                out.line(
                    #"{"kind":"keys","id":\#(layout.id),"name":\#(JSONLines.string(layout.name)),"#
                        + #""pitch":[\#(JSONLines.number(layout.keys.pitch.width)),\#(JSONLines.number(layout.keys.pitch.height))],"#
                        + #""join":\#(join),"centres":[\#(centres.joined(separator: ","))]}"#
                )
            }
        }

        var nextID = 0
        var durations: [UInt64] = []
        for setup in setups {
            let decoders = Dictionary(uniqueKeysWithValues: setup.contexts.map { ($0.id, Self.oracle($0, model: setup.model)) })
            let keys = Dictionary(uniqueKeysWithValues: setup.layouts.map { ($0.id, $0.keys) })
            var generator = SwipeParityGenerator(setup, firstID: nextID)
            for _ in 0..<count {
                let c = generator.next()
                let decoder = decoders[c.context]!
                let started = DispatchTime.now().uptimeNanoseconds
                let reading = decoder.read(c.points, times: c.times, keys: keys[c.layout]!, previous: c.previous, limit: c.limit)
                let elapsed = DispatchTime.now().uptimeNanoseconds - started
                durations.append(elapsed)
                let ticks = c.ticks.map { JSONLines.string(JSONLines.packed(zip($0, [0] + $0).map { $0 - $1 })) } ?? "null"
                let candidates = reading.candidates.map { "[\(JSONLines.string($0.text)),\(JSONLines.bits($0.cost))]" }
                out.line(
                    #"{"kind":"case","id":\#(c.id),"context":\#(c.context),"keys":\#(c.layout),"type":\#(JSONLines.string(c.kind.rawValue)),"#
                        + #""word":\#(JSONLines.string(c.word)),"path":\#(JSONLines.string(Self.path(c.screen))),"inputs":\#(Self.inputs(c)),"t0":\#(JSONLines.number(c.timeBase)),"#
                        + #""ticks":\#(ticks),"previous":\#(c.previous.map(JSONLines.string) ?? "null"),"limit":\#(c.limit),"#
                        + #""scored":\#(reading.scored),"out":[\#(candidates.joined(separator: ","))]\#(timing ? #","ns":\#(elapsed)"# : "")}"#
                )
            }
            nextID += count
        }
        out.flush()
        durations.sort()
        let median = durations[durations.count / 2], p95 = durations[durations.count * 95 / 100]
        print("swipe parity: wrote \(durations.count) cases to \(path); oracle read median \(median / 1000) µs, p95 \(p95 / 1000) µs")
    }

    /// The touches as whole quarter points, packed (`JSONLines.packed`): x and y of the first,
    /// then of each one's step from the one before.
    static func path(_ screen: [CGPoint]) -> String {
        var last = (0, 0), steps: [Int] = []
        for point in screen {
            let q = (Int(point.x * 4), Int(point.y * 4))
            steps += [q.0 - last.0, q.1 - last.1]
            last = q
        }
        return JSONLines.packed(steps)
    }

    /// FNV-1a (64-bit) of the decoder's inputs, as their IEEE 754 bits (little-endian): each
    /// point's x and y, then each time when there are times. A reader that rebuilds them from
    /// the fixture proves it rebuilt them exactly.
    static func inputs(_ c: SwipeParity.Case) -> String {
        var hash: UInt64 = 0xCBF2_9CE4_8422_2325
        func add(_ value: Double) {
            var bits = value.bitPattern
            for _ in 0..<8 {
                hash = (hash ^ (bits & 0xFF)) &* 0x0000_0100_0000_01B3
                bits >>= 8
            }
        }
        for point in c.points {
            add(Double(point.x))
            add(Double(point.y))
        }
        for time in c.times ?? [] { add(time) }
        return JSONLines.string(String(format: "%016llx", hash))
    }

    /// The decoder `SwipeTyping.syncWords` makes for `context`.
    static func oracle(_ context: SwipeParity.Context, model: TypingDecoderModel) -> SwipeDecoderOracle {
        var decoder = SwipeDecoderOracle(model: model)
        decoder.neverSuggest = TypingDecoder.NeverSuggest(context.blocked, model: model)
        var extra = SwipeDecoderOracle.ExtraWords()
        if model.language == KeyboardLanguage.english.rawValue {
            extra.add(context.contractions, cost: { SwipeDecoderOracle.costOfContraction($0) }, model: model, contractions: true)
        }
        extra.add(context.ownWords, cost: SwipeDecoderOracle.ownWordCost, model: model)
        decoder.extraWords = extra
        return decoder
    }

    /// The decoder `SwipeTyping.syncWords` makes for `context`.
    static func decoder(_ context: SwipeParity.Context, model: TypingDecoderModel) -> SwipeDecoder {
        var decoder = SwipeDecoder(model: model)
        decoder.neverSuggest = TypingDecoder.NeverSuggest(context.blocked, model: model)
        var extra = SwipeDecoder.ExtraWords()
        if model.language == KeyboardLanguage.english.rawValue {
            extra.add(context.contractions, cost: { SwipeDecoder.costOfContraction($0) }, model: model, contractions: true)
        }
        extra.add(context.ownWords, cost: SwipeDecoder.ownWordCost, model: model)
        decoder.extraWords = extra
        return decoder
    }

    /// The same words as one list, as a decoder that takes them at once gets them.
    static func extraWords(_ context: SwipeParity.Context) -> [(text: String, cost: Double, contraction: Bool)] {
        let contractions = context.language == .english ? context.contractions : []
        return contractions.map { ($0, SwipeDecoderOracle.costOfContraction($0), true) }
            + context.ownWords.map { ($0, SwipeDecoderOracle.ownWordCost, false) }
    }

    /// Where `TypingDecoderModel.bundled` maps the model from.
    static func modelURL(_ language: String) -> URL? {
        if let url = Bundle.main.url(forResource: language, withExtension: "hextm") { return url }
        guard let plugins = Bundle.main.builtInPlugInsURL,
              let keyboard = Bundle(url: plugins.appendingPathComponent("HEXKeyboard.appex")) else { return nil }
        return keyboard.url(forResource: language, withExtension: "hextm")
    }
}

/// Buffered JSON Lines and the few JSON values the fixture needs.
struct JSONLines {
    let file: FileHandle
    private var buffer = Data()

    init(file: FileHandle) { self.file = file }

    mutating func line(_ text: String) {
        buffer.append(contentsOf: text.utf8)
        buffer.append(0x0A)
        if buffer.count > 1 << 20 { flush() }
    }

    mutating func flush() {
        file.write(buffer)
        buffer.removeAll(keepingCapacity: true)
    }

    static func string(_ text: String) -> String {
        var out = "\""
        for scalar in text.unicodeScalars {
            switch scalar {
            case "\"": out += "\\\""
            case "\\": out += "\\\\"
            case _ where scalar.value < 0x20: out += String(format: "\\u%04x", scalar.value)
            default: out.unicodeScalars.append(scalar)
            }
        }
        return out + "\""
    }

    /// Swift's shortest decimal that reads back as the same `Double`.
    static func decimal(_ value: Double) -> String {
        let text = "\(value)"
        return text.hasSuffix(".0") ? String(text.dropLast(2)) : text
    }

    static func decimal(_ value: CGFloat) -> String { decimal(Double(value)) }

    static func number(_ value: Double) -> String { string(decimal(value)) }

    static func number(_ value: CGFloat) -> String { number(Double(value)) }

    /// Whole numbers zigzag-encoded, as LEB128 bytes, in base64.
    static func packed(_ values: [Int]) -> String {
        var bytes: [UInt8] = []
        for value in values {
            var z = UInt64(bitPattern: Int64((value << 1) ^ (value >> 63)))
            while z >= 0x80 {
                bytes.append(UInt8(z & 0x7F) | 0x80)
                z >>= 7
            }
            bytes.append(UInt8(z))
        }
        return Data(bytes).base64EncodedString()
    }

    /// The IEEE 754 bit pattern, 16 hex digits.
    static func bits(_ value: Double) -> String {
        string(String(format: "%016llx", value.bitPattern))
    }
}
