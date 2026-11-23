import SwiftUI

/// Export for your agent: the warning, every time (design §5: it is true because the logs skip
/// only fields marked password, one-time code, card or secure), the days to include, then
/// Continue builds the zip and opens the share sheet. The zip is deleted once shared.
struct TypingConfigExportSheet: View {
    let days: [TypingLogDay]
    @Environment(\.dismiss) private var dismiss
    @State private var chosen: Set<String>
    @State private var building = false
    @State private var failure: String?

    init(days: [TypingLogDay]) {
        self.days = days
        _chosen = State(initialValue: Set(days.map(\.day)))
    }

    private var chosenDays: [TypingLogDay] { days.filter { chosen.contains($0.day) } }
    private var allChosen: Bool { chosen.count == days.count }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                TypingConfigSheetHeader(title: String(localized: "Export for your agent"), close: { dismiss() })

                Text("One file with your typing logs and a skill your own AI agent follows to make a typing config for you. Open the file it makes on this iPhone to review it.")
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)

                HStack(alignment: .top, spacing: 10) {
                    Image(systemName: "exclamationmark.triangle")
                        .font(.system(size: 14))
                        .foregroundStyle(HexTheme.textBase)
                        .padding(.top, 1)
                    Text(allChosen
                        ? String(localized: "This holds everything you typed and dictated with Hex in the last 14 days, including any password you typed outside a password field. Give it only to an agent you trust, and don't upload it anywhere you wouldn't share your messages.")
                        : String(localized: "This holds everything you typed and dictated with Hex on the days you chose, including any password you typed outside a password field. Give it only to an agent you trust, and don't upload it anywhere you wouldn't share your messages."))
                        .font(.system(size: 14))
                        .foregroundStyle(HexTheme.textBase)
                        .fixedSize(horizontal: false, vertical: true)
                }
                .padding(14)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))

                SettingsSectionHeader(title: "Days")
                SettingsCard {
                    ForEach(Array(days.enumerated()), id: \.element.id) { n, day in
                        if n > 0 { SettingsDivider() }
                        dayRow(day)
                    }
                }

                if let failure {
                    Text(failure)
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.danger)
                }

                VStack(spacing: 4) {
                    OnboardingPrimaryButton(title: building ? "Preparing…" : "Continue", enabled: !building && !chosen.isEmpty, action: export)
                    OnboardingQuietButton(title: "Cancel") { dismiss() }
                }
                .padding(.top, 4)
            }
            .frame(maxWidth: 500)
            .padding(.horizontal, 20)
            .padding(.top, 24)
            .padding(.bottom, 20)
            .frame(maxWidth: .infinity)
        }
        .scrollBounceBehavior(.basedOnSize)
        .background(HexTheme.bgDeep.ignoresSafeArea())
        .presentationBackground(HexTheme.bgDeep)
        .preferredColorScheme(.dark)
        .interactiveDismissDisabled(building)
    }

    private func dayRow(_ day: TypingLogDay) -> some View {
        Button {
            if chosen.contains(day.day) { chosen.remove(day.day) } else { chosen.insert(day.day) }
        } label: {
            HStack(spacing: 12) {
                SettingsRowLabel(title: TypingLogsView.title(day), detail: ByteCountFormatter.string(fromByteCount: Int64(day.bytes), countStyle: .file))
                Spacer(minLength: 8)
                Image(systemName: chosen.contains(day.day) ? "checkmark.circle.fill" : "circle")
                    .font(.system(size: 20))
                    .foregroundStyle(chosen.contains(day.day) ? HexTheme.textBase : HexTheme.textFaint)
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 50)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(chosen.contains(day.day) ? .isSelected : [])
    }

    private func export() {
        building = true
        failure = nil
        Task {
            do {
                let built = try await TypingConfigExport.build(days: chosenDays)
                building = false
                ShareSheet.present([built.url]) {
                    TypingConfigExport.discard(built)
                    dismiss()
                }
            } catch {
                building = false
                failure = String(localized: "Hex couldn't prepare the export. Nothing was shared.")
            }
        }
    }
}
