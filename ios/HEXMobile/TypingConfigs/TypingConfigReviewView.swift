import SwiftUI
import UIKit

/// A typing config's review: what it changes in plain words, each part with its own switch,
/// what Hex skipped and why, the file's note as plain text, then Apply or Not now. A rendering
/// of `TypingConfigReview`, which decides everything shown here.
struct TypingConfigReviewView: View {
    let arrival: TypingConfigArrival
    let data: Data
    /// Done with this file: the sheet shows the next one, or closes.
    let done: () -> Void
    @Environment(\.openURL) private var openURL
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var review: TypingConfigReview
    @State private var partsOff: Set<String>
    @State private var expanded: Set<TypingConfigGroup> = []
    @State private var appliedNow = false
    @State private var failure: String?
    private let existing: TypingConfigEntry?
    private let isActive: Bool
    private let historyFull: Bool
    private let fullAccessOn: Bool
    /// Rows a group shows before Show all.
    private static let shownRows = 6

    init(arrival: TypingConfigArrival, data: Data, keyboard: KeyboardLayout, fullAccessOn: Bool, done: @escaping () -> Void) {
        self.arrival = arrival
        self.data = data
        self.done = done
        self.fullAccessOn = fullAccessOn
        let review = TypingConfigReview(data: data, keyboard: keyboard)
        let history = TypingConfigHistory()
        existing = history.entry(review.contentHash)
        isActive = history.activeID == review.contentHash
        historyFull = existing == nil && history.isFull
        _review = State(initialValue: review)
        _partsOff = State(initialValue: Set(history.entry(review.contentHash)?.partsOff ?? []))
    }

    private var name: String { existing?.name ?? arrival.name }

    var body: some View {
        if let refused = review.refused {
            refusedView(refused)
        } else if appliedNow {
            appliedView
        } else {
            reviewView
        }
    }

    // MARK: - Review

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

    private var switchedOnCount: Int {
        review.items.filter { $0.outcome.applies && $0.part != .note && !partsOff.contains($0.path) }.count
    }

    private var otherKeyboards: [TypingConfigReview.ContextCheck] {
        review.contexts.filter { !$0.matches }
    }

    private var canApply: Bool {
        fullAccessOn && !historyFull && switchedOnCount > 0
    }

    private var reviewView: some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                TypingConfigSheetHeader(title: name, close: notNow)
                Text(subtitle)
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if !fullAccessOn {
                fullAccessCard
            }

            if groups.isEmpty {
                plainCard(String(localized: "Nothing in this file applies to your keyboard. What Hex skipped is listed below."))
            }

            ForEach(groups, id: \.0) { group, items in
                SettingsSectionHeader(title: LocalizedStringKey(group.title))
                groupCard(group, items)
            }

            if !otherKeyboards.isEmpty {
                SettingsSectionHeader(title: "For another keyboard")
                SettingsCard {
                    ForEach(Array(otherKeyboards.enumerated()), id: \.element.block) { n, check in
                        if n > 0 { SettingsDivider() }
                        textRow(
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

            if let note = review.file?.note.map(TypingConfigInbox.noteText), !note.isEmpty {
                SettingsSectionHeader(title: "From the file")
                plainCard(note, muted: false)
            }

            plainCard(String(localized: "Hex can't check this config on your typing yet. Once it's on, Hex watches how your typing goes and tells you if it seems worse. You can go back in Settings › Keyboard › Typing configs."))
                .padding(.top, 4)

            if historyFull {
                Text("Hex keeps \(TypingConfigHistory.limit) typing configs, and you've applied all of them. Delete one in Typing configs to keep this one.")
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.danger)
                    .fixedSize(horizontal: false, vertical: true)
            }
            if let failure {
                Text(failure)
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.danger)
                    .fixedSize(horizontal: false, vertical: true)
            }

            VStack(spacing: 4) {
                OnboardingPrimaryButton(title: isActive ? "Apply again" : "Apply", enabled: canApply, action: apply)
                OnboardingQuietButton(title: "Not now", action: notNow)
            }
            .padding(.top, 8)
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: expanded)
    }

    private var subtitle: String {
        var parts: [String] = [String(localized: "Typing config")]
        if let days = review.file?.madeFrom?.logDays, !days.isEmpty {
            parts.append(days.count == 1
                ? String(localized: "made from 1 day of typing")
                : String(localized: "made from \(days.count) days of typing"))
        }
        if isActive {
            parts.append(String(localized: "the one you use now"))
        } else if existing != nil {
            parts.append(String(localized: "opened before"))
        }
        return parts.joined(separator: " · ")
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
                    isOn: Binding(
                        get: { allOn },
                        set: { on in
                            for item in items {
                                if on { partsOff.remove(item.path) } else { partsOff.insert(item.path) }
                            }
                        }
                    )
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
            set: { on in if on { partsOff.remove(item.path) } else { partsOff.insert(item.path) } }
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
        return textRow(title: reason.sentence, lines: [list])
    }

    private func textRow(title: String, lines: [String]) -> some View {
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

    private func plainCard(_ text: String, muted: Bool = true) -> some View {
        Text(text)
            .font(.system(size: 13))
            .foregroundStyle(muted ? HexTheme.textMuted : HexTheme.textBase)
            .fixedSize(horizontal: false, vertical: true)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 16)
            .padding(.vertical, 12)
            .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
    }

    private var fullAccessCard: some View {
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

    // MARK: - Choices

    /// Keeps the file in the history, unapplied (or as it was, if it was kept before).
    private func notNow() {
        if review.refused == nil {
            _ = TypingConfigHistory().add(data, hash: review.contentHash, name: arrival.name)
        }
        done()
    }

    private func apply() {
        let history = TypingConfigHistory()
        let entry: TypingConfigEntry
        switch history.add(data, hash: review.contentHash, name: arrival.name) {
        case .added(let added): entry = added
        case .existing(let kept): entry = kept
        case .full: return
        }
        do {
            try TypingConfigActivation.apply(entry, review: review, partsOff: partsOff, history: history)
            appliedNow = true
        } catch {
            failure = String(localized: "Hex couldn't save this config. Nothing has changed.")
        }
    }

    // MARK: - After

    private var appliedView: some View {
        VStack(alignment: .leading, spacing: 16) {
            TypingConfigSheetHeader(title: String(localized: "Typing config applied"), close: done)
            Text("“\(name)” takes effect the next time the keyboard opens. To go back, open Settings › Keyboard › Typing configs.")
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
            OnboardingPrimaryButton(title: "Done", action: done)
                .padding(.top, 4)
        }
    }

    private func refusedView(_ reason: TypingConfigReview.Reason) -> some View {
        VStack(alignment: .leading, spacing: 16) {
            TypingConfigSheetHeader(title: String(localized: "This isn't a typing config"), close: done)
            Text("\(reason.sentence) “\(arrival.name)” wasn't opened, and nothing has changed.")
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textMuted)
                .fixedSize(horizontal: false, vertical: true)
            OnboardingPrimaryButton(title: "Close", action: done)
                .padding(.top, 4)
        }
    }
}
