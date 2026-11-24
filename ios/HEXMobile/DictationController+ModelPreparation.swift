import Foundation

/// Getting the engine Hex dictates with ready: `prepareModel`, and Experiments ›
/// Multilingual model (Parakeet) downloading and loading beside the engine that dictates
/// meanwhile, including what the app does with it in the background.
extension DictationController {
    // MARK: - Model Preparation

    /// Experiments › Multilingual model (Parakeet) turned on or off. On, the engine Hex
    /// dictates with stays while Parakeet Ultra downloads and loads beside it; a load that
    /// ended Hex gets another try. Off, the download stops and is deleted, and Hex prepares
    /// the engine it picks without it if Ultra was the one dictating (now, or once the
    /// dictation in flight is done).
    func multilingualParakeetChanged() {
        let settings = MultilingualParakeetSettings()
        if settings.isEnabled {
            ParakeetLoadGuard(model: ParakeetUltraTranscriber.loadGuardModel).forget()
            multilingualDownload.settle()
            if isModelReady || phase == .preparingModel {
                syncMultilingualParakeet()
                return
            }
        } else {
            settings.isModelReady = false
            let download = multilingualDownload
            Task {
                // Nothing writes to the folder any more once `stop` returns.
                await download.stop()
                ParakeetUltraModel.remove()
            }
            if engine != .parakeetUltra, isModelReady, phase != .preparingModel {
                // Ultra wasn't dictating: drop a load of it beside the engine that is.
                modelPrepGeneration &+= 1
                return
            }
        }
        switch phase {
        case .recording, .transcribing, .requestingPermission:
            pendingLanguagePreparation = true
        case .modelRequired, .preparingModel, .ready, .failed:
            prepareModel()
        }
    }

    /// `fallbacksLeft` bounds how often Automatic moves on to another language after one
    /// whose model won't install here; each move sets a language aside, so it can't loop.
    func prepareModel(fallbacksLeft: Int = 2, candidate preset: (any LocalTranscribing)? = nil) {
        modelPrepGeneration &+= 1
        let prepGen = modelPrepGeneration
        // The preparation this replaces may still be inside a Speech call: cancel it so its work
        // stops instead of piling up behind the new one.
        preparationTask?.cancel()
        // A load beside the engine this replaces is dropped; the end of this one decides again.
        multilingualDownload.dropLoad()
        startMultilingualDownloadIfWanted()
        transition(model: .preparing)
        copied = false
        preparingMultilingualParakeet = false
        modelPreparation = ModelPreparationProgress(
            fractionCompleted: 0,
            detail: String(localized: "Checking the speech model")
        )
        preparationFailedInBackground = false
        watchPreparation(prepGen)
        // A few more seconds from iOS when Hex goes to the background mid-download or mid-compile.
        let backgroundTime = BackgroundTime("Prepare speech model")

        preparationTask = Task {
            defer {
                backgroundTime.end()
                if self.modelPrepGeneration == prepGen { self.preparationTask = nil }
            }
            do {
                let candidate = try await self.prepareCandidate(preset, generation: prepGen)
                guard self.modelPrepGeneration == prepGen else { return }
                self.languageOffer = nil
                self.transcriber = candidate
                self.transcriptionLanguage = await candidate.languageName
                self.transcriptionLanguageID = await candidate.languageIdentifier
                self.keyboardBridge.setModelInstalled(true)
                self.isModelReady = true
                self.transition(model: .ready)
                if self.preparingMultilingualParakeet {
                    self.multilingualParakeetLoaded()
                }
                self.publishKeyboardSnapshot()
                if self.pendingHandoff != nil {
                    self.activateWarmSessionAndRunHandoff()
                }
                self.syncMultilingualParakeet()
            } catch {
                guard self.modelPrepGeneration == prepGen else { return }
                if self.preparingMultilingualParakeet, fallbacksLeft > 0, !(error is CancellationError) {
                    // Parakeet Ultra didn't load: the engine Hex uses without it dictates, and
                    // Hex says why, with Try again.
                    self.multilingualParakeetFailedToLoad(error)
                    self.prepareModel(fallbacksLeft: fallbacksLeft - 1)
                    return
                }
                let unavailable = error as? SpeechModelUnavailable
                if fallbacksLeft > 0, let notice = Self.switchNotice(for: error) {
                    // Automatic set the language aside, or Parakeet had no room: prepare what
                    // Hex picks now.
                    self.languageNotice = notice
                    self.prepareModel(fallbacksLeft: fallbacksLeft - 1)
                    return
                }
                self.languageOffer = unavailable.flatMap { $0.automatic ? nil : $0.fallback }
                self.preparationFailedInBackground = self.isInBackground && Self.isDownloadInterruption(error)
                // This only differs from Parakeet's cache in one case: a re-preparation that
                // fails after an earlier success, where the engine that prepared before is still
                // loaded and can dictate. A first failure on Apple Speech has no model to keep.
                self.keyboardBridge.setModelInstalled(self.isModelReady || ParakeetTranscriber.hasCachedModel)
                self.pendingHandoff = nil
                self.failModel(UserFacingError.message(for: error, context: .model))
                self.publishKeyboardSnapshot()
                // A downloaded Parakeet Ultra can still dictate.
                self.syncMultilingualParakeet()
            }
        }
    }

    /// Ends a preparation that has said nothing new for too long, in plain words with Try
    /// again (Set up), instead of a bar that never moves: `firstReport` (30 s) before it has
    /// said anything (Apple's speech service or asset store not answering leaves "Checking
    /// the speech model" up for good, TestFlight build 126), `stall` (120 s) after, longer
    /// than the downloads' own stall limits. Time while iOS had Hex suspended doesn't count.
    /// The work it ends can't always be cancelled (an XPC call that never returns); its
    /// result is dropped, and Try again starts a new one.
    func watchPreparation(_ prepGen: UInt64) {
        preparationChangedAt = .now
        preparationReported = false
        Task { [weak self] in
            let clock = ContinuousClock()
            var polled = clock.now
            while true {
                try? await Task.sleep(for: .milliseconds(500))
                guard let self, self.modelPrepGeneration == prepGen, self.phase == .preparingModel else { return }
                let now = clock.now
                if now - polled > .seconds(5) {
                    self.preparationChangedAt = now
                }
                polled = now
                let limit = self.preparationReported ? self.preparationLimits.stall : self.preparationLimits.firstReport
                guard now - self.preparationChangedAt >= limit else { continue }
                print("speech model preparation said nothing new for \(limit): ended")
                self.modelPrepGeneration &+= 1
                self.preparationTask?.cancel()
                self.preparationTask = nil
                self.preparationFailedInBackground = self.isInBackground
                self.pendingHandoff = nil
                self.keyboardBridge.setModelInstalled(self.isModelReady || ParakeetTranscriber.hasCachedModel)
                self.failModel(String(localized: "The speech model isn't ready. Try again."))
                self.publishKeyboardSnapshot()
                return
            }
        }
    }

    /// Resolves the engine and gets it ready, naming it before its model loads so the window
    /// and the keyboard already say which one is coming. Throws `CancellationError` when a newer
    /// preparation replaced this one before it picked.
    private func prepareCandidate(
        _ preset: (any LocalTranscribing)?,
        generation: UInt64
    ) async throws -> any LocalTranscribing {
        let candidate: any LocalTranscribing
        if let preset {
            candidate = preset
        } else {
            candidate = await transcriberFactory()
        }
        guard modelPrepGeneration == generation else { throw CancellationError() }
        try Task.checkCancellation()
        engine = candidate.engine
        preparingMultilingualParakeet = candidate is ParakeetUltraTranscriber
        try await candidate.prepare { [weak self] progress in
            Task { @MainActor [weak self] in
                guard let self, self.modelPrepGeneration == generation else { return }
                if progress.fractionCompleted != self.modelPreparation.fractionCompleted
                    || progress.detail != self.modelPreparation.detail {
                    self.preparationChangedAt = .now
                    self.preparationReported = true
                }
                self.modelPreparation = progress
            }
        }
        return candidate
    }

    /// Parakeet V2 dictates and leaves no room to load Parakeet Ultra beside it (English
    /// before iOS 26): Ultra takes its place and loads as it would at launch, rather than
    /// never getting its first load. False when V2 isn't the engine.
    func replaceParakeetWithMultilingual(language: String) -> Bool {
        guard transcriber is ParakeetTranscriber, phase != .preparingModel else { return false }
        transcriber = ParakeetTranscriber()
        isModelReady = false
        prepareModel(candidate: ParakeetUltraTranscriber(language: language))
        return true
    }

    /// Loads Parakeet Ultra while the engine Hex has keeps dictating, and switches to it once
    /// it's loaded; a dictation in flight isn't interrupted. A failure keeps that engine and
    /// says why. Only while no other preparation runs.
    func loadMultilingualParakeetBeside(language: String) {
        guard phase != .preparingModel else { return }
        modelPrepGeneration &+= 1
        let prepGen = modelPrepGeneration
        multilingualDownload.loading()
        let backgroundTime = BackgroundTime("Prepare the multilingual model")
        Task {
            defer { backgroundTime.end() }
            let candidate = ParakeetUltraTranscriber(language: language)
            do {
                try await candidate.prepare { _ in }
                guard self.modelPrepGeneration == prepGen else { return }
                self.transcriber = candidate
                self.engine = candidate.engine
                self.transcriptionLanguage = await candidate.languageName
                self.transcriptionLanguageID = await candidate.languageIdentifier
                self.languageOffer = nil
                self.isModelReady = true
                self.keyboardBridge.setModelInstalled(true)
                self.transition(model: .ready)
                self.multilingualParakeetLoaded()
                self.publishKeyboardSnapshot()
            } catch {
                guard self.modelPrepGeneration == prepGen else { return }
                self.multilingualParakeetFailedToLoad(error)
            }
        }
    }

    func appMovedToBackground() {
        isInBackground = true
        multilingualDownload.appMovedToBackground()
    }

    /// Picks up a download that iOS cut off while Hex was in the background; FluidAudio
    /// resumes each file from the bytes it has.
    func appBecameActive() {
        isInBackground = false
        refreshWarmDuration()
        multilingualDownload.appBecameActive()
        guard preparationFailedInBackground else { return }
        preparationFailedInBackground = false
        if case .failed = phase, !isModelReady {
            prepareModel()
        }
    }

    nonisolated static func isDownloadInterruption(_ error: Error) -> Bool {
        if error is StallWatch.Stalled || error is URLError || error is SpeechModelUnresponsive {
            return true
        }
        return (error as? ParakeetTranscriber.TranscriberError) == .downloadFailed
    }
}
