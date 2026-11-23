import AVFAudio
import Foundation
import os
import Speech
import UIKit
import WebKit

/// Experiment: record and transcribe inside the keyboard extension, so dictation
/// needs no Hex app process. Each step is logged to the App Group container
/// (`keyboard-capture-probe.log`) to measure permission and memory limits.
@MainActor
final class KeyboardLocalDictation {
    /// The job being captured or transcribed, shared with its transcription task: whether the
    /// keyboard hid before it finished (its text is then held in the App Group for the next
    /// keyboard, `KeyboardHeldDictation`) and the field it was started in.
    private final class Job {
        let id: String
        let document: UUID?
        var preferences: DictationPipeline.Preferences
        var interrupted = false

        init(id: String, document: UUID?, preferences: DictationPipeline.Preferences) {
            self.id = id
            self.document = document
            self.preferences = preferences
        }
    }

    /// The open PCM writer and optional live preview stream created once WebKit's `.started` arrives.
    @MainActor
    private struct AudioSink {
        var pcmFile: KeyboardPCMFile
        var live: KeyboardLiveDictation?
        /// Audio since a possible send (`holdForSend`), at most `KeyboardLiveTyping.sendConfirmation` of it.
        var sendHeld: [Data]?

        mutating func append(_ data: Data) {
            if sendHeld != nil {
                sendHeld?.append(data)
            } else {
                pcmFile.append(data)
                live?.append(data)
            }
        }

        mutating func stopLive() {
            live?.stop()
            live = nil
        }

        mutating func discard() {
            stopLive()
            _ = pcmFile.close()
            try? FileManager.default.removeItem(at: pcmFile.url)
        }
    }

    /// Startup and streaming resources for one active capture. Holding `AudioSink` inside
    /// `.awaitingFirstAudio` and `.streaming` makes `.recording` without a PCM file unrepresentable
    /// and replaces `startPending`, `pcmFile`, `live`, `sendHeld`, and `timeline`.
    @MainActor
    private enum CaptureResources {
        case awaitingAudioSession(timeline: CaptureTimeline)
        case awaitingPage(timeline: CaptureTimeline)
        case awaitingFirstAudio(sink: AudioSink, timeline: CaptureTimeline)
        case streaming(sink: AudioSink, audioStartedAt: TimeInterval)

        var isStreaming: Bool {
            if case .streaming = self { return true }
            return false
        }

        mutating func discard() {
            switch self {
            case .awaitingFirstAudio(var sink, _), .streaming(var sink, _):
                sink.discard()
            case .awaitingAudioSession, .awaitingPage:
                break
            }
        }

        func sendMemoryWarning() {
            switch self {
            case .awaitingFirstAudio(let sink, _), .streaming(let sink, _):
                sink.live?.memoryWarning()
            case .awaitingAudioSession, .awaitingPage:
                break
            }
        }
    }

    @MainActor
    private struct CapturingState {
        let job: Job
        let requestedAt: TimeInterval
        var watch: KeyboardCaptureWatch
        var resources: CaptureResources
        var loudness: Float = -62
    }

    @MainActor
    private enum ArmedState {
        case capturing(CapturingState)
        /// A failure while armed, shown only if the touch is kept: a slide to the menu drops it.
        case failed(job: Job, requestedAt: TimeInterval, failure: KeyboardLocalCapturePhase.ArmedFailure)

        var job: Job {
            switch self {
            case .capturing(let state): state.job
            case .failed(let job, _, _): job
            }
        }

        var requestedAt: TimeInterval {
            switch self {
            case .capturing(let state): state.requestedAt
            case .failed(_, let requestedAt, _): requestedAt
            }
        }
    }

    @MainActor
    private enum Session {
        case idle
        case armed(ArmedState)
        case active(CapturingState)
        case transcribing(job: Job, startedAt: TimeInterval, task: Task<Void, Never>?)
        case finished(job: Job, result: KeyboardResult)
        case failed(message: String, until: TimeInterval)

        var phase: KeyboardLocalCapturePhase {
            switch self {
            case .idle:
                return .idle
            case .armed(.capturing(let state)):
                let readiness: KeyboardLocalCapturePhase.ArmedReadiness
                switch state.resources {
                case .awaitingAudioSession:
                    readiness = .starting(.awaitingAudioSession)
                case .awaitingPage:
                    readiness = .starting(.awaitingPage)
                case .awaitingFirstAudio:
                    readiness = .starting(.awaitingFirstAudio)
                case .streaming(_, let audioStartedAt):
                    readiness = .receivingAudio(startedAt: audioStartedAt)
                }
                return .armed(jobID: state.job.id, requestedAt: state.requestedAt, readiness: readiness)
            case .armed(.failed(let job, let requestedAt, let failure)):
                return .armed(jobID: job.id, requestedAt: requestedAt, readiness: .failed(failure))
            case .active(let state):
                switch state.resources {
                case .awaitingAudioSession:
                    return .starting(jobID: state.job.id, requestedAt: state.requestedAt, stage: .awaitingAudioSession)
                case .awaitingPage:
                    return .starting(jobID: state.job.id, requestedAt: state.requestedAt, stage: .awaitingPage)
                case .awaitingFirstAudio:
                    return .starting(jobID: state.job.id, requestedAt: state.requestedAt, stage: .awaitingFirstAudio)
                case .streaming(_, let audioStartedAt):
                    return .recording(jobID: state.job.id, startedAt: audioStartedAt)
                }
            case .transcribing(let job, _, _):
                return .transcribing(jobID: job.id)
            case .finished(let job, let result):
                return .finished(jobID: job.id, result: result)
            case .failed(let message, let until):
                return .failed(message: message, until: until)
            }
        }

        var job: Job? {
            switch self {
            case .armed(let armed): armed.job
            case .active(let state): state.job
            case .transcribing(let job, _, _), .finished(let job, _): job
            case .idle, .failed: nil
            }
        }

        var loudness: Float {
            switch self {
            case .armed(.capturing(let state)), .active(let state): state.loudness
            default: -62
            }
        }

        var isHoldingAudioSession: Bool {
            switch self {
            case .armed(.capturing), .active: true
            default: false
            }
        }
    }

    private var session: Session = .idle
    private var phase: KeyboardLocalCapturePhase { session.phase }

    private func setSession(_ next: Session) {
        let oldPhase = session.phase
        session = next
        let newPhase = next.phase
        if case .idle = newPhase {
            liveText?.end()
        } else if case .failed = newPhase {
            liveText?.end()
        }
        if newPhase.visibleStage != oldPhase.visibleStage {
            onPhaseChange?()
        }
    }

    /// Fired when the visible dictation stage changes (e.g. starting -> recording on first audio),
    /// so the keyboard refreshes on the same frame without waiting for the next poll tick.
    var onPhaseChange: (() -> Void)?
    /// The words so far while recording (`KeyboardLiveDictation`), for the keyboard to show.
    var liveText: LiveDictationText?
    /// One capture web view shared across presentations in this extension process: making a new
    /// `WKWebView` on every presentation and leaving it attached to the old `KeyboardInputView`
    /// kept every dismissed keyboard's view hierarchy alive through WebKit's layout and observer
    /// references. Detached from its host view whenever a keyboard hides or deallocates.
    private static var sharedCapture: KeyboardWebCapture?
    private static var ownership = KeyboardCaptureOwnership()
    private static var dedicatedCaptures: [Int: KeyboardWebCapture] = [:]
    private static var lastOwnerID = 0
    private let ownerID: Int = {
        KeyboardLocalDictation.lastOwnerID += 1
        return KeyboardLocalDictation.lastOwnerID
    }()
    private var capture: KeyboardWebCapture?
    /// Another app played audio as this capture began, which WebKit's capture interrupts.
    private var otherAudioAtStart = false
    /// After a capture ends, long enough for WebKit to have let its audio session go.
    static let resumeOtherAudioDelay: TimeInterval = 0.5
    private static let audioSessionQueue = DispatchQueue(label: "anomaly.hex.keyboard.audio-session", qos: .userInitiated)

    /// Keyboard dictation sets this process's audio session to record (WebKit captures for
    /// it), and iOS mutes a recording session's haptics by default: key haptics stay on. Set
    /// once per process on the audio session's queue: it waits on the audio server, which the
    /// keyboard's main thread must not do while iOS waits for its first frame.
    static func allowHapticsWhileRecording() {
        audioSessionQueue.async { _ = hapticsAllowed }
    }

    nonisolated private static let hapticsAllowed: Void = {
        try? AVAudioSession.sharedInstance().setAllowHapticsAndSystemSoundsDuringRecording(true)
    }()
    /// A touch-down capture neither kept nor dropped by then is dropped, so a lost touch never
    /// leaves the microphone open. A hold keeps it at 0.35 s.
    static let armedTimeout: TimeInterval = 3
    /// Learn how I type's dictation fixes, read when a dictation starts; empty while off.
    var corrections: () -> [DictationCorrection] = { [] }
    /// The field the keyboard is in, read when a dictation starts.
    var currentDocument: () -> UUID? = { nil }

    /// Updates the active job's style when the user cycles style on the recording panel.
    func updateActiveStyle(_ style: DictationStyle) {
        session.job?.preferences.style = style
    }

    static var isSupported: Bool {
        if #available(iOS 26.0, *) { return true }
        return false
    }

    /// Memory left below iOS's limit for this keyboard that the first dictation needs to start:
    /// the capture web view, its page and WebKit's first use (7–11 MB in the simulator with
    /// the recording screen), the Metal scope's device and pipeline (about 3.5 MB of GPU
    /// memory the simulator doesn't count) and drawables, with margin. iOS ends a keyboard
    /// that passes its limit, which varies by model (testers' 4 GB iPhones hit it), and
    /// switches to the system keyboard; short of this, Dictate opens Hex instead.
    static let startHeadroomNeeded = 32 * 1_048_576
    /// Later dictations reuse the loaded web view, so only the capture and recording screen
    /// start (1–5 MB in the simulator on the dictation bar; up to 8 MB on the full keyboard,
    /// which rebuilds its recording strip and briefly peaks leaving it).
    static let restartHeadroomNeeded = 24 * 1_048_576

    /// Whether in-keyboard dictation can start without passing the memory limit. Where iOS
    /// doesn't say (0, the simulator), it can.
    deinit {
        let id = ownerID
        Task { @MainActor in KeyboardLocalDictation.ownerDidDeinit(id) }
    }

    var hasMemoryToStart: Bool {
        let headroom = os_proc_available_memory()
        let warm = (capture ?? Self.sharedCapture) != nil
        return headroom == 0 || headroom >= (warm ? Self.restartHeadroomNeeded : Self.startHeadroomNeeded)
    }

    var isActive: Bool { phase.isActive }

    /// The capture for `jobID` is the one running: armed, starting, or recording.
    private func isCapturing(_ jobID: String) -> Bool { phase.isCapturing(jobID) }

    func snapshot(now: TimeInterval) -> KeyboardSnapshot? {
        if case .failed(_, let until) = session, until <= now {
            setSession(.idle)
            return nil
        }
        return phase.snapshot(now: now, loudnessDB: session.loudness)
    }

    func markDelivered(resultID: String) {
        if case .finished(_, let result) = session, result.id == resultID {
            setSession(.idle)
        }
    }

    /// Makes the capture web view and loads its page before the first dictation, so the Hex
    /// key only has the microphone left to open. Nothing here asks for the microphone or the
    /// audio session, so no orange dot shows until a dictation starts. Skipped while short of
    /// the memory a first dictation needs (it would open Hex instead) and without the
    /// microphone allowed (it would fail).
    func prepare(in host: UIView) {
        guard Self.isSupported, capture == nil, !isActive, Self.ownership.canAcquire(by: ownerID) else { return }
        let headroom = os_proc_available_memory()
        let needed = Self.sharedCapture == nil ? Self.startHeadroomNeeded : Self.restartHeadroomNeeded
        guard headroom == 0 || headroom >= needed else { return }
        guard Self.microphoneAllowed else { return }
        if let reused = Self.sharedCapture {
            Self.ownership.acquireIdle(by: ownerID)
            reused.attach(to: host)
            listenWhileIdle(reused)
            capture = reused
            return
        }
        let started = Date()
        let created = KeyboardWebCapture(host: host)
        Self.sharedCapture = created
        Self.ownership.acquireIdle(by: ownerID)
        capture = created
        listenWhileIdle(created)
        KeyboardProbeLog.shared.write("capture prepared in \(Int(Date().timeIntervalSince(started) * 1000)) ms \(KeyboardProbeLog.memory())")
    }

    private func acquireCaptureForStart(in host: UIView) -> KeyboardWebCapture {
        if let own = capture, own !== Self.sharedCapture {
            own.attach(to: host)
            return own
        }
        if Self.ownership.canAcquire(by: ownerID) {
            let shared = Self.sharedCapture ?? KeyboardWebCapture(host: host)
            shared.attach(to: host)
            Self.sharedCapture = shared
            Self.ownership.beginCapture(by: ownerID)
            capture = shared
            return shared
        }
        // Another live keyboard is actively recording with the shared web view: use a dedicated
        // one here rather than stealing its stream.
        let dedicated = KeyboardWebCapture(host: host)
        capture = dedicated
        Self.dedicatedCaptures[ownerID] = dedicated
        return dedicated
    }

    private func endCaptureOwnership() {
        if capture === Self.sharedCapture {
            Self.ownership.endCapture(by: ownerID)
        }
    }

    private func stopAndListenWhileIdleIfOwned() {
        if let capture, capture !== Self.sharedCapture || Self.ownership.owner == ownerID {
            listenWhileIdle(capture)
            capture.stop()
        }
    }

    /// Detaches the capture web view when this keyboard hides, only if this keyboard still owns
    /// the shared capture (or nobody does). When a newer keyboard already took ownership, this
    /// keyboard drops only its own pointer.
    private func parkCapture() {
        guard let current = capture else { return }
        capture = nil
        guard current === Self.sharedCapture else {
            Self.dedicatedCaptures.removeValue(forKey: ownerID)
            current.tearDown()
            return
        }
        guard Self.ownership.park(by: ownerID) else { return }
        current.detach()
        Self.listenWhileParked(current)
    }

    private static func ownerDidDeinit(_ id: Int) {
        if let dedicated = dedicatedCaptures.removeValue(forKey: id) {
            dedicated.stop()
            dedicated.tearDown()
        }
        guard ownership.ownerDidDeinit(id), let capture = sharedCapture else { return }
        capture.stop()
        capture.detach()
        listenWhileParked(capture)
    }

    func handleMemoryWarning() {
        switch session {
        case .armed(.capturing(let state)), .active(let state):
            state.resources.sendMemoryWarning()
        default:
            break
        }
        KeyboardParakeet.shared.handleMemoryWarning()
        guard !isActive else { return }
        let current = capture
        capture = nil
        if let current, current !== Self.sharedCapture {
            Self.dedicatedCaptures.removeValue(forKey: ownerID)
            current.tearDown()
        }
        guard Self.ownership.evictForMemoryWarning(by: ownerID) else { return }
        let evicted = Self.sharedCapture
        Self.sharedCapture = nil
        evicted?.tearDown()
    }

    /// The page between dictations: a prepared page loading, or one whose content process
    /// ends, which the next start replaces.
    private func listenWhileIdle(_ capture: KeyboardWebCapture) {
        capture.onEvent = { [weak self, weak capture] event in
            guard let self, let capture else { return }
            self.handleIdle(event, capture: capture)
        }
    }

    private static func listenWhileParked(_ capture: KeyboardWebCapture) {
        capture.onEvent = { [weak capture] event in
            guard let capture else { return }
            switch event {
            case .loaded:
                let ms = Int((Date().timeIntervalSince1970 - capture.createdAt) * 1000)
                KeyboardProbeLog.shared.write("capture page loaded in \(ms) ms \(KeyboardProbeLog.memory())")
            case .processTerminated:
                if sharedCapture === capture, ownership.dropTerminated(by: nil) {
                    sharedCapture = nil
                    Task { @MainActor in capture.tearDown() }
                }
            case .started:
                capture.stop()
            default:
                break
            }
        }
    }

    private func handleIdle(_ event: KeyboardWebCapture.Event, capture: KeyboardWebCapture) {
        switch event {
        case .loaded:
            let ms = Int((Date().timeIntervalSince1970 - capture.createdAt) * 1000)
            KeyboardProbeLog.shared.write("capture page loaded in \(ms) ms \(KeyboardProbeLog.memory())")
        case .processTerminated:
            if capture === self.capture || capture === Self.sharedCapture { dropEndedCapture() }
        case .started:
            // A dropped start that got its microphone anyway: close it.
            capture.stop()
        default:
            break
        }
    }

    /// The web view's content process ended: the next start makes a new one, and this one
    /// leaves the keyboard's view once WebKit's callback has returned.
    private func dropEndedCapture() {
        let current = capture
        capture = nil
        if let current, current !== Self.sharedCapture {
            Self.dedicatedCaptures.removeValue(forKey: ownerID)
            Task { @MainActor in current.tearDown() }
            return
        }
        guard Self.ownership.dropTerminated(by: ownerID) else { return }
        let ended = Self.sharedCapture
        Self.sharedCapture = nil
        Task { @MainActor in ended?.tearDown() }
    }

    /// The keyboard extension itself may not record (audiomxd: "it is an extension and
    /// doesn't have entitlements to record audio"), so capture runs in WebKit's media
    /// process through getUserMedia, attributed to Hex's microphone permission.
    /// The page asks for Settings › Microphone's input by name (`microphone(for:)`): left to
    /// itself, WebKit takes the input of the route already playing, the iPhone's microphone
    /// while a car or headphones play over A2DP, whatever the setting.
    /// WebKit's capture session doesn't mix: other apps' video and music pause while it
    /// records, and `releaseAudioSession` asks them to resume afterwards.
    /// `pressedAt` is the Hex key's touch-down (wall clock), where the start's timeline begins.
    /// A capture armed at touch-down becomes this dictation.
    func start(in host: UIView, microphone: MicrophoneChoice, pressedAt: TimeInterval? = nil) {
        if case .armed = session {
            keepArmed()
            return
        }
        guard !isActive else { return }
        begin(in: host, microphone: microphone, pressedAt: pressedAt, armed: false)
    }

    /// Touch-down on the Hex key: capture starts now, unseen, so the first words aren't lost
    /// while the touch becomes a tap or a hold (`start` keeps it) or a slide or a cancelled
    /// touch (`discard` drops it, with nothing transcribed). The microphone opens here.
    func arm(in host: UIView, microphone: MicrophoneChoice, pressedAt: TimeInterval) {
        // Without the microphone, the tap's own start says so.
        guard !isActive, Self.microphoneAllowed else { return }
        guard let jobID = begin(in: host, microphone: microphone, pressedAt: pressedAt, armed: true) else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.armedTimeout) { [weak self] in
            guard let self, case .armed(let armed) = self.session, armed.job.id == jobID else { return }
            self.discard(reason: "never kept")
        }
    }

    /// The touch became a slide (the style menu) or was cancelled: the touch-down capture
    /// stops, its audio is deleted, and nothing is transcribed.
    func discard(reason: String = "slide or cancelled touch") {
        guard case .armed(let armed) = session else { return }
        let startedAt = armed.requestedAt
        if case .capturing(var state) = armed {
            state.resources.discard()
        }
        endCaptureOwnership()
        stopAndListenWhileIdleIfOwned()
        setSession(.idle)
        releaseAudioSession()
        KeyboardProbeLog.shared.write("touch-down capture discarded after \(Int((Date().timeIntervalSince1970 - startedAt) * 1000)) ms (\(reason))")
    }

    private func keepArmed() {
        guard case .armed(let armed) = session else { return }
        switch armed {
        case .failed(_, _, let failure):
            endCaptureOwnership()
            releaseAudioSession()
            KeyboardProbeLog.shared.write("FAILED: \(failure.detail ?? failure.message) \(KeyboardProbeLog.memory())")
            setSession(.failed(message: failure.message, until: Date().timeIntervalSince1970 + 6))
        case .capturing(let state):
            setSession(.active(state))
            let liveLabel = state.resources.isStreaming ? "already live" : "waiting for first audio"
            KeyboardProbeLog.shared.write("touch-down capture kept after \(Int((Date().timeIntervalSince1970 - state.requestedAt) * 1000)) ms (\(liveLabel))")
        }
    }

    /// This process keeps the permission it read when it started, so a keyboard opened before
    /// Hex was allowed the microphone (onboarding's practice field) still reads it as not
    /// allowed: Hex's own reading counts too. WebKit still checks (NotAllowedError).
    private static var microphoneAllowed: Bool {
        AVAudioApplication.shared.recordPermission == .granted || KeyboardBridge().appMicrophoneAllowed
    }

    /// Opens the microphone for a new job, armed (touch-down) or recording; nil when it can't.
    @discardableResult
    private func begin(in host: UIView, microphone: MicrophoneChoice, pressedAt: TimeInterval?, armed: Bool) -> String? {
        let log = KeyboardProbeLog.shared
        let appAllowed = KeyboardBridge().appMicrophoneAllowed
        guard Self.microphoneAllowed else {
            log.write("start(web\(armed ? ", touch-down" : "")): mic=\(Self.describe(AVAudioApplication.shared.recordPermission)) app=no")
            fail("Allow the microphone in Hex first")
            return nil
        }
        let jobID = UUID().uuidString
        let requestedAt = Date()
        let requestedTime = requestedAt.timeIntervalSince1970
        let preferences = DictationPipeline.Preferences.current(
            style: KeyboardBridge().style,
            corrections: corrections()
        )
        let job = Job(id: jobID, document: currentDocument(), preferences: preferences)
        let timeline = CaptureTimeline(
            pressedAt: min(pressedAt ?? requestedTime, requestedTime),
            requestedAt: requestedTime
        )
        let state = CapturingState(
            job: job,
            requestedAt: requestedTime,
            watch: KeyboardCaptureWatch(startedAt: requestedTime),
            resources: .awaitingAudioSession(timeline: timeline)
        )
        setSession(armed ? .armed(.capturing(state)) : .active(state))
        let capture = acquireCaptureForStart(in: host)
        capture.onEvent = { [weak self] event in
            self?.handle(event, jobID: jobID, requestedAt: requestedAt)
        }
        // The audio session's calls are round trips to the audio server, which can be slow to
        // answer (a call, another app's audio, a capture that just ended): on the main thread
        // they held every key until it did. They run on the session's queue, after any earlier
        // capture's release there, and the page starts once they're back, unless the job was
        // dropped, stopped or replaced meanwhile.
        let label = armed ? ", touch-down" : ""
        Self.audioSessionQueue.async { [weak self, weak capture] in
            log.write("start(web\(label)): mic=\(Self.describe(AVAudioApplication.shared.recordPermission)) app=\(appAllowed ? "granted" : "no") speech=\(Self.describe(SFSpeechRecognizer.authorizationStatus())) \(KeyboardProbeLog.memory())")
            let input = Self.microphone(for: microphone)
            let otherAudio = AVAudioSession.sharedInstance().isOtherAudioPlaying
            DispatchQueue.main.async {
                MainActor.assumeIsolated {
                    guard let self, let capture, self.capture === capture,
                          self.advanceFromAudioSession(jobID: jobID) else { return }
                    self.otherAudioAtStart = self.otherAudioAtStart || otherAudio
                    // Without a name to ask for, every choice but Bluetooth asks for the iPhone's own.
                    capture.start(
                        microphone: input?.name,
                        builtInFallback: microphone != .bluetooth,
                        voiceIsolation: VoiceIsolationSettings().isEnabled
                    )
                }
            }
        }
        return jobID
    }

    private func advanceFromAudioSession(jobID: String) -> Bool {
        switch session {
        case .armed(.capturing(var state)) where state.job.id == jobID:
            guard case .awaitingAudioSession(let timeline) = state.resources else { return false }
            state.resources = .awaitingPage(timeline: timeline)
            setSession(.armed(.capturing(state)))
            return true
        case .active(var state) where state.job.id == jobID:
            guard case .awaitingAudioSession(let timeline) = state.resources else { return false }
            state.resources = .awaitingPage(timeline: timeline)
            setSession(.active(state))
            return true
        default:
            return false
        }
    }

    /// Asks the video or music the capture interrupted to resume (`InterruptedAudio`), once
    /// WebKit has let its session go and unless another capture has started.
    private func releaseAudioSession() {
        guard otherAudioAtStart else { return }
        otherAudioAtStart = false
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.resumeOtherAudioDelay) { [weak self] in
            guard let self else { return }
            if self.session.isHoldingAudioSession {
                // Another capture started: its end asks instead.
                self.otherAudioAtStart = true
                return
            }
            Self.audioSessionQueue.async {
                let session = AVAudioSession.sharedInstance()
                guard InterruptedAudio.shouldAskToResume(playingAtStart: true, playingNow: session.isOtherAudioPlaying) else {
                    KeyboardProbeLog.shared.write("other audio kept playing through the capture")
                    return
                }
                do {
                    try InterruptedAudio.askToResume(session)
                    KeyboardProbeLog.shared.write("asked the audio the capture interrupted to resume")
                } catch {
                    let ns = error as NSError
                    KeyboardProbeLog.shared.write("asking interrupted audio to resume failed: \(ns.domain) \(ns.code)")
                }
            }
        }
    }

    /// The input Settings › Microphone takes on this route (`MicrophoneRoute.capture`), read
    /// from the keyboard's own session with the category Hex records in: CarPlay's and
    /// Bluetooth hands-free inputs are listed only there. WebKit records in its own session,
    /// which only getUserMedia's `deviceId` steers (WebKit then prefers that input), so the
    /// page asks for this input by its name, the label WebKit gives it.
    nonisolated private static func microphone(for choice: MicrophoneChoice) -> Microphone? {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(
                .playAndRecord,
                mode: .measurement,
                options: MicrophoneRoute.categoryOptions(allowingBluetooth: choice.allowsBluetooth)
            )
            let inputs = session.microphones
            let input = MicrophoneRoute.capture(choice, from: inputs)
            try? session.prefer(input)
            let listed = inputs.map { "\($0.name) (\($0.port.rawValue))" }.joined(separator: ", ")
            KeyboardProbeLog.shared.write("microphone \(choice.rawValue): \(input?.name ?? "none") of [\(listed)]")
            return input
        } catch {
            KeyboardProbeLog.shared.write("microphone \(choice.rawValue) unknown: \(describe(error))")
            return nil
        }
    }

    private func mutateCapturing(jobID: String, _ body: (inout CapturingState) -> Void) -> Bool {
        switch session {
        case .armed(.capturing(var state)) where state.job.id == jobID:
            body(&state)
            setSession(.armed(.capturing(state)))
            return true
        case .active(var state) where state.job.id == jobID:
            body(&state)
            setSession(.active(state))
            return true
        default:
            return false
        }
    }

    private func handle(_ event: KeyboardWebCapture.Event, jobID: String, requestedAt: Date) {
        let log = KeyboardProbeLog.shared
        switch event {
        case .started(let sampleRate, let device, let steps):
            log.write("web capture started in \(Int(Date().timeIntervalSince(requestedAt) * 1000)) ms at \(Int(sampleRate)) Hz from \(device) \(KeyboardProbeLog.memory())")
            var openError: Error?
            var accepted = false
            let matched = mutateCapturing(jobID: jobID) { state in
                guard case .awaitingPage(var timeline) = state.resources else { return }
                accepted = true
                if let capture {
                    timeline.noteStarted(capture: capture, steps: steps, sampleRate: sampleRate)
                }
                do {
                    let url = FileManager.default.temporaryDirectory.appendingPathComponent("keyboard-\(jobID).wav")
                    let pcmFile = try KeyboardPCMFile(url: url, sampleRate: sampleRate)
                    let live = KeyboardLiveDictation.start(sampleRate: sampleRate, text: liveText)
                    state.resources = .awaitingFirstAudio(
                        sink: AudioSink(pcmFile: pcmFile, live: live),
                        timeline: timeline
                    )
                } catch {
                    openError = error
                }
            }
            guard matched, accepted else {
                // Done came before the microphone did, or a stale page started: nothing to record.
                capture?.stop()
                return
            }
            if let openError {
                capture?.stop()
                fail(UserFacingError.Context.microphone.fallback, detail: "Could not create audio file: \(Self.describe(openError))")
            }
        case .samples(let data, let peak):
            guard !data.isEmpty else { return }
            let now = Date().timeIntervalSince1970
            _ = mutateCapturing(jobID: jobID) { state in
                state.watch.heardAudio(at: now)
                state.loudness = peak > 0 ? max(-62, 20 * log10(peak)) : -62
                switch state.resources {
                case .awaitingFirstAudio(var sink, let timeline):
                    log.write("\(timeline.firstBuffer(at: now)) \(KeyboardProbeLog.memory())")
                    sink.append(data)
                    state.resources = .streaming(sink: sink, audioStartedAt: now)
                case .streaming(var sink, let audioStartedAt):
                    sink.append(data)
                    state.resources = .streaming(sink: sink, audioStartedAt: audioStartedAt)
                case .awaitingAudioSession, .awaitingPage:
                    break
                }
            }
        case .state(let pageState):
            let now = Date().timeIntervalSince1970
            _ = mutateCapturing(jobID: jobID) { state in
                let into = String(format: "%.1f", pageState.at - requestedAt.timeIntervalSince1970)
                log.write("page \(pageState.cause) at +\(into) s: \(pageState.summary)")
                switch state.watch.page(pageState, at: now) {
                case .interrupted:
                    log.write("capture interrupted: waiting up to \(Int(KeyboardJobWatchdog.captureInterruption)) s for it to come back")
                case .resumed(let after):
                    log.write("capture back after \(String(format: "%.1f", after)) s: the same dictation goes on")
                case nil:
                    break
                }
            }
        case .stopped:
            finishRecording(jobID: jobID)
        case .failed(let message):
            guard isCapturing(jobID) else { return }
            capture?.stop()
            fail(
                message.hasPrefix("NotAllowedError") ? "Allow the microphone in Hex first" : UserFacingError.Context.microphone.fallback,
                detail: "Web capture: \(message)"
            )
        case .processTerminated:
            // The next start needs a new web view; this one leaves the keyboard's view too,
            // instead of staying behind with its page.
            dropEndedCapture()
            // Only a capture loses anything: a later dictation's page may end while idle.
            guard isCapturing(jobID) else {
                KeyboardProbeLog.shared.write("capture process ended while idle")
                return
            }
            fail(UserFacingError.Context.microphone.fallback, detail: "WebKit capture process ended")
        case .loaded:
            break
        }
    }

    func cancel() {
        switch session {
        case .armed(.capturing(var state)), .active(var state):
            state.resources.discard()
        case .transcribing(_, _, let task):
            task?.cancel()
        default:
            break
        }
        endCaptureOwnership()
        stopAndListenWhileIdleIfOwned()
        setSession(.idle)
        releaseAudioSession()
        KeyboardProbeLog.shared.write("cancelled")
    }

    func stop() {
        guard case .active(let state) = session else { return }
        switch state.resources {
        case .awaitingAudioSession, .awaitingPage, .awaitingFirstAudio:
            // Stopped (e.g. push-to-talk lifted early) before any audio arrived: cancel startup quietly.
            stopUnstartedCapture(reason: "stopped before audio arrived")
        case .streaming:
            capture?.stop()
        }
    }

    private func stopUnstartedCapture(reason: String) {
        if case .active(var state) = session {
            state.resources.discard()
        }
        stopAndListenWhileIdleIfOwned()
        endQuietly(reason)
    }

    /// Type as I speak's field just read empty with its words in it: the audio from here is held
    /// until it is sure (`KeyboardLiveTyping.sendConfirmation`), then goes to a new dictation
    /// (a send) or on into this one, so the cut is where the field first read empty.
    func holdForSend() {
        guard case .active(var state) = session,
              case .streaming(var sink, let startedAt) = state.resources,
              sink.sendHeld == nil else { return }
        sink.sendHeld = []
        state.resources = .streaming(sink: sink, audioStartedAt: startedAt)
        setSession(.active(state))
    }

    func releaseSend(sent: Bool) {
        guard case .active(var state) = session,
              case .streaming(var sink, var startedAt) = state.resources,
              let held = sink.sendHeld else { return }
        sink.sendHeld = nil
        let ms = Int(Double(held.reduce(0) { $0 + $1.count } / 2) / sink.pcmFile.sampleRate * 1000)
        KeyboardProbeLog.shared.write("\(sent ? "a send" : "not a send"): \(held.count) chunks (\(ms) ms) held since the field read empty go to the \(sent ? "new" : "same") dictation")
        if sent {
            guard let refreshed = startAfresh(sink: &sink, startedAt: startedAt) else { return }
            startedAt = refreshed
        }
        for chunk in held {
            sink.append(chunk)
        }
        state.resources = .streaming(sink: sink, audioStartedAt: startedAt)
        setSession(.active(state))
    }

    /// Type as I speak's words were sent: the dictation so far is over and its audio goes, and
    /// the same capture goes on as a new dictation with its own file, live words, final text and
    /// length (`KeyboardParakeetBudget.longestAudio`). Every chunk the page sends lands in
    /// exactly one dictation's file, so none is lost or transcribed twice.
    private func startAfresh(sink: inout AudioSink, startedAt: TimeInterval) -> TimeInterval? {
        let old = sink.pcmFile
        let frames = old.close()
        try? FileManager.default.removeItem(at: old.url)
        sink.stopLive()
        if let text = liveText, text.transcript != nil {
            text.begin(showsPreview: text.showsPreview, typesIntoField: text.typesIntoField)
        }
        do {
            sink.pcmFile = try KeyboardPCMFile(url: old.url, sampleRate: old.sampleRate)
        } catch {
            capture?.stop()
            fail(UserFacingError.Context.microphone.fallback, detail: "Could not create audio file: \(Self.describe(error))")
            return nil
        }
        sink.live = KeyboardLiveDictation.start(sampleRate: old.sampleRate, text: liveText)
        let restartedAt = Date().timeIntervalSince1970
        KeyboardProbeLog.shared.write("sent after \(String(format: "%.1f", restartedAt - startedAt)) s, frames=\(frames): a new dictation from the next chunk \(KeyboardProbeLog.memory())")
        return restartedAt
    }

    /// The keyboard is going away mid-dictation (dismissed, or another app came forward).
    /// A touch still deciding is dropped; a recording stops and keeps what was said, without
    /// waiting for the page (WebKit may not answer once the keyboard is gone); and the job's
    /// text is held for the next keyboard instead of being lost with this one.
    func keyboardWillHide() {
        switch session {
        case .armed:
            discard(reason: "keyboard hidden")
        case .active(let state):
            if state.resources.isStreaming {
                state.job.interrupted = true
                KeyboardProbeLog.shared.write("keyboard hidden while recording: finishing with what was said")
                stopAndListenWhileIdleIfOwned()
                finishRecording(jobID: state.job.id)
            } else {
                stopUnstartedCapture(reason: "keyboard hidden before audio arrived")
            }
        case .transcribing(let job, _, _):
            if !job.interrupted {
                job.interrupted = true
                KeyboardProbeLog.shared.write("keyboard hidden while transcribing: holding its text for the next keyboard")
            }
        case .finished(let job, let result):
            // Shown but not yet inserted: the next keyboard inserts it.
            Self.hold(resultID: result.id, text: result.transcript, heard: result.heard, document: job.document)
            setSession(.idle)
        case .idle, .failed:
            break
        }
        parkCapture()
    }

    /// Self-heal, on every poll while the keyboard shows: a capture that went quiet finishes
    /// with what it heard, and a transcription that never returns is given up, so the
    /// keyboard never stays in recording or transcribing with nothing behind it.
    func healIfStalled(now: TimeInterval) {
        #if targetEnvironment(simulator)
        runSimulatorScript()
        #endif
        switch session {
        case .active(let state) where state.watch.isStalled(now: now):
            if state.resources.isStreaming {
                if let since = state.watch.interruptedSince {
                    KeyboardProbeLog.shared.write("capture interrupted for \(Int(now - since)) s: finishing with what it heard")
                } else {
                    KeyboardProbeLog.shared.write("capture silent for \(Int(now - state.watch.lastAudio)) s: finishing with what it heard")
                }
                if let capture {
                    listenWhileIdle(capture)
                    capture.stop()
                }
                finishRecording(jobID: state.job.id)
            } else {
                KeyboardProbeLog.shared.write("capture timed out after \(Int(now - state.watch.lastAudio)) s before first audio")
                stopAndListenWhileIdleIfOwned()
                fail(UserFacingError.Context.microphone.fallback, detail: "capture timed out before first audio")
            }
        case .transcribing(_, let startedAt, let task) where KeyboardJobWatchdog.isStalled(.transcribing, lastProgress: startedAt, now: now):
            KeyboardProbeLog.shared.write("transcription stuck for \(Int(now - startedAt)) s: giving up")
            task?.cancel()
            setSession(.idle)
        default:
            break
        }
    }

    #if targetEnvironment(simulator)
    /// Simulator only, which has no microphone and no interruptions (`ios/docs/dictation.md`):
    /// `capture-debug.js` left in the App Group runs once in the capture page, to stand in a
    /// recording for the microphone or interrupt the capture by hand, and each recording is
    /// kept there as `capture-debug.wav` to hear what the keyboard got.
    private func runSimulatorScript() {
        guard let capture, let group = Self.simulatorFolder else { return }
        let url = group.appendingPathComponent("capture-debug.js")
        guard let script = try? String(contentsOf: url, encoding: .utf8) else { return }
        try? FileManager.default.removeItem(at: url)
        capture.evaluate(script)
        KeyboardProbeLog.shared.write("simulator: ran capture-debug.js (\(script.count) characters)")
    }

    private static func keepForSimulator(_ recording: URL) {
        guard let kept = simulatorFolder?.appendingPathComponent("capture-debug.wav") else { return }
        try? FileManager.default.removeItem(at: kept)
        try? FileManager.default.copyItem(at: recording, to: kept)
    }

    private static var simulatorFolder: URL? {
        FileManager.default.containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)
    }
    #endif

    private static func hold(resultID: String, text: String, heard: String?, document: UUID?) {
        KeyboardBridge().heldDictation = KeyboardHeldDictation(
            id: resultID,
            text: text,
            heard: heard,
            document: document,
            finishedAt: Date().timeIntervalSince1970
        )
        KeyboardProbeLog.shared.write("held a dictation for the next keyboard: \(text.count) chars")
    }

    private func finishRecording(jobID: String) {
        releaseSend(sent: false)
        guard case .active(let state) = session, state.job.id == jobID else { return }
        guard case .streaming(var sink, let startedAt) = state.resources else {
            stopUnstartedCapture(reason: "stopped before audio arrived")
            return
        }
        sink.stopLive()
        endCaptureOwnership()
        releaseAudioSession()
        let job = state.job
        let file = sink.pcmFile
        let seconds = Date().timeIntervalSince1970 - startedAt
        let frames = file.close()
        KeyboardProbeLog.shared.write("stopped after \(String(format: "%.1f", seconds)) s, frames=\(frames) \(KeyboardProbeLog.memory())")
        #if targetEnvironment(simulator)
        Self.keepForSimulator(file.url)
        #endif
        guard !EmptyCapture.isTooShort(frames: frames, sampleRate: file.sampleRate) else {
            try? FileManager.default.removeItem(at: file.url)
            endQuietly("too short to hold a word")
            return
        }
        guard #available(iOS 26.0, *) else {
            setSession(.idle)
            return
        }
        let transcribingStartedAt = Date().timeIntervalSince1970
        setSession(.transcribing(job: job, startedAt: transcribingStartedAt, task: nil))
        let url = file.url
        let preferences = job.preferences
        let hints = preferences.recognitionHints
        let task = Task { [weak self] in
            let started = Date()
            do {
                let (heard, language, engine) = try await KeyboardTranscription.transcribe(url, hints: hints, audioSeconds: seconds)
                try? FileManager.default.removeItem(at: url)
                let locale = language.identifier
                // The app's steps (but Apple Intelligence's cleanup), snapshotted at capture start.
                let pipeline = DictationPipeline(preferences: preferences, language: locale)
                let steps = await pipeline.run(heard, translationSource: language.locale.language)
                let translated = steps.translated
                let text = steps.formatted
                KeyboardProbeLog.shared.write("transcribed in \(String(format: "%.2f", Date().timeIntervalSince(started))) s: \(text.count) chars \(KeyboardProbeLog.memory())")
                if let failure = steps.translationFailure {
                    KeyboardProbeLog.shared.write("pasted as spoken: \(failure.source)->\(failure.target) \(failure.reason.rawValue) \(failure.detail ?? "")")
                }
                guard !Task.isCancelled else { return }
                // A translation's words can't be compared with what was heard, so it isn't learned from.
                let resultID = UUID().uuidString
                let learnable = translated == nil ? steps.heard : nil
                guard let self, !job.interrupted else {
                    // The keyboard hid before it finished, or is gone: the next one delivers it.
                    if !text.isEmpty {
                        Self.hold(resultID: resultID, text: text, heard: learnable, document: job.document)
                    }
                    if let self, self.session.job === job {
                        self.setSession(.idle)
                    }
                    return
                }
                // Record typing logs keeps every step; nothing is kept while it is off.
                let stages: DictationStages? = TypingLogSettings().isEnabled
                    ? DictationStages(
                        engine: engine,
                        locale: locale,
                        style: preferences.style.rawValue,
                        recordedMS: Int((seconds * 1000).rounded()),
                        transcribeMS: Int((Date().timeIntervalSince(started) * 1000).rounded()),
                        raw: heard,
                        fixed: steps.fixed,
                        spelled: preferences.vocabulary.isEmpty ? nil : steps.spelled,
                        tagged: CleanupTaggerSettings().isEnabled && preferences.style != .verbatim ? steps.tagged : nil,
                        translated: translated,
                        formatted: steps.formatted
                    )
                    : nil
                self.setSession(.finished(
                    job: job,
                    result: KeyboardResult(
                        id: resultID,
                        transcript: text,
                        heard: learnable,
                        stages: stages,
                        translationFailure: steps.translationFailure
                    )
                ))
            } catch {
                try? FileManager.default.removeItem(at: url)
                guard let self, !Task.isCancelled,
                      case .transcribing(let currentJob, _, _) = self.session, currentJob.id == jobID else { return }
                if EmptyCapture.endsQuietly(error) {
                    self.endQuietly("no speech")
                } else {
                    self.fail(
                        UserFacingError.message(for: error, context: .transcription),
                        detail: "Transcription failed: \(Self.describe(error))"
                    )
                }
            }
        }
        if case .transcribing(let currentJob, let startedAt, _) = session, currentJob === job {
            setSession(.transcribing(job: job, startedAt: startedAt, task: task))
        }
    }

    /// `message` is what the keyboard shows; `detail`, for the log only, may carry codes.
    private func fail(_ message: String, detail: String? = nil) {
        if case .armed(let armed) = session {
            if case .capturing(var state) = armed {
                state.resources.discard()
            }
            endCaptureOwnership()
            releaseAudioSession()
            let failure = KeyboardLocalCapturePhase.ArmedFailure(message: message, detail: detail)
            setSession(.armed(.failed(job: armed.job, requestedAt: armed.requestedAt, failure: failure)))
            KeyboardProbeLog.shared.write("FAILED at touch-down (held): \(detail ?? message) \(KeyboardProbeLog.memory())")
            return
        }
        if case .active(var state) = session {
            state.resources.discard()
        }
        endCaptureOwnership()
        releaseAudioSession()
        KeyboardProbeLog.shared.write("FAILED: \(detail ?? message) \(KeyboardProbeLog.memory())")
        setSession(.failed(message: message, until: Date().timeIntervalSince1970 + 6))
    }

    /// Nothing to insert and nothing wrong: back to idle without a message.
    private func endQuietly(_ reason: String) {
        if case .active(var state) = session {
            state.resources.discard()
        }
        endCaptureOwnership()
        releaseAudioSession()
        KeyboardProbeLog.shared.write("ended quietly: \(reason)")
        // For onboarding's practice only; the keyboard shows nothing.
        KeyboardBridge().lastEmptyDictationAt = Date().timeIntervalSince1970
        setSession(.idle)
    }

    nonisolated private static func describe(_ error: Error) -> String {
        let ns = error as NSError
        return "\(ns.domain) \(ns.code) \(ns.localizedDescription)"
    }

    nonisolated private static func describe(_ permission: AVAudioApplication.recordPermission) -> String {
        switch permission {
        case .granted: "granted"
        case .denied: "denied"
        case .undetermined: "undetermined"
        @unknown default: "unknown"
        }
    }

    nonisolated private static func describe(_ status: SFSpeechRecognizerAuthorizationStatus) -> String {
        switch status {
        case .authorized: "authorized"
        case .denied: "denied"
        case .restricted: "restricted"
        case .notDetermined: "notDetermined"
        @unknown default: "unknown"
        }
    }
}
