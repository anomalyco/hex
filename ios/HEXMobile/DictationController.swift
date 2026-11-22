import AVFAudio
import Foundation
import Observation
import SwiftUI
import UIKit

@MainActor
@Observable
final class DictationController {
    static let shared = DictationController(
        autoPrepareOnInit: ProcessInfo.processInfo.environment["XCTestConfigurationFilePath"] == nil,
        cleaner: AppleIntelligenceCleaner()
    )

    private static let minCaptureDuration: TimeInterval = 0.30
    private static let maxCaptureDuration: TimeInterval = 60.0
    private static let resultDeliveryTTL: TimeInterval = 30.0

    enum Phase: Equatable {
        case modelRequired
        case preparingModel
        case ready
        case requestingPermission
        case recording
        case transcribing
        case failed(String)

        var isBusy: Bool {
            switch self {
            case .preparingModel, .requestingPermission, .transcribing:
                true
            default:
                false
            }
        }

        /// Recording and transcribing keep the screen on, so the iPhone doesn't dim and lock
        /// while you talk or wait for the text.
        var keepsScreenAwake: Bool {
            self == .recording || self == .transcribing
        }
    }

    private struct ActiveCaptureSession {
        let generation: UInt64
        let capture: any ActiveAudioCapture
        let startedAt: TimeInterval
        let keyboardJobID: String?
        let style: DictationStyle
        let usesCleanup: Bool
        /// Settings › Dictation › Lowercase, read at start.
        let lowercase: Bool
        /// Settings › Dictation › Format lists, read at start.
        let formatLists: Bool
        /// Settings › Dictation › Spoken punctuation, read at start.
        let spokenPunctuation: Bool
        /// Settings › Dictation › Numbers as digits, read at start.
        let numbersAsDigits: Bool
        /// Settings › Dictation › End with a period, read at start.
        let endsWithPeriod: Bool
        /// Experiments › Hinglish, read at start.
        let hinglish: Bool
        /// Experiments › Greeklish, read at start.
        let greeklish: Bool
        let vocabularyTerms: [String]
        /// Learn how I type's dictation fixes, read at start; empty while it is off.
        let personalCorrections: [DictationCorrection]
        /// Custom vocabulary plus the learned fixes' meant words, for Apple Speech.
        let recognitionHints: [String]
        /// Whether the keyboard gets the raw transcript to learn from.
        let sharesHeard: Bool
        let copyToPasteboard: Bool
        /// The transcriber's language (BCP 47) when the capture started.
        let language: String
    }

    struct PendingKeyboardHandoff {
        let jobID: String?
        let style: DictationStyle
        let requestedAt: TimeInterval
    }

    var transcriber: any LocalTranscribing
    let transcriberFactory: () async -> any LocalTranscribing
    /// Experiments › Multilingual model (Parakeet): its download, beside the engine Hex
    /// dictates with until it's ready.
    @ObservationIgnored let multilingualDownload: ParakeetUltraDownload
    private let cleaner: (any TranscriptCleaning)?
    private let cleanupSettings: AppleIntelligenceCleanupSettings
    private let customVocabularySettings: CustomVocabularySettings
    private let lowercaseSettings: DictationLowercaseSettings
    private let listSettings: DictationListSettings
    private let punctuationSettings: DictationPunctuationSettings
    private let numberSettings: DictationNumberSettings
    private let periodSettings: DictationPeriodSettings
    private let hinglishSettings: HinglishSettings
    private let greeklishSettings: GreeklishSettings
    private let typingJournalSettings: TypingJournalSettings
    private let learnedDictationCorrections: @MainActor (String?) -> [DictationCorrection]
    private let audio: any AudioManaging
    let keyboardBridge: KeyboardBridge
    private let fileManager: FileManager
    private let now: () -> TimeInterval
    private let liveActivity = LiveActivityCoordinator()

    private var jobGeneration: UInt64 = 0
    var modelPrepGeneration: UInt64 = 0
    /// The preparation now running, cancelled when a newer one replaces it.
    @ObservationIgnored var preparationTask: Task<Void, Never>?
    private var activeCapture: ActiveCaptureSession?
    private var meterTask: Task<Void, Never>?
    private var transcriptionTask: Task<Void, Never>?
    private var keyboardSessionTask: Task<Void, Never>?

    private var lastKeyboardCommandID: String?
    private var lastHandledLaunchJobID: String?
    private var activeKeyboardJobID: String?
    private var keyboardResultID: String?
    private var keyboardResultExpiresAt: TimeInterval?
    private var keyboardResult: String?
    private var keyboardResultHeard: String?
    /// Why the result went in as spoken while Translate what I say was on, for the keyboard's note.
    private var keyboardResultTranslationFailure: TranslationFailure?
    /// Record typing logs: each step of the result, for the keyboard's log; nil while off.
    private var keyboardResultStages: DictationStages?
    private var keyboardMessage: String? {
        didSet { keyboardMessageAt = keyboardMessage == nil ? nil : now() }
    }
    var pendingHandoff: PendingKeyboardHandoff?
    var keyboardSessionExpiresAt: TimeInterval = 0
    private var suspendedWarmSessionOnInterruption = false
    /// The warm session loop's restarts of a keepalive that stopped by itself. Each one
    /// activates the session and reopens the microphone under every app recording at the
    /// time (a screen recording too), so one that keeps stopping soon after its restart waits
    /// twice as long each time (1 s, 2 s, … 30 s), not 100 ms; one that ran a while restarts
    /// at once.
    @ObservationIgnored private var keepaliveRestartedAt = -TimeInterval.infinity
    @ObservationIgnored private var keepaliveRestartDelay: TimeInterval = 0
    @ObservationIgnored private var keepaliveRestartDue: TimeInterval?
    private static let steadyKeepalive: TimeInterval = 10
    private static let maxKeepaliveRestartDelay: TimeInterval = 30

    var phase = Phase.modelRequired {
        didSet {
            captureFailed = false
            if case .failed = phase { failedAt = now() }
            guard phase.keepsScreenAwake != oldValue.keepsScreenAwake else { return }
            // Only applies while Hex is in front; nothing else in Hex sets it, so ending a
            // dictation hands the screen back to Auto-Lock.
            UIApplication.shared.isIdleTimerDisabled = phase.keepsScreenAwake
        }
    }
    var isModelReady = false
    var transcript = ""
    @ObservationIgnored var audioLevel: Float = 0
    @ObservationIgnored var loudnessDB: Float = -120
    @ObservationIgnored private var rawRecordingDuration: TimeInterval = 0
    private(set) var recordingWholeSeconds: Int = 0
    var recordingDuration: TimeInterval {
        get { rawRecordingDuration }
        set {
            rawRecordingDuration = newValue
            let whole = max(0, Int(newValue))
            if recordingWholeSeconds != whole {
                recordingWholeSeconds = whole
            }
        }
    }
    var copied = false
    var keyboardSessionEnabled = false
    @ObservationIgnored private var rawKeyboardSessionRemaining: TimeInterval = 0
    private(set) var keyboardSessionDisplayRemaining = "0:00"
    var keyboardSessionRemaining: TimeInterval {
        get { rawKeyboardSessionRemaining }
        set {
            rawKeyboardSessionRemaining = newValue
            let formatted = HexTheme.formatRemaining(newValue)
            if keyboardSessionDisplayRemaining != formatted {
                keyboardSessionDisplayRemaining = formatted
            }
        }
    }
    @ObservationIgnored private var lastPublishedSnapshotSignature: String?
    @ObservationIgnored private var lastPublishedSnapshotAt: TimeInterval = -100
    /// When the phase last became failed, and when `keyboardMessage` was last set: the
    /// keyboard shows a message only for a few seconds after (`KeyboardSnapshot.messageAt`).
    @ObservationIgnored private var failedAt: TimeInterval?
    @ObservationIgnored private var keyboardMessageAt: TimeInterval?
    /// The failure on screen is a microphone that didn't start for a keyboard's dictation, with
    /// the model still loaded: the keyboard's next Hex key tries again.
    @ObservationIgnored private var captureFailed = false
    var warmDuration: WarmSessionDuration
    private(set) var style: DictationStyle
    private(set) var microphone: MicrophoneChoice
    private(set) var handoffNotice: String?
    var engineName = "Local speech"
    var transcriptionLanguage = "English"
    /// The BCP 47 identifier of `transcriptionLanguage`, for the steps that depend on it.
    var transcriptionLanguageID = "en-US"
    /// Settings › Dictation language: a BCP 47 identifier, or nil for Automatic.
    private(set) var dictationLanguage: String?
    /// The keyboard's language Automatic followed when the model was last asked for
    /// (`LanguageSettings.keyboardDictationLanguage`), so a switch on the space bar prepares
    /// the new one.
    private var followedKeyboardLanguage: String?
    /// A language change made while busy, applied once the dictation in flight is done.
    var pendingLanguagePreparation = false
    /// Said once Automatic moved on from a language this iPhone can't dictate yet
    /// ("Russian isn't available on this iPhone yet. Hex switched to English.").
    var languageNotice: String?
    /// The language to offer (BCP 47) when the one chosen can't be dictated here yet.
    var languageOffer: String?
    private let languageSettings: LanguageSettings
    var modelPreparation = ModelPreparationProgress(
        fractionCompleted: 0,
        detail: String(localized: "Checking the speech model")
    )
    /// Whether Hex is in the background, where iOS suspends it and a download's connection
    /// drops.
    var isInBackground = false
    /// A model download or preparation step that failed while Hex was in the background, tried
    /// again on return.
    var preparationFailedInBackground = false
    /// `watchPreparation`: when the preparation last said something new, whether it has said
    /// anything yet, and how long each may last.
    @ObservationIgnored var preparationChangedAt = ContinuousClock.now
    @ObservationIgnored var preparationReported = false
    @ObservationIgnored var preparationLimits = (firstReport: Duration.seconds(30), stall: Duration.seconds(120))
    /// Whether the preparation now running picked Parakeet Ultra, for its failure path.
    @ObservationIgnored var preparingMultilingualParakeet = false

    init(
        audio: (any AudioManaging)? = nil,
        keyboardBridge: KeyboardBridge? = nil,
        transcriber: (any LocalTranscribing)? = nil,
        transcriberFactory: (() async -> any LocalTranscribing)? = nil,
        fileManager: FileManager = .default,
        now: @escaping () -> TimeInterval = { Date().timeIntervalSince1970 },
        autoPrepareOnInit: Bool = true,
        cleaner: (any TranscriptCleaning)? = nil,
        cleanupSettings: AppleIntelligenceCleanupSettings = AppleIntelligenceCleanupSettings(),
        customVocabularySettings: CustomVocabularySettings = CustomVocabularySettings(),
        lowercaseSettings: DictationLowercaseSettings = DictationLowercaseSettings(),
        listSettings: DictationListSettings = DictationListSettings(),
        punctuationSettings: DictationPunctuationSettings = DictationPunctuationSettings(),
        numberSettings: DictationNumberSettings = DictationNumberSettings(),
        periodSettings: DictationPeriodSettings = DictationPeriodSettings(),
        hinglishSettings: HinglishSettings = HinglishSettings(),
        greeklishSettings: GreeklishSettings = GreeklishSettings(),
        typingJournalSettings: TypingJournalSettings = TypingJournalSettings(),
        learnedDictationCorrections: (@MainActor (String?) -> [DictationCorrection])? = nil,
        languageSettings: LanguageSettings = LanguageSettings(),
        multilingualDownload: ParakeetUltraDownload? = nil
    ) {
        // Before anything reads them: Parakeet's files live in the App Group now.
        SpeechModelStore.moveLegacyDownloads()
        let resolvedBridge = keyboardBridge ?? KeyboardBridge()
        let resolvedAudio = audio ?? SystemAudioManager()
        let initialTranscriber = transcriber ?? ParakeetTranscriber()

        self.audio = resolvedAudio
        self.keyboardBridge = resolvedBridge
        self.transcriber = initialTranscriber
        self.transcriberFactory = transcriberFactory ?? Self.defaultTranscriber
        self.multilingualDownload = multilingualDownload ?? ParakeetUltraDownload()
        self.cleaner = cleaner
        self.cleanupSettings = cleanupSettings
        self.customVocabularySettings = customVocabularySettings
        self.lowercaseSettings = lowercaseSettings
        self.listSettings = listSettings
        self.punctuationSettings = punctuationSettings
        self.numberSettings = numberSettings
        self.periodSettings = periodSettings
        self.hinglishSettings = hinglishSettings
        self.greeklishSettings = greeklishSettings
        self.typingJournalSettings = typingJournalSettings
        self.learnedDictationCorrections = learnedDictationCorrections ?? { language in
            TypingJournal.storedDictationCorrections(settings: typingJournalSettings, language: language)
        }
        self.fileManager = fileManager
        self.now = now
        self.languageSettings = languageSettings
        self.dictationLanguage = languageSettings.dictationLanguage
        self.followedKeyboardLanguage = languageSettings.keyboardDictationLanguage()
        resolvedBridge.reviewWarmDurationForKeyboardDictation()
        self.warmDuration = resolvedBridge.warmDuration
        self.style = resolvedBridge.style
        self.microphone = resolvedBridge.microphone

        let hasCachedModel = ParakeetTranscriber.hasCachedModel
        resolvedBridge.publish(.offline)
        resolvedBridge.setModelInstalled(hasCachedModel)

        wireAudioCallbacks()
        wireSystemIntentBus()
        self.multilingualDownload.onDownloaded = { [weak self] in
            self?.syncMultilingualParakeet()
        }
        if autoPrepareOnInit {
            // At launch no dictation is running, so any Hex Live Activity is left over.
            liveActivity.endImmediately()
        }

        if Self.applyLaunchFixtureIfPresent(to: self) {
            return
        }

        if autoPrepareOnInit {
            prepareAtLaunch(hasCachedModel: hasCachedModel)
        }
    }

    var isRecording: Bool {
        phase == .recording
    }

    var primaryTitle: String {
        switch phase {
        case .modelRequired, .failed:
            "Prepare \(engineName)"
        case .preparingModel:
            "Preparing \(engineName)"
        case .ready:
            "Start dictation"
        case .requestingPermission:
            "Checking microphone"
        case .recording:
            "Finish"
        case .transcribing:
            "Transcribing…"
        }
    }

    // MARK: - Settings

    func setWarmDuration(_ duration: WarmSessionDuration) {
        warmDuration = duration
        keyboardBridge.warmDuration = duration

        if keyboardSessionEnabled {
            if let seconds = duration.seconds {
                keyboardSessionExpiresAt = now() + seconds
                keyboardSessionRemaining = seconds
                publishKeyboardSnapshot()
            } else if phase == .ready {
                endWarmSession()
            }
        }
    }

    /// Used by the settings segmented control: picking a duration warms immediately; picking Off ends it.
    func selectWarmDuration(_ duration: WarmSessionDuration) {
        setWarmDuration(duration)
        if duration == .off {
            if keyboardSessionEnabled {
                endWarmSession()
            } else {
                endStaleLiveActivities()
            }
        } else if !keyboardSessionEnabled {
            requestWarmSession()
        }
    }

    func setStyle(_ newStyle: DictationStyle) {
        style = newStyle
        keyboardBridge.style = newStyle
    }

    /// Settings › Dictation language. Prepares the new language's model now, or once the
    /// dictation in flight is done.
    func setDictationLanguage(_ identifier: String?) {
        let normalized = (identifier?.isEmpty == true) ? nil : identifier
        guard normalized != dictationLanguage else { return }
        languageSettings.chooseDictationLanguage(normalized)
        applyDictationLanguage(normalized)
    }

    /// Picks up a language the keyboard switched to in the App Group: a pick in its language
    /// switch, or, on Automatic, the keyboard's own language switched on the space bar.
    func followDictationLanguageSetting() {
        let stored = languageSettings.dictationLanguage
        let following = languageSettings.keyboardDictationLanguage()
        guard stored != dictationLanguage || following != followedKeyboardLanguage else { return }
        applyDictationLanguage(stored)
    }

    /// Takes the language offered after the chosen one couldn't be dictated here.
    func useOfferedLanguage() {
        guard let languageOffer else { return }
        setDictationLanguage(languageOffer)
    }

    private func applyDictationLanguage(_ identifier: String?) {
        dictationLanguage = identifier
        followedKeyboardLanguage = languageSettings.keyboardDictationLanguage()
        languageNotice = nil
        languageOffer = nil
        switch phase {
        case .recording, .transcribing, .requestingPermission:
            pendingLanguagePreparation = true
        case .modelRequired, .preparingModel, .ready, .failed:
            prepareModel()
        }
    }

    /// Runs a language change that waited for the dictation in flight.
    private func prepareDeferredLanguageIfNeeded() {
        guard pendingLanguagePreparation else { return }
        switch phase {
        case .ready, .failed, .modelRequired:
            pendingLanguagePreparation = false
            prepareModel()
        case .recording, .transcribing, .requestingPermission, .preparingModel:
            break
        }
    }

    /// The next dictation records from the new choice's input; the warm keepalive stays on
    /// the iPhone's microphone whatever the choice.
    func setMicrophone(_ choice: MicrophoneChoice) {
        guard microphone != choice else { return }
        microphone = choice
        keyboardBridge.microphone = choice
    }

    // MARK: - Standalone & System Actions

    /// Home's button sets the model up (Set up) rather than dictating: no model yet, or a
    /// failure that left none ready. A microphone or transcription failure with the model
    /// still ready dictates again, as home's Ready says it can.
    var primaryActionPreparesModel: Bool {
        switch phase {
        case .modelRequired: true
        case .failed: !isModelReady
        default: false
        }
    }

    func performPrimaryAction() {
        if primaryActionPreparesModel {
            prepareModel()
            return
        }
        switch phase {
        case .ready, .failed:
            startRecording(copyToPasteboard: false)
        case .recording:
            finishRecording()
        case .modelRequired, .preparingModel, .requestingPermission, .transcribing:
            break
        }
    }

    func cancelCurrentJob() {
        jobGeneration &+= 1
        transcriptionTask?.cancel()
        transcriptionTask = nil
        liveActivity.endImmediately()

        if let session = activeCapture {
            activeCapture = nil
            stopMetering()
            session.capture.stop()
            try? fileManager.removeItem(at: session.capture.url)
        }

        if activeKeyboardJobID != nil {
            keyboardMessage = nil
        }
        activeKeyboardJobID = nil
        audioLevel = 0
        loudnessDB = -120
        recordingDuration = 0
        if phase == .recording || phase == .transcribing || phase == .requestingPermission {
            phase = .ready
        }
        restoreAudioAfterCapture()
        publishKeyboardSnapshot()
        prepareDeferredLanguageIfNeeded()
    }

    func clearTranscript() {
        transcript = ""
        copied = false
    }

    func copyTranscript() {
        guard !transcript.isEmpty else { return }
        UIPasteboard.general.string = transcript
        KeyboardClipboardTracker().markOwnedCopy()
        copied = true
    }

    func toggleWarmSession() {
        if keyboardSessionEnabled {
            endWarmSession()
        } else {
            if warmDuration == .off {
                setWarmDuration(.fifteenMinutes)
            }
            requestWarmSession()
        }
    }

    func endWarmSession(message: String? = nil) {
        keyboardSessionTask?.cancel()
        keyboardSessionTask = nil
        pendingHandoff = nil
        suspendedWarmSessionOnInterruption = false

        // Cancel an unaccepted in-progress recording, but preserve any already-accepted transcription!
        if let session = activeCapture {
            jobGeneration &+= 1
            activeCapture = nil
            stopMetering()
            session.capture.stop()
            try? fileManager.removeItem(at: session.capture.url)
            activeKeyboardJobID = nil
            audioLevel = 0
            loudnessDB = -120
            recordingDuration = 0
            liveActivity.endImmediately()
            if phase == .recording {
                phase = .ready
            }
        }

        keyboardSessionEnabled = false
        keyboardSessionRemaining = 0
        keyboardSessionExpiresAt = 0
        keyboardMessage = message

        if phase != .recording {
            audio.deactivateSession()
        }
        endStaleLiveActivities()

        publishKeyboardSnapshot()
        // An accepted transcription the keyboard waits on keeps its heartbeat.
        if keyboardJobInFlight {
            startKeyboardSessionLoop()
        }
    }

    /// Ends Live Activities no dictation owns, such as one a suspended or killed process left.
    func endStaleLiveActivities() {
        guard activeCapture == nil, phase != .recording, phase != .transcribing else { return }
        liveActivity.endImmediately()
    }

    // MARK: - Keyboard Launch

    func handleKeyboardLaunch(_ request: KeyboardLaunchRequest) {
        if request.opensPage {
            return
        }
        // Whatever made the keyboard open Hex, it isn't dictating by itself: the warm default is back.
        keyboardBridge.noteKeyboardOpenedHex()
        refreshWarmDuration()
        // A language picked in the keyboard: prepare it before the handoff records.
        followDictationLanguageSetting()
        handoffNotice = nil

        let currentTime = now()
        if request.isHandoffExpired(at: currentTime) {
            handoffNotice = String(localized: "Timed out")
            pendingHandoff = PendingKeyboardHandoff(
                jobID: nil,
                style: style,
                requestedAt: currentTime
            )
            fulfillPendingHandoffOrPrepare()
            return
        }

        switch request {
        case .setup, .translationSettings, .pasteSettings:
            break

        case .startSession:
            pendingHandoff = PendingKeyboardHandoff(
                jobID: nil,
                style: style,
                requestedAt: currentTime
            )
            fulfillPendingHandoffOrPrepare()

        case .startRecording(let jobID, _, let requestedStyle):
            guard !jobID.isEmpty, jobID != lastHandledLaunchJobID else { return }
            lastHandledLaunchJobID = jobID
            if let requestedStyle {
                setStyle(requestedStyle)
            }
            pendingHandoff = PendingKeyboardHandoff(
                jobID: jobID,
                style: requestedStyle ?? style,
                requestedAt: currentTime
            )
            fulfillPendingHandoffOrPrepare()
        }
    }

    private func fulfillPendingHandoffOrPrepare() {
        guard let handoff = pendingHandoff else { return }

        if now() - handoff.requestedAt > KeyboardLaunchRequest.handoffWindow {
            handoffNotice = String(localized: "Timed out")
            pendingHandoff = PendingKeyboardHandoff(
                jobID: nil,
                style: handoff.style,
                requestedAt: now()
            )
        }

        switch phase {
        case .modelRequired, .failed:
            prepareModel()
        case .ready:
            activateWarmSessionAndRunHandoff()
        case .preparingModel, .requestingPermission, .recording, .transcribing:
            break
        }
    }

    private func requestWarmSession() {
        pendingHandoff = PendingKeyboardHandoff(
            jobID: nil,
            style: style,
            requestedAt: now()
        )
        fulfillPendingHandoffOrPrepare()
    }

    func activateWarmSessionAndRunHandoff() {
        guard phase == .ready else { return }
        let handoff = pendingHandoff
        pendingHandoff = nil

        // If a one-shot recording was requested (`warmDuration == .off` and a jobID is present),
        // go straight into recording and release the mic as soon as capture finishes.
        if warmDuration == .off, let jobID = handoff?.jobID {
            let expired = handoff.map { now() - $0.requestedAt > KeyboardLaunchRequest.handoffWindow } ?? false
            if !expired {
                startRecording(
                    keyboardJobID: jobID,
                    styleOverride: handoff?.style,
                    copyToPasteboard: false
                )
                // The keyboard finishes what it handed off: without the loop its Done and
                // Cancel went unread and the recording ran to its 60 s limit.
                startKeyboardSessionLoop()
            } else {
                handoffNotice = String(localized: "Timed out")
            }
            return
        }

        if keyboardSessionEnabled, audio.isKeepaliveRunning {
            refreshWarmExpiry()
            if let jobID = handoff?.jobID {
                let expired = handoff.map { now() - $0.requestedAt > KeyboardLaunchRequest.handoffWindow } ?? false
                if !expired {
                    startRecording(
                        keyboardJobID: jobID,
                        styleOverride: handoff?.style,
                        copyToPasteboard: false
                    )
                    return
                } else {
                    handoffNotice = String(localized: "Timed out")
                }
            }
            return
        }

        jobGeneration &+= 1
        let requestGen = jobGeneration
        phase = .requestingPermission
        keyboardMessage = nil

        Task {
            let granted = await audio.requestPermission()
            guard self.jobGeneration == requestGen else { return }
            guard granted else {
                phase = .failed(String(localized: "Allow microphone access in Settings to dictate."))
                publishKeyboardSnapshot()
                return
            }

            // If we also have a fresh recording job, begin capture directly so we don't bounce keepalive -> recorder!
            if let jobID = handoff?.jobID,
               let requestedAt = handoff?.requestedAt,
               self.now() - requestedAt <= KeyboardLaunchRequest.handoffWindow {
                self.enableWarmSessionBookkeeping()
                self.phase = .ready
                self.startKeyboardSessionLoop()
                self.startRecording(
                    keyboardJobID: jobID,
                    styleOverride: handoff?.style,
                    copyToPasteboard: false
                )
                return
            }

            if let jobID = handoff?.jobID, !jobID.isEmpty {
                self.handoffNotice = String(localized: "Timed out")
            }

            do {
                try audio.startKeepalive()
                enableWarmSessionBookkeeping()
                phase = .ready
                startKeyboardSessionLoop()
                publishKeyboardSnapshot()
            } catch {
                audio.deactivateSession()
                phase = .failed(UserFacingError.message(for: error, context: .microphone))
                publishKeyboardSnapshot()
            }
        }
    }

    private func enableWarmSessionBookkeeping() {
        let seconds = warmDuration.seconds ?? (15 * 60)
        keyboardSessionEnabled = true
        keyboardSessionExpiresAt = now() + seconds
        keyboardSessionRemaining = seconds
    }

    private func refreshWarmExpiry() {
        guard keyboardSessionEnabled, let seconds = warmDuration.seconds else { return }
        keyboardSessionExpiresAt = now() + seconds
        keyboardSessionRemaining = seconds
    }

    // MARK: - Capture & Generation-Scoped Transcription

    func startRecording(
        keyboardJobID: String? = nil,
        styleOverride: DictationStyle? = nil,
        copyToPasteboard: Bool = false
    ) {
        guard activeCapture == nil else { return }
        if let keyboardJobID, keyboardJobID == activeKeyboardJobID {
            return
        }
        activeKeyboardJobID = keyboardJobID
        if keyboardJobID != nil {
            keyboardMessage = nil
        }

        // Bumping generation invalidates any stale winding-down transcription task so it can never
        // overwrite our new capture's phase, job state, or temporary WAV file.
        jobGeneration &+= 1
        let captureGeneration = jobGeneration
        transcriptionTask?.cancel()
        transcriptionTask = nil
        let effectiveStyle = styleOverride ?? style
        let usesCleanup = cleanupSettings.isEnabled
        let lowercase = lowercaseSettings.isEnabled
        let formatLists = listSettings.isEnabled
        let spokenPunctuation = punctuationSettings.isEnabled
        let numbersAsDigits = numberSettings.isEnabled
        let endsWithPeriod = periodSettings.isEnabled
        let hinglish = hinglishSettings.isEnabled
        let greeklish = greeklishSettings.isEnabled
        let language = transcriptionLanguageID
        CleanupTagger.prepareIfEnabled(style: effectiveStyle, language: language)
        let vocabularyTerms = customVocabularySettings.activeTerms
        let personalCorrections = learnedDictationCorrections(language)
        let recognitionHints = DictationCorrections.recognitionHints(
            vocabulary: vocabularyTerms,
            corrections: personalCorrections
        )
        let sharesHeard = typingJournalSettings.isEnabled

        phase = .requestingPermission
        copied = false

        Task {
            let granted = await audio.requestPermission()
            guard self.jobGeneration == captureGeneration else { return }
            guard granted else {
                activeKeyboardJobID = nil
                let message = String(localized: "Allow microphone access in Settings.")
                keyboardMessage = keyboardJobID == nil ? nil : message
                phase = .failed(message)
                publishKeyboardSnapshot()
                return
            }

            let captureURL = fileManager.temporaryDirectory
                .appendingPathComponent("hex-dictation-\(captureGeneration)-\(UUID().uuidString)")
                .appendingPathExtension("wav")

            do {
                let capture = try audio.beginCapture(
                    at: captureURL,
                    maxDuration: Self.maxCaptureDuration,
                    microphone: microphone
                )
                guard self.jobGeneration == captureGeneration else {
                    capture.stop()
                    try? fileManager.removeItem(at: captureURL)
                    return
                }

                let startedAt = now()
                activeCapture = ActiveCaptureSession(
                    generation: captureGeneration,
                    capture: capture,
                    startedAt: startedAt,
                    keyboardJobID: keyboardJobID,
                    style: effectiveStyle,
                    usesCleanup: usesCleanup,
                    lowercase: lowercase,
                    formatLists: formatLists,
                    spokenPunctuation: spokenPunctuation,
                    numbersAsDigits: numbersAsDigits,
                    endsWithPeriod: endsWithPeriod,
                    hinglish: hinglish,
                    greeklish: greeklish,
                    vocabularyTerms: vocabularyTerms,
                    personalCorrections: personalCorrections,
                    recognitionHints: recognitionHints,
                    sharesHeard: sharesHeard,
                    copyToPasteboard: copyToPasteboard,
                    language: language
                )
                refreshWarmExpiry()
                audioLevel = 0
                loudnessDB = -120
                recordingDuration = 0
                phase = .recording
                // Only the Action Button, Control Center, and Siri (the dictations that copy)
                // get a Live Activity: the keyboard and the app show their own recording UI.
                if copyToPasteboard {
                    liveActivity.startRecording(startedAt: Date(timeIntervalSince1970: startedAt))
                }
                startMetering(for: captureGeneration)
                publishKeyboardSnapshot()
            } catch {
                try? fileManager.removeItem(at: captureURL)
                activeKeyboardJobID = nil
                let message = UserFacingError.message(for: error, context: .microphone)
                keyboardMessage = keyboardJobID == nil ? nil : message
                phase = .failed(message)
                captureFailed = isModelReady
                restoreAudioAfterCapture()
                publishKeyboardSnapshot()
            }
        }
    }

    func finishRecording() {
        guard let session = activeCapture else { return }
        activeCapture = nil

        stopMetering()
        let finalDuration = max(recordingDuration, session.capture.currentTime)
        session.capture.stop()
        audioLevel = 0
        loudnessDB = -120

        let captureURL = session.capture.url
        let captureGeneration = session.generation
        let keyboardJobID = session.keyboardJobID

        guard finalDuration >= Self.minCaptureDuration else {
            try? fileManager.removeItem(at: captureURL)
            recordingDuration = 0
            activeKeyboardJobID = nil
            keyboardMessage = nil
            phase = .ready
            liveActivity.endImmediately()
            restoreAudioAfterCapture()
            publishKeyboardSnapshot()
            prepareDeferredLanguageIfNeeded()
            return
        }

        phase = .transcribing
        liveActivity.markTranscribing(startedAt: Date(timeIntervalSince1970: session.startedAt))
        restoreAudioAfterCapture()
        publishKeyboardSnapshot()

        let activeTranscriber = transcriber
        // A keyboard's dictation finishes with Hex in the background, and in One shot nothing
        // keeps Hex running once the microphone closes: iOS would suspend it mid-transcription
        // and the text would never reach the keyboard.
        let backgroundTime = keyboardJobID == nil ? nil : BackgroundTime("Keyboard dictation")

        transcriptionTask?.cancel()
        transcriptionTask = Task { [weak self] in
            defer {
                try? FileManager.default.removeItem(at: captureURL)
                backgroundTime?.end()
            }
            let transcribeStarted = Date()

            do {
                let rawText = try await activeTranscriber.transcribe(
                    captureURL,
                    vocabulary: session.recognitionHints
                )
                guard let self, self.jobGeneration == captureGeneration, !Task.isCancelled else {
                    return
                }
                // The steps in-keyboard dictation runs too, plus Apple Intelligence's cleanup.
                let pipeline = DictationPipeline(
                    style: session.style,
                    language: session.language,
                    vocabulary: session.vocabularyTerms,
                    corrections: session.personalCorrections,
                    lowercase: session.lowercase,
                    formatLists: session.formatLists,
                    spokenPunctuation: session.spokenPunctuation,
                    numbersAsDigits: session.numbersAsDigits,
                    hinglish: session.hinglish,
                    greeklish: session.greeklish,
                    endsWithPeriod: session.endsWithPeriod
                )
                let stages = await pipeline.run(rawText) { tagged in
                    await self.cleanedTranscriptIfEnabled(tagged, session: session)
                }
                let translated = stages.translated
                guard self.jobGeneration == captureGeneration, !Task.isCancelled else {
                    return
                }
                let formatted = stages.formatted
                self.recordingDuration = 0
                self.transcript = formatted
                if session.copyToPasteboard && !formatted.isEmpty {
                    UIPasteboard.general.string = formatted
                    KeyboardClipboardTracker().markOwnedCopy()
                    self.copied = true
                    self.liveActivity.finishCopied()
                } else {
                    self.liveActivity.endImmediately()
                }

                if keyboardJobID != nil {
                    self.keyboardResultID = UUID().uuidString
                    self.keyboardResult = formatted
                    // A translation's words can't be compared with what was heard.
                    self.keyboardResultHeard = session.sharesHeard && translated == nil ? stages.heard : nil
                    self.keyboardResultTranslationFailure = stages.translationFailure
                    self.keyboardResultStages = TypingLogSettings().isEnabled
                        ? DictationStages(
                            engine: self.engineName,
                            language: self.transcriptionLanguage,
                            style: session.style.rawValue,
                            recordedMS: Int((finalDuration * 1000).rounded()),
                            transcribeMS: Int((Date().timeIntervalSince(transcribeStarted) * 1000).rounded()),
                            raw: rawText,
                            fixed: stages.fixed,
                            spelled: stages.spelled,
                            tagged: CleanupTaggerSettings().isEnabled && session.style != .verbatim ? stages.tagged : nil,
                            cleaned: session.usesCleanup && session.style != .verbatim ? stages.cleaned : nil,
                            translated: translated,
                            formatted: formatted
                        )
                        : nil
                    self.keyboardResultExpiresAt = self.now() + Self.resultDeliveryTTL
                    self.keyboardMessage = nil
                }
                self.activeKeyboardJobID = nil
                self.refreshWarmExpiry()
                self.phase = .ready
                self.publishKeyboardSnapshot()
                self.prepareDeferredLanguageIfNeeded()
            } catch {
                guard !Task.isCancelled, let self, self.jobGeneration == captureGeneration else {
                    return
                }
                self.recordingDuration = 0
                self.activeKeyboardJobID = nil
                self.liveActivity.endImmediately()
                // Nothing was said: end quietly, as a capture too short to hold a word does.
                if EmptyCapture.endsQuietly(error) {
                    self.keyboardMessage = nil
                    self.refreshWarmExpiry()
                    self.phase = .ready
                    self.publishKeyboardSnapshot()
                    return
                }
                let message = UserFacingError.message(for: error, context: .transcription)
                self.keyboardMessage = keyboardJobID == nil ? nil : message
                self.phase = .failed(message)
                self.publishKeyboardSnapshot()
                self.prepareDeferredLanguageIfNeeded()
            }
        }
    }

    /// The cleanup experiment's one pass, before formatting. Any failure keeps the raw text.
    private func cleanedTranscriptIfEnabled(
        _ rawText: String,
        session: ActiveCaptureSession
    ) async -> String {
        guard session.usesCleanup,
              session.style != .verbatim,
              !rawText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              let cleaner,
              cleaner.isAvailable,
              cleaner.supports(language: session.language) else {
            return rawText
        }
        return await cleaner.clean(rawText) ?? rawText
    }

    private func restoreAudioAfterCapture() {
        if keyboardSessionEnabled, let seconds = warmDuration.seconds, seconds > 0 {
            let remaining = keyboardSessionExpiresAt - now()
            if remaining <= 0 {
                keyboardSessionEnabled = false
                keyboardSessionRemaining = 0
                keyboardSessionExpiresAt = 0
                audio.deactivateSession()
                return
            }
            do {
                try audio.startKeepalive()
            } catch {
                keyboardSessionEnabled = false
                keyboardSessionRemaining = 0
                keyboardSessionExpiresAt = 0
                keyboardMessage = UserFacingError.message(for: error, context: .microphone)
                audio.deactivateSession()
            }
        } else {
            keyboardSessionEnabled = false
            keyboardSessionRemaining = 0
            keyboardSessionExpiresAt = 0
            audio.deactivateSession()
        }
    }

    private func startMetering(for generation: UInt64) {
        stopMetering()
        meterTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self,
                      let session = self.activeCapture,
                      session.generation == generation else {
                    break
                }

                // Recorder readiness is a sample, not the capture's lifetime. A transient
                // false must not strand the app meter and keyboard heartbeat until Stop.
                // Completion/interruption callbacks and explicit Stop own capture teardown.
                guard session.capture.isRecording else {
                    try? await Task.sleep(for: .milliseconds(33))
                    continue
                }

                let db = session.capture.averagePowerDB()
                self.loudnessDB = db
                self.audioLevel = max(0, min(1, (db + 48) / 48))
                self.recordingDuration = session.capture.currentTime
                self.publishKeyboardSnapshot(minInterval: 0.10)

                if self.recordingDuration >= Self.maxCaptureDuration - 0.25 {
                    self.finishRecording()
                    return
                }

                try? await Task.sleep(for: .milliseconds(33))
            }
        }
    }

    private func stopMetering() {
        meterTask?.cancel()
        meterTask = nil
    }

    // MARK: - Warm Session Loop & Snapshot Truth

    /// A keyboard's dictation is recording or transcribing: Hex reads its commands and
    /// heartbeats its snapshot until the text is in, warm session or not (One shot).
    private var keyboardJobInFlight: Bool {
        activeKeyboardJobID != nil
    }

    /// Reads the keyboard's commands and keeps its snapshot's heartbeat fresh, for a warm
    /// session and for any keyboard dictation in flight. The keyboard takes a job whose
    /// heartbeat stops as over (`KeyboardSnapshot.isOrphanedJob`).
    private func startKeyboardSessionLoop() {
        keyboardSessionTask?.cancel()
        keepaliveRestartedAt = -.infinity
        keepaliveRestartDelay = 0
        keepaliveRestartDue = nil
        keyboardSessionTask = Task { [weak self] in
            while !Task.isCancelled {
                guard let self, self.keyboardSessionEnabled || self.keyboardJobInFlight else { return }

                // Never expire an in-flight recording or accepted transcription mid-work.
                if self.keyboardSessionEnabled, self.phase != .recording && self.phase != .transcribing {
                    // The keyboard's language switch writes the App Group; follow it.
                    self.followDictationLanguageSetting()
                    let remaining = self.keyboardSessionExpiresAt - self.now()
                    if remaining <= 0 {
                        self.endWarmSession()
                        return
                    }
                    self.keyboardSessionRemaining = remaining

                    // Verify the keepalive engine is genuinely alive so we never publish a stuck "Ready".
                    if self.audio.isKeepaliveRunning {
                        self.keepaliveRestartDue = nil
                    } else if self.phase == .ready && !self.suspendedWarmSessionOnInterruption,
                              self.keepaliveRestartIsDue() {
                        do {
                            try self.audio.startKeepalive()
                        } catch {
                            self.endWarmSession()
                            return
                        }
                    }
                }

                self.consumeKeyboardCommand()
                if self.phase != .recording {
                    self.publishKeyboardSnapshot(minInterval: 0.60)
                }
                try? await Task.sleep(for: .milliseconds(100))
            }
        }
    }

    private func keepaliveRestartIsDue() -> Bool {
        let time = now()
        if keepaliveRestartDue == nil {
            let stoppedSoon = time - keepaliveRestartedAt < Self.steadyKeepalive
            keepaliveRestartDelay = stoppedSoon
                ? min(max(1, keepaliveRestartDelay * 2), Self.maxKeepaliveRestartDelay)
                : 0
            keepaliveRestartDue = time + keepaliveRestartDelay
        }
        guard let due = keepaliveRestartDue, time >= due else { return false }
        keepaliveRestartDue = nil
        keepaliveRestartedAt = time
        return true
    }

    func processPendingKeyboardCommandForTesting() {
        consumeKeyboardCommand()
        publishKeyboardSnapshot()
    }

    func updateRecordingDurationForTesting(_ duration: TimeInterval) {
        recordingDuration = duration
    }

    private func consumeKeyboardCommand() {
        guard let command = keyboardBridge.latestCommand(),
              command.id != lastKeyboardCommandID else { return }
        lastKeyboardCommandID = command.id

        // Ignore stale commands left in UserDefaults from >10s ago.
        guard now() - command.sentAt <= KeyboardLaunchRequest.handoffWindow else { return }

        switch command.kind {
        case .start:
            // A warm session whose microphone failed for the last dictation stays marked failed:
            // the keyboard's next Hex key is its Try again. Without this Hex ignored every
            // start until it was opened, while the keyboard kept sending them.
            guard phase == .ready || captureFailed, activeKeyboardJobID == nil else { return }
            setStyle(command.style)
            startRecording(
                keyboardJobID: command.jobID,
                styleOverride: command.style,
                copyToPasteboard: false
            )
        case .stop:
            guard phase == .recording, activeKeyboardJobID == command.jobID else { return }
            finishRecording()
        case .cancel:
            guard (phase == .recording || phase == .transcribing),
                  activeKeyboardJobID == command.jobID else { return }
            cancelCurrentJob()
        }
    }

    func publishKeyboardSnapshot(minInterval: TimeInterval = 0) {
        let currentTime = now()

        // Past its time to go in by itself the keyboard can still offer it as Paste dictation.
        if let expires = keyboardResultExpiresAt, expires + KeyboardSnapshot.resultOfferWindow <= currentTime {
            keyboardResultID = nil
            keyboardResult = nil
            keyboardResultHeard = nil
            keyboardResultTranslationFailure = nil
            keyboardResultStages = nil
            keyboardResultExpiresAt = nil
        }

        let state: KeyboardDictationState
        switch phase {
        case .recording:
            // Hardware truth guard: only claim `.recording` while an active capture is genuinely recording.
            if let session = activeCapture, session.capture.isRecording {
                state = .recording
            } else {
                state = keyboardSessionEnabled ? .ready : .offline
            }
        case .transcribing:
            state = .transcribing
        case .failed:
            state = .failed
        case .modelRequired, .preparingModel, .requestingPermission:
            state = .offline
        case .ready:
            state = (keyboardSessionEnabled && audio.isKeepaliveRunning) ? .ready : .offline
        }

        let failureMessage: String?
        let messageAt: TimeInterval?
        if case .failed(let message) = phase {
            failureMessage = message
            messageAt = failedAt
        } else {
            failureMessage = keyboardMessage
            messageAt = keyboardMessageAt
        }

        let signature = "\(state.rawValue)|\(activeKeyboardJobID ?? "")|\(keyboardResultID ?? "")|\(failureMessage ?? "")"
        if minInterval > 0,
           signature == lastPublishedSnapshotSignature,
           currentTime - lastPublishedSnapshotAt < minInterval {
            return
        }
        lastPublishedSnapshotSignature = signature
        lastPublishedSnapshotAt = currentTime

        let effectiveHeartbeat: TimeInterval
        if state == .recording || state == .transcribing || (keyboardSessionEnabled && audio.isKeepaliveRunning) {
            effectiveHeartbeat = currentTime
        } else {
            effectiveHeartbeat = 0
        }

        keyboardBridge.publish(
            KeyboardSnapshot(
                state: state,
                heartbeat: effectiveHeartbeat,
                expiresAt: keyboardSessionEnabled ? keyboardSessionExpiresAt : 0,
                recordingStartedAt: activeCapture?.startedAt,
                audioLevel: state == .recording ? audioLevel : 0,
                loudnessDB: state == .recording ? loudnessDB : nil,
                jobID: activeKeyboardJobID,
                resultID: keyboardResultID,
                resultExpiresAt: keyboardResultExpiresAt,
                transcript: keyboardResult,
                message: failureMessage,
                heard: keyboardResultHeard,
                stages: keyboardResultStages,
                translationFailure: keyboardResultTranslationFailure,
                messageAt: failureMessage == nil ? nil : messageAt
            )
        )
    }

    // MARK: - Audio Interruptions, Route Changes, & System Intents

    private func wireAudioCallbacks() {
        audio.onUnexpectedCaptureEnd = { [weak self] finishedURL, succeeded in
            guard let self,
                  let session = self.activeCapture,
                  session.capture.url == finishedURL else {
                return
            }
            if succeeded {
                self.finishRecording()
            } else {
                self.cancelCurrentJob()
                self.phase = .failed(String(localized: "Recording stopped."))
                self.publishKeyboardSnapshot()
            }
        }

        audio.onSystemEvent = { [weak self] event in
            self?.handleAudioSystemEvent(event)
        }
    }

    func handleAudioSystemEvent(_ event: AudioSystemEvent) {
        switch event {
        case .interruptionBegan:
            suspendedWarmSessionOnInterruption = keyboardSessionEnabled
            if phase == .recording {
                // Save spoken audio if >= 300ms; otherwise cancel cleanly so "Listening" never sticks during a call.
                if let session = activeCapture, max(recordingDuration, session.capture.currentTime) >= Self.minCaptureDuration {
                    audio.stopKeepalive()
                    finishRecording()
                } else {
                    cancelCurrentJob()
                    keyboardMessage = String(localized: "Interrupted")
                }
            } else {
                audio.stopKeepalive()
            }
            publishKeyboardSnapshot()

        case .interruptionEnded(let shouldResume):
            guard suspendedWarmSessionOnInterruption else { return }
            suspendedWarmSessionOnInterruption = false
            guard keyboardSessionEnabled, keyboardSessionExpiresAt > now() else {
                endWarmSession()
                return
            }
            if shouldResume || UIApplication.shared.applicationState == .active {
                do {
                    try audio.startKeepalive()
                    publishKeyboardSnapshot()
                } catch {
                    endWarmSession()
                }
            } else {
                endWarmSession()
            }

        case .routeOldDeviceUnavailable:
            // e.g. AirPods placed in case: finish active speech >= 300ms or rebind keepalive tap to built-in mic.
            if phase == .recording {
                if let session = activeCapture, max(recordingDuration, session.capture.currentTime) >= Self.minCaptureDuration {
                    finishRecording()
                } else {
                    cancelCurrentJob()
                }
            } else if keyboardSessionEnabled && phase == .ready {
                do {
                    try audio.startKeepalive()
                    publishKeyboardSnapshot()
                } catch {
                    endWarmSession()
                }
            }

        case .routeNewDeviceAvailable:
            // A headset or car that just connected can take the input: the keepalive goes back
            // to the iPhone's microphone, on a fresh engine for the route's format.
            if keyboardSessionEnabled && phase == .ready {
                try? audio.startKeepalive()
                publishKeyboardSnapshot()
            }

        case .mediaServicesReset:
            if phase == .recording {
                cancelCurrentJob()
            }
            if keyboardSessionEnabled {
                do {
                    try audio.startKeepalive()
                    publishKeyboardSnapshot()
                } catch {
                    endWarmSession()
                }
            }
        }
    }

    private func wireSystemIntentBus() {
        Task {
            await HexSystemCommandBus.shared.register { [weak self] command in
                guard let self else { return nil }
                return await self.performSystemCommand(command)
            }
        }
    }

    private func performSystemCommand(_ command: HexSystemCommand) async -> String? {
        switch command {
        case .toggleDictation:
            if phase == .recording {
                finishRecording()
                if let task = transcriptionTask {
                    await task.value
                }
                return transcript
            }
            followDictationLanguageSetting()
            if phase == .modelRequired || isFailedPhase {
                prepareModel()
            }
            if phase == .preparingModel {
                try? await waitUntilReady(timeoutSeconds: 10)
            }
            guard phase == .ready else { return nil }
            startRecording(copyToPasteboard: true)
            return ""

        case .stopDictation:
            guard phase == .recording else { return transcript }
            finishRecording()
            if let task = transcriptionTask {
                await task.value
            }
            return transcript

        case .cancelDictation:
            cancelCurrentJob()
            return nil

        case .warmSession:
            if !keyboardSessionEnabled {
                if warmDuration == .off {
                    setWarmDuration(.fifteenMinutes)
                }
                requestWarmSession()
            }
            return nil
        }
    }

    private var isFailedPhase: Bool {
        if case .failed = phase { return true }
        return false
    }

    private func waitUntilReady(timeoutSeconds: TimeInterval) async throws {
        let deadline = now() + timeoutSeconds
        while phase == .preparingModel && now() < deadline {
            try await Task.sleep(for: .milliseconds(100))
        }
    }
}

/// Background time from iOS while Hex finishes a keyboard's dictation, ended when it's done
/// or when iOS asks for it back.
@MainActor
final class BackgroundTime {
    private var id = UIBackgroundTaskIdentifier.invalid

    init(_ name: String) {
        id = UIApplication.shared.beginBackgroundTask(withName: name) { [weak self] in
            self?.end()
        }
    }

    func end() {
        guard id != .invalid else { return }
        UIApplication.shared.endBackgroundTask(id)
        id = .invalid
    }
}
