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
        case .recording, .transcribing: true
        default: false
        }
    }

    func snapshot(now: TimeInterval) -> KeyboardSnapshot? {
        switch phase {
        case .idle:
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

    /// The keyboard extension itself may not record (audiomxd: "it is an extension and
    /// doesn't have entitlements to record audio"), so capture runs in WebKit's media
    /// process through getUserMedia, attributed to Hex's microphone permission.
    /// With Always use iPhone microphone (the app's default), capture uses the iPhone's own
    /// microphone and mixes with other audio, so a car or headset keeps playing music
    /// instead of switching to call audio, which pauses it and stalls the start.
    func start(in host: UIView, allowBluetoothMic: Bool) {
        guard !isActive else { return }
        let log = KeyboardProbeLog.shared
        let appAllowed = KeyboardBridge().appMicrophoneAllowed
        log.write("start(web): mic=\(Self.describe(AVAudioApplication.shared.recordPermission)) app=\(appAllowed ? "granted" : "no") speech=\(Self.describe(SFSpeechRecognizer.authorizationStatus())) \(KeyboardProbeLog.memory())")
        // This process keeps the permission it read when it started, so a keyboard opened
        // before Hex was allowed the microphone (onboarding's practice field) still reads it as
        // not allowed: Hex's own reading counts too. WebKit still checks (NotAllowedError).
        guard AVAudioApplication.shared.recordPermission == .granted || appAllowed else {
            fail("Allow the microphone in Hex first")
            return
        }
        let jobID = UUID().uuidString
        let requestedAt = Date()
        activeCorrections = corrections()
        loudness = -62
        phase = .recording(jobID: jobID, startedAt: requestedAt.timeIntervalSince1970)
        let capture = capture ?? KeyboardWebCapture(host: host)
        self.capture = capture
        capture.onEvent = { [weak self] event in
            self?.handle(event, jobID: jobID, requestedAt: requestedAt)
        }
        if !allowBluetoothMic {
            Self.preferBuiltInMicrophone()
        }
        capture.start(preferBuiltIn: !allowBluetoothMic)
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
        case .started(let sampleRate, let device):
            log.write("web capture started in \(Int(Date().timeIntervalSince(requestedAt) * 1000)) ms at \(Int(sampleRate)) Hz from \(device) \(KeyboardProbeLog.memory())")
            guard case .recording(let current, _) = phase, current == jobID else {
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
            pcmFile = nil
            // The next start needs a new web view; this one leaves the keyboard's view too,
            // once WebKit's callback has returned, instead of staying behind with its page.
            let ended = capture
            capture = nil
            Task { @MainActor in ended?.tearDown() }
            fail(UserFacingError.Context.microphone.fallback, detail: "WebKit capture process ended")
        }
    }

    func cancel() {
        transcription?.cancel()
        transcription = nil
        capture?.onEvent = nil
        capture?.stop()
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
                let corrected = DictationCorrections.apply(fixes, to: heard)
                let style = KeyboardBridge().style
                let cleaned = await CleanupTagger.cleanIfEnabled(corrected, style: style, language: locale)
                let translated = await DictationTranslator.translateIfEnabled(
                    cleaned,
                    source: language.locale.language
                )
                let text = translated ?? cleaned
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
                        fixed: corrected,
                        tagged: CleanupTaggerSettings().isEnabled && style != .verbatim ? cleaned : nil,
                        translated: translated
                    )
                    : nil
                // A translation's words can't be compared with what was heard, so it isn't learned from.
                self.phase = .finished(
                    jobID: jobID,
                    resultID: UUID().uuidString,
                    transcript: text,
                    heard: translated == nil ? heard : nil,
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
        Self.releaseAudioSession()
        KeyboardProbeLog.shared.write("FAILED: \(detail ?? message) \(KeyboardProbeLog.memory())")
        phase = .failed(message: message, until: Date().timeIntervalSince1970 + 6)
    }

    /// Nothing to insert and nothing wrong: back to idle without a message.
    private func endQuietly(_ reason: String) {
        Self.releaseAudioSession()
        KeyboardProbeLog.shared.write("ended quietly: \(reason)")
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
        case started(sampleRate: Double, device: String)
        case samples(Data, peak: Float)
        case stopped
        case failed(String)
        case processTerminated
    }

    var onEvent: ((Event) -> Void)?
    private let webView: WKWebView
    private var isLoaded = false
    private var startWhenLoaded = false

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
            onEvent?(.started(sampleRate: (body["rate"] as? Double) ?? 48_000, device: (body["device"] as? String) ?? "unknown"))
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
    let ctx, stream, source, node, running = false;
    const post = (m) => webkit.messageHandlers.hex.postMessage(m);
    const base = { channelCount: 1, echoCancellation: false, noiseSuppression: false, autoGainControl: false };
    async function builtInMicrophone() {
      const inputs = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === "audioinput");
      const found = inputs.find((d) => /iphone|ipad|built-in/i.test(d.label));
      return found ? found.deviceId : null;
    }
    async function hexStart(preferBuiltIn) {
      if (running) return;
      running = true;
      try {
        let id = preferBuiltIn ? await builtInMicrophone() : null;
        stream = await navigator.mediaDevices.getUserMedia({ audio: id ? { ...base, deviceId: { exact: id } } : base });
        if (preferBuiltIn && !id) {
          id = await builtInMicrophone();
          if (id && stream.getAudioTracks()[0].getSettings().deviceId !== id) {
            stream.getTracks().forEach((t) => t.stop());
            stream = await navigator.mediaDevices.getUserMedia({ audio: { ...base, deviceId: { exact: id } } });
          }
        }
        ctx = new AudioContext();
        await ctx.resume();
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
        post({ type: "started", rate: ctx.sampleRate, device: stream.getAudioTracks()[0].label });
      } catch (error) {
        running = false;
        post({ type: "error", message: (error && (error.name + ": " + error.message)) || String(error) });
      }
    }
    async function hexStop() {
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
        guard let picked = await AppleSpeechLanguages.resolve() else { throw Failure.noLocale }
        log.write("speech locale=\(picked.identifier) module=\(picked.module.rawValue) available=\(SpeechTranscriber.isAvailable)")
        // The model the app uses, else the same language on `DictationTranscriber`; when neither
        // installs, `SpeechModelUnavailable` says so in a sentence (never Apple's asset error).
        let started = Date()
        let language = try await AppleSpeechAssets.install(picked, log: { log.write($0) })
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
