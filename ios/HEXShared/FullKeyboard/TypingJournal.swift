import CoreGraphics
import Foundation
import UIKit

/// Experiment (Learn how I type): a private, on-device record of how you type, kept in the
/// App Group so the app can show it and later passes can personalize the keyboard.
///
/// It is deliberately not a keystroke log. Taps are folded into per-key averages (where
/// your finger lands relative to each key), so no sequence of letters is ever stored, and
/// corrections are single-word pairs: a word you erased and retyped, autocorrect fixes,
/// fixes you undid, and suggestions you picked. Nothing leaves the iPhone.
struct TypingJournalSettings {
    private static let key = "experiments.typingJournal"
    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.bool(forKey: Self.key) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.key) }
    }
}

nonisolated struct TypingJournalKeyStats: Codable, Equatable, Sendable {
    var count = 0
    /// Sums of the tap's offset from the keycap centre, in keycap widths and heights.
    var dx = 0.0
    var dy = 0.0

    var meanOffset: CGVector { count == 0 ? .zero : CGVector(dx: dx / Double(count), dy: dy / Double(count)) }
}

nonisolated struct TypingJournalCorrection: Codable, Equatable, Sendable {
    enum Kind: String, Codable, Sendable {
        /// You erased a word and typed another.
        case retyped
        /// Autocorrect replaced it on space.
        case autocorrected
        /// You undid autocorrect with one backspace.
        case undone
        /// You tapped a suggestion.
        case picked
    }

    var kind: Kind
    var typed: String
    var final: String
    var at: TimeInterval
}

nonisolated struct TypingJournalSnapshot: Codable, Equatable, Sendable {
    static let correctionLimit = 2000

    var keys: [String: TypingJournalKeyStats] = [:]
    var corrections: [TypingJournalCorrection] = []

    var tapCount: Int { keys.values.reduce(0) { $0 + $1.count } }
}

@MainActor
final class TypingJournal {
    static let fileName = "typing-journal.json"

    private let url: URL?
    private var snapshot: TypingJournalSnapshot
    private var unsavedTaps = 0
    private var erasedWord: String?

    init(directory: URL? = FileManager.default.containerURL(
        forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup
    )) {
        url = directory?.appendingPathComponent(Self.fileName)
        snapshot = Self.load(from: url)
    }

    static func load(from url: URL?) -> TypingJournalSnapshot {
        guard let url, let data = try? Data(contentsOf: url),
              let snapshot = try? JSONDecoder().decode(TypingJournalSnapshot.self, from: data)
        else { return TypingJournalSnapshot() }
        return snapshot
    }

    static func delete(in directory: URL? = FileManager.default.containerURL(
        forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup
    )) {
        guard let url = directory?.appendingPathComponent(fileName) else { return }
        try? FileManager.default.removeItem(at: url)
    }

    /// Fields a journal may learn from: plain text that allows autocorrect, never
    /// addresses, codes, or anything credential-shaped.
    static func allows(field: KeyboardFieldKind, target: KeyboardTextTarget) -> Bool {
        guard field == .text, target.autocorrectionType != .no else { return false }
        let blocked: Set<UITextContentType> = [
            .password, .newPassword, .oneTimeCode, .username, .emailAddress, .URL,
            .creditCardNumber, .telephoneNumber,
        ]
        if let type = target.textContentType, blocked.contains(type) { return false }
        return true
    }

    var current: TypingJournalSnapshot { snapshot }

    func recordTap(on key: KeyboardKey, at point: CGPoint) {
        guard let character = key.character?.lowercased(), character.count == 1,
              key.frame.width > 0, key.frame.height > 0 else { return }
        var stats = snapshot.keys[character] ?? TypingJournalKeyStats()
        stats.count += 1
        stats.dx += Double((point.x - key.frame.midX) / key.frame.width)
        stats.dy += Double((point.y - key.frame.midY) / key.frame.height)
        snapshot.keys[character] = stats
        unsavedTaps += 1
        if unsavedTaps >= 40 { save() }
    }

    /// Called before a backspace removes part of the word being typed.
    func willErase(word: String?) {
        guard erasedWord == nil, let word, !word.isEmpty else { return }
        erasedWord = word
    }

    /// Called when a word is finished; a retyped word close to the erased one is a fix.
    func didFinish(word: String?) {
        defer { erasedWord = nil }
        guard let erased = erasedWord, let word, !word.isEmpty,
              erased.lowercased() != word.lowercased(),
              Self.distance(erased.lowercased(), word.lowercased()) <= max(2, word.count / 2)
        else { return }
        record(.retyped, typed: erased, final: word)
    }

    func record(_ kind: TypingJournalCorrection.Kind, typed: String, final: String) {
        guard !typed.isEmpty, !final.isEmpty, typed != final else { return }
        if kind != .retyped { erasedWord = nil }
        snapshot.corrections.append(
            TypingJournalCorrection(kind: kind, typed: typed, final: final, at: Date().timeIntervalSince1970)
        )
        if snapshot.corrections.count > TypingJournalSnapshot.correctionLimit {
            snapshot.corrections.removeFirst(snapshot.corrections.count - TypingJournalSnapshot.correctionLimit)
        }
        save()
    }

    func save() {
        unsavedTaps = 0
        guard let url, let data = try? JSONEncoder().encode(snapshot) else { return }
        try? data.write(to: url, options: .atomic)
    }

    static func distance(_ a: String, _ b: String) -> Int {
        let a = Array(a), b = Array(b)
        if a.isEmpty { return b.count }
        if b.isEmpty { return a.count }
        var previous = Array(0...b.count)
        for i in 1...a.count {
            var row = [i] + Array(repeating: 0, count: b.count)
            for j in 1...b.count {
                row[j] = min(previous[j] + 1, row[j - 1] + 1, previous[j - 1] + (a[i - 1] == b[j - 1] ? 0 : 1))
            }
            previous = row
        }
        return previous[b.count]
    }
}
