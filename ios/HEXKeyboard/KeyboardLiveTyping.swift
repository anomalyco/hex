import Combine
import Foundation

/// Type as I speak in the keyboard: follows the live words (`LiveDictationText`) into the
/// field through `LiveFieldTyping` while the switch was on as the dictation started (never
/// with Translate what I say on), hands the field back as it was just before the dictation's
/// own text goes in (`handOver`), and takes the words back when a dictation ends without
/// text (cancelled, failed, nothing heard) or the keyboard hides mid-dictation (its text is
/// then held for the next keyboard, as without the switch).
@MainActor
final class KeyboardLiveTyping {
    private let target: KeyboardTextTarget
    private var typing: LiveFieldTyping?
    /// The dictation being followed (`LiveDictationText.dictation`), typed into or declined.
    private var dictation = 0
    private var subscription: AnyCancellable?
    /// False while the full keyboard waits to put a two-thumb Dictate's words over its range.
    var mayType: () -> Bool = { true }
    /// The field changed under the keys: they read it afresh.
    var didEdit: () -> Void = {}

    init(target: KeyboardTextTarget) {
        self.target = target
    }

    func attach(_ text: LiveDictationText) {
        subscription = text.$transcript.sink { [weak self, weak text] transcript in
            guard let self, let text else { return }
            self.follow(transcript, in: text)
        }
    }

    private func follow(_ transcript: LiveTranscript?, in text: LiveDictationText) {
        guard let transcript else {
            // Over without its text going in: the field goes back as it was.
            cancel()
            return
        }
        if typing == nil {
            guard text.typesIntoField, dictation != text.dictation, !transcript.isEmpty else { return }
            dictation = text.dictation
            guard mayType() else { return }
            guard let started = LiveFieldTyping(target: target) else {
                KeyboardProbeLog.shared.write("live: not typing in this field")
                return
            }
            typing = started
        }
        guard let typing, !typing.stopped else { return }
        typing.show(transcript.text)
        if typing.stopped {
            KeyboardProbeLog.shared.write("live: the field changed, stopped typing")
        }
        didEdit()
    }

    /// The dictation's own text is about to go in: the live words come out first, and the field
    /// as it was before them is returned for its spacing. Nil when nothing was typed, or the
    /// field changed (the words stay and the text goes in at the cursor).
    func handOver() -> DictationEditTracker.Context? {
        guard let typing else { return nil }
        self.typing = nil
        let typed = !typing.typed.isEmpty
        guard typing.takeBack(), typed else {
            if typed { KeyboardProbeLog.shared.write("live: field changed, the dictation goes in at the cursor") }
            return nil
        }
        didEdit()
        let after = target.documentContextAfterInput
        return DictationEditTracker.Context(
            before: typing.base,
            after: after,
            hasText: !(typing.base ?? "").isEmpty || !(after ?? "").isEmpty
        )
    }

    /// Cancelled, failed, nothing heard, or the keyboard hiding: the live words come out.
    func cancel() {
        guard let typing else { return }
        self.typing = nil
        let typed = !typing.typed.isEmpty
        if typing.takeBack() {
            if typed { didEdit() }
        } else {
            KeyboardProbeLog.shared.write("live: field changed, typed words left as they are")
        }
    }
}
