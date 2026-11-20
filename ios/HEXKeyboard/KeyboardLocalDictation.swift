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
        log.write("start(web): mic=\(Self.describe(AVAudioApplication.shared.recordPermission)) speech=\(Self.describe(SFSpeechRecognizer.authorizationStatus())) \(KeyboardProbeLog.memory())")
        guard AVAudioApplication.shared.recordPermission == .granted else {
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
            do {
                let url = FileManager.default.temporaryDirectory.appendingPathComponent("keyboard-\(jobID).wav")
                pcmFile = try KeyboardPCMFile(url: url, sampleRate: sampleRate)
            } catch {
                capture?.stop()
                fail("Could not create audio file: \(Self.describe(error))")
            }
        case .samples(let data, let peak):
            pcmFile?.append(data)
            loudness = peak > 0 ? max(-62, 20 * log10(peak)) : -62
        case .stopped:
            finishRecording(jobID: jobID)
        case .failed(let message):
            capture?.stop()
            pcmFile = nil
            fail("Web capture: \(message)")
        case .processTerminated:
            pcmFile = nil
            capture = nil
            fail("WebKit capture process ended")
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
        guard case .recording(_, let startedAt) = phase, let file = pcmFile else { return }
        pcmFile = nil
        Self.releaseAudioSession()
        let seconds = Date().timeIntervalSince1970 - startedAt
        let frames = file.close()
        KeyboardProbeLog.shared.write("stopped after \(String(format: "%.1f", seconds)) s, frames=\(frames) \(KeyboardProbeLog.memory())")
        guard #available(iOS 26.0, *) else { return }
        phase = .transcribing(jobID: jobID)
        let url = file.url
        let fixes = activeCorrections
        let hints = DictationCorrections.recognitionHints(vocabulary: [], corrections: fixes)
        transcription = Task { [weak self] in
            let started = Date()
            do {
                let (heard, locale) = try await KeyboardSpeech.transcribe(url, hints: hints)
                try? FileManager.default.removeItem(at: url)
                let corrected = DictationCorrections.apply(fixes, to: heard)
                let style = KeyboardBridge().style
                let cleaned = await CleanupTagger.cleanIfEnabled(corrected, style: style)
                let translated = await DictationTranslator.translateIfEnabled(cleaned)
                let text = translated ?? cleaned
                KeyboardProbeLog.shared.write("transcribed in \(String(format: "%.2f", Date().timeIntervalSince(started))) s: \(text.count) chars \(KeyboardProbeLog.memory())")
                guard let self, !Task.isCancelled else { return }
                // Record typing logs keeps every step; nothing is kept while it is off.
                let stages: DictationStages? = TypingLogSettings().isEnabled
                    ? DictationStages(
                        engine: "Apple Speech (keyboard)",
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
                guard let self, !Task.isCancelled else { return }
                self.fail("Transcription failed: \(Self.describe(error))")
            }
        }
    }

    private func fail(_ message: String) {
        Self.releaseAudioSession()
        KeyboardProbeLog.shared.write("FAILED: \(message) \(KeyboardProbeLog.memory())")
        phase = .failed(message: message, until: Date().timeIntervalSince1970 + 6)
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
    enum Failure: LocalizedError {
        case noLocale
        case assetsMissing(String)
        case empty

        var errorDescription: String? {
            switch self {
            case .noLocale: "No Apple Speech locale"
            case .assetsMissing(let status): "Apple Speech assets not installed (\(status))"
            case .empty: "No speech detected"
            }
        }
    }

    /// The transcript and the speech locale's identifier.
    static func transcribe(_ url: URL, hints: [String]) async throws -> (String, String) {
        let log = KeyboardProbeLog.shared
        // Same order as the app's AppleSpeechTranscriber.preferredLocale(), so the
        // keyboard uses the assets Hex downloaded.
        var locale: Locale?
        for identifier in Locale.preferredLanguages + [Locale.current.identifier, "en-US"] {
            if let supported = await SpeechTranscriber.supportedLocale(equivalentTo: Locale(identifier: identifier)) {
                locale = supported
                break
            }
        }
        guard let locale else { throw Failure.noLocale }
        let transcriber = SpeechTranscriber(locale: locale, preset: .transcription)
        let status = await AssetInventory.status(forModules: [transcriber])
        log.write("speech locale=\(locale.identifier) assets=\(status) available=\(SpeechTranscriber.isAvailable)")
        if status != .installed {
            let started = Date()
            _ = try? await AssetInventory.reserve(locale: locale)
            if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) {
                log.write("downloading speech assets")
                try await request.downloadAndInstall()
            }
            let after = await AssetInventory.status(forModules: [transcriber])
            log.write("speech assets \(after) after \(String(format: "%.1f", Date().timeIntervalSince(started))) s \(KeyboardProbeLog.memory())")
            guard after == .installed else { throw Failure.assetsMissing("\(after)") }
        }

        let analyzer = SpeechAnalyzer(
            modules: [transcriber],
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

        var transcript = AttributedString()
        for try await result in transcriber.results where result.isFinal {
            transcript += result.text
        }
        let text = String(transcript.characters).trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { throw Failure.empty }
        return (text, locale.identifier)
    }
}

final class KeyboardProbeLog: @unchecked Sendable {
    static let shared = KeyboardProbeLog()

    private let logger = Logger(subsystem: "anomaly.hex.mobile.keyboard", category: "capture-probe")
    private let lock = NSLock()
    private let url = FileManager.default
        .containerURL(forSecurityApplicationGroupIdentifier: KeyboardBridge.appGroup)?
        .appendingPathComponent("keyboard-capture-probe.log")

    func write(_ message: String) {
        logger.notice("\(message, privacy: .public)")
        guard let url else { return }
        let line = "\(ISO8601DateFormatter().string(from: Date())) \(message)\n"
        lock.withLock {
            if let handle = try? FileHandle(forWritingTo: url) {
                handle.seekToEndOfFile()
                handle.write(Data(line.utf8))
                try? handle.close()
            } else {
                try? Data(line.utf8).write(to: url)
            }
        }
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
        let available = Double(os_proc_available_memory()) / 1_048_576
        return String(format: "[footprint %.1f MB, headroom %.1f MB]", footprint, available)
    }
}
