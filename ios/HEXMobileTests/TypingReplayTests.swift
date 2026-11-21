import QuartzCore
import UIKit
import XCTest
@testable import HEX

/// A touch's timestamp base, far enough from zero to look like a touch already under way.
private let replayClockBase: TimeInterval = 100

/// The gap between gestures a recording doesn't place itself, in seconds: longer than every
/// window a lift leaves open behind it — the swipe trail's 0.45 s (`SwipeTrailView.tail`),
/// the accent and trackpad holds (0.36 s and 0.35 s, `FullKeyboardEngine.accentHoldDelay`
/// and `trackpadHoldDelay`), the double-space period (0.35 s) and the strip's touch guard
/// (0.25 s) — so a gap the replay invents never places a gesture inside a rule the one before
/// it started. Recordings should carry their own (`start`): real ones are mostly longer
/// (FUTO's median is 0.87 s).
private let replayDefaultGestureGap: TimeInterval = 0.5

/// Replaying recorded fingers through the full keyboard as it runs: the key plane's touches,
/// Swipe typing or taps with Spatial autocorrect, and the engine's spacing. Two opt-in tests
/// read recordings (`HEX_SWIPE_REPLAY`, `HEX_TAP_REPLAY`); one test with no environment
/// checks Hex's key geometry against the grid and runs clean gestures through the same
/// mapping to type the right words, so a wrong layout fails loudly, but it cannot catch a
/// converter that emits wrong coordinates.
///
/// `xcodebuild` passes environment variables to the test runner only when they are prefixed
/// with `TEST_RUNNER_`, and strips the prefix, so the variables below are set that way:
///
///     TEST_RUNNER_HEX_SWIPE_REPLAY=/path/swipes.json xcodebuild test \
///       -project HEXMobile.xcodeproj -scheme HEXMobile -destination "id=<SIM_UDID>" \
///       -only-testing:HEXMobileTests/TypingReplayTests
///
/// Recordings are not vendored. A swipe file is
/// `{"sentences":[{"source":String,"session":String,"words":[String],
/// "gestures":[{"x":[Double],"y":[Double],"t":[Double]}]}]}` with one gesture per word: x runs
/// 0…1 across the ten columns of the top letter row, from q's left edge to p's right edge
/// (their centres at 0.05 and 0.95), y runs 0…1 from the top of the first letter row to the
/// bottom of the third (row centres at 1/6, 1/2 and 5/6), and t is milliseconds since that
/// finger landed. A gesture's optional `start` is when that landing was, in milliseconds from
/// the sentence's first landing, so gestures keep the person's own gaps; without one the
/// replay begins the gesture `replayDefaultGestureGap` after the previous lift. The second
/// and third rows sit half a column and one and a half columns right of the first, as on
/// QWERTY
/// (`TypingReplayGrid` checks Hex's keys against that before replaying). A tap file replaces
/// `gestures` with `taps`, one `{x, y, t}` per touch, each array holding that touch's
/// samples (a tap is usually one) on the sentence's clock; y > 1 is the space-bar row, where
/// x spans 0.25…0.75 across the recorded space bar and keeps its fraction along Hex's own.
///
/// Both report a line per source between `TYPING-REPLAY` and `TYPING-REPLAY-END`: total
/// words, the words the decoder's dictionary lacks and their share (the check keeps only ASCII
/// a–z letters, so accented, hyphenated and numeric tokens count as outside), and accuracy
/// among the words it has. Taps report it over every in-dictionary word. Swipes report it over
/// the swipe attempts (`attempts`: the gestures that became swipes, the ones whose path left
/// its key by a swipe's pitch and weren't promoted, and the ones that read nothing) and also
/// as an `all-words-accuracy` over every in-dictionary word, taps included and a skipped
/// gesture counted wrong — what the person reading the field would see. They also report the
/// tap recordings (`taps`, and how many of those left their word in the field, `taps-right`:
/// a path that never leaves its key by a swipe's pitch is a tap, and the keyboard types its
/// letter), the swipe attempts this keyboard missed (`not-swipes`), the gestures that began
/// off the letter keys (`skipped`), and the decoder's own median and p90 milliseconds.
/// `HEX_REPLAY_LIMIT` caps sentences per source and `HEX_REPLAY_OUT` writes each sentence's
/// intended and typed words as JSON.
@MainActor
final class TypingReplayTests: XCTestCase {
    /// English's own letter rows, for the in-memory gestures: row 2 starts half a column in
    /// and row 3 one and a half, as Apple's and Hex's keyboards draw them.
    private static let qwertyRows = ["qwertyuiop", "asdfghjkl", "zxcvbnm"]
    private static let rowOffsets: [CGFloat] = [0, 0.5, 1.5]
    private var suiteName: String!
    private var defaults: UserDefaults!
    private var model: TypingDecoderModel!
    /// The engine's clock, driven by a recording's times.
    private var now: TimeInterval = replayClockBase
    /// The real time when `now` was last set, for `SwipeTyping.clock`.
    private var realBase: TimeInterval = CACurrentMediaTime()

    /// Moves the replay's clock to a recorded time. `SwipeTyping` asks its own clock for both
    /// preview throttling and its decode timing, so that clock runs from `now` plus the real
    /// time since this call: preview throttling sees the recording's gaps, and `lastDecodeTime`
    /// still measures the decode instead of zero.
    private func setNow(_ time: TimeInterval) {
        now = time
        realBase = CACurrentMediaTime()
    }

    override func setUp() async throws {
        suiteName = "TypingReplayTests-\(UUID().uuidString)"
        defaults = UserDefaults(suiteName: suiteName)!
        model = try XCTUnwrap(
            TypingDecoderModel.bundled(KeyboardLanguage.english.rawValue),
            "the English typing model is bundled"
        )
    }

    override func tearDown() async throws {
        defaults.removePersistentDomain(forName: suiteName)
    }

    // MARK: - Recordings

    /// Replays a `HEX_SWIPE_REPLAY` file: each word's recorded path through the key plane,
    /// on top of the sentence typed so far. Skipped unless the variable names a file.
    func testReplaysRecordedSwipes() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let path = environment["HEX_SWIPE_REPLAY"] else {
            throw XCTSkip("Set HEX_SWIPE_REPLAY to a file of recorded swipes")
        }
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        let file = try JSONDecoder().decode(SwipeReplayFile.self, from: data)
        let reports = try replaySwipes(file.sentences, limit: replayLimit(environment))
        XCTAssertGreaterThan(reports.reduce(0) { $0 + $1.sentences }, 0, "the file replayed no sentences")
        printReplayReport(reports)
        try writeReplayOutput(reports, to: environment["HEX_REPLAY_OUT"])
    }

    /// Replays a `HEX_TAP_REPLAY` file: every recorded tap in order, letters and space bar,
    /// with Spatial autocorrect reading the word on each space as it does on the keyboard.
    /// Skipped unless the variable names a file.
    func testReplaysRecordedTaps() throws {
        let environment = ProcessInfo.processInfo.environment
        guard let path = environment["HEX_TAP_REPLAY"] else {
            throw XCTSkip("Set HEX_TAP_REPLAY to a file of recorded taps")
        }
        let data = try Data(contentsOf: URL(fileURLWithPath: path))
        let file = try JSONDecoder().decode(TapReplayFile.self, from: data)
        let reports = try replayTaps(file.sentences, limit: replayLimit(environment))
        XCTAssertGreaterThan(reports.reduce(0) { $0 + $1.sentences }, 0, "the file replayed no sentences")
        printReplayReport(reports)
        try writeReplayOutput(reports, to: environment["HEX_REPLAY_OUT"])
    }

    /// The same replay without a recording: swipes drawn as straight segments through the
    /// letters' grid coordinates and taps on the key centres, both from QWERTY's rows. It
    /// fails if the mapping, the plane, or the decoder stops turning clean input into the
    /// words meant, which is what makes the opt-in tests above worth reading.
    func testTheReplayMappingTypesCleanSwipesAndTaps() throws {
        let words = ["hello", "world", "the", "keyboard"]
        for word in words {
            XCTAssertNotNil(model.index(of: word), "\(word) is in the English model")
        }

        let (_, engine, _) = try makeKeyboard()
        let grid = try TypingReplayGrid(keys: engine.swipeKeys)
        // The format is edge-based: q's centre is half a column in and a sixth down.
        let edges: [(Character, CGFloat, CGFloat)] = [
            ("q", 0.05, 1.0 / 6), ("p", 0.95, 1.0 / 6), ("a", 0.10, 0.5), ("z", 0.20, 5.0 / 6),
        ]
        for (letter, x, y) in edges {
            let centre = try XCTUnwrap(engine.swipeKeys.centres[letter])
            let mapped = grid.point(x: x, y: y)
            XCTAssertEqual(mapped.x, centre.x, accuracy: 0.5, "\(letter)'s centre x")
            XCTAssertEqual(mapped.y, centre.y, accuracy: 0.5, "\(letter)'s centre y")
        }

        let swipes = words.map { swipeGesture(for: $0, grid: grid) }
        let swipeReport = try XCTUnwrap(
            replaySwipes(
                [SwipeReplaySentence(source: "in-memory", session: "fixture", words: words, gestures: swipes)],
                limit: nil
            ).first
        )
        XCTAssertEqual(swipeReport.sentences, 1)
        XCTAssertEqual(swipeReport.words.count, words.count)
        XCTAssertTrue(swipeReport.words.allSatisfy(\.inDictionary), "every fixture word is in the dictionary")
        XCTAssertEqual(swipeReport.notSwipes, 0, "every fixture gesture is read as a swipe")
        let swiped = swipeReport.words.map { "\($0.intended)→\($0.typed)" }
        XCTAssertTrue(swipeReport.words.allSatisfy(\.right), "swiped \(swiped)")

        let taps = tapPoints(for: words)
        let tapReport = try XCTUnwrap(
            replayTaps(
                [TapReplaySentence(source: "in-memory", session: "fixture", words: words, taps: taps)],
                limit: nil
            ).first
        )
        XCTAssertEqual(tapReport.sentences, 1)
        XCTAssertEqual(tapReport.words.count, words.count)
        XCTAssertTrue(tapReport.words.allSatisfy(\.inDictionary), "every fixture word is in the dictionary")
        let tapped = tapReport.words.map { "\($0.intended)→\($0.typed)" }
        XCTAssertTrue(tapReport.words.allSatisfy(\.right), "tapped \(tapped)")
    }

    /// A recorded tap — a one-letter word's still finger — is a tap, not a missed swipe: the
    /// keyboard types its letter, as it does for a person, so it is neither a swipe that read
    /// nothing nor one this keyboard missed.
    func testATapIsNotAMissedSwipe() throws {
        let (_, engine, _) = try makeKeyboard()
        let grid = try TypingReplayGrid(keys: engine.swipeKeys)
        let tap = ReplayGesture(x: [0.10, 0.10], y: [0.5, 0.5], t: [0, 40])
        let report = try XCTUnwrap(
            replaySwipes(
                [SwipeReplaySentence(
                    source: "in-memory",
                    session: "fixture",
                    words: ["hello", "a"],
                    gestures: [swipeGesture(for: "hello", grid: grid), tap]
                )],
                limit: nil
            ).first
        )
        XCTAssertEqual(report.words.first?.swiped, true)
        XCTAssertEqual(report.words.last?.tap, true, "a path that stays on its key is a tap")
        XCTAssertEqual(report.words.last?.typed, "a", "the tap types its letter")
        XCTAssertEqual(report.notSwipes, 0, "no swipe attempt was missed")
        XCTAssertEqual(report.skipped, 0)
        XCTAssertEqual(report.output.first?.typed.first.map(Self.replayKey), "hello")
    }

    /// Tap word alignment: a split word and a missing word each leave the later intended
    /// words matched instead of shifting every word after them.
    func testAlignmentKeepsLaterWordsAfterASplitOrAMissingWord() {
        let missing: [Int?] = [0, nil, 1, 2]
        XCTAssertEqual(align(["one", "of", "the", "best"], ["one", "the", "best"]), missing)
        let split: [Int?] = [0, nil, 3, 4]
        XCTAssertEqual(align(["a", "lot", "of", "fun"], ["a", "lo", "t", "of", "fun"]), split)
    }

    // MARK: - Swipes

    private struct SwipeResult {
        var word: ReplayWord
        var typed: String
        /// A swipe attempt the plane never took as a swipe, or one that read nothing.
        var notSwiped: Bool
        /// The gesture never reached the plane: its first point wasn't on a letter key.
        var skipped: Bool
        var decodeMilliseconds: Double?
        /// Where a gesture with no recorded start begins on the sentence's clock.
        var nextStart: TimeInterval
    }

    /// Replays `sentences` through a fresh keyboard each, grouped by source. One gesture per
    /// word, read from the field as the difference the swipe made.
    private func replaySwipes(_ sentences: [SwipeReplaySentence], limit: Int?) throws -> [ReplayReport] {
        var reports: [ReplayReport] = []
        for group in grouped(sentences, by: \.source, limit: limit) {
            var report = ReplayReport(source: group.source, mode: "swipe")
            for sentence in group.sentences {
                guard sentence.words.count == sentence.gestures.count else {
                    throw ReplayError.malformed(
                        "\(group.source) \(sentence.session): \(sentence.words.count) words but \(sentence.gestures.count) gestures"
                    )
                }
                let (target, engine, plane) = try makeKeyboard()
                guard plane.swipe != nil else { throw ReplayError.missingSwipeModel }
                let grid = try TypingReplayGrid(keys: engine.swipeKeys)
                report.sentences += 1
                // A recording's own `start` says where the person's finger landed; without one
                // the gesture follows the previous lift by `replayDefaultGestureGap`. Either way
                // the clock only runs forward, as a real one does.
                var start = replayClockBase
                var typed: [String] = []
                for (index, word) in sentence.words.enumerated() {
                    let gesture = sentence.gestures[index]
                    let origin = gesture.start.map { replayClockBase + $0 / 1000 } ?? start
                    let result = try replaySwipe(
                        gesture,
                        intended: word,
                        target: target,
                        engine: engine,
                        plane: plane,
                        grid: grid,
                        id: index + 1,
                        start: origin
                    )
                    start = result.nextStart
                    typed.append(result.typed)
                    report.words.append(result.word)
                    if result.notSwiped { report.notSwipes += 1 }
                    if result.skipped { report.skipped += 1 }
                    if let milliseconds = result.decodeMilliseconds {
                        report.decodeTimes.append(milliseconds)
                    }
                }
                report.output.append(
                    ReplayOutput.Sentence(
                        source: group.source,
                        session: sentence.session,
                        mode: "swipe",
                        intended: sentence.words,
                        typed: typed
                    )
                )
            }
            reports.append(report)
        }
        return reports
    }

    private func replaySwipe(
        _ gesture: ReplayGesture,
        intended: String,
        target: FixtureTextTarget,
        engine: FullKeyboardEngine,
        plane: KeyboardKeyPlaneView,
        grid: TypingReplayGrid,
        id: Int,
        start: TimeInterval
    ) throws -> SwipeResult {
        let samples = try gesture.samples(grid: grid, start: start)
        let before = target.text
        var typed = ""
        var couldReadAddedWord = false
        var attempted = false
        var wasSwipe = false
        var decode: Double?

        // A swipe starts only on a letter key (`SwipeTyping.began`), and a path shorter than
        // two points can't be one. Skip the plane entirely: a first point that isn't on a
        // letter is a key row's gap, the page or the space bar the recording's grid can't
        // place, and it must not change the context the next word reads.
        if samples.count >= 2, startsOnALetter(samples[0].point, engine: engine) {
            attempted = true
            setNow(samples[0].time)
            plane.began(id: id, sample: TypingLogTouchSample(point: samples[0].point, timestamp: samples[0].time))
            for sample in samples.dropFirst().dropLast() {
                setNow(sample.time)
                plane.moved(id: id, sample: TypingLogTouchSample(point: sample.point, timestamp: sample.time))
            }
            let lift = samples[samples.count - 1]
            setNow(lift.time)
            wasSwipe = plane.swipe?.owns(id) ?? false
            plane.ended(id: id, sample: TypingLogTouchSample(point: lift.point, timestamp: lift.time))
            if wasSwipe, let last = plane.swipe?.lastDecodeTime {
                decode = last * 1000
            }
            if target.text.hasPrefix(before) {
                typed = String(target.text.dropFirst(before.count)).trimmingCharacters(in: .whitespacesAndNewlines)
                couldReadAddedWord = true
            } else {
                // The engine edited text elsewhere, so a prefix diff can't name what this
                // gesture added. Report the last token and mark the word wrong.
                typed = target.text.split(whereSeparator: \.isWhitespace).last.map(String.init) ?? ""
            }
        }

        // The recording says whether a swipe was there to read. `SwipeTyping.moved` promotes a
        // letter touch once it travels more than the layout's `swipePitch` (`threshold`,
        // SwipeTyping.swift:70 and :123), so a path that never does is a tap: FUTO records a
        // one-letter word as a still finger, and a keyboard types that key's letter for it. The
        // field keeps that letter, because a person who tapped "a" has an "a" in their text —
        // deleting it rewrites the sentence the next word is read in (`swipePreviousWord`,
        // FullKeyboardEngine+SwipeTyping.swift:35, and the casing that reads the text before
        // the cursor). A path that does travel a pitch and still wasn't promoted is a swipe the
        // keyboard missed; one that was promoted and read nothing is a swipe it failed.
        let travelled = samples.dropLast().contains {
            hypot($0.point.x - samples[0].point.x, $0.point.y - samples[0].point.y) > engine.layout.swipePitch
        }
        let tapped = attempted && !wasSwipe && !travelled
        let missed = attempted && !wasSwipe && travelled
        let notSwiped = missed || (wasSwipe && typed.isEmpty)

        let key = Self.replayKey(intended)
        let inDictionary = isInDictionary(intended)
        let right = couldReadAddedWord && inDictionary && !key.isEmpty && Self.replayKey(typed) == key
        let offered = wasSwipe && inDictionary && !key.isEmpty && engine.suggestions.contains {
            let replacement = Self.replayKey($0.replacement)
            return !replacement.isEmpty && replacement == key
        }
        return SwipeResult(
            word: ReplayWord(
                intended: intended,
                typed: typed,
                inDictionary: inDictionary,
                right: right,
                offered: offered,
                swiped: wasSwipe,
                attempt: wasSwipe || missed,
                tap: tapped
            ),
            typed: typed,
            notSwiped: notSwiped,
            skipped: !attempted,
            decodeMilliseconds: decode,
            nextStart: (samples.last?.time ?? start) + replayDefaultGestureGap
        )
    }

    // MARK: - Taps

    /// Replays `sentences` through a fresh keyboard each, grouped by source. Every tap goes
    /// down and up at its recorded point on the engine's clock; the last word gets the space
    /// the keyboard would have seen, so its correction applies.
    private func replayTaps(_ sentences: [TapReplaySentence], limit: Int?) throws -> [ReplayReport] {
        var reports: [ReplayReport] = []
        for group in grouped(sentences, by: \.source, limit: limit) {
            var report = ReplayReport(source: group.source, mode: "tap")
            for sentence in group.sentences {
                let (target, engine, plane) = try makeKeyboard()
                let grid = try TypingReplayGrid(keys: engine.swipeKeys)
                guard let space = engine.layout.key(.space)?.frame else { throw ReplayError.noSpaceBar }
                report.sentences += 1
                for (index, tap) in sentence.taps.enumerated() {
                    let samples = try tap.samples(grid: grid, space: space)
                    guard let first = samples.first, let lift = samples.last else { continue }
                    setNow(first.time)
                    plane.began(id: index + 1, sample: TypingLogTouchSample(point: first.point, timestamp: first.time))
                    for sample in samples.dropFirst().dropLast() {
                        setNow(sample.time)
                        plane.moved(id: index + 1, sample: TypingLogTouchSample(point: sample.point, timestamp: sample.time))
                    }
                    setNow(lift.time)
                    plane.ended(id: index + 1, sample: TypingLogTouchSample(point: lift.point, timestamp: lift.time))
                }
                if !target.text.hasSuffix(" ") {
                    setNow(replayClockBase + (sentence.taps.last?.t.last ?? 0) / 1000 + 0.4)
                    let point = CGPoint(x: space.midX, y: space.midY)
                    let sample = TypingLogTouchSample(point: point, timestamp: now)
                    plane.began(id: sentence.taps.count + 1, sample: sample)
                    plane.ended(id: sentence.taps.count + 1, sample: sample)
                }

                let typedWords = target.text.split(whereSeparator: \.isWhitespace).map(String.init)
                let matches = align(sentence.words, typedWords)
                for (index, intended) in sentence.words.enumerated() {
                    let key = Self.replayKey(intended)
                    let typed = matches[index].map { typedWords[$0] } ?? ""
                    let inDictionary = isInDictionary(intended)
                    let right = inDictionary && !key.isEmpty && Self.replayKey(typed) == key
                    report.words.append(
                        ReplayWord(intended: intended, typed: typed, inDictionary: inDictionary, right: right, offered: false)
                    )
                }
                report.output.append(
                    ReplayOutput.Sentence(
                        source: group.source,
                        session: sentence.session,
                        mode: "tap",
                        intended: sentence.words,
                        typed: typedWords
                    )
                )
            }
            reports.append(report)
        }
        return reports
    }

    // MARK: - The keyboard the recordings run on

    /// A 390 pt English keyboard set up as `FullKeyboardView` sets one up: Autocorrect and
    /// Auto-capitalization from the switches, the typing model for the letter odds and, while
    /// Spatial autocorrect is on, its decoder (`FullKeyboardView.syncSpatialAutocorrect`), and
    /// Swipe typing's model while the switch is on (`FullKeyboardView.syncSwipeTyping`). The
    /// field keeps `FixtureTextTarget`'s own default, `.sentences`
    /// (FullKeyboardSnapshotStage.swift:583): the people who recorded these sentences typed
    /// them into ordinary fields, which capitalise sentence starts, and the report compares
    /// letters only.
    private func makeKeyboard() throws -> (FixtureTextTarget, FullKeyboardEngine, KeyboardKeyPlaneView) {
        let target = FixtureTextTarget()
        let engine = FullKeyboardEngine(
            target: target,
            autocorrect: AutocorrectEngine(learnedWords: LearnedWordsStore(defaults: defaults))
        )
        engine.width = 390
        engine.language = .english
        engine.clock = { [unowned self] in self.now }
        let settings = FullKeyboardSettings(defaults: defaults)
        engine.autocorrect.isEnabled = settings.autocorrect
        engine.autoCapitalizes = settings.autoCapitalization
        let words = try XCTUnwrap(settings.typingModel(for: .english), "the English typing model is bundled")
        engine.letterOdds = words
        engine.autocorrect.decoder = settings.spatialAutocorrect ? TypingDecoder(model: words) : nil
        engine.configureForField()
        let plane = KeyboardKeyPlaneView(engine: engine)
        if settings.swipeTyping, engine.language.supportsSwipeTyping {
            plane.swipe = SwipeTyping(engine: engine, model: words)
            // SwipeTyping's clock runs from the recorded time plus the real time since the
            // last sample: preview throttling sees the recorded gaps, and its own
            // `lastDecodeTime` still measures the decode instead of zero.
            plane.swipe?.clock = { [unowned self] in self.now + (CACurrentMediaTime() - self.realBase) }
        }
        return (target, engine, plane)
    }

    private func startsOnALetter(_ point: CGPoint, engine: FullKeyboardEngine) -> Bool {
        guard case .character(let character)? = engine.keyKind(at: point) else { return false }
        return character.first?.isLetter == true
    }

    private func isInDictionary(_ word: String) -> Bool {
        let letters = Self.replayKey(word)
        guard !letters.isEmpty else { return false }
        return model.index(of: letters) != nil
    }

    private static func replayKey(_ word: String) -> String {
        String(word.lowercased().filter { $0.isASCII && $0.isLetter })
    }

    // MARK: - In-memory gestures

    /// A swipe through `word`'s letters, sampled every 6 pt at about 750 pt/s as a finger
    /// moves: straight segments between their grid coordinates.
    private func swipeGesture(for word: String, grid: TypingReplayGrid) -> ReplayGesture {
        var coordinates: [CGPoint] = []
        for letter in word.lowercased() {
            guard let position = Self.qwertyPosition(letter) else { continue }
            let coordinate = CGPoint(
                x: (CGFloat(position.column) + Self.rowOffsets[position.row] + 0.5) / 10,
                y: (CGFloat(position.row) + 0.5) / 3
            )
            if coordinates.last != coordinate { coordinates.append(coordinate) }
        }
        guard let first = coordinates.first else { return ReplayGesture(x: [], y: [], t: []) }
        var xs = [Double(first.x)]
        var ys = [Double(first.y)]
        var times: [Double] = [0]
        for (start, end) in zip(coordinates, coordinates.dropFirst()) {
            let a = grid.point(x: start.x, y: start.y)
            let b = grid.point(x: end.x, y: end.y)
            let distance = hypot(b.x - a.x, b.y - a.y)
            let steps = max(1, Int((distance / 6).rounded(.up)))
            let stepTime = Double(distance / 750 * 1000) / Double(steps)
            for step in 1...steps {
                let fraction = CGFloat(step) / CGFloat(steps)
                xs.append(Double(start.x + (end.x - start.x) * fraction))
                ys.append(Double(start.y + (end.y - start.y) * fraction))
                times.append(times[times.count - 1] + stepTime)
            }
        }
        return ReplayGesture(x: xs, y: ys, t: times)
    }

    /// Taps on each letter's centre, with a tap on the space bar between words (and none
    /// after the last, which the replay's own final space covers).
    private func tapPoints(for words: [String]) -> [ReplayTap] {
        var taps: [ReplayTap] = []
        var time = 0.0
        for (index, word) in words.enumerated() {
            if index > 0 {
                taps.append(ReplayTap(x: [0.5], y: [1.25], t: [time]))
                time += 180
            }
            for letter in word.lowercased() {
                guard let position = Self.qwertyPosition(letter) else { continue }
                taps.append(
                    ReplayTap(
                        x: [Double((CGFloat(position.column) + Self.rowOffsets[position.row] + 0.5) / 10)],
                        y: [Double((CGFloat(position.row) + 0.5) / 3)],
                        t: [time]
                    )
                )
                time += 180
            }
        }
        return taps
    }

    private static func qwertyPosition(_ letter: Character) -> (row: Int, column: Int)? {
        for (row, letters) in qwertyRows.enumerated() {
            if let index = letters.firstIndex(of: letter) {
                return (row, letters.distance(from: letters.startIndex, to: index))
            }
        }
        return nil
    }

    // MARK: - Reports

    private func grouped<T>(
        _ sentences: [T],
        by source: KeyPath<T, String>,
        limit: Int?
    ) -> [(source: String, sentences: [T])] {
        var order: [String] = []
        var groups: [String: [T]] = [:]
        for sentence in sentences {
            let key = sentence[keyPath: source]
            if groups[key] == nil { order.append(key) }
            groups[key, default: []].append(sentence)
        }
        return order.map { key in
            let all = groups[key] ?? []
            return (source: key, sentences: limit.map { Array(all.prefix($0)) } ?? all)
        }
    }

    private func replayLimit(_ environment: [String: String]) -> Int? {
        guard let limit = environment["HEX_REPLAY_LIMIT"].flatMap({ Int($0) }), limit > 0 else { return nil }
        return limit
    }

    private func printReplayReport(_ reports: [ReplayReport]) {
        let body = reports.map(\.line).joined(separator: "\n")
        print("TYPING-REPLAY\n" + body + "\nTYPING-REPLAY-END")
    }

    /// Writes each replayed sentence's intended and typed words. For swipes, `typed` holds
    /// what each gesture produced (empty when it wasn't read as one).
    private func writeReplayOutput(_ reports: [ReplayReport], to path: String?) throws {
        guard let path else { return }
        let output = ReplayOutput(sentences: reports.flatMap { $0.output })
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.prettyPrinted, .sortedKeys]
        try encoder.encode(output).write(to: URL(fileURLWithPath: path))
    }

    /// The pairs of intended and typed words that are equal after `replayKey`, matched in
    /// order with a longest-common-subsequence walk so a split or a missing word doesn't
    /// push every word after it out of place.
    private func align(_ intended: [String], _ typed: [String]) -> [Int?] {
        let a = intended.map(Self.replayKey)
        let b = typed.map(Self.replayKey)
        var lengths = [[Int]](repeating: [Int](repeating: 0, count: b.count + 1), count: a.count + 1)
        if !a.isEmpty, !b.isEmpty {
            for i in stride(from: a.count - 1, through: 0, by: -1) {
                for j in stride(from: b.count - 1, through: 0, by: -1) {
                    if !a[i].isEmpty, a[i] == b[j] {
                        lengths[i][j] = lengths[i + 1][j + 1] + 1
                    } else {
                        lengths[i][j] = max(lengths[i + 1][j], lengths[i][j + 1])
                    }
                }
            }
        }
        var matches = [Int?](repeating: nil, count: a.count)
        var i = 0, j = 0
        while i < a.count, j < b.count {
            if !a[i].isEmpty, a[i] == b[j] {
                matches[i] = j
                i += 1
                j += 1
            } else if lengths[i + 1][j] >= lengths[i][j + 1] {
                i += 1
            } else {
                j += 1
            }
        }
        return matches
    }
}

// MARK: - The unit grid

/// The recordings' unit grid on Hex's letter keys. x runs from q's left edge to p's right
/// edge across the ten columns, and y from the first letter row's top to the third's bottom;
/// a key's centre sits half a column and half a row inside those edges, so q, a and z centre
/// at 0.05, 0.10 and 0.20 across and 1/6, 1/2 and 5/6 down. QWERTY's second row starts half a
/// column in and its third one and a half, which is checked here so a layout that isn't the
/// one the recordings assume fails loudly instead of measuring nonsense.
private struct TypingReplayGrid {
    let q: CGPoint
    let p: CGPoint
    let z: CGPoint

    init(keys: TypingDecoder.Keys) throws {
        guard let q = keys.centres["q"], let p = keys.centres["p"],
              let a = keys.centres["a"], let z = keys.centres["z"] else {
            throw ReplayError.grid(
                "the replay grid needs q, p, a and z's key centres; it has \(keys.centres.keys.sorted())"
            )
        }
        let column = (p.x - q.x) / 9
        let aX = q.x + 0.5 * column
        let zX = q.x + 1.5 * column
        let middle = (q.y + z.y) / 2
        let tolerance: CGFloat = 0.5
        guard p.x > q.x, z.y > q.y,
              abs(a.x - aX) <= tolerance, abs(z.x - zX) <= tolerance,
              abs(a.y - middle) <= tolerance else {
            throw ReplayError.grid(
                "the replay grid assumes QWERTY's second letter row starts half a column right of the first "
                + "and its third one and a half: q=(\(q.x), \(q.y)) p=(\(p.x), \(p.y)) a=(\(a.x), \(a.y)) "
                + "z=(\(z.x), \(z.y)); expected a=(\(aX), \(middle)) and z=(\(zX), \(z.y))"
            )
        }
        self.q = q
        self.p = p
        self.z = z
    }

    /// The point `x` across the letter area, from q's left edge to p's right edge, and `y`
    /// down it from the first letter row's top to the third's bottom.
    func point(x: CGFloat, y: CGFloat) -> CGPoint {
        let column = (p.x - q.x) / 9
        let row = (z.y - q.y) / 2
        return CGPoint(x: q.x + (x * 10 - 0.5) * column, y: q.y + (y * 3 - 0.5) * row)
    }
}

// MARK: - Recordings

private struct SwipeReplayFile: Decodable {
    var sentences: [SwipeReplaySentence]
}

private struct SwipeReplaySentence: Decodable {
    var source: String
    var session: String
    var words: [String]
    var gestures: [ReplayGesture]
}

private struct ReplayGesture: Decodable {
    var x: [Double]
    var y: [Double]
    var t: [Double]
    /// When the finger landed, in milliseconds from the sentence's first landing. Absent in
    /// files written before recordings kept their gaps.
    var start: Double? = nil
}

private struct TapReplayFile: Decodable {
    var sentences: [TapReplaySentence]
}

private struct TapReplaySentence: Decodable {
    var source: String
    var session: String
    var words: [String]
    var taps: [ReplayTap]
}

private struct ReplayTap: Decodable {
    var x: [Double]
    var y: [Double]
    var t: [Double]
}

private struct ReplaySample {
    var point: CGPoint
    var time: TimeInterval
}

private extension ReplayGesture {
    /// The gesture's points on Hex's keys, with its milliseconds moved onto the sentence's
    /// clock at `start`.
    func samples(grid: TypingReplayGrid, start: TimeInterval) throws -> [ReplaySample] {
        guard x.count == y.count, y.count == t.count else {
            throw ReplayError.malformed("a gesture's x, y and t must have the same count")
        }
        return t.indices.map { index in
            ReplaySample(
                point: grid.point(x: CGFloat(x[index]), y: CGFloat(y[index])),
                time: start + t[index] / 1000
            )
        }
    }
}

private extension ReplayTap {
    /// The touch's points on Hex's keys, a space-bar sample (y > 1) keeping its fraction
    /// along the recorded 0.25…0.75 across Hex's own space key.
    func samples(grid: TypingReplayGrid, space: CGRect) throws -> [ReplaySample] {
        guard x.count == y.count, y.count == t.count else {
            throw ReplayError.malformed("a tap's x, y and t must have the same count")
        }
        return t.indices.map { index in
            let point: CGPoint
            if y[index] > 1 {
                let fraction = min(1, max(0, (x[index] - 0.25) / 0.5))
                let centre = space.minX + CGFloat(fraction) * space.width
                point = CGPoint(x: min(max(centre, space.minX + 0.5), space.maxX - 0.5), y: space.midY)
            } else {
                point = grid.point(x: CGFloat(x[index]), y: CGFloat(y[index]))
            }
            return ReplaySample(point: point, time: replayClockBase + t[index] / 1000)
        }
    }
}

private enum ReplayError: Error, CustomStringConvertible {
    case malformed(String)
    case missingSwipeModel
    case noSpaceBar
    case grid(String)

    var description: String {
        switch self {
        case .malformed(let message): return message
        case .missingSwipeModel: return "Swipe typing has no English model"
        case .noSpaceBar: return "the keyboard has no space bar"
        case .grid(let message): return message
        }
    }
}

// MARK: - Tallies

private struct ReplayWord {
    var intended: String
    var typed: String
    var inDictionary: Bool
    /// The field has the intended word (or, for a tap, its letter).
    var right: Bool
    var offered: Bool
    /// The gesture became a swipe, so the decoder read this word.
    var swiped = false
    /// The gesture was a swipe attempt: it became one, or its path left the key by a swipe's
    /// pitch and the plane didn't promote it.
    var attempt = false
    /// The recording is a tap: its path never left the key it went down on by a swipe's pitch.
    var tap = false
}

private struct ReplayReport {
    var source: String
    var mode: String
    var sentences = 0
    var words: [ReplayWord] = []
    /// Swipe attempts the plane never took as swipes, or that read nothing.
    var notSwipes = 0
    /// Gestures whose first point wasn't on a letter, which the replay didn't give the plane.
    var skipped = 0
    var decodeTimes: [Double] = []
    var output: [ReplayOutput.Sentence] = []

    var outOfDictionary: Int { words.filter { !$0.inDictionary }.count }
    var inDictionary: Int { words.count - outOfDictionary }
    /// Every in-dictionary word the field got right, taps included and a skipped gesture counted
    /// wrong: the one end-to-end number, what the person reading the field would see.
    var typedRight: Int { words.filter { $0.inDictionary && $0.right }.count }
    /// The in-dictionary words whose gesture was a swipe attempt — one the plane promoted, one
    /// it should have promoted, or one that read nothing: the decoder's own denominator.
    var attempts: Int { words.filter { $0.inDictionary && $0.attempt }.count }
    var attemptsRight: Int { words.filter { $0.inDictionary && $0.attempt && $0.right }.count }
    var attemptsOrOffered: Int { words.filter { $0.inDictionary && $0.attempt && ($0.right || $0.offered) }.count }
    /// The in-dictionary words recorded as taps, and how many of those left their word in the
    /// field. They are what the person typed, not what the decoder read.
    var taps: Int { words.filter { $0.inDictionary && $0.tap }.count }
    var tapsRight: Int { words.filter { $0.inDictionary && $0.tap && $0.right }.count }

    /// One stable line for the report block.
    var line: String {
        var line = "TYPING-REPLAY mode=\(mode) source=\(source) sentences=\(sentences) "
        line += "words=\(words.count) out-of-dictionary=\(outOfDictionary) (\(percent(outOfDictionary, of: words.count))%) "
        line += "in-dictionary=\(inDictionary)"
        if mode == "swipe" {
            line += " attempts=\(attempts) accuracy=\(percent(attemptsRight, of: attempts))%"
            line += " typed-or-offered=\(percent(attemptsOrOffered, of: attempts))%"
            line += " all-words-accuracy=\(percent(typedRight, of: inDictionary))%"
            line += " taps=\(taps) taps-right=\(tapsRight)"
            line += " not-swipes=\(notSwipes) skipped=\(skipped)"
            let median = String(format: "%.2f", percentile(decodeTimes, 0.5))
            let p90 = String(format: "%.2f", percentile(decodeTimes, 0.9))
            line += " decode-ms-median=\(median) decode-ms-p90=\(p90)"
        } else {
            line += " accuracy=\(percent(typedRight, of: inDictionary))%"
        }
        return line
    }
}

private struct ReplayOutput: Encodable {
    struct Sentence: Encodable {
        var source: String
        var session: String
        var mode: String
        var intended: [String]
        var typed: [String]
    }

    var sentences: [Sentence]
}

private func percent(_ part: Int, of whole: Int) -> String {
    guard whole > 0 else { return "0.0" }
    return String(format: "%.1f", Double(part) / Double(whole) * 100)
}

/// The nearest-rank percentile of `values` (0…1), zero when there are none.
private func percentile(_ values: [Double], _ fraction: Double) -> Double {
    guard !values.isEmpty else { return 0 }
    let sorted = values.sorted()
    let rank = Int((fraction * Double(sorted.count)).rounded(.up)) - 1
    return sorted[min(max(rank, 0), sorted.count - 1)]
}
