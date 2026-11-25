import SwiftUI
import UIKit

/// Where the speech model stands, as onboarding shows it.
struct OnboardingModel: Equatable {
    enum Stage: Equatable {
        case starting
        case downloading(Double)
        case ready
        case failed(String)
    }

    var stage: Stage
    /// The language it's for, in Hex's interface language ("English").
    var language: String
    /// What Hex's own model is doing ("Optimizing for this iPhone, about a minute"), while it
    /// prepares.
    var detail: String?
    /// Whether Hex downloads a model for it: not for Apple's on-device recognizer before iOS 26,
    /// which only needs Speech Recognition allowed.
    var downloads = true
    /// Whether the keyboard has its own Apple speech model ready, apart from Hex's: where Hex
    /// dictates with its own model (Parakeet), the keyboard still dictates with Apple's speech
    /// recognition if only Hex's download failed. Where Hex dictates with Apple's, the
    /// keyboard follows it (`.followsApp`), and without that model nothing dictates.
    var keyboardDictates = false

    var isReady: Bool { stage == .ready }

    var isFailed: Bool {
        if case .failed = stage { return true }
        return false
    }

    var fraction: Double {
        switch stage {
        case .downloading(let fraction): fraction
        case .ready: 1
        default: 0
        }
    }

    /// Hex's own model and the one the Hex key dictates with (KeyboardSpeechPreparation): ready
    /// only once both are, so Ready means the Hex key works in other apps.
    @MainActor
    init(dictation: DictationController, keyboard: KeyboardSpeechPreparation) {
        let app: Stage
        if dictation.isModelReady {
            app = .ready
        } else {
            switch dictation.phase {
            case .preparingModel:
                app = .downloading(dictation.modelPreparation.fractionCompleted)
                detail = dictation.modelPreparation.detail
            case .failed(let message): app = .failed(message)
            default: app = .starting
            }
        }
        let keys: Stage? = switch keyboard.state {
        case .ready, .followsApp: keyboard.downloads ? .ready : nil
        case .downloading(let fraction): .downloading(fraction)
        case .failed(let message): .failed(message)
        case .checking: .starting
        }
        stage = Self.combine(app, keys)
        downloads = dictation.engineDownloads
        keyboardDictates = keyboard.state == .ready
        let id: String
        if dictation.isModelReady {
            id = dictation.transcriptionLanguageID
        } else if dictation.engine == .parakeetV2 {
            id = "en-US"
        } else {
            id = LanguageSettings().effectiveDictationLanguage ?? Locale.preferredLanguages.first ?? "en-US"
        }
        language = SpeechModelUnavailable.interfaceName(id)
    }

    init(stage: Stage, language: String, detail: String? = nil) {
        self.stage = stage
        self.language = language
        self.detail = detail
    }

    /// One stage for two downloads: a failure first, then ready only when both are, else the
    /// two fractions averaged (a ready one counts as whole). No "b": the keyboard has nothing
    /// to download.
    static func combine(_ a: Stage, _ b: Stage?) -> Stage {
        guard let b else { return a }
        if case .failed = a { return a }
        if case .failed = b { return b }
        if a == .ready, b == .ready { return .ready }
        if a == .starting, b == .starting { return .starting }
        func fraction(_ stage: Stage) -> Double {
            switch stage {
            case .ready: 1
            case .downloading(let value): value
            default: 0
            }
        }
        return .downloading((fraction(a) + fraction(b)) / 2)
    }
}

struct OnboardingView: View {
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let dictation: DictationController
    let monitor: KeyboardSetupMonitor
    let keyboardSpeech: KeyboardSpeechPreparation
    @Binding var page: OnboardingPage?
    @Namespace private var pillSpace
    @State private var tryText = OnboardingFixture.current == .tryDone ? "On my way, see you at ten." : ""
    @State private var fixtureModel: OnboardingModel? = OnboardingView.initialFixtureModel

    private static var initialFixtureModel: OnboardingModel? {
        guard let fixture = OnboardingFixture.current else { return nil }
        let english = SpeechModelUnavailable.interfaceName("en-US")
        if fixture == .failed {
            return OnboardingModel(
                stage: .failed(String(localized: "Hex couldn't download the speech model. Check your connection and try again.")),
                language: english
            )
        }
        if let fraction = fixture.modelFraction {
            return OnboardingModel(
                stage: .downloading(fraction),
                language: english,
                detail: String(localized: "Downloading the speech model")
            )
        }
        return OnboardingModel(stage: .ready, language: english)
    }

    private var model: OnboardingModel {
        fixtureModel ?? OnboardingModel(dictation: dictation, keyboard: keyboardSpeech)
    }

    var body: some View {
        ZStack {
            switch page {
            case .setup?:
                OnboardingSetupPage(model: model, onRetry: retryModel) { go(to: .demo) }
                    .transition(pageTransition)
            case .demo?:
                OnboardingDemoPage(model: model) { go(to: .permissions) }
                    .transition(pageTransition)
            case .permissions?:
                OnboardingPermissionsPage(monitor: monitor, model: model) {
                    go(to: model.isReady ? .practice : .waiting)
                }
                .transition(pageTransition)
            case .waiting?:
                OnboardingWaitingPage(
                    model: model,
                    onRetry: retryModel,
                    onWithoutModel: finish,
                    onReady: { go(to: .practice) }
                )
                .transition(pageTransition)
            case .practice?:
                OnboardingPracticePage(monitor: monitor, model: model, text: $tryText) {
                    go(to: .ready)
                }
                .transition(pageTransition)
            case .ready?:
                OnboardingReadyPage(
                    monitor: monitor,
                    model: model,
                    onRetry: retryModel,
                    onTryAgain: { go(to: .practice) },
                    onStart: finish
                )
                .transition(pageTransition)
            case nil:
                EmptyView()
            }
        }
        .frame(maxWidth: 500)
        .padding(.horizontal, 20)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .environment(\.onboardingPillSpace, reduceMotion ? nil : pillSpace)
        .task {
            while !Task.isCancelled {
                monitor.refresh()
                try? await Task.sleep(for: .seconds(1))
            }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active {
                monitor.refresh()
            }
        }
        .task { await startModelDownload() }
        .task { await runTour() }
    }

    /// The model starts downloading as onboarding opens, as on the desktop. Launch already
    /// prepares a cached Parakeet or Apple Speech; this starts the Parakeet download that home's
    /// Set up used to wait for, or, before iOS 26, gets Apple's on-device recognizer ready,
    /// which asks for Speech Recognition here. A launch that found Parakeet ended mid-load
    /// takes Apple's engine instead (SpeechEngineChoice), or stays failed where there's none.
    private func startModelDownload() async {
        guard !OnboardingFixture.isSnapshot else { return }
        // Launch decides Apple Speech or Parakeet in a task of its own; let it answer first.
        try? await Task.sleep(for: .milliseconds(600))
        guard !Task.isCancelled, !dictation.isModelReady, dictation.phase == .modelRequired else { return }
        dictation.prepareModel()
    }

    private func retryModel() {
        guard !OnboardingFixture.isSnapshot else { return }
        if !dictation.isModelReady {
            dictation.prepareModel()
        }
        keyboardSpeech.start(retry: true) { dictation.engine }
    }

    /// "onboarding-tour": steps through every page on the fixture timeline.
    private func runTour() async {
        guard OnboardingFixture.current == .tour else { return }
        typealias Tour = OnboardingFixture.Tour
        if Tour.start == nil {
            Tour.start = Date().addingTimeInterval(Tour.leadIn)
        }
        let english = SpeechModelUnavailable.interfaceName("en-US")
        while !Task.isCancelled {
            let t = Tour.elapsed
            monitor.refresh()
            fixtureModel = t < Tour.modelReady
                ? OnboardingModel(stage: .downloading(min(0.97, 0.05 + 0.92 * max(0, t) / Tour.modelReady)), language: english)
                : OnboardingModel(stage: .ready, language: english)
            if t >= Tour.done {
                finish()
                return
            } else if t >= Tour.toReady, page == .practice {
                go(to: .ready)
            } else if t >= Tour.text, tryText.isEmpty {
                tryText = "On my way, see you at ten."
            } else if t >= Tour.toTry, page == .waiting {
                go(to: .practice)
            } else if t >= Tour.toWaiting, page == .permissions {
                go(to: .waiting)
            } else if t >= Tour.toPermissions, page == .demo {
                go(to: .permissions)
            } else if t >= Tour.toDemo, page == .setup {
                go(to: .demo)
            }
            try? await Task.sleep(for: .milliseconds(33))
        }
    }

    /// The page leaving lifts away as the desktop's roll does; the arriving page's parts roll in
    /// themselves, staggered (onboardingStagger).
    private var pageTransition: AnyTransition {
        guard !reduceMotion else { return .opacity }
        return .asymmetric(
            insertion: .identity,
            removal: AnyTransition.opacity
                .combined(with: .offset(y: -OnboardingMotion.travel))
                .animation(OnboardingMotion.exit)
        )
    }

    private func go(to next: OnboardingPage) {
        guard page != next else { return }
        OnboardingProgress.save(next)
        // The pill flies on the desktop's flight spring; the rest follows its own springs.
        withAnimation(reduceMotion ? HexMotion.reducedFade : OnboardingMotion.fly) {
            page = next
        }
    }

    private func finish() {
        OnboardingProgress.complete()
        withAnimation(HexMotion.settleAnimation(reduceMotion: reduceMotion)) {
            page = nil
        }
    }
}

// MARK: - Setting up Hex

/// Page 1: the model downloads by itself from the start, and the page moves on to the demo
/// after a beat. Only a failed start holds it, with Try again.
private struct OnboardingSetupPage: View {
    let model: OnboardingModel
    let onRetry: () -> Void
    let onNext: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            Spacer(minLength: 24)
            VStack(spacing: 14) {
                HexMarkView(size: 56)
                    .padding(.bottom, 10)
                    .onboardingStagger(0)
                OnboardingTitle("Setting up Hex")
                    .onboardingStagger(1)
                HStack(spacing: 6) {
                    Text(model.isReady ? "Ready" : model.downloads ? "Downloading" : "Preparing")
                    OnboardingLanguageCapsule(language: model.language)
                    Text("speech model")
                }
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textMuted)
                .lineLimit(1)
                .minimumScaleFactor(0.8)
                .onboardingStagger(2)

                if case .failed(let message) = model.stage {
                    OnboardingFailure(message: message, onRetry: onRetry)
                        .padding(.top, 6)
                    OnboardingQuietButton(title: "Continue", action: onNext)
                } else {
                    OnboardingProgressPill(model: model, large: true)
                        .padding(.top, 6)
                        .onboardingStagger(3)
                }
            }
            .frame(maxWidth: .infinity)
            Spacer(minLength: 24)
            OnboardingPrivacyLine()
                .padding(.bottom, 16)
                .onboardingStagger(4)
        }
        .task(id: model.isFailed) {
            guard OnboardingFixture.autoAdvances, !model.isFailed else { return }
            try? await Task.sleep(for: .seconds(2.6))
            guard !Task.isCancelled else { return }
            onNext()
        }
    }
}

// MARK: - The demo

/// Page 2, optional: a reply dictated with the Hex key, then on to permissions. Skip passes it.
private struct OnboardingDemoPage: View {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let model: OnboardingModel
    let onNext: () -> Void

    @State private var beat = OnboardingFixture.current == .demo ? 3 : 0
    @State private var shownWords = OnboardingFixture.current == .demo ? Int.max : 0

    private var reply: String { String(localized: "On my way. Oat or regular?") }
    private var replyWords: [Substring] { reply.split(separator: " ", omittingEmptySubsequences: true) }

    private var typed: String {
        replyWords.prefix(shownWords).joined(separator: " ")
    }

    var body: some View {
        VStack(spacing: 0) {
            OnboardingTopBar(model: model, onSkip: onNext)
            VStack(spacing: 10) {
                OnboardingTitle("With Hex")
                Text("Tap the Hex key and say it.")
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
            }
            .padding(.top, 12)
            .padding(.bottom, 24)
            .onboardingStagger(1)

            stage
                .onboardingStagger(2)
            Spacer(minLength: 16)
        }
        .task { await play() }
    }

    private var stage: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(spacing: 10) {
                Text(verbatim: "S")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(HexTheme.textBase)
                    .frame(width: 30, height: 30)
                    .background(Color.white.opacity(0.12), in: Circle())
                Text(verbatim: "Sam")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(HexTheme.textBase)
                Spacer()
            }

            bubble(String(localized: "Are you near the store? Could you grab some milk?"), mine: false)

            if beat >= 4 {
                bubble(reply, mine: true)
                    .transition(.opacity.combined(with: .offset(y: 6)))
            }

            HStack(spacing: 10) {
                Group {
                    if beat >= 4 || typed.isEmpty {
                        Text("Message").foregroundStyle(HexTheme.textFaint)
                    } else {
                        Text(typed).foregroundStyle(HexTheme.textBase)
                    }
                }
                .font(.system(size: 15))
                .lineLimit(1)
                Spacer(minLength: 8)
                // The Hex key, lit while it listens.
                HexMarkView(size: 16, color: beat == 1 || beat == 2 ? HexTheme.textContrast : HexTheme.textBase)
                    .frame(width: 34, height: 30)
                    .background(beat == 1 || beat == 2 ? HexTheme.bgContrast : Color.white.opacity(0.09), in: Capsule())
            }
            .padding(.leading, 14)
            .padding(.trailing, 6)
            .frame(height: 42)
            .background(Color.white.opacity(0.06), in: Capsule())
        }
        .padding(16)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: beat)
        .animation(HexMotion.reducedFade, value: shownWords)
        .accessibilityElement(children: .combine)
    }

    private func bubble(_ text: String, mine: Bool) -> some View {
        HStack {
            if mine { Spacer(minLength: 40) }
            Text(text)
                .font(.system(size: 15))
                .foregroundStyle(mine ? HexTheme.textContrast : HexTheme.textBase)
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
                .background(mine ? HexTheme.bgContrast : Color.white.opacity(0.09), in: RoundedRectangle(cornerRadius: 16, style: .continuous))
            if !mine { Spacer(minLength: 40) }
        }
    }

    /// Hex key down, the words arrive, sent, then on to permissions.
    private func play() async {
        guard OnboardingFixture.current != .demo else { return }
        if reduceMotion {
            beat = 4
            shownWords = Int.max
        } else {
            try? await Task.sleep(for: .seconds(0.9))
            beat = 1
            try? await Task.sleep(for: .seconds(0.9))
            beat = 2
            for index in 1...replyWords.count {
                guard !Task.isCancelled else { return }
                shownWords = index
                try? await Task.sleep(for: .seconds(0.2))
            }
            beat = 3
            try? await Task.sleep(for: .seconds(0.6))
            beat = 4
        }
        try? await Task.sleep(for: .seconds(1.8))
        guard !Task.isCancelled, OnboardingFixture.autoAdvances else { return }
        onNext()
    }
}

// MARK: - Permissions

/// Page 3: the microphone and the Hex keyboard. No Skip, as on the desktop, but Continue never
/// waits on them: someone who declines the microphone can still type with Hex and allow it
/// later (App Review 5.1.1(iv)). Full Access can't be verified from the app; the practice
/// checks it. Moves on by itself once both rows are done.
private struct OnboardingPermissionsPage: View {
    @Environment(\.openURL) private var openURL
    let monitor: KeyboardSetupMonitor
    let model: OnboardingModel
    let onContinue: () -> Void
    @State private var showsKeyboardFallback = false

    private var microphoneDone: Bool { monitor.microphone == .granted }
    private var keyboardDone: Bool { monitor.keyboardAdded == true }

    /// Before iOS's microphone prompt, Hex's own buttons say Continue or Open Settings, never
    /// Allow (App Review 5.1.1(iv)).
    private func continueTapped() {
        if monitor.microphone == .undetermined {
            monitor.requestMicrophone { onContinue() }
        } else {
            onContinue()
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            OnboardingTopBar(model: model, onSkip: nil)
            Spacer(minLength: 16)
            VStack(spacing: 10) {
                OnboardingTitle("Let Hex hear you")
                Text("Hex listens only after you tap the Hex key.")
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
                    .multilineTextAlignment(.center)
            }
            .padding(.bottom, 24)
            .onboardingStagger(1)

            VStack(spacing: 0) {
                OnboardingPermissionRow(
                    icon: "mic",
                    title: "Microphone",
                    detail: "For dictation",
                    done: microphoneDone,
                    // iOS asks right after this tap, so the pill says Continue, never Allow.
                    actionTitle: monitor.microphone == .denied ? "Open Settings" : "Continue"
                ) {
                    if monitor.microphone == .denied {
                        openSettings(openURL)
                    } else {
                        monitor.requestMicrophone()
                    }
                }
                Rectangle().fill(HexTheme.borderWeak).frame(height: 0.5)
                OnboardingPermissionRow(
                    icon: "keyboard",
                    title: "Hex Keyboard",
                    detail: "With Full Access",
                    done: keyboardDone,
                    actionTitle: "Open Settings"
                ) {
                    openSettings(openURL)
                }
            }
            .padding(.horizontal, 16)
            .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
            .onboardingStagger(2)

            VStack(alignment: .leading, spacing: 8) {
                Text("Keyboards  ›  Hex Keyboard  ›  Allow Full Access")
                Button {
                    withAnimation(HexMotion.reducedFade) { showsKeyboardFallback.toggle() }
                } label: {
                    Text("Don't see Keyboards?")
                        .underline()
                }
                .buttonStyle(.plain)
                if showsKeyboardFallback {
                    Text("General  ›  Keyboard  ›  Keyboards  ›  Add New Keyboard  ›  Hex")
                        .transition(.opacity)
                }
            }
            .font(.system(size: 13))
            .foregroundStyle(HexTheme.textMuted)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, 4)
            .padding(.top, 14)
            .opacity(keyboardDone ? 0 : 1)
            .accessibilityHidden(keyboardDone)
            .onboardingStagger(3)

            Spacer(minLength: 16)
            OnboardingPrivacyLine()
                .padding(.bottom, 14)
                .onboardingStagger(4)
            OnboardingPrimaryButton(title: "Continue", enabled: monitor.keyboardAdded != false, action: continueTapped)
                .padding(.bottom, 12)
                .onboardingStagger(5)
        }
        .task(id: microphoneDone && keyboardDone) {
            guard OnboardingFixture.autoAdvances, microphoneDone, keyboardDone else { return }
            try? await Task.sleep(for: .seconds(0.8))
            guard !Task.isCancelled else { return }
            onContinue()
        }
    }
}

// MARK: - Almost ready

/// Page 4, only while the model still downloads: the practice needs it. Continue without a
/// model shows only once the download failed or stalled.
private struct OnboardingWaitingPage: View {
    let model: OnboardingModel
    let onRetry: () -> Void
    let onWithoutModel: () -> Void
    let onReady: () -> Void

    var body: some View {
        VStack(spacing: 0) {
            Spacer(minLength: 24)
            OnboardingModelWait(model: model, onRetry: onRetry, onWithoutModel: onWithoutModel)
                .onboardingStagger(1)
            Spacer(minLength: 24)
        }
        .task(id: model.isReady) {
            guard OnboardingFixture.autoAdvances, model.isReady else { return }
            try? await Task.sleep(for: .seconds(0.7))
            guard !Task.isCancelled else { return }
            onReady()
        }
    }
}

/// "Almost ready" with the download's progress, Try again after a failure, and Continue without
/// a model after a failure or 30 s without progress.
struct OnboardingModelWait: View {
    let model: OnboardingModel
    let onRetry: () -> Void
    let onWithoutModel: () -> Void
    @State private var lastFraction = -1.0
    @State private var stalled = OnboardingFixture.current == .failed

    /// A download that gained nothing for this long has stalled.
    private static let stallAfter: Duration = .seconds(30)

    var body: some View {
        VStack(spacing: 14) {
            // A failed download says so above its message, rather than that it's getting ready.
            if model.isFailed {
                OnboardingTitle(model.downloads ? "Download failed" : "Speech recognition isn't ready")
            } else {
                OnboardingTitle("Almost ready")
                Text("Getting the speech model ready.")
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }

            if case .failed(let message) = model.stage {
                OnboardingFailure(message: message, onRetry: onRetry)
                    .padding(.top, 6)
            } else {
                VStack(alignment: .leading, spacing: 8) {
                    ProgressView(value: model.fraction)
                        .tint(HexTheme.textBase)
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        if let detail = model.detail {
                            Text(detail)
                                .lineLimit(2)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        Spacer(minLength: 0)
                        Text(model.fraction, format: .percent.precision(.fractionLength(0)))
                            .monospacedDigit()
                    }
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
                }
                .frame(maxWidth: 260)
                .padding(.top, 10)
            }

            if model.isFailed || stalled {
                VStack(spacing: 6) {
                    OnboardingQuietButton(title: "Continue without a model", action: onWithoutModel)
                    Text(model.keyboardDictates
                        ? "The Hex keyboard dictates with Apple's speech recognition until it downloads."
                        : model.downloads ? "Dictation won't work until the speech model downloads." : "Dictation won't work until speech recognition is ready.")
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.textFaint)
                        .multilineTextAlignment(.center)
                }
                .padding(.top, 10)
            }
        }
        .frame(maxWidth: .infinity)
        .task(id: model.fraction) {
            guard !model.isReady, !model.isFailed else { return }
            if lastFraction != model.fraction {
                lastFraction = model.fraction
                if OnboardingFixture.current != .failed { stalled = false }
            }
            try? await Task.sleep(for: Self.stallAfter)
            guard !Task.isCancelled else { return }
            stalled = true
        }
    }
}

// MARK: - Your turn

/// Page 5, optional: the practice, with the Hex keyboard in a field that keeps the keyboard up.
/// It counts once a dictation arrives or a few words are typed, and the done state keeps the
/// keyboard.
private struct OnboardingPracticePage: View {
    @Environment(\.openURL) private var openURL
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let monitor: KeyboardSetupMonitor
    let model: OnboardingModel
    @Binding var text: String
    let onNext: () -> Void

    @State private var dictated = OnboardingFixture.current == .tryDone
    @State private var heardNothing = OnboardingFixture.current == .tryEmpty
    @State private var keyboardActivity = 0
    @State private var lastSwitchAt: Date?
    @State private var keyboardCameUpAt = Date.distantPast
    @State private var fullAccessLooksOff = OnboardingFixture.current == .tryOff
    /// A keyboard switch by hand (not the keyboard coming up) since the page opened.
    @State private var switchedKeyboards = false
    /// Since when Hex's keyboard has been off screen after a switch.
    @State private var hexKeyboardAwaySince: Date?
    /// Hex's keyboard didn't come on screen after a switch: the page says how to bring it up.
    @State private var hexKeyboardMissing = false
    /// Words that arrived while Hex's keyboard was on screen: only these count as typed.
    @State private var hexWords = 0
    /// The last text that arrived came from iOS's dictation or another keyboard.
    @State private var otherInput = false
    @FocusState private var fieldFocused: Bool

    /// How long the keyboard gets to report Full Access after a switch or new text.
    private static let fullAccessGrace: Duration = .seconds(4)
    /// The keyboard coming up (focus, or back from Settings) also posts an input mode change.
    private static let keyboardSettle: TimeInterval = 1.5
    /// How long Hex's keyboard gets to come on screen after a switch.
    private static let hexKeyboardGrace: TimeInterval = 2

    private var hasText: Bool {
        !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
    }

    private var complete: Bool {
        OnboardingPractice.isDone(text: text, dictated: dictated, typedWithHex: hexWords, fullAccessOn: monitor.fullAccessOn)
    }

    var body: some View {
        VStack(spacing: 0) {
            OnboardingTopBar(model: model, onSkip: complete ? nil : onNext)

            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    HStack(alignment: .center, spacing: 10) {
                        if complete {
                            Image(systemName: "checkmark.circle.fill")
                                .font(.system(size: 24, weight: .medium))
                                .foregroundStyle(HexTheme.success)
                                .transition(.opacity.combined(with: .scale(scale: 0.9)))
                        }
                        OnboardingTitle(complete ? "That's Hex" : "Your turn")
                            .contentTransition(.opacity)
                    }
                    .onboardingStagger(1)

                    Group {
                        if complete {
                            Text("It types wherever your cursor is, in any app.")
                        } else {
                            Text("Switch to Hex with \(Image(systemName: "globe")), then tap the Hex key.")
                        }
                    }
                    .font(.system(size: 15))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.top, -8)
                    .onboardingStagger(2)

                    TextField("Say anything…", text: $text, axis: .vertical)
                        .font(.system(size: 17))
                        .foregroundStyle(HexTheme.textBase)
                        .focused($fieldFocused)
                        .lineLimit(2...5)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 14)
                        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
                        .onboardingStagger(3)

                    if heardNothing && !complete {
                        practiceNote(icon: "waveform", Text("Hex didn't hear any words. Try speaking louder."))
                    }

                    if otherInput && !complete && monitor.fullAccessOn {
                        practiceNote(
                            icon: "hand.raised",
                            Text("That wasn't Hex. Tap the Hex key on the Hex keyboard.")
                        )
                    } else if hexKeyboardMissing && !complete {
                        practiceNote(
                            icon: "keyboard",
                            Text("Don't see the Hex key? Tap \(Image(systemName: "globe")) until the Hex keyboard appears.")
                        )
                    }

                    if fullAccessLooksOff && !monitor.fullAccessOn {
                        fullAccessNote
                            .transition(.opacity.combined(with: .scale(scale: 0.98)))
                    }
                }
                .padding(.top, 12)
                .padding(.bottom, 16)
            }
            .scrollBounceBehavior(.basedOnSize)

            OnboardingPrimaryButton(title: "Continue", action: onNext)
                .hexControlReveal(complete, reduceMotion: reduceMotion, scale: 0.96)
                .allowsHitTesting(complete)
                .padding(.top, 8)
                .padding(.bottom, 12)
        }
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: complete)
        .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: fullAccessLooksOff)
        .animation(HexMotion.reducedFade, value: heardNothing)
        .animation(HexMotion.reducedFade, value: hexKeyboardMissing)
        .animation(HexMotion.reducedFade, value: otherInput)
        .onAppear {
            guard OnboardingFixture.current != .tryDone else { return }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) {
                fieldFocused = true
            }
        }
        .task { await watchKeyboardDictations() }
        .onReceive(NotificationCenter.default.publisher(for: UITextInputMode.currentInputModeDidChangeNotification)) { _ in
            lastSwitchAt = Date()
            if Date().timeIntervalSince(keyboardCameUpAt) >= Self.keyboardSettle { switchedKeyboards = true }
            noteKeyboardActivity()
        }
        .onChange(of: fieldFocused) { _, focused in
            if focused { keyboardCameUpAt = Date() }
        }
        .onChange(of: scenePhase) { _, phase in
            guard phase == .active else { return }
            keyboardCameUpAt = Date()
            // The page can open while Hex comes back from Settings, before it can take focus.
            if !fieldFocused && OnboardingFixture.current != .tryDone {
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { fieldFocused = true }
            }
        }
        .onChange(of: text) { old, new in
            if hasText {
                heardNothing = false
                noteKeyboardActivity()
            }
            attributeText(from: old, to: new)
        }
        .onChange(of: monitor.fullAccessOn) { _, on in
            if on { fullAccessLooksOff = false }
        }
        .task(id: keyboardActivity) {
            // iOS tells only the keyboard about Full Access, and the keyboard writes it to the
            // App Group as soon as it opens with it; nothing a few seconds after a keyboard
            // switch or new text means it's off (or the Hex keyboard isn't the one up).
            guard keyboardActivity > 0 else { return }
            try? await Task.sleep(for: Self.fullAccessGrace)
            guard !Task.isCancelled, !monitor.fullAccessOn else { return }
            if !hasText, let switchedAt = lastSwitchAt,
               abs(switchedAt.timeIntervalSince(keyboardCameUpAt)) < Self.keyboardSettle {
                return
            }
            fullAccessLooksOff = true
        }
    }

    /// The keyboard's dictations, through the App Group: a new inserted result means one
    /// arrived; a new empty one means it heard nothing (the keyboard itself stays quiet).
    private func watchKeyboardDictations() async {
        guard !OnboardingFixture.isSnapshot else { return }
        let bridge = KeyboardBridge()
        let insertedAtStart = bridge.lastInsertedResultID
        var emptyAtStart = bridge.lastEmptyDictationAt
        while !Task.isCancelled {
            try? await Task.sleep(for: .milliseconds(250))
            followHexKeyboard(bridge)
            if bridge.lastInsertedResultID != insertedAtStart {
                dictated = true
                heardNothing = false
            }
            let empty = bridge.lastEmptyDictationAt
            if let empty, empty != emptyAtStart {
                emptyAtStart = empty
                if !dictated { heardNothing = true }
            }
        }
    }

    /// After a keyboard switch, Hex's keyboard has `hexKeyboardGrace` to come on screen (it says
    /// so in the App Group, `KeyboardPresence`); if it doesn't, another keyboard is up, or Hex's
    /// never drew, and the page says to switch again. Only with Full Access, which the keyboard
    /// needs to write there; without it the Full Access note shows instead.
    private func followHexKeyboard(_ bridge: KeyboardBridge) {
        guard switchedKeyboards, monitor.fullAccessOn, !bridge.isKeyboardShown() else {
            hexKeyboardAwaySince = nil
            if hexKeyboardMissing { hexKeyboardMissing = false }
            return
        }
        let since = hexKeyboardAwaySince ?? Date()
        hexKeyboardAwaySince = since
        let missing = Date().timeIntervalSince(since) >= Self.hexKeyboardGrace
        if hexKeyboardMissing != missing { hexKeyboardMissing = missing }
    }

    /// New words count only while Hex's keyboard is on screen (`KeyboardPresence`): iOS's
    /// dictation and other keyboards take it off screen, so what they type never finishes the
    /// practice, and the page says to use the Hex key. Text Hex's keyboard adds clears that.
    private func attributeText(from old: String, to new: String) {
        let total = OnboardingPractice.words(in: new)
        guard total > 0 else {
            hexWords = 0
            otherInput = false
            return
        }
        hexWords = min(hexWords, total)
        guard new.count > old.count else { return }
        if OnboardingFixture.isSnapshot || KeyboardBridge().isKeyboardShown() {
            hexWords = min(total, hexWords + OnboardingPractice.wordsAdded(from: old, to: new))
            otherInput = false
        } else {
            otherInput = true
        }
    }

    private func practiceNote(icon: String, _ text: Text) -> some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: icon)
                .font(.system(size: 13, weight: .medium))
                .padding(.top, 1)
            text
        }
        .font(.system(size: 13))
        .foregroundStyle(HexTheme.textMuted)
        .padding(.horizontal, 4)
        .transition(.opacity)
        .accessibilityElement(children: .combine)
    }

    private func noteKeyboardActivity() {
        guard !monitor.fullAccessOn else { return }
        keyboardActivity += 1
    }

    private var fullAccessNote: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .top, spacing: 10) {
                Image(systemName: "exclamationmark.circle.fill")
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(HexTheme.warmMic)
                    .padding(.top, 1)
                Text("Turn on Allow Full Access")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
            }
            .accessibilityElement(children: .combine)

            OnboardingSecondaryButton(title: "Open Settings", icon: "gearshape.fill") {
                openSettings(openURL)
            }
        }
        .padding(16)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
    }
}

// MARK: - Hex is ready

/// What's left on page 6, first what matters most.
enum OnboardingLeft: Equatable {
    case nothing
    case model
    case microphone
    case keyboard
    case fullAccess

    static func resolve(modelReady: Bool, microphoneGranted: Bool, keyboardAdded: Bool?, fullAccessOn: Bool) -> Self {
        if !modelReady { return .model }
        if !microphoneGranted { return .microphone }
        if keyboardAdded == false { return .keyboard }
        if !fullAccessOn { return .fullAccess }
        return .nothing
    }

    /// Try again goes back to the practice: worth it once Settings has the microphone or the
    /// keyboard, or to use the keyboard once for Full Access. Not on "Hex is ready", with
    /// nothing left, nor while the model gets ready, whose card has its own Try again.
    var offersTryAgain: Bool {
        self == .microphone || self == .keyboard || self == .fullAccess
    }
}

/// Page 6. "Hex is ready" only once every step is done (the model included); until then it
/// says what's left. Start using Hex never waits on it (App Review 5.1.1(iv)).
private struct OnboardingReadyPage: View {
    @Environment(\.openURL) private var openURL
    let monitor: KeyboardSetupMonitor
    let model: OnboardingModel
    let onRetry: () -> Void
    let onTryAgain: () -> Void
    let onStart: () -> Void

    private var left: OnboardingLeft {
        OnboardingLeft.resolve(
            modelReady: model.isReady,
            microphoneGranted: monitor.microphone == .granted,
            keyboardAdded: monitor.keyboardAdded,
            fullAccessOn: monitor.fullAccessOn
        )
    }

    var body: some View {
        VStack(spacing: 0) {
            Spacer(minLength: 24)
            Group {
                switch left {
                case .model:
                    OnboardingModelWait(model: model, onRetry: onRetry, onWithoutModel: onStart)
                default:
                    VStack(spacing: 14) {
                        if left == .nothing {
                            HexMarkView(size: 56)
                                .padding(.bottom, 10)
                        }
                        OnboardingTitle(left == .nothing ? "Hex is ready" : "Almost ready")
                        subtitle
                            .font(.system(size: 15))
                            .foregroundStyle(HexTheme.textMuted)
                            .multilineTextAlignment(.center)
                            .fixedSize(horizontal: false, vertical: true)
                        if left == .microphone || left == .keyboard {
                            OnboardingQuietButton(title: "Open Settings") { openSettings(openURL) }
                                .padding(.top, 4)
                        }
                    }
                    .frame(maxWidth: .infinity)
                }
            }
            .onboardingStagger(1)
            Spacer(minLength: 24)
            if left.offersTryAgain {
                OnboardingQuietButton(title: "Try again", action: onTryAgain)
                    .padding(.bottom, 8)
                    .onboardingStagger(2)
            }
            OnboardingPrimaryButton(title: "Start using Hex", action: onStart)
                .padding(.bottom, 12)
                .onboardingStagger(3)
        }
    }

    @ViewBuilder
    private var subtitle: some View {
        switch left {
        case .nothing, .model: Text("Tap the Hex key in any app to talk.")
        case .microphone: Text("Allow the microphone in Settings.")
        case .keyboard: Text("Add the Hex keyboard in Settings.")
        case .fullAccess: Text("Use the Hex keyboard once to finish.")
        }
    }
}
