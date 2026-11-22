import Foundation
import Network
import Observation
import SwiftUI

/// Settings › Dictation › Speech model in keyboard: gets Parakeet V2 into the App Group for the keyboard
/// from Hex (`ParakeetTranscriber.downloadModel`), at launch, whenever Hex comes back to the
/// front, on Wi-Fi coming back and once the keyboard measured its memory, until every file is
/// there. By itself only where the keyboard would use it (`hold`) and while the keyboard isn't
/// downloading it itself (`SpeechModelBackgroundDownload`); turned on by hand, at once on any
/// connection.
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

    typealias Hold = KeyboardParakeetDownloadPolicy.Hold

    private(set) var state = State.idle
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var network: NWPathMonitor?
    /// Whether the connection is one to download 464 MB on, once `network` said.
    @ObservationIgnored private var isUnmetered: Bool?

    /// What the download weighs: Parakeet V2's four models and vocabulary, about 464 MB.
    static let bytes: Int64 = 464_400_000
    /// The keyboard's compiled copy in its own Caches, measured on an iPhone 17 Pro.
    static let compiledBytes: Int64 = 234_000_000
    nonisolated static var freeSpaceNeeded: Int64 { KeyboardParakeetDownloadPolicy.freeSpaceNeeded }

    /// What it takes on the iPhone, the download and the keyboard's compiled copy, to 10 MB.
    static var size: String {
        let total = (bytes + compiledBytes + 5_000_000) / 10_000_000 * 10_000_000
        return format(total)
    }

    static func format(_ bytes: Int64) -> String {
        ByteCountFormatter.string(fromByteCount: bytes, countStyle: .file)
            .replacingOccurrences(of: " ", with: "\u{00A0}")
    }

    var isDownloaded: Bool { SpeechModelStore.isComplete(ParakeetTranscriber.cacheDirectory) }

    /// What holds the download back now, nil once it may start by itself.
    var hold: Hold? {
        KeyboardParakeetDownloadPolicy.hold(unmetered: isUnmetered)
    }

    nonisolated static func hold(
        otherEngine: Bool,
        english: Bool,
        keyboardLimit: Int?,
        unmetered: Bool?,
        freeBytes: Int64?
    ) -> Hold? {
        KeyboardParakeetDownloadPolicy.hold(
            otherEngine: otherEngine, english: english, keyboardLimit: keyboardLimit, unmetered: unmetered, freeBytes: freeBytes
        )
    }

    /// The keyboard downloads it itself now (`SpeechModelBackgroundDownload`).
    var keyboardDownloads: Bool { SpeechModelDownloadClaim().isLive(.keyboard) }

    /// Known to be off Wi-Fi, where the keyboard's download waits.
    var isOffWiFi: Bool { isUnmetered == false }

    func startIfWanted() {
        let settings = KeyboardParakeetSettings()
        guard settings.isEnabled, KeyboardParakeetSettings.fits(), !isDownloaded, task == nil else {
            return
        }
        if !settings.isTurnedOnByHand {
            watch()
            guard hold == nil, !keyboardDownloads else { return }
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

    /// The connection, and the keyboard's word that it measured its memory, both of which can
    /// let a held download start while Hex is open.
    private func watch() {
        guard network == nil else { return }
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { path in
            let unmetered = path.status == .satisfied && !path.isExpensive && !path.isConstrained
            Task { @MainActor in KeyboardParakeetDownload.shared.networkChanged(unmetered) }
        }
        monitor.start(queue: DispatchQueue.global(qos: .utility))
        network = monitor
        CFNotificationCenterAddObserver(
            CFNotificationCenterGetDarwinNotifyCenter(),
            nil,
            { _, _, _, _, _ in
                Task { @MainActor in DictationController.shared.startKeyboardParakeetDownloadIfWanted() }
            },
            KeyboardParakeetSettings.keyboardLimitNotification as CFString,
            nil,
            .deliverImmediately
        )
    }

    /// Off Wi-Fi, a download Hex started by itself stops (its files resume on Wi-Fi).
    private func networkChanged(_ unmetered: Bool) {
        guard unmetered != isUnmetered else { return }
        isUnmetered = unmetered
        if unmetered {
            DictationController.shared.startKeyboardParakeetDownloadIfWanted()
        } else if task != nil, !KeyboardParakeetSettings().isTurnedOnByHand {
            Task { await stop() }
        }
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

/// Settings › Dictation › Speech model in keyboard: on unless turned off, the keyboard's own
/// English dictation transcribes with Parakeet once it's downloaded and compiled there; off, it
/// keeps Apple's speech recognition. Under the switch, what it's doing (`KeyboardParakeetStatus`).
struct KeyboardParakeetRow: View {
    @State private var isOn = KeyboardParakeetSettings().isOn
    private let fits = KeyboardParakeetSettings.fits()

    var body: some View {
        SettingsToggleRow(
            title: String(localized: "Speech model in keyboard"),
            detail: fits
                ? String(localized: "For English. Uses about \(KeyboardParakeetDownload.size), downloaded on Wi-Fi. Until then, the keyboard uses Apple's speech recognition.")
                : String(localized: "Needs an iPhone with 6 GB of memory or more"),
            isOn: $isOn
        )
        .disabled(!fits)
        .onChange(of: isOn) { _, enabled in
            guard enabled != KeyboardParakeetSettings().isEnabled else { return }
            KeyboardParakeetSettings().isEnabled = enabled
            DictationController.shared.keyboardParakeetChanged()
        }
        .onAppear { isOn = KeyboardParakeetSettings().isOn }

        if isOn {
            KeyboardParakeetStatus(download: KeyboardParakeetDownload.shared)
                .padding(.horizontal, 12)
                .padding(.bottom, 12)
        }
    }
}

/// Under the switch: what the download waits for, downloading, ready to get ready in the
/// keyboard, getting ready, ready, or why the keyboard isn't using it, in a few words. The
/// keyboard's word stays only while fresh.
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
                    if let fraction = fraction(now: context.date) {
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

    /// Hex's own download's share, else the keyboard's while it says its bytes.
    private func fraction(now: Date) -> Double? {
        if case .downloading(let fraction) = download.state { return fraction }
        guard !download.isDownloaded, download.keyboardDownloads,
              let bytes = SpeechModelDownloadClaim().freshBytes(now: now) else { return nil }
        return min(1, Double(bytes) / Double(ParakeetV2Files.totalBytes))
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
        guard download.isDownloaded else {
            if download.keyboardDownloads {
                return download.isOffWiFi ? String(localized: "Downloads on Wi-Fi.") : String(localized: "Downloading the speech model")
            }
            return Self.line(for: download.hold)
        }
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

    private static func line(for hold: KeyboardParakeetDownload.Hold?) -> String? {
        switch hold {
        case .notEnglish:
            String(localized: "Only for English dictation.")
        case .keyboardNotMeasured:
            String(localized: "Downloads after you first use the Hex keyboard.")
        case .keyboardTooSmall:
            String(localized: "The keyboard can't use it right now, so it uses Apple's speech recognition.")
        case .notOnWiFi:
            String(localized: "Downloads on Wi-Fi.")
        case .needsSpace:
            String(localized: "Downloads once the iPhone has \(KeyboardParakeetDownload.format(KeyboardParakeetDownload.freeSpaceNeeded)) free.")
        case .otherEngine, nil:
            nil
        }
    }
}
