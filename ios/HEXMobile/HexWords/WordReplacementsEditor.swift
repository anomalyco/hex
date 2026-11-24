import SwiftUI

/// Settings › Dictation › Custom vocabulary's Word replacements: heard words and what to
/// write instead, then Export words and, after an import, Undo last import.
struct WordReplacementsEditor: View {
    private let settings = WordReplacementsSettings()

    @State private var entries: [WordReplacement] = []
    @State private var heard = ""
    @State private var write = ""
    @State private var notice: String?
    @State private var canUndo = false
    @State private var showsExport = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            SettingsRowLabel(
                title: String(localized: "Word replacements"),
                detail: String(localized: "Write something else when Hex hears certain words")
            )
            .padding(.bottom, 2)

            ForEach(entries, id: \.self) { entry in
                HStack(spacing: 10) {
                    HStack(alignment: .firstTextBaseline, spacing: 6) {
                        Text(verbatim: entry.heard)
                            .foregroundStyle(HexTheme.textMuted)
                        Image(systemName: "arrow.right")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(HexTheme.textFaint)
                        Text(verbatim: entry.write)
                            .foregroundStyle(HexTheme.textBase)
                    }
                    .font(.system(size: 14))
                    .lineLimit(1)

                    Spacer(minLength: 8)

                    Button {
                        remove(entry)
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(HexTheme.textMuted)
                            .frame(width: 26, height: 26)
                            .background(HexTheme.layer02, in: Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(String(localized: "Remove \(entry.heard)"))
                }
                .padding(.leading, 12)
                .padding(.trailing, 6)
                .frame(minHeight: 36)
                .background(HexTheme.layer01, in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }

            HStack(spacing: 8) {
                field(String(localized: "When Hex hears"), text: $heard)
                Image(systemName: "arrow.right")
                    .font(.system(size: 11, weight: .semibold))
                    .foregroundStyle(HexTheme.textFaint)
                field(String(localized: "Write"), text: $write)
            }

            if !heard.isEmpty || !write.isEmpty {
                Button(String(localized: "Add"), action: add)
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(HexTheme.textBase)
                    .buttonStyle(.plain)
                    .padding(.horizontal, 14)
                    .frame(minHeight: 34)
                    .background(HexTheme.layer02, in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }

            if let notice {
                Text(notice)
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Button {
                showsExport = true
            } label: {
                SettingsRowLabel(
                    title: String(localized: "Export words"),
                    detail: String(localized: "Share them as a Hex words file")
                )
                .frame(maxWidth: .infinity, alignment: .leading)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(.top, 6)

            if canUndo {
                Button {
                    HexWordsImport().undo()
                    NotificationCenter.default.post(name: .hexWordsListsDidChange, object: nil)
                } label: {
                    SettingsRowLabel(
                        title: String(localized: "Undo last import"),
                        detail: String(localized: "Puts both lists back as they were")
                    )
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .onAppear(perform: reload)
        .onReceive(NotificationCenter.default.publisher(for: .hexWordsListsDidChange)) { _ in reload() }
        .sheet(isPresented: $showsExport) {
            HexWordsExportSheet()
        }
    }

    private func field(_ prompt: String, text: Binding<String>) -> some View {
        TextField(prompt, text: text)
            .font(.system(size: 14))
            .foregroundStyle(HexTheme.textBase)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .submitLabel(.done)
            .onSubmit(add)
            .onChange(of: text.wrappedValue) { _, _ in notice = nil }
            .padding(.horizontal, 12)
            .frame(minHeight: 38)
            .background(HexTheme.layer01, in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
    }

    private func reload() {
        entries = settings.entries
        canUndo = HexWordsImport().availableUndo != nil
    }

    private func add() {
        guard !HexWordsText.cleanLine(heard).isEmpty else { return }
        if let problem = WordReplacementsSettings.problem(heard: heard, write: write, existing: entries) {
            notice = Self.notice(for: problem)
            return
        }
        settings.entries = entries + [WordReplacement(heard: heard, write: write)]
        heard = ""
        write = ""
        notice = nil
        reload()
    }

    private func remove(_ entry: WordReplacement) {
        settings.entries = entries.filter { $0 != entry }
        notice = nil
        reload()
    }

    static func notice(for problem: WordReplacementsSettings.Problem) -> String {
        switch problem {
        case .noLetters: String(localized: "Needs at least one letter or digit")
        case .hiddenCharacters: String(localized: "Has a hidden character")
        case .tooLong: String(localized: "Too long")
        case .tooManyWords: String(localized: "Up to 8 words")
        case .emptyWrite: String(localized: "Write something to replace it with")
        case .duplicate: String(localized: "Already in the list")
        case .full: String(localized: "Up to 1,000 replacements")
        }
    }
}

/// Export words: a switch for the learned fixes, what goes in and what's left out, then the
/// share sheet with `hex-words.hexwords`, deleted once sharing ends.
struct HexWordsExportSheet: View {
    @Environment(\.dismiss) private var dismiss
    @State private var includesLearned = false

    private var output: HexWordsWriter.Output {
        let learned = Self.learnedFixes()
        return HexWordsWriter.phone(
            vocabulary: CustomVocabularySettings().terms,
            replacements: WordReplacementsSettings().entries,
            learnedFixes: learned,
            includeLearned: includesLearned,
            appVersion: Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String ?? ""
        )
    }

    var body: some View {
        let output = output
        let leftOut = Dictionary(grouping: output.notExported, by: \.reason)
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 12) {
                    SettingsCard {
                        SettingsToggleRow(
                            title: String(localized: "Include fixes Hex learned"),
                            detail: String(localized: "Only the ones in use"),
                            isOn: $includesLearned
                        )
                    }
                    Text(String(localized: "\(output.terms) terms and \(output.replacements) replacements"))
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.textMuted)
                        .padding(.horizontal, 16)
                    if !leftOut.isEmpty {
                        SettingsSectionHeader(title: "Left out")
                        SettingsCard {
                            ForEach(Array(HexWordsSkipReason.allCases.filter { leftOut[$0] != nil }.enumerated()), id: \.element) { index, reason in
                                if index > 0 { SettingsDivider() }
                                HStack {
                                    Text(Self.reason(reason))
                                        .font(.system(size: 15))
                                        .foregroundStyle(HexTheme.textBase)
                                    Spacer(minLength: 8)
                                    Text(verbatim: (leftOut[reason]?.count ?? 0).formatted())
                                        .font(.system(size: 15).monospacedDigit())
                                        .foregroundStyle(HexTheme.textMuted)
                                }
                                .padding(.horizontal, 16)
                                .frame(minHeight: 50)
                            }
                        }
                    }
                }
                .padding(.horizontal, 20)
                .padding(.top, 8)
            }
            .background(HexTheme.bgDeep.ignoresSafeArea())
            .navigationTitle(String(localized: "Export words"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbarBackground(HexTheme.bgDeep, for: .navigationBar)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(String(localized: "Cancel")) { dismiss() }
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(String(localized: "Share")) { share(output.data) }
                        .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium, .large])
        .preferredColorScheme(.dark)
    }

    private func share(_ data: Data) {
        guard let url = try? HexWordsWriter.temporaryFile(data) else { return }
        ShareSheet.present([url]) { _ in
            try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
            dismiss()
        }
    }

    /// Learn how I type's learned dictation fixes, each marked in use unless switched off in
    /// Typing insights; empty while Learn how I type is off.
    static func learnedFixes(settings: TypingJournalSettings = TypingJournalSettings()) -> [HexWordsWriter.LearnedFix] {
        guard settings.isEnabled else { return [] }
        let folder = FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)
        let snapshot = TypingJournal.load(from: folder?.appendingPathComponent(TypingJournal.fileName))
        let fixes = settings.learnsPerLanguage ? snapshot.learnedDictationFixesByLanguage() : snapshot.learnedDictationFixes
        let disabled = settings.disabledDictationFixes
        return fixes.map { HexWordsWriter.LearnedFix(heard: $0.heard, meant: $0.meant, active: !disabled.contains($0.id)) }
    }

    static func reason(_ reason: HexWordsSkipReason) -> String {
        switch reason {
        case .duplicate: String(localized: "Repeats an earlier entry")
        default: HexWordsReviewSheet.sentence(for: reason)
        }
    }
}
