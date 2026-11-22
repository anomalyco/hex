import Foundation

/// Experiments › Speech model in keyboard, in the App Group so the keyboard reads it. Off by
/// default. While it's on, the keyboard's own dictation transcribes with Hex's speech model
/// (Parakeet V2 for English, Parakeet Ultra where Experiments › Multilingual model dictates)
/// on the Neural Engine, without opening Hex, once the files are in the App Group and the
/// keyboard has compiled the model. Whenever that isn't so, or isn't safe, it uses Apple's
/// speech recognition exactly as with the experiment off.
nonisolated struct KeyboardParakeetSettings {
    static let enabledKey = "experiments.keyboardParakeet"
    /// What the keyboard last said about it (`KeyboardState`), for the app's Experiments row.
    static let keyboardStateKey = "experiments.keyboardParakeet.keyboardState"
    static let keyboardStateAtKey = "experiments.keyboardParakeet.keyboardStateAt"
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
        get { defaults?.bool(forKey: Self.enabledKey) ?? false }
        nonmutating set { defaults?.set(newValue, forKey: Self.enabledKey) }
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

    func setKeyboardState(_ state: KeyboardState?, now: Date = Date()) {
        defaults?.set(state?.rawValue, forKey: Self.keyboardStateKey)
        defaults?.set(now.timeIntervalSince1970, forKey: Self.keyboardStateAtKey)
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

/// How much memory Parakeet needs in the keyboard, measured on an iPhone 17 Pro on iOS 27.2 (Xcode
/// 27.2 build), whose keyboard limit is 177 MB. The Neural Engine's copy of the model (468 MB for
/// V2, 607 MB for Ultra) is outside the footprint iOS limits; what counts:
/// - Hex's keyboard records at 56 MB (the capture and recording screen), with or without the model.
/// - V2's transcription rose 16 MB above that, to 72 MB; its first load, compiling, rose 42 MB in an
///   idle keyboard (probe keyboard, `~/dev/experiments/2026-11-22-hex-keyboard-asr/native`).
/// - Ultra, in the probe keyboard at the same footprint, peaked 19 MB above V2 and its first load
///   rose 48 MB; not yet measured in Hex's keyboard.
/// So a keyboard needs a limit of the recording footprint, the transcription's rise and `margin`
/// (96 MB for V2, 115 MB for Ultra), as much left (`os_proc_available_memory`) as the load's rise
/// plus 16 MB before it loads, and the transcription's rise plus 8 MB before each transcription.
nonisolated enum KeyboardParakeetBudget {
    static let megabyte = 1_048_576
    static let recordingFootprint = 56 * megabyte
    static let margin = 24 * megabyte

    static func transcriptionRise(multilingual: Bool) -> Int {
        (multilingual ? 16 + 19 : 16) * megabyte
    }

    static func loadRise(multilingual: Bool) -> Int {
        (multilingual ? 48 : 42) * megabyte
    }

    static func requiredLimit(multilingual: Bool) -> Int {
        recordingFootprint + transcriptionRise(multilingual: multilingual) + margin
    }

    static func loadHeadroom(multilingual: Bool) -> Int {
        loadRise(multilingual: multilingual) + 16 * megabyte
    }

    static func transcribeHeadroom(multilingual: Bool) -> Int {
        transcriptionRise(multilingual: multilingual) + 8 * megabyte
    }

    /// Whether a keyboard with `footprint` and `available` (bytes) may load the model.
    /// `available` 0 is a process iOS sets no limit for (the simulator): it may.
    static func allowsLoad(multilingual: Bool, footprint: Int, available: Int) -> Bool {
        guard available > 0 else { return true }
        return footprint + available >= requiredLimit(multilingual: multilingual)
            && available >= loadHeadroom(multilingual: multilingual)
    }

    static func allowsTranscription(multilingual: Bool, footprint: Int, available: Int) -> Bool {
        guard available > 0 else { return true }
        return footprint + available >= requiredLimit(multilingual: multilingual)
            && available >= transcribeHeadroom(multilingual: multilingual)
    }
}
