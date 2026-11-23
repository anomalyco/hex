import SwiftUI

/// Settings › Keyboard › Typing configs, beside Vocabulary while the experiment is on: the
/// config in use and the history behind it.
struct KeyboardTypingConfigsRow: View {
    let fullAccessOn: Bool
    @State private var showsPage = false
    @State private var activeName: String?

    var body: some View {
        Button {
            showsPage = true
        } label: {
            HStack(spacing: 12) {
                SettingsRowLabel(
                    title: String(localized: "Typing configs"),
                    detail: activeName.map { String(localized: "Using “\($0)”") } ?? String(localized: "Hex defaults")
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
        .navigationDestination(isPresented: $showsPage) {
            TypingConfigsPage(fullAccessOn: fullAccessOn)
        }
        .onAppear(perform: refresh)
        .onChange(of: showsPage) { _, shown in
            if !shown { refresh() }
        }
    }

    private func refresh() {
        activeName = TypingConfigHistory().active?.name
    }
}

/// Every typing config Hex was given, newest first under Hex defaults: one tap uses one (going
/// back deletes nothing), a row opens its parts, and Delete all clears the history.
struct TypingConfigsPage: View {
    let fullAccessOn: Bool
    @State private var entries: [TypingConfigEntry] = []
    @State private var activeID: String?
    @State private var bestID: String?
    @State private var confirmingDeleteAll = false
    @State private var failure: String?
    @State private var alertsOff = TypingConfigHealth().alertsOff
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        SettingsPageScaffold(title: "Typing configs") {
            if !fullAccessOn {
                TypingConfigFullAccessCard()
            }

            SettingsCard {
                row(title: String(localized: "Hex defaults"), detail: String(localized: "No typing config"), isActive: activeID == nil) {
                    TypingConfigActivation.useDefaults(history: TypingConfigHistory())
                    reload()
                }
                ForEach(entries) { entry in
                    SettingsDivider()
                    entryRow(entry)
                }
            }

            Text(entries.isEmpty
                ? String(localized: "No typing configs yet. Export your typing logs for your agent from Settings › Privacy › Typing logs, then open the file it makes.")
                : String(localized: "Going back deletes nothing. Hex keeps your last \(TypingConfigHistory.limit) typing configs."))
                .font(.system(size: 12))
                .foregroundStyle(HexTheme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 16)

            if let failure {
                Text(failure)
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.danger)
                    .padding(.horizontal, 16)
            }

            SettingsCard {
                if TypingConfigHealth().comparison != nil {
                    Button {
                        TypingConfigWatch.shared.openCompare()
                    } label: {
                        SettingsLinkLabel(title: String(localized: "Compare with the one before"))
                    }
                    .buttonStyle(.plain)
                    SettingsDivider()
                }
                SettingsToggleRow(
                    title: String(localized: "Alerts"),
                    detail: String(localized: "A notice in the keyboard, or a notification, if typing seems worse with a new config"),
                    isOn: Binding(get: { !alertsOff }, set: { on in
                        TypingConfigHealth().alertsOff = !on
                        alertsOff = !on
                    })
                )
            }

            if !entries.isEmpty {
                SettingsCard {
                    Button {
                        confirmingDeleteAll = true
                    } label: {
                        Text("Delete all typing configs")
                            .font(.system(size: 15))
                            .foregroundStyle(HexTheme.danger)
                            .frame(maxWidth: .infinity, minHeight: 50, alignment: .leading)
                            .padding(.horizontal, 16)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .confirmationDialog("Delete all typing configs?", isPresented: $confirmingDeleteAll, titleVisibility: .visible) {
                        Button("Delete all", role: .destructive) {
                            let history = TypingConfigHistory()
                            TypingConfigActivation.useDefaults(history: history)
                            history.deleteAll()
                            TypingConfigHealth().deleteAll()
                            reload()
                        }
                    } message: {
                        Text("The keyboard goes back to Hex defaults, and every typing config is deleted from this iPhone.")
                    }
                }
                .padding(.top, 8)
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: entries)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: activeID)
        .onAppear(perform: reload)
    }

    private func reload() {
        let history = TypingConfigHistory()
        entries = history.entries
        activeID = history.activeID
        bestID = history.bestID
    }

    private func entryRow(_ entry: TypingConfigEntry) -> some View {
        HStack(spacing: 10) {
            NavigationLink {
                TypingConfigDetailPage(entryID: entry.id, fullAccessOn: fullAccessOn)
                    .onDisappear(perform: reload)
            } label: {
                HStack(spacing: 8) {
                    SettingsRowLabel(title: entry.name, detail: Self.status(entry, active: entry.id == activeID, best: entry.id == bestID))
                    Spacer(minLength: 4)
                    Image(systemName: "chevron.right")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(HexTheme.textFaint)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            if entry.id == activeID {
                Image(systemName: "checkmark")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(HexTheme.textBase)
                    .frame(width: 52)
                    .accessibilityLabel("In use")
            } else {
                useButton(entry)
            }
        }
        .padding(.leading, 16)
        .padding(.trailing, 12)
        .padding(.vertical, 10)
        .frame(minHeight: 50)
    }

    private func useButton(_ entry: TypingConfigEntry) -> some View {
        Button {
            do {
                try TypingConfigActivation.goBack(to: entry, history: TypingConfigHistory())
                failure = nil
            } catch {
                failure = String(localized: "Hex couldn't use “\(entry.name)”. Nothing has changed.")
            }
            reload()
        } label: {
            Text("Use")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(fullAccessOn ? HexTheme.textBase : HexTheme.textFaint)
                .padding(.horizontal, 12)
                .frame(height: 30)
                .background(Color.white.opacity(fullAccessOn ? 0.09 : 0.05), in: Capsule())
                .frame(minWidth: 52, minHeight: 44)
                .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!fullAccessOn)
        .accessibilityLabel("Use \(entry.name)")
    }

    private func row(title: String, detail: String, isActive: Bool, use: @escaping () -> Void) -> some View {
        Button(action: use) {
            HStack(spacing: 10) {
                SettingsRowLabel(title: title, detail: detail)
                Spacer(minLength: 8)
                if isActive {
                    Image(systemName: "checkmark")
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(HexTheme.textBase)
                        .frame(width: 52)
                }
            }
            .padding(.leading, 16)
            .padding(.trailing, 12)
            .frame(minHeight: 56)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(isActive)
    }

    /// "In use · applied 23 Nov", "Applied 22 Nov", "Not applied · opened 23 Nov".
    static func status(_ entry: TypingConfigEntry, active: Bool, best: Bool) -> String {
        var parts: [String] = []
        if active {
            parts.append(String(localized: "In use"))
        }
        if best {
            parts.append(String(localized: "Best so far"))
        }
        if let applied = entry.appliedAt {
            let date = applied.formatted(.dateTime.month(.abbreviated).day())
            parts.append(active ? String(localized: "applied \(date)") : String(localized: "Applied \(date)"))
        } else {
            parts.append(String(localized: "Not applied"))
            parts.append(String(localized: "opened \(entry.addedAt.formatted(.dateTime.month(.abbreviated).day()))"))
        }
        return parts.joined(separator: " · ")
    }
}

/// One typing config: what it is, Use or Hex defaults, its parts with their switches (applied at
/// once while it's the one in use), Rename, Share and Delete.
struct TypingConfigDetailPage: View {
    let entryID: String
    let fullAccessOn: Bool
    @Environment(\.dismiss) private var dismiss
    @State private var entry: TypingConfigEntry?
    @State private var review: TypingConfigReview?
    @State private var partsOff: Set<String> = []
    @State private var isActive = false
    @State private var renaming = false
    @State private var newName = ""
    @State private var confirmingDelete = false
    @State private var failure: String?
    @State private var basedOn: String?
    @State private var fromAnotherPhone = false

    var body: some View {
        SettingsPageScaffold(title: LocalizedStringKey(entry?.name ?? "")) {
            if let entry, let review {
                Text(facts(entry, review))
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 16)

                if !fullAccessOn && !isActive {
                    TypingConfigFullAccessCard()
                }

                SettingsCard {
                    if isActive {
                        actionRow(String(localized: "Use Hex defaults instead"), icon: "arrow.uturn.backward") {
                            TypingConfigActivation.useDefaults(history: TypingConfigHistory())
                            load()
                        }
                    } else {
                        actionRow(String(localized: "Use this config"), icon: "checkmark.circle", enabled: fullAccessOn) {
                            use(entry)
                        }
                    }
                    SettingsDivider(leading: 50)
                    actionRow(String(localized: "Rename"), icon: "pencil") {
                        newName = entry.name
                        renaming = true
                    }
                    SettingsDivider(leading: 50)
                    actionRow(String(localized: "Share"), icon: "square.and.arrow.up") {
                        if let url = TypingConfigHistory().shareableCopy(entry.id) { ShareSheet.present([url]) }
                    }
                }

                if let failure {
                    Text(failure)
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.danger)
                        .padding(.horizontal, 16)
                }

                if isActive {
                    Text("Switches here take effect the next time the keyboard opens.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .padding(.horizontal, 16)
                        .padding(.top, 4)
                }

                TypingConfigParts(review: review, partsOff: $partsOff)

                SettingsCard {
                    Button {
                        confirmingDelete = true
                    } label: {
                        Text("Delete this config")
                            .font(.system(size: 15))
                            .foregroundStyle(HexTheme.danger)
                            .frame(maxWidth: .infinity, minHeight: 50, alignment: .leading)
                            .padding(.horizontal, 16)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .confirmationDialog("Delete “\(entry.name)”?", isPresented: $confirmingDelete, titleVisibility: .visible) {
                        Button("Delete", role: .destructive) {
                            let history = TypingConfigHistory()
                            if history.activeID == entry.id { TypingConfigActivation.useDefaults(history: history) }
                            history.delete(entry.id)
                            TypingConfigHealth().forget(entry.id)
                            dismiss()
                        }
                    } message: {
                        Text(isActive
                            ? String(localized: "The keyboard goes back to Hex defaults, and this config is deleted from this iPhone.")
                            : String(localized: "This config is deleted from this iPhone."))
                    }
                }
                .padding(.top, 8)
            }
        }
        .alert("Rename typing config", isPresented: $renaming) {
            TextField("Name", text: $newName)
            Button("Cancel", role: .cancel) {}
            Button("Rename") {
                TypingConfigHistory().rename(entryID, to: newName)
                load()
            }
        }
        .onChange(of: partsOff) { _, off in
            switchesChanged(off)
        }
        .onAppear(perform: load)
    }

    private func load() {
        let history = TypingConfigHistory()
        guard let entry = history.entry(entryID), let data = history.data(entryID) else {
            self.entry = nil
            return
        }
        self.entry = entry
        let review = TypingConfigReview(data: data, keyboards: TypingConfigSettings().keyboards)
        self.review = review
        if partsOff != Set(entry.partsOff) { partsOff = Set(entry.partsOff) }
        isActive = history.activeID == entryID
        if let exportID = review.file?.madeFrom?.exportId {
            fromAnotherPhone = !history.exportIDs.contains(exportID)
        }
        basedOn = review.file?.basedOn.flatMap { id in
            history.entries.first { other in
                other.id != entryID && history.data(other.id).flatMap { try? JSONDecoder().decode(TypingConfigFile.self, from: $0) }?.id == id
            }?.name
        }
    }

    /// The parts' switches: kept with the config, and applied at once while it is in use.
    private func switchesChanged(_ off: Set<String>) {
        guard let entry, Set(entry.partsOff) != off else { return }
        let history = TypingConfigHistory()
        history.setPartsOff(entry.id, off.sorted())
        if history.activeID == entry.id, let review {
            try? TypingConfigActivation.apply(entry, review: review, partsOff: off, history: history)
        }
        self.entry = history.entry(entry.id)
    }

    private func use(_ entry: TypingConfigEntry) {
        do {
            try TypingConfigActivation.goBack(to: entry, history: TypingConfigHistory())
            failure = nil
        } catch {
            failure = String(localized: "Hex couldn't use “\(entry.name)”. Nothing has changed.")
        }
        load()
    }

    private func facts(_ entry: TypingConfigEntry, _ review: TypingConfigReview) -> String {
        var lines = [TypingConfigsPage.status(entry, active: isActive, best: TypingConfigHistory().bestID == entry.id)]
        let on = TypingConfigParts.switchedOn(review, partsOff: partsOff)
        lines.append(on == 1 ? String(localized: "1 change switched on") : String(localized: "\(on) changes switched on"))
        if let days = review.file?.madeFrom?.logDays, !days.isEmpty {
            lines.append(days.count == 1
                ? String(localized: "made from 1 day of typing")
                : String(localized: "made from \(days.count) days of typing"))
        }
        var text = lines.joined(separator: " · ")
        if let basedOn { text += "\n" + String(localized: "Based on “\(basedOn)”") }
        if fromAnotherPhone { text += "\n" + String(localized: "Made from another iPhone's typing") }
        return text
    }

    private func actionRow(_ title: String, icon: String, enabled: Bool = true, action: @escaping () -> Void) -> some View {
        Button(action: action) {
            HStack(spacing: 12) {
                Image(systemName: icon)
                    .font(.system(size: 15))
                    .foregroundStyle(enabled ? HexTheme.textMuted : HexTheme.textFaint)
                    .frame(width: 22)
                Text(title)
                    .font(.system(size: 15))
                    .foregroundStyle(enabled ? HexTheme.textBase : HexTheme.textFaint)
                Spacer()
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 50)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(!enabled)
    }
}
