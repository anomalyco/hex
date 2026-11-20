import Foundation

/// Counts Hex for iPhone on Anomaly's DAU page (`https://anoma.ly/release/admin/?app=hex-ios`).
/// The release service (anomalyco/release, docs/protocol.md) counts a user as a
/// distinct IP that checked an app's channel manifest, the same way it counts the
/// desktop app's update checks. TestFlight and App Store builds check
/// `hex-ios/ios/<channel>.json` when the app comes to the foreground or the
/// keyboard appears with Full Access, at most once an hour across both (the last
/// check is in the App Group). It sends only the protocol's User-Agent and ignores
/// the answer: the App Store and TestFlight install updates. Debug builds never check.
struct ActiveUserCheck: Equatable, Sendable {
    static let interval: TimeInterval = 60 * 60
    private static let lastCheckKey = "activeUsers.lastCheckAt"

    /// `beta` for TestFlight, `stable` for the App Store.
    var channel: String
    /// `<CFBundleShortVersionString>-<CFBundleVersion>`, e.g. `0.2.0-57`.
    var version: String

    static var current: ActiveUserCheck? {
        #if DEBUG
        return nil
        #else
        let info = Bundle.main.infoDictionary ?? [:]
        let short = info["CFBundleShortVersionString"] as? String ?? "0"
        let build = info["CFBundleVersion"] as? String ?? "0"
        let testFlight = Bundle.main.appStoreReceiptURL?.lastPathComponent == "sandboxReceipt"
        return ActiveUserCheck(channel: testFlight ? "beta" : "stable", version: "\(short)-\(build)")
        #endif
    }

    var request: URLRequest {
        var request = URLRequest(
            url: URL(string: "https://anoma.ly/release/hex-ios/ios/\(channel).json")!,
            cachePolicy: .reloadIgnoringLocalCacheData,
            timeoutInterval: 30
        )
        request.setValue("hex-ios/\(channel)/\(version)/ios", forHTTPHeaderField: "User-Agent")
        return request
    }

    static func isDue(lastCheck: TimeInterval?, now: TimeInterval) -> Bool {
        guard let lastCheck else { return true }
        return now - lastCheck >= interval || now < lastCheck
    }

    /// Checks unless the app or the keyboard checked within the last hour.
    /// Any answer counts, a 404 included; a failed request is retried next time.
    static func checkIfDue(suiteName: String = KeyboardBridge.appGroup) {
        guard let check = current, let defaults = UserDefaults(suiteName: suiteName) else { return }
        let now = Date().timeIntervalSince1970
        guard isDue(lastCheck: defaults.object(forKey: lastCheckKey) as? TimeInterval, now: now) else { return }
        defaults.set(now, forKey: lastCheckKey)
        session.dataTask(with: check.request) { _, response, _ in
            guard response == nil, let defaults = UserDefaults(suiteName: suiteName),
                  defaults.object(forKey: lastCheckKey) as? TimeInterval == now else { return }
            defaults.removeObject(forKey: lastCheckKey)
        }.resume()
    }

    private static let session = URLSession(configuration: .ephemeral)
}
