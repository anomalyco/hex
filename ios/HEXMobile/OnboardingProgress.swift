import Foundation

/// The first-run pages, in the desktop's order: Setting up Hex (the model starts downloading),
/// the demo, permissions, Almost ready (only while the model downloads), the practice, and
/// Hex is ready.
enum OnboardingPage: String, CaseIterable {
    case setup
    case demo
    case permissions
    case waiting
    case practice = "try"
    case ready

    /// A saved page, including the ones earlier builds saved.
    init?(saved: String) {
        switch saved {
        case "welcome": self = .setup
        case "allow": self = .permissions
        default: self.init(rawValue: saved)
        }
    }

    var number: Int {
        (Self.allCases.firstIndex(of: self) ?? 0) + 1
    }

    /// The demo and the practice: the pages setup doesn't need, which show a quiet Skip.
    var isOptional: Bool {
        self == .demo || self == .practice
    }
}

/// The practice counts once it has really been tried: a dictation inserted, or a few words
/// typed with the Hex keyboard (Full Access on), not a first letter.
enum OnboardingPractice {
    static let wordsToCount = 3

    static func isDone(text: String, dictated: Bool, fullAccessOn: Bool) -> Bool {
        if dictated { return true }
        return fullAccessOn && words(in: text) >= wordsToCount
    }

    /// Words as the language splits them, so Chinese counts without spaces.
    static func words(in text: String) -> Int {
        var count = 0
        text.enumerateSubstrings(in: text.startIndex..., options: [.byWords, .substringNotRequired]) { _, _, _, _ in
            count += 1
        }
        return count
    }
}

/// First-run progress in the app's own UserDefaults: "onboarding.completed" once it's done
/// or skipped, and "onboarding.page" so a quit part-way resumes on the same page.
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
        if let saved = defaults.string(forKey: pageKey).flatMap(OnboardingPage.init(saved:)) {
            return saved
        }
        // Someone whose keyboard and microphone already work (an earlier build) never sees it.
        let monitor = KeyboardSetupMonitor()
        if monitor.keyboardReady && monitor.microphone == .granted {
            defaults.set(true, forKey: completedKey)
            return nil
        }
        return .setup
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

/// "--snap-state onboarding-<page>[-<state>]" on Simulator, and "onboarding-tour", which walks
/// the whole flow by itself for a screen recording. "KeyboardSetupMonitor.refresh" supplies the
/// matching setup state.
enum OnboardingFixture: String {
    case setup = "onboarding-setup"
    case demo = "onboarding-demo"
    case permissions = "onboarding-permissions"
    case permissionsDone = "onboarding-permissions-done"
    case waiting = "onboarding-waiting"
    case failed = "onboarding-failed"
    case tryKeyboard = "onboarding-try"
    case tryDone = "onboarding-try-done"
    case tryEmpty = "onboarding-try-empty"
    case tryOff = "onboarding-try-off"
    case ready = "onboarding-ready"
    case tour = "onboarding-tour"

    static let isSnapshot = ProcessInfo.processInfo.arguments.contains("--snap-state")

    /// Pages move on by themselves (after a beat, or once a step is done), except in
    /// snapshots, where the tour moves them instead.
    static var autoAdvances: Bool { !isSnapshot }

    static let current: OnboardingFixture? = {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else { return nil }
        return OnboardingFixture(rawValue: args[idx + 1])
    }()

    var page: OnboardingPage {
        switch self {
        case .setup, .tour: .setup
        case .demo: .demo
        case .permissions, .permissionsDone: .permissions
        case .waiting, .failed: .waiting
        case .tryKeyboard, .tryDone, .tryEmpty, .tryOff: .practice
        case .ready: .ready
        }
    }

    /// How far the fixture's model download is; nil once it's ready.
    var modelFraction: Double? {
        switch self {
        case .setup, .demo, .permissions, .permissionsDone: 0.31
        case .waiting: 0.62
        case .tour: 0.05
        default: nil
        }
    }

    /// The tour's timeline, in seconds after its clock starts.
    enum Tour {
        /// Page 1 holds this long first: the simulator's screen recording stalls during launch.
        static let leadIn = 4.0
        static let toDemo = 3.0
        static let toPermissions = 9.0
        static let keyboardAdded = 11.0
        static let microphoneAllowed = 12.5
        static let toWaiting = 14.0
        static let modelReady = 17.0
        static let toTry = 17.8
        static let fullAccess = 19.0
        static let text = 21.0
        static let toReady = 24.0
        static let done = 28.0

        /// Set when the onboarding first shows (plus the lead-in), so a slow launch doesn't eat page 1.
        @MainActor static var start: Date?

        @MainActor static var elapsed: TimeInterval {
            start.map { Date().timeIntervalSince($0) } ?? 0
        }
    }
}
