import CryptoKit
import Foundation

/// One file of a speech model pinned to a revision: where it sits in the model's folder, its
/// size and its SHA-256.
nonisolated struct PinnedModelFile: Sendable, Equatable {
    let path: String
    let size: Int64
    let sha256: String

    static func size(of url: URL) -> Int64? {
        (try? FileManager.default.attributesOfItem(atPath: url.path)[.size] as? NSNumber)?.int64Value
    }

    static func sha256(of url: URL) throws -> String {
        let handle = try FileHandle(forReadingFrom: url)
        defer { try? handle.close() }
        var hasher = SHA256()
        // Each chunk drained at once: otherwise the whole 445 MB encoder stays in memory.
        while try autoreleasepool(invoking: {
            guard let chunk = try handle.read(upToCount: 4 << 20), !chunk.isEmpty else { return false }
            hasher.update(data: chunk)
            return true
        }) {}
        return hasher.finalize().map { String(format: "%02x", $0) }.joined()
    }

    /// Whether `url` holds exactly this file.
    func matches(_ url: URL) -> Bool {
        Self.size(of: url) == size && (try? Self.sha256(of: url)) == sha256
    }
}

/// Parakeet V2 as Hex downloads it, for the app's engine and for Speech model in keyboard:
/// FluidInference's Core ML build pinned to one revision, the 21 files FluidAudio's V2 loader
/// reads, 464 MB. Before, Hex downloaded whatever Hugging Face's `main` held.
nonisolated enum ParakeetV2Files {
    static let repository = "FluidInference/parakeet-tdt-0.6b-v2-coreml"
    static let revision = "ee09c569f73759e6d44c9bd16766f477b2b36d39"

    static var baseURL: String {
        "https://huggingface.co/\(repository)/resolve/\(revision)"
    }

    /// The sizes and SHA-256 digests at `revision`.
    static let files: [PinnedModelFile] = [
        PinnedModelFile(path: "Preprocessor.mlmodelc/analytics/coremldata.bin", size: 243,
                        sha256: "03ab3c1327a054c54c07a40325db967ec574f2c91dcc8192bfa44aa561bcf2d8"),
        PinnedModelFile(path: "Preprocessor.mlmodelc/coremldata.bin", size: 494,
                        sha256: "d88ea1fc349459c9e100d6a96688c5b29a1f0d865f544be103001724b986b6d6"),
        PinnedModelFile(path: "Preprocessor.mlmodelc/metadata.json", size: 2974,
                        sha256: "fb16c581ff5e1b962e7cb2181ed892cd32f9f84c12b6e80ff3e089f28e35bcbb"),
        PinnedModelFile(path: "Preprocessor.mlmodelc/model.mil", size: 27_166,
                        sha256: "3e06d16fd061294c8a75be68c43a3b1ed1f593d4a9c35249e9cdbccadc59721e"),
        PinnedModelFile(path: "Preprocessor.mlmodelc/weights/weight.bin", size: 298_880,
                        sha256: "a5f7df6c7f47147ae9486fe18cc7792f9a44d093ec3c6a11e91ef2dc363c48dc"),
        PinnedModelFile(path: "Encoder.mlmodelc/analytics/coremldata.bin", size: 243,
                        sha256: "42e638870d73f26b332918a3496ce36793fbb413a81cbd3d16ba01328637a105"),
        PinnedModelFile(path: "Encoder.mlmodelc/coremldata.bin", size: 485,
                        sha256: "4def7aa848599ad0e17a8b9a982edcdbf33cf92e1f4b798de32e2ca0bc74b030"),
        PinnedModelFile(path: "Encoder.mlmodelc/metadata.json", size: 2926,
                        sha256: "58222fbc48c13c49d9715567803cd50cb9c23e4360462e0f8ffcea59a2c73c63"),
        PinnedModelFile(path: "Encoder.mlmodelc/model.mil", size: 959_769,
                        sha256: "ed7b19156ca29fa7dfd6891deb9fda4b0e8893f68597c985d135736546a43808"),
        PinnedModelFile(path: "Encoder.mlmodelc/weights/weight.bin", size: 445_187_200,
                        sha256: "4adc7ad44f9d05e1bffeb2b06d3bb02861a5c7602dff63a6b494aed3bf8a6c3e"),
        PinnedModelFile(path: "Decoder.mlmodelc/analytics/coremldata.bin", size: 243,
                        sha256: "46de1a6fe2e49d19a2125bc91acf020df7f2aea84ba821532aade8427a440b05"),
        PinnedModelFile(path: "Decoder.mlmodelc/coremldata.bin", size: 554,
                        sha256: "d200ca07694a347f6d02a3886a062ae839831e094e443222f2e48a14945966a8"),
        PinnedModelFile(path: "Decoder.mlmodelc/metadata.json", size: 3427,
                        sha256: "90a279b822496316458febc0ce761ab05954fadd9d66aa97bea077a35fc8f2b2"),
        PinnedModelFile(path: "Decoder.mlmodelc/model.mil", size: 13_106,
                        sha256: "7b95a5a6b672c652000348a67b6d4d92bb8e176b978c6666fe73c28a4d7ec579"),
        PinnedModelFile(path: "Decoder.mlmodelc/weights/weight.bin", size: 14_429_952,
                        sha256: "27d26890221d82322c1092fd99d7b40578e435d5cf4b83c887c42603caf97aba"),
        PinnedModelFile(path: "JointDecision.mlmodelc/analytics/coremldata.bin", size: 243,
                        sha256: "f1183ba213bb94a918c8d2cad19ab045320618f97f6ca662245b3936d7b090f7"),
        PinnedModelFile(path: "JointDecision.mlmodelc/coremldata.bin", size: 534,
                        sha256: "e2c6752f1c8cf2d3f6f26ec93195c9bfa759ad59edf9f806696a138154f96f11"),
        PinnedModelFile(path: "JointDecision.mlmodelc/metadata.json", size: 2936,
                        sha256: "ba8d309417b9acd4a175fdb15687de6a941db2f5b06666a60e7cf3cc8e2d3c3c"),
        PinnedModelFile(path: "JointDecision.mlmodelc/model.mil", size: 9722,
                        sha256: "93bf82042235127cb81ab537dcae47a1c2e7e242ce4ffdaf772981b45eedc4f0"),
        PinnedModelFile(path: "JointDecision.mlmodelc/weights/weight.bin", size: 3_453_388,
                        sha256: "ca22a65903a05e64137677da608077578a8606090a598abf4875fa6199aaa19d"),
        PinnedModelFile(path: "parakeet_vocab.json", size: 18_762,
                        sha256: "57019fe3c745772ca83a1b048a4bb951cd51329504ea33d4d83316b96e279a97"),
    ]

    static let totalBytes: Int64 = files.reduce(0) { $0 + $1.size }

    static func url(for file: PinnedModelFile, base: String = baseURL) -> URL {
        URL(string: "\(base)/\(file.path)")!
    }

    /// Every file at its full size; each was checked against its digest before it got its name.
    static func isDownloaded(in directory: URL) -> Bool {
        files.allSatisfy { PinnedModelFile.size(of: directory.appendingPathComponent($0.path)) == $0.size }
    }

    /// The bytes of the files that got their names so far.
    static func placedBytes(in directory: URL) -> Int64 {
        files.reduce(0) { total, file in
            PinnedModelFile.size(of: directory.appendingPathComponent(file.path)) == file.size ? total + file.size : total
        }
    }

    /// What an unfinished download leaves beside the files: `.partial` (and FluidAudio's
    /// `.etag`), `.download` (a file not yet checked) and `.resume` (a background task's resume data).
    static func removeLeftovers(in directory: URL) {
        let fileManager = FileManager.default
        for file in files {
            let url = directory.appendingPathComponent(file.path)
            for suffix in ["partial", "partial.etag", "download", "resume"] {
                try? fileManager.removeItem(at: url.appendingPathExtension(suffix))
            }
        }
    }
}
