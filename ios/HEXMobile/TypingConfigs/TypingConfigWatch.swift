import Observation
import SwiftUI
import UIKit
import UserNotifications

/// Typing configs' regression watch in the app (design §7): the comparison a notification, the
/// keyboard's bar notice or Home's card opens; the start-up fallback the keyboard asked for; and
/// best so far. It only ever suggests going back.
@MainActor
@Observable
final class TypingConfigWatch {
    static let shared = TypingConfigWatch()

    /// The comparison sheet is up.
    var showsCompare = false
    /// Bumped when what Home's cards show may have changed.
    private(set) var revision = 0

    nonisolated static func handles(_ url: URL) -> Bool {
        url.scheme == TypingConfigHealth.notificationURL.scheme && url.host == TypingConfigHealth.notificationURL.host
    }

    func openCompare() {
        guard TypingConfigSettings().isEnabled else { return }
        showsCompare = true
    }

    /// When Hex comes to the front: carries out a start-up fallback the keyboard asked for, and
    /// marks best so far.
    func refresh(history: TypingConfigHistory = TypingConfigHistory(), store: TypingConfigStore = TypingConfigStore()) {
        guard store.settings.isEnabled else { return }
        let health = TypingConfigHealth(defaults: store.settings.defaults)
        let guardian = TypingConfigStartupGuard(defaults: store.settings.defaults, store: store)
        if let fallback = guardian.fallback, !fallback.handled {
            // Back to the config in use before the one that failed; the keyboard already left it.
            let previous = health.pair.flatMap { $0.current == fallback.key ? $0.previous : nil } ?? TypingConfigHealth.defaultsKey
            if previous != TypingConfigHealth.defaultsKey, let entry = history.entry(previous) {
                try? TypingConfigActivation.goBack(to: entry, history: history, store: store)
            } else {
                TypingConfigActivation.useDefaults(history: history, store: store)
            }
            store.settings.defaults?.removeObject(forKey: TypingConfigHealth.Keys.pair)
            guardian.markHandled()
        }
        // Best so far: the lowest trouble rate over enough words, once there are two to compare
        // (Hex defaults among them, which can't be marked).
        let counts = health.allCounts
        let measured = counts.filter { $0.value.words >= TypingConfigHealth.minimumWords }
        let lowest = measured.min { $0.value.rate < $1.value.rate }?.key
        let marked = measured.count >= 2 && lowest != TypingConfigHealth.defaultsKey && lowest.flatMap { history.entry($0) } != nil
        history.setBest(marked ? lowest : nil)
        revision += 1
    }

    // MARK: - Choices

    func goBack(_ comparison: TypingConfigHealth.Comparison) {
        let history = TypingConfigHistory()
        if comparison.previous != TypingConfigHealth.defaultsKey, let entry = history.entry(comparison.previous) {
            try? TypingConfigActivation.goBack(to: entry, history: history)
        } else {
            TypingConfigActivation.useDefaults(history: history)
        }
        resolve(comparison)
    }

    func keep(_ comparison: TypingConfigHealth.Comparison) { resolve(comparison) }

    func turnOff(_ comparison: TypingConfigHealth.Comparison) {
        TypingConfigActivation.useDefaults(history: TypingConfigHistory())
        TypingConfigSettings().isEnabled = false
        resolve(comparison)
    }

    func dismissHomeCard(_ key: String) {
        TypingConfigHealth().updateAlert(key) { $0.homeCardDismissed = true }
        revision += 1
    }

    func dismissFallbackCard() {
        TypingConfigStartupGuard().clearFallback()
        revision += 1
    }

    private func resolve(_ comparison: TypingConfigHealth.Comparison) {
        TypingConfigHealth().updateAlert(comparison.current) { $0.resolved = true }
        UNUserNotificationCenter.current().removeDeliveredNotifications(withIdentifiers: ["typing-config-worse-\(comparison.current.prefix(16))"])
        showsCompare = false
        revision += 1
    }

    // MARK: - Notification permission, asked when a config is applied

    static let askedKey = "typingConfig.askedNotifications"

    /// Whether the applied sheet asks: never asked before and the system hasn't been asked.
    static func shouldAsk() async -> Bool {
        guard UserDefaults(suiteName: KeyboardBridge.appGroup)?.bool(forKey: askedKey) != true else { return false }
        return await UNUserNotificationCenter.current().notificationSettings().authorizationStatus == .notDetermined
    }

    static func answered(allow: Bool) async {
        UserDefaults(suiteName: KeyboardBridge.appGroup)?.set(true, forKey: askedKey)
        guard allow else { return }
        _ = try? await UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .badge])
    }
}

/// Taps on the regression notification open the comparison; while Hex is in front it still shows.
final class TypingConfigNotificationDelegate: NSObject, UNUserNotificationCenterDelegate, Sendable {
    static let shared = TypingConfigNotificationDelegate()

    func userNotificationCenter(_ center: UNUserNotificationCenter, willPresent notification: UNNotification) async -> UNNotificationPresentationOptions {
        [.banner, .list]
    }

    func userNotificationCenter(_ center: UNUserNotificationCenter, didReceive response: UNNotificationResponse) async {
        guard let url = (response.notification.request.content.userInfo["url"] as? String).flatMap(URL.init(string:)),
              TypingConfigWatch.handles(url) else { return }
        await MainActor.run { TypingConfigWatch.shared.openCompare() }
    }
}

/// Home's cards: typing seems worse with the new config (Compare, ✕), and the keyboard's
/// start-up fallback, said once.
struct TypingConfigHomeCards: View {
    let watch: TypingConfigWatch

    var body: some View {
        let _ = watch.revision
        if TypingConfigSettings().isEnabled {
            if let fallback = TypingConfigStartupGuard().fallback, fallback.handled {
                card(
                    title: String(localized: "Hex went back to your last config"),
                    detail: String(localized: "The keyboard couldn't start with your new config, so Hex went back to the one before."),
                    button: nil,
                    dismiss: { watch.dismissFallbackCard() }
                )
            }
            if let worse = TypingConfigHealth().worse, !TypingConfigHealth().alert(worse.current).homeCardDismissed {
                card(
                    title: String(localized: "Typing may be worse with your new config"),
                    detail: String(localized: "Since you applied it you've undone or fixed \(worse.now.troubles) words in \(worse.now.words.formatted()), against \(worse.before.troubles) in \(worse.before.words.formatted()) before."),
                    button: String(localized: "Compare"),
                    dismiss: { watch.dismissHomeCard(worse.current) }
                )
            }
        }
    }

    private func card(title: String, detail: String, button: String?, dismiss: @escaping () -> Void) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "keyboard")
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
                    .padding(.top, 1)
                VStack(alignment: .leading, spacing: 3) {
                    Text(title)
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(HexTheme.textBase)
                    Text(detail)
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 4)
                Button(action: dismiss) {
                    Image(systemName: "xmark")
                        .font(.system(size: 11, weight: .semibold))
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(width: 28, height: 28)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Dismiss")
            }
            if let button {
                Button { watch.openCompare() } label: {
                    Text(button)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(HexTheme.textBase)
                        .padding(.horizontal, 12)
                        .frame(height: 30)
                        .background(Color.white.opacity(0.09), in: Capsule())
                }
                .buttonStyle(.plain)
                .padding(.leading, 25)
            }
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
    }
}

/// The comparison: this config's trouble rate against the one before it, the words counted and
/// the counts behind it, then Go back, Keep this one and Turn configs off.
struct TypingConfigCompareSheet: View {
    let watch: TypingConfigWatch
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                if let comparison = TypingConfigHealth().comparison {
                    content(comparison)
                } else {
                    TypingConfigSheetHeader(title: String(localized: "Nothing to compare yet"), close: { dismiss() })
                    Text("Hex compares a typing config with the one before it once you've typed with it for a while.")
                        .font(.system(size: 15))
                        .foregroundStyle(HexTheme.textMuted)
                        .fixedSize(horizontal: false, vertical: true)
                }
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
    }

    private func name(_ key: String) -> String {
        key == TypingConfigHealth.defaultsKey ? String(localized: "Hex defaults") : (TypingConfigHistory().entry(key)?.name ?? key)
    }

    @ViewBuilder
    private func content(_ c: TypingConfigHealth.Comparison) -> some View {
        TypingConfigSheetHeader(
            title: c.seemsWorse ? String(localized: "Typing may be worse with this config") : String(localized: "How typing is going"),
            close: { dismiss() }
        )
        Text(c.isReady
            ? String(localized: "Words you undid, kept as typed or fixed by hand, per 100 words you typed. Counts only: no words are kept.")
            : String(localized: "Hex compares once you've typed \(TypingConfigHealth.minimumWords) words with each. Counts only: no words are kept."))
            .font(.system(size: 13))
            .foregroundStyle(HexTheme.textMuted)
            .fixedSize(horizontal: false, vertical: true)

        HStack(alignment: .top, spacing: 10) {
            column(String(localized: "This config"), name(c.current), c.now)
            column(String(localized: "Before"), name(c.previous), c.before)
        }

        VStack(spacing: 4) {
            OnboardingPrimaryButton(title: LocalizedStringKey(String(localized: "Go back to “\(name(c.previous))”"))) { watch.goBack(c) }
            OnboardingQuietButton(title: "Keep this one") { watch.keep(c) }
            OnboardingQuietButton(title: "Turn typing configs off") { watch.turnOff(c) }
        }
        .padding(.top, 4)
        Text("Going back deletes nothing. You can switch again in Settings › Keyboard › Typing configs.")
            .font(.system(size: 12))
            .foregroundStyle(HexTheme.textMuted)
            .fixedSize(horizontal: false, vertical: true)
    }

    private func column(_ label: String, _ name: String, _ counts: TypingConfigHealth.Counts) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(label)
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
            Text(name)
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textBase)
                .lineLimit(2)
            Text(counts.rate.formatted(.number.precision(.fractionLength(1))))
                .font(.system(size: 28, weight: .semibold))
                .monospacedDigit()
                .foregroundStyle(HexTheme.textBase)
            Text("per 100 words")
                .font(.system(size: 12))
                .foregroundStyle(HexTheme.textMuted)
            SettingsDivider(leading: 0)
            row(String(localized: "Words typed"), counts.words)
            row(String(localized: "Corrections undone"), counts.undone + counts.keptTyped)
            row(String(localized: "Fixed by hand"), counts.retyped)
            row(String(localized: "Autocorrections"), counts.autocorrected)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
    }

    private func row(_ title: String, _ value: Int) -> some View {
        HStack {
            Text(title).font(.system(size: 12)).foregroundStyle(HexTheme.textMuted)
            Spacer(minLength: 4)
            Text(value.formatted()).font(.system(size: 12)).monospacedDigit().foregroundStyle(HexTheme.textBase)
        }
    }
}

/// On the applied sheet, the first time: why Hex would like to send a notification, in plain
/// words, with Hex's own buttons before the system asks.
struct TypingConfigNotificationAsk: View {
    let done: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Hex can tell you if a typing config seems to make your typing worse. Allow notifications for that?")
                .font(.system(size: 14))
                .foregroundStyle(HexTheme.textBase)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 8) {
                Button {
                    Task { await TypingConfigWatch.answered(allow: true); done() }
                } label: {
                    Text("Allow notifications")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(HexTheme.textContrast)
                        .padding(.horizontal, 14)
                        .frame(height: 32)
                        .background(HexTheme.bgContrast, in: Capsule())
                }
                .buttonStyle(.plain)
                Button {
                    Task { await TypingConfigWatch.answered(allow: false); done() }
                } label: {
                    Text("Not now")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(HexTheme.textBase)
                        .padding(.horizontal, 14)
                        .frame(height: 32)
                        .background(Color.white.opacity(0.09), in: Capsule())
                }
                .buttonStyle(.plain)
            }
            Text("Without them, the keyboard and Home still tell you.")
                .font(.system(size: 12))
                .foregroundStyle(HexTheme.textMuted)
        }
        .padding(14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
    }
}
