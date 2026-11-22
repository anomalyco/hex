import Foundation
import Observation
import SwiftUI

/// Experiments › Multilingual model (Parakeet): gets Parakeet Ultra onto the iPhone beside the
/// engine Hex dictates with meanwhile, so turning the experiment on never takes dictation away.
/// Each file resumes from its `.partial`, at the next launch too. A dropped connection or a
/// download that stops moving for `stallLimit` tries again by itself (after 2, 4, 8, 16, 30 s),
/// and at once when Hex comes back to the front; after `automaticTries` in a row that got nothing
/// further, it says what went wrong and waits for Try again (or, for a connection, the next
/// time Hex comes to the front).
@MainActor
@Observable
final class ParakeetUltraDownload {
    enum State: Equatable {
        /// Nothing to say: the experiment off, not wanted for this language, or Ultra dictates.
        case idle
        /// Bytes on disk.
        case downloading(Int64)
        /// Stopped, and tries again by itself in `retryIn` seconds, or once Hex is in front (nil).
        case waiting(Int64, retryIn: Int?)
        /// On disk; Hex gets it ready beside the engine it dictates with.
        case loading
        /// What went wrong, in plain words.
        case failed(String)
    }

    private(set) var state = State.idle
    /// Called once every file is on disk.
    @ObservationIgnored var onDownloaded: (@MainActor () -> Void)?

    nonisolated static let stallLimit: Duration = .seconds(20)
    static let automaticTries = 6

    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var tries = 0
    /// What was on disk at the last stop: a download that moved since starts its tries over.
    @ObservationIgnored private var bytesAtLastStop: Int64 = 0
    @ObservationIgnored private var failedForConnection = false
    @ObservationIgnored private var backgroundedAt: Date?
    @ObservationIgnored private var backgroundTime: BackgroundTime?
    @ObservationIgnored private let directory: URL
    @ObservationIgnored private let files: [ParakeetUltraModel.File]
    @ObservationIgnored private let base: String
    @ObservationIgnored private let session: URLSession
    @ObservationIgnored private let stallLimit: Duration
    @ObservationIgnored private let backoff: (Int) -> Int
    /// Now, so a background trip's length is testable.
    @ObservationIgnored private let now: () -> Date

    init(
        directory: URL = ParakeetUltraModel.directory,
        files: [ParakeetUltraModel.File] = ParakeetUltraModel.files,
        base: String = ParakeetUltraModel.baseURL,
        session: URLSession = ParakeetUltraDownload.makeSession(),
        stallLimit: Duration = ParakeetUltraDownload.stallLimit,
        backoff: @escaping (Int) -> Int = ParakeetUltraDownload.backoff,
        now: @escaping () -> Date = { Date() }
    ) {
        self.directory = directory
        self.files = files
        self.base = base
        self.session = session
        self.stallLimit = stallLimit
        self.backoff = backoff
        self.now = now
    }

    /// A connection that sends nothing for 30 s ends, rather than after the default minute.
    nonisolated static func makeSession() -> URLSession {
        let configuration = URLSessionConfiguration.default
        configuration.timeoutIntervalForRequest = 30
        return URLSession(configuration: configuration)
    }

    /// Seconds before automatic try `n` (1, 2, …): 2, 4, 8, 16, then 30.
    nonisolated static func backoff(_ tries: Int) -> Int {
        min(30, 1 << max(1, min(tries, 5)))
    }

    var isRunning: Bool { task != nil }

    var total: Int64 { files.reduce(0) { $0 + $1.size } }

    /// Starts, or resumes from what's on disk; does nothing while it runs.
    func start() {
        guard task == nil else { return }
        tries = 0
        run()
    }

    /// Starts over from what's on disk, with all the automatic tries again. The transfer it
    /// replaces is cancelled and awaited first, as `stop` does, so a late write can't land in
    /// the `.partial` file the new one resumes from.
    func restart() {
        let running = task
        task = nil
        tries = 0
        guard let running else {
            run()
            return
        }
        running.cancel()
        task = Task { [weak self] in
            await running.value
            guard let self, !Task.isCancelled else { return }
            self.task = nil
            self.run()
        }
    }

    /// Stops it and waits until nothing writes to the folder any more, so it can be deleted.
    func stop() async {
        let running = task
        task = nil
        running?.cancel()
        await running?.value
        endBackgroundTime()
        state = .idle
    }

    func loading() {
        state = .loading
    }

    /// A load beside the usual engine that another preparation replaced.
    func dropLoad() {
        if state == .loading { state = .idle }
    }

    /// Ultra dictates, or isn't wanted any more.
    func settle() {
        guard task == nil else { return }
        state = .idle
    }

    /// A load that failed, or one that ended Hex: said with Try again beside it.
    func fail(_ message: String) {
        task?.cancel()
        task = nil
        failedForConnection = false
        state = .failed(message)
    }

    func appMovedToBackground() {
        backgroundedAt = now()
        // A few more seconds from iOS, so a quick look at another app doesn't stop it.
        if task != nil, backgroundTime == nil {
            backgroundTime = BackgroundTime("Download the multilingual model")
        }
    }

    /// Back in front: a download that stopped tries again at once, and one suspended long
    /// enough for iOS to drop its connection starts a fresh one from where it is.
    func appBecameActive() {
        let away = backgroundedAt.map { now().timeIntervalSince($0) } ?? 0
        backgroundedAt = nil
        endBackgroundTime()
        switch state {
        case .waiting:
            restart()
        case .failed where failedForConnection:
            restart()
        case .downloading where away > 25:
            restart()
        default:
            break
        }
    }

    private func endBackgroundTime() {
        backgroundTime?.end()
        backgroundTime = nil
    }

    private func run() {
        failedForConnection = false
        bytesAtLastStop = ParakeetUltraModel.bytesOnDisk(in: directory, files: files)
        state = .downloading(bytesAtLastStop)
        task = Task { [weak self] in
            await self?.loop()
        }
    }

    private func loop() async {
        while !Task.isCancelled {
            do {
                try await Self.fetch(
                    directory: directory,
                    files: files,
                    base: base,
                    session: session,
                    stallLimit: stallLimit
                ) { [weak self] bytes in
                    await MainActor.run {
                        guard let self, self.task != nil, case .downloading = self.state else { return }
                        self.state = .downloading(bytes)
                    }
                }
                guard !Task.isCancelled else { return }
                task = nil
                endBackgroundTime()
                state = .loading
                onDownloaded?()
                return
            } catch {
                guard !Task.isCancelled else { return }
                let onDisk = ParakeetUltraModel.bytesOnDisk(in: directory, files: files)
                if onDisk > bytesAtLastStop {
                    tries = 0
                }
                bytesAtLastStop = onDisk
                tries += 1
                print("multilingual model: download try \(tries) stopped at \(onDisk) bytes: \(error)")
                guard Self.triesAgain(after: error), tries < Self.automaticTries else {
                    task = nil
                    endBackgroundTime()
                    failedForConnection = Self.triesAgain(after: error)
                    state = .failed(Self.message(for: error))
                    return
                }
                if backgroundedAt != nil {
                    // Suspended soon anyway: tries again once Hex is back in front.
                    task = nil
                    endBackgroundTime()
                    state = .waiting(onDisk, retryIn: nil)
                    return
                }
                for remaining in stride(from: backoff(tries), to: 0, by: -1) {
                    state = .waiting(onDisk, retryIn: remaining)
                    try? await Task.sleep(for: .seconds(1))
                    guard !Task.isCancelled else { return }
                }
                state = .downloading(onDisk)
            }
        }
    }

    /// One try: the download beside a watch that publishes its bytes twice a second and ends
    /// it once they stop moving for `stallLimit` (time while iOS had Hex suspended doesn't
    /// count). Returns only once nothing writes to the folder any more.
    nonisolated static func fetch(
        directory: URL,
        files: [ParakeetUltraModel.File],
        base: String,
        session: URLSession,
        stallLimit: Duration,
        publish: @escaping @Sendable (Int64) async -> Void
    ) async throws {
        let bytes = ByteReading(ParakeetUltraModel.bytesOnDisk(in: directory, files: files))
        try await withThrowingTaskGroup(of: Void.self) { group in
            group.addTask {
                try await ParakeetUltraModel.download(to: directory, files: files, base: base, session: session) {
                    bytes.value = $0
                }
            }
            group.addTask {
                let clock = ContinuousClock()
                let poll = Duration.milliseconds(500)
                var last = bytes.value
                var since = clock.now
                var polled = since
                while true {
                    try await Task.sleep(for: poll)
                    let now = clock.now
                    let resumed = now - polled > poll + StallWatch.suspensionGap
                    polled = now
                    let current = bytes.value
                    await publish(current)
                    if current != last || resumed {
                        last = current
                        since = now
                    } else if now - since >= stallLimit {
                        throw StallWatch.Stalled()
                    }
                }
            }
            // The download finishing, or either one throwing, ends the other.
            _ = try await group.next()
            group.cancelAll()
        }
    }

    /// A dropped connection, a download that stopped moving, a busy server or a damaged file
    /// is tried again; a refused request, a full iPhone or anything else waits for the person.
    nonisolated static func triesAgain(after error: Error) -> Bool {
        if error is StallWatch.Stalled || error is URLError { return true }
        switch error as? ParakeetUltraModel.DownloadError {
        case .badStatus(let status):
            return status == 0 || status == 408 || status == 429 || status >= 500
        case .corrupt:
            return true
        case nil:
            return false
        }
    }

    nonisolated static func isOutOfSpace(_ error: Error) -> Bool {
        let ns = error as NSError
        return (ns.domain == NSCocoaErrorDomain && ns.code == NSFileWriteOutOfSpaceError)
            || (ns.domain == NSPOSIXErrorDomain && ns.code == Int(ENOSPC))
    }

    nonisolated static func message(for error: Error) -> String {
        if isOutOfSpace(error) {
            return String(localized: "The multilingual model needs \(ParakeetUltraTranscriber.downloadSize) of free space. Free some up, then try again.")
        }
        if error is StallWatch.Stalled || error is URLError {
            return String(localized: "The multilingual model's download stopped. Check your connection and try again.")
        }
        switch error as? ParakeetUltraModel.DownloadError {
        case .badStatus(429):
            return String(localized: "Hugging Face is limiting downloads right now. Try again in a few minutes.")
        case .badStatus(let status):
            return String(localized: "The multilingual model didn't download (error \(status)). Try again later.")
        case .corrupt:
            return String(localized: "Part of the multilingual model arrived damaged. Try again.")
        case nil:
            return String(localized: "The multilingual model didn't download. Try again.")
        }
    }

    /// What Hex says about it, or nil while there's nothing to say.
    var status: String? {
        switch state {
        case .idle:
            nil
        case .downloading(let bytes):
            String(localized: "Downloading the multilingual model: \(Self.size(bytes)) of \(Self.size(total))")
        case .waiting(_, let seconds?):
            String(localized: "The multilingual model's download stopped. Trying again in \(seconds) s.")
        case .waiting(_, nil):
            String(localized: "The multilingual model's download resumes when you open Hex.")
        case .loading:
            String(localized: "Getting the multilingual model ready")
        case .failed(let message):
            message
        }
    }

    /// The share on disk, while it downloads or waits.
    var fraction: Double? {
        switch state {
        case .downloading(let bytes), .waiting(let bytes, _):
            total > 0 ? min(1, Double(bytes) / Double(total)) : 0
        default:
            nil
        }
    }

    var isFailed: Bool {
        if case .failed = state { return true }
        return false
    }

    nonisolated static func size(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
            .replacingOccurrences(of: " ", with: "\u{00A0}")
    }
}

/// Bytes written by the download and read by its watch.
private nonisolated final class ByteReading: @unchecked Sendable {
    private let lock = NSLock()
    private var current: Int64

    init(_ value: Int64) {
        current = value
    }

    var value: Int64 {
        get { lock.withLock { current } }
        set { lock.withLock { current = newValue } }
    }
}

/// The multilingual model's download or load, on home and under its switch in Experiments:
/// what it's doing, that Hex dictates with its usual model meanwhile, and after a failure
/// Try again and Turn off.
struct MultilingualModelStatus: View {
    let download: ParakeetUltraDownload
    let retry: () -> Void
    let turnOff: () -> Void

    var body: some View {
        if let status = download.status {
            VStack(alignment: .leading, spacing: 10) {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    Image(systemName: download.isFailed ? "exclamationmark.circle.fill" : "arrow.down.circle")
                        .font(.system(size: 15))
                        .foregroundStyle(download.isFailed ? HexTheme.danger : HexTheme.textMuted)
                    VStack(alignment: .leading, spacing: 3) {
                        Text(status)
                            .font(.system(size: 13))
                            .foregroundStyle(HexTheme.textBase)
                            .monospacedDigit()
                            .fixedSize(horizontal: false, vertical: true)
                            .accessibilityIdentifier("multilingual-model-status")
                        Text("Until then, Hex uses its usual model.")
                            .font(.system(size: 12))
                            .foregroundStyle(HexTheme.textMuted)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    Spacer(minLength: 0)
                }
                if let fraction = download.fraction {
                    ProgressView(value: fraction)
                        .tint(HexTheme.textBase)
                }
                if download.isFailed {
                    HStack(spacing: 8) {
                        Button("Try again", action: retry)
                            .accessibilityIdentifier("multilingual-model-try-again")
                        Button("Turn off", action: turnOff)
                            .accessibilityIdentifier("multilingual-model-turn-off")
                    }
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
                    .buttonStyle(MultilingualStatusButtonStyle())
                }
            }
            .padding(14)
            .background(
                download.isFailed ? HexTheme.dangerSurface : Color.white.opacity(0.055),
                in: RoundedRectangle(cornerRadius: 14, style: .continuous)
            )
        }
    }
}

private struct MultilingualStatusButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(Color.white.opacity(configuration.isPressed ? 0.15 : 0.09), in: Capsule())
    }
}
