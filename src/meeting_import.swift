import AVFAudio
import AudioToolbox
import Foundation

private let importSampleRate: Double = 16_000
private let importInputFrames: AVAudioFrameCount = 32_768

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
    _ isCancelled: (@convention(c) () -> Int32)?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Int32 {
    guard let inputPath = importPath(inputPath),
          let outputPath = importPath(outputPath),
          let isCancelled
    else {
        writeImportError(
            "The audio import received no file path or cancellation check.",
            to: errorOutput
        )
        return 1
    }
    do {
        try convertAudio(
            URL(fileURLWithPath: inputPath),
            to: URL(fileURLWithPath: outputPath),
            isCancelled: isCancelled
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
/// `isCancelled` is checked between blocks, and in each input read, so a quit
/// stops decode early enough for the caller to remove its folder.
private func convertAudio(
    _ inputURL: URL,
    to outputURL: URL,
    isCancelled: @convention(c) () -> Int32
) throws {
    let input = try AVAudioFile(forReading: inputURL)
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
    // Stereo and multichannel files mix down; the converter otherwise keeps
    // one channel and drops the rest.
    converter.downmix = true
    let settings: [String: Any] = [
        AVFormatIDKey: kAudioFormatLinearPCM,
        AVAudioFileTypeKey: kAudioFileWAVEType,
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
    // The header is written when the file closes; do it before Rust reads it.
    defer { output.close() }

    let inputFormat = input.processingFormat
    // One output buffer holds a whole input block, so the converter never has
    // to keep a returned input buffer for a later call.
    let ratio = importSampleRate / max(inputFormat.sampleRate, 1)
    let outputFrames = AVAudioFrameCount(
        min(1_048_576, max(1, Int((Double(importInputFrames) * max(1, ratio)).rounded(.up)) + 64))
    )
    guard
        let inputBuffer = AVAudioPCMBuffer(
            pcmFormat: inputFormat,
            frameCapacity: importInputFrames
        ),
        let outputBuffer = AVAudioPCMBuffer(
            pcmFormat: outputFormat,
            frameCapacity: outputFrames
        )
    else {
        throw ImportError("Hex ran out of memory converting this audio file.")
    }

    var inputExhausted = false
    var finished = false
    var failure: Error?
    var wroteFrames: AVAudioFramePosition = 0
    var previousReadShort = false

    while !finished {
        if isCancelled() != 0 {
            throw ImportError("Hex is quitting; the audio was not imported.")
        }
        try autoreleasepool {
            outputBuffer.frameLength = 0
            var conversionError: NSError?
            let status = converter.convert(to: outputBuffer, error: &conversionError) {
                _, inputStatus in
                // Once the file has no more audio, keep answering end of
                // stream; a no-data answer would make the converter wait.
                if inputExhausted {
                    inputStatus.pointee = .endOfStream
                    return nil
                }
                if isCancelled() != 0 {
                    failure = ImportError("Hex is quitting; the audio was not imported.")
                    inputStatus.pointee = .endOfStream
                    return nil
                }
                inputBuffer.frameLength = 0
                do {
                    try input.read(into: inputBuffer)
                } catch {
                    let code = (error as NSError).code
                    // A short or malformed file ends at EOF, not as a read
                    // failure. AVAudioFile can answer the tail with a generic
                    // nilError instead of a zero-frame read; at or after the
                    // file's recorded end, or after the previous read was
                    // short (MP3's estimated length can be too high), that is
                    // EOF. A mid-file error after a full read still fails.
                    if code == Int(kAudioFileEndOfFileError)
                        || input.framePosition >= input.length
                        || previousReadShort
                    {
                        inputExhausted = true
                        inputStatus.pointee = .endOfStream
                    } else {
                        failure = ImportError(
                            "Hex could not read the audio at frame \(input.framePosition) of \(input.length): \(error.localizedDescription)"
                        )
                        inputStatus.pointee = .endOfStream
                    }
                    return nil
                }
                if inputBuffer.frameLength == 0 {
                    inputExhausted = true
                    inputStatus.pointee = .endOfStream
                    return nil
                }
                // A short read is not EOF by itself; it only lets a following
                // read error at the tail count as EOF.
                previousReadShort =
                    inputBuffer.frameLength < inputBuffer.frameCapacity
                inputStatus.pointee = .haveData
                return inputBuffer
            }
            if let failure {
                throw failure
            }
            if let conversionError {
                throw conversionError
            }
            // Every status may carry audio: write it before deciding whether
            // the run is over.
            if outputBuffer.frameLength > 0 {
                try output.write(from: outputBuffer)
                wroteFrames += AVAudioFramePosition(outputBuffer.frameLength)
            }
            switch status {
            case .haveData:
                break
            case .inputRanDry:
                // With the input block always answering data or end of
                // stream, this only means the converter has no output and
                // nothing left to read.
                if inputExhausted && outputBuffer.frameLength == 0 {
                    finished = true
                }
            case .endOfStream:
                finished = true
            case .error:
                throw ImportError("Hex could not convert this audio file.")
            @unknown default:
                throw ImportError("Hex could not convert this audio file.")
            }
        }
    }
    if wroteFrames == 0 {
        throw ImportError("The audio file has no sound.")
    }
}
