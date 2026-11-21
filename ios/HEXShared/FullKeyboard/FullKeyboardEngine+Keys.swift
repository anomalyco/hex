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

    /// A key's label as the keys read it: upper case on the letters page with shift on.
    func displayLabel(for key: KeyboardKey) -> String {
        guard let character = key.character else { return key.label }
        return isUppercase && layerState.layer == .letters ? character.uppercased() : character
    }

    /// Whether the letter keys read as capitals: as `isUppercase`, except while a press that
    /// turns shift on is still down (`ShiftStateMachine.pressShowsOff`).
    var keycapsUppercase: Bool { isUppercase && !shift.pressShowsOff }

    /// A key's label as its keycap shows it (`keycapsUppercase`); pop-ups use `displayLabel`.
    func keycapLabel(for key: KeyboardKey) -> String {
        guard let character = key.character else { return key.label }
        return keycapsUppercase ? character.uppercased() : character
    }
}
