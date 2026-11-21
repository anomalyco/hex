import CoreGraphics
import Foundation
import UIKit

/// Learn how I type (on by default): a private, on-device record of how you type and dictate,
/// kept in the App Group so the app can show it and the keyboard can personalize itself.
///
/// It is deliberately not a keystroke log or a copy of what you write. Taps are folded into
/// per-key averages (where your finger lands relative to each key), so no sequence of letters
/// is ever stored, and corrections are word-level pairs: a word you erased and retyped,
/// autocorrect fixes, fixes you undid, suggestions you picked, and words you fixed in a
/// dictation, with at most a word of context on each side. Nothing leaves the iPhone.
struct TypingJournalSettings {
    private enum Key {
        static let enabled = "experiments.typingJournal"
        static let learnsPerLanguage = "experiments.learnPerLanguage"
        static let disabledDictationFixes = "typingJournal.disabledDictationFixes"
        static let erasedAt = "typingJournal.erasedAt"
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Learn how I type: on until you turn it off. Only a switch you turned off is saved as
    /// off, so an iPhone that never touched it learns.
    var isEnabled: Bool {
        get { defaults.map { $0.object(forKey: Key.enabled) as? Bool ?? true } ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.enabled) }
    }

    /// Experiment (Learn each language separately): learned words and fixes stay with the
    /// language you typed or dictated them in. Off by default, like every experiment: one
    /// shared list and one shared learning, exactly as before.
    var learnsPerLanguage: Bool {
        get { defaults?.bool(forKey: Key.learnsPerLanguage) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Key.learnsPerLanguage) }
    }

    /// Learned dictation fixes you switched off in Typing insights (their ids).
    var disabledDictationFixes: Set<String> {
        get { Set(defaults?.stringArray(forKey: Key.disabledDictationFixes) ?? []) }
        nonmutating set { defaults?.set(newValue.sorted(), forKey: Key.disabledDictationFixes) }
    }

    /// The words autocorrect learned to leave alone (`LearnedWordsStore`), the shared list
    /// and every language's own.
    func eraseLearnedWords() {
        guard let defaults else { return }
        defaults.removeObject(forKey: LearnedWordsStore.defaultsKey)
        for key in defaults.dictionaryRepresentation().keys where LearnedWordsStore.isScopedKey(key) {
            defaults.removeObject(forKey: key)
        }
    }

    /// When Delete what Hex learned last ran, so another process drops what it still holds.
    var erasedAt: TimeInterval {
        get { defaults?.double(forKey: Key.erasedAt) ?? 0 }
        nonmutating set { defaults?.set(newValue, forKey: Key.erasedAt) }
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
        /// You tapped what you typed instead of the autocorrection space would apply.
        case keptTyped
    }

    var kind: Kind
    var typed: String
    var final: String
    var at: TimeInterval
    /// The keyboard language it was typed in (`KeyboardLanguage.rawValue`); nil for a
    /// journal written before corrections carried one. It marks the language either way
    /// (`Learn each language separately`).
    var language: String? = nil
}

nonisolated struct TypingJournalSnapshot: Codable, Equatable, Sendable {
    static let correctionLimit = 2000
    static let dictationFixLimit = 1000
    static let dictationOutcomeLimit = 500

    var keys: [String: TypingJournalKeyStats] = [:]
    var corrections: [TypingJournalCorrection] = []
    /// Word-level (heard → meant) pairs from dictations, with counts.
    var dictationFixes: [TypingJournalDictationFix] = []
    /// What happened to each dictation (kept, edited, …), without its text.
    var dictationOutcomes: [TypingJournalDictationOutcome] = []

    var tapCount: Int { keys.values.reduce(0) { $0 + $1.count } }

    var isEmpty: Bool {
        tapCount == 0 && corrections.isEmpty && dictationFixes.isEmpty && dictationOutcomes.isEmpty
    }
}

extension TypingJournalSnapshot {
    private enum CodingKeys: String, CodingKey {
        case keys, corrections, dictationFixes, dictationOutcomes
    }

    /// Journals written before dictation learning have only keys and corrections.
    nonisolated init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        keys = try container.decodeIfPresent([String: TypingJournalKeyStats].self, forKey: .keys) ?? [:]
        corrections = try container.decodeIfPresent([TypingJournalCorrection].self, forKey: .corrections) ?? []
        dictationFixes = try container.decodeIfPresent([TypingJournalDictationFix].self, forKey: .dictationFixes) ?? []
        dictationOutcomes = try container.decodeIfPresent([TypingJournalDictationOutcome].self, forKey: .dictationOutcomes) ?? []
    }
}

@MainActor
final class TypingJournal {
    static let fileName = "typing-journal.json"
    private static var shared: TypingJournal?

    /// The one journal this process writes while Learn how I type is on, shared by the
    /// keys and dictation so they never overwrite each other; nil while it is off.
    static func sharedIfEnabled(settings: TypingJournalSettings = TypingJournalSettings()) -> TypingJournal? {
        guard settings.isEnabled else {
            shared?.save()
            shared = nil
            return nil
        }
        if let shared { return shared }
        let journal = TypingJournal(settings: settings)
        shared = journal
        return journal
    }

    private let url: URL?
    private let settings: TypingJournalSettings
    private var snapshot: TypingJournalSnapshot
    /// Taps not yet written, added to whatever is on disk when saving.
    private var pendingKeys: [String: TypingJournalKeyStats] = [:]
    private var unsavedTaps = 0
    private var erasedWord: String?
    private var loadedAt: TimeInterval
    private var fileDate: Date?
    private var learning: (revision: Int, value: TypingLearning)?
    /// `typingLearning(for:)`, one language at a time, for Learn each language separately.
    private var languageLearning: [String: (revision: Int, value: TypingLearning)] = [:]
    /// Bumped whenever corrections or dictation fixes change.
    private(set) var revision = 0

    init(
        directory: URL? = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup
        ),
        settings: TypingJournalSettings = TypingJournalSettings()
    ) {
        url = directory?.appendingPathComponent(Self.fileName)
        self.settings = settings
        snapshot = Self.load(from: url)
        loadedAt = Date().timeIntervalSince1970
        fileDate = url.flatMap(Self.modificationDate)
    }

    static func load(from url: URL?) -> TypingJournalSnapshot {
        guard let url, let data = try? Data(contentsOf: url),
              let snapshot = try? JSONDecoder().decode(TypingJournalSnapshot.self, from: data)
        else { return TypingJournalSnapshot() }
        return snapshot
    }

    /// Erases everything Learn how I type gathered: the file, the switched-off fixes, the words
    /// autocorrect learned to keep, and (through `erasedAt`) whatever another process still
    /// holds in memory.
    static func delete(
        in directory: URL? = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup
        ),
        settings: TypingJournalSettings = TypingJournalSettings()
    ) {
        settings.erasedAt = Date().timeIntervalSince1970
        settings.disabledDictationFixes = []
        settings.eraseLearnedWords()
        if let url = directory?.appendingPathComponent(fileName) {
            try? FileManager.default.removeItem(at: url)
        }
        shared?.refreshFromDisk()
    }

    /// The learned dictation fixes on disk, for the app's dictation; empty while off.
    static func storedDictationCorrections(
        settings: TypingJournalSettings = TypingJournalSettings(),
        directory: URL? = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup
        ),
        language: String? = nil
    ) -> [DictationCorrection] {
        guard settings.isEnabled else { return [] }
        return load(from: directory?.appendingPathComponent(fileName)).dictationCorrections(
            disabled: settings.disabledDictationFixes,
            language: settings.learnsPerLanguage ? LanguageLearning.dictationCode(language) : nil
        )
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

    var current: TypingJournalSnapshot {
        var merged = snapshot
        Self.add(pendingKeys, to: &merged)
        return merged
    }

    /// Typo fixes and rejected autocorrections for the keys, derived once per change.
    var typingLearning: TypingLearning {
        refreshFromDisk()
        if let learning, learning.revision == revision { return learning.value }
        let value = TypingLearning.derive(from: snapshot.corrections)
        learning = (revision, value)
        return value
    }

    /// Typo fixes and rejected autocorrections for `language`'s keys: with Learn each
    /// language separately on, only the corrections typed in that language and the ones never
    /// tagged; off, all of them.
    func typingLearning(for language: KeyboardLanguage?) -> TypingLearning {
        guard settings.learnsPerLanguage, let language else { return typingLearning }
        refreshFromDisk()
        let code = language.rawValue
        if let cached = languageLearning[code], cached.revision == revision { return cached.value }
        let value = TypingLearning.derive(from: snapshot.corrections, language: code)
        languageLearning[code] = (revision, value)
        return value
    }

    /// The learned dictation fixes, minus the ones switched off in Typing insights.
    func dictationCorrections(language: String? = nil) -> [DictationCorrection] {
        refreshFromDisk()
        return snapshot.dictationCorrections(
            disabled: settings.disabledDictationFixes,
            language: settings.learnsPerLanguage ? LanguageLearning.dictationCode(language) : nil
        )
    }

    func recordTap(on key: KeyboardKey, at point: CGPoint) {
        guard let character = key.character?.lowercased(), character.count == 1,
              key.frame.width > 0, key.frame.height > 0 else { return }
        dropPendingIfErased()
        var stats = pendingKeys[character] ?? TypingJournalKeyStats()
        stats.count += 1
        stats.dx += Double((point.x - key.frame.midX) / key.frame.width)
        stats.dy += Double((point.y - key.frame.midY) / key.frame.height)
        pendingKeys[character] = stats
        unsavedTaps += 1
        if unsavedTaps >= 40 { save() }
    }

    /// Called before a backspace removes part of the word being typed.
    func willErase(word: String?) {
        guard erasedWord == nil, let word, !word.isEmpty else { return }
        erasedWord = word
    }

    /// Called when a word is finished; a retyped word close to the erased one is a fix.
    func didFinish(word: String?, language: String? = nil) {
        defer { erasedWord = nil }
        guard let erased = erasedWord, let word, !word.isEmpty,
              erased.lowercased() != word.lowercased(),
              Self.distance(erased.lowercased(), word.lowercased()) <= max(2, word.count / 2)
        else { return }
        record(.retyped, typed: erased, final: word, language: language)
    }

    func record(_ kind: TypingJournalCorrection.Kind, typed: String, final: String, language: String? = nil) {
        guard !typed.isEmpty, !final.isEmpty, typed != final else { return }
        if kind != .retyped { erasedWord = nil }
        refreshFromDisk()
        snapshot.corrections.append(
            TypingJournalCorrection(
                kind: kind, typed: typed, final: final, at: Date().timeIntervalSince1970, language: language
            )
        )
        if snapshot.corrections.count > TypingJournalSnapshot.correctionLimit {
            snapshot.corrections.removeFirst(snapshot.corrections.count - TypingJournalSnapshot.correctionLimit)
        }
        revision += 1
        save()
    }

    /// Adds what happened to one dictation: its outcome, and its word pairs with counts,
    /// kept under the language it was dictated in (`LanguageLearning.dictationCode`).
    func recordDictation(_ result: DictationEditResult, language: String? = nil) {
        refreshFromDisk()
        let now = Date().timeIntervalSince1970
        snapshot.dictationOutcomes.append(TypingJournalDictationOutcome(
            outcome: result.outcome,
            words: result.wordCount,
            changedWords: result.changedWordCount,
            at: now
        ))
        if snapshot.dictationOutcomes.count > TypingJournalSnapshot.dictationOutcomeLimit {
            snapshot.dictationOutcomes.removeFirst(
                snapshot.dictationOutcomes.count - TypingJournalSnapshot.dictationOutcomeLimit
            )
        }
        snapshot.dictationFixes = TypingJournalDictationFix.merging(
            result, into: snapshot.dictationFixes, at: now, language: LanguageLearning.dictationCode(language)
        )
        revision += 1
        save()
    }

    func save() {
        refreshFromDisk()
        Self.add(pendingKeys, to: &snapshot)
        pendingKeys = [:]
        unsavedTaps = 0
        guard let url, let data = try? JSONEncoder().encode(snapshot) else { return }
        try? data.write(to: url, options: .atomic)
        fileDate = Self.modificationDate(url)
    }

    /// Picks up what another process (the keyboard, or the app's keyboard preview) wrote,
    /// and forgets everything when Delete what Hex learned ran since this journal loaded.
    private func refreshFromDisk() {
        dropPendingIfErased()
        guard let url else { return }
        let date = Self.modificationDate(url)
        guard date != fileDate else { return }
        snapshot = Self.load(from: url)
        fileDate = date
        revision += 1
    }

    private func dropPendingIfErased() {
        let erasedAt = settings.erasedAt
        guard erasedAt > loadedAt else { return }
        snapshot = TypingJournalSnapshot()
        pendingKeys = [:]
        unsavedTaps = 0
        erasedWord = nil
        loadedAt = erasedAt
        fileDate = nil
        revision += 1
    }

    private static func add(_ pending: [String: TypingJournalKeyStats], to snapshot: inout TypingJournalSnapshot) {
        for (key, stats) in pending {
            var merged = snapshot.keys[key] ?? TypingJournalKeyStats()
            merged.count += stats.count
            merged.dx += stats.dx
            merged.dy += stats.dy
            snapshot.keys[key] = merged
        }
    }

    private static func modificationDate(_ url: URL) -> Date? {
        (try? FileManager.default.attributesOfItem(atPath: url.path))?[.modificationDate] as? Date
    }

    static func distance(_ a: String, _ b: String) -> Int {
        DictationText.distance(a, b)
    }
}