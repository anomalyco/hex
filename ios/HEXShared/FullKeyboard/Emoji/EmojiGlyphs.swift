import CoreText
import UIKit

/// One emoji drawn for the panel: its bitmap and where it sits, in points, from the centre
/// of the box a centred `UILabel` would lay the emoji's line in.
final class EmojiGlyph {
    let image: CGImage
    let rect: CGRect
    let bytes: Int

    init(image: CGImage, rect: CGRect) {
        self.image = image
        self.rect = rect
        bytes = image.bytesPerRow * image.height
    }
}

/// Draws the emoji panel's emoji into bitmaps of its own, so browsing the panel costs a
/// bounded amount of memory.
///
/// Drawn as text (a `UILabel` per cell, as the panel did), each emoji's image, about 48 KB
/// at the panel's size, stays in a cache of the shared Apple Color Emoji font that nothing
/// public empties: paging through every category kept about 17 MB, and scrolling past
/// every emoji about 90 MB. The renderer draws instead with a private copy of that font,
/// read from the same file (memory-mapped, so its pages are clean and not counted), and
/// replaces the copy after `rendersPerFont` emoji or once drawing pauses, which frees what
/// it cached. Finished bitmaps stay in a least-recently-used cache of `byteLimit` bytes,
/// so scrolling back redraws nothing. The panel owns one and drops it when it closes.
@MainActor
final class EmojiGlyphRenderer {
    struct Key: Hashable {
        let emoji: String
        let size: CGFloat
        let scale: CGFloat
    }

    /// Emoji drawn with one copy of the font before it's replaced.
    let rendersPerFont: Int
    /// What the finished bitmaps may hold.
    let byteLimit: Int

    private(set) var cachedBytes = 0
    /// For probes: emoji drawn, font copies made, and the time drawing took.
    private(set) var renders = 0
    private(set) var fontsMade = 0
    private(set) var renderTime: CFTimeInterval = 0
    private(set) var slowestRender: CFTimeInterval = 0
    private(set) var slowestFont: CFTimeInterval = 0

    private var glyphs: [Key: Node] = [:]
    /// Most recently used first.
    private var head: Node?
    private var tail: Node?

    private let fontData: CFData?
    private var font: CGFont?
    private var fontName: String?
    private var sizedFonts: [CGFloat: CTFont] = [:]
    private var rendersOnFont = 0
    /// The font copy goes once drawing stops for a moment (a page drawn, a scroll ended):
    /// the bitmaps are here, so what it cached is only weight.
    private var idleDrop: DispatchWorkItem?
    static let idleDropDelay: TimeInterval = 0.3
    /// Each size's label box: the ascent and descent of "😀" in the system font.
    private var lineBoxes: [CGFloat: LineBox] = [:]

    private struct LineBox {
        let ascent: CGFloat
        let descent: CGFloat
    }

    private final class Node {
        let key: Key
        let glyph: EmojiGlyph
        /// Weak, so the list holds no cycle once the dictionary lets go.
        weak var previous: Node?
        var next: Node?

        init(key: Key, glyph: EmojiGlyph) {
            self.key = key
            self.glyph = glyph
        }
    }

    init(byteLimit: Int = 2 << 20, rendersPerFont: Int = 32) {
        self.byteLimit = byteLimit
        self.rendersPerFont = max(1, rendersPerFont)
        fontData = Self.emojiFontData()
    }

    /// Whether the renderer found the emoji font's file; without it emoji are drawn with the
    /// system's shared font, as text is.
    var hasPrivateFont: Bool { fontData != nil }

    var cachedCount: Int { glyphs.count }

    /// `emoji` drawn at `size` points for a screen of `scale`, from the cache when it's there.
    func glyph(for emoji: String, size: CGFloat, scale: CGFloat) -> EmojiGlyph? {
        let scale = max(1, scale)
        let key = Key(emoji: emoji, size: size, scale: scale)
        if let node = glyphs[key] {
            moveToFront(node)
            return node.glyph
        }
        let started = CACurrentMediaTime()
        guard let glyph = render(emoji, size: size, scale: scale) else { return nil }
        let took = CACurrentMediaTime() - started
        renders += 1
        renderTime += took
        slowestRender = max(slowestRender, took)
        let node = Node(key: key, glyph: glyph)
        glyphs[key] = node
        insertAtFront(node)
        cachedBytes += glyph.bytes
        while cachedBytes > byteLimit, let last = tail, last !== node {
            remove(last)
        }
        return glyph
    }

    /// Drops every bitmap and the font copy (a memory warning).
    func purge() {
        glyphs.removeAll()
        head = nil
        tail = nil
        cachedBytes = 0
        dropFont()
    }

    // MARK: - Drawing

    private func render(_ emoji: String, size: CGFloat, scale: CGFloat) -> EmojiGlyph? {
        autoreleasepool { draw(emoji, size: size, scale: scale) }
    }

    private func draw(_ emoji: String, size: CGFloat, scale: CGFloat) -> EmojiGlyph? {
        let box = lineBox(size: size)
        // Laid out as a label lays it out (the system font, which falls back to the emoji
        // font at its own size and place); laying out decodes no image.
        let line = CTLineCreateWithAttributedString(NSAttributedString(
            string: emoji,
            attributes: [.font: UIFont.systemFont(ofSize: size)]
        ))
        let width = CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
        var pieces: [(font: CTFont, glyphs: [CGGlyph], positions: [CGPoint])] = []
        var ink = CGRect.null
        for run in (CTLineGetGlyphRuns(line) as? [CTRun]) ?? [] {
            let count = CTRunGetGlyphCount(run)
            guard count > 0 else { continue }
            var glyphs = [CGGlyph](repeating: 0, count: count)
            var positions = [CGPoint](repeating: .zero, count: count)
            CTRunGetGlyphs(run, CFRange(location: 0, length: count), &glyphs)
            CTRunGetPositions(run, CFRange(location: 0, length: count), &positions)
            let attributes = CTRunGetAttributes(run) as NSDictionary
            guard let used = attributes[kCTFontAttributeName as String] else { continue }
            let runFont = used as! CTFont
            // The emoji font's own glyphs drawn with the private copy, so its cache stays
            // ours; anything else (a text-style emoji, a fallback box) as the label would.
            let font = privateFont(matching: runFont) ?? runFont
            var rects = [CGRect](repeating: .zero, count: count)
            CTFontGetBoundingRectsForGlyphs(font, .default, glyphs, &rects, count)
            for (rect, position) in zip(rects, positions) where !rect.isEmpty {
                ink = ink.union(rect.offsetBy(dx: position.x, dy: position.y))
            }
            pieces.append((font, glyphs, positions))
        }
        guard !pieces.isEmpty, !ink.isNull else { return nil }
        let pad = 1 / scale
        ink = ink.insetBy(dx: -pad, dy: -pad)
        // A centred label lays the line in a box as wide as the line and \`ascent + descent\`
        // tall around its centre, the baseline \`ascent\` below the box's top.
        let baselineX = -width / 2
        let baselineY = -(box.ascent + box.descent) / 2 + box.ascent
        let rect = Self.pixelAligned(CGRect(
            x: baselineX + ink.minX,
            y: baselineY - ink.maxY,
            width: ink.width,
            height: ink.height
        ), scale: scale)
        let pixelWidth = Int((rect.width * scale).rounded())
        let pixelHeight = Int((rect.height * scale).rounded())
        guard pixelWidth > 0, pixelHeight > 0,
              let space = CGColorSpace(name: CGColorSpace.displayP3),
              let context = CGContext(
                  data: nil,
                  width: pixelWidth,
                  height: pixelHeight,
                  bitsPerComponent: 8,
                  bytesPerRow: 0,
                  space: space,
                  bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
              ) else { return nil }
        context.scaleBy(x: scale, y: scale)
        // The bitmap's origin is its bottom left, and the baseline \`rect.maxY - baselineY\`
        // above it.
        context.translateBy(x: baselineX - rect.minX, y: rect.maxY - baselineY)
        for piece in pieces {
            CTFontDrawGlyphs(piece.font, piece.glyphs, piece.positions, piece.glyphs.count, context)
        }
        guard let image = context.makeImage() else { return nil }
        rendersOnFont += 1
        if rendersOnFont >= rendersPerFont {
            dropFont()
        } else {
            scheduleIdleDrop()
        }
        return EmojiGlyph(image: image, rect: rect)
    }

    private func lineBox(size: CGFloat) -> LineBox {
        if let box = lineBoxes[size] { return box }
        // Measured, never drawn: laying a line out doesn't decode the emoji's image.
        let line = CTLineCreateWithAttributedString(NSAttributedString(
            string: "😀",
            attributes: [.font: UIFont.systemFont(ofSize: size)]
        ))
        var ascent: CGFloat = 0
        var descent: CGFloat = 0
        CTLineGetTypographicBounds(line, &ascent, &descent, nil)
        let box = LineBox(ascent: ascent, descent: descent)
        lineBoxes[size] = box
        return box
    }

    private static func pixelAligned(_ rect: CGRect, scale: CGFloat) -> CGRect {
        let minX = (rect.minX * scale).rounded(.down) / scale
        let minY = (rect.minY * scale).rounded(.down) / scale
        let maxX = (rect.maxX * scale).rounded(.up) / scale
        let maxY = (rect.maxY * scale).rounded(.up) / scale
        return CGRect(x: minX, y: minY, width: maxX - minX, height: maxY - minY)
    }

    // MARK: - The font copy

    /// The private copy at \`runFont\`'s size, when \`runFont\` is a face of the system emoji
    /// font (the text system's fallback is its UI face, ".AppleColorEmojiUI").
    private func privateFont(matching runFont: CTFont) -> CTFont? {
        let name = CTFontCopyPostScriptName(runFont) as String
        guard name.contains(Self.emojiFontName), let fontData else { return nil }
        let size = CTFontGetSize(runFont)
        if fontName == name, let sized = sizedFonts[size] { return sized }
        if font == nil || fontName != name {
            dropFont()
            let started = CACurrentMediaTime()
            // The face of that name in the font file, made from its bytes, so its caches are
            // its own and go with it.
            let faces = (CTFontManagerCreateFontDescriptorsFromData(fontData) as? [CTFontDescriptor]) ?? []
            let face = faces.first { (CTFontDescriptorCopyAttribute($0, kCTFontNameAttribute) as? String) == name }
            guard let face else { return nil }
            font = CTFontCopyGraphicsFont(CTFontCreateWithFontDescriptor(face, size, nil), nil)
            fontName = name
            fontsMade += 1
            slowestFont = max(slowestFont, CACurrentMediaTime() - started)
        }
        guard let font else { return nil }
        var matrix = CTFontGetMatrix(runFont)
        let sized = CTFontCreateWithGraphicsFont(font, size, &matrix, nil)
        sizedFonts[size] = sized
        return sized
    }

    private func dropFont() {
        idleDrop?.cancel()
        idleDrop = nil
        sizedFonts.removeAll()
        font = nil
        fontName = nil
        rendersOnFont = 0
    }

    private func scheduleIdleDrop() {
        idleDrop?.cancel()
        let drop = DispatchWorkItem { [weak self] in
            MainActor.assumeIsolated { self?.dropFont() }
        }
        idleDrop = drop
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.idleDropDelay, execute: drop)
    }

    /// Whether a font copy is held, for tests.
    var holdsFont: Bool { font != nil }

    static let emojiFontName = "AppleColorEmoji"

    /// The system emoji font's file, memory-mapped.
    private static func emojiFontData() -> CFData? {
        let system = CTFontCreateWithName(emojiFontName as CFString, 12, nil)
        guard let url = CTFontCopyAttribute(system, kCTFontURLAttribute) as? URL,
              let data = try? NSData(contentsOf: url, options: .alwaysMapped) else { return nil }
        return data as CFData
    }

    // MARK: - The list

    private func insertAtFront(_ node: Node) {
        node.previous = nil
        node.next = head
        head?.previous = node
        head = node
        if tail == nil { tail = node }
    }

    private func moveToFront(_ node: Node) {
        guard head !== node else { return }
        unlink(node)
        insertAtFront(node)
    }

    private func unlink(_ node: Node) {
        node.previous?.next = node.next
        node.next?.previous = node.previous
        if head === node { head = node.next }
        if tail === node { tail = node.previous }
        node.previous = nil
        node.next = nil
    }

    private func remove(_ node: Node) {
        unlink(node)
        glyphs[node.key] = nil
        cachedBytes -= node.glyph.bytes
    }
}
