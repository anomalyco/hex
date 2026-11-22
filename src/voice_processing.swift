import AVFAudio
import AudioToolbox
import CoreAudio
import Foundation

public typealias VoiceProcessingCallback = @convention(c) (
    UnsafeMutableRawPointer?,
    UnsafePointer<Float>?,
    Int,
    UInt64,
    UInt32
) -> Void

/// An AVAudioEngine configuration change, reported on the same gate as the tap.
public typealias VoiceProcessingErrorCallback = @convention(c) (UnsafeMutableRawPointer?) -> Void

private struct VoiceProcessingError: LocalizedError {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var errorDescription: String? { message }
}

private func writeVoiceProcessingError(
    _ message: String,
    to destination: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) {
    destination?.pointee = strdup(message)
}

/// Serializes the tap and the configuration observer with stop(), so neither
/// can touch the Rust context after it is freed. A callback runs while holding
/// the lock; stop clears `active` under it and then tears the engine down.
private final class VoiceProcessingGate {
    let lock = NSLock()
    var active = true
    /// Set once start() has returned, so notifications from building or
    /// starting the graph are not mistaken for a reconfigured running engine.
    var started = false
}

/// Runs `body`, turning an Objective-C exception AVAudioEngine raises into a
/// Swift error the caller can fall back from.
private func withVoiceProcessingGuard(_ body: @escaping () throws -> Void) throws {
    var failure: Error?
    var reason: UnsafeMutablePointer<CChar>?
    let status = hex_voice_processing_guard({
        do {
            try body()
        } catch {
            failure = error
        }
    }, &reason)
    if status != 0 {
        if let reason {
            let message = String(cString: reason)
            free(reason)
            throw VoiceProcessingError(message)
        }
        throw VoiceProcessingError("Objective-C exception")
    }
    if let failure {
        throw failure
    }
}

/// The CoreAudio device behind a cpal device UID, such as
/// `BuiltInMicrophoneDevice`.
private func voiceProcessingDeviceID(_ uid: String) throws -> AudioDeviceID {
    var deviceID = AudioDeviceID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioDeviceID>.size)
    var address = AudioObjectPropertyAddress(
        mSelector: kAudioHardwarePropertyTranslateUIDToDevice,
        mScope: kAudioObjectPropertyScopeGlobal,
        mElement: kAudioObjectPropertyElementMain
    )
    var uidReference: CFString = uid as CFString
    let status = withUnsafeMutablePointer(to: &uidReference) { pointer in
        AudioObjectGetPropertyData(
            AudioObjectID(kAudioObjectSystemObject),
            &address,
            UInt32(MemoryLayout<CFString>.size),
            pointer,
            &size,
            &deviceID
        )
    }
    guard status == noErr, deviceID != kAudioObjectUnknown else {
        throw VoiceProcessingError("Hex couldn't find the microphone (\(status)).")
    }
    return deviceID
}

/// Sets the CoreAudio device the input node's audio unit records from.
private func voiceProcessingSelect(
    _ deviceID: AudioDeviceID,
    on input: AVAudioInputNode
) throws {
    guard let audioUnit = input.audioUnit else {
        throw VoiceProcessingError("Hex couldn't reach the microphone's audio unit.")
    }
    var device = deviceID
    let status = AudioUnitSetProperty(
        audioUnit,
        kAudioOutputUnitProperty_CurrentDevice,
        kAudioUnitScope_Global,
        0,
        &device,
        UInt32(MemoryLayout<AudioDeviceID>.size)
    )
    guard status == noErr else {
        throw VoiceProcessingError("Hex couldn't select the microphone (\(status)).")
    }
}

/// The CoreAudio device the input node's audio unit records from now.
private func voiceProcessingSelected(_ input: AVAudioInputNode) throws -> AudioDeviceID {
    guard let audioUnit = input.audioUnit else {
        throw VoiceProcessingError("Hex couldn't reach the microphone's audio unit.")
    }
    var device = AudioDeviceID(kAudioObjectUnknown)
    var size = UInt32(MemoryLayout<AudioDeviceID>.size)
    let status = AudioUnitGetProperty(
        audioUnit,
        kAudioOutputUnitProperty_CurrentDevice,
        kAudioUnitScope_Global,
        0,
        &device,
        &size
    )
    guard status == noErr, device != kAudioObjectUnknown else {
        throw VoiceProcessingError("Hex couldn't read which microphone voice processing uses (\(status)).")
    }
    return device
}

/// A connected voice-processing engine and the format its tap delivers.
private struct VoiceProcessingGraph {
    let engine: AVAudioEngine
    let sampleRate: Double
    let channels: Int
}

/// Builds the engine, its connections, and its tap without starting it. Every
/// AVAudioEngine call that can raise runs inside the Objective-C guard, so a
/// refused graph becomes a Swift error and Rust opens the raw stream instead.
private func makeVoiceProcessingGraph(
    deviceID: AudioDeviceID,
    gate: VoiceProcessingGate,
    context: UnsafeMutableRawPointer,
    callback: VoiceProcessingCallback
) throws -> VoiceProcessingGraph {
    var graph: VoiceProcessingGraph?
    try withVoiceProcessingGuard {
        let engine = AVAudioEngine()
        let input = engine.inputNode
        // The device is set before voice processing turns on, which is when
        // the input node builds its duplex unit around it.
        try voiceProcessingSelect(deviceID, on: input)
        try input.setVoiceProcessingEnabled(true)
        // Enabling voice processing can reconfigure the unit, so the device
        // is set again on whichever unit now backs the input node.
        try voiceProcessingSelect(deviceID, on: input)
        // Advanced ducking lowers other audio only while the voice-processing
        // unit hears speech; `.min` is the least ducking macOS offers.
        input.voiceProcessingOtherAudioDuckingConfiguration =
            AVAudioVoiceProcessingOtherAudioDuckingConfiguration(
                enableAdvancedDucking: true,
                duckingLevel: .min
            )
        engine.connect(input, to: engine.mainMixerNode, format: input.outputFormat(forBus: 0))
        engine.connect(
            engine.mainMixerNode,
            to: engine.outputNode,
            format: engine.mainMixerNode.outputFormat(forBus: 0)
        )
        // Silent: voice processing is duplex and needs an output to run, but
        // the microphone is never played back into the room.
        engine.mainMixerNode.outputVolume = 0

        // The connected graph's formats decide whether a tap can work; check
        // all three after connecting, before installTap can raise.
        let inputInput = input.inputFormat(forBus: 0)
        let inputOutput = input.outputFormat(forBus: 0)
        let outputOutput = engine.outputNode.outputFormat(forBus: 0)
        for format in [inputInput, inputOutput, outputOutput] {
            guard format.sampleRate.isFinite, format.sampleRate > 0,
                  format.channelCount > 0
            else {
                throw VoiceProcessingError("The microphone reported no usable format.")
            }
        }
        guard
            let tapFormat = AVAudioFormat(
                commonFormat: .pcmFormatFloat32,
                sampleRate: inputOutput.sampleRate,
                channels: inputOutput.channelCount,
                interleaved: false
            )
        else {
            throw VoiceProcessingError("Hex couldn't make a tap format for the microphone.")
        }
        let rate = inputOutput.sampleRate
        input.installTap(onBus: 0, bufferSize: 1024, format: tapFormat) { buffer, when in
            gate.lock.lock()
            defer { gate.lock.unlock() }
            guard gate.active else { return }
            VoiceProcessingCapture.deliver(
                buffer,
                when: when,
                sampleRate: rate,
                context: context,
                callback: callback
            )
        }
        graph = VoiceProcessingGraph(
            engine: engine,
            sampleRate: inputOutput.sampleRate,
            channels: Int(inputOutput.channelCount)
        )
    }
    guard let graph else {
        throw VoiceProcessingError("Hex couldn't build the voice-processing graph.")
    }
    return graph
}

/// One AVAudioEngine running the chosen input through Apple's voice
/// processing. Its tap and configuration observer report while holding the
/// gate's lock; stop clears that lock's flag before tearing the engine down.
final class VoiceProcessingCapture {
    private let engine: AVAudioEngine
    private let gate: VoiceProcessingGate
    private let deviceID: AudioDeviceID
    private let sampleRate: Double
    private let channels: Int
    private var configurationObserver: NSObjectProtocol?
    private var tapInstalled = false

    init(
        deviceUID: String,
        context: UnsafeMutableRawPointer,
        errorCallback: VoiceProcessingErrorCallback,
        callback: VoiceProcessingCallback
    ) throws {
        let deviceID = try voiceProcessingDeviceID(deviceUID)
        let gate = VoiceProcessingGate()
        let graph = try makeVoiceProcessingGraph(
            deviceID: deviceID,
            gate: gate,
            context: context,
            callback: callback
        )
        self.engine = graph.engine
        self.gate = gate
        self.deviceID = deviceID
        self.sampleRate = graph.sampleRate
        self.channels = graph.channels
        self.tapInstalled = true
        // A route or hardware change that stops a started engine is reported
        // so the listener reopens. One that leaves it running is its own
        // business; one during setup or start is not a reconfiguration yet.
        let engine = graph.engine
        self.configurationObserver = NotificationCenter.default.addObserver(
            forName: .AVAudioEngineConfigurationChange,
            object: engine,
            queue: nil
        ) { _ in
            gate.lock.lock()
            defer { gate.lock.unlock() }
            guard gate.active, gate.started, !engine.isRunning else { return }
            errorCallback(context)
        }
    }

    var captureRate: Double { sampleRate }

    var captureChannels: Int { channels }

    func start() throws {
        let engine = self.engine
        let gate = self.gate
        let deviceID = self.deviceID
        try withVoiceProcessingGuard {
            engine.prepare()
            try engine.start()
            // The engine may have moved to another device while it started;
            // Hex records from the chosen one or not at all.
            let selected = try voiceProcessingSelected(engine.inputNode)
            guard selected == deviceID else {
                throw VoiceProcessingError(
                    "Voice processing started on microphone \(selected) instead of \(deviceID)."
                )
            }
            gate.lock.lock()
            gate.started = true
            gate.lock.unlock()
        }
    }

    /// Clears the gate first, so a tap or notification that already holds the
    /// lock finishes before teardown; after that no callback can touch the
    /// Rust context. Cleans up even when start() threw after prepare.
    func stop() {
        gate.lock.lock()
        gate.active = false
        gate.started = false
        gate.lock.unlock()
        if let configurationObserver {
            NotificationCenter.default.removeObserver(configurationObserver)
            self.configurationObserver = nil
        }
        if tapInstalled {
            engine.inputNode.removeTap(onBus: 0)
            tapInstalled = false
        }
        engine.stop()
        engine.reset()
    }

    /// One tap buffer as mono float samples and the Mach ticks it was
    /// captured at. Channel 0 is the voice-processed channel; a
    /// non-interleaved buffer keeps each channel's frames contiguous, so it
    /// goes to Rust without a copy.
    fileprivate static func deliver(
        _ buffer: AVAudioPCMBuffer,
        when: AVAudioTime,
        sampleRate: Double,
        context: UnsafeMutableRawPointer,
        callback: VoiceProcessingCallback
    ) {
        let frames = Int(buffer.frameLength)
        guard frames > 0, let channels = buffer.floatChannelData else {
            return
        }
        let hostTime = when.isHostTimeValid ? when.hostTime : 0
        callback(
            context,
            UnsafePointer(channels[0]),
            frames,
            hostTime,
            UInt32(sampleRate.rounded())
        )
    }
}

/// Builds the voice-processing engine for `deviceUID` and connects its graph,
/// without starting it. Answers a retained handle, the capture rate and
/// channels, or null with a message in `errorOutput`.
@_cdecl("hex_voice_processing_start")
public func hexVoiceProcessingStart(
    _ deviceUID: UnsafePointer<CChar>?,
    _ callback: VoiceProcessingCallback?,
    _ errorCallback: VoiceProcessingErrorCallback?,
    _ context: UnsafeMutableRawPointer?,
    _ sampleRate: UnsafeMutablePointer<Double>?,
    _ channels: UnsafeMutablePointer<UInt32>?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    guard let deviceUID, let callback, let errorCallback, let context else {
        writeVoiceProcessingError(
            "Hex's voice isolation received no microphone, callback, or context.",
            to: errorOutput
        )
        return nil
    }
    do {
        let capture = try VoiceProcessingCapture(
            deviceUID: String(cString: deviceUID),
            context: context,
            errorCallback: errorCallback,
            callback: callback
        )
        sampleRate?.pointee = capture.captureRate
        channels?.pointee = UInt32(capture.captureChannels)
        return Unmanaged.passRetained(capture).toOpaque()
    } catch {
        writeVoiceProcessingError(
            (error as? LocalizedError)?.errorDescription ?? error.localizedDescription,
            to: errorOutput
        )
        return nil
    }
}

/// Starts the engine behind `handle` and verifies its device; 0 on success,
/// else 1 with a message in `errorOutput`.
@_cdecl("hex_voice_processing_begin")
public func hexVoiceProcessingBegin(
    _ handle: UnsafeMutableRawPointer?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    guard let handle else {
        writeVoiceProcessingError("Hex lost the voice isolation engine.", to: errorOutput)
        return 1
    }
    do {
        try Unmanaged<VoiceProcessingCapture>.fromOpaque(handle).takeUnretainedValue().start()
        return 0
    } catch {
        writeVoiceProcessingError(
            (error as? LocalizedError)?.errorDescription ?? error.localizedDescription,
            to: errorOutput
        )
        return 1
    }
}

/// Clears the callback gate, removes the tap, stops the engine, resets it,
/// and releases the handle.
@_cdecl("hex_voice_processing_stop")
public func hexVoiceProcessingStop(_ handle: UnsafeMutableRawPointer?) {
    guard let handle else { return }
    Unmanaged<VoiceProcessingCapture>.fromOpaque(handle).takeRetainedValue().stop()
}

@_cdecl("hex_voice_processing_free_string")
public func hexVoiceProcessingFreeString(_ string: UnsafeMutablePointer<CChar>?) {
    free(string)
}
