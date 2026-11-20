import CoreGraphics
import XCTest
@testable import HEX

/// Swipe typing's decoder (`SwipeDecoder`) on the bundled English and Spanish models, with
/// swipes drawn through Hex's own key geometry. It has its own file because the decoder reads
/// a path without the keys' engine and its fixtures are paths; how the keys type a swipe is
/// in FullKeyboardEngineTests.
@MainActor
final class SwipeDecoderTests: XCTestCase {
    private func decoder(_ language: KeyboardLanguage, swearing: Bool = false) throws -> (SwipeDecoder, TypingDecoder.Keys) {
        let model = try XCTUnwrap(TypingDecoderModel.bundled(language.rawValue), "the \(language) model is bundled")
        var decoder = SwipeDecoder(model: model)
        decoder.neverSuggest = TypingDecoder.NeverSuggest(
            OffensiveWords.bundled(language.rawValue).blocked(allowSwearing: swearing), model: model
        )
        if language == .english {
            decoder.extraWords = SwipeDecoder.ExtraWords(
                AutocorrectEngine.englishContractions, cost: { SwipeDecoder.costOfContraction($0) }, model: model
            )
        }
        let keys = try XCTUnwrap(TypingDecoder.Keys(layout: KeyboardLayout(width: 390, language: language)))
        return (decoder, keys)
    }

    private func read(_ word: String, _ decoder: SwipeDecoder, _ keys: TypingDecoder.Keys, after previous: String? = nil, offset: CGVector = .zero) -> [String] {
        decoder.decode(SwipePath.through(word, keys: keys, offset: offset), keys: keys, previous: previous).map(\.text)
    }

    func testCleanSwipesReadAsTheirWords() throws {
        let (en, keys) = try decoder(.english)
        for word in ["hello", "the", "keyboard", "world", "typing", "people", "because", "quick", "with", "you", "and"] {
            XCTAssertEqual(read(word, en, keys).first, word)
        }
    }

    /// A finger that runs low all the way (as taps land below the keys' centres), or low and
    /// to the left.
    func testASwipeOffTheCentresStillReads() throws {
        let (en, keys) = try decoder(.english)
        for off in [CGVector(dx: 0, dy: keys.pitch.height * 0.3), CGVector(dx: -keys.pitch.width / 4, dy: keys.pitch.height / 4)] {
            for word in ["hello", "keyboard", "people", "world", "typing", "because", "quick"] {
                XCTAssertEqual(read(word, en, keys, offset: off).first, word, "\(off)")
            }
        }
    }

    func testSpanishSwipesKeepTheirAccentsAndTheirÑ() throws {
        let (es, keys) = try decoder(.spanish)
        for word in ["año", "también", "gracias", "mañana", "niño", "teclado"] {
            XCTAssertEqual(read(word, es, keys).first, word)
        }
        XCTAssertEqual(Set(read("esta", es, keys).prefix(2)), ["esta", "está"], "one path, both readings")
    }

    /// Google Books splits "don't" into "do n't", so the model barely counts it: the decoder
    /// reads Hex's English contractions as common words.
    func testContractionsReadAsCommonWords() throws {
        let (en, keys) = try decoder(.english)
        XCTAssertEqual(read("dont", en, keys).first, "don't")
        XCTAssertEqual(read("im", en, keys).first, "I'm")
    }

    func testTheWordBeforePicksBetweenWordsOnOnePath() throws {
        let (en, keys) = try decoder(.english)
        XCTAssertEqual(Array(read("to", en, keys).prefix(2)), ["to", "too"])
        XCTAssertEqual(read("too", en, keys, after: "far").first, "too")
    }

    func testSlursAreNeverReadAndSwearWordsOnlyWhenAsked() throws {
        for language in [KeyboardLanguage.english, .spanish] {
            let words = OffensiveWords.bundled(language.rawValue)
            for swearing in [false, true] {
                let (decoder, keys) = try decoder(language, swearing: swearing)
                let probes = language == .english ? ["fuck", "shit", "bitch", "nigger", "faggot"] : ["mierda", "cabron", "maricón"]
                for probe in probes {
                    for reading in read(probe, decoder, keys) {
                        XCTAssertFalse(words.blocks(reading, allowSwearing: swearing), "\(probe) read as \(reading), swear words \(swearing)")
                    }
                }
            }
        }
        let (swearing, keys) = try decoder(.english, swearing: true)
        XCTAssertEqual(read("fuck", swearing, keys).first, "fuck", "with Suggest swear words on")
    }

    func testAPathTooShortReadsAsNothing() throws {
        let (en, keys) = try decoder(.english)
        let h = try XCTUnwrap(keys.centres["h"])
        XCTAssertTrue(en.decode([h, CGPoint(x: h.x + 4, y: h.y)], keys: keys).isEmpty)
        XCTAssertTrue(en.decode([h], keys: keys).isEmpty)
    }
}

/// Swipes for tests: straight strokes through the centres of a word's keys.
enum SwipePath {
    /// Through the centres of `word`'s keys on `keys` (an accented letter on its base
    /// letter's key, an apostrophe on none), a point every `step` points, moved by `offset`.
    static func through(_ word: String, keys: TypingDecoder.Keys, offset: CGVector = .zero, step: CGFloat = 6) -> [CGPoint] {
        var centres: [CGPoint] = []
        for letter in word.lowercased() where letter != "'" {
            let base = String(letter).decomposedStringWithCanonicalMapping.unicodeScalars.first.map(Character.init)
            guard let centre = keys.centres[letter] ?? base.flatMap({ keys.centres[$0] }) else { continue }
            if centres.last != centre { centres.append(centre) }
        }
        guard let first = centres.first else { return [] }
        var points = [first]
        for (a, b) in zip(centres, centres.dropFirst()) {
            let count = max(1, Int((hypot(b.x - a.x, b.y - a.y) / step).rounded(.up)))
            for i in 1...count {
                let t = CGFloat(i) / CGFloat(count)
                points.append(CGPoint(x: a.x + (b.x - a.x) * t, y: a.y + (b.y - a.y) * t))
            }
        }
        return points.map { CGPoint(x: $0.x + offset.dx, y: $0.y + offset.dy) }
    }
}
