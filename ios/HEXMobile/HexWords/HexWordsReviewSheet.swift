import SwiftUI

extension Notification.Name {
    /// Custom vocabulary or Word replacements changed outside their editors: an import or Undo.
    static let hexWordsListsDidChange = Notification.Name("anomaly.hex.words.listsDidChange")
}

/// The review sheet for a Hex words file: what it adds, what is already there, each clash
/// (Keep mine by default), what this iPhone can't use, what the file skipped, and a switch per
/// part. Nothing changes until Add. Every string from the file is shown verbatim (no Markdown,
/// no links), shortened, in its own direction-isolated run. Words as Android's sheet has them.
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
    private static let shownRows = 20
    private static let shownExamples = 3

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text(title)
                    .font(.system(size: 26, weight: .semibold))
                    .foregroundStyle(HexTheme.textBase)
                    .accessibilityAddTraits(.isHeader)
                    .frame(minHeight: 52, alignment: .leading)
                content
            }
            .frame(maxWidth: 520, alignment: .leading)
            .padding(.horizontal, 20)
            .padding(.top, 12)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity)
        }
        .background(HexTheme.bgDeep.ignoresSafeArea())
        .onDisappear(perform: close)
    }

    private var title: String {
        if let added, !undone, added.summary.changesAnything { return String(localized: "Words added") }
        return String(localized: "Words from a file")
    }

    @ViewBuilder
    private var content: some View {
        switch result {
        case .refused(let reason):
            paragraph(Self.sentence(for: reason))
            buttons(primary: String(localized: "Done")) { dismiss() }
        case .read(let file):
            if undone {
                paragraph(String(localized: "Your lists are back as they were."))
                buttons(primary: String(localized: "Done")) { dismiss() }
            } else if let added {
                paragraph(Self.confirmation(added.summary))
                buttons(primary: String(localized: "Done"), { dismiss() }, secondary: String(localized: "Undo")) {
                    undone = HexWordsImport().undo()
                    NotificationCenter.default.post(name: .hexWordsListsDidChange, object: nil)
                }
            } else {
                review(file)
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

    // MARK: Review

    @ViewBuilder
    private func review(_ file: HexWordsFile) -> some View {
        let store = HexWordsImport()
        let before = store.lists
        // Counts and rows show the whole file; what Add does follows the switches.
        let whole = store.preview(file, useTheirs: useTheirs)
        let merged = store.preview(file, parts: parts, useTheirs: useTheirs)
        let summary = whole.summary
        let claim = [file.madeByApp, file.madeByVersion].compactMap { $0 }.joined(separator: " ")

        if !claim.isEmpty {
            note(String(localized: "Made by \(HexWordsText.display(claim)), as the file says"))
        }
        if file.isNewerVersion {
            note(String(localized: "Made by a newer Hex. Anything this version doesn’t know is left out."))
        }
        if !customVocabularyWasOn {
            SettingsCard {
                SettingsToggleRow(
                    title: String(localized: "Turn on Custom vocabulary"),
                    detail: String(localized: "Off, the words are added but Hex doesn’t use them until you turn it on."),
                    isOn: $turnsOnVocabulary
                )
            }
        }
        if !file.vocabulary.isEmpty {
            SettingsCard {
                SettingsToggleRow(
                    title: String(localized: "Vocabulary"),
                    detail: Self.counts(new: summary.addedTerms, there: summary.alreadyHaveTerms, clashes: 0),
                    isOn: $parts.vocabulary
                )
                if parts.vocabulary {
                    rows(whole.after.vocabulary.dropFirst(before.vocabulary.count).map { HexWordsText.display($0) })
                }
            }
        }
        if !file.replacements.isEmpty || file.vocabulary.contains(where: { !$0.soundsLike.isEmpty }) {
            SettingsCard {
                SettingsToggleRow(
                    title: String(localized: "Word replacements"),
                    detail: Self.counts(new: summary.addedReplacements, there: summary.alreadyHaveReplacements, clashes: whole.clashes.count),
                    isOn: $parts.replacements
                )
                if parts.replacements {
                    rows(whole.after.replacements.dropFirst(before.replacements.count).map {
                        HexWordsText.display($0.heard) + " → " + HexWordsText.display($0.write)
                    })
                    ForEach(whole.clashes) { clash in
                        SettingsDivider()
                        clashRow(clash)
                    }
                }
            }
        }
        if !merged.notApplied.isEmpty {
            section(String(localized: "Not usable on this iPhone")) {
                let groups = HexWordsNotAppliedReason.allCases.compactMap { reason -> (HexWordsNotAppliedReason, [HexWordsNotApplied])? in
                    let items = merged.notApplied.filter { $0.reason == reason }
                    return items.isEmpty ? nil : (reason, items)
                }
                ForEach(Array(groups.enumerated()), id: \.offset) { index, group in
                    if index > 0 { SettingsDivider() }
                    let texts = group.1.compactMap { file.text(at: $0.path) }.map { HexWordsText.display($0) }
                    let examples = texts.prefix(Self.shownExamples).joined(separator: ", ")
                        + (texts.count > Self.shownExamples ? ", …" : "")
                    line(Self.notUsable(group.0, count: group.1.count, examples: examples))
                }
            }
        }
        if !file.skipped.isEmpty {
            section(String(localized: "Skipped in the file")) {
                let groups = HexWordsSkipReason.allCases.compactMap { reason -> (HexWordsSkipReason, Int)? in
                    let count = file.skipped.filter { $0.reason == reason }.count
                    return count == 0 ? nil : (reason, count)
                }
                ForEach(Array(groups.enumerated()), id: \.offset) { index, group in
                    if index > 0 { SettingsDivider() }
                    line(String(localized: "\(group.1) skipped: \(Self.skipped(group.0))"))
                }
            }
        }
        buttons(primary: String(localized: "Add"), { add(file) }, secondary: String(localized: "Cancel")) { dismiss() }
    }

    private func note(_ text: String) -> some View {
        Text(verbatim: text)
            .font(.system(size: 14))
            .foregroundStyle(HexTheme.textMuted)
            .lineLimit(3)
    }

    private func paragraph(_ text: String) -> some View {
        Text(verbatim: text)
            .font(.system(size: 17))
            .foregroundStyle(HexTheme.textBase)
            .fixedSize(horizontal: false, vertical: true)
    }

    private func line(_ text: String, size: CGFloat = 14, muted: Bool = false) -> some View {
        Text(verbatim: text)
            .font(.system(size: size))
            .foregroundStyle(muted ? HexTheme.textMuted : HexTheme.textBase)
            .lineLimit(3)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
    }

    @ViewBuilder
    private func rows(_ items: [String]) -> some View {
        ForEach(Array(items.prefix(Self.shownRows).enumerated()), id: \.offset) { _, item in
            SettingsDivider()
            line(item, size: 16)
        }
        if items.count > Self.shownRows {
            SettingsDivider()
            line(String(localized: "and \(items.count - Self.shownRows) more"), muted: true)
        }
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(title)
                .font(.system(size: 14))
                .foregroundStyle(HexTheme.textMuted)
                .padding(.leading, 16)
            SettingsCard { content() }
        }
    }

    private func clashRow(_ clash: HexWordsClash) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(verbatim: HexWordsText.display(clash.heard))
                .font(.system(size: 16, weight: .medium))
                .foregroundStyle(HexTheme.textBase)
                .lineLimit(2)
            Text(verbatim: String(localized: "Yours: \(HexWordsText.display(clash.yours))"))
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textMuted)
                .lineLimit(3)
            Text(verbatim: String(localized: "This file: \(HexWordsText.display(clash.theirs))"))
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textMuted)
                .lineLimit(3)
            SettingsSegments(
                items: [false, true],
                selected: useTheirs.contains(clash.path),
                label: \.clashChoice,
                onSelect: { theirs in
                    if theirs { useTheirs.insert(clash.path) } else { useTheirs.remove(clash.path) }
                }
            )
            .padding(.top, 6)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    /// A secondary text action and the one primary button, at the end of the row.
    private func buttons(primary: String, _ onPrimary: @escaping () -> Void, secondary: String? = nil, _ onSecondary: @escaping () -> Void = {}) -> some View {
        HStack(spacing: 8) {
            Spacer(minLength: 0)
            if let secondary {
                Button(secondary, action: onSecondary)
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
                    .padding(.horizontal, 16)
                    .frame(minHeight: 48)
                    .contentShape(Rectangle())
                    .buttonStyle(.plain)
            }
            Button(action: onPrimary) {
                Text(primary)
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(HexTheme.textContrast)
                    .padding(.horizontal, 24)
                    .frame(minHeight: 48)
                    .background(HexTheme.bgContrast, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
            .buttonStyle(.plain)
        }
    }

    // MARK: Words

    static func counts(new: Int, there: Int, clashes: Int) -> String {
        var pieces = [String(localized: "\(new) new")]
        if there > 0 { pieces.append(String(localized: "\(there) already there")) }
        if clashes > 0 { pieces.append(String(localized: "\(clashes) different from yours")) }
        return pieces.joined(separator: " · ")
    }

    static func terms(_ count: Int) -> String { String(localized: "\(count) terms") }
    static func replacements(_ count: Int) -> String { String(localized: "\(count) replacements") }

    /// One line: "Added 2 terms and 2 replacements · 1 already there · kept yours for 1".
    static func confirmation(_ summary: HexWordsSummary) -> String {
        let terms = summary.addedTerms > 0 ? terms(summary.addedTerms) : nil
        let replacements = summary.addedReplacements > 0 ? replacements(summary.addedReplacements) : nil
        var pieces: [String] = []
        if let terms, let replacements {
            pieces.append(String(localized: "Added \(terms) and \(replacements)"))
        } else if let one = terms ?? replacements {
            pieces.append(String(localized: "Added \(one)"))
        } else if summary.changedReplacements == 0 {
            pieces.append(String(localized: "No new words in that file"))
        }
        let there = summary.alreadyHaveTerms + summary.alreadyHaveReplacements
        if there > 0 { pieces.append(String(localized: "\(there) already there")) }
        if summary.keptYours > 0 { pieces.append(String(localized: "kept yours for \(summary.keptYours)")) }
        if summary.changedReplacements > 0 { pieces.append(String(localized: "used the file’s for \(summary.changedReplacements)")) }
        return pieces.joined(separator: " · ")
    }

    static func sentence(for refusal: HexWordsRefusal) -> String {
        switch refusal {
        case .tooLarge: String(localized: "Hex didn’t open this file: it’s larger than 8 MB.")
        case .notUTF8: String(localized: "Hex didn’t open this file: it isn’t text Hex can read.")
        case .empty: String(localized: "Hex didn’t open this file: it’s empty.")
        case .tooDeep: String(localized: "Hex didn’t open this file: it’s nested too deeply to be a words file.")
        case .duplicateKey: String(localized: "Hex didn’t open this file: it names the same field twice.")
        case .notJSON: String(localized: "Hex didn’t open this file: it’s damaged or isn’t a words file.")
        case .notAnObject, .notHexWords: String(localized: "Hex didn’t open this file: it isn’t a Hex words file.")
        case .badVersion: String(localized: "Hex didn’t open this file: its version isn’t one Hex understands.")
        }
    }

    static func notUsable(_ reason: HexWordsNotAppliedReason, count: Int, examples: String) -> String {
        switch reason {
        case .tooShortForThisDevice:
            String(localized: "\(count) terms have fewer than 2 letters or digits, too short for this iPhone: \(examples)")
        case .overDeviceLimit:
            String(localized: "\(count) didn’t fit, as this iPhone keeps up to 100 terms and 1,000 replacements: \(examples)")
        case .deletionNotSupported:
            String(localized: "\(count) replacements remove words instead of writing something, which this iPhone can’t do yet: \(examples)")
        case .duplicate:
            String(localized: "\(count) spellings repeat words the file already replaces, so the first one is used: \(examples)")
        }
    }

    static func skipped(_ reason: HexWordsSkipReason) -> String {
        switch reason {
        case .badPart: String(localized: "a part that isn’t a list")
        case .badEntry: String(localized: "entries of the wrong shape")
        case .badField: String(localized: "fields of the wrong shape")
        case .unknownField: String(localized: "fields this Hex doesn’t know")
        case .hiddenCharacters: String(localized: "hidden characters")
        case .noLetters: String(localized: "no letters or digits")
        case .tooLong: String(localized: "longer than the limit")
        case .tooManyWords: String(localized: "more than 8 words")
        case .duplicate: String(localized: "already in the file")
        case .sameAsTerm: String(localized: "a spelling the same as its term")
        case .overLimit: String(localized: "past the file’s limit")
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
