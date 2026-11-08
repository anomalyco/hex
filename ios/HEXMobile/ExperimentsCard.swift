import SwiftUI

/// Unfinished features, off until turned on here. They live in the App Group so the
/// keyboard reads them too.
struct ExperimentsCard: View {
    @State private var fullKeyboard = FullKeyboardSettings().isEnabled
    @State private var appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
    @State private var customVocabulary = CustomVocabularySettings().isEnabled
    @State private var cleanupAvailability = AppleIntelligenceCleanupAvailability.current

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Experiments")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
                .padding(.leading, 16)

            Toggle(isOn: $fullKeyboard) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Full keyboard")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    Text("Type with Hex, with a Hex key to dictate")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }
            .tint(HexTheme.success)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
            .onChange(of: fullKeyboard) { _, enabled in
                FullKeyboardSettings().isEnabled = enabled
            }

            Toggle(isOn: $appleIntelligenceCleanup) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Clean up with Apple Intelligence")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    Text("Fix misheard words and spoken corrections on device")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                    if let reason = cleanupAvailability.reason {
                        Text(reason)
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
            }
            .tint(HexTheme.success)
            .disabled(!cleanupAvailability.isAvailable)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
            .onChange(of: appleIntelligenceCleanup) { _, enabled in
                AppleIntelligenceCleanupSettings().isEnabled = enabled
            }

            Toggle(isOn: $customVocabulary) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Custom vocabulary")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    Text("Spell names and terms your way")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }
            .tint(HexTheme.success)
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
            .onChange(of: customVocabulary) { _, enabled in
                CustomVocabularySettings().isEnabled = enabled
            }

            if customVocabulary {
                CustomVocabularyEditor()
                    .padding(12)
                    .background(
                        HexTheme.bgBase,
                        in: RoundedRectangle(
                            cornerRadius: HexTheme.cardRadius,
                            style: .continuous
                        )
                    )
            }
        }
        .onAppear {
            fullKeyboard = FullKeyboardSettings().isEnabled
            appleIntelligenceCleanup = AppleIntelligenceCleanupSettings().isEnabled
            customVocabulary = CustomVocabularySettings().isEnabled
            cleanupAvailability = AppleIntelligenceCleanupAvailability.current
        }
    }
}
