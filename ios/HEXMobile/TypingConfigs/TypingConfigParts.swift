import SwiftUI

/// A typing config's parts as the review sheet and its page in Settings show them: what it
/// changes, grouped, each part with its own switch; key blocks for another keyboard; what Hex
/// skipped and why; and the file's note. A rendering of `TypingConfigReview`.
struct TypingConfigParts: View {
    let review: TypingConfigReview
    /// The parts switched off, by `Item.path`.
    @Binding var partsOff: Set<String>
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var expanded: Set<TypingConfigGroup> = []
    /// Rows a group shows before Show all.
    static let shownRows = 6

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            if groups.isEmpty {
                Self.plainCard(String(localized: "Nothing in this file applies to your keyboard. What Hex skipped is listed below."))
            }

            ForEach(groups, id: \.0) { group, items in
                SettingsSectionHeader(title: LocalizedStringKey(group.title))
                groupCard(group, items)
            }

            if !otherKeyboards.isEmpty {
                SettingsSectionHeader(title: "For another keyboard")
                SettingsCard {
                    // Nobody's keyboard is recorded until the Hex keyboard has been used with
                    // the experiment on and Full Access (TypingConfigSettings.keyboards).
                    if noKeyboardYet {
                        Self.textRow(
                            title: String(localized: "Hex hasn't seen your keyboard yet"),
                            lines: [String(localized: "Key positions are for one keyboard. Type with the Hex keyboard for a moment, then open Hex again: those made for it apply then.")]
                        )
                        SettingsDivider()
                    }
                    ForEach(Array(otherKeyboards.enumerated()), id: \.element.block) { n, check in
                        if n > 0 { SettingsDivider() }
                        Self.textRow(
                            title: String(localized: "Key positions for another keyboard"),
                            lines: TypingConfigContextWords.differences(check) + [otherKeyboardParts(check.block)]
                        )
                    }
                }
            }

            if !skipped.isEmpty {
                SettingsSectionHeader(title: "Skipped")
                SettingsCard {
                    ForEach(Array(skipped.enumerated()), id: \.element.0) { n, entry in
                        if n > 0 { SettingsDivider() }
                        skippedRow(entry.0, entry.1)
                    }
                }
            }

            if let note = review.note, !note.isEmpty {
                SettingsSectionHeader(title: "From the file")
                Self.plainCard(note, muted: false)
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: expanded)
    }

    /// Parts that apply and are switched on.
    static func switchedOn(_ review: TypingConfigReview, partsOff: Set<String>) -> Int {
        review.items.filter { $0.outcome.applies && $0.part != .note && !partsOff.contains($0.path) }.count
    }

    private var groups: [(TypingConfigGroup, [TypingConfigReview.Item])] {
        let applying = review.items.filter { $0.outcome.applies }
        return TypingConfigGroup.allCases.compactMap { group in
            let items = applying.filter { TypingConfigGroup($0.part) == group }
            return items.isEmpty ? nil : (group, items)
        }
    }

    private var skipped: [(TypingConfigReview.Reason, [TypingConfigReview.Item])] {
        var byReason: [TypingConfigReview.Reason: [TypingConfigReview.Item]] = [:]
        for item in review.items {
            // Another keyboard's parts are listed under For another keyboard.
            if case .skipped(let reason) = item.outcome, reason != .forAnotherKeyboard { byReason[reason, default: []].append(item) }
        }
        return byReason.keys.sorted().map { ($0, byReason[$0] ?? []) }
    }

    private var noKeyboardYet: Bool { TypingConfigSettings().keyboards.isEmpty }

    private var otherKeyboards: [TypingConfigReview.ContextCheck] {
        review.contexts.filter { !$0.matches }
    }

    private func toggle(_ items: [TypingConfigReview.Item], on: Bool) {
        for item in items {
            if on { partsOff.remove(item.path) } else { partsOff.insert(item.path) }
        }
    }

    private func groupCard(_ group: TypingConfigGroup, _ items: [TypingConfigReview.Item]) -> some View {
        let showsAll = expanded.contains(group) || items.count <= Self.shownRows + 1
        let shown = showsAll ? items : Array(items.prefix(Self.shownRows))
        let allOn = items.allSatisfy { !partsOff.contains($0.path) }
        return SettingsCard {
            Text(group.detail)
                .font(.system(size: 12))
                .foregroundStyle(HexTheme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.horizontal, 16)
                .padding(.top, 12)
                .padding(.bottom, 4)
            // A long list gets one switch for all of it too.
            if items.count > Self.shownRows {
                SettingsToggleRow(
                    title: String(localized: "All \(items.count)"),
                    isOn: Binding(get: { allOn }, set: { toggle(items, on: $0) })
                )
            }
            ForEach(Array(shown.enumerated()), id: \.element.path) { n, item in
                if n > 0 || items.count > Self.shownRows { SettingsDivider() }
                itemRow(item)
            }
            if !showsAll {
                SettingsDivider()
                Button {
                    expanded.insert(group)
                } label: {
                    Text("Show all \(items.count)")
                        .font(.system(size: 15))
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                        .padding(.leading, 16)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
    }

    private func itemRow(_ item: TypingConfigReview.Item) -> some View {
        let words = TypingConfigItemWords(item, review: review)
        var lines: [String] = []
        if let detail = words.detail { lines.append(detail) }
        if let asked = TypingConfigItemWords.asked(item) { lines.append(asked) }
        if let claim = TypingConfigItemWords.evidence(item, review: review) { lines.append(claim) }
        return Toggle(isOn: Binding(
            get: { !partsOff.contains(item.path) },
            set: { toggle([item], on: $0) }
        )) {
            VStack(alignment: .leading, spacing: 2) {
                Text(words.title)
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textBase)
                ForEach(lines, id: \.self) { line in
                    Text(line)
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
        .tint(HexTheme.success)
        .padding(.horizontal, 16)
        .padding(.vertical, 8)
        .frame(minHeight: 50)
    }

    /// "Aim for O and O, bottom edge: kept in the file, not applied."
    private func otherKeyboardParts(_ block: Int) -> String {
        let labels = review.items.compactMap { item -> String? in
            switch item.part {
            case .aim(block, _), .zone(block, _): TypingConfigItemWords.label(item, review: review)
            default: nil
            }
        }
        return labels.isEmpty
            ? String(localized: "Kept in the file, not applied.")
            : String(localized: "\(labels.formatted(.list(type: .and))): kept in the file, not applied.")
    }

    private func skippedRow(_ reason: TypingConfigReview.Reason, _ items: [TypingConfigReview.Item]) -> some View {
        let labels = items.prefix(4).map { TypingConfigItemWords.label($0, review: review) }
        let more = items.count - labels.count
        let list = labels.joined(separator: ", ") + (more > 0 ? String(localized: " and \(more) more") : "")
        return Self.textRow(title: reason.sentence, lines: [list])
    }

    static func textRow(title: String, lines: [String]) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(title)
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textBase)
                .fixedSize(horizontal: false, vertical: true)
            ForEach(lines, id: \.self) { line in
                Text(line)
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    static func plainCard(_ text: String, muted: Bool = true) -> some View {
        Text(text)
            .font(.system(size: 13))
            .foregroundStyle(muted ? HexTheme.textMuted : HexTheme.textBase)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
    }
}

/// Needs Full Access: why the keyboard can't use a config yet and where to turn it on.
struct TypingConfigFullAccessCard: View {
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            VStack(alignment: .leading, spacing: 3) {
                Text("Needs Full Access")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
                Text("The keyboard can only use a typing config with Full Access. Turn on Allow Full Access in Settings › Apps › Hex › Keyboards, then apply.")
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Button {
                if let url = URL(string: UIApplication.openSettingsURLString) { openURL(url) }
            } label: {
                HStack(spacing: 5) {
                    Text("Open Settings")
                    Image(systemName: "arrow.up.right")
                        .font(.system(size: 11, weight: .semibold))
                }
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textBase)
                .padding(.horizontal, 12)
                .frame(height: 30)
                .background(Color.white.opacity(0.09), in: Capsule())
            }
            .buttonStyle(.plain)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(14)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
    }
}
