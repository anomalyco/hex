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

private struct BridgeChapter: Codable {
    let title: String
    let summary: String
    let same_topic: Bool
}

private struct BridgeChapterLines: Codable {
    let lines: [Int]
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

/// Field order is generation order: the new lines are named on their own
/// before the model compares them with the current chapter.
@available(macOS 26.0, *)
@Generable
private struct GeneratedChapter {
    @Guide(description: "A short name of two to five words for what the new lines are about, lowercase except names, without the word meeting")
    let title: String
    @Guide(description: "One short sentence on what the new lines cover")
    let summary: String
    @Guide(description: "true when the new lines are still about the current chapter, false when the conversation moved to a different topic")
    let sameTopic: Bool
}

@available(macOS 26.0, *)
@Generable
private struct GeneratedChapterLines {
    @Guide(description: "The L numbers of every line about the chapter after, in order")
    let lines: [Int]
}

@available(macOS 26.0, *)
private func respond<Content: Generable>(
    instructions: String,
    prompt: String,
    generating type: Content.Type,
    maximumResponseTokens: Int
) async throws -> Content {
    let model = SystemLanguageModel(
        useCase: .general,
        guardrails: .permissiveContentTransformations
    )
    let session = LanguageModelSession(model: model, instructions: instructions)
    do {
        return try await session.respond(
            to: prompt,
            generating: type,
            options: GenerationOptions(temperature: 0.2, maximumResponseTokens: maximumResponseTokens)
        ).content
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

/// Runs one guided generation and returns its JSON, or nil with `errorOutput` set.
private func bridgeCall<Value: Encodable & Sendable>(
    _ instructions: UnsafePointer<CChar>?,
    _ prompt: UnsafePointer<CChar>?,
    _ timeoutMs: UInt64,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?,
    _ operation: @escaping @Sendable (String, String) async throws -> Value
) -> UnsafeMutablePointer<CChar>? {
    guard let instructions, let prompt else {
        writeError(BridgeError("Apple Intelligence requires macOS 26."), to: errorOutput)
        return nil
    }
    let instructionText = String(cString: instructions)
    let promptText = String(cString: prompt)
    let result = blocking(timeout: TimeInterval(timeoutMs) / 1_000) {
        try await operation(instructionText, promptText)
    }
    switch result {
    case .success(let value):
        do {
            let data = try JSONEncoder().encode(value)
            return strdup(String(decoding: data, as: UTF8.self))
        } catch {
            writeError(error, to: errorOutput)
            return nil
        }
    case .failure(let failure):
        writeError(failure, to: errorOutput)
        return nil
    }
}

@_cdecl("hex_apple_intelligence_chapter")
public func hexAppleIntelligenceChapter(
    _ instructions: UnsafePointer<CChar>?,
    _ prompt: UnsafePointer<CChar>?,
    _ timeoutMs: UInt64,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutablePointer<CChar>? {
    #if canImport(FoundationModels)
    guard #available(macOS 26.0, *) else {
        writeError(BridgeError("Apple Intelligence requires macOS 26."), to: errorOutput)
        return nil
    }
    return bridgeCall(instructions, prompt, timeoutMs, errorOutput) { instructions, prompt in
        let chapter = try await respond(
            instructions: instructions,
            prompt: prompt,
            generating: GeneratedChapter.self,
            maximumResponseTokens: 120
        )
        return BridgeChapter(
            title: chapter.title,
            summary: chapter.summary,
            same_topic: chapter.sameTopic
        )
    }
    #else
    writeError(BridgeError("Apple Intelligence requires macOS 26."), to: errorOutput)
    return nil
    #endif
}

@_cdecl("hex_apple_intelligence_chapter_lines")
public func hexAppleIntelligenceChapterLines(
    _ instructions: UnsafePointer<CChar>?,
    _ prompt: UnsafePointer<CChar>?,
    _ timeoutMs: UInt64,
    _ errorOutput: UnsafeMutablePointer<UnsafeMutablePointer<CChar>?>?
) -> UnsafeMutablePointer<CChar>? {
    #if canImport(FoundationModels)
    guard #available(macOS 26.0, *) else {
        writeError(BridgeError("Apple Intelligence requires macOS 26."), to: errorOutput)
        return nil
    }
    return bridgeCall(instructions, prompt, timeoutMs, errorOutput) { instructions, prompt in
        let lines = try await respond(
            instructions: instructions,
            prompt: prompt,
            generating: GeneratedChapterLines.self,
            maximumResponseTokens: 240
        )
        return BridgeChapterLines(lines: lines.lines)
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
