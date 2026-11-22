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
    /// The words that went out with a send, as heard and as typed, after the field before them.
    struct Sent {
        let heard: String
        let typed: String
        let before: String?
    }

    /// The field just read empty with the words in it: hold the audio from here.
    var willSend: () -> Void = {}
    /// `sendConfirmation` later: still empty, a send (start afresh from the held audio), or nil
    /// when the field shows the words again (a host reporting late; the audio goes on as before).
    var didSettleSend: (Sent?) -> Void = { _ in }
    /// How long the field must stay empty to be a send: a host that reports late shows the
    /// words again well within it.
    static let sendConfirmation: TimeInterval = 0.3
    private var heard = ""
    private var confirming = false

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
        guard let typing, !typing.stopped, !confirming, !checkSent() else { return }
        heard = transcript.text
        typing.show(transcript.text)
        if typing.stopped {
            KeyboardProbeLog.shared.write("live: the field changed, stopped typing")
        }
        didEdit()
    }

    /// The app changed the field: emptied while it ended with the words, they were sent. They
    /// stay sent (nothing is taken back or inserted again) and the dictation starts afresh from
    /// the audio after the moment the field first read empty.
    @discardableResult
    func checkSent() -> Bool {
        guard !confirming, let typing, typing.wasSent else { return false }
        confirming = true
        willSend()
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.sendConfirmation) { [weak self] in self?.settleSend() }
        return true
    }

    private func settleSend() {
        guard confirming else { return }
        confirming = false
        guard let typing, typing.wasSent else {
            didSettleSend(nil)
            return
        }
        self.typing = nil
        KeyboardProbeLog.shared.write("live: the field was emptied with the words in it (a send): starting afresh")
        didSettleSend(Sent(heard: heard, typed: typing.typed, before: typing.base))
        didEdit()
    }

    /// The dictation's own text is about to go in: the live words come out first, and the field
    /// as it was before them is returned for its spacing. Nil when nothing was typed, or the
    /// field changed (the words stay and the text goes in at the cursor).
    func handOver() -> DictationEditTracker.Context? {
        guard let typing = typing ?? resumeLeftover() else { return nil }
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

    /// The words an earlier keyboard left when it hid (`LiveDictationSettings.leftover`), for
    /// the held text that replaces them, within the time it is offered.
    private func resumeLeftover() -> LiveFieldTyping? {
        let settings = LiveDictationSettings()
        guard let left = settings.leftover else { return nil }
        settings.leftover = nil
        guard Date().timeIntervalSince1970 - left.at < KeyboardSnapshot.resultOfferWindow else { return nil }
        let resumed = LiveFieldTyping(resuming: left.text, in: target)
        KeyboardProbeLog.shared.write("live: words left by a hidden keyboard \(resumed == nil ? "no longer in the field" : "replaced")")
        return resumed
    }

    /// The keyboard hides mid-dictation: the words come out where the app still takes edits, and
    /// are noted for the held text to replace where it doesn't (one gone to the background).
    func keyboardWillHide() {
        if let typing, !typing.stopped, !typing.typed.isEmpty {
            LiveDictationSettings().leftover = (typing.typed, Date().timeIntervalSince1970)
        }
        cancel()
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
