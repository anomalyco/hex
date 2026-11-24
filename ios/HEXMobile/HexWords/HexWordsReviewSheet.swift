import SwiftUI

extension Notification.Name {
    /// Custom vocabulary or Word replacements changed outside their editors: an import or Undo.
    static let hexWordsListsDidChange = Notification.Name("anomaly.hex.words.listsDidChange")
}

/// The review sheet for a Hex words file: what it adds, what is already there, each clash
/// (Keep mine by default), what can't be used here, what the file skipped, and a switch per
/// part. Nothing changes until Add. Every string from the file is shown verbatim (no Markdown,
/// no links), shortened, in its own direction-isolated run.
struct HexWordsReviewSheet: View {
    let result: HexWordsReadResult
    let onClose: () -> Void

    @Environment(\.dismiss) private var dismiss
    @State private var parts = HexWordsMerge.Parts()
    @State private var useTheirs: Set<String> = []
    @State private var turnsOnVocabulary = false
    @State private var added: HexWordsMergeResult?
    @State private var undone = false
    @State private var closed = false

    private let customVocabularyWasOn = CustomVocabularySettings().isEnabled
    private static let examples = 3
    private static let shownEntries = 8

    var body: some View {
        NavigationStack {
            Group {
                switch result {
                case .refused(let reason):
                    message(Self.sentence(for: reason))
                case .read(let file):
                    if let added {
                        message(undone ? String(localized: "Your lists are back as they were") : Self.confirmation(added.summary))
                    } else {
                        review(file)
                    }
                }
            }
            .background(HexTheme.bgDeep.ignoresSafeArea())
            .navigationTitle(String(localized: "Words from a file"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(HexTheme.bgDeep, for: .navigationBar)
            .toolbar { toolbar }
        }
        .onDisappear(perform: close)
    }

    private var file: HexWordsFile? {
        if case .read(let file) = result { file } else { nil }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        if let file {
            if added == nil {
                ToolbarItem(placement: .cancellationAction) {
                    Button(String(localized: "Cancel")) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(String(localized: "Add")) { add(file) }
                        .fontWeight(.semibold)
                        .disabled(!customVocabularyWasOn && !turnsOnVocabulary)
                }
            } else {
                if !undone {
                    ToolbarItem(placement: .cancellationAction) {
                        Button(String(localized: "Undo")) {
                            undone = HexWordsImport().undo()
                            NotificationCenter.default.post(name: .hexWordsListsDidChange, object: nil)
                        }
                    }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(String(localized: "Done")) { dismiss() }
                        .fontWeight(.semibold)
                }
            }
        } else {
            ToolbarItem(placement: .confirmationAction) {
                Button(String(localized: "Close")) { dismiss() }
            }
        }
    }

    private func add(_ file: HexWordsFile) {
        added = HexWordsImport().add(
            file,
            parts: parts,
            useTheirs: useTheirs,
            turnOnCustomVocabulary: turnsOnVocabulary
        )
        NotificationCenter.default.post(name: .hexWordsListsDidChange, object: nil)
        close()
    }

    /// Deletes the file's copies once: after Add, or when the sheet goes (Cancel, a refusal,
    /// a swipe down).
    private func close() {
        guard !closed else { return }
        closed = true
        onClose()
    }

    private func message(_ text: String) -> some View {
        VStack {
            Text(text)
                .font(.system(size: 17))
                .foregroundStyle(HexTheme.textBase)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    // MARK: Review

    private func review(_ file: HexWordsFile) -> some View {
        let preview = HexWordsImport().preview(file, parts: parts, useTheirs: useTheirs)
        let byReason = Dictionary(grouping: preview.notApplied, by: \.reason)
        let skipped = Dictionary(grouping: file.skipped, by: \.reason)
        return ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                header(file)

                if !customVocabularyWasOn {
                    SettingsCard {
                        SettingsToggleRow(
                            title: String(localized: "Turn on Custom vocabulary"),
                            detail: String(localized: "Words from this file go into Custom vocabulary and Word replacements"),
                            isOn: $turnsOnVocabulary
                        )
                    }
                }

                SettingsCard {
                    SettingsToggleRow(
                        title: String(localized: "Vocabulary"),
                        detail: Self.counts(new: preview.summary.addedTerms, there: preview.summary.alreadyHaveTerms),
                        isOn: $parts.vocabulary
                    )
                    entries(preview.after.vocabulary.suffix(preview.summary.addedTerms).map { (nil, $0) })
                }

                SettingsCard {
                    SettingsToggleRow(
                        title: String(localized: "Replacements"),
                        detail: Self.counts(new: preview.summary.addedReplacements, there: preview.summary.alreadyHaveReplacements),
                        isOn: $parts.replacements
                    )
                    entries(preview.after.replacements.suffix(preview.summary.addedReplacements).map { ($0.heard, $0.write) })
                }

                if !preview.clashes.isEmpty {
                    SettingsSectionHeader(title: "Replaced differently here")
                    SettingsCard {
                        ForEach(Array(preview.clashes.enumerated()), id: \.element.id) { index, clash in
                            if index > 0 { SettingsDivider() }
                            clashRow(clash)
                        }
                    }
                }

                if !byReason.isEmpty {
                    SettingsSectionHeader(title: "Not usable on this iPhone")
                    SettingsCard {
                        ForEach(Array(HexWordsNotAppliedReason.allCases.filter { byReason[$0] != nil }.enumerated()), id: \.element) { index, reason in
                            if index > 0 { SettingsDivider() }
                            countRow(
                                Self.sentence(for: reason),
                                count: byReason[reason]?.count ?? 0,
                                examples: (byReason[reason] ?? []).prefix(Self.examples).compactMap { file.text(at: $0.path) }
                            )
                        }
                    }
                }

                if !skipped.isEmpty {
                    SettingsSectionHeader(title: "Skipped in the file")
                    SettingsCard {
                        ForEach(Array(HexWordsSkipReason.allCases.filter { skipped[$0] != nil }.enumerated()), id: \.element) { index, reason in
                            if index > 0 { SettingsDivider() }
                            countRow(Self.sentence(for: reason), count: skipped[reason]?.count ?? 0, examples: [])
                        }
                    }
                }

                if !customVocabularyWasOn, !turnsOnVocabulary {
                    Text(String(localized: "Turn on Custom vocabulary to add these words"))
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .padding(.horizontal, 16)
                }
            }
            .frame(maxWidth: 500)
            .padding(.horizontal, 20)
            .padding(.top, 8)
            .padding(.bottom, 28)
            .frame(maxWidth: .infinity)
        }
    }

    private func header(_ file: HexWordsFile) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            if let app = file.madeByApp {
                let claim = [app, file.madeByVersion].compactMap { $0 }.joined(separator: " ")
                Text(verbatim: String(localized: "The file says it's from \(HexWordsText.display(claim))"))
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
            }
            if file.isNewerVersion {
                Text(String(localized: "Made by a newer Hex. Parts this version doesn't know are left out."))
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, 16)
    }

    /// New entries: a term alone, or heard → write.
    @ViewBuilder
    private func entries(_ items: [(String?, String)]) -> some View {
        if !items.isEmpty {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.prefix(Self.shownEntries).enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        if let heard = item.0 {
                            Text(verbatim: HexWordsText.display(heard))
                                .foregroundStyle(HexTheme.textMuted)
                            Image(systemName: "arrow.right")
                                .font(.system(size: 10, weight: .semibold))
                                .foregroundStyle(HexTheme.textFaint)
                        }
                        Text(verbatim: HexWordsText.display(item.1))
                            .foregroundStyle(HexTheme.textBase)
                    }
                    .font(.system(size: 14))
                    .lineLimit(2)
                }
                if items.count > Self.shownEntries {
                    Text(String(localized: "and \(items.count - Self.shownEntries) more"))
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 12)
        }
    }

    private func clashRow(_ clash: HexWordsClash) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(verbatim: HexWordsText.display(clash.heard))
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textBase)
            labeled(String(localized: "Yours"), clash.yours)
            labeled(String(localized: "This file"), clash.theirs)
            SettingsSegments(
                items: [false, true],
                selected: useTheirs.contains(clash.path),
                label: \.clashChoice,
                onSelect: { theirs in
                    if theirs { useTheirs.insert(clash.path) } else { useTheirs.remove(clash.path) }
                }
            )
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private func labeled(_ label: String, _ value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(label)
                .foregroundStyle(HexTheme.textMuted)
                .frame(width: 72, alignment: .leading)
            Text(verbatim: HexWordsText.display(value))
                .foregroundStyle(HexTheme.textBase)
                .lineLimit(2)
        }
        .font(.system(size: 13))
    }

    private func countRow(_ sentence: String, count: Int, examples: [String]) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(sentence)
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textBase)
                    .fixedSize(horizontal: false, vertical: true)
                if !examples.isEmpty {
                    Text(verbatim: examples.map { HexWordsText.display($0) }.joined(separator: ", "))
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .lineLimit(2)
                }
            }
            Spacer(minLength: 8)
            Text(verbatim: count.formatted())
                .font(.system(size: 15).monospacedDigit())
                .foregroundStyle(HexTheme.textMuted)
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .frame(minHeight: 50)
    }

    // MARK: Words

    static func counts(new: Int, there: Int) -> String {
        String(localized: "\(new) new · \(there) already there")
    }

    /// One line: "Added 3 terms and 6 replacements · 2 already there · kept yours for 1".
    static func confirmation(_ summary: HexWordsSummary) -> String {
        let there = summary.alreadyHaveTerms + summary.alreadyHaveReplacements
        guard summary.changesAnything else {
            return String(localized: "No new words in that file")
        }
        var pieces = [String(localized: "Added \(summary.addedTerms) terms and \(summary.addedReplacements) replacements")]
        if there > 0 { pieces.append(String(localized: "\(there) already there")) }
        if summary.keptYours > 0 { pieces.append(String(localized: "kept yours for \(summary.keptYours)")) }
        if summary.changedReplacements > 0 { pieces.append(String(localized: "used this file's for \(summary.changedReplacements)")) }
        return pieces.joined(separator: " · ")
    }

    static func sentence(for refusal: HexWordsRefusal) -> String {
        switch refusal {
        case .tooLarge: String(localized: "This file is larger than 8 MB, so Hex didn't open it.")
        case .notUTF8: String(localized: "Hex can't read this file because it isn't UTF-8 text.")
        case .empty: String(localized: "This file is empty.")
        case .tooDeep: String(localized: "Hex can't read this file because it's nested too deeply.")
        case .duplicateKey: String(localized: "Hex can't read this file because a name in it appears twice.")
        case .notJSON: String(localized: "Hex can't read this file because it isn't valid JSON.")
        case .notAnObject, .notHexWords: String(localized: "This isn't a Hex words file.")
        case .badVersion: String(localized: "Hex can't read this file because its version is missing or wrong.")
        }
    }

    static func sentence(for reason: HexWordsNotAppliedReason) -> String {
        switch reason {
        case .tooShortForThisDevice: String(localized: "Terms need two or more letters or digits here")
        case .deletionNotSupported: String(localized: "This iPhone can't delete words yet")
        case .overDeviceLimit: String(localized: "Your list is full")
        case .duplicate: String(localized: "The file already replaces these words")
        }
    }

    static func sentence(for reason: HexWordsSkipReason) -> String {
        switch reason {
        case .badPart: String(localized: "A list in the wrong shape")
        case .badEntry: String(localized: "Entries in the wrong shape")
        case .badField: String(localized: "Fields in the wrong shape")
        case .unknownField: String(localized: "Fields this version of Hex doesn't know")
        case .hiddenCharacters: String(localized: "Hidden characters")
        case .noLetters: String(localized: "No letters or digits")
        case .tooLong: String(localized: "Too long")
        case .tooManyWords: String(localized: "More than 8 words")
        case .duplicate: String(localized: "Repeated in the file")
        case .sameAsTerm: String(localized: "A spelling the same as its term")
        case .overLimit: String(localized: "Past the file format's limits")
        }
    }
}

private extension Bool {
    var clashChoice: String {
        self ? String(localized: "Use this one") : String(localized: "Keep mine")
    }
}

extension HexWordsFile {
    /// The text at a path the merge names (`vocabulary[2]`, `replacements[0]`,
    /// `vocabulary[0].soundsLike[1]`), for examples.
    func text(at path: String) -> String? {
        let numbers = path.split(whereSeparator: { !$0.isNumber }).compactMap { Int($0) }
        if path.hasPrefix("replacements["), let index = numbers.first, replacements.indices.contains(index) {
            return replacements[index].heard
        }
        guard path.hasPrefix("vocabulary["), let index = numbers.first, vocabulary.indices.contains(index) else { return nil }
        if numbers.count > 1 {
            let spellings = vocabulary[index].soundsLike
            return spellings.indices.contains(numbers[1]) ? spellings[numbers[1]] : nil
        }
        return vocabulary[index].term
    }
}
