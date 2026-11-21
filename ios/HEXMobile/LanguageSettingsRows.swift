import SwiftUI
import UIKit

/// Settings › Dictation language: Automatic, the first iPhone language Apple Speech
/// transcribes, or any language `SpeechTranscriber` or `DictationTranscriber` has (before
/// iOS 26, any language Apple's recognizer has on the iPhone: `OnDeviceSpeechLanguages`).
/// Remove from keyboard takes a language off the keyboard's quick switch.
struct DictationLanguageRow: View {
    @Bindable var dictation: DictationController
    @Environment(\.scenePhase) private var scenePhase
    @State private var options: [SpeechLanguageOption] = []
    @State private var automaticName: String?
    /// While Automatic follows the space bar, the line naming each keyboard language's
    /// dictation language ("Follows keyboard: English (US) · Spanish (US)").
    @State private var followsKeyboard: String?
    /// The keyboard's quick switch (`LanguageSettings.keyboardLanguageChoices`).
    @State private var keyboardChoices: [String] = []
    /// Before iOS 26: where the languages come from.
    @State private var listsOnDeviceLanguages = false

    var body: some View {
        VStack(spacing: 0) {
            LanguageRowDivider()
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 2) {
                    Text("Dictation language")
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    if dictation.dictationLanguage == nil, let detail = followsKeyboard ?? automaticName {
                        Text(detail)
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                            .lineLimit(followsKeyboard == nil ? 1 : 2)
                    }
                }
                Spacer(minLength: 8)
                Menu {
                    Button {
                        dictation.setDictationLanguage(nil)
                    } label: {
                        if dictation.dictationLanguage == nil {
                            Label("Automatic", systemImage: "checkmark")
                        } else {
                            Text("Automatic")
                        }
                    }
                    if !keyboardChoices.isEmpty {
                        // The keyboard's quick switch: take a language off it here too.
                        Menu("Remove from keyboard") {
                            ForEach(keyboardChoices, id: \.self) { id in
                                Button(role: .destructive) {
                                    let settings = LanguageSettings()
                                    settings.removeDictationLanguage(id)
                                    if dictation.dictationLanguage != nil, settings.dictationLanguage == nil {
                                        dictation.setDictationLanguage(nil)
                                    }
                                    // The switch's chips and the "Follows keyboard" line
                                    // both change when a language comes off.
                                    refreshLanguageChoices()
                                } label: {
                                    Text(name(for: id))
                                }
                            }
                        }
                    }
                    Divider()
                    ForEach(options) { option in
                        Button {
                            dictation.setDictationLanguage(option.id)
                        } label: {
                            if dictation.dictationLanguage == option.id {
                                Label(option.name, systemImage: "checkmark")
                            } else {
                                Text(option.name)
                            }
                        }
                    }
                } label: {
                    LanguageMenuValue(value: selectedName)
                }
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 48)
            if listsOnDeviceLanguages {
                Text("The languages Apple's on-device speech has on this iPhone. One shows here once its keyboard and dictation are on in Settings › General › Keyboard.")
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 16)
                    .padding(.bottom, 12)
            }
        }
        .onChange(of: dictation.dictationLanguage) { _, _ in
            refreshLanguageChoices()
        }
        // The keyboard may have switched the language while Hex was away: re-read the
        // choices and what Automatic follows whenever Hex comes back to the foreground.
        .task(id: scenePhase) {
            guard scenePhase == .active else { return }
            await reloadLanguages()
        }
    }

    @MainActor
    private func reloadLanguages() async {
        dictation.followDictationLanguageSetting()
        guard #available(iOS 26.0, *) else {
            // Apple's on-device recognizer (OnDeviceSpeechTranscriber); the keyboard's quick
            // switch is iOS 26's.
            options = OnDeviceSpeechLanguages.options()
            listsOnDeviceLanguages = true
            refreshLanguageChoices()
            automaticName = OnDeviceSpeechLanguages.automatic().map { SpeechLanguagePicker.displayName(for: $0) }
            return
        }
        options = await AppleSpeechLanguages.options()
        refreshLanguageChoices()
        automaticName = await AppleSpeechLanguages.automatic().map {
            HinglishSettings().name(for: $0.identifier) ?? SpeechLanguagePicker.displayName(for: $0.identifier)
        }
    }

    @MainActor
    private func refreshLanguageChoices() {
        guard #available(iOS 26.0, *) else {
            // The keyboard's quick switch is iOS 26's.
            keyboardChoices = []
            followsKeyboard = nil
            return
        }
        let settings = LanguageSettings()
        keyboardChoices = settings.keyboardLanguageChoices()
        followsKeyboard = followsKeyboardSummary(settings)
    }

    /// "Follows keyboard: English (US) · Spanish (US)" while Automatic dictates in the space
    /// bar's language: each keyboard language's dictation language, short.
    private func followsKeyboardSummary(_ settings: LanguageSettings) -> String? {
        guard settings.keyboardDictationLanguage() != nil else { return nil }
        let preferred = Locale.preferredLanguages
        let region = Locale.current.region?.identifier
        var names: [String] = []
        for language in settings.keyboardLanguages {
            guard let identifier = settings.dictationLanguage(
                for: language,
                preferred: preferred,
                region: region
            ) else { continue }
            names.append(SpeechLanguagePicker.shortName(for: identifier))
        }
        guard !names.isEmpty else { return nil }
        return String(localized: "Follows keyboard: \(names.joined(separator: " · "))")
    }

    private var selectedName: String {
        guard let id = dictation.dictationLanguage else { return String(localized: "Automatic") }
        return name(for: id)
    }

    private func name(for id: String) -> String {
        if let hinglish = HinglishSettings().name(for: id) { return hinglish }
        return SpeechLanguagePicker.shortNames(for: options.map(\.id))[id]
            ?? SpeechLanguagePicker.displayName(for: id)
    }
}

/// What Automatic picks on this iPhone now: the dictation language's layout.
@MainActor
private func automaticKeyboardLanguage() -> KeyboardLanguage {
    KeyboardLanguage.automatic(
        dictationLanguage: LanguageSettings().effectiveDictationLanguage,
        inputModes: UITextInputMode.activeInputModes.compactMap(\.primaryLanguage),
        preferredLanguages: Locale.preferredLanguages
    )
}

/// Settings › Keyboard › Languages (while the full keyboard is on): Automatic with the
/// layout it picks, or the languages chosen, in order. Opens `KeyboardLanguagesPage`.
struct KeyboardLanguagesRow: View {
    @State private var chosen = LanguageSettings().keyboardLanguages
    @State private var showsPage = SettingsSnapshotFixture.opensKeyboardLanguages

    private var summary: String {
        guard !chosen.isEmpty else {
            return String(localized: "\(automaticKeyboardLanguage().displayName), like your dictation language")
        }
        return chosen.map(\.displayName).joined(separator: ", ")
    }

    var body: some View {
        VStack(spacing: 0) {
            LanguageRowDivider()
            Button {
                showsPage = true
            } label: {
                HStack(spacing: 12) {
                    VStack(alignment: .leading, spacing: 2) {
                        Text("Languages")
                            .font(.system(size: 15, weight: .regular))
                            .foregroundStyle(HexTheme.textBase)
                        Text(summary)
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                            .lineLimit(1)
                    }
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
        .navigationDestination(isPresented: $showsPage) {
            KeyboardLanguagesPage()
        }
        .onAppear { chosen = LanguageSettings().keyboardLanguages }
        .onChange(of: showsPage) { _, shown in
            if !shown { chosen = LanguageSettings().keyboardLanguages }
        }
    }
}

/// Settings › Keyboard › Languages: Automatic (the dictation language's layout, as before),
/// or one or more languages ticked and dragged into order; with two or more the space bar
/// switches between them, and Switch dictation too makes dictation follow. The iPhone's own
/// keyboards' and languages' layouts are offered first. Ticking a language while Automatic
/// is on keeps Automatic's language beside it; unticking the last goes back to Automatic.
struct KeyboardLanguagesPage: View {
    @State private var chosen = LanguageSettings().keyboardLanguages
    @State private var switchDictation = LanguageSettings().dictationFollowsKeyboard
    @State private var letterLayouts = LanguageSettings().letterLayouts
    private let automatic = automaticKeyboardLanguage()
    private let suggested = KeyboardLanguage.suggested(
        inputModes: UITextInputMode.activeInputModes.compactMap(\.primaryLanguage),
        preferred: Locale.preferredLanguages
    )

    var body: some View {
        let onPhone = suggested.filter { !chosen.contains($0) }
        let others = KeyboardLanguage.allCases.filter { !chosen.contains($0) && !onPhone.contains($0) }
        List {
            Section {
                LanguageChoiceRow(
                    title: String(localized: "Automatic"),
                    detail: String(localized: "\(automatic.displayName), like your dictation language"),
                    isChosen: chosen.isEmpty
                ) {
                    choose([])
                }
            }

            if !chosen.isEmpty {
                Section {
                    ForEach(chosen, id: \.self) { language in
                        languageRow(language)
                    }
                    .onMove { from, to in
                        var next = chosen
                        next.move(fromOffsets: from, toOffset: to)
                        choose(next)
                    }
                    // A row added in edit mode gets no drag handle until its rows are rebuilt.
                    .id(chosen.map(\.rawValue).joined(separator: ","))
                } footer: {
                    if chosen.count > 1 {
                        Text("Swipe the space bar to switch. Hold it, then slide, to move the cursor.")
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
            }

            if !arranged.isEmpty {
                Section {
                    ForEach(arranged, id: \.self) { language in
                        layoutRow(language)
                    }
                } header: {
                    LanguageSectionHeader(title: "Layout")
                } footer: {
                    Text("Moves the letters. Accents and autocorrect stay the language's.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                }
            }

            if !onPhone.isEmpty {
                Section {
                    ForEach(onPhone, id: \.self) { language in
                        languageRow(language)
                    }
                } header: {
                    LanguageSectionHeader(title: "On this iPhone")
                }
            }

            if !others.isEmpty {
                Section {
                    ForEach(others, id: \.self) { language in
                        languageRow(language)
                    }
                } header: {
                    LanguageSectionHeader(title: "More languages")
                }
            }

            if chosen.count > 1, #available(iOS 26.0, *) {
                Section {
                    Toggle(isOn: $switchDictation) {
                        VStack(alignment: .leading, spacing: 2) {
                            Text("Switch dictation too")
                                .font(.system(size: 15, weight: .regular))
                                .foregroundStyle(HexTheme.textBase)
                            Text("Dictate in the keyboard's language")
                                .font(.system(size: 12))
                                .foregroundStyle(HexTheme.textMuted)
                        }
                    }
                    .tint(HexTheme.success)
                    .listRowBackground(HexTheme.bgBase)
                }
            }
        }
        // Always in edit mode, so the chosen languages show their drag handles.
        .environment(\.editMode, .constant(.active))
        .listSectionSpacing(12)
        .scrollContentBackground(.hidden)
        .background(HexTheme.bgDeep.ignoresSafeArea())
        .navigationTitle("Languages")
        .navigationBarTitleDisplayMode(.inline)
        .toolbarBackground(HexTheme.bgDeep, for: .navigationBar)
        .onChange(of: switchDictation) { _, on in
            LanguageSettings().dictationFollowsKeyboard = on
        }
        .onAppear {
            chosen = LanguageSettings().keyboardLanguages
            switchDictation = LanguageSettings().dictationFollowsKeyboard
            letterLayouts = LanguageSettings().letterLayouts
        }
    }

    /// The languages whose keys the keyboard shows, with more than one layout to pick: each
    /// chosen one (Automatic's while none is), but only the first of English and Spanish
    /// together, whose keys both type on (`KeyboardLanguageList.layout`).
    private var arranged: [KeyboardLanguage] {
        let typing = chosen.isEmpty ? [automatic] : chosen
        var seen = Set<KeyboardLanguage>()
        return typing
            .map { KeyboardLanguageList(languages: typing, current: $0).layout }
            .filter { $0.letterLayouts.count > 1 && seen.insert($0).inserted }
    }

    private func layoutRow(_ language: KeyboardLanguage) -> some View {
        let current = language.offered(letterLayouts[language.rawValue].flatMap(KeyboardLetterLayout.init(rawValue:)))
        return HStack(spacing: 12) {
            Text(language.displayName)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(HexTheme.textBase)
            Spacer(minLength: 8)
            Menu {
                ForEach(language.letterLayouts, id: \.self) { letters in
                    Button {
                        LanguageSettings().setLetterLayout(letters, for: language)
                        letterLayouts = LanguageSettings().letterLayouts
                    } label: {
                        if letters == current {
                            Label(language.layoutName(letters), systemImage: "checkmark")
                        } else {
                            Text(language.layoutName(letters))
                        }
                    }
                }
            } label: {
                LanguageMenuValue(value: language.layoutName(current))
            }
        }
        .frame(minHeight: 34)
        .listRowBackground(HexTheme.bgBase)
        .listRowSeparatorTint(HexTheme.borderWeak)
    }

    private func languageRow(_ language: KeyboardLanguage) -> some View {
        // The language's own name, and under it its name in the iPhone's language when that differs.
        let local = Locale.current.localizedString(forIdentifier: language == .french ? "fr-FR" : language.rawValue)
        return LanguageChoiceRow(
            title: language.displayName,
            detail: local.flatMap { $0.caseInsensitiveCompare(language.displayName) == .orderedSame ? nil : $0 },
            isChosen: chosen.contains(language)
        ) {
            toggle(language)
        }
    }

    private func toggle(_ language: KeyboardLanguage) {
        if let index = chosen.firstIndex(of: language) {
            var next = chosen
            next.remove(at: index)
            choose(next)
        } else if chosen.isEmpty {
            // From Automatic: keep the language it was typing in, and add this one.
            choose(language == automatic ? [language] : [automatic, language])
        } else {
            choose(chosen + [language])
        }
    }

    private func choose(_ languages: [KeyboardLanguage]) {
        chosen = languages
        LanguageSettings().keyboardLanguages = languages
    }
}

/// A row that is ticked or not: a filled check circle when chosen, an empty one otherwise.
private struct LanguageChoiceRow: View {
    let title: String
    let detail: String?
    let isChosen: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: isChosen ? "checkmark.circle.fill" : "circle")
                    .font(.system(size: 19, weight: .regular))
                    .foregroundStyle(isChosen ? HexTheme.success : HexTheme.textFaint)
                VStack(alignment: .leading, spacing: 2) {
                    Text(title)
                        .font(.system(size: 15, weight: .regular))
                        .foregroundStyle(HexTheme.textBase)
                    if let detail {
                        Text(detail)
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                            .lineLimit(1)
                    }
                }
                Spacer(minLength: 0)
            }
            .frame(minHeight: 34)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityAddTraits(isChosen ? .isSelected : [])
        .listRowBackground(HexTheme.bgBase)
        .listRowSeparatorTint(HexTheme.borderWeak)
    }
}

/// A section's small label, as the settings pages' `SettingsSectionHeader`.
private struct LanguageSectionHeader: View {
    let title: LocalizedStringKey

    var body: some View {
        Text(title)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(HexTheme.textMuted)
            .textCase(nil)
    }
}

/// The settings card's row divider.
private struct LanguageRowDivider: View {
    var body: some View {
        Rectangle()
            .fill(HexTheme.borderWeak)
            .frame(height: 0.5)
            .padding(.leading, 16)
    }
}

/// A menu's current value with the up-down chevron, as the settings card's other menus show it.
private struct LanguageMenuValue: View {
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
