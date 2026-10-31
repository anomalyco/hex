import SwiftUI
import UIKit

private struct AppRecordingClockLabel: View {
    let dictation: DictationController

    var body: some View {
        let formatted = HexTheme.formatClock(dictation.recordingDuration)
        Text(formatted)
            .font(.system(size: 13, weight: .medium))
            .monospacedDigit()
            .contentTransition(.numericText())
            .foregroundStyle(HexTheme.textBase)
            .animation(.easeOut(duration: 0.16), value: formatted)
    }
}

private struct AppWarmRemainingLabel: View {
    let dictation: DictationController

    var body: some View {
        Text(HexTheme.formatRemaining(dictation.keyboardSessionRemaining))
            .font(.system(size: 13, weight: .medium))
            .monospacedDigit()
            .contentTransition(.numericText())
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
    @State private var setupMonitor = KeyboardSetupMonitor()
    @State private var practiceText = ""
    @FocusState private var practiceFocused: Bool

    private static let stageWellHeight: CGFloat = 48
    private static let stageControlHeight: CGFloat = 36
    private static let stageInset: CGFloat = 6
    private static let stageCancelWidth: CGFloat = 36
    private static let stageDoneWidth: CGFloat = 70

    private static var autoFocusPracticeField: Bool {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else { return false }
        return args[idx + 1].hasPrefix("hosted-keyboard")
    }

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
            } else {
                standaloneView
                    .transition(.opacity)
            }
        }
        .preferredColorScheme(.dark)
        .animation(HexMotion.reducedFade, value: openedFromKeyboard)
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

    private var setupComplete: Bool {
        setupMonitor.keyboardReady
            && setupMonitor.microphone == .granted
            && dictation.isModelReady
    }

    private var hasTranscript: Bool {
        !dictation.transcript.isEmpty
    }

    // MARK: - Standalone App

    private var standaloneView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                headerRow

                if showSetup || !setupComplete {
                    KeyboardSetupCard(dictation: dictation, monitor: setupMonitor)
                        .transition(.opacity.combined(with: .scale(scale: 0.98)))
                }

                stageBar

                if hasTranscript {
                    transcriptCard
                        .transition(.opacity.combined(with: .scale(scale: 0.98)))
                }

                if case .failed(let message) = dictation.phase {
                    errorBanner(message)
                        .transition(.opacity.combined(with: .scale(scale: 0.98)))
                }

                settingsCard
                practiceField
            }
            .frame(maxWidth: 500)
            .padding(.horizontal, 20)
            .padding(.top, 14)
            .padding(.bottom, 28)
            .frame(maxWidth: .infinity)
            .animation(HexMotion.stageAnimation(reduceMotion: reduceMotion), value: appStageMode)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: dictation.keyboardSessionEnabled)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: hasTranscript)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: setupComplete)
        }
        .scrollDismissesKeyboard(.interactively)
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
                .accessibilityLabel("End warm microphone session")
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
                Text("Preparing model…")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(HexTheme.textMuted)
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

                Button(dictation.copied ? "Copied" : "Copy") {
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

    private var settingsCard: some View {
        VStack(spacing: 0) {
            HStack(spacing: 12) {
                rowTitle("Keep mic warm")
                Spacer(minLength: 8)
                compactSegments(
                    items: WarmSessionDuration.allCases,
                    selected: dictation.warmDuration,
                    label: \.shortLabel,
                    onSelect: { duration in
                        withAnimation(HexMotion.settleAnimation(reduceMotion: reduceMotion)) {
                            dictation.selectWarmDuration(duration)
                        }
                    }
                )
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 50)

            rowDivider

            HStack(spacing: 12) {
                rowTitle("Style")
                Spacer(minLength: 8)
                Menu {
                    ForEach(DictationStyle.allCases, id: \.self) { candidate in
                        Button {
                            dictation.setStyle(candidate)
                        } label: {
                            if dictation.style == candidate {
                                Label(candidate.label, systemImage: "checkmark")
                            } else {
                                Text(candidate.label)
                            }
                        }
                    }
                } label: {
                    menuValueLabel(dictation.style.label)
                }
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 48)

            rowDivider

            HStack(spacing: 12) {
                rowTitle("Return to")
                Spacer(minLength: 8)
                Menu {
                    Button {
                        dictation.setReturnAppID(nil)
                    } label: {
                        if dictation.returnAppID == nil {
                            Label("Swipe back", systemImage: "checkmark")
                        } else {
                            Text("Swipe back")
                        }
                    }

                    Divider()

                    ForEach(HostAppCatalog.all) { app in
                        Button {
                            dictation.setReturnAppID(app.id)
                        } label: {
                            if dictation.returnAppID == app.id {
                                Label(app.name, systemImage: "checkmark")
                            } else {
                                Text(app.name)
                            }
                        }
                    }
                } label: {
                    menuValueLabel(dictation.selectedReturnApp?.name ?? "Swipe back")
                }
            }
            .padding(.horizontal, 16)
            .frame(minHeight: 48)

            rowDivider

            Toggle(
                isOn: Binding(
                    get: { dictation.allowBluetoothMic },
                    set: { dictation.setAllowBluetoothMic($0) }
                )
            ) {
                rowTitle("Bluetooth microphone")
            }
            .tint(HexTheme.success)
            .padding(.horizontal, 16)
            .frame(minHeight: 48)

            rowDivider

            Button {
                if let url = URL(string: UIApplication.openSettingsURLString) {
                    openURL(url)
                }
            } label: {
                HStack(spacing: 8) {
                    rowTitle("Keyboard settings")
                    Spacer()
                    Image(systemName: "arrow.up.right")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                }
                .padding(.horizontal, 16)
                .frame(minHeight: 48)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
        }
        .background(
            HexTheme.bgBase,
            in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
        )
    }

    private var practiceField: some View {
        TextField("Test keyboard…", text: $practiceText, axis: .vertical)
            .font(.system(size: 15))
            .foregroundStyle(HexTheme.textBase)
            .focused($practiceFocused)
            .lineLimit(1...4)
            .padding(.horizontal, 16)
            .padding(.vertical, 13)
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )
            .onAppear {
                if Self.autoFocusPracticeField {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                        practiceFocused = true
                    }
                }
            }
    }

    // MARK: - Cold-Start Handoff Screen (unified bar + home-indicator swipe cue)

    private var keyboardHandoffView: some View {
        GeometryReader { geo in
            let bottomInset: CGFloat = Self.simulatesHomeButtonDevice ? 0 : geo.safeAreaInsets.bottom
            let hasHomeIndicator = bottomInset > 0
            let homeBarWidth: CGFloat = round(min(max(geo.size.width * 0.355, 134), 156))
            let swipeCueWidth: CGFloat = min(max(180, geo.size.width - 64), max(264, round(homeBarWidth * 1.95)))

            let mode = appStageMode
            let showScope = handoffScopePhase != .hidden
            let showRecordingControls = mode == .recording
            let showFailedRetry = mode == .failed
            let showPreparing = mode == .preparing
            let status = handoffStatusLine
            let isAutoReturning: Bool = {
                switch dictation.autoReturnState {
                case .attempting, .succeeded:
                    return true
                case .idle, .swipeFallback:
                    return false
                }
            }()
            let showReturnChips = (showRecordingControls || dictation.keyboardSessionEnabled) && !isAutoReturning
            let showHomeSwipeCue = hasHomeIndicator && showReturnChips

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
                            .hexControlReveal(showScope, reduceMotion: reduceMotion, scale: 0.96, blur: 2.5)

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

                    // Quiet app quick-jump strip comfortably above the bottom gesture zone
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            ForEach(HostAppCatalog.quickChips) { app in
                                let isSelected = dictation.returnAppID == app.id
                                Button {
                                    dictation.returnToHostApp(app, remember: true)
                                } label: {
                                    Text(app.name)
                                        .font(.system(size: 13, weight: .medium))
                                        .foregroundStyle(isSelected ? HexTheme.textBase : HexTheme.textMuted)
                                        .padding(.horizontal, 12)
                                        .frame(height: 32)
                                        .background(
                                            Color.white.opacity(isSelected ? 0.15 : 0.075),
                                            in: Capsule()
                                        )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(.horizontal, 20)
                    }
                    .hexScrollEdgeEffectHidden()
                    .hexHorizontalEdgeFade(leading: 16, trailing: 20)
                    .padding(.horizontal, -20)
                    .padding(.bottom, hasHomeIndicator ? 18 : 12)
                    .hexControlReveal(showReturnChips, reduceMotion: reduceMotion, scale: 0.97, blur: 2.5)
                }
                .frame(maxWidth: 500)
                .padding(.horizontal, 20)
                .padding(.top, 14)
                .frame(maxWidth: .infinity, maxHeight: .infinity)

                // Anchored directly onto the physical iOS Home Indicator bar (center 10.5 pt above screen bottom),
                // shown only on Face ID / Dynamic Island devices (`bottomInset > 0`) when swipe-back is needed.
                if hasHomeIndicator {
                    VStack(spacing: 5) {
                        Text("Swipe bar to return")
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(HexTheme.textMuted)

                        DotSwipeCueView(isPaused: !showHomeSwipeCue)
                            .frame(width: swipeCueWidth, height: 18)
                    }
                    .allowsHitTesting(false)
                    .hexControlReveal(showHomeSwipeCue, reduceMotion: reduceMotion, scale: 0.97, blur: 2.0, anchor: .bottom)
                    .offset(y: max(CGFloat(0), bottomInset - 1.5))
                }
            }
            .animation(HexMotion.stageAnimation(reduceMotion: reduceMotion), value: mode)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: showReturnChips)
            .animation(HexMotion.settleAnimation(reduceMotion: reduceMotion), value: showHomeSwipeCue)
        }
        .persistentSystemOverlays(.hidden)
    }

    // MARK: - Pieces

    private func rowTitle(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 15, weight: .regular))
            .foregroundStyle(HexTheme.textBase)
    }

    private var rowDivider: some View {
        Rectangle()
            .fill(HexTheme.borderWeak)
            .frame(height: 0.5)
            .padding(.leading, 16)
    }

    private func menuValueLabel(_ value: String) -> some View {
        HStack(spacing: 5) {
            Text(value)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(HexTheme.textMuted)
            Image(systemName: "chevron.up.chevron.down")
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
        }
        .padding(.vertical, 6)
        .contentShape(Rectangle())
    }

    private func compactSegments<Item: Hashable>(
        items: [Item],
        selected: Item,
        label: KeyPath<Item, String>,
        onSelect: @escaping (Item) -> Void
    ) -> some View {
        HStack(spacing: 2) {
            ForEach(items, id: \.self) { item in
                let isSelected = item == selected
                Button {
                    onSelect(item)
                } label: {
                    Text(item[keyPath: label])
                        .font(.system(size: 13, weight: .medium))
                        .lineLimit(1)
                        .fixedSize(horizontal: true, vertical: false)
                        .foregroundStyle(isSelected ? HexTheme.textBase : HexTheme.textMuted)
                        .padding(.horizontal, 10)
                        .frame(height: 28)
                        .background(
                            isSelected ? Color.white.opacity(0.14) : Color.clear,
                            in: RoundedRectangle(cornerRadius: 7, style: .continuous)
                        )
                        .contentShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
                }
                .buttonStyle(.plain)
            }
        }
        .fixedSize(horizontal: true, vertical: false)
        .padding(2)
        .background(
            Color.white.opacity(0.055),
            in: RoundedRectangle(cornerRadius: 9, style: .continuous)
        )
    }

    private func errorBanner(_ message: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: "exclamationmark.circle.fill")
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.danger)
            Text(message)
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textBase)
            Spacer(minLength: 0)
        }
        .padding(14)
        .background(
            HexTheme.dangerSurface,
            in: RoundedRectangle(cornerRadius: 14, style: .continuous)
        )
    }

    private var primaryActionLabel: String {
        switch dictation.phase {
        case .modelRequired, .failed:
            "Set up"
        default:
            "Dictate"
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
        if case .attempting(let app) = dictation.autoReturnState {
            return app.name
        }
        switch dictation.phase {
        case .failed(let message):
            return message
        case .requestingPermission:
            return "Allow microphone access"
        case .preparingModel:
            return "Preparing model…"
        case .transcribing:
            return "Transcribing…"
        case .ready where dictation.keyboardSessionEnabled:
            return "Ready"
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
