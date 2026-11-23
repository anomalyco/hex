import UIKit

/// Typing configs' regression notice in the bar (`TypingConfigHealth`): once per config, when
/// typing seems worse since it was applied, with Open Hex and an ✕ for good. A day later, if
/// neither was tapped, a notification instead (design §7). Needs no notification permission.
extension FullKeyboardView {
    var typingConfigNotice: KeyboardStripNotice? {
        guard showsTypingConfigNotice else { return nil }
        return KeyboardStripNotice(
            // The bar fits about 30 characters a line: design §7's words over two.
            title: String(localized: "Typing may be worse"),
            detail: String(localized: "with this config · Compare with your last one"),
            button: String(localized: "Open Hex"),
            dismissible: true
        )
    }

    /// When the keyboard appears, with Full Access, never in a password or code field and never
    /// over another notice.
    func offerTypingConfigNotice(now: Date = Date()) {
        let health = TypingConfigHealth()
        guard model.hasFullAccess, !health.alertsOff, let worse = health.worse,
              engine.target.isSecureTextEntry != true, engine.target.textContentType != .oneTimeCode else { return }
        let alert = health.alert(worse.current)
        if alert.barShownAt == nil {
            guard !showsTypingConfigNotice, !showsModelNotice else { return }
            health.updateAlert(worse.current) { $0.barShownAt = now.timeIntervalSince1970 }
            showsTypingConfigNotice = true
            typingLog?.ui("typing_config_notice")
            refreshStrip()
        } else if let shown = alert.barShownAt, !alert.barActed, alert.notifiedAt == nil, now.timeIntervalSince1970 - shown >= 24 * 3600 {
            health.notify(worse, now: now)
        }
    }

    func openHexForTypingConfig() {
        if let key = TypingConfigHealth().worse?.current {
            TypingConfigHealth().updateAlert(key) { $0.barActed = true }
        }
        hideTypingConfigNotice()
        model.onOpenURL?(TypingConfigHealth.notificationURL)
    }

    /// The ✕: never shown again for this config, and no notification later.
    func dismissTypingConfigNotice() {
        if let key = TypingConfigHealth().worse?.current {
            TypingConfigHealth().updateAlert(key) { $0.barActed = true }
        }
        hideTypingConfigNotice()
    }

    func hideTypingConfigNotice() {
        guard showsTypingConfigNotice else { return }
        showsTypingConfigNotice = false
        refreshStrip()
    }
}
