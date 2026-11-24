import Foundation
import Observation

/// The speech model the Hex keyboard itself dictates with, so "ready" means the Hex key works
/// in other apps, not only that Hex's own model is ready.
///
/// On iOS 26 the keyboard transcribes inside itself with Apple Speech (KeyboardSpeech),
/// whatever Hex uses: Parakeet can't load within a keyboard's memory limit, and handing every
/// dictation to Hex would leave the app being typed in each time. So when Hex dictates with
/// Parakeet (an English iPhone without SpeechTranscriber, or a Parakeet model already cached),
/// Hex installs the keyboard's Apple Speech model too, with the call the keyboard makes
/// (AppleSpeechAssets.install: same language, same module, and it ends the keyboard's handoff
/// to Hex). When Hex itself dictates with Apple Speech, its own model is that same one.
/// Before iOS 26, and where Speech lists no language at all, the Hex key hands dictation to
/// Hex, so Hex's own model is the one that counts.
@MainActor
@Observable
final class KeyboardSpeechPreparation {
    enum State: Equatable {
        case checking
        /// The keyboard uses what Hex prepares: Hex's own model decides.
        case followsApp
        case downloading(Double)
        case ready
        case failed(String)
    }

    private(set) var state: State = OnboardingFixture.isSnapshot ? .ready : .checking {
        didSet {
            if case .downloading = state { downloads = true }
        }
    }
    /// Whether it downloaded the keyboard's model, so a model already in place (or the handoff
    /// to Hex) doesn't count as half of onboarding's progress.
    private(set) var downloads = false
    @ObservationIgnored private var task: Task<Void, Never>?
    /// Which check is current; one a retry replaced changes nothing when it answers late.
    @ObservationIgnored private var generation = 0

    var isReady: Bool {
        state == .ready || state == .followsApp
    }

    /// How long it may check before saying so: Speech or the asset store not answering would
    /// otherwise leave setup's last step on "Preparing" for good.
    @ObservationIgnored var checkLimit: Duration = .seconds(30)

    /// Checks, and downloads if needed. Runs once unless "retry" (after a failure).
    func start(retry: Bool = false, recheck: Bool = false, appEngine: @escaping @MainActor () -> DictationEngine) {
        guard !OnboardingFixture.isSnapshot else { return }
        if task != nil, !retry, !recheck { return }
        if retry, !recheck, isReady { return }
        generation &+= 1
        let currentGeneration = generation
        task?.cancel()
        state = .checking
        task = Task { await run(appEngine: appEngine, generation: currentGeneration) }
        let started = task
        Task { [weak self, checkLimit] in
            try? await Task.sleep(for: checkLimit)
            guard let self, self.task == started, self.state == .checking else { return }
            self.task?.cancel()
            self.task = nil
            self.state = .failed(String(localized: "The speech model isn't ready. Try again."))
            // Nothing this check started may change the row after it failed: a cancelled Speech
            // call can still return late.
            self.generation &+= 1
        }
    }

    /// Back in front while the keyboard hands its dictations to Hex: when the keyboard's
    /// language has Apple's model here after all, its handoff ends (`SpeechModelInstall`
    /// clears it) and the keyboard dictates itself again, rather than for the rest of the day.
    func recheckKeyboardHandoff(appEngine: @escaping @MainActor () -> DictationEngine) {
        guard !OnboardingFixture.isSnapshot, LanguageSettings().keyboardNeedsHex() else { return }
        switch state {
        case .checking, .downloading: return
        default: break
        }
        start(recheck: true, appEngine: appEngine)
    }

    /// Back in front after Hex was actually in the background: a check that never answered
    /// starts over, and one that failed tries again. A download already moving is left alone.
    func resumeOnForeground(appEngine: @escaping @MainActor () -> DictationEngine) {
        switch state {
        case .checking, .failed:
            start(retry: true, appEngine: appEngine)
        case .downloading, .followsApp, .ready:
            break
        }
    }

    private func run(appEngine: @MainActor () -> DictationEngine, generation: Int) async {
        guard #available(iOS 26.0, *) else {
            publish(.followsApp, generation: generation)
            return
        }
        // Launch decides Hex's engine in a task of its own; give it a moment to answer.
        for _ in 0..<30 where appEngine() == .undecided {
            try? await Task.sleep(for: .milliseconds(100))
        }
        guard !Task.isCancelled else { return }
        // Apple Speech in Hex is the keyboard's model too, unless the keyboard couldn't get it:
        // then it checks, so a model in place ends that. With Parakeet in Hex (English, or the
        // multilingual model) the keyboard needs Apple's model of its own all the same.
        let needsHex = LanguageSettings().keyboardNeedsHex()
        if appEngine().keyboardSpeechFollowsApp(keyboardNeedsHex: needsHex) {
            publish(.followsApp, generation: generation)
            return
        }
        // Automatic can move on to another language once one won't install: try that one.
        for _ in 0..<3 {
            guard await prepare(generation: generation) else { return }
        }
    }

    /// One language's model. Returns true when Automatic moved on and the next one should be tried.
    @available(iOS 26.0, *)
    private func prepare(generation: Int) async -> Bool {
        let settings = LanguageSettings()
        guard let language = await AppleSpeechLanguages.resolve(settings: settings) else {
            // Speech lists nothing here: the keyboard hands its dictations to Hex.
            handOffToHex(settings, generation: generation)
            return false
        }
        if await AppleSpeechAssets.steps.status(language) == .installed {
            settings.clearKeyboardNeedsHex()
            publish(.ready, generation: generation)
            return false
        }
        publish(.downloading(0), generation: generation)
        do {
            _ = try await AppleSpeechAssets.install(
                language,
                settings: settings,
                log: { print("keyboard speech: \($0)") },
                progress: { fraction in
                    Task { @MainActor [weak self] in
                        guard let self,
                              generation == self.generation,
                              case .downloading = self.state else { return }
                        self.state = .downloading(fraction)
                    }
                }
            )
            guard !Task.isCancelled else { return false }
            publish(.ready, generation: generation)
        } catch is CancellationError {
        } catch let unavailable as SpeechModelUnavailable where unavailable.reason == .notOnThisIPhone {
            if unavailable.fallback != nil, unavailable.automatic { return true }
            // Apple has no model for it on this iPhone: the keyboard hands its dictations to
            // Hex, as it would after its own first try, so the Hex key works from the start.
            handOffToHex(settings, generation: generation)
        } catch {
            publish(.failed(UserFacingError.message(for: error, context: .model)), generation: generation)
        }
        return false
    }

    /// `state` for the current check only: one a retry replaced changes nothing when it
    /// answers late.
    private func publish(_ next: State, generation: Int) {
        guard !Task.isCancelled, generation == self.generation else { return }
        state = next
    }

    /// The Hex key opens Hex, which dictates with its own model (KeyboardViewController reads
    /// keyboardNeedsHex); marked again at each launch, as the mark lasts a day.
    private func handOffToHex(_ settings: LanguageSettings, generation: Int) {
        guard !Task.isCancelled, generation == self.generation else { return }
        settings.markKeyboardNeedsHex()
        state = .followsApp
        // A download that ended in the handoff isn't part of onboarding's progress either.
        downloads = false
    }
}
