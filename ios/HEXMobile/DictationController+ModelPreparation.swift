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
            if engineName != ParakeetUltraTranscriber.name, isModelReady, phase != .preparingModel {
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
        // A load beside the engine this replaces is dropped; the end of this one decides again.
        multilingualDownload.dropLoad()
        startMultilingualDownloadIfWanted()
        phase = .preparingModel
        copied = false
        modelPreparation = ModelPreparationProgress(
            fractionCompleted: 0,
            detail: String(localized: "Checking the speech model")
        )
        preparationFailedInBackground = false
        // A few more seconds from iOS when Hex goes to the background mid-download or mid-compile.
        let backgroundTime = BackgroundTime("Prepare speech model")

        Task {
            defer { backgroundTime.end() }
            var candidateIsUltra = false
            do {
                let candidate: any LocalTranscribing
                if let preset {
                    candidate = preset
                } else {
                    candidate = await transcriberFactory()
                }
                guard self.modelPrepGeneration == prepGen else { return }
                engineName = candidate.displayName
                candidateIsUltra = candidate is ParakeetUltraTranscriber
                try await candidate.prepare { [weak self] progress in
                    Task { @MainActor [weak self] in
                        guard let self, self.modelPrepGeneration == prepGen else { return }
                        self.modelPreparation = progress
                    }
                }
                guard self.modelPrepGeneration == prepGen else { return }
                self.languageOffer = nil
                self.transcriber = candidate
                self.transcriptionLanguage = await candidate.languageName
                self.transcriptionLanguageID = await candidate.languageIdentifier
                self.keyboardBridge.setModelInstalled(true)
                self.isModelReady = true
                self.phase = .ready
                if candidateIsUltra {
                    self.multilingualParakeetLoaded()
                }
                self.publishKeyboardSnapshot()
                if self.pendingHandoff != nil {
                    self.activateWarmSessionAndRunHandoff()
                }
                self.syncMultilingualParakeet()
            } catch {
                guard self.modelPrepGeneration == prepGen else { return }
                if candidateIsUltra, fallbacksLeft > 0, !(error is CancellationError) {
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
                self.keyboardBridge.setModelInstalled(ParakeetTranscriber.hasCachedModel)
                self.pendingHandoff = nil
                self.phase = .failed(UserFacingError.message(for: error, context: .model))
                self.publishKeyboardSnapshot()
                // A downloaded Parakeet Ultra can still dictate.
                self.syncMultilingualParakeet()
            }
        }
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
                self.engineName = candidate.displayName
                self.transcriptionLanguage = await candidate.languageName
                self.transcriptionLanguageID = await candidate.languageIdentifier
                self.languageOffer = nil
                self.isModelReady = true
                self.keyboardBridge.setModelInstalled(true)
                switch self.phase {
                case .modelRequired, .failed:
                    self.phase = .ready
                default:
                    break
                }
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
        multilingualDownload.appBecameActive()
        guard preparationFailedInBackground else { return }
        preparationFailedInBackground = false
        if case .failed = phase, !isModelReady {
            prepareModel()
        }
    }

    nonisolated static func isDownloadInterruption(_ error: Error) -> Bool {
        if error is StallWatch.Stalled || error is URLError { return true }
        return (error as? ParakeetTranscriber.TranscriberError) == .downloadFailed
    }
}
