import CoreML
import Foundation

// Labs › Clean up what I say: Hex's cleanup tagger, a small BERT token
// classifier, behind a C bridge. Labs › Clean up what I say v2 runs a
// four-head ModernBERT tagger through the same compile and load. Rust owns the tokenizer, the chunks, the
// guard, and the model's lifetime (`dictation/cleanup/tagger.rs`); this file
// only compiles, loads, and runs the Core ML model.
//
// The model runs on the CPU only. The Neural Engine ran it three to four times
// slower and took 8.1 s to load it; on the CPU a chunk takes about a
// millisecond and the int8 weights stay memory-mapped.

private struct TaggerError: LocalizedError {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var errorDescription: String? { message }
}

/// One loaded model. Rust holds it retained and runs it one call at a time.
private final class Tagger {
    let model: MLModel

    init(model: MLModel) {
        self.model = model
    }
}

private func writeError(_ error: Error, to output: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?) {
    output?.pointee = strdup(error.localizedDescription)
}

/// Where one compilation's answer waits for the thread that asked for it.
private final class Compilation: @unchecked Sendable {
    var result: Result<URL, Error> = .failure(TaggerError("The cleanup model did not compile."))
}

/// Compiles the package at `source` into a temporary folder and waits for
/// it: Rust calls from a thread of its own, never from the main thread or a
/// Swift task.
private func compile(_ source: URL) -> Result<URL, Error> {
    let compilation = Compilation()
    let done = DispatchSemaphore(value: 0)
    Task.detached(priority: .utility) {
        do {
            compilation.result = .success(try await MLModel.compileModel(at: source))
        } catch {
            compilation.result = .failure(error)
        }
        done.signal()
    }
    done.wait()
    return compilation.result
}

/// Compiles the model package at `package` and moves the compiled model to
/// `compiled`, which must not exist. Core ML compiles into a temporary folder
/// of its own; a failed move leaves nothing at `compiled`.
@_cdecl("hex_cleanup_tagger_compile")
public func hexCleanupTaggerCompile(
    _ package: UnsafePointer<CChar>?,
    _ compiled: UnsafePointer<CChar>?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Bool {
    guard let package, let compiled else {
        writeError(TaggerError("The cleanup model has no path."), to: errorOutput)
        return false
    }
    let source = URL(fileURLWithPath: String(cString: package))
    let destination = URL(fileURLWithPath: String(cString: compiled))
    switch compile(source) {
    case .success(let temporary):
        do {
            try FileManager.default.moveItem(at: temporary, to: destination)
            return true
        } catch {
            try? FileManager.default.removeItem(at: temporary)
            writeError(error, to: errorOutput)
            return false
        }
    case .failure(let failure):
        writeError(failure, to: errorOutput)
        return false
    }
}

/// Loads the compiled model at `compiled` on the CPU. The caller owns the
/// answer and gives it back to `hex_cleanup_tagger_release` exactly once.
@_cdecl("hex_cleanup_tagger_load")
public func hexCleanupTaggerLoad(
    _ compiled: UnsafePointer<CChar>?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutableRawPointer? {
    guard let compiled else {
        writeError(TaggerError("The cleanup model has no path."), to: errorOutput)
        return nil
    }
    let configuration = MLModelConfiguration()
    configuration.computeUnits = .cpuOnly
    do {
        let model = try MLModel(
            contentsOf: URL(fileURLWithPath: String(cString: compiled)),
            configuration: configuration
        )
        return Unmanaged.passRetained(Tagger(model: model)).toOpaque()
    } catch {
        writeError(error, to: errorOutput)
        return nil
    }
}

/// One model run over a chunk: `inputIDs` holds `paddedLength` ids, [CLS] ids
/// [SEP] zero-padded to a length the model takes, and the first `length` are
/// real. Writes the DELETE logit of each real token to `logits`, which holds
/// `length` values.
@_cdecl("hex_cleanup_tagger_run")
public func hexCleanupTaggerRun(
    _ tagger: UnsafeMutableRawPointer?,
    _ inputIDs: UnsafePointer<Int32>?,
    _ paddedLength: Int32,
    _ length: Int32,
    _ logits: UnsafeMutablePointer<Float>?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Bool {
    guard let tagger, let inputIDs, let logits, length > 0, length <= paddedLength else {
        writeError(TaggerError("The cleanup model was given no words."), to: errorOutput)
        return false
    }
    let model = Unmanaged<Tagger>.fromOpaque(tagger).takeUnretainedValue().model
    let padded = Int(paddedLength)
    let count = Int(length)
    do {
        let ids = try MLMultiArray(shape: [1, NSNumber(value: padded)], dataType: .int32)
        ids.withUnsafeMutableBufferPointer(ofType: Int32.self) { buffer, strides in
            let step = strides[1]
            for position in 0..<padded {
                buffer[position * step] = inputIDs[position]
            }
        }
        let realLength = try MLMultiArray(shape: [1], dataType: .int32)
        realLength[0] = NSNumber(value: count)
        let input = try MLDictionaryFeatureProvider(dictionary: [
            "input_ids": MLFeatureValue(multiArray: ids),
            "length": MLFeatureValue(multiArray: realLength),
        ])
        let output = try model.prediction(from: input)
        guard let scores = output.featureValue(for: "delete_logit")?.multiArrayValue,
              scores.count >= count else {
            throw TaggerError("The cleanup model answered fewer words than it was given.")
        }
        for position in 0..<count {
            logits[position] = scores[[0, NSNumber(value: position)]].floatValue
        }
        return true
    } catch {
        writeError(error, to: errorOutput)
        return false
    }
}

/// Copies one `[1, length, classes]` head into `destination`, row by row,
/// whatever its strides.
private func copyHead(
    _ output: MLFeatureProvider,
    _ name: String,
    length: Int,
    classes: Int,
    into destination: UnsafeMutablePointer<Float>
) throws {
    guard let scores = output.featureValue(for: name)?.multiArrayValue,
          scores.shape.count == 3,
          scores.shape[1].intValue == length,
          scores.shape[2].intValue == classes else {
        throw TaggerError("The cleanup model's \(name) has the wrong shape.")
    }
    let rowStride = scores.strides[1].intValue
    let classStride = scores.strides[2].intValue
    if scores.dataType == .float32 {
        scores.withUnsafeBufferPointer(ofType: Float.self) { buffer in
            for row in 0..<length {
                for column in 0..<classes {
                    destination[row * classes + column] = buffer[row * rowStride + column * classStride]
                }
            }
        }
    } else {
        for row in 0..<length {
            for column in 0..<classes {
                let index: [NSNumber] = [0, NSNumber(value: row), NSNumber(value: column)]
                destination[row * classes + column] = scores[index].floatValue
            }
        }
    }
}

/// Labs › Clean up what I say v2: one run of the four-head tagger over
/// `length` token ids, [CLS] ids [SEP], with nothing padded. Writes each
/// head's logits row by row: `action` holds `length` × 5 values,
/// `punct` × 8, `casing` × 4, and `span` × 11.
@_cdecl("hex_cleanup_tagger_v2_run")
public func hexCleanupTaggerV2Run(
    _ tagger: UnsafeMutableRawPointer?,
    _ inputIDs: UnsafePointer<Int32>?,
    _ length: Int32,
    _ action: UnsafeMutablePointer<Float>?,
    _ punct: UnsafeMutablePointer<Float>?,
    _ casing: UnsafeMutablePointer<Float>?,
    _ span: UnsafeMutablePointer<Float>?,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> Bool {
    guard let tagger, let inputIDs, let action, let punct, let casing, let span, length > 0 else {
        writeError(TaggerError("The cleanup model was given no words."), to: errorOutput)
        return false
    }
    let model = Unmanaged<Tagger>.fromOpaque(tagger).takeUnretainedValue().model
    let count = Int(length)
    do {
        let ids = try MLMultiArray(shape: [1, NSNumber(value: count)], dataType: .int32)
        let mask = try MLMultiArray(shape: [1, NSNumber(value: count)], dataType: .int32)
        ids.withUnsafeMutableBufferPointer(ofType: Int32.self) { buffer, strides in
            for position in 0..<count {
                buffer[position * strides[1]] = inputIDs[position]
            }
        }
        mask.withUnsafeMutableBufferPointer(ofType: Int32.self) { buffer, strides in
            for position in 0..<count {
                buffer[position * strides[1]] = 1
            }
        }
        let input = try MLDictionaryFeatureProvider(dictionary: [
            "input_ids": MLFeatureValue(multiArray: ids),
            "attention_mask": MLFeatureValue(multiArray: mask),
        ])
        let output = try model.prediction(from: input)
        try copyHead(output, "action_logits", length: count, classes: 5, into: action)
        try copyHead(output, "punct_logits", length: count, classes: 8, into: punct)
        try copyHead(output, "case_logits", length: count, classes: 4, into: casing)
        try copyHead(output, "span_logits", length: count, classes: 11, into: span)
        return true
    } catch {
        writeError(error, to: errorOutput)
        return false
    }
}

/// Frees a model `hex_cleanup_tagger_load` returned.
@_cdecl("hex_cleanup_tagger_release")
public func hexCleanupTaggerRelease(_ tagger: UnsafeMutableRawPointer?) {
    guard let tagger else { return }
    Unmanaged<Tagger>.fromOpaque(tagger).release()
}

@_cdecl("hex_cleanup_tagger_free_string")
public func hexCleanupTaggerFreeString(_ string: UnsafeMutablePointer<CChar>?) {
    free(string)
}
