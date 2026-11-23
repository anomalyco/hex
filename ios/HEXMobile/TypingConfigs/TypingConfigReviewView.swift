import SwiftUI

/// A typing config's review in the sheet a file opens: its parts (`TypingConfigParts`), Needs
/// Full Access before Apply, which kept config a new one replaces at the limit, then Apply or
/// Not now. A rendering of `TypingConfigReview`, which decides everything shown here.
struct TypingConfigReviewView: View {
    let arrival: TypingConfigArrival
    let data: Data
    /// Done with this file: the sheet shows the next one, or closes.
    let done: () -> Void
    @State private var review: TypingConfigReview
    @State private var partsOff: Set<String>
    @State private var appliedNow = false
    @State private var failure: String?
    @State private var asksNotifications = false
    private let existing: TypingConfigEntry?
    private let isActive: Bool
    /// The kept config keeping this one would remove (`TypingConfigHistory.limit`).
    private let replaces: TypingConfigEntry?
    private let fullAccessOn: Bool

    /// `keyboards`: the keyboards Hex's keys have run on (`TypingConfigSettings.keyboards`).
    init(arrival: TypingConfigArrival, data: Data, keyboards: [TypingConfigKeyboard], fullAccessOn: Bool, done: @escaping () -> Void) {
        self.arrival = arrival
        self.data = data
        self.done = done
        self.fullAccessOn = fullAccessOn
        let review = TypingConfigReview(data: data, keyboards: keyboards)
        let history = TypingConfigHistory()
        existing = history.entry(review.contentHash)
        isActive = history.activeID == review.contentHash
        replaces = history.removedByAdding(review.contentHash)
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

    private var canApply: Bool {
        fullAccessOn && TypingConfigParts.switchedOn(review, partsOff: partsOff) > 0
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
                TypingConfigFullAccessCard()
            }

            TypingConfigParts(review: review, partsOff: $partsOff)

            TypingConfigParts.plainCard(String(localized: "Hex can't check this config on your typing yet. Once it's on, Hex watches how your typing goes and tells you if it seems worse. You can go back in Settings › Keyboard › Typing configs."))
                .padding(.top, 4)

            if let replaces {
                Text("Hex keeps \(TypingConfigHistory.limit) typing configs. Keeping this one removes “\(replaces.name)”, the oldest one you're not using.")
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
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

    // MARK: - Choices

    /// Keeps the file in the history, unapplied (or as it was, if it was kept before).
    private func notNow() {
        if review.refused == nil {
            let history = TypingConfigHistory()
            if case .added(let entry) = history.add(data, hash: review.contentHash, name: arrival.name), !partsOff.isEmpty {
                history.setPartsOff(entry.id, partsOff.sorted())
            }
        }
        done()
    }

    private func apply() {
        let history = TypingConfigHistory()
        let entry: TypingConfigEntry
        switch history.add(data, hash: review.contentHash, name: arrival.name) {
        case .added(let added): entry = added
        case .existing(let kept): entry = kept
        case .failed:
            failure = String(localized: "Hex couldn't save this config. Nothing has changed.")
            return
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
            // Asked here, the first time a config is applied, never at first launch.
            if asksNotifications {
                TypingConfigNotificationAsk { asksNotifications = false }
            }
            OnboardingPrimaryButton(title: "Done", action: done)
                .padding(.top, 4)
        }
        .task { asksNotifications = await TypingConfigWatch.shouldAsk() }
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
