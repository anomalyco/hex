import UIKit

/// The space bar's trackpad, as Apple's moves it (`TrackpadCursor`): held, or with one language
/// slid, it moves the cursor through the text in two dimensions, faster the faster the finger.
@MainActor
extension FullKeyboardEngine {
    /// The space bar held `trackpadHoldDelay` without sliding: the trackpad starts where the
    /// finger is, as Apple's does, so letting go types no space (a press held to move the
    /// cursor used to type one beside the space already there).
    func holdSpace(forTouch id: Int) {
        guard var touch = touches[id], touch.key.kind == .space, !touch.isTrackpad, !touch.isLanguageSwipe else { return }
        startTrackpad(&touch, id: id, hold: true, time: nil)
    }

    /// A space bar touch moved to `point`: true once it's the keys' to keep (a language swipe,
    /// or the trackpad starting or moving the cursor), false while it may still be a tap.
    func moveSpaceBar(_ touch: inout ActiveTouch, id: Int, to point: CGPoint, time: TimeInterval?) -> Bool {
        if touch.isLanguageSwipe { return true }
        if !touch.isTrackpad, abs(point.x - touch.startPoint.x) >= Self.trackpadActivationDistance {
            if switchesLanguage {
                touch.isLanguageSwipe = true
            } else {
                startTrackpad(&touch, id: id, hold: false, time: time)
            }
            return true
        }
        guard touch.isTrackpad else { return false }
        let offset = trackpadStep(&touch, to: point, time: time ?? clock())
        moveCursor(&touch, id: id, by: offset)
        return true
    }

    private func startTrackpad(_ touch: inout ActiveTouch, id: Int, hold: Bool, time: TimeInterval?) {
        touch.isTrackpad = true
        touch.trackpadPath = [TrackpadCursor.Sample(time: time ?? clock(), point: touch.point)]
        touch.trackpad = TrackpadCursor.State(
            before: target.documentContextBeforeInput ?? "",
            after: target.documentContextAfterInput ?? "",
            wrapWidth: TrackpadCursor.wrapWidth(forWidth: width),
            lineHeight: Self.trackpadFont().lineHeight,
            width: Self.trackpadWidth
        )
        trackpadUpdated(touch, id: id, started: true)
        var fields = [TypingLogField("id", .int(id))]
        if hold { fields.append(TypingLogField("hold", .bool(true))) }
        typingLog?.ui("trackpad_start", fields)
        onChange?()
    }

    /// The finger moved to `point` at `time`: the UTF-16 offset to move the cursor by. The
    /// finger's speed sets how far the point goes (`TrackpadCursor.gain(speed:)`); with no time
    /// between moves (a test's clock) it goes as far as the finger.
    private func trackpadStep(_ touch: inout ActiveTouch, to point: CGPoint, time: TimeInterval) -> Int {
        let last = touch.trackpadPath.last ?? TrackpadCursor.Sample(time: time, point: point)
        let sample = TrackpadCursor.Sample(time: time, point: point)
        touch.trackpadPath = time > last.time ? touch.trackpadPath.suffix(7) + [sample] : [sample]
        guard var state = touch.trackpad else { return 0 }
        let gain = TrackpadCursor.gain(speed: TrackpadCursor.speed(along: touch.trackpadPath))
        let dx = point.x - last.point.x, dy = point.y - last.point.y
        let vertical = touch.trackpadPath.count > 1 ? TrackpadCursor.isVertical(touch.trackpadPath) : abs(dy) > abs(dx)
        var offset = state.move(by: CGVector(dx: dx * gain, dy: dy * gain), at: time, vertical: vertical)
        if state.nearsEdge, catchUp(&state) { offset += state.settle(at: time) }
        touch.trackpad = state
        return offset
    }

    /// Adds what the field shows beyond the trackpad's copy of the text, if it does.
    private func catchUp(_ state: inout TrackpadCursor.State) -> Bool {
        state.catchUp(before: target.documentContextBeforeInput ?? "", after: target.documentContextAfterInput ?? "", width: Self.trackpadWidth)
    }

    /// The field changed: while the trackpad is on that's mostly the host following its moves.
    /// It may show more of the text, or that the cursor is at the text's start or a line's end,
    /// so the cursor goes where the point is once more.
    func trackpadFieldChanged() {
        guard let (id, found) = touches.first(where: { $0.value.isTrackpad }), var state = found.trackpad else { return }
        var touch = found
        if state.nearsEdge { _ = catchUp(&state) }
        let offset = state.settle(at: clock())
        touch.trackpad = state
        moveCursor(&touch, id: id, by: offset)
    }

    /// The trackpad finger lifts: the cursor goes where the point is, among the rows known.
    func liftTrackpad(_ touch: inout ActiveTouch) {
        guard var state = touch.trackpad else { return }
        let offset = state.settle(at: clock(), lifting: true)
        touch.trackpad = state
        log(&touch, offset)
        if offset != 0 { trackpadMovedCursor(by: offset) }
    }

    private func moveCursor(_ touch: inout ActiveTouch, id: Int, by offset: Int) {
        log(&touch, offset)
        trackpadUpdated(touch, id: id)
        if offset != 0 { trackpadMovedCursor(by: offset) }
    }

    private func log(_ touch: inout ActiveTouch, _ offset: Int) {
        guard offset != 0 else { return }
        touch.trackpadOffset += offset
        typingLog?.edit("cursor", [TypingLogField("offset", .int(offset))])
    }

    private static var trackpadWidths: (font: UIFont, widths: [Character: CGFloat]) = (.systemFont(ofSize: 17), [:])

    /// Body text, as most fields show it (17 pt, larger with Dynamic Type): the font the
    /// trackpad measures characters in (`trackpadWidth`).
    static func trackpadFont() -> UIFont {
        let font = UIFont.preferredFont(forTextStyle: .body)
        if font != trackpadWidths.font { trackpadWidths = (font, [:]) }
        return font
    }

    /// A character's width in `trackpadFont()`, measured once.
    static func trackpadWidth(_ character: Character) -> CGFloat {
        if let width = trackpadWidths.widths[character] { return width }
        let width = character.isNewline ? 0 : (String(character) as NSString).size(withAttributes: [.font: trackpadWidths.font]).width
        trackpadWidths.widths[character] = width
        return width
    }
}
