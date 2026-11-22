import SwiftUI
import UIKit

private struct AppRecordingClockLabel: View {
    let dictation: DictationController

    var body: some View {
        Text(HexTheme.formatClock(TimeInterval(dictation.recordingWholeSeconds)))
            .font(.system(size: 13, weight: .medium))
            .monospacedDigit()
            .foregroundStyle(HexTheme.textBase)
    }
}

private struct AppWarmRemainingLabel: View {
    let dictation: DictationController

    var body: some View {
        Text(dictation.keyboardSessionDisplayRemaining)
            .font(.system(size: 13, weight: .medium))
            .monospacedDigit()
            .foregroundStyle(HexTheme.textMuted)
    }
}

struct ContentView: View {
    private enum AppStageMode: Equatable {
        case idle
        case recording
        case transcribing
        case preparing
        case permission
        case failed
    }

    @Environment(\.openURL) private var openURL
    @Environment(\.scenePhase) private var scenePhase
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @Bindable var dictation: DictationController
    @Binding var openedFromKeyboard: Bool
    @Binding var showSetup: Bool
    /// A settings page the keyboard asked for (`KeyboardLaunchRequest.translationSettings`).
    var openPage: Binding<SettingsPage?> = .constant(nil)
    @State private var setupMonitor = KeyboardSetupMonitor()
    @State private var keyboardSpeech = KeyboardSpeechPreparation()
    @State private var onboardingPage = OnboardingProgress.launchPage
    @State private var settingsPath = SettingsSnapshotFixture.initialPath
    @State private var showAcknowledgements = false

    private static let stageWellHeight: CGFloat = 48
    private static let stageControlHeight: CGFloat = 36
    private static let stageInset: CGFloat = 6
    private static let stageCancelWidth: CGFloat = 36
    private static let stageDoneWidth: CGFloat = 70

    private static var simulatesHomeButtonDevice: Bool {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else { return false }
        return args[idx + 1] == "handoff-no-home-bar"
    }

    var body: some View {
        ZStack {
            HexTheme.bgDeep
                .ignoresSafeArea()

            if openedFromKeyboard {
                keyboardHandoffView
                    .transition(.opacity)
            } else if onboardingPage != nil {
                OnboardingView(dictation: dictation, monitor: setupMonitor, keyboardSpeech: keyboardSpeech, page: $onboardingPage)
                    .transition(.opacity)
            } else {
                standaloneView
                    .transition(.opacity)
            }
        }
        .preferredColorScheme(.dark)
        .background {
            // Translate what I say's session before iOS 26 (TranslationSessionHost).
            if #available(iOS 26.0, *) {
                EmptyView()
            } else {
                TranslationSessionHostView()
            }
        }
        .task { keyboardSpeech.start { dictation.engineName } }
        .onChange(of: scenePhase) { _, phase in
            // Opened from a keyboard handing its dictations to Hex: check again whether it must.
            if phase == .active { keyboardSpeech.recheckKeyboardHandoff { dictation.engineName } }
        }
        .animation(HexMotion.reducedFade, value: openedFromKeyboard)
        .animation(HexMotion.reducedFade, value: onboardingPage == nil)
    }

    private var appStageMode: AppStageMode {
        switch dictation.phase {
        case .recording:
            .recording
        case .transcribing:
            .transcribing
        case .preparingModel:
            .preparing
        case .requestingPermission:
            .permission
        case .failed:
            .failed
        case .modelRequired, .ready:
            .idle
        }
    }

    private var handoffScopePhase: DotScopePhase {
        switch dictation.phase {
        case .recording:
            .recording
        case .transcribing:
            .processing
        default:
            .hidden
        }
    }

    private var stageScopePhase: DotScopePhase {
        switch dictation.phase {
        case .recording:
            .recording
        case .transcribing:
            .processing
        case .preparingModel, .requestingPermission:
            .hidden
        case .failed:
            .failed
        case .modelRequired, .ready:
            .idle
        }
    }

    private var setupStepsLeft: Int {
        // The Hex key's own model counts: Ready means dictation works in other apps.
        setupMonitor.stepsLeft(modelReady: dictation.isModelReady && keyboardSpeech.isReady)
    }

    private var hasTranscript: Bool {
        !dictation.transcript.isEmpty
    }

    // MARK: - Standalone App

    private var standaloneView: some View {
        NavigationStack(path: $settingsPath) {
            homeView
                .navigationTitle("Hex")
                .toolbar(.hidden, for: .navigationBar)
                .navigationDestination(for: SettingsPage.self) { page in
                    settingsPage(page)
                }
        }
        .onChange(of: openPage.wrappedValue, initial: true) { _, page in
            guard let page else { return }
            settingsPath = [page]
            openPage.wrappedValue = nil
        }
        .tint(HexTheme.textBase)
        .sheet(isPresented: $showSetup) {
            SetupSheet(dictation: dictation, monitor: setupMonitor, keyboardSpeech: keyboardSpeech)
        }
        .task {
            while !Task.isCancelled {
                setupMonitor.refresh()
                try? await Task.sleep(for: .seconds(1))
            }
        }
        .onChange(of: scenePhase) { _, phase in
            if phase == .active {
                setupMonitor.refresh()
            }
        }
    }

    /// Fits one screen: the header and its status line, Dictate, and the four pages.
    private var homeView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 2) {
                    headerRow
                    SetupStatusRow(stepsLeft: setupStepsLeft) {
                        showSetup = true
                    }
                }

                stageBar

                if hasTranscript {
                    transcriptCard
                        .transition(.opacity.combined(with: .scale(scale: 0.98)))
                }

                if case .failed(let message) = dictation.phase {
                    errorBanner(message, offer: dictation.languageOffer)
                        .transition(.opacity.combined(with: .scale(scale: 0.98)))
                } else if let notice = dictation.languageNotice {
                    noticeBanner(notice)
                        .transition(.opacity.combined(with: .scale(scale: 0.98)))
                }

                // Experiments › Multilingual model: its download or load, beside dictation.
                MultilingualModelStatus(
                    download: dictation.multilingualDownload,
                    retry: { dictation.retryMultilingualParakeet() },
                    turnOff: { dictation.turnOffMultilingualParakeet() }
                )

                SettingsCard {
                    ForEach(Array(SettingsPage.allCases.enumerated()), id: \.element) { index, page in
                        if index > 0 {
                            SettingsDivider(leading: 50)
                        }
                        NavigationLink(value: page) {
                            SettingsLinkLabel(title: page.title, icon: page.icon)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.top, 8)

                SettingsCard {
                    Button {
                        if let url = URL(string: UIApplication.openSettingsURLString) {
                            openURL(url)
                        }
                    } label: {
                        SettingsLinkLabel(title: String(localized: "Keyboard settings"), icon: "gearshape", trailingIcon: "arrow.up.right")
                    }
                    .buttonStyle(.plain)

                    SettingsDivider(leading: 50)

                    Button {
                        OnboardingProgress.save(.setup)
                        onboardingPage = .setup
                    } label: {
                        SettingsLinkLabel(title: String(localized: "Run setup again"), icon: "arrow.counterclockwise")
                    }
                    .buttonStyle(.plain)
                }

                // The privacy policy link App Review 5.1.1(i) asks for, with support and the
                // licence notices.
                SettingsCard {
                    Button {
                        SafariSheet.present(HexLinks.privacyPolicy)
                    } label: {
                        SettingsLinkLabel(title: String(localized: "Privacy policy"), icon: "hand.raised", trailingIcon: "arrow.up.right")
                    }
                    .buttonStyle(.plain)

                    SettingsDivider(leading: 50)

                    Button {
                        SafariSheet.present(HexLinks.support)
                    } label: {
                        SettingsLinkLabel(title: String(localized: "Support"), icon: "questionmark.circle", trailingIcon: "arrow.up.right")
                    }
                    .buttonStyle(.plain)

                    SettingsDivider(leading: 50)

                    Button {
                        showAcknowledgements = true
                    } label: {
                        SettingsLinkLabel(title: String(localized: "Acknowledgements"), icon: "doc.text")
                    }
                    .buttonStyle(.plain)
                    .sheet(isPresented: $showAcknowledgements) {
                        AcknowledgementsView()
                    }
                }
            }
            .frame(maxWidth: 500)
            .padding(.horizontal, 20)
            .padding(.top, 14)
            .padding(.bottom, 28)
            .frame(maxWidth: .infinity)
            .animation(HexMotion.stageAnimation(reduceMotion: reduceMotion), value: appStageMode)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: dictation.keyboardSessionEnabled)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: hasTranscript)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: setupStepsLeft)
        }
        .scrollBounceBehavior(.basedOnSize)
        .scrollDismissesKeyboard(.interactively)
        .background(HexTheme.bgDeep.ignoresSafeArea())
    }

    @ViewBuilder
    private func settingsPage(_ page: SettingsPage) -> some View {
        switch page {
        case .dictation:
            DictationSettingsPage(dictation: dictation)
        case .keyboard:
            KeyboardSettingsPage(fullAccessOn: setupMonitor.fullAccessOn)
        case .privacy:
            PrivacySettingsPage()
        case .experiments:
            ExperimentsSettingsPage()
        }
    }

    private var headerRow: some View {
        let showRecordingPill = appStageMode == .recording
        let showWarmPill = appStageMode != .recording
            && appStageMode != .transcribing
            && dictation.keyboardSessionEnabled

        return HStack(alignment: .center, spacing: 9) {
            HexMarkView(
                size: 20,
                color: showRecordingPill ? HexTheme.warmPearl : HexTheme.textBase
            )

            Text("Hex")
                .font(.system(size: 17, weight: .semibold))
                .tracking(-0.2)
                .foregroundStyle(HexTheme.textBase)

            Spacer()

            ZStack(alignment: .trailing) {
                HStack(spacing: 6) {
                    HexStatusDot(color: HexTheme.danger, size: 6.5)
                    AppRecordingClockLabel(dictation: dictation)
                }
                .padding(.horizontal, 10)
                .frame(height: 28)
                .background(Color.white.opacity(0.07), in: Capsule())
                .hexTrayReveal(showRecordingPill, reduceMotion: reduceMotion, offsetY: 2)

                Button {
                    withAnimation(HexMotion.settleAnimation(reduceMotion: reduceMotion)) {
                        dictation.endWarmSession()
                    }
                } label: {
                    HStack(spacing: 6) {
                        HexStatusDot(color: HexTheme.warmMic, size: 6)
                        AppWarmRemainingLabel(dictation: dictation)
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 28)
                    .background(Color.white.opacity(0.07), in: Capsule())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Turn off warm mic")
                .hexTrayReveal(showWarmPill, reduceMotion: reduceMotion, offsetY: 2)
            }
        }
        .frame(height: 30)
    }

    private var stageBar: some View {
        let mode = appStageMode
        let showScope = stageScopePhase != .hidden
        let showRecordingControls = mode == .recording
        let showIdleTrigger = mode == .idle || mode == .failed
        let showPreparing = mode == .preparing
        let showPermission = mode == .permission
        let idleWidth: CGFloat = (dictation.phase == .modelRequired || mode == .failed) ? 90 : 96

        let leadInset: Float = showRecordingControls
            ? Float(Self.stageInset + Self.stageCancelWidth + 6)
            : 0.0
        let trailInset: Float = if showRecordingControls {
            Float(Self.stageInset + Self.stageDoneWidth + 6)
        } else if showIdleTrigger {
            Float(Self.stageInset + idleWidth + 6)
        } else {
            0.0
        }

        return ZStack {
            HexRecessedWell(isDark: true)

            // Permanently mounted so phosphor trails and inset springs never reset mid-session.
            DotScopeView(
                phase: stageScopePhase,
                tuning: .bar(leadingInset: leadInset, trailingInset: trailInset, fade: 28.0, spacing: 3.5),
                drawPlate: false,
                loudnessProvider: { [weak dictation] in dictation?.loudnessDB ?? -120.0 }
            )
            .clipShape(Capsule())
            .opacity(showScope ? 1.0 : 0.0)
            .allowsHitTesting(false)

            HStack(spacing: 10) {
                Button {
                    withAnimation(HexMotion.stageAnimation(reduceMotion: reduceMotion)) {
                        dictation.cancelCurrentJob()
                    }
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(width: Self.stageCancelWidth, height: Self.stageControlHeight)
                        .background(Color.white.opacity(0.085), in: Circle())
                        .frame(minWidth: 44, minHeight: Self.stageWellHeight, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Cancel")
                .hexControlReveal(
                    showRecordingControls,
                    reduceMotion: reduceMotion,
                    scale: 0.90,
                    blur: 2.5,
                    anchor: .leading
                )

                Spacer(minLength: 0)

                Button {
                    withAnimation(HexMotion.stageAnimation(reduceMotion: reduceMotion)) {
                        dictation.finishRecording()
                    }
                } label: {
                    Text("Done")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(HexTheme.textBase)
                        .frame(width: Self.stageDoneWidth, height: Self.stageControlHeight)
                        .background(Color.white.opacity(0.15), in: Capsule())
                        .frame(minHeight: Self.stageWellHeight, alignment: .trailing)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .hexControlReveal(
                    showRecordingControls,
                    reduceMotion: reduceMotion,
                    scale: 0.90,
                    blur: 2.5,
                    anchor: .trailing
                )
            }
            .padding(.horizontal, Self.stageInset)

            HStack(spacing: 12) {
                Text(dictation.modelPreparation.detail)
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(HexTheme.textMuted)
                    .lineLimit(1)
                    .minimumScaleFactor(0.85)
                    .layoutPriority(1)
                ProgressView(value: dictation.modelPreparation.fractionCompleted)
                    .tint(HexTheme.textBase)
            }
            .padding(.horizontal, 18)
            .hexControlReveal(showPreparing, reduceMotion: reduceMotion, scale: 0.96, blur: 2.0)

            Text("Allow microphone access…")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
                .hexControlReveal(showPermission, reduceMotion: reduceMotion, scale: 0.96, blur: 2.0)

            Button {
                withAnimation(HexMotion.stageAnimation(reduceMotion: reduceMotion)) {
                    dictation.performPrimaryAction()
                }
            } label: {
                HStack(spacing: 0) {
                    Spacer(minLength: 0)
                    HStack(spacing: 6) {
                        Image(systemName: primaryActionIcon)
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(HexTheme.textMuted)
                        Text(primaryActionLabel)
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(HexTheme.textBase)
                    }
                    .frame(width: idleWidth, height: Self.stageControlHeight)
                    .background(Color.white.opacity(0.09), in: Capsule())
                }
                .padding(.horizontal, Self.stageInset)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .hexControlReveal(
                showIdleTrigger,
                reduceMotion: reduceMotion,
                scale: 0.92,
                blur: 2.5,
                anchor: .trailing
            )
        }
        .frame(height: Self.stageWellHeight)
        .clipShape(Capsule())
    }

    private var transcriptCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(dictation.transcript)
                .font(.system(size: 17))
                .lineSpacing(3)
                .foregroundStyle(HexTheme.textBase)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: 6) {
                Spacer()

                Button(dictation.copied ? String(localized: "Copied") : String(localized: "Copy")) {
                    dictation.copyTranscript()
                }
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(dictation.copied ? HexTheme.success : HexTheme.textBase)
                .padding(.horizontal, 12)
                .frame(height: 28)
                .background(Color.white.opacity(0.09), in: Capsule())

                Button("Clear") {
                    withAnimation(HexMotion.settleAnimation(reduceMotion: reduceMotion)) {
                        dictation.clearTranscript()
                    }
                }
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
                .padding(.horizontal, 12)
                .frame(height: 28)
                .background(Color.white.opacity(0.055), in: Capsule())
            }
            .buttonStyle(.plain)
        }
        .padding(16)
        .background(
            HexTheme.bgBase,
            in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
        )
    }

    // MARK: - Cold-Start Handoff Screen (unified bar + home-indicator swipe cue)

    private var keyboardHandoffView: some View {
        GeometryReader { geo in
            let bottomInset: CGFloat = Self.simulatesHomeButtonDevice ? 0 : geo.safeAreaInsets.bottom
            let hasHomeIndicator = bottomInset > 0

            let mode = appStageMode
            let showScope = handoffScopePhase != .hidden
            let showRecordingControls = mode == .recording
            let showFailedRetry = mode == .failed
            let showPreparing = mode == .preparing
            let status = handoffStatusLine
            let showHomeSwipeCue = hasHomeIndicator && (showRecordingControls || dictation.keyboardSessionEnabled)

            let handoffWellHeight: CGFloat = 52
            let handoffControlHeight: CGFloat = 38
            let handoffInset: CGFloat = 7
            let handoffCancelWidth: CGFloat = 38
            let handoffDoneWidth: CGFloat = 74
            let leadInset: Float = showRecordingControls
                ? Float(handoffInset + handoffCancelWidth + 6)
                : 0.0
            let trailInset: Float = showRecordingControls
                ? Float(handoffInset + handoffDoneWidth + 6)
                : 0.0

            ZStack(alignment: .bottom) {
                VStack(spacing: 0) {
                    HStack(alignment: .center, spacing: 9) {
                        HexMarkView(
                            size: 20,
                            color: showRecordingControls ? HexTheme.warmPearl : HexTheme.textBase
                        )

                        Text("Hex")
                            .font(.system(size: 17, weight: .semibold))
                            .tracking(-0.2)
                            .foregroundStyle(HexTheme.textBase)

                        Spacer()

                        Button("Settings") {
                            openedFromKeyboard = false
                        }
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                        .padding(.horizontal, 12)
                        .frame(height: 30)
                        .background(Color.white.opacity(0.07), in: Capsule())
                        .buttonStyle(.plain)
                    }

                    Spacer()

                    // Centered unified dictation stage platter
                    VStack(spacing: 14) {
                        ZStack {
                            HStack(spacing: 6) {
                                HexStatusDot(color: HexTheme.danger, size: 6.5)
                                AppRecordingClockLabel(dictation: dictation)
                            }
                            .padding(.horizontal, 11)
                            .frame(height: 26)
                            .background(Color.white.opacity(0.06), in: Capsule())
                            .hexTrayReveal(showRecordingControls, reduceMotion: reduceMotion, offsetY: 2)

                            Text(status ?? " ")
                                .font(.system(size: 15, weight: .medium))
                                .foregroundStyle(HexTheme.textMuted)
                                .multilineTextAlignment(.center)
                                .lineLimit(2)
                                .hexTrayReveal(!showRecordingControls && status != nil, reduceMotion: reduceMotion, offsetY: 2)
                        }
                        .frame(minHeight: 28)

                        ZStack {
                            ZStack {
                                HexRecessedWell(isDark: true)

                                DotScopeView(
                                    phase: handoffScopePhase,
                                    tuning: .bar(leadingInset: leadInset, trailingInset: trailInset, fade: 30.0, spacing: 3.5),
                                    drawPlate: false,
                                    loudnessProvider: { [weak dictation] in dictation?.loudnessDB ?? -120.0 }
                                )
                                .clipShape(Capsule())
                                .allowsHitTesting(false)

                                HStack(spacing: 10) {
                                    Button {
                                        withAnimation(HexMotion.stageAnimation(reduceMotion: reduceMotion)) {
                                            dictation.cancelCurrentJob()
                                        }
                                    } label: {
                                        Image(systemName: "xmark")
                                            .font(.system(size: 13, weight: .semibold))
                                            .foregroundStyle(HexTheme.textMuted)
                                            .frame(width: handoffCancelWidth, height: handoffControlHeight)
                                            .background(Color.white.opacity(0.085), in: Circle())
                                            .frame(minWidth: 44, minHeight: handoffWellHeight, alignment: .leading)
                                            .contentShape(Rectangle())
                                    }
                                    .buttonStyle(.plain)
                                    .accessibilityLabel("Cancel")
                                    .hexControlReveal(
                                        showRecordingControls,
                                        reduceMotion: reduceMotion,
                                        scale: 0.90,
                                        blur: 2.5,
                                        anchor: .leading
                                    )

                                    Spacer(minLength: 0)

                                    Button {
                                        withAnimation(HexMotion.stageAnimation(reduceMotion: reduceMotion)) {
                                            dictation.finishRecording()
                                        }
                                    } label: {
                                        Text("Done")
                                            .font(.system(size: 15, weight: .semibold))
                                            .foregroundStyle(HexTheme.textBase)
                                            .frame(width: handoffDoneWidth, height: handoffControlHeight)
                                            .background(Color.white.opacity(0.15), in: Capsule())
                                            .frame(minHeight: handoffWellHeight, alignment: .trailing)
                                            .contentShape(Rectangle())
                                    }
                                    .buttonStyle(.plain)
                                    .hexControlReveal(
                                        showRecordingControls,
                                        reduceMotion: reduceMotion,
                                        scale: 0.90,
                                        blur: 2.5,
                                        anchor: .trailing
                                    )
                                }
                                .padding(.horizontal, handoffInset)
                            }
                            .frame(maxWidth: .infinity)
                            .frame(height: handoffWellHeight)
                            .clipShape(Capsule())
                            .hexControlReveal(showScope, reduceMotion: reduceMotion, scale: 0.96, blur: 0.0)

                            VStack(spacing: 12) {
                                ProgressView(value: dictation.modelPreparation.fractionCompleted)
                                    .tint(HexTheme.textBase)
                                    .frame(maxWidth: 200)
                                    .hexControlReveal(showPreparing, reduceMotion: reduceMotion, scale: 0.96, blur: 2.0)

                                Button("Try again") {
                                    withAnimation(HexMotion.stageAnimation(reduceMotion: reduceMotion)) {
                                        dictation.prepareModel()
                                    }
                                }
                                .font(.system(size: 15, weight: .semibold))
                                .foregroundStyle(HexTheme.textBase)
                                .padding(.horizontal, 18)
                                .frame(height: 36)
                                .background(Color.white.opacity(0.15), in: Capsule())
                                .buttonStyle(.plain)
                                .hexControlReveal(showFailedRetry, reduceMotion: reduceMotion, scale: 0.94, blur: 2.5)
                            }
                            .hexControlReveal(showPreparing || showFailedRetry, reduceMotion: reduceMotion, scale: 0.96, blur: 2.5)
                        }
                        .frame(minHeight: handoffWellHeight)
                    }
                    .frame(maxWidth: .infinity)

                    Spacer()
                }
                .frame(maxWidth: 500)
                .padding(.horizontal, 20)
                .padding(.top, 14)
                .frame(maxWidth: .infinity, maxHeight: .infinity)

                // Full-bottom-width Home-Indicator return cue (21 pt flush with screen bottom => center 10.5 pt
                // above bottom edge, aligned dead-on the iOS Home Indicator pill across the full screen width).
                if hasHomeIndicator {
                    VStack(spacing: 3) {
                        Text("Swipe bar to return")
                            .font(.system(size: 11.5, weight: .medium))
                            .tracking(0.1)
                            .foregroundStyle(HexTheme.textFaint)
                            .opacity(showHomeSwipeCue ? 1 : 0)

                        DotSwipeCueView(isActive: showHomeSwipeCue)
                            .frame(width: geo.size.width, height: 21)
                    }
                    .allowsHitTesting(false)
                    .offset(y: bottomInset)
                }
            }
            .animation(HexMotion.stageAnimation(reduceMotion: reduceMotion), value: mode)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: showHomeSwipeCue)
        }
        .persistentSystemOverlays(.hidden)
    }

    // MARK: - Pieces

    /// `offer`: a language (BCP 47) that works here, when the one chosen doesn't yet.
    private func errorBanner(_ message: String, offer: String? = nil) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "exclamationmark.circle.fill")
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.danger)
            Text(message)
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textBase)
            Spacer(minLength: 0)
            if let offer {
                Button("Use \(SpeechModelUnavailable.interfaceName(offer))") {
                    dictation.useOfferedLanguage()
                }
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textBase)
                .padding(.horizontal, 12)
                .frame(height: 28)
                .background(Color.white.opacity(0.09), in: Capsule())
                .buttonStyle(.plain)
                .fixedSize()
            }
        }
        .padding(14)
        .background(
            HexTheme.dangerSurface,
            in: RoundedRectangle(cornerRadius: 14, style: .continuous)
        )
    }

    private func noticeBanner(_ message: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "info.circle.fill")
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textMuted)
            Text(message)
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textBase)
            Spacer(minLength: 0)
        }
        .padding(14)
        .background(
            Color.white.opacity(0.055),
            in: RoundedRectangle(cornerRadius: 14, style: .continuous)
        )
    }

    private var primaryActionLabel: String {
        switch dictation.phase {
        case .modelRequired, .failed:
            String(localized: "Set up")
        default:
            String(localized: "Dictate")
        }
    }

    private var primaryActionIcon: String {
        switch dictation.phase {
        case .modelRequired, .failed:
            "arrow.down"
        default:
            "mic.fill"
        }
    }

    private var handoffStatusLine: String? {
        if let notice = dictation.handoffNotice {
            return notice
        }
        switch dictation.phase {
        case .failed(let message):
            return message
        case .requestingPermission:
            return String(localized: "Allow microphone access")
        case .preparingModel:
            return dictation.modelPreparation.detail
        case .transcribing:
            return String(localized: "Transcribing…")
        case .ready where dictation.keyboardSessionEnabled:
            return String(localized: "Ready")
        default:
            return nil
        }
    }
}

#Preview {
    ContentView(
        dictation: DictationController.shared,
        openedFromKeyboard: .constant(false),
        showSetup: .constant(false)
    )
}
