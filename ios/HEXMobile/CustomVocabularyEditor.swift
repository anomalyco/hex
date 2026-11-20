import SwiftUI

/// The custom vocabulary list shown under its switch in Settings › Dictation.
struct CustomVocabularyEditor: View {
    private let settings = CustomVocabularySettings()

    @State private var terms: [String] = []
    @State private var draft = ""
    @State private var notice: String?

    private var isAtLimit: Bool {
        terms.count >= CustomVocabularySettings.maxTermCount
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(terms, id: \.self) { term in
                HStack(spacing: 10) {
                    Text(term)
                        .font(.system(size: 14))
                        .foregroundStyle(HexTheme.textBase)
                        .lineLimit(1)

                    Spacer(minLength: 8)

                    Button {
                        remove(term)
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(HexTheme.textMuted)
                            .frame(width: 26, height: 26)
                            .background(HexTheme.layer02, in: Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Remove \(term)")
                }
                .padding(.leading, 12)
                .padding(.trailing, 6)
                .frame(minHeight: 36)
                .background(
                    HexTheme.layer01,
                    in: RoundedRectangle(
                        cornerRadius: HexTheme.controlRadius,
                        style: .continuous
                    )
                )
            }

            TextField("Add a word or phrase", text: $draft)
                .font(.system(size: 14))
                .foregroundStyle(HexTheme.textBase)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .submitLabel(.done)
                .onSubmit(add)
                .onChange(of: draft) { _, _ in
                    notice = nil
                }
                .disabled(isAtLimit)
                .padding(.horizontal, 12)
                .frame(minHeight: 38)
                .background(
                    HexTheme.layer01,
                    in: RoundedRectangle(
                        cornerRadius: HexTheme.controlRadius,
                        style: .continuous
                    )
                )
                .opacity(isAtLimit ? 0.5 : 1)

            if isAtLimit {
                Text("Up to 100 terms")
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
            }

            if let notice {
                Text(notice)
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Text("Fixes spelling, not words Hex mishears")
            .font(.system(size: 12))
            .foregroundStyle(HexTheme.textMuted)
            .fixedSize(horizontal: false, vertical: true)
        }
        .onAppear {
            terms = settings.terms
        }
    }

    private func add() {
        guard !isAtLimit else { return }
        let candidate = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !candidate.isEmpty else { return }

        guard candidate.count <= CustomVocabularySettings.maxTermLength else {
            notice = String(localized: "Too long")
            return
        }
        let key = VocabularySpelling.normalizedKey(candidate)
        guard key.count >= 2 else {
            notice = String(localized: "Needs at least two letters or digits")
            return
        }
        let isDuplicate = terms.contains {
            VocabularySpelling.normalizedKey($0) == key
        }
        guard !isDuplicate else {
            notice = String(localized: "Already in the list")
            return
        }

        let updated = CustomVocabularySettings.sanitize(terms + [candidate])
        terms = updated
        settings.terms = updated
        draft = ""
        notice = nil
    }

    private func remove(_ term: String) {
        terms.removeAll { $0 == term }
        settings.terms = terms
        notice = nil
    }
}
