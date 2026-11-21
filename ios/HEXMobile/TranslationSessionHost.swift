import SwiftUI
@preconcurrency import Translation

/// Translate what I say before iOS 26. There a `TranslationSession` comes only from SwiftUI's
/// `translationTask` (iOS 26 makes one directly, `TranslationSession(installedSource:target:)`),
/// so Hex's root view hosts one (`TranslationSessionHostView`) and each translation asks it for
/// a session for its pair. iOS may not run the task, or the translation, while Hex is in the
/// background (a dictation finished from the keyboard): one that isn't done within `wait` goes
/// in as spoken, and Settings says why (`TranslationFailure.Reason.needsHexOpen`).
@MainActor
@Observable
final class TranslationSessionHost {
    static let shared = TranslationSessionHost()

    static let wait: Duration = .seconds(10)

    private(set) var configuration: TranslationSession.Configuration?
    /// Texts waiting for a session, in order.
    @ObservationIgnored private var queue: [(id: UUID, text: String)] = []
    /// Every translation not answered yet, queued or in a session.
    @ObservationIgnored private var waiting: [UUID: CheckedContinuation<String, Error>] = [:]

    var engine: TranslationEngine {
        TranslationEngine(
            status: { source, target in await LanguageAvailability().status(from: source, to: target) },
            translate: { text, source, target in
                try await TranslationSessionHost.shared.translate(text, from: source, to: target)
            }
        )
    }

    func translate(_ text: String, from source: Locale.Language, to target: Locale.Language) async throws -> String {
        let id = UUID()
        return try await withCheckedThrowingContinuation { continuation in
            waiting[id] = continuation
            queue.append((id, text))
            if configuration?.source == source, configuration?.target == target {
                // Same pair: a new run of the task, with a new session.
                configuration?.invalidate()
            } else {
                configuration = TranslationSession.Configuration(source: source, target: target)
            }
            Task { @MainActor in
                try? await Task.sleep(for: Self.wait)
                self.answer(id, with: .failure(TranslationEngine.NoSession()))
            }
        }
    }

    /// The view's task: translates whatever waits, then returns, as a session can't be used
    /// after its task ends.
    func serve(_ session: TranslationSession) async {
        while !queue.isEmpty {
            let (id, text) = queue.removeFirst()
            guard waiting[id] != nil else { continue }
            do {
                answer(id, with: .success(try await session.translate(text).targetText))
            } catch {
                answer(id, with: .failure(error))
            }
        }
    }

    private func answer(_ id: UUID, with result: Result<String, Error>) {
        waiting.removeValue(forKey: id)?.resume(with: result)
    }
}

/// Hosts `TranslationSessionHost`'s session in Hex's window, before iOS 26.
struct TranslationSessionHostView: View {
    private let host = TranslationSessionHost.shared

    var body: some View {
        Color.clear
            .frame(width: 0, height: 0)
            .accessibilityHidden(true)
            .translationTask(host.configuration) { session in
                await host.serve(session)
            }
            .onAppear { TranslationEngine.hosted = host.engine }
    }
}
