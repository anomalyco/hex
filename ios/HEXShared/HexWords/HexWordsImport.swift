import Foundation

/// The copy of both lists from before an import, kept in the App Group until the person edits
/// either list or imports again, so Undo survives a restart. It holds the lists, never the file.
nonisolated struct HexWordsUndo: Codable, Equatable, Sendable {
    var before: HexWordsLists
    var after: HexWordsLists
    /// The sheet turned Custom vocabulary on; Undo turns it off again.
    var turnedOnCustomVocabulary: Bool
}

/// Adds a file that was read to the iPhone's lists, and puts them back.
nonisolated struct HexWordsImport {
    private static let undoKey = "hexWords.undo"
    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    private var vocabulary: CustomVocabularySettings { CustomVocabularySettings(defaults: defaults) }
    private var replacements: WordReplacementsSettings { WordReplacementsSettings(defaults: defaults) }

    var lists: HexWordsLists {
        HexWordsLists(vocabulary: vocabulary.terms, replacements: replacements.entries)
    }

    /// What Add would do with these choices, without changing anything.
    func preview(
        _ file: HexWordsFile,
        parts: HexWordsMerge.Parts = HexWordsMerge.Parts(),
        useTheirs: Set<String> = [],
        limits: HexWordsMerge.Limits = HexWordsMerge.Limits()
    ) -> HexWordsMergeResult {
        HexWordsMerge.phone(before: lists, incoming: file, limits: limits, parts: parts, useTheirs: useTheirs)
    }

    /// Add: merges into both lists, turns Custom vocabulary on when the person asked the sheet
    /// to, and keeps what Undo needs. Nothing else changes.
    @discardableResult
    func add(
        _ file: HexWordsFile,
        parts: HexWordsMerge.Parts = HexWordsMerge.Parts(),
        useTheirs: Set<String> = [],
        turnOnCustomVocabulary: Bool = false,
        limits: HexWordsMerge.Limits = HexWordsMerge.Limits()
    ) -> HexWordsMergeResult {
        let before = lists
        let result = HexWordsMerge.phone(before: before, incoming: file, limits: limits, parts: parts, useTheirs: useTheirs)
        vocabulary.terms = result.after.vocabulary
        replacements.entries = result.after.replacements
        let turnsOn = turnOnCustomVocabulary && !vocabulary.isEnabled
        if turnsOn { vocabulary.isEnabled = true }
        let undo = HexWordsUndo(before: before, after: lists, turnedOnCustomVocabulary: turnsOn)
        defaults?.set(try? JSONEncoder().encode(undo), forKey: Self.undoKey)
        return result
    }

    /// The last import's copy, while both lists are still as that import left them.
    var availableUndo: HexWordsUndo? {
        guard let data = defaults?.data(forKey: Self.undoKey),
              let undo = try? JSONDecoder().decode(HexWordsUndo.self, from: data) else { return nil }
        return undo.after == lists ? undo : nil
    }

    /// Puts both lists (and a switch the sheet turned on) back exactly. False when the lists
    /// changed since, which ends Undo.
    @discardableResult
    func undo() -> Bool {
        defer { defaults?.removeObject(forKey: Self.undoKey) }
        guard let undo = availableUndo else { return false }
        vocabulary.terms = undo.before.vocabulary
        replacements.entries = undo.before.replacements
        if undo.turnedOnCustomVocabulary { vocabulary.isEnabled = false }
        return true
    }
}
