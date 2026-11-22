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

/// One AVAudioEngine running the chosen input through Apple's voice
/// processing. It is built and tapped first, then started once Rust has
/// installed the tap's capture feed.
final class VoiceProcessingCapture {
    private let engine: AVAudioEngine
    private let sampleRate: Double
    private var started = false

    init(
        deviceUID: String,
        context: UnsafeMutableRawPointer,
        callback: VoiceProcessingCallback
    ) throws {
        let deviceID = try voiceProcessingDeviceID(deviceUID)
        let engine = AVAudioEngine()
        let input = engine.inputNode
        // The device is set before voice processing turns on, which is when
        // the input node builds its duplex unit around it.
        try voiceProcessingSelect(deviceID, on: input)
        try input.setVoiceProcessingEnabled(true)
        // Enabling voice processing can reconfigure the unit, so the device
        // is set again on whichever unit now backs the input node.
        try voiceProcessingSelect(deviceID, on: input)
        // Advanced ducking lowers other audio as much as it can while the
        // microphone is open; macOS's own call apps do the same.
        input.voiceProcessingOtherAudioDuckingConfiguration =
            AVAudioVoiceProcessingOtherAudioDuckingConfiguration(
                enableAdvancedDucking: true,
                duckingLevel: .min
            )
        let format = input.outputFormat(forBus: 0)
        guard format.sampleRate.isFinite, format.sampleRate > 0, format.channelCount > 0 else {
            throw VoiceProcessingError("The microphone reported no usable format.")
        }
        engine.connect(input, to: engine.mainMixerNode, format: format)
        // Voice processing is duplex: the unit needs an output to run. The
        // mixer is silent, so the microphone is never played back.
        engine.connect(
            engine.mainMixerNode,
            to: engine.outputNode,
            format: engine.mainMixerNode.outputFormat(forBus: 0)
        )
        engine.mainMixerNode.outputVolume = 0
        let rate = format.sampleRate
        input.installTap(onBus: 0, bufferSize: 1024, format: format) { buffer, when in
            autoreleasepool {
                VoiceProcessingCapture.deliver(
                    buffer,
                    when: when,
                    sampleRate: rate,
                    context: context,
                    callback: callback
                )
            }
        }
        self.engine = engine
        self.sampleRate = rate
    }

    var captureRate: Double { sampleRate }

    func start() throws {
        engine.prepare()
        try engine.start()
        started = true
    }

    func stop() {
        if started {
            engine.stop()
            started = false
        }
        engine.inputNode.removeTap(onBus: 0)
    }

    /// One tap buffer as mono float samples and the Mach ticks it was
    /// captured at. The tap's own format is float32, one buffer per channel.
    private static func deliver(
        _ buffer: AVAudioPCMBuffer,
        when: AVAudioTime,
        sampleRate: Double,
        context: UnsafeMutableRawPointer,
        callback: VoiceProcessingCallback
    ) {
        let frames = Int(buffer.frameLength)
        let channelCount = Int(buffer.format.channelCount)
        guard frames > 0, channelCount > 0, let channels = buffer.floatChannelData else {
            return
        }
        var mono = [Float](repeating: 0, count: frames)
        for channel in 0..<channelCount {
            let samples = channels[channel]
            for frame in 0..<frames {
                mono[frame] += samples[frame]
            }
        }
        if channelCount > 1 {
            let scale = 1 / Float(channelCount)
            for frame in 0..<frames {
                mono[frame] *= scale
            }
        }
        let hostTime = when.isHostTimeValid ? when.hostTime : 0
        mono.withUnsafeBufferPointer { samples in
            callback(
                context,
                samples.baseAddress,
                frames,
                hostTime,
                UInt32(sampleRate.rounded())
            )
        }
    }
}

/// Builds the voice-processing engine for `deviceUID` and installs its tap,
/// without starting it. Answers a retained handle and the capture rate, or
/// null with a message in `errorOutput`.
@_cdecl("hex_voice_processing_start")
public func hexVoiceProcessingStart(
    _ deviceUID: UnsafePointer<CChar>?,
    _ callback: VoiceProcessingCallback?,
    _ context: UnsafeMutableRawPointer?,
    _ sampleRate: UnsafeMutablePointer<Double>?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    guard let deviceUID, let callback, let context else {
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
            callback: callback
        )
        sampleRate?.pointee = capture.captureRate
        return Unmanaged.passRetained(capture).toOpaque()
    } catch {
        writeVoiceProcessingError(
            (error as? LocalizedError)?.errorDescription ?? error.localizedDescription,
            to: errorOutput
        )
        return nil
    }
}

/// Starts the engine behind `handle`; 0 on success, else 1 with a message in
/// `errorOutput`.
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

/// Stops the engine, removes its tap, and releases the handle.
@_cdecl("hex_voice_processing_stop")
public func hexVoiceProcessingStop(_ handle: UnsafeMutableRawPointer?) {
    guard let handle else { return }
    Unmanaged<VoiceProcessingCapture>.fromOpaque(handle).takeRetainedValue().stop()
}

@_cdecl("hex_voice_processing_free_string")
public func hexVoiceProcessingFreeString(_ string: UnsafeMutablePointer<CChar>?) {
    free(string)
}
