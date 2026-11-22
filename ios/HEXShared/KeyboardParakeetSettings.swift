import Foundation

/// Experiments › Speech model in keyboard, in the App Group so the keyboard reads it. On unless
/// the person turned it off. While it's on, the keyboard's own dictation transcribes with Hex's
/// speech model (Parakeet V2 for English, Parakeet Ultra where Experiments › Multilingual model
/// dictates) on the Neural Engine, without opening Hex, once the files are in the App Group and
/// the keyboard has compiled the model. Whenever that isn't so, or isn't safe, it uses Apple's
/// speech recognition exactly as with the switch off. Hex downloads the model by itself only
/// where the keyboard can use it (`KeyboardParakeetDownload.hold`).
nonisolated struct KeyboardParakeetSettings {
    static let enabledKey = "experiments.keyboardParakeet"
    /// What the keyboard last said about it (`KeyboardState`), for the app's Experiments row.
    static let keyboardStateKey = "experiments.keyboardParakeet.keyboardState"
    static let keyboardStateAtKey = "experiments.keyboardParakeet.keyboardStateAt"
    /// Whether that state is the multilingual model's (Parakeet Ultra) rather than V2's.
    static let keyboardStateMultilingualKey = "experiments.keyboardParakeet.keyboardStateMultilingual"
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
        /// This keyboard can't use it (its memory limit, no Neural Engine, a load that failed).
        case unavailable
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

    func setKeyboardState(_ state: KeyboardState?, multilingual: Bool = false, now: Date = Date()) {
        defaults?.set(state?.rawValue, forKey: Self.keyboardStateKey)
        defaults?.set(multilingual, forKey: Self.keyboardStateMultilingualKey)
        defaults?.set(now.timeIntervalSince1970, forKey: Self.keyboardStateAtKey)
    }

    /// The keyboard has the multilingual model loaded, so it dictates that model's language
    /// itself instead of opening Hex (`KeyboardSelfDictation`).
    var keyboardHasMultilingualModel: Bool {
        isEnabled && keyboardState?.state == .ready && defaults?.bool(forKey: Self.keyboardStateMultilingualKey) == true
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

/// How much memory Parakeet needs in the keyboard, measured in Hex's keyboard on an iPhone 17 Pro
/// on iOS 27.2, whose keyboard limit is 177 MB. The Neural Engine's copy of the model (468 MB for
/// V2, 607 MB for Ultra) is outside the footprint iOS limits; what counts:
/// - Hex's keyboard records at 56 MB (the capture and recording screen), with or without the model.
/// - V2's first transcription rose 16 MB above that, to 72 MB; Ultra's rose 9 MB, to 66 MB, so
///   both use V2's numbers. The first load, compiling, rose 42 MB in an idle probe keyboard
///   (`~/dev/experiments/2026-11-22-hex-keyboard-asr/native`).
/// So a keyboard needs a limit of the recording footprint, the transcription's rise and `margin`
/// (96 MB), as much left (`os_proc_available_memory`) as the load's rise plus 16 MB before it
/// loads, and the transcription's rise plus 8 MB before each transcription.
nonisolated enum KeyboardParakeetBudget {
    static let megabyte = 1_048_576
    static let recordingFootprint = 56 * megabyte
    static let margin = 24 * megabyte
    static let transcriptionRise = 16 * megabyte
    static let loadRise = 42 * megabyte

    static var requiredLimit: Int { recordingFootprint + transcriptionRise + margin }
    static var loadHeadroom: Int { loadRise + 16 * megabyte }
    static var transcribeHeadroom: Int { transcriptionRise + 8 * megabyte }

    /// Whether a keyboard with `footprint` and `available` (bytes) may load the model.
    /// `available` 0 is a process iOS sets no limit for (the simulator): it may.
    static func allowsLoad(footprint: Int, available: Int) -> Bool {
        guard available > 0 else { return true }
        return footprint + available >= requiredLimit && available >= loadHeadroom
    }

    static func allowsTranscription(footprint: Int, available: Int) -> Bool {
        guard available > 0 else { return true }
        return footprint + available >= requiredLimit && available >= transcribeHeadroom
    }
}
