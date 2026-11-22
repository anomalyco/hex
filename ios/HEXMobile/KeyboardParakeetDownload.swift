import Foundation
import Observation
import SwiftUI

/// Experiments › Speech model in keyboard: gets Parakeet V2 into the App Group for the keyboard,
/// with FluidAudio's download (each file resumes from its `.partial`). Turned on, at launch
/// with it on, and whenever Hex comes back to the front, until every file is there.
@MainActor
@Observable
final class KeyboardParakeetDownload {
    static let shared = KeyboardParakeetDownload()

    enum State: Equatable {
        case idle
        /// The share downloaded.
        case downloading(Double)
        case failed(String)
    }

    private(set) var state = State.idle
    @ObservationIgnored private var task: Task<Void, Never>?

    /// What the download weighs: Parakeet V2's four models and vocabulary, about 464 MB.
    static let bytes: Int64 = 464_400_000

    static var size: String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
            .replacingOccurrences(of: " ", with: "\u{00A0}")
    }

    var isDownloaded: Bool { SpeechModelStore.isComplete(ParakeetTranscriber.cacheDirectory) }

    func startIfWanted() {
        guard KeyboardParakeetSettings().isEnabled, KeyboardParakeetSettings.fits(), !isDownloaded, task == nil else {
            return
        }
        state = .downloading(0)
        task = Task { [weak self] in
            do {
                try await ParakeetTranscriber.downloadModel { fraction in
                    Task { @MainActor in KeyboardParakeetDownload.shared.show(fraction) }
                }
                guard let self, !Task.isCancelled else { return }
                self.task = nil
                self.state = .idle
            } catch {
                guard let self, !Task.isCancelled else { return }
                print("speech model in keyboard: download stopped: \(error)")
                self.task = nil
                self.state = .failed(String(localized: "The speech model didn't download. Check your connection and try again."))
            }
        }
    }

    private func show(_ fraction: Double) {
        guard task != nil, case .downloading(let shown) = state, fraction > shown else { return }
        state = .downloading(fraction)
    }

    /// Stops it and waits until nothing writes to the folder any more.
    func stop() async {
        let running = task
        task = nil
        running?.cancel()
        await running?.value
        state = .idle
    }

    func retry() {
        if case .failed = state { state = .idle }
        startIfWanted()
    }
}

extension DictationController {
    /// The switch in Experiments: on downloads Parakeet V2 for the keyboard unless it's there;
    /// off stops the download and deletes it, unless Hex itself dictates with Parakeet V2.
    func keyboardParakeetChanged() {
        let settings = KeyboardParakeetSettings()
        settings.setKeyboardState(nil)
        if settings.isEnabled {
            startKeyboardParakeetDownloadIfWanted()
            return
        }
        let keeps = transcriber is ParakeetTranscriber && (isModelReady || phase == .preparingModel)
        Task {
            await KeyboardParakeetDownload.shared.stop()
            if !keeps {
                try? FileManager.default.removeItem(at: ParakeetTranscriber.cacheDirectory)
            }
        }
    }

    /// Not while Hex's own Parakeet V2 gets ready: it downloads into the same folder.
    func startKeyboardParakeetDownloadIfWanted() {
        guard !(transcriber is ParakeetTranscriber && phase == .preparingModel) else { return }
        KeyboardParakeetDownload.shared.startIfWanted()
    }
}

/// Under the switch: downloading, ready to get ready in the keyboard, getting ready, ready, or
/// why the keyboard isn't using it, in a few words. The keyboard's word stays only while fresh.
struct KeyboardParakeetStatus: View {
    let download: KeyboardParakeetDownload

    var body: some View {
        TimelineView(.periodic(from: .now, by: 2)) { context in
            if let line = line(now: context.date) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(line)
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.textBase)
                        .fixedSize(horizontal: false, vertical: true)
                        .accessibilityIdentifier("keyboard-parakeet-status")
                    if case .downloading(let fraction) = download.state {
                        ProgressView(value: fraction)
                            .tint(HexTheme.textBase)
                    }
                    if case .failed = download.state {
                        Button("Try again") { download.retry() }
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(HexTheme.textBase)
                            .accessibilityIdentifier("keyboard-parakeet-try-again")
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(14)
                .background(Color.white.opacity(0.055), in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
        }
    }

    private func line(now: Date) -> String? {
        switch download.state {
        case .downloading:
            return String(localized: "Downloading the speech model")
        case .failed(let message):
            return message
        case .idle:
            break
        }
        guard download.isDownloaded else { return nil }
        switch KeyboardParakeetSettings().keyboardState {
        case (.preparing, let at)? where now.timeIntervalSince(at) < 180:
            return String(localized: "Getting ready in the keyboard")
        case (.ready, _)?:
            return String(localized: "Ready in the keyboard")
        case (.unavailable, _)?:
            return String(localized: "The keyboard can't use it right now, so it uses Apple's speech recognition.")
        default:
            return String(localized: "Downloaded. The keyboard gets it ready the next time it opens.")
        }
    }
}
