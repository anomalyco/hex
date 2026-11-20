import SwiftUI

/// The home screen's four settings pages.
enum SettingsPage: String, Hashable, CaseIterable {
    case dictation
    case keyboard
    case privacy
    case experiments

    var title: String {
        switch self {
        case .dictation: "Dictation"
        case .keyboard: "Keyboard"
        case .privacy: "Privacy"
        case .experiments: "Experiments"
        }
    }

    var icon: String {
        switch self {
        case .dictation: "waveform"
        case .keyboard: "keyboard"
        case .privacy: "lock"
        case .experiments: "testtube.2"
        }
    }
}

/// A settings page: a short scrolling column of cards under an inline title.
struct SettingsPageScaffold<Content: View>: View {
    let title: String
    @ViewBuilder var content: Content

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                content
            }
            .frame(maxWidth: 500)
            .padding(.horizontal, 20)
            .padding(.top, 8)
            .padding(.bottom, 28)
            .frame(maxWidth: .infinity)
        }
        .scrollBounceBehavior(.basedOnSize)
        .scrollDismissesKeyboard(.interactively)
        .defaultScrollAnchor(SettingsSnapshotFixture.scrollsToEnd ? .bottom : .top)
        .background(HexTheme.bgDeep.ignoresSafeArea())
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(HexTheme.bgDeep, for: .navigationBar)
    }
}

/// Rows on `bgBase` with the theme's corner radius.
struct SettingsCard<Content: View>: View {
    @ViewBuilder var content: Content

    var body: some View {
        VStack(spacing: 0) {
            content
        }
        .background(
            HexTheme.bgBase,
            in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
        )
    }
}

/// A small label above a card, such as Privacy's "Advanced".
struct SettingsSectionHeader: View {
    let title: String

    var body: some View {
        Text(title)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(HexTheme.textMuted)
            .padding(.leading, 16)
            .padding(.top, 12)
    }
}

struct SettingsDivider: View {
    var leading: CGFloat = 16

    var body: some View {
        Rectangle()
            .fill(HexTheme.borderWeak)
            .frame(height: 0.5)
            .padding(.leading, leading)
    }
}

/// A row's title, and its one-line description when it has one.
struct SettingsRowLabel: View {
    let title: String
    var detail: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(title)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(HexTheme.textBase)
            if let detail {
                Text(detail)
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
            }
        }
    }
}

/// A switch with its one-line description.
struct SettingsToggleRow: View {
    let title: String
    var detail: String?
    @Binding var isOn: Bool

    var body: some View {
        Toggle(isOn: $isOn) {
            SettingsRowLabel(title: title, detail: detail)
        }
        .tint(HexTheme.success)
        .padding(.horizontal, 16)
        .padding(.vertical, 10)
        .frame(minHeight: 50)
    }
}

/// A row that opens something: a page (chevron) or another app (arrow).
struct SettingsLinkLabel: View {
    let title: String
    var icon: String?
    var trailingIcon = "chevron.right"

    var body: some View {
        HStack(spacing: 12) {
            if let icon {
                Image(systemName: icon)
                    .font(.system(size: 15, weight: .regular))
                    .foregroundStyle(HexTheme.textMuted)
                    .frame(width: 22)
            }
            Text(title)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(HexTheme.textBase)
            Spacer(minLength: 8)
            Image(systemName: trailingIcon)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
        }
        .padding(.horizontal, 16)
        .frame(minHeight: 50)
        .contentShape(Rectangle())
    }
}

/// A menu's current value with the up-down chevron.
struct SettingsMenuValue: View {
    let value: String

    var body: some View {
        HStack(spacing: 5) {
            Text(value)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(HexTheme.textMuted)
                .lineLimit(1)
            Image(systemName: "chevron.up.chevron.down")
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
        }
        .padding(.vertical, 6)
        .contentShape(Rectangle())
    }
}

/// A small segmented control in the settings rows' style.
struct SettingsSegments<Item: Hashable>: View {
    let items: [Item]
    let selected: Item
    let label: KeyPath<Item, String>
    let onSelect: (Item) -> Void

    var body: some View {
        HStack(spacing: 2) {
            ForEach(items, id: \.self) { item in
                let isSelected = item == selected
                Button {
                    onSelect(item)
                } label: {
                    Text(item[keyPath: label])
                        .font(.system(size: 13, weight: .medium))
                        .lineLimit(1)
                        .fixedSize(horizontal: true, vertical: false)
                        .foregroundStyle(isSelected ? HexTheme.textBase : HexTheme.textMuted)
                        .padding(.horizontal, 10)
                        .frame(height: 28)
                        .background(
                            isSelected ? Color.white.opacity(0.14) : Color.clear,
                            in: RoundedRectangle(cornerRadius: 7, style: .continuous)
                        )
                        .contentShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
                }
                .buttonStyle(.plain)
            }
        }
        .fixedSize(horizontal: true, vertical: false)
        .padding(2)
        .background(
            Color.white.opacity(0.055),
            in: RoundedRectangle(cornerRadius: 9, style: .continuous)
        )
    }
}

/// Simulator scenes for the home screen and its pages (`--snap-state <name>`):
///
/// - `settings-home`: home, everything ready. `setup`: home, keyboard added but Full Access
///   not confirmed yet. `setup-sheet`: the same, with the setup sheet open;
///   `setup-sheet-microphone`: the sheet before iOS has asked for the microphone.
/// - `settings-<page>` opens a page (`dictation`, `keyboard`, `privacy`, `experiments`) with
///   every switch at its default; `-on` turns the switches on, `-end` starts scrolled to the end.
///   `settings-privacy-confirm` opens Record typing logs' consent dialog.
///
/// `KeyboardSetupMonitor.refresh` and `DictationController` supply the matching state.
enum SettingsSnapshotFixture {
    static let scene: String? = {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else { return nil }
        let value = args[idx + 1]
        return value.hasPrefix("settings-") || value.hasPrefix("setup-sheet") ? value : nil
    }()

    static var initialPath: [SettingsPage] {
        guard let scene else { return [] }
        let name = scene.dropFirst("settings-".count).split(separator: "-").first.map(String.init)
        return name.flatMap(SettingsPage.init(rawValue:)).map { [$0] } ?? []
    }

    static var showsSetupSheet: Bool {
        scene?.hasPrefix("setup-sheet") == true
    }

    /// `settings-privacy-confirm`: Record typing logs' consent dialog, open.
    static var confirmsTypingLogs: Bool {
        scene?.hasSuffix("-confirm") == true
    }

    static var scrollsToEnd: Bool {
        scene?.hasSuffix("-end") == true
    }

    /// Writes the page's switches to the App Group so every run renders the same.
    @MainActor
    static func applyDefaults() {
        guard let scene, scene.hasPrefix("settings-") else { return }
        let on = scene.contains("-on")
        let keyboard = FullKeyboardSettings()
        keyboard.isEnabled = on
        keyboard.autocorrect = true
        keyboard.keyPopups = true
        keyboard.haptics = true
        keyboard.emoji = on
        let emojiUsage = EmojiUsageStore()
        emojiUsage.clear()
        if on {
            emojiUsage.record("🎉")
        }
        LanguageSettings().keyboardLayout = nil
        CleanupTaggerSettings().isEnabled = on
        AppleIntelligenceCleanupSettings().isEnabled = false
        CustomVocabularySettings().isEnabled = on
        CustomVocabularySettings().terms = on ? ["OpenCode", "SwiftUI", "TestFlight"] : []
        DictationTranslationSettings().isEnabled = on
        DictationTranslationSettings().targetLanguage = "ja"
        TypingJournalSettings().isEnabled = on
        TypingLogSettings().isEnabled = on
    }
}
