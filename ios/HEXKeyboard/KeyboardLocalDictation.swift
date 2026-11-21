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
    private enum Phase {
        case idle
        /// The Hex key's touch-down, before the touch is a tap, a hold or a slide: capturing,
        /// unseen (no snapshot), until `start` keeps it or `discard` drops it.
        case armed(jobID: String, startedAt: TimeInterval)
        case recording(jobID: String, startedAt: TimeInterval)
        case transcribing(jobID: String)
        case finished(jobID: String, resultID: String, transcript: String, heard: String?, stages: DictationStages?)
        case failed(message: String, until: TimeInterval)
    }

    private var phase: Phase = .idle
    private var capture: KeyboardWebCapture?
    private var pcmFile: KeyboardPCMFile?
    private var loudness: Float = -62
    private var transcription: Task<Void, Never>?
    /// The current start's steps, logged once its first audio arrives.
    private var timeline: CaptureTimeline?
    /// A failure while armed, shown only if the touch is kept: a slide to the menu drops it.
    private var armedFailure: (message: String, detail: String?)?
    /// A touch-down capture neither kept nor dropped by then is dropped, so a lost touch never
    /// leaves the microphone open. A hold keeps it at 0.35 s.
    static let armedTimeout: TimeInterval = 3
    /// Learn how I type's dictation fixes, read when a dictation starts; empty while off.
    var corrections: () -> [DictationCorrection] = { [] }
    private var activeCorrections: [DictationCorrection] = []

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
    var hasMemoryToStart: Bool {
        let headroom = os_proc_available_memory()
        return headroom == 0 || headroom >= (capture == nil ? Self.startHeadroomNeeded : Self.restartHeadroomNeeded)
    }

    var isActive: Bool {
        switch phase {
        case .armed, .recording, .transcribing: true
        default: false
        }
    }

    /// The capture for `jobID` is the one running, armed or recording.
    private func isCapturing(_ jobID: String) -> Bool {
        switch phase {
        case .armed(let current, _), .recording(let current, _): current == jobID
        default: false
        }
    }

    func snapshot(now: TimeInterval) -> KeyboardSnapshot? {
        switch phase {
        case .idle, .armed:
            return nil
        case .recording(let jobID, let startedAt):
            return KeyboardSnapshot(
                state: .recording,
                heartbeat: now,
                expiresAt: now + 60,
                recordingStartedAt: startedAt,
                loudnessDB: loudness,
                jobID: jobID
            )
        case .transcribing(let jobID):
            return KeyboardSnapshot(state: .transcribing, heartbeat: now, expiresAt: now + 60, jobID: jobID)
        case .finished(let jobID, let resultID, let transcript, let heard, let stages):
            return KeyboardSnapshot(
                state: .ready,
                heartbeat: now,
                expiresAt: now + 1,
                jobID: jobID,
                resultID: resultID,
                transcript: transcript,
                heard: heard,
                stages: stages
            )
        case .failed(let message, let until):
            guard until > now else {
                phase = .idle
                return nil
            }
            return KeyboardSnapshot(state: .failed, heartbeat: now, expiresAt: until, message: message)
        }
    }

    func markDelivered(resultID: String) {
        if case .finished(_, let current, _, _, _) = phase, current == resultID {
            phase = .idle
        }
    }

    /// Makes the capture web view and loads its page before the first dictation, so the Hex
    /// key only has the microphone left to open. Nothing here asks for the microphone or the
    /// audio session, so no orange dot shows until a dictation starts. Skipped while short of
    /// the memory a first dictation needs (it would open Hex instead) and without the
    /// microphone allowed (it would fail).
    func prepare(in host: UIView) {
        guard Self.isSupported, capture == nil, !isActive else { return }
        let headroom = os_proc_available_memory()
        guard headroom == 0 || headroom >= Self.startHeadroomNeeded else { return }
        guard Self.microphoneAllowed else { return }
        let started = Date()
        let capture = KeyboardWebCapture(host: host)
        listenWhileIdle(capture)
        self.capture = capture
        KeyboardProbeLog.shared.write("capture prepared in \(Int(Date().timeIntervalSince(started) * 1000)) ms \(KeyboardProbeLog.memory())")
    }

    /// The page between dictations: a prepared page loading, or one whose content process
    /// ends, which the next start replaces.
    private func listenWhileIdle(_ capture: KeyboardWebCapture) {
        capture.onEvent = { [weak self, weak capture] event in
            guard let self, let capture else { return }
            self.handleIdle(event, capture: capture)
        }
    }

    private func handleIdle(_ event: KeyboardWebCapture.Event, capture: KeyboardWebCapture) {
        switch event {
        case .loaded:
            let ms = Int((Date().timeIntervalSince1970 - capture.createdAt) * 1000)
            KeyboardProbeLog.shared.write("capture page loaded in \(ms) ms \(KeyboardProbeLog.memory())")
        case .processTerminated:
            if capture === self.capture { dropEndedCapture() }
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
        let ended = capture
        capture = nil
        Task { @MainActor in ended?.tearDown() }
    }

    /// The keyboard extension itself may not record (audiomxd: "it is an extension and
    /// doesn't have entitlements to record audio"), so capture runs in WebKit's media
    /// process through getUserMedia, attributed to Hex's microphone permission.
    /// With Always use iPhone microphone (the app's default), capture uses the iPhone's own
    /// microphone and mixes with other audio, so a car or headset keeps playing music
    /// instead of switching to call audio, which pauses it and stalls the start.
    /// `pressedAt` is the Hex key's touch-down (wall clock), where the start's timeline begins.
    /// A capture armed at touch-down becomes this dictation.
    func start(in host: UIView, allowBluetoothMic: Bool, pressedAt: TimeInterval? = nil) {
        if case .armed(let jobID, let startedAt) = phase {
            keepArmed(jobID: jobID, startedAt: startedAt)
            return
        }
        guard !isActive else { return }
        begin(in: host, allowBluetoothMic: allowBluetoothMic, pressedAt: pressedAt, armed: false)
    }

    /// Touch-down on the Hex key: capture starts now, unseen, so the first words aren't lost
    /// while the touch becomes a tap or a hold (`start` keeps it) or a slide or a cancelled
    /// touch (`discard` drops it, with nothing transcribed). The microphone opens here.
    func arm(in host: UIView, allowBluetoothMic: Bool, pressedAt: TimeInterval) {
        // Without the microphone, the tap's own start says so.
        guard !isActive, Self.microphoneAllowed else { return }
        guard let jobID = begin(in: host, allowBluetoothMic: allowBluetoothMic, pressedAt: pressedAt, armed: true) else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + Self.armedTimeout) { [weak self] in
            guard let self, case .armed(let current, _) = self.phase, current == jobID else { return }
            self.discard(reason: "never kept")
        }
    }

    /// The touch became a slide (the style menu) or was cancelled: the touch-down capture
    /// stops, its audio is deleted, and nothing is transcribed.
    func discard(reason: String = "slide or cancelled touch") {
        guard case .armed(_, let startedAt) = phase else { return }
        if let capture {
            listenWhileIdle(capture)
            capture.stop()
        }
        if let url = pcmFile?.url { try? FileManager.default.removeItem(at: url) }
        pcmFile = nil
        timeline = nil
        armedFailure = nil
        phase = .idle
        Self.releaseAudioSession()
        KeyboardProbeLog.shared.write("touch-down capture discarded after \(Int((Date().timeIntervalSince1970 - startedAt) * 1000)) ms (\(reason))")
    }

    private func keepArmed(jobID: String, startedAt: TimeInterval) {
        phase = .recording(jobID: jobID, startedAt: startedAt)
        KeyboardProbeLog.shared.write("touch-down capture kept after \(Int((Date().timeIntervalSince1970 - startedAt) * 1000)) ms")
        if let failure = armedFailure {
            armedFailure = nil
            fail(failure.message, detail: failure.detail)
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
    private func begin(in host: UIView, allowBluetoothMic: Bool, pressedAt: TimeInterval?, armed: Bool) -> String? {
        let log = KeyboardProbeLog.shared
        let appAllowed = KeyboardBridge().appMicrophoneAllowed
        log.write("start(web\(armed ? ", touch-down" : "")): mic=\(Self.describe(AVAudioApplication.shared.recordPermission)) app=\(appAllowed ? "granted" : "no") speech=\(Self.describe(SFSpeechRecognizer.authorizationStatus())) \(KeyboardProbeLog.memory())")
        guard Self.microphoneAllowed else {
            fail("Allow the microphone in Hex first")
            return nil
        }
        let jobID = UUID().uuidString
        let requestedAt = Date()
        activeCorrections = corrections()
        loudness = -62
        armedFailure = nil
        phase = armed
            ? .armed(jobID: jobID, startedAt: requestedAt.timeIntervalSince1970)
            : .recording(jobID: jobID, startedAt: requestedAt.timeIntervalSince1970)
        timeline = CaptureTimeline(
            pressedAt: min(pressedAt ?? requestedAt.timeIntervalSince1970, requestedAt.timeIntervalSince1970),
            requestedAt: requestedAt.timeIntervalSince1970
        )
        let capture = capture ?? KeyboardWebCapture(host: host)
        self.capture = capture
        capture.onEvent = { [weak self] event in
            self?.handle(event, jobID: jobID, requestedAt: requestedAt)
        }
        if !allowBluetoothMic {
            Self.preferBuiltInMicrophone()
        }
        capture.start(preferBuiltIn: !allowBluetoothMic)
        return jobID
    }

    /// Lets music or navigation that the capture paused resume.
    private static func releaseAudioSession() {
        try? AVAudioSession.sharedInstance().setActive(false, options: .notifyOthersOnDeactivation)
    }

    private static func preferBuiltInMicrophone() {
        let session = AVAudioSession.sharedInstance()
        do {
            try session.setCategory(
                .playAndRecord,
                mode: .measurement,
                options: [.mixWithOthers, .allowBluetoothA2DP, .defaultToSpeaker]
            )
            if let builtIn = session.availableInputs?.first(where: { $0.portType == .builtInMic }) {
                try session.setPreferredInput(builtIn)
            }
        } catch {
            KeyboardProbeLog.shared.write("built-in mic preference failed: \(describe(error))")
        }
    }

    private func handle(_ event: KeyboardWebCapture.Event, jobID: String, requestedAt: Date) {
        let log = KeyboardProbeLog.shared
        switch event {
        case .started(let sampleRate, let device, let steps):
            log.write("web capture started in \(Int(Date().timeIntervalSince(requestedAt) * 1000)) ms at \(Int(sampleRate)) Hz from \(device) \(KeyboardProbeLog.memory())")
            if let capture {
                timeline?.noteStarted(capture: capture, steps: steps, sampleRate: sampleRate)
            }
            guard isCapturing(jobID) else {
                // Done came before the microphone did: nothing to record.
                capture?.stop()
                return
            }
            do {
                let url = FileManager.default.temporaryDirectory.appendingPathComponent("keyboard-\(jobID).wav")
                pcmFile = try KeyboardPCMFile(url: url, sampleRate: sampleRate)
            } catch {
                capture?.stop()
                fail(UserFacingError.Context.microphone.fallback, detail: "Could not create audio file: \(Self.describe(error))")
            }
        case .samples(let data, let peak):
            if let line = timeline?.firstBuffer(at: Date().timeIntervalSince1970) {
                log.write("\(line) \(KeyboardProbeLog.memory())")
                timeline = nil
            }
            pcmFile?.append(data)
            loudness = peak > 0 ? max(-62, 20 * log10(peak)) : -62
        case .stopped:
            finishRecording(jobID: jobID)
        case .failed(let message):
            capture?.stop()
            pcmFile = nil
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
            pcmFile = nil
            fail(UserFacingError.Context.microphone.fallback, detail: "WebKit capture process ended")
        case .loaded:
            break
        }
    }

    func cancel() {
        transcription?.cancel()
        transcription = nil
        timeline = nil
        armedFailure = nil
        if let capture {
            listenWhileIdle(capture)
            capture.stop()
        }
        if let url = pcmFile?.url { try? FileManager.default.removeItem(at: url) }
        pcmFile = nil
        phase = .idle
        Self.releaseAudioSession()
        KeyboardProbeLog.shared.write("cancelled")
    }

    func stop() {
        guard case .recording = phase else { return }
        capture?.stop()
    }

    private func finishRecording(jobID: String) {
        guard case .recording(_, let startedAt) = phase else { return }
        guard let file = pcmFile else {
            endQuietly("stopped before the microphone started")
            return
        }
        pcmFile = nil
        Self.releaseAudioSession()
        let seconds = Date().timeIntervalSince1970 - startedAt
        let frames = file.close()
        KeyboardProbeLog.shared.write("stopped after \(String(format: "%.1f", seconds)) s, frames=\(frames) \(KeyboardProbeLog.memory())")
        guard !EmptyCapture.isTooShort(frames: frames, sampleRate: file.sampleRate) else {
            try? FileManager.default.removeItem(at: file.url)
            endQuietly("too short to hold a word")
            return
        }
        guard #available(iOS 26.0, *) else { return }
        phase = .transcribing(jobID: jobID)
        let url = file.url
        let fixes = activeCorrections
        let hints = DictationCorrections.recognitionHints(vocabulary: [], corrections: fixes)
        transcription = Task { [weak self] in
            let started = Date()
            do {
                let (heard, language) = try await KeyboardSpeech.transcribe(url, hints: hints)
                try? FileManager.default.removeItem(at: url)
                let locale = language.identifier
                let style = KeyboardBridge().style
                let vocabulary = CustomVocabularySettings().activeTerms
                // The app's steps (but Apple Intelligence's cleanup), its style, Format lists,
                // End with a period and Lowercase included.
                let pipeline = DictationPipeline(
                    style: style,
                    language: locale,
                    vocabulary: vocabulary,
                    corrections: fixes,
                    lowercase: DictationLowercaseSettings().isEnabled,
                    formatLists: DictationListSettings().isEnabled,
                    hinglish: HinglishSettings().isEnabled,
                    endsWithPeriod: DictationPeriodSettings().isEnabled
                )
                let steps = await pipeline.run(heard, translationSource: language.locale.language)
                let translated = steps.translated
                let text = steps.formatted
                KeyboardProbeLog.shared.write("transcribed in \(String(format: "%.2f", Date().timeIntervalSince(started))) s: \(text.count) chars \(KeyboardProbeLog.memory())")
                guard let self, !Task.isCancelled else { return }
                // Record typing logs keeps every step; nothing is kept while it is off.
                let stages: DictationStages? = TypingLogSettings().isEnabled
                    ? DictationStages(
                        engine: language.module == .dictation
                            ? "Apple Speech (keyboard, DictationTranscriber)"
                            : "Apple Speech (keyboard)",
                        locale: locale,
                        style: style.rawValue,
                        recordedMS: Int((seconds * 1000).rounded()),
                        transcribeMS: Int((Date().timeIntervalSince(started) * 1000).rounded()),
                        raw: heard,
                        fixed: steps.fixed,
                        spelled: vocabulary.isEmpty ? nil : steps.spelled,
                        tagged: CleanupTaggerSettings().isEnabled && style != .verbatim ? steps.tagged : nil,
                        translated: translated,
                        formatted: steps.formatted
                    )
                    : nil
                // A translation's words can't be compared with what was heard, so it isn't learned from.
                self.phase = .finished(
                    jobID: jobID,
                    resultID: UUID().uuidString,
                    transcript: text,
                    heard: translated == nil ? steps.heard : nil,
                    stages: stages
                )
            } catch {
                try? FileManager.default.removeItem(at: url)
                guard let self, !Task.isCancelled,
                      case .transcribing(let current) = self.phase, current == jobID else { return }
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
    }

    /// `message` is what the keyboard shows; `detail`, for the log only, may carry codes.
    private func fail(_ message: String, detail: String? = nil) {
        if case .armed = phase {
            // Shown only if the touch is kept.
            armedFailure = (message, detail)
            Self.releaseAudioSession()
            KeyboardProbeLog.shared.write("FAILED at touch-down (held): \(detail ?? message) \(KeyboardProbeLog.memory())")
            return
        }
        Self.releaseAudioSession()
        KeyboardProbeLog.shared.write("FAILED: \(detail ?? message) \(KeyboardProbeLog.memory())")
        phase = .failed(message: message, until: Date().timeIntervalSince1970 + 6)
    }

    /// Nothing to insert and nothing wrong: back to idle without a message.
    private func endQuietly(_ reason: String) {
        Self.releaseAudioSession()
        KeyboardProbeLog.shared.write("ended quietly: \(reason)")
        // For onboarding's practice only; the keyboard shows nothing.
        KeyboardBridge().lastEmptyDictationAt = Date().timeIntervalSince1970
        phase = .idle
    }

    private static func describe(_ error: Error) -> String {
        let ns = error as NSError
        return "\(ns.domain) \(ns.code) \(ns.localizedDescription)"
    }

    private static func describe(_ permission: AVAudioApplication.recordPermission) -> String {
        switch permission {
        case .granted: "granted"
        case .denied: "denied"
        case .undetermined: "undetermined"
        @unknown default: "unknown"
        }
    }

    private static func describe(_ status: SFSpeechRecognizerAuthorizationStatus) -> String {
        switch status {
        case .authorized: "authorized"
        case .denied: "denied"
        case .restricted: "restricted"
        case .notDetermined: "notDetermined"
        @unknown default: "unknown"
        }
    }
}

/// What the page timed while starting (wall clock): the device list, the first
/// getUserMedia, the stream kept (after a second one for the built-in microphone) and the
/// audio context running.
struct CaptureSteps {
    var listedAt: TimeInterval?
    var openedAt: TimeInterval?
    var microphoneAt: TimeInterval?
    var audioAt: TimeInterval?
}

/// One in-keyboard start, from the Hex key's touch-down (or the start call without one) to
/// the first audio, as one probe log line. Times are wall clock, so a web view or page made
/// before the touch reads negative.
@MainActor
private struct CaptureTimeline {
    let pressedAt: TimeInterval
    let requestedAt: TimeInterval
    private var stages: [(String, TimeInterval?)] = []
    private var bufferSeconds: TimeInterval = 4096.0 / 48_000

    init(pressedAt: TimeInterval, requestedAt: TimeInterval) {
        self.pressedAt = pressedAt
        self.requestedAt = requestedAt
    }

    mutating func noteStarted(capture: KeyboardWebCapture, steps: CaptureSteps, sampleRate: Double) {
        bufferSeconds = 4096 / max(8_000, sampleRate)
        stages = [
            ("start", requestedAt),
            ("web view", capture.createdAt),
            ("page", capture.loadedAt),
            ("script", capture.scriptAt),
            ("devices", steps.listedAt),
            ("getUserMedia", steps.openedAt),
            ("microphone", steps.microphoneAt),
            ("audio", steps.audioAt),
            ("started", Date().timeIntervalSince1970),
        ]
    }

    /// The line for the first buffer, which holds the 4,096 samples before it.
    func firstBuffer(at time: TimeInterval) -> String {
        func ms(_ t: TimeInterval?) -> String {
            guard let t else { return "-" }
            let value = Int(((t - pressedAt) * 1000).rounded())
            return value >= 0 ? "+\(value)" : "\(value)"
        }
        let steps = (stages + [("first buffer", time)]).map { "\($0.0) \(ms($0.1))" }.joined(separator: ", ")
        return "first audio \(ms(time - bufferSeconds)) ms after touch-down (\(steps))"
    }
}

/// A Hex-key start through a warm Hex session (Keep mic warm): from touch-down to Hex's
/// recorder running, logged once the keyboard sees it.
struct WarmStartTimeline {
    let jobID: String
    let pressedAt: TimeInterval
    let sentAt: TimeInterval

    /// Logs and returns true once the snapshot shows this job recording, or gives up after 10 s.
    func observe(_ snapshot: KeyboardSnapshot, now: TimeInterval) -> Bool {
        if snapshot.state == .recording, snapshot.jobID == jobID {
            func ms(_ t: TimeInterval?) -> String { t.map { "+\(Int((($0 - pressedAt) * 1000).rounded()))" } ?? "-" }
            KeyboardProbeLog.shared.write(
                "warm start: recorder running \(ms(snapshot.recordingStartedAt)) ms after touch-down (sent \(ms(sentAt)), seen \(ms(now)))"
            )
            return true
        }
        return now - sentAt > 10
    }
}

/// 16-bit mono PCM chunks from the page, written as a WAV file.
@MainActor
private final class KeyboardPCMFile {
    let url: URL
    private var file: AVAudioFile?
    private let format: AVAudioFormat
    private var frames: AVAudioFramePosition = 0

    init(url: URL, sampleRate: Double) throws {
        self.url = url
        guard let format = AVAudioFormat(commonFormat: .pcmFormatInt16, sampleRate: sampleRate, channels: 1, interleaved: true) else {
            throw CocoaError(.featureUnsupported)
        }
        self.format = format
        file = try AVAudioFile(forWriting: url, settings: format.settings, commonFormat: .pcmFormatInt16, interleaved: true)
    }

    func append(_ data: Data) {
        let count = data.count / 2
        guard count > 0, let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(count)) else { return }
        buffer.frameLength = AVAudioFrameCount(count)
        data.withUnsafeBytes { raw in
            guard let base = raw.baseAddress, let target = buffer.int16ChannelData?[0] else { return }
            memcpy(target, base, count * 2)
        }
        try? file?.write(from: buffer)
        frames += AVAudioFramePosition(count)
    }

    var sampleRate: Double { format.sampleRate }

    func close() -> AVAudioFramePosition {
        file = nil
        return frames
    }
}

@MainActor
private final class KeyboardWebCapture: NSObject, WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler {
    enum Event {
        case started(sampleRate: Double, device: String, steps: CaptureSteps)
        case samples(Data, peak: Float)
        case stopped
        case loaded
        case failed(String)
        case processTerminated
    }

    var onEvent: ((Event) -> Void)?
    private let webView: WKWebView
    private var isLoaded = false
    private var startWhenLoaded = false
    /// When the web view was made, its page loaded and the page last asked to start (wall clock).
    let createdAt = Date().timeIntervalSince1970
    private(set) var loadedAt: TimeInterval?
    private(set) var scriptAt: TimeInterval?

    init(host: UIView) {
        let configuration = WKWebViewConfiguration()
        configuration.allowsInlineMediaPlayback = true
        configuration.mediaTypesRequiringUserActionForPlayback = []
        webView = WKWebView(frame: CGRect(x: 0, y: 0, width: 2, height: 2), configuration: configuration)
        super.init()
        configuration.userContentController.add(WeakMessageHandler(self), name: "hex")
        webView.navigationDelegate = self
        webView.uiDelegate = self
        webView.isUserInteractionEnabled = false
        webView.alpha = 0.02
        host.addSubview(webView)
        webView.loadHTMLString(Self.page, baseURL: URL(string: "https://keyboard.hex.local/"))
    }

    private var preferBuiltIn = true

    func start(preferBuiltIn: Bool) {
        self.preferBuiltIn = preferBuiltIn
        guard isLoaded else {
            startWhenLoaded = true
            return
        }
        scriptAt = Date().timeIntervalSince1970
        webView.evaluateJavaScript("hexStart(\(preferBuiltIn))")
    }

    func stop() {
        startWhenLoaded = false
        webView.evaluateJavaScript("hexStop()")
    }

    /// Takes the web view out of the keyboard, after its content process ended, so it doesn't
    /// stay behind next to the new one the next dictation makes.
    func tearDown() {
        onEvent = nil
        startWhenLoaded = false
        webView.stopLoading()
        webView.navigationDelegate = nil
        webView.uiDelegate = nil
        webView.configuration.userContentController.removeScriptMessageHandler(forName: "hex")
        webView.removeFromSuperview()
    }

    func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
        isLoaded = true
        loadedAt = Date().timeIntervalSince1970
        onEvent?(.loaded)
        if startWhenLoaded {
            startWhenLoaded = false
            start(preferBuiltIn: preferBuiltIn)
        }
    }

    func webView(
        _ webView: WKWebView,
        requestMediaCapturePermissionFor origin: WKSecurityOrigin,
        initiatedByFrame frame: WKFrameInfo,
        type: WKMediaCaptureType,
        decisionHandler: @escaping @MainActor (WKPermissionDecision) -> Void
    ) {
        decisionHandler(type == .microphone ? .grant : .deny)
    }

    func webViewWebContentProcessDidTerminate(_ webView: WKWebView) {
        isLoaded = false
        onEvent?(.processTerminated)
    }

    func userContentController(_ userContentController: WKUserContentController, didReceive message: WKScriptMessage) {
        guard let body = message.body as? [String: Any], let type = body["type"] as? String else { return }
        switch type {
        case "started":
            onEvent?(.started(
                sampleRate: (body["rate"] as? Double) ?? 48_000,
                device: (body["device"] as? String) ?? "unknown",
                steps: CaptureSteps(
                    listedAt: (body["listed"] as? Double).map { $0 / 1000 },
                    openedAt: (body["opened"] as? Double).map { $0 / 1000 },
                    microphoneAt: (body["mic"] as? Double).map { $0 / 1000 },
                    audioAt: (body["audio"] as? Double).map { $0 / 1000 }
                )
            ))
        case "pcm":
            guard let encoded = body["data"] as? String, let data = Data(base64Encoded: encoded) else { return }
            onEvent?(.samples(data, peak: Float((body["peak"] as? Double) ?? 0)))
        case "stopped":
            onEvent?(.stopped)
        case "error":
            onEvent?(.failed((body["message"] as? String) ?? "unknown"))
        default:
            break
        }
    }

    private static let page = """
    <!doctype html><meta charset="utf-8"><script>
    let ctx, stream, source, node, running = false, generation = 0;
    const post = (m) => webkit.messageHandlers.hex.postMessage(m);
    const release = (s, c) => {
      if (s) s.getTracks().forEach((t) => t.stop());
      if (c) c.close().catch(() => {});
    };
    const base = { channelCount: 1, echoCancellation: false, noiseSuppression: false, autoGainControl: false };
    async function builtInMicrophone() {
      const inputs = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === "audioinput");
      const found = inputs.find((d) => /iphone|ipad|built-in/i.test(d.label));
      return found ? found.deviceId : null;
    }
    async function hexStart(preferBuiltIn) {
      if (running) return;
      running = true;
      // A hexStop while this start waits (a slide off the Hex key right after touch-down)
      // makes it stale: it releases what it opened instead of going on.
      const start = ++generation;
      const stale = () => start !== generation;
      let s = null, c = null;
      try {
        let id = preferBuiltIn ? await builtInMicrophone() : null;
        if (stale()) return;
        const listed = Date.now();
        s = await navigator.mediaDevices.getUserMedia({ audio: id ? { ...base, deviceId: { exact: id } } : base });
        if (stale()) return release(s);
        const opened = Date.now();
        if (preferBuiltIn && !id) {
          id = await builtInMicrophone();
          if (stale()) return release(s);
          if (id && s.getAudioTracks()[0].getSettings().deviceId !== id) {
            s.getTracks().forEach((t) => t.stop());
            s = await navigator.mediaDevices.getUserMedia({ audio: { ...base, deviceId: { exact: id } } });
            if (stale()) return release(s);
          }
        }
        const mic = Date.now();
        c = new AudioContext();
        await c.resume();
        if (stale()) return release(s, c);
        stream = s;
        ctx = c;
        source = ctx.createMediaStreamSource(stream);
        node = ctx.createScriptProcessor(4096, 1, 1);
        node.onaudioprocess = (event) => {
          if (!running) return;
          const input = event.inputBuffer.getChannelData(0);
          const pcm = new Int16Array(input.length);
          let peak = 0;
          for (let i = 0; i < input.length; i++) {
            const s = Math.max(-1, Math.min(1, input[i]));
            peak = Math.max(peak, Math.abs(s));
            pcm[i] = s < 0 ? s * 0x8000 : s * 0x7fff;
          }
          const bytes = new Uint8Array(pcm.buffer);
          let binary = "";
          for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
          post({ type: "pcm", data: btoa(binary), peak });
        };
        source.connect(node);
        node.connect(ctx.destination);
        post({ type: "started", rate: ctx.sampleRate, device: stream.getAudioTracks()[0].label, listed, opened, mic, audio: Date.now() });
      } catch (error) {
        if (stale()) return release(s, c);
        running = false;
        // What it opened before taking it on; hexStop releases what it took.
        if (s !== stream) release(s, c);
        post({ type: "error", message: (error && (error.name + ": " + error.message)) || String(error) });
      }
    }
    async function hexStop() {
      generation++;
      const wasRunning = running;
      running = false;
      try { node && node.disconnect(); source && source.disconnect(); } catch (e) {}
      if (stream) stream.getTracks().forEach((t) => t.stop());
      if (ctx) { try { await ctx.close(); } catch (e) {} }
      ctx = stream = source = node = null;
      if (wasRunning) post({ type: "stopped" });
    }
    </script>
    """
}

private final class WeakMessageHandler: NSObject, WKScriptMessageHandler {
    weak var target: WKScriptMessageHandler?

    init(_ target: WKScriptMessageHandler) {
        self.target = target
    }

    func userContentController(_ controller: WKUserContentController, didReceive message: WKScriptMessage) {
        target?.userContentController(controller, didReceive: message)
    }
}

@available(iOS 26.0, *)
private enum KeyboardSpeech {
    enum Failure: HexDescribedError, NoSpeechError {
        case noLocale
        case empty

        var errorDescription: String? {
            switch self {
            case .noLocale: "Apple Speech doesn't have your language on this iPhone."
            case .empty: "No speech detected"
            }
        }

        var meansNoSpeech: Bool {
            if case .empty = self { return true }
            return false
        }
    }

    /// The transcript and the language it was transcribed in.
    static func transcribe(_ url: URL, hints: [String]) async throws -> (String, SpeechLanguage) {
        let log = KeyboardProbeLog.shared
        // The same rule as the app's (Settings › Dictation language, else Automatic), so the
        // keyboard uses the assets Hex downloaded.
        let settings = LanguageSettings()
        guard let picked = await AppleSpeechLanguages.resolve(settings: settings) else {
            // Only Speech listing languages, none of them the iPhone's, means it lacks yours.
            guard await AppleSpeechLanguages.listsNone() else { throw Failure.noLocale }
            log.write("speech lists no languages in the keyboard: handing over to Hex")
            throw SpeechModelNeedsHex.handOff(settings.effectiveDictationLanguage ?? "en-US", settings: settings)
        }
        log.write("speech locale=\(picked.identifier) module=\(picked.module.rawValue) available=\(SpeechTranscriber.isAvailable)")
        // The model the app uses, else the same language on `DictationTranscriber`. One the
        // keyboard can't get (released since Hex downloaded it, or a download that failed here)
        // is Hex's to download: the Hex key opens Hex until it installs.
        let started = Date()
        let language: SpeechLanguage
        do {
            language = try await AppleSpeechAssets.install(picked, settings: settings, log: { log.write($0) })
        } catch {
            throw SpeechModelNeedsHex.keyboardError(for: error, language: picked.identifier, settings: settings)
        }
        if language != picked || Date().timeIntervalSince(started) > 0.5 {
            log.write("speech assets ready (\(language.module.rawValue) \(language.identifier)) after \(String(format: "%.1f", Date().timeIntervalSince(started))) s \(KeyboardProbeLog.memory())")
        }
        let speech = AppleSpeechModule(language)

        let analyzer = SpeechAnalyzer(
            modules: [speech.module],
            options: SpeechAnalyzer.Options(priority: .userInitiated, modelRetention: .whileInUse)
        )
        let file = try AVAudioFile(forReading: url)
        // Learn how I type's meant words, best effort, as the app does for custom vocabulary.
        if !hints.isEmpty {
            let context = AnalysisContext()
            context.contextualStrings[.general] = hints
            try? await analyzer.setContext(context)
        }
        try await analyzer.start(inputAudioFile: file, finishAfterFile: true)
        log.write("analyzer started \(KeyboardProbeLog.memory())")

        let text = try await speech.finalText()
        guard !text.isEmpty else { throw Failure.empty }
        return (text, language)
    }
}

final class KeyboardProbeLog: @unchecked Sendable {
    static let shared = KeyboardProbeLog()

    private let logger = Logger(subsystem: "anomaly.hex.mobile.keyboard", category: "capture-probe")
    private let lock = NSLock()
    private let url = FileManager.default
        .containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
        .appendingPathComponent("keyboard-capture-probe.log")

    /// Past this size the log keeps only its newest `trimmedBytes`, so it never grows
    /// without limit in the App Group.
    static let maximumBytes: UInt64 = 512 * 1024
    static let trimmedBytes = 256 * 1024

    func write(_ message: String) {
        logger.notice("\(message, privacy: .public)")
        guard let url else { return }
        let line = "\(ISO8601DateFormatter().string(from: Date())) \(message)\n"
        lock.withLock {
            if let handle = try? FileHandle(forWritingTo: url) {
                let size = handle.seekToEndOfFile()
                handle.write(Data(line.utf8))
                try? handle.close()
                if size + UInt64(line.utf8.count) > Self.maximumBytes {
                    Self.trim(url)
                }
            } else {
                try? Data(line.utf8).write(to: url)
            }
        }
    }

    /// Keeps the newest `trimmedBytes`, from the first whole line in them.
    static func trim(_ url: URL, keeping bytes: Int = trimmedBytes) {
        guard let data = try? Data(contentsOf: url), data.count > bytes else { return }
        var tail = data.suffix(bytes)
        if let newline = tail.firstIndex(of: UInt8(ascii: "\n")) {
            tail = tail[tail.index(after: newline)...]
        }
        try? Data(tail).write(to: url, options: .atomic)
    }

    static func memory() -> String {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        let footprint = result == KERN_SUCCESS ? Double(info.phys_footprint) / 1_048_576 : -1
        // The highest footprint this keyboard process has reached, for what iOS's limit saw.
        let peak = result == KERN_SUCCESS ? Double(info.ledger_phys_footprint_peak) / 1_048_576 : -1
        let available = Double(os_proc_available_memory()) / 1_048_576
        return String(format: "[footprint %.1f MB, peak %.1f MB, headroom %.1f MB]", footprint, peak, available)
    }
}
