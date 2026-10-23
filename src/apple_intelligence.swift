import Foundation
#if canImport(FoundationModels)
import FoundationModels
#endif

private struct BridgeNotes: Codable {
    let title: String
    let summary: String
}

private struct BridgeError: LocalizedError {
    let message: String

    init(_ message: String) {
        self.message = message
    }

    var errorDescription: String? { message }
}

private struct BridgeLiveNote: Codable {
    let kind: String
    let text: String
    let owner: String
    let time: String
}

private struct BridgeLiveNotes: Codable {
    let topic: String
    let notes: [BridgeLiveNote]
}

/// Distinguishes an oversized prompt so Rust can shorten the excerpt once.
private let contextWindowMarker = "context-window:"
/// A refused or guardrail-blocked transcript, which retrying won't change.
private let refusalMarker = "refused:"

#if canImport(FoundationModels)
@available(macOS 26.0, *)
@Generable
private struct GeneratedNotes {
    @Guide(description: "A short, specific meeting title of at most eight words, without dates or the word meeting")
    let title: String
    @Guide(description: "At most two short sentences on what the meeting covered and concluded")
    let summary: String
}

@available(macOS 26.0, *)
@Generable
private struct GeneratedTitle {
    @Guide(description: "A short, specific meeting title of at most eight words, without dates or the word meeting")
    let title: String
}

@available(macOS 26.0, *)
@Generable
private enum GeneratedNoteKind: String {
    case decision
    case action
    case question
    case point
}

@available(macOS 26.0, *)
@Generable
private struct GeneratedLiveNote {
    @Guide(description: "decision for something agreed, action for a task someone will do, question for something left open, point for a key fact")
    let kind: GeneratedNoteKind
    @Guide(description: "The note in at most twelve words")
    let text: String
    @Guide(description: "Who will do an action: You, Them, or a name said in the transcript; empty when unknown or not an action")
    let owner: String
    @Guide(description: "The mm:ss timestamp copied from the transcript line the note comes from")
    let time: String
}

@available(macOS 26.0, *)
@Generable
private struct GeneratedLiveNotes {
    @Guide(description: "A short, specific name of two to five words for what this part of the conversation is about, without the word meeting")
    let topic: String
    @Guide(description: "At most three notes worth keeping; none for small talk", .maximumCount(3))
    let notes: [GeneratedLiveNote]
}

@available(macOS 26.0, *)
private func generateLiveNotes(instructions: String, prompt: String) async throws -> BridgeLiveNotes {
    let model = SystemLanguageModel(
        useCase: .general,
        guardrails: .permissiveContentTransformations
    )
    let session = LanguageModelSession(model: model, instructions: instructions)
    do {
        let response = try await session.respond(
            to: prompt,
            generating: GeneratedLiveNotes.self,
            options: GenerationOptions(temperature: 0.2, maximumResponseTokens: 240)
        )
        let content = response.content
        return BridgeLiveNotes(
            topic: content.topic,
            notes: content.notes.map {
                BridgeLiveNote(kind: $0.kind.rawValue, text: $0.text, owner: $0.owner, time: $0.time)
            }
        )
    } catch LanguageModelSession.GenerationError.exceededContextWindowSize(let context) {
        throw BridgeError("\(contextWindowMarker) \(context.debugDescription)")
    } catch LanguageModelSession.GenerationError.refusal(_, let context) {
        throw BridgeError("\(refusalMarker) \(context.debugDescription)")
    } catch LanguageModelSession.GenerationError.guardrailViolation(let context) {
        throw BridgeError("\(refusalMarker) \(context.debugDescription)")
    }
}

@available(macOS 26.0, *)
private func generate(instructions: String, prompt: String, titleOnly: Bool) async throws -> BridgeNotes {
    let model = SystemLanguageModel(
        useCase: .general,
        guardrails: .permissiveContentTransformations
    )
    let session = LanguageModelSession(model: model, instructions: instructions)
    do {
        if titleOnly {
            let response = try await session.respond(
                to: prompt,
                generating: GeneratedTitle.self,
                options: GenerationOptions(temperature: 0.2, maximumResponseTokens: 60)
            )
            return BridgeNotes(title: response.content.title, summary: "")
        }
        let response = try await session.respond(
            to: prompt,
            generating: GeneratedNotes.self,
            options: GenerationOptions(temperature: 0.2, maximumResponseTokens: 220)
        )
        return BridgeNotes(title: response.content.title, summary: response.content.summary)
    } catch LanguageModelSession.GenerationError.exceededContextWindowSize(let context) {
        throw BridgeError("\(contextWindowMarker) \(context.debugDescription)")
    }
}
#endif

private final class BlockingResult<Value: Sendable>: @unchecked Sendable {
    private let lock = NSLock()
    private var value: Result<Value, Error>?

    func store(_ value: Result<Value, Error>) {
        lock.withLock { self.value = value }
    }

    func take() -> Result<Value, Error> {
        lock.withLock { value! }
    }
}

private func blocking<Value: Sendable>(
    timeout: TimeInterval,
    _ operation: @escaping @Sendable () async throws -> Value
) -> Result<Value, Error> {
    let semaphore = DispatchSemaphore(value: 0)
    let result = BlockingResult<Value>()
    let task = Task.detached(priority: .utility) {
        do {
            result.store(.success(try await operation()))
        } catch {
            result.store(.failure(error))
        }
        semaphore.signal()
    }
    guard semaphore.wait(timeout: .now() + timeout) == .success else {
        task.cancel()
        return .failure(BridgeError("Apple Intelligence timed out."))
    }
    return result.take()
}

private func writeError(
    _ error: Error,
    to destination: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) {
    destination?.pointee = strdup(
        (error as? LocalizedError)?.errorDescription ?? error.localizedDescription
    )
}

/// 0 available, 1 device not eligible, 2 Apple Intelligence off,
/// 3 model not ready, 4 unsupported macOS, 5 unknown reason.
@_cdecl("hex_apple_intelligence_availability")
public func hexAppleIntelligenceAvailability() -> Int32 {
    #if canImport(FoundationModels)
    guard #available(macOS 26.0, *) else { return 4 }
    switch SystemLanguageModel.default.availability {
    case .available:
        return 0
    case .unavailable(.deviceNotEligible):
        return 1
    case .unavailable(.appleIntelligenceNotEnabled):
        return 2
    case .unavailable(.modelNotReady):
        return 3
    case .unavailable:
        return 5
    }
    #else
    return 4
    #endif
}

@_cdecl("hex_apple_intelligence_context_size")
public func hexAppleIntelligenceContextSize() -> Int {
    #if canImport(FoundationModels)
    guard #available(macOS 26.0, *) else { return 0 }
    return SystemLanguageModel.default.contextSize
    #else
    return 0
    #endif
}

@_cdecl("hex_apple_intelligence_notes")
public func hexAppleIntelligenceNotes(
    _ instructions: UnsafePointer<CChar>?,
    _ prompt: UnsafePointer<CChar>?,
    _ titleOnly: Bool,
    _ timeoutMs: UInt64,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutablePointer<CChar>? {
    #if canImport(FoundationModels)
    guard #available(macOS 26.0, *), let instructions, let prompt else {
        writeError(BridgeError("Apple Intelligence requires macOS 26."), to: errorOutput)
        return nil
    }
    let instructionText = String(cString: instructions)
    let promptText = String(cString: prompt)
    let result = blocking(timeout: TimeInterval(timeoutMs) / 1_000) {
        try await generate(instructions: instructionText, prompt: promptText, titleOnly: titleOnly)
    }
    switch result {
    case .success(let notes):
        do {
            let data = try JSONEncoder().encode(notes)
            return strdup(String(decoding: data, as: UTF8.self))
        } catch {
            writeError(error, to: errorOutput)
            return nil
        }
    case .failure(let failure):
        writeError(failure, to: errorOutput)
        return nil
    }
    #else
    writeError(BridgeError("Apple Intelligence requires macOS 26."), to: errorOutput)
    return nil
    #endif
}

@_cdecl("hex_apple_intelligence_live_notes")
public func hexAppleIntelligenceLiveNotes(
    _ instructions: UnsafePointer<CChar>?,
    _ prompt: UnsafePointer<CChar>?,
    _ timeoutMs: UInt64,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutablePointer<CChar>? {
    #if canImport(FoundationModels)
    guard #available(macOS 26.0, *), let instructions, let prompt else {
        writeError(BridgeError("Apple Intelligence requires macOS 26."), to: errorOutput)
        return nil
    }
    let instructionText = String(cString: instructions)
    let promptText = String(cString: prompt)
    let result = blocking(timeout: TimeInterval(timeoutMs) / 1_000) {
        try await generateLiveNotes(instructions: instructionText, prompt: promptText)
    }
    switch result {
    case .success(let notes):
        do {
            let data = try JSONEncoder().encode(notes)
            return strdup(String(decoding: data, as: UTF8.self))
        } catch {
            writeError(error, to: errorOutput)
            return nil
        }
    case .failure(let failure):
        writeError(failure, to: errorOutput)
        return nil
    }
    #else
    writeError(BridgeError("Apple Intelligence requires macOS 26."), to: errorOutput)
    return nil
    #endif
}

@_cdecl("hex_apple_intelligence_free_string")
public func hexAppleIntelligenceFreeString(_ string: UnsafeMutablePointer<CChar>?) {
    free(string)
}
