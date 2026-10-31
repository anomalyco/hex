import SwiftUI
import UIKit

struct ContentView: View {
    @Environment(\.openURL) private var openURL
    @Environment(\.scenePhase) private var scenePhase
    @Bindable var dictation: DictationController
    @Binding var openedFromKeyboard: Bool
    @Binding var showSetup: Bool
    @State private var setupMonitor = KeyboardSetupMonitor()
    @State private var practiceText = ""
    @State private var idleTriggerWidth: CGFloat = 98
    @State private var doneButtonWidth: CGFloat = 70
    @FocusState private var practiceFocused: Bool

    private static var autoFocusPracticeField: Bool {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else { return false }
        return args[idx + 1].hasPrefix("hosted-keyboard")
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
        .animation(.easeOut(duration: 0.16), value: openedFromKeyboard)
    }

    private var scopePhase: DotScopePhase {
        switch dictation.phase {
        case .recording:
            .recording(loudnessDB: dictation.loudnessDB)
        case .transcribing:
            .processing
        default:
            .hidden
        }
    }

    private var stageScopePhase: DotScopePhase {
        switch dictation.phase {
        case .recording:
            .recording(loudnessDB: dictation.loudnessDB)
        case .transcribing:
            .processing
        case .preparingModel, .requestingPermission:
            .hidden
        default:
            .idle
        }
    }

    private var setupComplete: Bool {
        setupMonitor.keyboardReady
            && setupMonitor.microphone == .granted
            && dictation.isModelReady
    }

    // MARK: - Standalone App

    private var standaloneView: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                headerRow

                if showSetup || !setupComplete {
                    KeyboardSetupCard(dictation: dictation, monitor: setupMonitor)
                }

                stageBar

                if !dictation.transcript.isEmpty {
                    transcriptCard
                }

                if case .failed(let message) = dictation.phase {
                    errorBanner(message)
                }

                settingsCard
                practiceField
            }
            .frame(maxWidth: 500)
            .padding(.horizontal, 20)
            .padding(.top, 14)
            .padding(.bottom, 28)
            .frame(maxWidth: .infinity)
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
        HStack(alignment: .center, spacing: 9) {
            HexMarkView(
                size: 20,
                color: dictation.isRecording ? HexTheme.warmPearl : HexTheme.textBase
            )

            Text("Hex")
                .font(.system(size: 20, weight: .semibold))
                .tracking(-0.3)
                .foregroundStyle(HexTheme.textBase)

            Spacer()

            if dictation.isRecording {
                HStack(spacing: 6) {
                    HexStatusDot(color: HexTheme.danger, size: 7)
                    Text(HexTheme.formatClock(dictation.recordingDuration))
                        .font(.system(size: 12, weight: .semibold))
                        .monospacedDigit()
                        .foregroundStyle(HexTheme.textBase)
                }
                .padding(.horizontal, 10)
                .frame(height: 28)
                .background(HexTheme.bgBase, in: Capsule())
                .overlay(Capsule().stroke(HexTheme.borderWeak, lineWidth: 1))
            } else if dictation.keyboardSessionEnabled {
                Button {
                    dictation.endWarmSession()
                } label: {
                    HStack(spacing: 6) {
                        HexStatusDot(color: HexTheme.warmMic, size: 6)
                        Text(HexTheme.formatRemaining(dictation.keyboardSessionRemaining))
                            .font(.system(size: 12, weight: .medium))
                            .monospacedDigit()
                            .foregroundStyle(HexTheme.textMuted)
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 28)
                    .background(HexTheme.bgBase, in: Capsule())
                    .overlay(Capsule().stroke(HexTheme.borderWeak, lineWidth: 1))
                }
                .buttonStyle(.plain)
                .accessibilityLabel("End warm microphone session")
            }
        }
        .frame(height: 30)
    }

    private var stageBar: some View {
        let isActive = scopePhase != .hidden
        let showScope = stageScopePhase != .hidden
        let leadInset: Float = (isActive && dictation.isRecording) ? 52.0 : 0.0
        let trailInset: Float = if isActive {
            dictation.isRecording ? Float(doneButtonWidth + 12.0) : 0.0
        } else {
            Float(idleTriggerWidth + 12.0)
        }

        return ZStack {
            Capsule()
                .fill(HexTheme.bgBase)
                .overlay(
                    Capsule()
                        .stroke(isActive ? HexTheme.borderStrong : HexTheme.borderWeak, lineWidth: 1)
                )

            if showScope {
                DotScopeView(
                    phase: stageScopePhase,
                    tuning: .bar(leadingInset: leadInset, trailingInset: trailInset, fade: 16.0, spacing: 3.2),
                    drawPlate: false
                )
                .clipShape(Capsule())
                .allowsHitTesting(false)
            }

            if isActive {
                if dictation.isRecording {
                    HStack(spacing: 10) {
                        Button {
                            dictation.cancelCurrentJob()
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(HexTheme.textMuted)
                                .frame(width: 40, height: 40)
                                .background(HexTheme.layer02, in: Circle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Cancel")

                        Spacer(minLength: 0)

                        Button {
                            dictation.finishRecording()
                        } label: {
                            Text("Done")
                                .font(.system(size: 14, weight: .semibold))
                                .foregroundStyle(HexTheme.textContrast)
                                .padding(.horizontal, 16)
                                .frame(height: 40)
                                .background(HexTheme.bgContrast, in: Capsule())
                        }
                        .buttonStyle(.plain)
                        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { doneButtonWidth = $0 }
                    }
                    .padding(.horizontal, 8)
                }
            } else if dictation.phase == .preparingModel {
                HStack(spacing: 12) {
                    Text("Preparing model…")
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                    ProgressView(value: dictation.modelPreparation.fractionCompleted)
                        .tint(HexTheme.textBase)
                }
                .padding(.horizontal, 18)
            } else if dictation.phase == .requestingPermission {
                Text("Allow microphone access…")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(HexTheme.textMuted)
            } else {
                Button(action: dictation.performPrimaryAction) {
                    HStack(spacing: 0) {
                        Spacer(minLength: 0)
                        HStack(spacing: 6) {
                            Image(systemName: primaryActionIcon)
                                .font(.system(size: 13, weight: .semibold))
                            Text(primaryActionLabel)
                                .font(.system(size: 14, weight: .semibold))
                        }
                        .foregroundStyle(HexTheme.textContrast)
                        .padding(.leading, 15)
                        .padding(.trailing, 18)
                        .frame(height: 40)
                        .background(HexTheme.bgContrast, in: Capsule())
                        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { idleTriggerWidth = $0 }
                    }
                    .padding(.horizontal, 8)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .contentShape(Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .frame(height: 56)
        .animation(.spring(duration: 0.28, bounce: 0.0), value: isActive)
    }

    private var transcriptCard: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(dictation.transcript)
                .font(.system(size: 16))
                .lineSpacing(3)
                .foregroundStyle(HexTheme.textBase)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: 6) {
                Spacer()

                Button(dictation.copied ? "Copied" : "Copy") {
                    dictation.copyTranscript()
                }
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(dictation.copied ? HexTheme.success : HexTheme.textBase)
                .padding(.horizontal, 11)
                .frame(height: 28)
                .background(HexTheme.layer02, in: Capsule())

                Button("Clear") {
                    dictation.clearTranscript()
                }
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
                .padding(.horizontal, 11)
                .frame(height: 28)
                .background(HexTheme.layer01, in: Capsule())
            }
            .buttonStyle(.plain)
        }
        .padding(16)
        .background(
            HexTheme.bgBase,
            in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
        )
        .overlay(
            RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
                .stroke(HexTheme.borderWeak, lineWidth: 1)
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
                    onSelect: dictation.selectWarmDuration
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
                        .font(.system(size: 12, weight: .semibold))
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
        .overlay(
            RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
                .stroke(HexTheme.borderWeak, lineWidth: 1)
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
            .overlay(
                RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
                    .stroke(practiceFocused ? HexTheme.borderStrong : HexTheme.borderWeak, lineWidth: 1)
            )
            .onAppear {
                if Self.autoFocusPracticeField {
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.15) {
                        practiceFocused = true
                    }
                }
            }
    }

    // MARK: - Cold-Start Handoff Screen

    private var keyboardHandoffView: some View {
        VStack(spacing: 0) {
            HStack(alignment: .center, spacing: 9) {
                HexMarkView(
                    size: 20,
                    color: dictation.isRecording ? HexTheme.warmPearl : HexTheme.textBase
                )

                Text("Hex")
                    .font(.system(size: 20, weight: .semibold))
                    .tracking(-0.3)
                    .foregroundStyle(HexTheme.textBase)

                Spacer()

                Button("Settings") {
                    openedFromKeyboard = false
                }
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.textMuted)
                .padding(.horizontal, 12)
                .frame(height: 30)
                .background(HexTheme.bgBase, in: Capsule())
                .overlay(Capsule().stroke(HexTheme.borderWeak, lineWidth: 1))
                .buttonStyle(.plain)
            }

            Spacer()

            VStack(spacing: 18) {
                if scopePhase != .hidden {
                    DotScopeView(
                        phase: scopePhase,
                        tuning: .handoff,
                        drawPlate: true
                    )
                    .frame(maxWidth: .infinity)
                    .frame(height: 64)
                } else if case .failed = dictation.phase {
                    Image(systemName: "exclamationmark.circle.fill")
                        .font(.system(size: 24))
                        .foregroundStyle(HexTheme.danger)
                }

                if dictation.isRecording {
                    Text(HexTheme.formatClock(dictation.recordingDuration))
                        .font(.system(size: 14, weight: .medium))
                        .monospacedDigit()
                        .foregroundStyle(HexTheme.textMuted)

                    HStack(spacing: 8) {
                        Button("Cancel") {
                            dictation.cancelCurrentJob()
                        }
                        .font(.system(size: 14, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                        .padding(.horizontal, 16)
                        .frame(height: 38)
                        .background(HexTheme.layer01, in: Capsule())

                        Button("Done") {
                            dictation.finishRecording()
                        }
                        .font(.system(size: 14, weight: .semibold))
                        .foregroundStyle(HexTheme.textContrast)
                        .padding(.horizontal, 18)
                        .frame(height: 38)
                        .background(HexTheme.bgContrast, in: Capsule())
                    }
                    .buttonStyle(.plain)
                } else if let status = handoffStatusLine {
                    Text(status)
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                        .multilineTextAlignment(.center)
                }

                if dictation.phase == .preparingModel {
                    ProgressView(value: dictation.modelPreparation.fractionCompleted)
                        .tint(HexTheme.textBase)
                        .frame(maxWidth: 200)
                }

                if case .failed = dictation.phase {
                    Button("Try again") {
                        dictation.prepareModel()
                    }
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(HexTheme.textContrast)
                    .padding(.horizontal, 18)
                    .frame(height: 38)
                    .background(HexTheme.bgContrast, in: Capsule())
                    .buttonStyle(.plain)
                }
            }
            .frame(maxWidth: .infinity)

            Spacer()

            if dictation.isRecording || dictation.keyboardSessionEnabled {
                VStack(spacing: 14) {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 6) {
                            ForEach(HostAppCatalog.quickChips) { app in
                                let isSelected = dictation.returnAppID == app.id
                                Button {
                                    dictation.returnToHostApp(app, remember: true)
                                } label: {
                                    HStack(spacing: 4) {
                                        Text(app.name)
                                            .font(.system(size: 13, weight: .medium))
                                        Image(systemName: "arrow.up.forward")
                                            .font(.system(size: 10, weight: .semibold))
                                    }
                                    .foregroundStyle(isSelected ? HexTheme.textBase : HexTheme.textMuted)
                                    .padding(.horizontal, 12)
                                    .frame(height: 34)
                                    .background(isSelected ? HexTheme.layer02 : HexTheme.layer01, in: Capsule())
                                    .overlay(
                                        Capsule().stroke(
                                            isSelected ? HexTheme.borderStrong : HexTheme.borderWeak,
                                            lineWidth: 1
                                        )
                                    )
                                }
                                .buttonStyle(.plain)
                            }
                        }
                    }
                    .contentMargins(.horizontal, 20, for: .scrollContent)
                    .padding(.horizontal, -20)

                    VStack(spacing: 7) {
                        Text("Swipe bottom edge to return")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(HexTheme.textMuted)

                        DotSwipeCueView()
                            .frame(maxWidth: 248)
                            .frame(height: 18)
                    }
                    .padding(.bottom, 2)
                }
            }
        }
        .frame(maxWidth: 500)
        .padding(.horizontal, 20)
        .padding(.top, 14)
        .padding(.bottom, 16)
        .frame(maxWidth: .infinity)
    }

    // MARK: - Pieces

    private func rowTitle(_ text: String) -> some View {
        Text(text)
            .font(.system(size: 14, weight: .regular))
            .foregroundStyle(HexTheme.textBase)
    }

    private var rowDivider: some View {
        Rectangle()
            .fill(HexTheme.borderWeak)
            .frame(height: 1)
            .padding(.leading, 16)
    }

    private func menuValueLabel(_ value: String) -> some View {
        HStack(spacing: 5) {
            Text(value)
                .font(.system(size: 14, weight: .regular))
                .foregroundStyle(HexTheme.textMuted)
            Image(systemName: "chevron.up.chevron.down")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(HexTheme.textFaint)
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
                        .font(.system(size: 12, weight: isSelected ? .semibold : .medium))
                        .foregroundStyle(isSelected ? HexTheme.textBase : HexTheme.textMuted)
                        .padding(.horizontal, 10)
                        .frame(height: 28)
                        .background(
                            isSelected ? HexTheme.layer02 : Color.clear,
                            in: RoundedRectangle(cornerRadius: 7, style: .continuous)
                        )
                        .contentShape(RoundedRectangle(cornerRadius: 7, style: .continuous))
                }
                .buttonStyle(.plain)
            }
        }
        .padding(2)
        .background(
            HexTheme.layer01,
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
        .overlay(
            RoundedRectangle(cornerRadius: 14, style: .continuous)
                .stroke(HexTheme.danger.opacity(0.25), lineWidth: 1)
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
