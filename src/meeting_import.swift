import AVFAudio
import AudioToolbox
import Foundation

private let importSampleRate: Double = 16_000
private let importInputFrames: AVAudioFrameCount = 32_768
private let importOutputFrames: AVAudioFrameCount = 16_384

private struct ImportError: LocalizedError {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var errorDescription: String? { message }
}

private func writeImportError(
    _ message: String,
    to destination: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) {
    destination?.pointee = strdup(message)
}

private func importPath(_ pointer: UnsafePointer<CChar>?) -> String? {
    guard let pointer else { return nil }
    return String(cString: pointer)
}

@_cdecl("hex_meeting_import_decode")
public func hexMeetingImportDecode(
    _ inputPath: UnsafePointer<CChar>?,
    _ outputPath: UnsafePointer<CChar>?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    guard let inputPath = importPath(inputPath), let outputPath = importPath(outputPath) else {
        writeImportError("The audio import received no file path.", to: errorOutput)
        return 1
    }
    do {
        try convertAudio(
            URL(fileURLWithPath: inputPath),
            to: URL(fileURLWithPath: outputPath)
        )
        return 0
    } catch {
        writeImportError(
            (error as? LocalizedError)?.errorDescription ?? error.localizedDescription,
            to: errorOutput
        )
        return 1
    }
}

@_cdecl("hex_meeting_import_free_string")
public func hexMeetingImportFreeString(_ string: UnsafeMutablePointer<CChar>?) {
    free(string)
}

/// Streams `inputURL` to a 16 kHz mono 32-bit float WAV at `outputURL` in
/// bounded blocks, so a long recording never loads whole into memory.
private func convertAudio(_ inputURL: URL, to outputURL: URL) throws {
    let input = try AVAudioFile(forReading: inputURL)
    guard input.length > 0 else {
        throw ImportError("The audio file has no sound.")
    }
    guard let outputFormat = AVAudioFormat(
        commonFormat: .pcmFormatFloat32,
        sampleRate: importSampleRate,
        channels: 1,
        interleaved: false
    ) else {
        throw ImportError("Hex could not convert audio to 16 kHz.")
    }
    guard let converter = AVAudioConverter(from: input.processingFormat, to: outputFormat) else {
        throw ImportError("Hex could not read this audio file's format.")
    }
    let settings: [String: Any] = [
        AVFormatIDKey: kAudioFormatLinearPCM,
        AVSampleRateKey: importSampleRate,
        AVNumberOfChannelsKey: 1,
        AVLinearPCMBitDepthKey: 32,
        AVLinearPCMIsFloatKey: true,
        AVLinearPCMIsBigEndianKey: false,
        AVLinearPCMIsNonInterleaved: false,
    ]
    let output = try AVAudioFile(
        forWriting: outputURL,
        settings: settings,
        commonFormat: .pcmFormatFloat32,
        interleaved: false
    )
    let inputFormat = input.processingFormat
    var finished = false
    var failure: Error?
    while !finished {
        guard let outputBuffer = AVAudioPCMBuffer(
            pcmFormat: outputFormat,
            frameCapacity: importOutputFrames
        ) else {
            throw ImportError("Hex ran out of memory converting this audio file.")
        }
        var conversionError: NSError?
        let status = converter.convert(to: outputBuffer, error: &conversionError) {
            requestedPackets, inputStatus in
            if finished {
                inputStatus.pointee = .noDataNow
                return nil
            }
            let remaining = input.length - input.framePosition
            if remaining <= 0 {
                finished = true
                inputStatus.pointee = .endOfStream
                return nil
            }
            let wanted = min(Int64(importInputFrames), max(1, Int64(requestedPackets)))
            let frames = AVAudioFrameCount(min(wanted, remaining))
            guard let inputBuffer = AVAudioPCMBuffer(
                pcmFormat: inputFormat,
                frameCapacity: frames
            ) else {
                failure = ImportError("Hex ran out of memory reading this audio file.")
                finished = true
                inputStatus.pointee = .endOfStream
                return nil
            }
            do {
                try input.read(into: inputBuffer, frameCount: frames)
            } catch {
                failure = error
                finished = true
                inputStatus.pointee = .endOfStream
                return nil
            }
            if inputBuffer.frameLength == 0 {
                finished = true
                inputStatus.pointee = .endOfStream
                return nil
            }
            inputStatus.pointee = .haveData
            return inputBuffer
        }
        if let failure {
            throw failure
        }
        if let conversionError {
            throw conversionError
        }
        switch status {
        case .haveData:
            try output.write(from: outputBuffer)
        case .inputRanDry:
            continue
        case .endOfStream:
            if outputBuffer.frameLength > 0 {
                try output.write(from: outputBuffer)
            }
            finished = true
        case .error:
            throw ImportError("Hex could not convert this audio file.")
        @unknown default:
            throw ImportError("Hex could not convert this audio file.")
        }
    }
}
