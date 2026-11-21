import AVFoundation

/// `mux`: a movie with audio files laid in on its clock. The video is copied as it is; the
/// audio files are mixed into one AAC track, each starting `offsetMs` after the movie's start
/// (before it when negative), trimmed to the movie.
enum Mux {
  struct Audio {
    let url: URL
    let offsetMs: Int
  }

  static func run(movie: URL, audio: [Audio], out: URL) async throws -> [String: Any] {
    let source = AVURLAsset(url: movie)
    guard let video = try await source.loadTracks(withMediaType: .video).first else {
      throw HelperError("The movie has no video")
    }
    let duration = try await source.load(.duration)
    let composition = AVMutableComposition()
    guard let videoTrack = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid) else {
      throw HelperError("Could not lay out the movie")
    }
    try videoTrack.insertTimeRange(CMTimeRange(start: .zero, duration: duration), of: video, at: .zero)
    var audioTracks: [AVMutableCompositionTrack] = []
    for input in audio {
      let asset = AVURLAsset(url: input.url)
      guard let track = try await asset.loadTracks(withMediaType: .audio).first else { continue }
      let length = try await asset.load(.duration)
      let offset = CMTime(value: CMTimeValue(input.offsetMs), timescale: 1000)
      let from = offset < .zero ? CMTimeMultiply(offset, multiplier: -1) : .zero
      let at = offset < .zero ? .zero : offset
      let span = CMTimeMinimum(CMTimeSubtract(length, from), CMTimeSubtract(duration, at))
      guard span > .zero,
        let laid = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)
      else { continue }
      try laid.insertTimeRange(CMTimeRange(start: from, duration: span), of: track, at: at)
      audioTracks.append(laid)
    }

    try? FileManager.default.removeItem(at: out)
    let reader = try AVAssetReader(asset: composition)
    let writer = try AVAssetWriter(outputURL: out, fileType: .mp4)
    writer.shouldOptimizeForNetworkUse = true

    let videoOut = AVAssetReaderTrackOutput(track: videoTrack, outputSettings: nil)
    videoOut.alwaysCopiesSampleData = false
    reader.add(videoOut)
    let hint = try await video.load(.formatDescriptions).first
    let videoIn = AVAssetWriterInput(mediaType: .video, outputSettings: nil, sourceFormatHint: hint)
    videoIn.transform = try await video.load(.preferredTransform)
    videoIn.expectsMediaDataInRealTime = false
    writer.add(videoIn)

    var pairs: [(AVAssetReaderOutput, AVAssetWriterInput)] = [(videoOut, videoIn)]
    if !audioTracks.isEmpty {
      let audioOut = AVAssetReaderAudioMixOutput(audioTracks: audioTracks, audioSettings: [
        AVFormatIDKey: kAudioFormatLinearPCM,
        AVSampleRateKey: 48_000,
        AVNumberOfChannelsKey: 2,
        AVLinearPCMBitDepthKey: 32,
        AVLinearPCMIsFloatKey: true,
        AVLinearPCMIsBigEndianKey: false,
        AVLinearPCMIsNonInterleaved: false,
      ])
      reader.add(audioOut)
      var layout = AudioChannelLayout()
      layout.mChannelLayoutTag = kAudioChannelLayoutTag_Stereo
      let audioIn = AVAssetWriterInput(mediaType: .audio, outputSettings: [
        AVFormatIDKey: kAudioFormatMPEG4AAC,
        AVSampleRateKey: 48_000,
        AVNumberOfChannelsKey: 2,
        AVEncoderBitRateKey: 192_000,
        AVChannelLayoutKey: Data(bytes: &layout, count: MemoryLayout<AudioChannelLayout>.size),
      ])
      audioIn.expectsMediaDataInRealTime = false
      writer.add(audioIn)
      pairs.append((audioOut, audioIn))
    }

    guard reader.startReading() else {
      throw HelperError(reader.error?.localizedDescription ?? "Could not read the movie")
    }
    guard writer.startWriting() else {
      throw HelperError(writer.error?.localizedDescription ?? "Could not write the movie")
    }
    writer.startSession(atSourceTime: .zero)
    let copying = DispatchGroup()
    for (index, (output, input)) in pairs.enumerated() {
      copying.enter()
      input.requestMediaDataWhenReady(on: DispatchQueue(label: "mux.\(index)")) {
        while input.isReadyForMoreMediaData {
          guard reader.status == .reading, let buffer = output.copyNextSampleBuffer(), input.append(buffer) else {
            input.markAsFinished()
            copying.leave()
            return
          }
        }
      }
    }
    await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
      copying.notify(queue: .global()) { done.resume() }
    }
    if reader.status == .failed {
      writer.cancelWriting()
      throw HelperError(reader.error?.localizedDescription ?? "Could not read the movie")
    }
    await writer.finishWriting()
    if writer.status != .completed {
      throw HelperError(writer.error?.localizedDescription ?? "Could not write the movie")
    }
    return ["path": out.path, "audioTracks": audioTracks.count]
  }
}
