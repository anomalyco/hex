import CoreML
import Darwin
import FluidAudio
import Foundation

/// Experiments › Speech model in keyboard (on unless turned off): the keyboard's own dictation
/// transcribed with Parakeet on the Neural Engine, in the keyboard's process, so Hex needn't be
/// open or warm.
/// English uses Parakeet V2; a language Experiments › Multilingual model dictates uses Parakeet
/// Ultra. Both load from the App Group (`SpeechModelStore`), after the keyboard's first frame
/// and launch work (`warmUp`), and stay loaded while the keyboard's process lives, until a
/// memory warning. The first load compiles the model for the Neural Engine (17 s for V2, 12 s
/// for Ultra on an iPhone 17 Pro); dictation never waits for it. Whenever the experiment is off
/// or the model isn't loaded, on the Neural Engine and within the keyboard's memory
/// (`KeyboardParakeetBudget`), `KeyboardTranscription` uses Apple's speech recognition,
/// exactly as with the experiment off.
@MainActor
final class KeyboardParakeet {
    static let shared = KeyboardParakeet()

    struct Choice: Equatable {
        let multilingual: Bool
        /// BCP 47: the language the text goes through Hex's steps in.
        let language: String

        var directory: URL { multilingual ? SpeechModelStore.parakeetUltra : SpeechModelStore.parakeetV2 }
        var engineName: String { multilingual ? "Parakeet Ultra (keyboard)" : "Parakeet V2 (keyboard)" }
        var label: String { multilingual ? "parakeet-ultra" : "parakeet-v2" }
    }

    struct Transcript {
        let text: String
        let language: SpeechLanguage
        let engine: String
    }

    private struct Loaded {
        let multilingual: Bool
        let manager: AsrManager
        let seconds: Double
        let cold: Bool
        let at: Date
    }

    private var loaded: Loaded?
    private var loadTask: Task<Void, Never>?
    /// Models that didn't load or didn't run on the Neural Engine in this process.
    private var setAside: Set<Bool> = []
    private var memoryWarnedAt: Date?
    private var isTranscribing = false
    private var releasesAfterTranscription = false
    private var lastNote = ""

    /// iOS ends nothing for a load past this; the keyboard stops saying it's getting ready.
    static let loadPatience: TimeInterval = 180
    /// Evidence the model is on the Neural Engine: the keyboard's Neural Engine memory, which
    /// rose 468 MB (V2) and 607 MB (Ultra) on the iPhone 17 Pro.
    static let neuralEngineEvidence = 100 * KeyboardParakeetBudget.megabyte
    /// After a memory warning, no new load in this process for this long.
    static let reloadAfterWarning: TimeInterval = 300
    /// A load this long compiled the model: on an iPhone 17 Pro a load Core ML had compiled
    /// before took 0.3 s, one that compiled 11.6 s (Ultra) to 16.5 s (V2).
    static let compilingLoad: TimeInterval = 5

    /// The model the keyboard would transcribe with, or nil for Apple's speech recognition:
    /// the experiment on, an iPhone with the memory, not Two-language dictation, and the
    /// model's files complete in the App Group.
    nonisolated static func choice() -> Choice? {
        guard KeyboardParakeetSettings().isEnabled, KeyboardParakeetSettings.fits(),
              !TwoLanguageDictationSettings().isEnabled else { return nil }
        if let language = MultilingualParakeetSettings().language(),
           SpeechModelStore.isComplete(SpeechModelStore.parakeetUltra) {
            return Choice(multilingual: true, language: language)
        }
        let languages = LanguageSettings()
        guard let wanted = languages.dictationLanguage ?? languages.keyboardDictationLanguage() ?? Locale.preferredLanguages.first,
              Locale(identifier: wanted).language.languageCode?.identifier == "en",
              SpeechModelStore.isComplete(SpeechModelStore.parakeetV2) else { return nil }
        return Choice(multilingual: false, language: wanted)
    }

    /// Whether the keyboard dictates the multilingual model's language itself now, so the Hex
    /// key needn't open Hex for it.
    var dictatesMultilingualHere: Bool {
        guard let loaded, loaded.multilingual else { return false }
        return Self.choice()?.multilingual == true
    }

    /// After the keyboard's first frame and launch work: notes the keyboard's memory limit for
    /// Hex's download, then loads the model it would use, off the main thread, when it fits.
    func warmUp() {
        let measured = Self.memory()
        if measured.available > 0 {
            KeyboardParakeetSettings().recordKeyboardLimit(measured.footprint + measured.available)
        }
        guard let choice = Self.choice() else { return }
        if loaded?.multilingual == choice.multilingual { return }
        guard loadTask == nil, !setAside.contains(choice.multilingual) else { return }
        if let warned = memoryWarnedAt, Date().timeIntervalSince(warned) < Self.reloadAfterWarning { return }
        let memory = Self.memory()
        let settings = KeyboardParakeetSettings()
        guard KeyboardParakeetBudget.allowsLoad(footprint: memory.footprint, available: memory.available) else {
            note("parakeet: \(choice.label) not loaded, short of memory \(memory)")
            settings.setKeyboardState(.unavailable)
            return
        }
        guard Self.hasNeuralEngine else {
            note("parakeet: no Neural Engine here")
            setAside.insert(choice.multilingual)
            settings.setKeyboardState(.unavailable)
            return
        }
        if loaded != nil { release(reason: "language changed") }
        settings.setKeyboardState(.preparing)
        KeyboardProbeLog.shared.write("parakeet: loading \(choice.label) \(memory)")
        let started = Date()
        loadTask = Task { [weak self] in
            let neuralBefore = Self.memory().neural
            do {
                let manager = try await Self.load(choice)
                let after = Self.memory()
                let onEngine: Bool
                if after.neural - neuralBefore >= Self.neuralEngineEvidence {
                    onEngine = true
                } else {
                    // Before iOS 27 the Neural Engine's memory isn't the process's: ask Core ML.
                    onEngine = after.neural == 0 ? await Self.plansNeuralEngine(choice) : false
                }
                guard let self else { return }
                self.loadTask = nil
                let seconds = Date().timeIntervalSince(started)
                guard onEngine else {
                    await manager.cleanup()
                    self.setAside.insert(choice.multilingual)
                    settings.setKeyboardState(.unavailable)
                    KeyboardProbeLog.shared.write("parakeet: \(choice.label) not on the Neural Engine, Apple's speech recognition \(Self.memory())")
                    return
                }
                let cold = seconds >= Self.compilingLoad
                self.loaded = Loaded(multilingual: choice.multilingual, manager: manager, seconds: seconds, cold: cold, at: Date())
                settings.setKeyboardState(.ready)
                KeyboardProbeLog.shared.write(
                    "parakeet: \(choice.label) loaded in \(String(format: "%.1f", seconds)) s (\(cold ? "cold" : "warm")) \(after)"
                )
            } catch {
                guard let self else { return }
                self.loadTask = nil
                self.setAside.insert(choice.multilingual)
                settings.setKeyboardState(.unavailable)
                KeyboardProbeLog.shared.write("parakeet: \(choice.label) didn't load: \(error) \(Self.memory())")
            }
        }
        Task { [weak self] in
            try? await Task.sleep(for: .seconds(Self.loadPatience))
            guard let self, self.loadTask != nil, self.loaded == nil else { return }
            KeyboardParakeetSettings().setKeyboardState(.unavailable)
            KeyboardProbeLog.shared.write("parakeet: \(choice.label) still loading after \(Int(Self.loadPatience)) s")
        }
    }

    /// Transcribes `url` with the loaded model, or returns nil for Apple's speech recognition
    /// (with the reason in the probe log): the experiment off, the model not loaded yet, short
    /// of memory, another transcription running, nothing heard, or an error.
    func transcribe(_ url: URL, audioSeconds: Double) async -> Transcript? {
        guard let choice = Self.choice() else { return nil }
        let before = Self.memory()
        func fallback(_ reason: String) -> Transcript? {
            KeyboardProbeLog.shared.write("parakeet: dictation engine=apple (\(reason)) audio=\(String(format: "%.1f", audioSeconds)) s \(before)")
            return nil
        }
        guard let loaded, loaded.multilingual == choice.multilingual else {
            return fallback(loadTask != nil ? "\(choice.label) still loading" : "\(choice.label) not loaded")
        }
        guard !isTranscribing else { return fallback("another transcription running") }
        guard KeyboardParakeetBudget.allowsTranscription(footprint: before.footprint, available: before.available) else {
            return fallback("short of memory")
        }
        isTranscribing = true
        defer {
            isTranscribing = false
            if releasesAfterTranscription {
                releasesAfterTranscription = false
                release(reason: "memory warning")
            }
        }
        let sampler = FootprintPeak()
        sampler.start()
        let started = Date()
        do {
            let manager = loaded.manager
            let layers = await manager.decoderLayerCount
            var state = TdtDecoderState.make(decoderLayers: layers)
            let result = try await manager.transcribe(url, decoderState: &state)
            let seconds = Date().timeIntervalSince(started)
            let peak = sampler.stop()
            let text = result.text.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !text.isEmpty else { return fallback("\(choice.label) heard nothing") }
            KeyboardProbeLog.shared.write(String(
                format: "parakeet: dictation engine=%@ limit=%.0f MB footprint before=%.1f MB peak=%.1f MB load=%.1f s (%@, %.0f s ago) transcribe=%.2f s audio=%.1f s neural=%.0f MB",
                choice.label, Self.mb(before.footprint + before.available), Self.mb(before.footprint), Self.mb(peak),
                loaded.seconds, loaded.cold ? "cold" : "warm", Date().timeIntervalSince(loaded.at), seconds, audioSeconds,
                Self.mb(Self.memory().neural)
            ))
            return Transcript(
                text: text,
                language: SpeechLanguage(locale: Locale(identifier: choice.language), module: .speech),
                engine: choice.engineName
            )
        } catch {
            _ = sampler.stop()
            return fallback("\(choice.label) failed: \(error)")
        }
    }

    /// iOS warned the keyboard about memory: the model goes, now or once its transcription
    /// ends, and no new load starts in this process for `reloadAfterWarning`.
    func handleMemoryWarning() {
        memoryWarnedAt = Date()
        guard loaded != nil else { return }
        if isTranscribing {
            releasesAfterTranscription = true
        } else {
            release(reason: "memory warning")
        }
    }

    private func release(reason: String) {
        guard let current = loaded else { return }
        loaded = nil
        Task { await current.manager.cleanup() }
        if KeyboardParakeetSettings().isEnabled {
            KeyboardParakeetSettings().setKeyboardState(nil)
        }
        KeyboardProbeLog.shared.write("parakeet: released (\(reason)) \(Self.memory())")
    }

    private func note(_ message: String) {
        guard message != lastNote else { return }
        lastNote = message
        KeyboardProbeLog.shared.write(message)
    }

    // MARK: - Loading

    /// The four models as the app loads Parakeet (the preprocessor on the CPU, the rest on the
    /// CPU and Neural Engine, never the GPU, which a keyboard may only use in the background
    /// role), off the main thread; one chunk at a time.
    nonisolated private static func load(_ choice: Choice) async throws -> AsrManager {
        let models = try await Task.detached(priority: .utility) { try loadModels(choice) }.value
        let manager = AsrManager(config: ASRConfig(parallelChunkConcurrency: 1))
        try await manager.loadModels(models)
        return manager
    }

    nonisolated private static func loadModels(_ choice: Choice) throws -> AsrModels {
        let directory = choice.directory
        func model(_ name: String, _ units: MLComputeUnits) throws -> MLModel {
            let configuration = AsrModels.defaultConfiguration()
            configuration.computeUnits = units
            return try MLModel(contentsOf: directory.appendingPathComponent(name), configuration: configuration)
        }
        let preprocessor = try model(ModelNames.ASR.preprocessorFile, .cpuOnly)
        let encoder = try model(ModelNames.ASR.encoderFile, .cpuAndNeuralEngine)
        let decoder = try model(ModelNames.ASR.decoderFile, .cpuAndNeuralEngine)
        let joint = try model(choice.multilingual ? ModelNames.ASR.jointV3File : ModelNames.ASR.jointFile, .cpuAndNeuralEngine)
        let data = try Data(contentsOf: directory.appendingPathComponent(ModelNames.ASR.vocabularyFile))
        var vocabulary: [Int: String] = [:]
        for (key, token) in (try JSONSerialization.jsonObject(with: data) as? [String: String]) ?? [:] {
            if let id = Int(key) { vocabulary[id] = token }
        }
        let configuration = AsrModels.defaultConfiguration()
        return AsrModels(
            encoder: encoder,
            preprocessor: preprocessor,
            decoder: decoder,
            joint: joint,
            configuration: configuration,
            vocabulary: vocabulary,
            version: choice.multilingual ? .v3 : .v2
        )
    }

    nonisolated private static var hasNeuralEngine: Bool {
        MLModel.availableComputeDevices.contains { device in
            if case .neuralEngine = device { return true }
            return false
        }
    }

    /// Whether Core ML puts most of the encoder's work on the Neural Engine.
    nonisolated private static func plansNeuralEngine(_ choice: Choice) async -> Bool {
        let configuration = AsrModels.defaultConfiguration()
        configuration.computeUnits = .cpuAndNeuralEngine
        guard let plan = try? await MLComputePlan.load(
            contentsOf: choice.directory.appendingPathComponent(ModelNames.ASR.encoderFile),
            configuration: configuration
        ), case .program(let program) = plan.modelStructure else { return false }
        var neural = 0.0
        var total = 0.0
        func walk(_ block: MLModelStructure.Program.Block) {
            for operation in block.operations {
                let weight = plan.estimatedCost(of: operation)?.weight ?? 0
                total += weight
                if case .neuralEngine = plan.deviceUsage(for: operation)?.preferred { neural += weight }
                operation.blocks.forEach(walk)
            }
        }
        program.functions.values.forEach { walk($0.block) }
        return total > 0 && neural / total >= 0.5
    }

    // MARK: - Memory

    struct Memory: CustomStringConvertible {
        let footprint: Int
        let available: Int
        /// The Neural Engine's memory for this process, outside the footprint iOS limits.
        let neural: Int

        var description: String {
            String(
                format: "[limit %.0f MB, footprint %.1f MB, available %.1f MB, neural %.0f MB]",
                KeyboardParakeet.mb(footprint + available), KeyboardParakeet.mb(footprint),
                KeyboardParakeet.mb(available), KeyboardParakeet.mb(neural)
            )
        }
    }

    nonisolated static func memory() -> Memory {
        var info = task_vm_info_data_t()
        var count = mach_msg_type_number_t(MemoryLayout<task_vm_info_data_t>.size / MemoryLayout<natural_t>.size)
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: integer_t.self, capacity: Int(count)) {
                task_info(mach_task_self_, task_flavor_t(TASK_VM_INFO), $0, &count)
            }
        }
        guard result == KERN_SUCCESS else { return Memory(footprint: 0, available: os_proc_available_memory(), neural: 0) }
        return Memory(
            footprint: Int(info.phys_footprint),
            available: os_proc_available_memory(),
            neural: Int(info.ledger_tag_neural_nofootprint + info.ledger_tag_neural_nofootprint_compressed)
        )
    }

    nonisolated static func mb(_ bytes: Int) -> Double {
        Double(bytes) / Double(KeyboardParakeetBudget.megabyte)
    }
}

/// The highest footprint while a transcription runs, sampled every 20 ms.
private nonisolated final class FootprintPeak: @unchecked Sendable {
    private let lock = NSLock()
    private var peak = 0
    private var timer: DispatchSourceTimer?

    func start() {
        let timer = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
        timer.schedule(deadline: .now(), repeating: .milliseconds(20), leeway: .milliseconds(5))
        timer.setEventHandler { [weak self] in
            self?.record(KeyboardParakeet.memory().footprint)
        }
        timer.resume()
        self.timer = timer
    }

    private func record(_ footprint: Int) {
        lock.withLock { peak = max(peak, footprint) }
    }

    func stop() -> Int {
        timer?.cancel()
        timer = nil
        record(KeyboardParakeet.memory().footprint)
        return lock.withLock { peak }
    }
}

/// The keyboard's transcription: Parakeet where `KeyboardParakeet` can, else Apple's speech
/// recognition exactly as before (`KeyboardSpeech`). Returns the engine's name with the text.
@available(iOS 26.0, *)
enum KeyboardTranscription {
    static func transcribe(_ url: URL, hints: [String], audioSeconds: Double) async throws -> (String, SpeechLanguage, String) {
        if let parakeet = await KeyboardParakeet.shared.transcribe(url, audioSeconds: audioSeconds) {
            return (parakeet.text, parakeet.language, parakeet.engine)
        }
        let started = Date()
        let (text, language) = try await KeyboardSpeech.transcribe(url, hints: hints)
        if KeyboardParakeetSettings().isEnabled {
            KeyboardProbeLog.shared.write(String(
                format: "parakeet: dictation engine=apple transcribe=%.2f s audio=%.1f s %@",
                Date().timeIntervalSince(started), audioSeconds, KeyboardParakeet.memory().description
            ))
        }
        let engine = language.module == .dictation ? "Apple Speech (keyboard, DictationTranscriber)" : "Apple Speech (keyboard)"
        return (text, language, engine)
    }
}
