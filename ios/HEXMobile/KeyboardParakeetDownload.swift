import Foundation
import Network
import Observation
import SwiftUI

/// Settings › Dictation › Model's two downloads, English's and Multilingual's, start by themselves
/// only on Wi-Fi (`isUnmetered`: not cellular, a personal hotspot or Low Data Mode). Use mobile
/// data on the line that says so lets that one model's download use any connection until it is
/// on the iPhone, a stop and a try again included; picking the other model asks again
/// (`forgetMobileData`). The decision is kept in the App Group, so a download Hex picks up at
/// the next launch keeps it. `--simulate-cellular` (simulator scenes) reads as off Wi-Fi.
@MainActor
@Observable
final class SpeechModelDownloadNetwork {
    static let shared = SpeechModelDownloadNetwork(
        isUnmetered: ProcessInfo.processInfo.arguments.contains(cellularLaunchArgument) ? false : nil,
        watchesConnection: !ProcessInfo.processInfo.arguments.contains(cellularLaunchArgument)
    )
    static let mobileDataKey = "settings.speechModel.mobileData"
    static let cellularLaunchArgument = "--simulate-cellular"

    /// Whether the connection is one to download on by itself; nil until the monitor says.
    private(set) var isUnmetered: Bool?
    /// The model whose download may use mobile data (`SpeechModelChoice.rawValue`).
    private(set) var mobileDataModel: String?
    @ObservationIgnored private let defaults: UserDefaults?
    @ObservationIgnored private let watchesConnection: Bool
    @ObservationIgnored private var monitor: NWPathMonitor?
    @ObservationIgnored private var observers: [@MainActor (Bool) -> Void] = []

    init(
        defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup),
        isUnmetered: Bool? = nil,
        watchesConnection: Bool = true
    ) {
        self.defaults = defaults
        self.isUnmetered = isUnmetered
        self.watchesConnection = watchesConnection
        mobileDataModel = defaults?.string(forKey: Self.mobileDataKey)
    }

    func usesMobileData(for model: SpeechModelChoice) -> Bool {
        mobileDataModel == model.rawValue
    }

    /// Whether `model`'s download may start or go on now.
    func mayDownload(_ model: SpeechModelChoice) -> Bool {
        isUnmetered == true || usesMobileData(for: model)
    }

    /// Use mobile data, tapped on `model`'s waiting line.
    func allowMobileData(for model: SpeechModelChoice) {
        mobileDataModel = model.rawValue
        defaults?.set(model.rawValue, forKey: Self.mobileDataKey)
    }

    /// `model` is on the iPhone, or (nil) the other model was picked: the next download asks again.
    func forgetMobileData(for model: SpeechModelChoice? = nil) {
        guard let current = mobileDataModel, model == nil || model?.rawValue == current else { return }
        mobileDataModel = nil
        defaults?.removeObject(forKey: Self.mobileDataKey)
    }

    /// Told of every change of connection after this.
    func observe(_ observer: @escaping @MainActor (Bool) -> Void) {
        observers.append(observer)
    }

    /// A connection report: a change tells each observer.
    func report(unmetered: Bool) {
        guard unmetered != isUnmetered else { return }
        isUnmetered = unmetered
        for observer in observers {
            observer(unmetered)
        }
    }

    func watch() {
        guard watchesConnection, monitor == nil else { return }
        let monitor = NWPathMonitor()
        monitor.pathUpdateHandler = { [weak self] path in
            let unmetered = KeyboardParakeetDownload.isUnmetered(
                satisfied: path.status == .satisfied,
                expensive: path.isExpensive,
                constrained: path.isConstrained,
                cellular: path.usesInterfaceType(.cellular)
            )
            Task { @MainActor in self?.report(unmetered: unmetered) }
        }
        monitor.start(queue: DispatchQueue.global(qos: .utility))
        self.monitor = monitor
    }
}

/// Settings › Dictation › Speech model in keyboard: gets Parakeet V2 into the App Group for the keyboard,
/// with FluidAudio's download (each file resumes from its `.partial`), at launch, whenever Hex
/// comes back to the front, on Wi-Fi coming back and once the keyboard measured its memory,
/// until every file is there. By itself only where the keyboard would use it (`hold`), on Wi-Fi
/// or after Use mobile data (`SpeechModelDownloadNetwork`); turned on by hand, at once on any
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
    @ObservationIgnored private var watching = false

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
        let network = SpeechModelDownloadNetwork.shared
        return Self.hold(
            plan: KeyboardParakeetPlan.current(),
            unmetered: network.isUnmetered,
            freeBytes: Self.freeBytes(),
            mobileData: network.usesMobileData(for: .english)
        )
    }

    /// Only for a keyboard that would use it: resolved from `KeyboardParakeetPlan`, plus a
    /// connection that isn't cellular, a hotspot or Low Data Mode (unless Use mobile data was
    /// tapped for it), and `freeSpaceNeeded` free.
    nonisolated static func hold(
        plan: KeyboardParakeetPlan,
        unmetered: Bool?,
        freeBytes: Int64?,
        mobileData: Bool = false
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
        case .off, .awaitingV2Download, .awaitingV2DownloadUsingUltra, .readyV2:
            guard unmetered == true || mobileData else { return .notOnWiFi }
            if let freeBytes, freeBytes < freeSpaceNeeded { return .needsSpace }
            return nil
        }
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
                SpeechModelDownloadNetwork.shared.forgetMobileData(for: .english)
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

    /// Use mobile data on the line that says it downloads on Wi-Fi: starts now, on any connection.
    func useMobileData() {
        SpeechModelDownloadNetwork.shared.allowMobileData(for: .english)
        DictationController.shared.startKeyboardParakeetDownloadIfWanted()
    }

    /// The connection, and the keyboard's word that it measured its memory, both of which can
    /// let a held download start while Hex is open.
    private func watch() {
        SpeechModelDownloadNetwork.shared.watch()
        guard !watching else { return }
        watching = true
        SpeechModelDownloadNetwork.shared.observe { unmetered in
            KeyboardParakeetDownload.shared.networkChanged(unmetered)
        }
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

    /// Off Wi-Fi, a download Hex started by itself stops (its files resume on Wi-Fi), unless Use
    /// mobile data was tapped for it.
    private func networkChanged(_ unmetered: Bool) {
        if unmetered {
            DictationController.shared.startKeyboardParakeetDownloadIfWanted()
        } else if task != nil, !KeyboardParakeetSettings().isTurnedOnByHand,
                  !SpeechModelDownloadNetwork.shared.usesMobileData(for: .english) {
            Task { await stop() }
        }
    }
}

extension DictationController {
    /// The switch in Settings › Dictation: on downloads Parakeet V2 for the keyboard unless it's there;
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
    /// About the multilingual model (Settings › Dictation › Model) rather than English's.
    var multilingual = false
    /// Use mobile data beside "Downloads on Wi-Fi." (Settings; Home's card opens Settings).
    var offersMobileData = false

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
                    // Only beside a load that failed, which the keyboard doesn't try again in that
                    // process by itself.
                    if line == Self.unavailableLine(.loadFailed) {
                        Button("Try again") { Self.retryInKeyboard() }
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(HexTheme.textBase)
                            .buttonStyle(MultilingualStatusButtonStyle())
                            .accessibilityIdentifier("keyboard-parakeet-retry-load")
                    }
                    if offersMobileData, !multilingual, download.state == .idle, !download.isDownloaded,
                       download.hold == .notOnWiFi {
                        Button("Use mobile data") { download.useMobileData() }
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(HexTheme.textBase)
                            .buttonStyle(MultilingualStatusButtonStyle())
                            .accessibilityIdentifier("keyboard-parakeet-use-mobile-data")
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(framed ? 14 : 0)
                .background(framed ? Color.white.opacity(0.055) : .clear, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
            }
        }
    }

    private func line(now: Date) -> String? {
        let settings = KeyboardParakeetSettings()
        guard multilingual else {
            return Self.line(
                state: download.state,
                isDownloaded: download.isDownloaded,
                hold: download.hold,
                keyboardState: settings.keyboardState,
                reason: settings.unavailableReason,
                now: now
            )
        }
        return Self.multilingualLine(
            isDownloaded: ParakeetUltraModel.isDownloaded(),
            takesLanguage: MultilingualParakeetSettings().language() != nil,
            fitsKeyboard: settings.keyboardLimit.map { $0 >= KeyboardParakeetBudget.requiredLimit } ?? true,
            keyboardState: settings.keyboardStateIsMultilingual ? settings.keyboardState : nil,
            reason: settings.keyboardStateIsMultilingual ? settings.unavailableReason : nil,
            now: now
        )
    }

    /// Try again beside "The keyboard couldn't load it": the keyboard drops its compiled copies
    /// and loads it again the next time it opens, even in the process that set it aside;
    /// meanwhile the row says it gets it ready then.
    static func retryInKeyboard(_ settings: KeyboardParakeetSettings = KeyboardParakeetSettings()) {
        settings.purgesCompiled = true
        settings.setKeyboardState(nil)
    }

    /// Under Model: Multilingual. Its download says where it stands itself
    /// (`MultilingualModelStatus`); once it's on the iPhone, what the keyboard does with it.
    static func multilingualLine(
        isDownloaded: Bool,
        takesLanguage: Bool,
        fitsKeyboard: Bool,
        keyboardState: (state: KeyboardParakeetSettings.KeyboardState, at: Date)?,
        reason: KeyboardParakeetSettings.UnavailableReason? = nil,
        now: Date
    ) -> String? {
        guard isDownloaded else { return nil }
        guard takesLanguage else {
            return String(localized: "Not for this dictation language, so the keyboard uses Apple's speech recognition.")
        }
        guard fitsKeyboard else { return line(for: .keyboardTooSmall) }
        return keyboardLine(keyboardState, reason: reason, now: now)
    }

    /// What the row says. The keyboard's word is about English: dictating in another language
    /// (Hinglish, French), Apple's speech recognition takes it, so a downloaded model says
    /// it's only for English rather than "Ready in the keyboard".
    static func line(
        state: KeyboardParakeetDownload.State,
        isDownloaded: Bool,
        hold: KeyboardParakeetDownload.Hold?,
        keyboardState: (state: KeyboardParakeetSettings.KeyboardState, at: Date)?,
        reason: KeyboardParakeetSettings.UnavailableReason? = nil,
        now: Date
    ) -> String? {
        switch state {
        case .downloading:
            return String(localized: "Downloading the speech model")
        case .failed(let message):
            return message
        case .idle:
            break
        }
        // A limit under 131 MB never loads, downloaded or not (a download turned on by hand
        // skips the hold): it says so, not what the keyboard last said.
        guard isDownloaded, hold != .notEnglish, hold != .keyboardTooSmall else { return line(for: hold) }
        return keyboardLine(keyboardState, reason: reason, now: now)
    }

    /// What the keyboard did with a downloaded model, while its word is fresh. "Getting ready"
    /// lasts 3 minutes; the rest stand until the keyboard says something else, so each says
    /// what happens next rather than that it's so now.
    private static func keyboardLine(
        _ keyboardState: (state: KeyboardParakeetSettings.KeyboardState, at: Date)?,
        reason: KeyboardParakeetSettings.UnavailableReason?,
        now: Date
    ) -> String {
        switch keyboardState {
        case (.preparing, let at)? where now.timeIntervalSince(at) < 180:
            return String(localized: "Getting ready in the keyboard. After a download or an update, this takes a little longer.")
        case (.ready, _)?:
            return String(localized: "Ready in the keyboard")
        case (.unavailable, _)? where reason != nil:
            return unavailableLine(reason)
        default:
            // Nothing from the keyboard yet, or an older keyboard's `.unavailable` without a
            // reason (forgotten at launch: `forgetUnavailableWithoutReason`).
            return String(localized: "Downloaded. The keyboard gets it ready the next time it opens.")
        }
    }

    /// Why the keyboard couldn't use it (`KeyboardParakeetSettings.UnavailableReason`).
    static func unavailableLine(_ reason: KeyboardParakeetSettings.UnavailableReason?) -> String {
        switch reason {
        case .shortOfMemory?:
            String(localized: "Not enough free memory when the keyboard last opened, so it used Apple's speech recognition. It tries again the next time it opens.")
        case .noNeuralEngine?:
            String(localized: "This iPhone can't run it in the keyboard, so the keyboard uses Apple's speech recognition.")
        case .notOnNeuralEngine?, .loadFailed?:
            String(localized: "The keyboard couldn't load it, so it uses Apple's speech recognition.")
        case nil:
            String(localized: "Downloaded. The keyboard gets it ready the next time it opens.")
        }
    }

    private static func line(for hold: KeyboardParakeetDownload.Hold?) -> String? {
        switch hold {
        case .notEnglish:
            String(localized: "Only for English dictation.")
        case .keyboardNotMeasured:
            String(localized: "Downloads after you first use the Hex keyboard.")
        case .keyboardTooSmall:
            String(localized: "This iPhone's keyboard doesn't have the memory for it, so it uses Apple's speech recognition.")
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

    /// Not where the model can never run in this keyboard (a limit under 131 MB, or no Neural
    /// Engine), where the card couldn't be dismissed either; still where trying again can help
    /// (short of memory, a load that failed, with Try again).
    static func shows(
        enabled: Bool,
        fits: Bool,
        hold: KeyboardParakeetDownload.Hold?,
        dismissed: Bool,
        unavailable: KeyboardParakeetSettings.UnavailableReason? = nil
    ) -> Bool {
        guard enabled, fits, !dismissed, unavailable != .noNeuralEngine else { return false }
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
            if Self.shows(
                enabled: settings.isEnabled,
                fits: KeyboardParakeetSettings.fits(),
                hold: download.hold,
                dismissed: dismissed,
                unavailable: settings.unavailableReason
            ) {
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
