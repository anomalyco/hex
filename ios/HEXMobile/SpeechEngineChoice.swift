import Foundation

/// The engines Hex dictates with.
nonisolated enum SpeechEngine: Equatable, Sendable {
    /// Apple Speech (`SpeechAnalyzer`, iOS 26+): `SpeechTranscriber`, else `DictationTranscriber`.
    case appleSpeech
    /// FluidAudio Parakeet V2: English only, loaded into Hex.
    case parakeet
    /// Apple's `SFSpeechRecognizer` on the iPhone only (`OnDeviceSpeechTranscriber`), before
    /// iOS 26. Its model runs in Apple's speech service, so it needs no room in Hex.
    case onDeviceRecognizer
}

/// What picks the engine: the iPhone's iOS and memory, and how Parakeet's loads went.
///
/// From iOS 26, Apple Speech takes every language it has, and English stays on Parakeet only
/// where Apple has no `SpeechTranscriber` and Parakeet has room (`appleSpeechPicks`), as
/// before. Never the recognizer there, so iOS 26 never asks for Speech Recognition.
///
/// Before iOS 26 Parakeet dictates English where it has room; everything else goes to the
/// recognizer: every other language (Parakeet hears Chinese as English words), and English on
/// an iPhone with 3 GB (iPhone XR: iOS ends Hex while Parakeet loads, TestFlight build 56), after
/// a Parakeet load that ended with Hex (`ParakeetLoadGuard`), or while less memory is left
/// than loading Parakeet takes (`ParakeetTranscriber.loadMemory`).
nonisolated struct SpeechEngineChoice: Equatable, Sendable {
    /// iOS's major version.
    var system: Int
    /// `ProcessInfo.physicalMemory`.
    var physicalMemory: UInt64
    /// `os_proc_available_memory()`: 0 where iOS sets no limit (the simulator).
    var availableMemory: UInt64
    /// A Parakeet load ended with Hex, or found too little memory (`ParakeetLoadGuard`).
    var parakeetSetAside: Bool

    /// 3 GB iPhones (iPhone XR, SE 2nd generation) report about 2.8 GB, 4 GB ones about 3.7 GB.
    static let smallMemory: UInt64 = 3 << 30

    static var current: SpeechEngineChoice {
        SpeechEngineChoice(
            system: ProcessInfo.processInfo.operatingSystemVersion.majorVersion,
            physicalMemory: ProcessInfo.processInfo.physicalMemory,
            availableMemory: UInt64(max(0, os_proc_available_memory())),
            parakeetSetAside: ParakeetLoadGuard().lastLoadWasInterrupted
        )
    }

    /// Whether Parakeet can load here now, before iOS 26.
    var parakeetHasRoom: Bool {
        physicalMemory > Self.smallMemory
            && !parakeetSetAside
            && ParakeetTranscriber.hasRoomToLoad(available: availableMemory)
    }

    /// From iOS 26, `AppleSpeechTranscriber.shouldUse`'s `parakeetFits`: 6 GB and up, as before,
    /// and never after a load that ended Hex or with too little left.
    var parakeetFitsBesideAppleSpeech: Bool {
        physicalMemory >= 5 << 30
            && !parakeetSetAside
            && ParakeetTranscriber.hasRoomToLoad(available: availableMemory)
    }

    /// `language` is the dictation language (BCP 47), nil when unknown (English).
    /// `appleSpeechPicks` is iOS 26's answer (`DictationController.appleSpeechLanguage`).
    func engine(for language: String?, appleSpeechPicks: Bool) -> SpeechEngine {
        if system >= 26 {
            return appleSpeechPicks ? .appleSpeech : .parakeet
        }
        guard SpeechLanguagePicker.isEnglish(language), parakeetHasRoom else { return .onDeviceRecognizer }
        return .parakeet
    }

    /// Why Hex moved English off Parakeet before iOS 26, when it did because of Parakeet
    /// itself rather than the iPhone's size, for home's notice.
    func notice(for language: String?) -> String? {
        guard system < 26, SpeechLanguagePicker.isEnglish(language), physicalMemory > Self.smallMemory,
              !parakeetHasRoom else { return nil }
        return String(localized: "Parakeet needs more memory than Hex has here. Hex uses Apple's on-device speech instead.")
    }
}
