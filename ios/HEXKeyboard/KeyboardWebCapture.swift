import AVFAudio
import Foundation
import UIKit
import WebKit

/// What the page timed while starting (wall clock): the device list, the first
/// getUserMedia, the stream kept (after a second one for the chosen microphone) and the
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
struct CaptureTimeline {
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
final class KeyboardPCMFile {
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
final class KeyboardWebCapture: NSObject, WKNavigationDelegate, WKUIDelegate, WKScriptMessageHandler {
    enum Event {
        case started(sampleRate: Double, device: String, steps: CaptureSteps)
        case samples(Data, peak: Float)
        /// The page's audio context or microphone track changed (`KeyboardCapturePageState`).
        case state(KeyboardCapturePageState)
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

    private var microphone: String?
    private var builtInFallback = true
    private var voiceIsolation = false

    /// Records from the input labelled `microphone` (`KeyboardLocalDictation.microphone(for:)`),
    /// else, with `builtInFallback`, the iPhone's own microphone, else WebKit's default.
    /// With `voiceIsolation` (Labs › Voice isolation) the page asks WebKit for echo
    /// cancellation and noise suppression, as iOS's voice processing does; off it asks for
    /// neither, exactly as before.
    func start(microphone: String?, builtInFallback: Bool, voiceIsolation: Bool) {
        self.microphone = microphone
        self.builtInFallback = builtInFallback
        self.voiceIsolation = voiceIsolation
        guard isLoaded else {
            startWhenLoaded = true
            return
        }
        scriptAt = Date().timeIntervalSince1970
        let label = microphone
            .flatMap { try? JSONEncoder().encode($0) }
            .flatMap { String(data: $0, encoding: .utf8) } ?? "null"
        webView.evaluateJavaScript("hexStart(\(label), \(builtInFallback), \(voiceIsolation))")
    }

    func attach(to host: UIView) {
        if webView.superview !== host {
            host.addSubview(webView)
        }
    }

    func detach() {
        startWhenLoaded = false
        webView.removeFromSuperview()
    }

    func stop() {
        startWhenLoaded = false
        webView.evaluateJavaScript("hexStop()")
    }

    #if targetEnvironment(simulator)
    func evaluate(_ script: String) {
        webView.evaluateJavaScript(script)
    }
    #endif

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
            start(
                microphone: microphone,
                builtInFallback: builtInFallback,
                voiceIsolation: voiceIsolation
            )
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
        case "state":
            onEvent?(.state(KeyboardCapturePageState(
                cause: (body["cause"] as? String) ?? "unknown",
                context: (body["context"] as? String) ?? "unknown",
                muted: (body["muted"] as? Bool) ?? false,
                ended: (body["ended"] as? Bool) ?? false,
                at: ((body["at"] as? Double) ?? 0) / 1000
            )))
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
    let ctx, stream, source, node, running = false, generation = 0, revive = null;
    // A stop waiting for the buffer the processor still holds (`hexStop`), and its end, which a
    // new start waits for.
    let flushed = null, stopping = null;
    // For opening the microphone again (`heal`): the capture's constraints, when its track went
    // muted, the next try and its back-off, and the state the keyboard last heard (`report`).
    let constraints = null, mutedAt = 0, reopening = false, nextReopen = 0, backoff = 1000, reported = "";
    const post = (m) => webkit.messageHandlers.hex.postMessage(m);
    const release = (s, c) => {
      if (s) s.getTracks().forEach((t) => t.stop());
      if (c) c.close().catch(() => {});
    };
    const base = { channelCount: 1, echoCancellation: false, noiseSuppression: false, autoGainControl: false };
    // Labs › Voice isolation: WebKit's own voice processing, like iOS's voice modes.
    const isolated = { channelCount: 1, echoCancellation: true, noiseSuppression: true, autoGainControl: false };
    // The input named `label`, else (with `builtIn`) the iPhone's own. Labels stay empty
    // until this page has had the microphone once.
    async function chosenMicrophone(label, builtIn) {
      const inputs = (await navigator.mediaDevices.enumerateDevices()).filter((d) => d.kind === "audioinput");
      const found = (label && inputs.find((d) => d.label === label))
        || (builtIn && inputs.find((d) => /iphone|ipad|built-in/i.test(d.label)));
      return found ? found.deviceId : null;
    }
    const track = () => stream && stream.getAudioTracks()[0];
    // Tells the keyboard the context's state and the track's whenever either changes, so it
    // doesn't take an interrupted capture's silence for the end of the dictation.
    function report(cause) {
      if (!running || !ctx) return;
      const t = track();
      const state = { context: ctx.state, muted: !!(t && t.muted), ended: !t || t.readyState === "ended" };
      const key = state.context + state.muted + state.ended;
      if (key === reported && cause === "check") return;
      reported = key;
      post({ type: "state", cause, ...state, at: Date.now() });
    }
    function listen(t) {
      t.onmute = () => { if (t === track()) mutedAt = Date.now(); report("mute"); };
      t.onunmute = () => report("unmute");
      t.onended = () => report("ended");
    }
    // iOS gives the audio session to other sounds (a notification, an alarm, a call): WebKit
    // interrupts or suspends the context and mutes the track, may end it, and may leave either
    // so once the sound is over; the page then sends nothing. Every 250 ms: resume a context
    // that isn't running, then open the microphone again for a track that ended or stayed muted
    // 3 s, trying again after 1, 2, 4, then every 8 s. The same dictation carries on.
    function heal() {
      if (!running || !ctx) return;
      report("check");
      if (ctx.state !== "running") {
        if (ctx.state !== "closed") ctx.resume().catch(() => {});
        return;
      }
      const t = track();
      const ended = !t || t.readyState === "ended";
      if (!ended && !(t.muted && Date.now() - mutedAt >= 3000)) return;
      if (reopening || Date.now() < nextReopen) return;
      reopen(ended ? "ended" : "muted");
    }
    async function reopen(why) {
      reopening = true;
      const start = generation;
      report("opening the microphone again, track " + why);
      try {
        const s = await navigator.mediaDevices.getUserMedia(constraints);
        if (start !== generation || !running) return release(s);
        const old = stream;
        try { source.disconnect(); } catch (e) {}
        stream = s;
        source = ctx.createMediaStreamSource(s);
        source.connect(node);
        listen(track());
        mutedAt = Date.now();
        if (!track().muted) backoff = 1000;
        release(old);
        report("microphone opened again");
      } catch (error) {
        report("opening the microphone again failed: " + ((error && error.name) || String(error)));
      } finally {
        reopening = false;
        nextReopen = Date.now() + backoff;
        backoff = Math.min(8000, backoff * 2);
      }
    }
    async function hexStart(label, builtIn, voiceIsolation) {
      while (stopping) await stopping;
      if (running) return;
      running = true;
      // A hexStop while this start waits (a slide off the Hex key right after touch-down)
      // makes it stale: it releases what it opened instead of going on.
      const start = ++generation;
      const stale = () => start !== generation;
      const audio = voiceIsolation ? isolated : base;
      let s = null, c = null;
      try {
        const choosing = !!label || builtIn;
        let id = choosing ? await chosenMicrophone(label, builtIn) : null;
        if (stale()) return;
        const listed = Date.now();
        s = await navigator.mediaDevices.getUserMedia({ audio: id ? { ...audio, deviceId: { exact: id } } : audio });
        if (stale()) return release(s);
        const opened = Date.now();
        if (choosing && !id) {
          id = await chosenMicrophone(label, builtIn);
          if (stale()) return release(s);
          if (id && s.getAudioTracks()[0].getSettings().deviceId !== id) {
            s.getTracks().forEach((t) => t.stop());
            s = await navigator.mediaDevices.getUserMedia({ audio: { ...audio, deviceId: { exact: id } } });
            if (stale()) return release(s);
          }
        }
        const mic = Date.now();
        c = new AudioContext();
        await c.resume();
        if (stale()) return release(s, c);
        stream = s;
        ctx = c;
        constraints = { audio: id ? { ...audio, deviceId: { ideal: id } } : audio };
        mutedAt = Date.now();
        listen(track());
        c.onstatechange = () => report("statechange");
        source = ctx.createMediaStreamSource(stream);
        node = ctx.createScriptProcessor(4096, 1, 1);
        node.onaudioprocess = (event) => {
          if (!running && !flushed) return;
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
          if (flushed) flushed();
        };
        source.connect(node);
        node.connect(ctx.destination);
        revive = setInterval(heal, 250);
        post({ type: "started", rate: ctx.sampleRate, device: stream.getAudioTracks()[0].label, listed, opened, mic, audio: Date.now() });
      } catch (error) {
        if (stale()) return release(s, c);
        running = false;
        // What it opened before taking it on; hexStop releases what it took.
        if (s !== stream) release(s, c);
        post({ type: "error", message: (error && (error.name + ": " + error.message)) || String(error) });
      }
    }
    // The processor hands audio over 4,096 samples at a time (85 ms at 48 kHz), so at a stop it
    // still holds the end of what was said before the tap: the stop waits for that buffer (at
    // most 250 ms, for a context that went quiet) before it lets the microphone go. A second
    // stop meanwhile (the keyboard hiding) stops at once.
    async function hexStop() {
      generation++;
      const wasRunning = running;
      running = false;
      if (revive) { clearInterval(revive); revive = null; }
      if (flushed) flushed();
      let done = () => {};
      if (wasRunning) stopping = new Promise((resolve) => (done = resolve));
      if (wasRunning && node && ctx && ctx.state === "running") {
        await new Promise((resolve) => {
          const timer = setTimeout(() => flushed && flushed(), 250);
          flushed = () => { clearTimeout(timer); flushed = null; resolve(); };
        });
      }
      try { node && node.disconnect(); source && source.disconnect(); } catch (e) {}
      if (stream) stream.getTracks().forEach((t) => t.stop());
      if (ctx) { try { await ctx.close(); } catch (e) {} }
      ctx = stream = source = node = constraints = null;
      reopening = false;
      nextReopen = 0;
      backoff = 1000;
      reported = "";
      if (wasRunning) post({ type: "stopped" });
      if (wasRunning) stopping = null;
      done();
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
