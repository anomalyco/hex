import CoreML
import CryptoKit
import FluidAudio
import Foundation

/// Parakeet Ultra: Moondream's post-training of NVIDIA Parakeet TDT 0.6B v3 (25 European
/// languages, CC BY 4.0), as FluidInference's Core ML build, pinned to one revision.
/// FluidAudio 0.15.5 has no `.ultra` (it came in 0.17.7), but the build keeps v3's files,
/// shapes and vocabulary, so Hex downloads and loads it itself and hands FluidAudio v3 models.
/// FluidAudio's own loader is never pointed at it: on a failed load it deletes the folder and
/// downloads v3 from its own repository in its place.
nonisolated enum ParakeetUltraModel {
    struct File: Sendable, Equatable {
        let path: String
        let size: Int64
        let sha256: String
    }

    enum DownloadError: Error, Equatable {
        case badStatus(Int)
        case corrupt(String)
    }

    /// The stand-in server simulator runs download from (`HEX_ULTRA_MODEL_BASE`), in debug
    /// builds only: it serves the same files at the same paths.
    static var baseURL: String {
        #if DEBUG
        if let base = ProcessInfo.processInfo.environment["HEX_ULTRA_MODEL_BASE"], !base.isEmpty {
            return base
        }
        #endif
        return "https://huggingface.co/\(repository)/resolve/\(revision)"
    }

    static let repository = "FluidInference/parakeet-ultra-coreml"
    static let revision = "95eaa59a39d4394f047a4dc5cce480388a60d1b6"

    /// The files FluidAudio's v3 path reads, with the sizes and SHA-256 digests at `revision`.
    static let files: [File] = [
        File(path: "Preprocessor.mlmodelc/analytics/coremldata.bin", size: 243,
             sha256: "c9beeb989c8d66f8be11df59bc6df277ec76cee404f6865b46243835ef562f6d"),
        File(path: "Preprocessor.mlmodelc/coremldata.bin", size: 486,
             sha256: "dbde3f2300842c1fd51ef3ff948a0bcffe65ffd2dca10707f2509f32c1d65b1d"),
        File(path: "Preprocessor.mlmodelc/metadata.json", size: 2841,
             sha256: "2a98699e22d279dd37fa1d238aeb1c6db1df0d6fad687775324157689d8f3acf"),
        File(path: "Preprocessor.mlmodelc/model.mil", size: 28181,
             sha256: "4b8518a956450fec57f06c2a21bdffc26973f7f1fa6842fb38fe917f896b6b93"),
        File(path: "Preprocessor.mlmodelc/weights/weight.bin", size: 491_072,
             sha256: "129b76e3aeafa8afa3ea76d995b964b145fe83700d579f6ff42c4c38fa0968ea"),
        File(path: "Encoder.mlmodelc/analytics/coremldata.bin", size: 243,
             sha256: "d87101d824d6723cf95304da33755c3c60e762663b9ef2b0c4bd0aa166a09a0d"),
        File(path: "Encoder.mlmodelc/coremldata.bin", size: 514,
             sha256: "397a84a4062f563cbc5f56077c674f09a61d85be5090f61d2f1932afb92ac0fe"),
        File(path: "Encoder.mlmodelc/model.mil", size: 1_002_653,
             sha256: "f5d601568a4171d99a314c0fe3f6bc67715da2623732a3fb566e050ea83e5848"),
        File(path: "Encoder.mlmodelc/weights/weight.bin", size: 594_211_328,
             sha256: "315ba01f33cadbf601d43ac7f5c86208b7aa75fdaa34705c9869d5abe3521c9b"),
        File(path: "Decoder.mlmodelc/analytics/coremldata.bin", size: 243,
             sha256: "fe92b6cfaa012abd5248c0bc877832f19807015abffc60d87b8ccc8ccb48b3b5"),
        File(path: "Decoder.mlmodelc/coremldata.bin", size: 560,
             sha256: "3b06e66768f0df7e21795f50e2b29300e33eeb1a2579dc42c695279c2d308497"),
        File(path: "Decoder.mlmodelc/model.mil", size: 13110,
             sha256: "956f600207f88396017ca5c96cfa3acd5bfee60835a5b44b762e033a8fb28955"),
        File(path: "Decoder.mlmodelc/weights/weight.bin", size: 23_604_992,
             sha256: "02a0d219f281b9665bc10c8768649403b2eebbbf4b44c627041d948e0de11bb4"),
        File(path: "JointDecisionv3.mlmodelc/analytics/coremldata.bin", size: 243,
             sha256: "68d38ca646aebafa7a9329e2efda50f5767c49e89fdb5f77f212072bb66f97c4"),
        File(path: "JointDecisionv3.mlmodelc/coremldata.bin", size: 592,
             sha256: "5e3af5a4ce686f6c237cadbd9284e10d333bc0e1546633cd0e430e6194044bc4"),
        File(path: "JointDecisionv3.mlmodelc/model.mil", size: 11777,
             sha256: "791b3c3cf3eb2079c84623fc880f6bba008d1366e5f8b03e9a8ed8bd4d7194a0"),
        File(path: "JointDecisionv3.mlmodelc/weights/weight.bin", size: 12_642_764,
             sha256: "3f310b85b82341c53ec383025ab094a4e462ee1c592e1ad7c6bfe39cff66ca25"),
        File(path: "parakeet_vocab.json", size: 151_122,
             sha256: "7ec60e05f1b24480736ec0eed40900f4626bce1fa9a60fd700ec7e2a59198735"),
    ]

    /// What the download weighs, about 632 MB.
    static let totalBytes: Int64 = files.reduce(0) { $0 + $1.size }

    /// In the app's Application Support (the keyboard never reads it), out of backups.
    static var directory: URL {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
        return support
            .appendingPathComponent("SpeechModels", isDirectory: true)
            .appendingPathComponent("parakeet-ultra-coreml-\(revision.prefix(7))", isDirectory: true)
    }

    /// Every file at its full size; each was checked against its digest before it got its name.
    static func isDownloaded(in directory: URL = directory, files: [File] = files) -> Bool {
        files.allSatisfy { size(of: directory.appendingPathComponent($0.path)) == $0.size }
    }

    /// What's on disk so far, finished files and `.partial` ones: where a download resumes.
    static func bytesOnDisk(in directory: URL = directory, files: [File] = files) -> Int64 {
        files.reduce(0) { total, file in
            let destination = directory.appendingPathComponent(file.path)
            if let size = size(of: destination), size == file.size { return total + size }
            let partial = size(of: destination.appendingPathExtension("partial")) ?? 0
            return total + min(partial, file.size)
        }
    }

    static func url(for file: File, base: String = baseURL) -> URL {
        URL(string: "\(base)/\(file.path)")!
    }

    /// Downloads what's missing, resuming a file from its `.partial`. `progress` gets the
    /// bytes on disk, first what an earlier download left.
    static func download(
        to directory: URL = directory,
        files: [File] = files,
        base: String = baseURL,
        session: URLSession = .shared,
        progress: @escaping @Sendable (Int64) -> Void
    ) async throws {
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        var folder = directory
        var values = URLResourceValues()
        values.isExcludedFromBackup = true
        try? folder.setResourceValues(values)

        progress(bytesOnDisk(in: directory, files: files))
        var done: Int64 = 0
        for file in files {
            try Task.checkCancellation()
            let destination = directory.appendingPathComponent(file.path)
            if size(of: destination) == file.size {
                done += file.size
                continue
            }
            let before = done
            try await fetch(file, to: destination, base: base, session: session) { written in
                progress(before + written)
            }
            done += file.size
            progress(done)
        }
    }

    /// Removes the download, partial files included.
    static func remove(from directory: URL = directory) {
        try? FileManager.default.removeItem(at: directory)
    }

    /// Loads the four models as FluidAudio's v3 loader would (preprocessor on the CPU, the rest
    /// on the CPU and Neural Engine), calling `modelLoaded` after each. The first load on an
    /// iPhone compiles the encoder for its Neural Engine and reports nothing until it's done.
    static func load(
        from directory: URL = directory,
        modelLoaded: @Sendable () -> Void = {}
    ) throws -> AsrModels {
        func model(_ name: String, _ units: MLComputeUnits) throws -> MLModel {
            let configuration = AsrModels.defaultConfiguration()
            configuration.computeUnits = units
            let loaded = try MLModel(contentsOf: directory.appendingPathComponent(name), configuration: configuration)
            modelLoaded()
            return loaded
        }
        let preprocessor = try model(ModelNames.ASR.preprocessorFile, .cpuOnly)
        let encoder = try model(ModelNames.ASR.encoderFile, .cpuAndNeuralEngine)
        let decoder = try model(ModelNames.ASR.decoderFile, .cpuAndNeuralEngine)
        let joint = try model(ModelNames.ASR.jointV3File, .cpuAndNeuralEngine)
        return AsrModels(
            encoder: encoder,
            preprocessor: preprocessor,
            decoder: decoder,
            joint: joint,
            configuration: AsrModels.defaultConfiguration(),
            vocabulary: try vocabulary(at: directory.appendingPathComponent(ModelNames.ASR.vocabularyFile)),
            version: .v3
        )
    }

    /// v3's vocabulary: token IDs as string keys.
    static func vocabulary(at url: URL) throws -> [Int: String] {
        let object = try JSONSerialization.jsonObject(with: Data(contentsOf: url))
        guard let entries = object as? [String: String] else {
            throw DownloadError.corrupt(url.lastPathComponent)
        }
        var vocabulary: [Int: String] = [:]
        for (key, token) in entries {
            if let id = Int(key) { vocabulary[id] = token }
        }
        return vocabulary
    }

    private static func size(of url: URL) -> Int64? {
        (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.int64Value
    }

    private static func fetch(
        _ file: File,
        to destination: URL,
        base: String,
        session: URLSession,
        onBytes: @escaping @Sendable (Int64) -> Void
    ) async throws {
        let fileManager = FileManager.default
        try fileManager.createDirectory(at: destination.deletingLastPathComponent(), withIntermediateDirectories: true)
        let partial = destination.appendingPathExtension("partial")
        var offset = size(of: partial) ?? 0
        if offset > file.size {
            try? fileManager.removeItem(at: partial)
            offset = 0
        }
        if offset < file.size {
            var request = URLRequest(url: url(for: file, base: base))
            if offset > 0 {
                request.setValue("bytes=\(offset)-", forHTTPHeaderField: "Range")
            }
            let writer = try PartialFileWriter(url: partial, offset: offset, onBytes: onBytes)
            try await writer.run(request, session: session)
        }
        guard size(of: partial) == file.size, try sha256(of: partial) == file.sha256 else {
            try? fileManager.removeItem(at: partial)
            throw DownloadError.corrupt(file.path)
        }
        try? fileManager.removeItem(at: destination)
        try fileManager.moveItem(at: partial, to: destination)
    }

    static func sha256(of url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        // Each chunk drained at once: otherwise the whole 594 MB encoder stays in memory.
        while try autoreleasepool(invoking: {
            guard let chunk = try handle.read(upToCount: 4 << 20), !chunk.isEmpty else { return false }
            hasher.update(data: chunk)
            return true
        }) {}
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }
}

/// Streams one response into a `.partial` file: appended after a 206, from the start after
/// a 200 (a server that ignored the range).
private nonisolated final class PartialFileWriter: NSObject, URLSessionDataDelegate, @unchecked Sendable {
    private let lock = NSLock()
    private let url: URL
    private let offset: Int64
    private let onBytes: @Sendable (Int64) -> Void
    private var handle: FileHandle?
    private var written: Int64 = 0
    private var failure: Error?
    private var continuation: CheckedContinuation<Void, Error>?

    init(url: URL, offset: Int64, onBytes: @escaping @Sendable (Int64) -> Void) throws {
        self.url = url
        self.offset = offset
        self.onBytes = onBytes
        if !FileManager.default.fileExists(atPath: url.path) {
            FileManager.default.createFile(atPath: url.path, contents: nil)
        }
    }

    func run(_ request: URLRequest, session: URLSession) async throws {
        let task = session.dataTask(with: request)
        task.delegate = self
        try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { (continuation: CheckedContinuation<Void, Error>) in
                lock.withLock { self.continuation = continuation }
                task.resume()
            }
        } onCancel: {
            task.cancel()
        }
    }

    func urlSession(
        _ session: URLSession,
        dataTask: URLSessionDataTask,
        didReceive response: URLResponse,
        completionHandler: @escaping @Sendable (URLSession.ResponseDisposition) -> Void
    ) {
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        let opened: Bool = lock.withLock {
            guard status == 200 || status == 206, let handle = try? FileHandle(forWritingTo: url) else {
                failure = ParakeetUltraModel.DownloadError.badStatus(status)
                return false
            }
            let start = status == 206 ? offset : 0
            try? handle.truncate(atOffset: UInt64(start))
            _ = try? handle.seekToEnd()
            self.handle = handle
            written = start
            return true
        }
        completionHandler(opened ? .allow : .cancel)
    }

    func urlSession(_ session: URLSession, dataTask: URLSessionDataTask, didReceive data: Data) {
        let total: Int64? = lock.withLock {
            guard let handle else { return nil }
            do {
                try handle.write(contentsOf: data)
            } catch {
                failure = error
                dataTask.cancel()
                return nil
            }
            written += Int64(data.count)
            return written
        }
        if let total { onBytes(total) }
    }

    func urlSession(_ session: URLSession, task: URLSessionTask, didCompleteWithError error: Error?) {
        let (continuation, result): (CheckedContinuation<Void, Error>?, Error?) = lock.withLock {
            try? handle?.close()
            handle = nil
            let continuation = self.continuation
            self.continuation = nil
            return (continuation, failure ?? error)
        }
        if let result {
            continuation?.resume(throwing: result)
        } else {
            continuation?.resume()
        }
    }
}
