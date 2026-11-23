import Foundation
import Network
import Observation
import SwiftUI

/// Settings › Dictation › Speech model in keyboard: gets Parakeet V2 into the App Group for the keyboard,
/// with FluidAudio's download (each file resumes from its `.partial`), at launch, whenever Hex
/// comes back to the front, on Wi-Fi coming back and once the keyboard measured its memory,
/// until every file is there. By itself only where the keyboard would use it (`hold`);
/// turned on by hand, at once on any connection.
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

    /// Why it doesn't download by itself yet.
    enum Hold: Equatable {
        /// Two-language dictation or the Multilingual model takes the keyboard's dictation.
        case otherEngine
        case notEnglish
        /// The keyboard hasn't opened on this iPhone yet, so its memory limit isn't known.
        case keyboardNotMeasured
        case keyboardTooSmall
        /// Cellular, a personal hotspot, Low Data Mode or no connection.
        case notOnWiFi
        case needsSpace
    }

    private(set) var state = State.idle
    @ObservationIgnored private var task: Task<Void, Never>?
    @ObservationIgnored private var network: NWPathMonitor?
    /// Whether the connection is one to download 464 MB on, once `network` said.
    @ObservationIgnored private var isUnmetered: Bool?

    /// What the download weighs: Parakeet V2's four models and vocabulary, about 464 MB.
    static let bytes: Int64 = 464_400_000
    /// The keyboard's compiled copy in its own Caches, measured on an iPhone 17 Pro.
    static let compiledBytes: Int64 = 234_000_000
    /// Free space it waits for: the model and the keyboard's copy, with room to spare.
    nonisolated static let freeSpaceNeeded: Int64 = 2_000_000_000

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
        Self.hold(
            plan: KeyboardParakeetPlan.current(),
            unmetered: isUnmetered,
            freeBytes: Self.freeBytes()
        )
    }

    /// Only for a keyboard that would use it: resolved from `KeyboardParakeetPlan`, plus a
    /// connection that isn't cellular, a hotspot or Low Data Mode, and `freeSpaceNeeded` free.
    nonisolated static func hold(
        plan: KeyboardParakeetPlan,
        unmetered: Bool?,
        freeBytes: Int64?
    ) -> Hold? {
        switch plan {
        case .twoLanguage, .multilingual:
            return .otherEngine
        case .notEnglish:
            return .notEnglish
        case .unmeasured:
            return .keyboardNotMeasured
        case .tooSmall:
            return .keyboardTooSmall
        case .off, .awaitingV2Download, .readyV2:
            guard unmetered == true else { return .notOnWiFi }
            if let freeBytes, freeBytes < freeSpaceNeeded { return .needsSpace }
            return nil
        }
    }

    /// Convenience over the individual inputs; delegates to `KeyboardParakeetPlan`.
    nonisolated static func hold(
        otherEngine: Bool,
        english: Bool,
        keyboardLimit: Int?,
        unmetered: Bool?,
        freeBytes: Int64?
    ) -> Hold? {
        let plan = KeyboardParakeetPlan.resolve(
            isEnabled: true,
            fitsDevice: true,
            twoLanguageEnabled: otherEngine,
            multilingualLanguage: nil,
            wantedLanguage: english ? "en-US" : "fr-FR",
            keyboardLimit: keyboardLimit,
            v2Downloaded: false,
            ultraDownloaded: false
        )
        return hold(plan: plan, unmetered: unmetered, freeBytes: freeBytes)
    }

    /// Whether a connection lets the download start by itself: up, not expensive (a personal
    /// hotspot, or cellular as iOS usually prices it), not Low Data Mode, and not over cellular
    /// at all, because iOS reports 5G as not expensive under "Allow More Data on 5G".
    nonisolated static func isUnmetered(satisfied: Bool, expensive: Bool, constrained: Bool, cellular: Bool) -> Bool {
        satisfied && !expensive && !constrained && !cellular
    }

    nonisolated static func freeBytes() -> Int64? {
        let home = URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
        return try? home.resourceValues(forKeys: [.volumeAvailableCapacityForImportantUsageKey])
            .volumeAvailableCapacityForImportantUsage
    }

    func startIfWanted() {
        let settings = KeyboardParakeetSettings()
        guard settings.isEnabled, KeyboardParakeetSettings.fits(), !isDownloaded, task == nil else {
            return
        }
        if !settings.isTurnedOnByHand {
            watch()
            guard hold == nil else { return }
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

    /// `--snap-state model-card-waiting|downloading|ready`: Home's card in each state on the
    /// simulator. The keyboard's limit is cleared, so nothing really downloads.
    func applyFixture(_ scene: String) {
        let group = UserDefaults(suiteName: KeyboardBridge.appGroup)
        for key in [KeyboardParakeetSettings.enabledKey, KeyboardParakeetSettings.keyboardLimitKey, SpeechModelHomeCard.dismissedKey] {
            group?.removeObject(forKey: key)
        }
        // English dictation, whatever the iPhone's language, so the card shows in every locale.
        LanguageSettings().dictationLanguage = "en-US"
        SpeechModelStore.clearComplete(ParakeetTranscriber.cacheDirectory)
        KeyboardParakeetSettings().setKeyboardState(nil)
        switch scene {
        case "model-card-downloading":
            state = .downloading(0.42)
        case "model-card-ready":
            try? FileManager.default.createDirectory(at: ParakeetTranscriber.cacheDirectory, withIntermediateDirectories: true)
            SpeechModelStore.markComplete(ParakeetTranscriber.cacheDirectory)
            KeyboardParakeetSettings().setKeyboardState(.ready)
        default:
            break
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
            let unmetered = KeyboardParakeetDownload.isUnmetered(
                satisfied: path.status == .satisfied,
                expensive: path.isExpensive,
                constrained: path.isConstrained,
                cellular: path.usesInterfaceType(.cellular)
            )
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

/// Under the switch: what the download waits for, downloading, ready to get ready in the
/// keyboard, getting ready, ready, or why the keyboard isn't using it, in a few words. The
/// keyboard's word stays only while fresh.
struct KeyboardParakeetStatus: View {
    let download: KeyboardParakeetDownload
    /// In a box of its own (Settings); Home's card draws the box around it instead.
    var framed = true

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
                .padding(framed ? 14 : 0)
                .background(framed ? Color.white.opacity(0.055) : .clear, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
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
        guard download.isDownloaded else { return Self.line(for: download.hold) }
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

/// Home's card about Speech model in keyboard: what it is, then where its download stands in
/// the status lines Settings › Dictation shows; a tap opens that page. Only where the keyboard
/// would use it (the switch on, an iPhone that fits, none of the holds that mean it never
/// will); once it's ready it can be dismissed, and a dismissed card never comes back.
struct SpeechModelHomeCard: View {
    let download: KeyboardParakeetDownload
    /// Opens Settings › Dictation, where the switch is.
    let open: () -> Void
    @State private var dismissed = SpeechModelHomeCard.isDismissed()
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    static let dismissedKey = "home.speechModelCard.dismissed"

    static func isDismissed(_ defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) -> Bool {
        defaults?.bool(forKey: dismissedKey) ?? false
    }

    static func dismiss(_ defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        defaults?.set(true, forKey: dismissedKey)
    }

    static func shows(enabled: Bool, fits: Bool, hold: KeyboardParakeetDownload.Hold?, dismissed: Bool) -> Bool {
        guard enabled, fits, !dismissed else { return false }
        switch hold {
        case .otherEngine?, .notEnglish?, .keyboardTooSmall?:
            return false
        default:
            return true
        }
    }

    var body: some View {
        TimelineView(.periodic(from: .now, by: 2)) { _ in
            let settings = KeyboardParakeetSettings()
            if Self.shows(enabled: settings.isEnabled, fits: KeyboardParakeetSettings.fits(), hold: download.hold, dismissed: dismissed) {
                card(ready: download.isDownloaded && settings.keyboardState?.state == .ready)
                    .transition(.opacity.combined(with: .scale(scale: 0.98)))
            }
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: dismissed)
    }

    private func card(ready: Bool) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "waveform")
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
                    .padding(.top, 1)
                VStack(alignment: .leading, spacing: 3) {
                    Text("New: Hex's own speech model")
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(HexTheme.textBase)
                    Text("English dictation, right in the keyboard.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .fixedSize(horizontal: false, vertical: true)
                }
                Spacer(minLength: 4)
                if !ready {
                    Image(systemName: "chevron.right")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(width: 28, height: 28)
                }
                if ready {
                    Button {
                        Self.dismiss()
                        dismissed = true
                    } label: {
                        Image(systemName: "xmark")
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(HexTheme.textMuted)
                            .frame(width: 28, height: 28)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Dismiss")
                }
            }
            KeyboardParakeetStatus(download: download, framed: false)
                .padding(.leading, 25)
        }
        .padding(14)
        .background(
            Color.white.opacity(0.055),
            in: RoundedRectangle(cornerRadius: 14, style: .continuous)
        )
        .contentShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        .onTapGesture(perform: open)
        .accessibilityElement(children: .contain)
        .accessibilityAddTraits(.isButton)
        .accessibilityAction(named: Text("Dictation"), open)
        .accessibilityIdentifier("speech-model-card")
    }
}
