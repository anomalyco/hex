import Foundation
import UserNotifications

/// Settings › Experiments › Typing configs: how typing goes with each config, so Hex can suggest
/// going back when a new one seems to make it worse (design §7). The keyboard counts, per config
/// and with no words, what Learn how I type already sees; the app compares. Hex only ever
/// suggests: it never switches by itself, except when the keyboard failed to start twice after a
/// new config (`TypingConfigStartupGuard`).
nonisolated struct TypingConfigHealth {
    /// The key for no config.
    static let defaultsKey = "defaults"
    /// No comparison before both sides have this many finished words (design §7: a 2× rise
    /// shows at about 360 words each).
    static let minimumWords = 400
    /// The new config's trouble rate must be at least this many times the last one's.
    static let worseRatio = 2.0
    /// …and come from at least this many troubles, so a handful never alerts.
    static let minimumTroubles = 8

    enum Keys {
        /// The config the keyboard uses now: its history id (SHA-256), or `defaultsKey`.
        static let activeKey = "typingConfig.activeKey"
        /// `Pair`: the config applied last and the one in use before it, which it is compared with.
        static let pair = "typingConfig.comparePair"
        static let counts = "typingConfig.health"
        static let alerts = "typingConfig.alerts"
        /// Settings › Keyboard › Typing configs › Alerts off: no bar notice and no notification.
        static let alertsOff = "typingConfig.alertsOff"
    }

    /// Per config: words finished and what went wrong with them (no words kept).
    struct Counts: Codable, Equatable, Sendable {
        var words = 0
        var autocorrected = 0
        /// Autocorrections taken back (one backspace, or the strip's revert).
        var undone = 0
        /// What was typed tapped over the autocorrection space would apply.
        var keptTyped = 0
        /// A word erased and typed again as another (a slip fixed by hand).
        var retyped = 0
        var picked = 0

        var troubles: Int { undone + keptTyped + retyped }
        /// Troubles per 100 finished words.
        var rate: Double { words == 0 ? 0 : Double(troubles) * 100 / Double(words) }
    }

    struct Pair: Codable, Equatable, Sendable {
        var current: String
        var previous: String
        var appliedAt: TimeInterval
    }

    /// What Hex said about one config, so it says it at most once.
    struct Alert: Codable, Equatable, Sendable {
        var barShownAt: TimeInterval?
        var barActed = false
        var notifiedAt: TimeInterval?
        /// Go back, Keep this one or Turn configs off: never asked again.
        var resolved = false
        var homeCardDismissed = false
    }

    struct Comparison: Equatable, Sendable {
        var current: String
        var previous: String
        var now: Counts
        var before: Counts
        /// Both sides have `minimumWords`.
        var isReady: Bool { now.words >= TypingConfigHealth.minimumWords && before.words >= TypingConfigHealth.minimumWords }
        var seemsWorse: Bool {
            guard isReady, now.troubles >= TypingConfigHealth.minimumTroubles else { return false }
            // now.rate >= ratio × before.rate, without dividing by a zero.
            return Double(now.troubles) * Double(before.words) >= TypingConfigHealth.worseRatio * Double(before.troubles) * Double(now.words)
        }
    }

    var defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)

    private var enabled: Bool { defaults?.bool(forKey: TypingConfigSettings.Keys.enabled) ?? false }

    var activeKey: String { defaults?.string(forKey: Keys.activeKey) ?? Self.defaultsKey }

    func setActive(_ key: String?, previous: String?, now: Date = Date()) {
        let current = key ?? Self.defaultsKey
        defaults?.set(current, forKey: Keys.activeKey)
        if let previous, previous != current {
            save(Pair(current: current, previous: previous, appliedAt: now.timeIntervalSince1970), forKey: Keys.pair)
        }
    }

    var pair: Pair? { load(Pair.self, forKey: Keys.pair) }

    var allCounts: [String: Counts] { load([String: Counts].self, forKey: Keys.counts) ?? [:] }

    func counts(_ key: String) -> Counts { allCounts[key] ?? Counts() }

    var alertsOff: Bool {
        get { defaults?.bool(forKey: Keys.alertsOff) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Keys.alertsOff) }
    }

    func alert(_ key: String) -> Alert { (load([String: Alert].self, forKey: Keys.alerts) ?? [:])[key] ?? Alert() }

    func updateAlert(_ key: String, _ change: (inout Alert) -> Void) {
        var all = load([String: Alert].self, forKey: Keys.alerts) ?? [:]
        var alert = all[key] ?? Alert()
        change(&alert)
        all[key] = alert
        save(all, forKey: Keys.alerts)
    }

    /// The last config applied against the one before it, while it is still in use.
    var comparison: Comparison? {
        guard let pair, pair.current == activeKey, pair.current != Self.defaultsKey else { return nil }
        return Comparison(current: pair.current, previous: pair.previous, now: counts(pair.current), before: counts(pair.previous))
    }

    /// The comparison Hex should say something about: worse, still in use, not resolved.
    var worse: Comparison? {
        guard enabled, let comparison, comparison.seemsWorse, !alert(comparison.current).resolved else { return nil }
        return comparison
    }

    // MARK: - Counting (the keyboard, through Learn how I type's journal)

    func finishedWord() { bump { $0.words += 1 } }

    func recorded(_ kind: TypingJournalCorrection.Kind) {
        bump { counts in
            switch kind {
            case .autocorrected: counts.autocorrected += 1
            case .undone: counts.undone += 1
            case .keptTyped: counts.keptTyped += 1
            case .retyped: counts.retyped += 1
            case .picked: counts.picked += 1
            }
        }
    }

    private func bump(_ change: (inout Counts) -> Void) {
        guard enabled else { return }
        var all = allCounts
        change(&all[activeKey, default: Counts()])
        save(all, forKey: Keys.counts)
    }

    /// Delete what Hex learned and Delete all: the counts and what was said go too.
    func deleteAll() {
        for key in [Keys.counts, Keys.alerts, Keys.pair] { defaults?.removeObject(forKey: key) }
    }

    func forget(_ key: String) {
        var all = allCounts
        all[key] = nil
        save(all, forKey: Keys.counts)
    }

    // MARK: - The notification

    static let notificationURL = URL(string: "hex-dictation://typing-configs/compare")!

    /// Design §7's words; the title fits a banner (about 34 characters).
    static var notificationTitle: String { String(localized: "Typing may be worse with this config") }
    static var notificationBody: String { String(localized: "Tap to compare it with your last one and go back if you want.") }

    /// Posts the notification for `comparison` once, quietly (no sound, not time-sensitive);
    /// a Focus that silences Hex silences it.
    func notify(_ comparison: Comparison, now: Date = Date()) {
        updateAlert(comparison.current) { $0.notifiedAt = now.timeIntervalSince1970 }
        let content = UNMutableNotificationContent()
        content.title = Self.notificationTitle
        content.body = Self.notificationBody
        content.userInfo = ["url": Self.notificationURL.absoluteString]
        content.threadIdentifier = "typing-configs"
        let request = UNNotificationRequest(identifier: "typing-config-worse-\(comparison.current.prefix(16))", content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }

    private func load<T: Decodable>(_ type: T.Type, forKey key: String) -> T? {
        defaults?.data(forKey: key).flatMap { try? JSONDecoder().decode(T.self, from: $0) }
    }

    private func save(_ value: some Encodable, forKey key: String) {
        if let data = try? JSONEncoder().encode(value) { defaults?.set(data, forKey: key) }
    }
}

/// The one thing Hex does by itself: if the keyboard didn't come up twice running with a new
/// config, it starts without it and the app says so. The keyboard marks "starting with config X"
/// before it loads one and clears the mark once it is on screen; a mark still there at the next
/// start means that start died.
nonisolated struct TypingConfigStartupGuard {
    static let failuresToFallBack = 2

    enum Keys {
        static let mark = "typingConfig.startMark"
        /// `Fallback`: written by the keyboard, read once by the app.
        static let fallback = "typingConfig.fallback"
    }

    struct Mark: Codable, Equatable { var key: String; var died: Int }
    struct Fallback: Codable, Equatable { var key: String; var at: TimeInterval; var handled = false }

    var defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)
    var store = TypingConfigStore()

    /// Before the keyboard loads the active config. Returns true when it fell back.
    @discardableResult
    func willStart(now: Date = Date()) -> Bool {
        let health = TypingConfigHealth(defaults: defaults)
        let key = health.activeKey
        guard defaults?.bool(forKey: TypingConfigSettings.Keys.enabled) == true, key != TypingConfigHealth.defaultsKey,
              let url = store.activeURL, FileManager.default.fileExists(atPath: url.path) else {
            defaults?.removeObject(forKey: Keys.mark)
            return false
        }
        let previous = defaults?.data(forKey: Keys.mark).flatMap { try? JSONDecoder().decode(Mark.self, from: $0) }
        let died = previous?.key == key ? previous!.died + 1 : 0
        if died >= Self.failuresToFallBack {
            store.removeActive()
            defaults?.set(TypingConfigHealth.defaultsKey, forKey: TypingConfigHealth.Keys.activeKey)
            defaults?.removeObject(forKey: Keys.mark)
            if let data = try? JSONEncoder().encode(Fallback(key: key, at: now.timeIntervalSince1970)) {
                defaults?.set(data, forKey: Keys.fallback)
            }
            return true
        }
        if let data = try? JSONEncoder().encode(Mark(key: key, died: died)) { defaults?.set(data, forKey: Keys.mark) }
        return false
    }

    /// Once the keys are on screen.
    func didStart() {
        defaults?.removeObject(forKey: Keys.mark)
    }

    var fallback: Fallback? { defaults?.data(forKey: Keys.fallback).flatMap { try? JSONDecoder().decode(Fallback.self, from: $0) } }

    func markHandled() {
        guard var f = fallback else { return }
        f.handled = true
        if let data = try? JSONEncoder().encode(f) { defaults?.set(data, forKey: Keys.fallback) }
    }

    func clearFallback() { defaults?.removeObject(forKey: Keys.fallback) }
}
