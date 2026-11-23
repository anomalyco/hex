import QuartzCore
import UIKit

/// What the engine hands Spatial autocorrect and Record typing logs when the keys or the
/// field change: the letter keys to read taps against, and the keys' and shift's state.
@MainActor
extension FullKeyboardEngine {
    /// The letter keys Spatial autocorrect scores taps against: this layout's, moved by
    /// `tapOffsets`; with Adaptive touch on, by its personal centres instead (none while it
    /// has too little evidence), with the others beside them as the baseline no correction
    /// may go past (`AutocorrectEngine.decoderBaselineKeys`). Off the letters page the last
    /// letters page's stay: a closing mark from the 123 page still reads the word's taps
    /// ("ofthe." -> "of the.").
    func syncDecoderKeys() {
        guard let keys = TypingDecoder.Keys(layout: layout, offsets: tapOffsets) else { return }
        guard let adaptive = adaptiveTouch?.offsets(for: layout),
              let personal = TypingDecoder.Keys(layout: layout, offsets: adaptive) else {
            autocorrect.decoderKeys = keys
            autocorrect.decoderBaselineKeys = nil
            return
        }
        autocorrect.decoderKeys = personal
        autocorrect.decoderBaselineKeys = personal == keys ? nil : keys
    }

    /// Hands the layout and shift to typing logs, which writes a line when either changed.
    func logState(_ cause: String) {
        typingLog?.keyboardState(layout: layout, shift: shift, cause: cause)
    }

    /// Whether a finger going down at `point`, in the strip above the keys, belongs to the
    /// keys: in the strip's lower half while you type (a key is down, or one went down or came
    /// up within `stripTouchGuard`) it is a row-1 key aimed high, not a pick. Nobody reads a
    /// new suggestion and taps it that fast; Record typing logs showed such picks were the e
    /// below the left slot, which kept "th" and typed a space where "the" was meant.
    func takesStripTouch(at point: CGPoint) -> Bool {
        let strip = layout.stripFrame
        guard point.y < layout.touchAreaFrame.minY, point.y >= strip.midY,
              point.x >= strip.minX, point.x < strip.maxX else { return false }
        if !touches.isEmpty { return true }
        guard let last = lastKeyTouchAt else { return false }
        let elapsed = clock() - last
        return elapsed >= 0 && elapsed < Self.stripTouchGuard
    }

    /// `point`, or for one above the keys (a strip touch `takesStripTouch(at:)` gave them)
    /// the same spot on row 1's top edge.
    func pointOnKeys(_ point: CGPoint) -> CGPoint {
        let top = layout.touchAreaFrame.minY
        return point.y < top ? CGPoint(x: point.x, y: top) : point
    }

    /// The kind of key a finger going down at `point` lands on, or nil off the keys: what
    /// `touchBegan(id:at:)` will press, up to the next-letter odds choosing a neighbouring
    /// letter. Cheap and free of side effects, so the key haptic can fire before any typing.
    func keyKind(at point: CGPoint) -> KeyKind? {
        layout.hitTest(pointOnKeys(point))?.kind
    }

    /// The kinds of every key a finger holds down now, its slide's key too.
    var pressedKinds: Set<KeyKind> {
        Set(touches.values.flatMap { touch in [touch.key.kind] + (touch.slideKey.map { [$0.kind] } ?? []) })
    }

    /// A key's label as the keys read it: upper case on the letters page with shift on, and the
    /// Dvorak punctuation keys' `<`, `>` and `:` with a shift you pressed (the docked iPad draws
    /// Apple's own pairs).
    func displayLabel(for key: KeyboardKey) -> String {
        guard let character = key.character else { return key.label }
        if typesFrenchApostrophe(character) { return SmartPunctuation.apostrophe }
        guard isUppercase, layerState.layer == .letters else { return character }
        if let shifted = key.shifted, layout.metrics.pad?.isDocked != true, shift.isUserShift { return shifted }
        return layout.language == .korean ? HangulComposer.shifted(character) : layout.language.uppercased(character)
    }

    /// Whether the letter keys read as capitals: as `isUppercase`, except while a press that
    /// turns shift on is still down (`ShiftStateMachine.pressShowsOff`).
    var keycapsUppercase: Bool { isUppercase && !shift.pressShowsOff }

    /// A key's label as its keycap shows it (`keycapsUppercase`); pop-ups use `displayLabel`.
    func keycapLabel(for key: KeyboardKey) -> String {
        guard let character = key.character else { return key.label }
        if typesFrenchApostrophe(character) { return SmartPunctuation.apostrophe }
        guard keycapsUppercase else { return character }
        if let shifted = key.shifted, layout.metrics.pad?.isDocked != true, shift.isUserShift { return shifted }
        return layout.language == .korean ? HangulComposer.shifted(character) : layout.language.uppercased(character)
    }

    /// The key a touch going down at `point` presses: the one under it, or a neighbouring
    /// letter within `letterReach` points that is enough likelier to come next in the word.
    /// The number row's digits are never letters: a digit's tap stays a digit, and no digit
    /// claims a row-1 letter's.
    func key(at point: CGPoint) -> KeyboardKey? {
        let point = pointOnKeys(point)
        guard let key = layout.hitTest(point) else { return nil }
        guard key.character != nil, !key.isNumberRow, let model = letterOdds, layerState.layer == .letters,
              field.takesWords, autocorrect.isEnabled, target.autocorrectionType != .no,
              shift.state != .locked, model.language == language.rawValue else { return key }
        let reach = letterReach
        let near = layout.keys.filter {
            $0.character != nil && !$0.isNumberRow && $0.kind != key.kind && Self.distance(from: point, to: $0.hitFrame) < reach
        }
        guard !near.isEmpty, let continuations = continuationMass(model) else { return key }
        // An accented letter without a key of its own is typed from its base letter's (é from e).
        var mass = [Double](repeating: 0, count: continuations.count)
        let keyed = Set(layout.keys.compactMap { key in
            key.character.flatMap { $0.count == 1 ? model.codes[Character($0)] : nil }
        })
        for (code, m) in continuations.enumerated() where m > 0 {
            mass[keyed.contains(UInt8(code)) ? code : Int(model.baseCodes[code])] += m
        }
        let total = mass.reduce(0, +)
        func odds(_ k: KeyboardKey) -> Double? {
            guard let c = k.character, c.count == 1, let code = model.codes[Character(c)] else { return nil }
            return log(mass[Int(code)] + total * 1e-4)
        }
        guard let own = odds(key) else { return key }
        var best = key
        var bestScore = own
        for other in near {
            guard let theirs = odds(other) else { continue }
            let score = theirs - letterOddsPerPoint * Double(Self.distance(from: point, to: other.hitFrame))
            if score > bestScore {
                best = other
                bestScore = score
            }
        }
        return best
    }
}
