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
}
