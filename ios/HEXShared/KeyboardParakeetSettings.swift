import Foundation

/// Settings › Dictation › Speech model in keyboard, in the App Group so the keyboard reads it. On unless
/// the person turned it off. While it's on, the keyboard's own dictation transcribes with Hex's
/// speech model (Parakeet V2 for English, Parakeet Ultra where Settings › Dictation › Model: Multilingual
/// dictates) on the Neural Engine, without opening Hex, once the files are in the App Group and
/// the keyboard has compiled the model. Whenever that isn't so, or isn't safe, it uses Apple's
/// speech recognition exactly as with the switch off. Hex downloads the model by itself only
/// where the keyboard can use it (`KeyboardParakeetDownload.hold`).
nonisolated struct KeyboardParakeetSettings {
    static let enabledKey = "experiments.keyboardParakeet"
    /// What the keyboard last said about it (`KeyboardState`), for the app's Speech model in keyboard row.
    static let keyboardStateKey = "experiments.keyboardParakeet.keyboardState"
    static let keyboardStateAtKey = "experiments.keyboardParakeet.keyboardStateAt"
    /// Whether that state is the multilingual model's (Parakeet Ultra) rather than V2's.
    static let keyboardStateMultilingualKey = "experiments.keyboardParakeet.keyboardStateMultilingual"
    /// Why the keyboard said `.unavailable` (`UnavailableReason`); none from builds before 1.0.5.
    static let keyboardStateReasonKey = "experiments.keyboardParakeet.keyboardStateReason"
    /// The keyboard's memory limit on this iPhone, as the keyboard measured it.
    static let keyboardLimitKey = "experiments.keyboardParakeet.keyboardLimit"
    static let keyboardLimitNotification = "anomaly.hex.keyboardParakeet.keyboardLimit"
    static let launchArgument = "--experiment-keyboard-parakeet"

    /// What the keyboard did with the model, written by the keyboard.
    enum KeyboardState: String {
        /// Loading it, the first time a minute or less while Core ML compiles it for the
        /// Neural Engine. The keyboard dictates with Apple's speech recognition meanwhile.
        case preparing
        case ready
        /// This keyboard can't use it (`UnavailableReason` says why).
        case unavailable
    }

    /// Why the keyboard couldn't use it, so Settings › Dictation can say so and what happens next.
    enum UnavailableReason: String {
        /// Too little memory left when the keyboard opened (`KeyboardParakeetBudget.allowsLoad`).
        /// For now: the keyboard tries again the next time it opens.
        case shortOfMemory
        /// No Neural Engine on this device. For good.
        case noNeuralEngine
        /// It loaded, but not on the Neural Engine. Set aside in that keyboard process.
        case notOnNeuralEngine
        /// The load threw. Set aside in that keyboard process until Try again (`purgesCompiled`).
        case loadFailed
    }

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    var isEnabled: Bool {
        get { defaults?.object(forKey: Self.enabledKey) as? Bool ?? true }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
    }

    /// What the switch shows: on where the iPhone has the memory and the person didn't turn it off.
    var isOn: Bool { isEnabled && Self.fits() }

    /// Turned on by the person (in this build or when it was off by default), as stored: its
    /// download starts at once on any connection.
    var isTurnedOnByHand: Bool {
        defaults?.object(forKey: Self.enabledKey) as? Bool == true
    }

    /// Turned off by the person, as stored: an App Group iOS doesn't let Hex read yet (before
    /// the first unlock) reads nil, which must not delete a download.
    var isTurnedOff: Bool {
        defaults?.object(forKey: Self.enabledKey) as? Bool == false
    }

    var keyboardState: (state: KeyboardState, at: Date)? {
        guard let raw = defaults?.string(forKey: Self.keyboardStateKey), let state = KeyboardState(rawValue: raw) else {
            return nil
        }
        let at = defaults?.double(forKey: Self.keyboardStateAtKey) ?? 0
        return (state, Date(timeIntervalSince1970: at))
    }

    /// Settings › Dictation › Model keeps only the picked model: the other goes once the
    /// picked one has loaded once, here (the keyboard) or in Hex (`SpeechModelChoice.unpicked`).
    func hasLoadedOnce(multilingual: Bool) -> Bool {
        defaults?.bool(forKey: Self.loadedOnceKey(multilingual)) == true
    }

    func setLoadedOnce(_ loaded: Bool, multilingual: Bool) {
        defaults?.set(loaded, forKey: Self.loadedOnceKey(multilingual))
    }

    private static func loadedOnceKey(_ multilingual: Bool) -> String {
        multilingual ? "experiments.keyboardParakeet.loadedOnce.ultra" : "experiments.keyboardParakeet.loadedOnce.v2"
    }

    /// Hex removed a model the keyboard may have compiled, or Try again was tapped after a load
    /// that failed: the keyboard clears its compiled copies the next time it starts, before it
    /// loads the picked one, and tries again a model it had set aside (`KeyboardParakeet`).
    static let purgeCompiledKey = "experiments.keyboardParakeet.purgeCompiled"

    var purgesCompiled: Bool {
        get { defaults?.bool(forKey: Self.purgeCompiledKey) == true }
        nonmutating set { defaults?.set(newValue, forKey: Self.purgeCompiledKey) }
    }

    /// Whether `keyboardState` is about the multilingual model rather than English's.
    var keyboardStateIsMultilingual: Bool {
        defaults?.bool(forKey: Self.keyboardStateMultilingualKey) == true
    }

    func setKeyboardState(
        _ state: KeyboardState?,
        multilingual: Bool = false,
        reason: UnavailableReason? = nil,
        now: Date = Date()
    ) {
        // Hex reads these from another process: `.unavailable` gets its reason before the
        // state, so it is never seen without one (`forgetUnavailableWithoutReason`).
        if state == .unavailable {
            defaults?.set(reason?.rawValue, forKey: Self.keyboardStateReasonKey)
        }
        defaults?.set(state?.rawValue, forKey: Self.keyboardStateKey)
        defaults?.set(multilingual, forKey: Self.keyboardStateMultilingualKey)
        defaults?.set(now.timeIntervalSince1970, forKey: Self.keyboardStateAtKey)
        if state != .unavailable {
            defaults?.removeObject(forKey: Self.keyboardStateReasonKey)
        }
    }

    /// A keyboard from before 1.0.5 said `.unavailable` without a reason, and nothing else
    /// clears it until the keyboard loads again, which some never do (a limit under 131 MB:
    /// `KeyboardParakeetPlan.tooSmall` loads nothing and writes nothing). It says nothing true
    /// any more, so Hex forgets it at launch; since 1.0.5 the keyboard always writes a reason.
    @discardableResult
    func forgetUnavailableWithoutReason() -> Bool {
        guard keyboardState?.state == .unavailable,
              defaults?.string(forKey: Self.keyboardStateReasonKey) == nil else { return false }
        setKeyboardState(nil)
        return true
    }

    /// Why the keyboard last said `.unavailable`; nil for any other state.
    var unavailableReason: UnavailableReason? {
        guard keyboardState?.state == .unavailable,
              let raw = defaults?.string(forKey: Self.keyboardStateReasonKey) else { return nil }
        return UnavailableReason(rawValue: raw)
    }

    /// The keyboard's memory limit (bytes), nil until the keyboard first opened on this iPhone.
    var keyboardLimit: Int? {
        defaults?.object(forKey: Self.keyboardLimitKey) as? Int
    }

    /// The keyboard, at each launch: its footprint plus `os_proc_available_memory`. Written, and
    /// Hex told, only when it moved by a megabyte or more.
    func recordKeyboardLimit(_ limit: Int) {
        guard limit > 0 else { return }
        if let known = keyboardLimit, abs(known - limit) < KeyboardParakeetBudget.megabyte { return }
        defaults?.set(limit, forKey: Self.keyboardLimitKey)
        CFNotificationCenterPostNotification(
            CFNotificationCenterGetDarwinNotifyCenter(),
            CFNotificationName(Self.keyboardLimitNotification as CFString),
            nil,
            nil,
            true
        )
    }

    /// As the app's Parakeet: 6 GB iPhones report about 5.5 GB.
    static func fits(physicalMemory: UInt64 = ProcessInfo.processInfo.physicalMemory) -> Bool {
        physicalMemory >= MultilingualParakeetSettings.minimumPhysicalMemory
    }

    /// Simulator scenes pass `--experiment-keyboard-parakeet` to turn the experiment on.
    func applyLaunchArguments(_ arguments: [String] = ProcessInfo.processInfo.arguments) {
        if arguments.contains(Self.launchArgument) {
            isEnabled = true
        }
    }
}

/// The keyboard's New speech model notice (`FullKeyboardView.offerModelNotice`): Hex downloads
/// the model only when it opens on Wi-Fi, so someone who only uses the keyboard would never get
/// it. Shown when the keyboard appears, while it would dictate English with the model and its
/// files aren't in the App Group, at most once a day and three times in all, and never again
/// once the person opened Hex from it or dismissed it with its ✕. The counts are in the App Group.
nonisolated struct KeyboardParakeetNotice {
    static let shownKey = "experiments.keyboardParakeet.notice.shown"
    static let dayKey = "experiments.keyboardParakeet.notice.day"
    static let openedKey = "experiments.keyboardParakeet.notice.opened"
    static let dismissedKey = "experiments.keyboardParakeet.notice.dismissed"
    static let maximumShowings = 3
    /// Opens Hex as it is, starting nothing (Home when it wasn't running), where the model
    /// downloads by itself on Wi-Fi.
    static let opensHex = URL(string: "hex-dictation://keyboard")!

    private let defaults: UserDefaults?

    init(defaults: UserDefaults? = UserDefaults(suiteName: KeyboardBridge.appGroup)) {
        self.defaults = defaults
    }

    /// Not opened from or dismissed, under three showings, and not shown today.
    func mayShow(now: Date = Date(), calendar: Calendar = .current) -> Bool {
        guard let defaults, !defaults.bool(forKey: Self.openedKey), !defaults.bool(forKey: Self.dismissedKey),
              defaults.integer(forKey: Self.shownKey) < Self.maximumShowings else { return false }
        return defaults.string(forKey: Self.dayKey) != Self.day(now, calendar)
    }

    func recordShown(now: Date = Date(), calendar: Calendar = .current) {
        defaults?.set((defaults?.integer(forKey: Self.shownKey) ?? 0) + 1, forKey: Self.shownKey)
        defaults?.set(Self.day(now, calendar), forKey: Self.dayKey)
    }

    func recordOpened() {
        defaults?.set(true, forKey: Self.openedKey)
    }

    /// Its ✕: never again on this iPhone, whatever the counts.
    func recordDismissed() {
        defaults?.set(true, forKey: Self.dismissedKey)
    }

    private static func day(_ date: Date, _ calendar: Calendar) -> String {
        let parts = calendar.dateComponents([.year, .month, .day], from: date)
        return "\(parts.year ?? 0)-\(parts.month ?? 0)-\(parts.day ?? 0)"
    }
}

/// How much memory Parakeet needs in the keyboard, measured in Hex's keyboard on an iPhone 17 Pro
/// on iOS 27.2, whose keyboard limit is 177 MB. The Neural Engine's copy of the model (468 MB for
/// V2, 607 MB for Ultra) is outside the footprint iOS limits; what counts:
/// - Hex's keyboard records at up to 91 MB with the model loaded: over twenty dictations in one
///   keyboard process, recordings started at 74.7–78.0 MB through the ninth and at 87.9–91.4 MB
///   after (56 MB for a first dictation, and with Apple's speech recognition throughout).
/// - V2's first transcription rose 16 MB above the recording; Ultra's rose 9 MB, so both use
///   V2's numbers. The first load, compiling, rose 42 MB in an idle probe keyboard
///   (`~/dev/experiments/2026-11-22-hex-keyboard-asr/native`).
/// - FluidAudio reads the whole recording into memory before it transcribes (the 48 kHz file as
///   floats, a copy of it and the 16 kHz result, `AudioConverter.resampleAudioFile`): 7.9 MB
///   for 18.9 s of audio in Hex's keyboard, about 0.42 MB a second, counted at 0.5.
/// So a keyboard needs a limit of the recording footprint, the transcription's rise and `margin`
/// (131 MB), as much left (`os_proc_available_memory`) as the load's rise plus 16 MB before it
/// loads, and the transcription's rise plus 8 MB plus the recording's own cost before each
/// transcription, which is at most `longestAudio` long. Longer or tighter goes to Apple's speech
/// recognition, which streams the file.
nonisolated enum KeyboardParakeetBudget {
    static let megabyte = 1_048_576
    static let recordingFootprint = 91 * megabyte
    static let margin = 24 * megabyte
    static let transcriptionRise = 16 * megabyte
    static let loadRise = 42 * megabyte

    static var requiredLimit: Int { recordingFootprint + transcriptionRise + margin }
    static var loadHeadroom: Int { loadRise + 16 * megabyte }
    static var transcribeHeadroom: Int { transcriptionRise + 8 * megabyte }
    /// What a second of audio costs while FluidAudio holds the recording.
    static let audioCostPerSecond = megabyte / 2
    /// The longest recording Parakeet transcribes in the keyboard: 45 MB of audio at
    /// `audioCostPerSecond`, which with the 91 MB recording and the 16 MB transcription leaves
    /// 25 MB under an iPhone 17 Pro's 177 MB, a margin the room check alone can't promise if
    /// memory moves while it runs. Almost every dictation is far shorter.
    static let longestAudio: TimeInterval = 90

    /// Room left for a transcription of `audioSeconds` of audio.
    static func transcribeHeadroom(audioSeconds: Double) -> Int {
        transcribeHeadroom + Int(max(0, audioSeconds) * Double(audioCostPerSecond))
    }

    /// Whether a keyboard with `footprint` and `available` (bytes) may load the model.
    /// `available` 0 is a process iOS sets no limit for (the simulator): it may.
    static func allowsLoad(footprint: Int, available: Int) -> Bool {
        guard available > 0 else { return true }
        return footprint + available >= requiredLimit && available >= loadHeadroom
    }

    /// Whether Parakeet may transcribe `audioSeconds` of audio now: no longer than `longestAudio`,
    /// and (where iOS sets a limit) the room it needs left.
    static func allowsTranscription(footprint: Int, available: Int, audioSeconds: Double) -> Bool {
        guard audioSeconds <= longestAudio else { return false }
        guard available > 0 else { return true }
        return footprint + available >= requiredLimit && available >= transcribeHeadroom(audioSeconds: audioSeconds)
    }
}
