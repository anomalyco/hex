import Combine
import Foundation

/// The words heard so far while a dictation records, shown over the keys while it does:
/// `settled` words the recogniser won't change again, then the `tentative` tail it still may.
nonisolated struct LiveTranscript: Equatable, Sendable {
    var settled = ""
    var tentative = ""

    init(settled: String = "", tentative: String = "") {
        self.settled = settled
        self.tentative = tentative
    }

    /// Both parts as one line, one space between them unless either already has one.
    var text: String {
        settled + tentativeJoined
    }

    /// The tentative words as they follow the settled ones.
    var tentativeJoined: String {
        guard let last = settled.last, let first = tentative.first,
              !last.isWhitespace, !first.isWhitespace,
              !LiveTranscript.joinsWithoutSpace(last, first) else { return tentative }
        return " " + tentative
    }

    var isEmpty: Bool {
        settled.allSatisfy(\.isWhitespace) && tentative.allSatisfy(\.isWhitespace)
    }

    /// Chinese and Japanese run on without spaces; a mark never takes one before it.
    static func joinsWithoutSpace(_ last: Character, _ first: Character) -> Bool {
        if first.isPunctuation { return true }
        guard let a = last.unicodeScalars.first?.value, let b = first.unicodeScalars.first?.value else { return false }
        func written(_ v: UInt32) -> Bool {
            (0x3041...0x30FF).contains(v) || (0x3400...0x4DBF).contains(v)
                || (0x4E00...0x9FFF).contains(v) || (0xF900...0xFAFF).contains(v) || (0x3000...0x303F).contains(v)
        }
        return written(a) || written(b)
    }
}

/// A recogniser's results as one transcript: a final result settles its words and the latest
/// volatile one is the tentative tail (Apple's `SpeechTranscriber` replaces its volatile
/// result each time and never revisits a final one).
nonisolated struct LiveTranscriptAccumulator {
    private var finals: [String] = []
    private(set) var transcript = LiveTranscript()

    @discardableResult
    mutating func apply(_ text: String, isFinal: Bool) -> LiveTranscript {
        if isFinal {
            finals.append(text)
            transcript = LiveTranscript(settled: Self.joined(finals), tentative: "")
        } else {
            transcript.tentative = text.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        return transcript
    }

    /// Final results back to back, as Apple writes them, trimmed at both ends.
    static func joined(_ segments: [String]) -> String {
        var text = ""
        for segment in segments {
            let piece = segment.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !piece.isEmpty else { continue }
            text = LiveTranscript(settled: text, tentative: piece).text
        }
        return text
    }
}

/// Where live words come from while the keyboard records: Apple's speech recognition fed as
/// the audio arrives today, the keyboard's speech model later. `append` takes the capture's
/// 16-bit mono chunks; `stop` frees it at once and reports nothing more.
@MainActor
protocol LiveTranscriptSource: AnyObject {
    var onUpdate: ((LiveTranscript) -> Void)? { get set }
    /// The source gave up (no model, no memory, an error): the preview goes and the dictation
    /// goes on as without it.
    var onFailure: ((String) -> Void)? { get set }
    func append(_ pcm: Data)
    func stop()
}

/// What the keyboard shows while it records: nil while no live source runs (the keys stay as
/// they are), else the words so far. The preview reads it; changing it never refreshes the
/// rest of the keyboard.
@MainActor
final class LiveDictationText: ObservableObject {
    @Published private(set) var transcript: LiveTranscript?
    /// Settings › Dictation › Show words as you speak, as the dictation started.
    private(set) var showsPreview = true
    private(set) var look: LiveDictationSettings.Look = .dim

    /// A source started: the preview shows, empty until the first words.
    func begin(showsPreview: Bool, look: LiveDictationSettings.Look) {
        self.showsPreview = showsPreview
        self.look = look
        transcript = LiveTranscript()
    }

    func update(_ next: LiveTranscript) {
        guard transcript != nil, next != transcript else { return }
        transcript = next
    }

    /// The source stopped and kept its words (they show until the dictation goes in).
    func settle() {
        guard let current = transcript, !current.tentative.isEmpty else { return }
        transcript = LiveTranscript(settled: current.text, tentative: "")
    }

    func end() {
        if transcript != nil { transcript = nil }
    }
}

/// Settings › Dictation's two switches for what happens while you speak, in the App Group so
/// the keyboard reads them: words over the keys (on by default) and words typed into the
/// field (off by default).
nonisolated struct LiveDictationSettings {
    static let previewKey = "settings.liveDictationPreview"
    static let typingKey = "settings.liveDictationTyping"
    /// Which look the preview takes, while Ryan chooses (`dim` or `hide`).
    static let lookKey = "debug.liveDictationLook"

    enum Look: String {
        /// The keys stay, faint, with the words over them.
        case dim
        /// The keys go and the words have their space.
        case hide
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var showsPreview: Bool {
        get { defaults?.object(forKey: Self.previewKey) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Self.previewKey) }
    }

    var typesIntoField: Bool {
        get { defaults?.object(forKey: Self.typingKey) as? Bool ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.typingKey) }
    }

    var look: Look {
        get { defaults?.string(forKey: Self.lookKey).flatMap(Look.init(rawValue:)) ?? .dim }
        nonmutating set { defaults?.set(newValue.rawValue, forKey: Self.lookKey) }
    }

    /// Either switch needs live words while recording.
    var wantsLiveWords: Bool { showsPreview || typesIntoField }
}
