import CoreGraphics

/// Key flicks on a docked iPad (`KeyboardFlicks`): a key pulled down types the character
/// over its letter.
@MainActor
extension FullKeyboardEngine {
    /// A move of a key that flicks: once the finger has gone down more than across, the touch
    /// keeps its key (no slide to the key below, no swipe) and its pull is tracked. False for
    /// every other touch, which moves as before.
    func moveFlick(_ touch: inout ActiveTouch, id: Int, to point: CGPoint) -> Bool {
        guard KeyboardFlicks.isEnabled, layout.metrics.pad?.isDocked == true, !touch.committed,
              touch.key.flick != nil, activeAccentMenu == nil else { return false }
        let dy = point.y - touch.startPoint.y
        let dx = abs(point.x - touch.startPoint.x)
        if touch.flick == nil {
            guard dy > 4, dy > dx else { return false }
        }
        let progress = KeyboardFlicks.progress(pull: dy)
        guard progress != touch.flick else {
            touches[id] = touch
            return true
        }
        touch.flick = progress
        touches[id] = touch
        onChange?()
        return true
    }

    /// Letting go of a pulled key: past half way, its character goes in as a typed one (as a
    /// slide from 123 does: a mark after a word corrects it, a digit is just a digit). False
    /// otherwise, and the letter goes in.
    func commitFlick(_ touch: ActiveTouch) -> Bool {
        guard let progress = touch.flick, progress >= KeyboardFlicks.commitProgress, let character = touch.key.flick else { return false }
        typingLog?.ui("flick", [TypingLogField("key", .string(touch.key.label)), TypingLogField("chosen", .string(character))])
        if dropsSuggestionSpace(before: character) || !correctWord(endingWith: character) {
            insert(character, source: "flick")
        }
        if layerState.layer == .letters { shift.didType() }
        refreshAutoCapitalization()
        logState("typed")
        return true
    }
}
