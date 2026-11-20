import Foundation

/// The first-run pages, in order.
enum OnboardingPage: String, CaseIterable {
    case welcome
    case allow
    case tryKeyboard = "try"

    var number: Int {
        (Self.allCases.firstIndex(of: self) ?? 0) + 1
    }
}

/// First-run progress in the app's own UserDefaults: `onboarding.completed` once it's done
/// or skipped, and `onboarding.page` so a quit part-way resumes on the same page.
@MainActor
enum OnboardingProgress {
    static let completedKey = "onboarding.completed"
    static let pageKey = "onboarding.page"

    /// The page to open at launch, decided once per process.
    static let launchPage: OnboardingPage? = decideLaunchPage(defaults: .standard)

    static func decideLaunchPage(defaults: UserDefaults) -> OnboardingPage? {
        if OnboardingFixture.isSnapshot {
            return OnboardingFixture.current?.page
        }
        if defaults.bool(forKey: completedKey) {
            return nil
        }
        if let saved = defaults.string(forKey: pageKey).flatMap(OnboardingPage.init(rawValue:)) {
            return saved
        }
        // Someone whose keyboard and microphone already work (an earlier build) never sees it.
        let monitor = KeyboardSetupMonitor()
        if monitor.keyboardReady && monitor.microphone == .granted {
            defaults.set(true, forKey: completedKey)
            return nil
        }
        return .welcome
    }

    static func save(_ page: OnboardingPage, defaults: UserDefaults = .standard) {
        guard !OnboardingFixture.isSnapshot else { return }
        defaults.set(false, forKey: completedKey)
        defaults.set(page.rawValue, forKey: pageKey)
    }

    static func complete(defaults: UserDefaults = .standard) {
        guard !OnboardingFixture.isSnapshot else { return }
        defaults.set(true, forKey: completedKey)
        defaults.removeObject(forKey: pageKey)
    }
}

/// `--snap-state onboarding-welcome|onboarding-allow|onboarding-try[-done|-off]` on Simulator, and
/// `onboarding-tour`, which walks the whole flow by itself for a screen recording.
/// `KeyboardSetupMonitor.refresh` supplies the matching setup state.
enum OnboardingFixture: String {
    case welcome = "onboarding-welcome"
    case allow = "onboarding-allow"
    case tryKeyboard = "onboarding-try"
    case tryDone = "onboarding-try-done"
    case tryOff = "onboarding-try-off"
    case tour = "onboarding-tour"

    static let isSnapshot = ProcessInfo.processInfo.arguments.contains("--snap-state")

    static let current: OnboardingFixture? = {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else { return nil }
        return OnboardingFixture(rawValue: args[idx + 1])
    }()

    var page: OnboardingPage {
        switch self {
        case .welcome, .tour: .welcome
        case .allow: .allow
        case .tryKeyboard, .tryDone, .tryOff: .tryKeyboard
        }
    }

    /// The tour's timeline, in seconds after its clock starts.
    enum Tour {
        /// Page 1 holds this long first: the simulator's screen recording stalls during launch.
        static let leadIn = 4.0
        static let toAllow = 4.5
        static let keyboardAdded = 7.0
        static let microphoneAllowed = 9.0
        static let modelReady = 13.0
        static let toTry = 15.0
        static let fullAccess = 18.0
        static let text = 20.5
        static let done = 24.0

        /// Set when the onboarding first shows (plus the lead-in), so a slow launch doesn't eat page 1.
        @MainActor static var start: Date?

        @MainActor static var elapsed: TimeInterval {
            start.map { Date().timeIntervalSince($0) } ?? 0
        }
    }
}
