import SwiftUI

/// The custom vocabulary list shown under its experiment toggle.
struct CustomVocabularyEditor: View {
    private let settings = CustomVocabularySettings()

    @State private var terms: [String] = []
    @State private var draft = ""

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

            Text(
                "Hex fixes the spelling of listed words when it hears them. "
                    + "On Apple Speech, the list also guides recognition."
            )
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

        let updated = CustomVocabularySettings.sanitize(terms + [candidate])
        draft = ""
        guard updated.count > terms.count else { return }

        terms = updated
        settings.terms = updated
    }

    private func remove(_ term: String) {
        terms.removeAll { $0 == term }
        settings.terms = terms
    }
}
