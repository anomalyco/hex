import SwiftUI
import UniformTypeIdentifiers

/// Custom vocabulary's own rows: Word replacements and Words file while it's on, and the last
/// import's Undo while there is one. Words as Android's.
struct HexWordsRows: View {
    let isOn: Bool

    @State private var count = WordReplacementsSettings().entries.count
    @State private var canUndo = HexWordsImport().availableUndo != nil
    @State private var showsReplacements = false
    @State private var showsFile = false

    var body: some View {
        VStack(spacing: 0) {
            if isOn {
                SettingsDivider()
                row(
                    String(localized: "Word replacements"),
                    count == 0 ? String(localized: "Words Hex hears, and what to write instead") : HexWordsReviewSheet.replacements(count)
                ) { showsReplacements = true }
                SettingsDivider()
                row(String(localized: "Words file"), String(localized: "Open, share or save your words")) { showsFile = true }
            }
            if canUndo {
                SettingsDivider()
                HexWordsUndoRow()
            }
        }
        .navigationDestination(isPresented: $showsReplacements) {
            SettingsPageScaffold(title: "Word replacements") { WordReplacementsPage() }
        }
        .navigationDestination(isPresented: $showsFile) {
            SettingsPageScaffold(title: "Words file") { WordsFilePage() }
        }
        .onAppear(perform: reload)
        .onChange(of: showsReplacements) { _, _ in reload() }
        .onChange(of: showsFile) { _, _ in reload() }
        .onReceive(NotificationCenter.default.publisher(for: .hexWordsListsDidChange)) { _ in reload() }
    }

    private func reload() {
        count = WordReplacementsSettings().entries.count
        canUndo = HexWordsImport().availableUndo != nil
    }

    private func row(_ title: String, _ detail: String, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                SettingsRowLabel(title: title, detail: detail)
                Spacer(minLength: 8)
                Image(systemName: "chevron.right")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(HexTheme.textMuted)
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .frame(minHeight: 50)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// Undo last import: puts both lists back as the last import found them.
struct HexWordsUndoRow: View {
    var body: some View {
        Button {
            HexWordsImport().undo()
            NotificationCenter.default.post(name: .hexWordsListsDidChange, object: nil)
        } label: {
            HStack {
                SettingsRowLabel(title: String(localized: "Undo last import"))
                Spacer(minLength: 8)
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 50)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }
}

/// Settings › Dictation › Custom vocabulary › Word replacements: add and remove the person's
/// own entries. Any edit ends the last import's Undo.
struct WordReplacementsPage: View {
    private let settings = WordReplacementsSettings()

    @State private var entries: [WordReplacement] = []
    @State private var heard = ""
    @State private var write = ""
    @State private var problem: WordReplacementsSettings.Problem?

    var body: some View {
        Text(String(localized: "Hex writes these instead of the words it hears, matching whole words, up to 8. Yours win over fixes Hex learned."))
            .font(.system(size: 14))
            .foregroundStyle(HexTheme.textMuted)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 16)

        SettingsCard {
            field(String(localized: "Words Hex hears"), text: $heard, axis: .horizontal)
            SettingsDivider()
            field(String(localized: "What to write"), text: $write, axis: .vertical)
            SettingsDivider()
            HStack(spacing: 8) {
                Text(problem.map(Self.text(for:)) ?? "")
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
                    .lineLimit(3)
                    .frame(maxWidth: .infinity, alignment: .leading)
                Button(String(localized: "Add"), action: add)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(HexTheme.textBase)
                    .padding(.horizontal, 12)
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
                    .buttonStyle(.plain)
            }
            .padding(.leading, 16)
            .padding(.trailing, 4)
            .frame(minHeight: 52)
        }

        if !entries.isEmpty {
            SettingsCard {
                ForEach(Array(entries.enumerated()), id: \.element) { index, entry in
                    if index > 0 { SettingsDivider() }
                    HStack(spacing: 10) {
                        Text(verbatim: entry.heard + " → " + entry.write)
                            .font(.system(size: 15))
                            .foregroundStyle(HexTheme.textBase)
                            .lineLimit(3)
                            .frame(maxWidth: .infinity, alignment: .leading)
                        Button {
                            settings.entries = entries.filter { $0 != entry }
                            reload()
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 11, weight: .semibold))
                                .foregroundStyle(HexTheme.textMuted)
                                .frame(width: 26, height: 26)
                                .background(HexTheme.layer02, in: Circle())
                                .frame(width: 44, height: 44)
                                .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel(String(localized: "Remove"))
                    }
                    .padding(.leading, 16)
                    .padding(.trailing, 4)
                    .frame(minHeight: 52)
                }
            }
        }

        Color.clear.frame(height: 0)
            .onAppear(perform: reload)
    }

    private func field(_ prompt: String, text: Binding<String>, axis: Axis) -> some View {
        TextField(prompt, text: text, axis: axis)
            .lineLimit(axis == .vertical ? 1...4 : 1...1)
            .font(.system(size: 16))
            .foregroundStyle(HexTheme.textBase)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .onChange(of: text.wrappedValue) { _, _ in problem = nil }
            .padding(.horizontal, 16)
            .padding(.vertical, 14)
    }

    private func reload() {
        entries = settings.entries
    }

    private func add() {
        if let found = WordReplacementsSettings.problem(heard: heard, write: write, existing: entries) {
            problem = found
            return
        }
        settings.entries = entries + [WordReplacement(heard: heard, write: write)]
        heard = ""
        write = ""
        reload()
    }

    static func text(for problem: WordReplacementsSettings.Problem) -> String {
        switch problem {
        case .noLetters: String(localized: "The words Hex hears need a letter or digit.")
        case .hiddenCharacters: String(localized: "This has a hidden character in it.")
        case .tooLong: String(localized: "Too long: up to 128 characters heard and 1,000 written.")
        case .tooManyWords: String(localized: "Up to 8 words heard.")
        case .emptyWrite: String(localized: "Write something. Removing words isn’t supported yet.")
        case .duplicate: String(localized: "You already replace these words.")
        case .full: String(localized: "The list holds 1,000 replacements.")
        }
    }
}

/// Settings › Dictation › Custom vocabulary › Words file: open one from Files, or share the
/// terms and Word replacements as `hex-words.hexwords` (the share sheet's Save to Files saves
/// it), with the learned fixes in use only when the person includes them.
struct WordsFilePage: View {
    @State private var includesLearned = false
    @State private var status: String?
    @State private var opens = false
    @State private var learnedInUse = false
    @State private var canUndo = false

    var body: some View {
        SettingsCard {
            Button {
                opens = true
            } label: {
                HStack(spacing: 12) {
                    SettingsRowLabel(
                        title: String(localized: "Open a words file…"),
                        detail: String(localized: "Check what a .hexwords file adds before anything changes")
                    )
                    Spacer(minLength: 8)
                    Image(systemName: "chevron.right")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .frame(minHeight: 50)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }

        VStack(alignment: .leading, spacing: 8) {
            Text(String(localized: "Export"))
                .font(.system(size: 14))
                .foregroundStyle(HexTheme.textMuted)
                .padding(.leading, 16)
            SettingsCard {
                if learnedInUse {
                    SettingsToggleRow(
                        title: String(localized: "Include fixes Hex learned"),
                        detail: String(localized: "Adds the learned fixes in use, marked as learned"),
                        isOn: $includesLearned
                    )
                    SettingsDivider()
                }
                Button(action: share) {
                    SettingsLinkLabel(title: String(localized: "Share…"), trailingIcon: "square.and.arrow.up")
                }
                .buttonStyle(.plain)
            }
            Text(status ?? String(localized: "Exports your Custom vocabulary terms and Word replacements as hex-words.hexwords, which Hex on a Mac, a PC, an iPhone or an Android phone can open."))
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 16)
        }

        if canUndo {
            SettingsCard { HexWordsUndoRow() }
        }

        Color.clear.frame(height: 0)
            .onAppear(perform: reload)
            .onReceive(NotificationCenter.default.publisher(for: .hexWordsListsDidChange)) { _ in reload() }
            .fileImporter(isPresented: $opens, allowedContentTypes: [.data]) { result in
                if case .success(let url) = result { HexInbox.open(url) }
            }
    }

    private func reload() {
        learnedInUse = Self.learnedFixes().contains(where: \.active)
        canUndo = HexWordsImport().availableUndo != nil
    }

    private func share() {
        let written = Self.write(includeLearned: includesLearned && learnedInUse)
        guard let url = try? HexWordsWriter.temporaryFile(written.data) else {
            status = String(localized: "Hex couldn’t write the file.")
            return
        }
        ShareSheet.present([url]) { _ in
            try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
        }
        status = Self.exported(written)
    }

    static func write(includeLearned: Bool) -> HexWordsWriter.Output {
        HexWordsWriter.phone(
            vocabulary: CustomVocabularySettings().terms,
            replacements: WordReplacementsSettings().entries,
            learnedFixes: learnedFixes(),
            includeLearned: includeLearned,
            appVersion: HexWordsWriter.appVersion()
        )
    }

    static func exported(_ written: HexWordsWriter.Output) -> String {
        let counts = String(localized: "Exported \(HexWordsReviewSheet.terms(written.terms)) and \(HexWordsReviewSheet.replacements(written.replacements)).")
        guard !written.notExported.isEmpty else { return counts }
        var reasons: [HexWordsSkipReason] = []
        for skip in written.notExported where !reasons.contains(skip.reason) { reasons.append(skip.reason) }
        let why = reasons.map(HexWordsReviewSheet.skipped).joined(separator: ", ")
        return counts + " " + String(localized: "Left out \(written.notExported.count): \(why).")
    }

    /// Learn how I type's learned dictation fixes, each in use unless switched off in Typing
    /// insights; none while Learn how I type is off, since only then are they applied.
    static func learnedFixes(settings: TypingJournalSettings = TypingJournalSettings()) -> [HexWordsWriter.LearnedFix] {
        guard settings.isEnabled else { return [] }
        let folder = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)
        let snapshot = TypingJournal.load(from: folder?.appendingPathComponent(TypingJournal.fileName))
        let fixes = settings.learnsPerLanguage ? snapshot.learnedDictationFixesByLanguage() : snapshot.learnedDictationFixes
        let disabled = settings.disabledDictationFixes
        return fixes.map { HexWordsWriter.LearnedFix(heard: $0.heard, meant: $0.meant, active: !disabled.contains($0.id)) }
    }
}
