import QuartzCore
import XCTest
@testable import HEX

/// The emoji panel's ranking: the index, context suggestions, search, the strip's emoji for a
/// finished word, your history, and the 60 hand-written sentences behind the README's numbers.
@MainActor
final class EmojiSuggesterTests: XCTestCase {
    private static var libraries: [String: EmojiLibrary] = [:]

    private func library(_ language: String = "en") throws -> EmojiLibrary {
        if let cached = Self.libraries[language] { return cached }
        let loaded = try XCTUnwrap(EmojiLibrary.load(language: language, bundle: Bundle(for: FullKeyboardView.self)))
        Self.libraries[language] = loaded
        return loaded
    }

    private func suggest(_ text: String, usage: [Int: Float] = [:], limit: Int = 5, language: String = "en") throws -> [String] {
        let library = try library(language)
        let context = EmojiContext(before: text, stopwords: library.stopwords)
        return EmojiSuggester.suggestions(context: context, library: library, usage: usage, limit: limit, fill: false)
            .map { library.entries[$0].emoji }
    }

    // MARK: - Index

    func testBundledCacheReusesInstanceAcrossPresentationsAndEvictsOnMemoryWarning() throws {
        EmojiLibrary.evictCached()
        let bundle = Bundle(for: FullKeyboardView.self)
        let first = try XCTUnwrap(EmojiLibrary.bundled(language: "en", bundle: bundle))
        let second = try XCTUnwrap(EmojiLibrary.bundled(language: "en", bundle: bundle))
        XCTAssertTrue(first === second, "presentations in the same language share one EmojiLibrary")
        let fallback = try XCTUnwrap(EmojiLibrary.bundled(language: "fr", bundle: bundle))
        XCTAssertTrue(first === fallback, "languages using the English index share the cached English instance")
        let spanish = try XCTUnwrap(EmojiLibrary.bundled(language: "es", bundle: bundle))
        XCTAssertFalse(first === spanish)
        EmojiLibrary.evictCached()
        let afterWarning = try XCTUnwrap(EmojiLibrary.bundled(language: "es", bundle: bundle))
        XCTAssertFalse(spanish === afterWarning, "a memory warning releases the cached library")
    }

    func testLoadsEveryCategoryAndOnlyEmojiTheFontDraws() throws {
        let library = try library()
        XCTAssertGreaterThan(library.entries.count, 1_850)
        for category in EmojiCategory.allCases {
            XCTAssertTrue(library.entries.contains { $0.category == category }, "\(category)")
        }
        // Emoji newer than every supported iOS draws are listed only where the font has them.
        XCTAssertTrue(EmojiLibrary.fontDraws("🍕"))
        XCTAssertTrue(EmojiLibrary.fontDraws("🧑🏽‍🚀"))
        XCTAssertFalse(EmojiLibrary.fontDraws("\u{E000}"))
        XCTAssertFalse(EmojiLibrary.fontDraws("🍕🍕"))
        let wave = try XCTUnwrap(library.entryByEmoji["👋"])
        XCTAssertEqual(library.entries[wave].tones.count, 5)
        XCTAssertEqual(library.entryByEmoji["👋🏽"], wave)
        XCTAssertEqual(library.name(of: wave), "waving hand")
    }

    func testLooksUpWordsAndPhrases() throws {
        let library = try library()
        let pizza = try XCTUnwrap(library.hits(forWord: "pizza"))
        XCTAssertEqual(library.entries[Int(pizza[0].entry)].emoji, "🍕")
        XCTAssertNil(library.hits(forWord: "qqqzzz"))
        let newYear = try XCTUnwrap(library.hits(forPhrase: "new year"))
        XCTAssertEqual(library.entries[Int(newYear[0].entry)].emoji, "🎆")
    }

    func testFoldsLikeTheIndex() {
        XCTAssertEqual(EmojiText.fold("Café Ñandú"), "cafe nandu")
        XCTAssertEqual(EmojiText.words(in: "That’s SO good!!"), ["that", "so", "good"])
        XCTAssertEqual(EmojiText.collapsingRepeats("soooo"), ["soo", "so"])
        XCTAssertEqual(EmojiText.collapsingRepeats("good"), [])
    }

    // MARK: - Context

    func testSuggestsTheWordsOwnEmoji() throws {
        XCTAssertEqual(try suggest("pizza tonight? ").prefix(3).contains("🍕"), true)
        let launch = try suggest("congrats on the launch ")
        XCTAssertTrue(launch.contains("🎉"), "\(launch)")
        XCTAssertTrue(launch.contains("🚀"), "\(launch)")
        XCTAssertTrue(try suggest("happy new year ").contains("🎆"))
    }

    func testTheNewestWordCountsMost() throws {
        let tail = try suggest("pizza was great, now coffee ")
        XCTAssertLessThan(try XCTUnwrap(tail.firstIndex(of: "☕")), try XCTUnwrap(tail.firstIndex(of: "🍕")))
    }

    func testShowsOneEmojiPerFamily() throws {
        let run = try suggest("going for a run ", limit: 8)
        XCTAssertEqual(run.filter { $0.hasPrefix("🏃") }.count, 1, "\(run)")
    }

    func testReadsHeldDownLettersAndEndings() throws {
        XCTAssertTrue(try suggest("lmaooo ").contains { $0 == "🤣" || $0 == "😂" })
        XCTAssertTrue(try suggest("two dogs ").contains { $0 == "🐶" || $0 == "🐕" })
    }

    func testWordStillBeingTypedFindsWhatItStarts() throws {
        XCTAssertTrue(try suggest("piz").contains("🍕"))
    }

    func testYourEmojiBreakTiesAndFillTheRow() throws {
        let library = try library()
        let fire = try XCTUnwrap(library.entryByEmoji["🔥"])
        let filled = EmojiSuggester.suggestions(
            context: EmojiContext(before: "", stopwords: library.stopwords),
            library: library,
            usage: [fire: 1],
            limit: 4
        ).map { library.entries[$0].emoji }
        XCTAssertEqual(filled.first, "🔥")
        XCTAssertEqual(filled.count, 4)
    }

    func testOtherLayoutsReadTheirLanguageAndEnglish() throws {
        XCTAssertTrue(try suggest("en kaffe ", language: "nb").contains("☕"))
        XCTAssertTrue(try suggest("feliz cumpleaños ", language: "es").contains("🎂"))
        XCTAssertTrue(try suggest("pizza heute ", language: "de").contains("🍕"))
    }

    // MARK: - Strip and search

    func testStripOffersEmojiForAFinishedWordOnly() throws {
        let library = try library()
        func strip(_ text: String) -> [String] {
            EmojiSuggester.forFinishedWord(text, library: library).map { library.entries[$0].emoji }
        }
        XCTAssertEqual(strip("pizza "), ["🍕"])
        XCTAssertEqual(strip("Happy birthday "), ["🎂", "🎉", "🥳"])
        XCTAssertEqual(strip("pizza"), [])
        XCTAssertEqual(strip("pizza  "), [])
        XCTAssertEqual(strip("on the "), [])
        XCTAssertEqual(strip("at 7 "), [])
        // A word several emoji only describe offers none.
        XCTAssertEqual(strip("face "), [])
    }

    func testSearchMatchesNamesBeforeKeywords() throws {
        let library = try library()
        func search(_ query: String) -> [String] {
            EmojiSuggester.search(query, library: library, limit: 10).map { library.entries[$0].emoji }
        }
        XCTAssertEqual(search("piz").first, "🍕")
        XCTAssertTrue(search("heart").prefix(3).contains("❤️"), "\(search("heart"))")
        XCTAssertEqual(search("flag jap").first, "🇯🇵")
        XCTAssertEqual(search("zzzzqq"), [])
    }

    // MARK: - History

    func testHistoryFadesByHalfEveryTenDaysAndKeepsTones() throws {
        let suite = "anomaly.hex.tests.emoji"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defaults.removePersistentDomain(forName: suite)
        var now = Date(timeIntervalSince1970: 1_000_000)
        let store = EmojiUsageStore(defaults: defaults, now: { now })
        store.record("🍕")
        store.record("🍕")
        now += 20 * 86_400
        store.record("☕")
        // 🍕: 2 halved twice = 0.5; ☕: 1.
        XCTAssertEqual(store.ranked().map(\.emoji), ["☕", "🍕"])
        XCTAssertEqual(store.ranked().last?.score ?? 0, 0.5, accuracy: 0.001)
        store.setTone(3, for: "👍")
        XCTAssertEqual(EmojiUsageStore(defaults: defaults).tone(for: "👍"), 3)
        store.clear()
        XCTAssertTrue(EmojiUsageStore(defaults: defaults).isEmpty)
        defaults.removePersistentDomain(forName: suite)
    }

    func testSearchTypingGoesToTheQueryNotTheField() {
        let field = FixtureTextTarget(text: "Hi ")
        let routed = EmojiRoutedTarget(document: field)
        var returned = false
        routed.onSearchReturn = { returned = true }
        routed.query = ""
        routed.insertText("ca")
        routed.insertText("t")
        routed.deleteBackward()
        XCTAssertEqual(routed.query, "ca")
        XCTAssertEqual(routed.autocorrectionType, .no)
        routed.insertText("\n")
        XCTAssertTrue(returned)
        XCTAssertEqual(field.documentContextBeforeInput, "Hi ")
        routed.query = nil
        routed.insertText("🍕")
        XCTAssertEqual(field.documentContextBeforeInput, "Hi 🍕")
    }

    // MARK: - Drawing

    /// The box, in points, that \`draw\`'s opaque pixels cover in a 3x bitmap of \`size\`.
    private func inkBounds(_ size: CGSize, _ draw: (CGContext) -> Void) throws -> CGRect {
        let width = Int(size.width * 3)
        let height = Int(size.height * 3)
        let context = try XCTUnwrap(CGContext(
            data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
            space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
        ))
        // UIKit's coordinates: the origin at the top left, in points.
        context.translateBy(x: 0, y: CGFloat(height))
        context.scaleBy(x: 3, y: -3)
        draw(context)
        let pixels = try XCTUnwrap(context.data).bindMemory(to: UInt8.self, capacity: width * height * 4)
        var minX = width, minY = height, maxX = -1, maxY = -1
        for y in 0..<height {
            for x in 0..<width where pixels[(y * width + x) * 4 + 3] > 24 {
                minX = min(minX, x); maxX = max(maxX, x); minY = min(minY, y); maxY = max(maxY, y)
            }
        }
        XCTAssertGreaterThanOrEqual(maxX, 0, "nothing drawn")
        return CGRect(x: CGFloat(minX) / 3, y: CGFloat(minY) / 3, width: CGFloat(maxX - minX + 1) / 3, height: CGFloat(maxY - minY + 1) / 3)
    }

    /// The grid's cells draw their own bitmaps (iOS keeps every emoji a label draws), so
    /// they must land where the label put them.
    func testAnEmojiCellDrawsItsEmojiWhereALabelDid() throws {
        let renderer = EmojiGlyphRenderer()
        XCTAssertTrue(renderer.hasPrivateFont)
        let size = CGSize(width: 46, height: 40)
        for emoji in ["😀", "👍🏽", "🇳🇴", "❤️", "#️⃣", "🧑‍🧑‍🧒", "©️"] {
            for fontSize: CGFloat in [32, 26] {
                let label = UILabel(frame: CGRect(origin: .zero, size: size))
                label.textAlignment = .center
                label.font = .systemFont(ofSize: fontSize)
                label.text = emoji
                let expected = try inkBounds(size) { context in
                    UIGraphicsPushContext(context)
                    label.drawText(in: label.bounds)
                    UIGraphicsPopContext()
                }
                let cell = EmojiCell(frame: CGRect(origin: .zero, size: size))
                cell.configure(emoji: emoji, name: emoji, fontSize: fontSize, palette: FullKeyboardPalette(isDark: false), renderer: renderer, scale: 3)
                cell.layoutIfNeeded()
                let drawn = try inkBounds(size) { cell.contentView.layer.render(in: $0) }
                for (a, b, edge) in [(drawn.minX, expected.minX, "left"), (drawn.maxX, expected.maxX, "right"),
                                     (drawn.minY, expected.minY, "top"), (drawn.maxY, expected.maxY, "bottom")] {
                    XCTAssertEqual(a, b, accuracy: 0.7, "\(emoji) at \(fontSize) pt: \(edge) edge \(drawn) vs \(expected)")
                }
            }
        }
    }

    /// Paging past every emoji keeps a bounded set of bitmaps, reuses what it has, and leaves
    /// the process about where it started (drawn as text it grew by about 90 MB).
    func testDrawingEveryEmojiStaysWithinTheRenderersLimit() throws {
        let library = try library()
        let renderer = EmojiGlyphRenderer(byteLimit: 1 << 20, rendersPerFont: 32)
        let before = Self.footprint()
        for (index, entry) in library.entries.enumerated() {
            XCTAssertNotNil(renderer.glyph(for: entry.emoji, size: 29, scale: 3), entry.emoji)
            XCTAssertLessThanOrEqual(renderer.cachedBytes, 1 << 20)
            // The panel draws a screen at a time and its run loop drains between them.
            if index % 40 == 39 { RunLoop.current.run(until: Date()) }
        }
        let grown = Self.footprint() - before
        XCTAssertLessThan(grown, 12, "drawing every emoji grew the process by \(grown) MB")
        XCTAssertGreaterThan(renderer.fontsMade, library.entries.count / 32 - 1)
        let last = try XCTUnwrap(library.entries.last?.emoji)
        let renders = renderer.renders
        let first = renderer.glyph(for: last, size: 29, scale: 3)
        XCTAssertTrue(first === renderer.glyph(for: last, size: 29, scale: 3))
        XCTAssertEqual(renderer.renders, renders, "a cached emoji was drawn again")
        renderer.purge()
        XCTAssertEqual(renderer.cachedBytes, 0)
        XCTAssertEqual(renderer.cachedCount, 0)
    }

    func testTheGridShowsTheSkinToneYouPicked() throws {
        let library = try library()
        let suite = "anomaly.hex.tests.emoji-grid"
        let defaults = try XCTUnwrap(UserDefaults(suiteName: suite))
        defaults.removePersistentDomain(forName: suite)
        let usage = EmojiUsageStore(defaults: defaults)
        usage.setTone(3, for: "👍")
        let thumbs = try XCTUnwrap(library.entryByEmoji["👍"])
        let panel = EmojiPanelView(library: library, usage: usage)
        panel.frame = CGRect(x: 0, y: 0, width: 402, height: 300)
        panel.show(suggestions: [thumbs], frequent: [thumbs])
        panel.layoutIfNeeded()
        let grid = try XCTUnwrap(panel.subviews.compactMap { $0 as? UICollectionView }.first { !($0.superview is EmojiRowView) })
        grid.layoutIfNeeded()
        let first = grid.cellForItem(at: IndexPath(item: 0, section: 0)) as? EmojiCell
        XCTAssertEqual(first?.emoji, library.entries[thumbs].tones[2])
        XCTAssertGreaterThan(grid.visibleCells.count, 20)
        defaults.removePersistentDomain(forName: suite)
    }

    /// A tap a little under or over the bar's icons, or beside ABC and delete, still lands
    /// on them (it used to hit nothing, so a category could take two taps): the whole band
    /// under the grid, each category across the full width of its slot.
    func testTheBarsKeysTakeTheWholeBandUnderTheGrid() throws {
        let library = try library()
        let panel = EmojiPanelView(library: library, usage: EmojiUsageStore(defaults: UserDefaults(suiteName: "anomaly.hex.tests.emoji-band")))
        panel.frame = CGRect(x: 0, y: 0, width: 402, height: 300)
        panel.show(suggestions: [], frequent: [])
        panel.layoutIfNeeded()
        let bar = try XCTUnwrap(panel.subviews.compactMap { $0 as? EmojiCategoryBar }.first)
        let buttons = bar.subviews.compactMap { $0 as? UIButton }
        XCTAssertEqual(buttons.count, EmojiCategory.allCases.count + 1)
        let grid = try XCTUnwrap(panel.subviews.compactMap { $0 as? UICollectionView }.first { !($0.superview is EmojiRowView) })
        for (index, button) in buttons.enumerated() {
            let frame = button.convert(button.bounds, to: panel)
            // The bar reaches the panel's bottom edge, as row 4 does, so nothing is under it there.
            for y in [grid.frame.maxY + 1, frame.minY - 1, frame.midY, frame.maxY + 1, panel.bounds.maxY - 1] where y < panel.bounds.maxY {
                for x in [frame.minX + 0.5, frame.midX, frame.maxX - 0.5] {
                    let point = CGPoint(x: x, y: y)
                    let hit = panel.hitTest(point, with: nil)
                    XCTAssertTrue(hit === bar, "\(button.accessibilityLabel ?? "") at \(point): \(String(describing: hit))")
                    XCTAssertEqual(bar.slot(atX: panel.convert(point, to: bar).x).index, index, "\(button.accessibilityLabel ?? "") at \(point)")
                }
            }
        }
        let below = panel.bounds.maxY - 1
        XCTAssertEqual(panel.hitTest(CGPoint(x: 20, y: below), with: nil)?.accessibilityLabel, "Letters")
        XCTAssertEqual(panel.hitTest(CGPoint(x: 390, y: below), with: nil)?.accessibilityLabel, "Delete")
        XCTAssertTrue(panel.hitTest(CGPoint(x: 200, y: grid.frame.midY), with: nil)?.isDescendant(of: grid) ?? false)
    }

    /// As on the system emoji keyboard: a tap on a category jumps the grid there when the
    /// finger lifts; a finger sliding along the bar scrubs the grid through every category
    /// live, with a light tick for each new one and no key click; and the bar follows the
    /// grid when it scrolls by hand.
    func testSlidingAlongTheBarScrubsTheGrid() throws {
        let library = try library()
        let panel = EmojiPanelView(library: library, usage: EmojiUsageStore(defaults: UserDefaults(suiteName: "anomaly.hex.tests.emoji-scrub")))
        panel.frame = CGRect(x: 0, y: 0, width: 402, height: 300)
        panel.show(suggestions: [], frequent: [])
        panel.layoutIfNeeded()
        var clicks = 0, ticks = 0
        panel.onKeyDown = { clicks += 1 }
        panel.onSelectionStep = { ticks += 1 }
        let bar = try XCTUnwrap(panel.subviews.compactMap { $0 as? EmojiCategoryBar }.first)
        let grid = try XCTUnwrap(panel.subviews.compactMap { $0 as? UICollectionView }.first { !($0.superview is EmojiRowView) })
        grid.layoutIfNeeded()
        let count = EmojiCategory.allCases.count + 1
        let slot = bar.bounds.width / CGFloat(count)
        let food = 1 + EmojiCategory.foodDrink.rawValue
        let foodX = slot * (CGFloat(food) + 0.5)

        bar.touchBegan(atX: foodX)
        bar.touchMoved(toX: foodX + 4)
        XCTAssertEqual(grid.contentOffset.x, 0, "a finger that hardly moves is a tap")
        XCTAssertEqual(bar.selected, 0)
        bar.touchEnded(atX: foodX + 4)
        XCTAssertEqual(bar.selected, food, "a tap picks its category as it lifts")
        let foodStart = grid.contentOffset.x
        XCTAssertGreaterThan(foodStart, 0)
        XCTAssertEqual(clicks, 1)
        XCTAssertEqual(ticks, 0)

        // From Smileys to the far end of Flags, a few points a move.
        var x = slot * 1.1
        bar.touchBegan(atX: x)
        var offsets: [CGFloat] = []
        var seen: [Int] = []
        while x < bar.bounds.width + 20 {
            x += 3
            bar.touchMoved(toX: x)
            guard bar.isScrubbing else { continue }
            offsets.append(grid.contentOffset.x)
            if seen.last != bar.selected { seen.append(bar.selected) }
        }
        XCTAssertEqual(seen, Array(1..<count), "the chip follows the finger through every category")
        XCTAssertEqual(ticks, count - 1, "a tick for each new category (Food and drink to Smileys first)")
        XCTAssertEqual(offsets, offsets.sorted(), "the grid only moves on as the finger does")
        XCTAssertGreaterThan(Set(offsets).count, 40, "the grid moves within each category too, not only between them")
        let maxX = grid.contentSize.width - grid.bounds.width
        XCTAssertEqual(offsets.last ?? 0, maxX, accuracy: 1, "the end of the bar is the end of the grid")
        XCTAssertTrue(bar.isScrubbing)
        bar.touchEnded(atX: x)
        XCTAssertFalse(bar.isScrubbing)
        XCTAssertEqual(bar.selected, count - 1, "lifting keeps the grid and the chip where the slide left them")
        XCTAssertEqual(grid.contentOffset.x, maxX, accuracy: 1)
        XCTAssertEqual(clicks, 1, "a slide types no key click")

        // A slide back stops at Food and drink's start where its slot begins.
        bar.touchBegan(atX: bar.bounds.width - 1)
        bar.touchMoved(toX: slot * CGFloat(food) + 0.01)
        XCTAssertEqual(bar.selected, food)
        XCTAssertEqual(grid.contentOffset.x, foodStart, accuracy: 1)
        bar.touchCancelled()

        // The grid scrolled by hand moves the chip.
        grid.setContentOffset(CGPoint(x: 0, y: 0), animated: false)
        XCTAssertEqual(bar.selected, 0)
        grid.setContentOffset(CGPoint(x: foodStart, y: 0), animated: false)
        XCTAssertEqual(bar.selected, food)
    }

    private static func footprint() -> Double {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        return result == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : 0
    }

    // MARK: - Quality

    private struct Sentence: Decodable {
        let text: String
        let accept: [String]
    }

    private struct Fixture: Decodable {
        let dev: [Sentence]
        let heldOut: [Sentence]
        let blind: [Sentence]
    }

    private static func plain(_ emoji: String) -> String {
        String(String.UnicodeScalarView(emoji.unicodeScalars.filter {
            $0.value != 0xFE0F && !(0x1F3FB...0x1F3FF).contains($0.value)
        }))
    }

    /// How many sentences get a sensible emoji in the top five, with the words' own emoji
    /// (CLDR and Hex's chat words) and then with Apple's word embedding added. The README
    /// quotes these; the floors catch a ranking that got worse.
    func testSentencesGetASensibleEmojiInTheTopFive() throws {
        let url = try XCTUnwrap(Bundle(for: Self.self).url(forResource: "emoji-context-sentences", withExtension: "json"))
        let fixture = try JSONDecoder().decode(Fixture.self, from: Data(contentsOf: url))
        let library = try library()

        let start = CACurrentMediaTime()
        let semantic = EmojiSemanticIndex(language: "en", vocabulary: library.vocabulary)
        let built = CACurrentMediaTime() - start
        let neighbors: EmojiSuggester.Neighbors? = semantic.map { index in { index.neighbors(of: $0) } }

        func score(_ sentences: [Sentence], _ neighbors: EmojiSuggester.Neighbors?) -> (hits: Int, top1: Int, ms: Double) {
            var hits = 0
            var top1 = 0
            let begin = CACurrentMediaTime()
            for sentence in sentences {
                let context = EmojiContext(before: sentence.text + " ", stopwords: library.stopwords)
                let top = EmojiSuggester.suggestions(context: context, library: library, neighbors: neighbors, limit: 5, fill: false)
                    .map { Self.plain(library.entries[$0].emoji) }
                let accepted = Set(sentence.accept.map(Self.plain))
                if top.contains(where: accepted.contains) { hits += 1 }
                if let first = top.first, accepted.contains(first) { top1 += 1 }
            }
            return (hits, top1, (CACurrentMediaTime() - begin) * 1000 / Double(max(1, sentences.count)))
        }
        let devWords = score(fixture.dev, nil)
        let heldWords = score(fixture.heldOut, nil)
        let blindWords = score(fixture.blind, nil)
        let devSemantic = score(fixture.dev, neighbors)
        let heldSemantic = score(fixture.heldOut, neighbors)
        let blindSemantic = score(fixture.blind, neighbors)
        print("EMOJI_QUALITY words: dev \(devWords.hits)/30 (top-1 \(devWords.top1)), held-out \(heldWords.hits)/30 (top-1 \(heldWords.top1)), blind \(blindWords.hits)/\(fixture.blind.count) (top-1 \(blindWords.top1)), \(String(format: "%.2f", heldWords.ms)) ms a sentence")
        print("EMOJI_QUALITY words + embeddings: dev \(devSemantic.hits)/30 (top-1 \(devSemantic.top1)), held-out \(heldSemantic.hits)/30 (top-1 \(heldSemantic.top1)), blind \(blindSemantic.hits)/\(fixture.blind.count) (top-1 \(blindSemantic.top1)), \(String(format: "%.2f", heldSemantic.ms)) ms a sentence, index built in \(String(format: "%.0f", built * 1000)) ms (\(semantic?.vocabularySize ?? 0) words)")
        XCTAssertGreaterThanOrEqual(devWords.hits, 28)
        XCTAssertGreaterThanOrEqual(heldWords.hits, 27)
        XCTAssertGreaterThanOrEqual(blindWords.hits, 35)
    }
}
