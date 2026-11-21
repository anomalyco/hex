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
    /// Whether it had anything to download, so a model already in place (or the handoff to
    /// Hex) doesn't count as half of onboarding's progress.
    private(set) var downloads = false
    @ObservationIgnored private var task: Task<Void, Never>?

    var isReady: Bool {
        state == .ready || state == .followsApp
    }

    /// Checks, and downloads if needed. Runs once unless "retry" (after a failure).
    func start(retry: Bool = false, appEngine: @escaping @MainActor () -> String) {
        guard !OnboardingFixture.isSnapshot else { return }
        if task != nil, !retry { return }
        if retry, isReady { return }
        task?.cancel()
        state = .checking
        task = Task { await run(appEngine: appEngine) }
    }

    private func run(appEngine: @MainActor () -> String) async {
        guard #available(iOS 26.0, *) else {
            state = .followsApp
            return
        }
        // Launch decides Hex's engine in a task of its own; give it a moment to answer.
        for _ in 0..<30 where appEngine() == "Local speech" {
            try? await Task.sleep(for: .milliseconds(100))
        }
        guard !Task.isCancelled else { return }
        if appEngine() == "Apple Speech" {
            state = .followsApp
            return
        }
        // Automatic can move on to another language once one won't install: try that one.
        for _ in 0..<3 {
            guard await prepare() else { return }
        }
    }

    /// One language's model. Returns true when Automatic moved on and the next one should be tried.
    @available(iOS 26.0, *)
    private func prepare() async -> Bool {
        let settings = LanguageSettings()
        guard let language = await AppleSpeechLanguages.resolve(settings: settings) else {
            // Speech lists nothing here: the keyboard hands its dictations to Hex.
            handOffToHex(settings)
            return false
        }
        if await AppleSpeechAssets.steps.status(language) == .installed {
            settings.clearKeyboardNeedsHex()
            state = .ready
            return false
        }
        state = .downloading(0)
        do {
            _ = try await AppleSpeechAssets.install(
                language,
                settings: settings,
                log: { print("keyboard speech: \($0)") },
                progress: { fraction in
                    Task { @MainActor [weak self] in
                        guard let self, case .downloading = self.state else { return }
                        self.state = .downloading(fraction)
                    }
                }
            )
            guard !Task.isCancelled else { return false }
            state = .ready
        } catch is CancellationError {
        } catch let unavailable as SpeechModelUnavailable where unavailable.reason == .notOnThisIPhone {
            if unavailable.fallback != nil, unavailable.automatic { return true }
            // Apple has no model for it on this iPhone: the keyboard hands its dictations to
            // Hex, as it would after its own first try, so the Hex key works from the start.
            handOffToHex(settings)
        } catch {
            state = .failed(UserFacingError.message(for: error, context: .model))
        }
        return false
    }

    /// The Hex key opens Hex, which dictates with its own model (KeyboardViewController reads
    /// keyboardNeedsHex); marked again at each launch, as the mark lasts a day.
    private func handOffToHex(_ settings: LanguageSettings) {
        settings.markKeyboardNeedsHex()
        state = .followsApp
    }
}
