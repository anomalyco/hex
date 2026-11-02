import SwiftUI

/// Unfinished features, off until turned on here. They live in the App Group so the
/// keyboard reads them too.
struct ExperimentsCard: View {
    @State private var fullKeyboard = FullKeyboardSettings().isEnabled
    @State private var autocorrect = FullKeyboardSettings().autocorrect

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text("Experiments")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
                .padding(.leading, 16)

            VStack(spacing: 0) {
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
                .onChange(of: fullKeyboard) { _, enabled in
                    FullKeyboardSettings().isEnabled = enabled
                }

                if fullKeyboard {
                    Divider()
                        .overlay(HexTheme.borderWeak)
                        .padding(.leading, 16)

                    Toggle(isOn: $autocorrect) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Autocorrect")
                                .font(.system(size: 15, weight: .regular))
                                .foregroundStyle(HexTheme.textBase)
                            Text("Fix contractions, shortcuts, and one-edit typos on space")
                                .font(.system(size: 12))
                                .foregroundStyle(HexTheme.textMuted)
                        }
                    }
                    .tint(HexTheme.success)
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
                    .onChange(of: autocorrect) { _, enabled in
                        FullKeyboardSettings().autocorrect = enabled
                    }
                }
            }
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
        }
        .onAppear {
            let settings = FullKeyboardSettings()
            fullKeyboard = settings.isEnabled
            autocorrect = settings.autocorrect
        }
    }
}
