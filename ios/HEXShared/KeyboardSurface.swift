import Combine
import SwiftUI
import UIKit

@MainActor
final class KeyboardViewModel: ObservableObject {
    enum PickerTray: Equatable {
        case none
        case style
        case returnApp
    }

    struct PendingCommand: Equatable {
        let kind: KeyboardCommandKind
        let jobID: String
        let sentAt: TimeInterval
    }

    static let commandAckTimeout: TimeInterval = 1.6

    @Published var hasFullAccess = true
    @Published var needsGlobeKey = true
    @Published var returnKeyLabel = "Return"
    @Published var snapshot: KeyboardSnapshot = .offline
    @Published var isModelInstalled = false
    @Published var style: DictationStyle = .polished
    @Published var returnAppID: String?
    @Published var activeTray: PickerTray = .none
    @Published var now: TimeInterval = Date().timeIntervalSince1970
    @Published var coldStartURL: URL = KeyboardLaunchRequest.startSession(requestedAt: nil, returnAppID: nil).url
    @Published var pendingCommand: PendingCommand?

    var onDictateTap: (() -> Void)?
    var onStopTap: (() -> Void)?
    var onCancelTap: (() -> Void)?
    var onStyleSelected: ((DictationStyle) -> Void)?
    var onReturnAppSelected: ((String?) -> Void)?
    var onSpace: (() -> Void)?
    var onDelete: (() -> Void)?
    var onReturn: (() -> Void)?
    var onOpenURL: ((URL) -> Void)?
    weak var inputViewController: UIInputViewController?

    var isWarmAvailable: Bool {
        snapshot.isAvailable(at: now)
    }

    var selectedReturnApp: HostApp? {
        HostAppCatalog.app(for: returnAppID)
    }

    var isOptimisticStarting: Bool {
        guard let pending = pendingCommand, pending.kind == .start else { return false }
        return now - pending.sentAt < Self.commandAckTimeout && snapshot.state != .recording
    }

    var isOptimisticStopping: Bool {
        guard let pending = pendingCommand, pending.kind == .stop else { return false }
        return now - pending.sentAt < Self.commandAckTimeout && snapshot.state == .recording
    }

    var dotScopePhase: DotScopePhase {
        if isOptimisticStopping || snapshot.state == .transcribing {
            return .processing
        }
        if isOptimisticStarting {
            return .recording(loudnessDB: -62.0)
        }
        if snapshot.state == .recording {
            return .recording(loudnessDB: snapshot.effectiveLoudnessDB)
        }
        return .idle
    }
}

private struct GlobeKeyRepresentable: UIViewRepresentable {
    weak var controller: UIInputViewController?

    func makeUIView(context: Context) -> UIButton {
        let button = UIButton(type: .system)
        var config = UIButton.Configuration.plain()
        config.image = UIImage(
            systemName: "globe",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 15, weight: .medium)
        )
        config.baseForegroundColor = UIColor(white: 0.90, alpha: 1)
        button.configuration = config
        button.accessibilityLabel = "Next keyboard"
        if let controller {
            button.addTarget(
                controller,
                action: #selector(UIInputViewController.handleInputModeList(from:with:)),
                for: .allTouchEvents
            )
        }
        return button
    }

    func updateUIView(_ uiView: UIButton, context: Context) {}
}

private struct RepeatDeleteKey: View {
    let onDelete: () -> Void
    @State private var isPressing = false
    @State private var repeatTask: Task<Void, Never>?

    var body: some View {
        Image(systemName: "delete.left")
            .font(.system(size: 15, weight: .medium))
            .foregroundStyle(HexTheme.textBase)
            .frame(width: 52, height: 40)
            .background(
                isPressing ? HexTheme.layer02 : HexTheme.layer01,
                in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    .stroke(HexTheme.borderWeak, lineWidth: 1)
            )
            .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            .gesture(
                DragGesture(minimumDistance: 0)
                    .onChanged { _ in
                        guard !isPressing else { return }
                        isPressing = true
                        onDelete()
                        repeatTask = Task { @MainActor in
                            try? await Task.sleep(for: .milliseconds(360))
                            while !Task.isCancelled {
                                onDelete()
                                try? await Task.sleep(for: .milliseconds(75))
                            }
                        }
                    }
                    .onEnded { _ in
                        isPressing = false
                        repeatTask?.cancel()
                        repeatTask = nil
                    }
            )
            .accessibilityLabel("Delete")
    }
}

enum KeyboardSetupCopy {
    static let fullAccessPath = "Allow Full Access: Settings › Apps › Hex › Keyboards"
}

struct KeyboardSurfaceView: View {
    @ObservedObject var model: KeyboardViewModel

    private var isStageActive: Bool {
        switch model.dotScopePhase {
        case .recording, .processing, .failed:
            true
        case .hidden, .idle:
            false
        }
    }

    var body: some View {
        VStack(spacing: 8) {
            topBar
            primaryStage
            utilityRow
        }
        .padding(.horizontal, 10)
        .padding(.top, 8)
        .padding(.bottom, 6)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HexTheme.bgDeep)
        .preferredColorScheme(.dark)
    }

    // MARK: - Top Bar (26 pt)

    @ViewBuilder
    private var topBar: some View {
        switch model.activeTray {
        case .style:
            HStack(spacing: 4) {
                ForEach(DictationStyle.allCases, id: \.self) { candidate in
                    let active = candidate == model.style
                    Button {
                        model.onStyleSelected?(candidate)
                        model.activeTray = .none
                    } label: {
                        Text(candidate.label)
                            .font(.system(size: 12, weight: active ? .semibold : .medium))
                            .foregroundStyle(active ? HexTheme.textBase : HexTheme.textMuted)
                            .padding(.horizontal, 10)
                            .frame(height: 26)
                            .background(active ? HexTheme.layer02 : HexTheme.layer01, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
                Spacer(minLength: 4)
                closeTrayButton
            }
            .frame(height: 26)

        case .returnApp:
            HStack(spacing: 4) {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 4) {
                        returnChip(id: nil, label: "Swipe")
                        ForEach(HostAppCatalog.quickChips) { app in
                            returnChip(id: app.id, label: app.name)
                        }
                    }
                }
                closeTrayButton
            }
            .frame(height: 26)

        case .none:
            HStack(spacing: 8) {
                leadingAccessory
                Spacer(minLength: 4)
                if model.hasFullAccess {
                    stylePill
                }
            }
            .frame(height: 26)
        }
    }

    @ViewBuilder
    private var leadingAccessory: some View {
        if !model.hasFullAccess {
            HStack(spacing: 6) {
                HexStatusDot(color: HexTheme.warmMic, size: 6)
                Text(KeyboardSetupCopy.fullAccessPath)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(HexTheme.textMuted)
                    .lineLimit(1)
                    .minimumScaleFactor(0.75)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Full Access is off. \(KeyboardSetupCopy.fullAccessPath)")
        } else if model.isWarmAvailable {
            switch model.snapshot.state {
            case .recording:
                HStack(spacing: 6) {
                    HexStatusDot(color: HexTheme.danger, size: 7)
                    Text(HexTheme.formatClock(model.snapshot.recordingElapsed(at: model.now)))
                        .font(.system(size: 12, weight: .semibold))
                        .monospacedDigit()
                        .foregroundStyle(HexTheme.textBase)
                }
            case .transcribing:
                EmptyView()
            case .failed:
                if let message = model.snapshot.message, !message.isEmpty {
                    HStack(spacing: 6) {
                        HexStatusDot(color: HexTheme.danger, size: 6)
                        Text(message)
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(HexTheme.textMuted)
                            .lineLimit(1)
                    }
                }
            default:
                if let message = model.snapshot.message, !message.isEmpty {
                    Text(message)
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                        .lineLimit(1)
                } else {
                    HStack(spacing: 5) {
                        HexStatusDot(color: HexTheme.warmMic, size: 6)
                        Text(HexTheme.formatRemaining(model.snapshot.expiresAt - model.now))
                            .font(.system(size: 12, weight: .medium))
                            .monospacedDigit()
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
            }
        } else {
            Button {
                model.activeTray = .returnApp
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: "arrow.uturn.backward")
                        .font(.system(size: 10, weight: .semibold))
                    Text(model.selectedReturnApp?.name ?? "Swipe")
                        .font(.system(size: 12, weight: .medium))
                }
                .foregroundStyle(HexTheme.textMuted)
                .padding(.horizontal, 9)
                .frame(height: 26)
                .background(HexTheme.layer01, in: Capsule())
                .overlay(Capsule().stroke(HexTheme.borderWeak, lineWidth: 1))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Return app")
        }
    }

    private var stylePill: some View {
        Button {
            model.activeTray = .style
        } label: {
            HStack(spacing: 4) {
                Text(model.style.label)
                    .font(.system(size: 12, weight: .medium))
                Image(systemName: "chevron.down")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(HexTheme.textMuted)
            }
            .foregroundStyle(HexTheme.textBase)
            .padding(.horizontal, 10)
            .frame(height: 26)
            .background(HexTheme.layer01, in: Capsule())
            .overlay(Capsule().stroke(HexTheme.borderWeak, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Style: \(model.style.label)")
    }

    private func returnChip(id: String?, label: String) -> some View {
        let active = model.returnAppID == id
        return Button {
            model.onReturnAppSelected?(id)
            model.activeTray = .none
        } label: {
            Text(label)
                .font(.system(size: 12, weight: active ? .semibold : .medium))
                .foregroundStyle(active ? HexTheme.textBase : HexTheme.textMuted)
                .padding(.horizontal, 10)
                .frame(height: 26)
                .background(active ? HexTheme.layer02 : HexTheme.layer01, in: Capsule())
        }
        .buttonStyle(.plain)
    }

    private var closeTrayButton: some View {
        Button {
            model.activeTray = .none
        } label: {
            Image(systemName: "xmark")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(HexTheme.textMuted)
                .frame(width: 26, height: 26)
                .background(HexTheme.layer01, in: Circle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - Unified 52 pt Stage Well

    private var primaryStage: some View {
        let showRecordingControls = (model.snapshot.state == .recording || model.isOptimisticStarting)
            && !model.isOptimisticStopping

        return ZStack {
            Capsule()
                .fill(HexTheme.bgBase)
                .overlay(
                    Capsule()
                        .stroke(isStageActive ? HexTheme.borderStrong : HexTheme.borderWeak, lineWidth: 1)
                )

            DotScopeView(
                phase: model.dotScopePhase,
                tuning: .keyboard,
                drawPlate: false
            )
            .frame(width: 168, height: 38)
            .padding(.leading, isStageActive ? 0 : 12)
            .frame(maxWidth: .infinity, alignment: isStageActive ? .center : .leading)
            .allowsHitTesting(false)

            if isStageActive {
                if showRecordingControls {
                    HStack(spacing: 8) {
                        Button {
                            model.onCancelTap?()
                        } label: {
                            Image(systemName: "xmark")
                                .font(.system(size: 13, weight: .semibold))
                                .foregroundStyle(HexTheme.textMuted)
                                .frame(width: 38, height: 38)
                                .background(HexTheme.layer02, in: Circle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityLabel("Cancel dictation")

                        Spacer(minLength: 0)

                        Button {
                            model.onStopTap?()
                        } label: {
                            Text("Done")
                                .font(.system(size: 14, weight: .semibold))
                                .foregroundStyle(HexTheme.textContrast)
                                .padding(.horizontal, 16)
                                .frame(height: 38)
                                .background(HexTheme.bgContrast, in: Capsule())
                        }
                        .buttonStyle(.plain)
                    }
                    .padding(.horizontal, 7)
                }
            } else if !model.hasFullAccess {
                Button {
                    model.onOpenURL?(KeyboardLaunchRequest.setup.url)
                } label: {
                    idleTrailingTrigger(icon: "arrow.up.forward.app.fill", title: "Open Hex setup")
                }
                .buttonStyle(.plain)
            } else if !model.isWarmAvailable || model.snapshot.state == .failed {
                Button {
                    model.onOpenURL?(model.coldStartURL)
                } label: {
                    idleTrailingTrigger(
                        icon: "mic.fill",
                        title: model.isModelInstalled ? "Dictate" : "Set up"
                    )
                }
                .buttonStyle(.plain)
            } else {
                Button {
                    model.onDictateTap?()
                } label: {
                    idleTrailingTrigger(icon: "mic.fill", title: "Dictate")
                }
                .buttonStyle(.plain)
            }
        }
        .frame(height: 52)
        .animation(.spring(duration: 0.30, bounce: 0.12), value: isStageActive)
    }

    private func idleTrailingTrigger(icon: String, title: String) -> some View {
        HStack(spacing: 0) {
            Spacer(minLength: 0)
            HStack(spacing: 6) {
                Image(systemName: icon)
                    .font(.system(size: 13, weight: .semibold))
                Text(title)
                    .font(.system(size: 14, weight: .semibold))
            }
            .foregroundStyle(HexTheme.textContrast)
            .padding(.leading, 14)
            .padding(.trailing, 16)
            .frame(height: 38)
            .background(HexTheme.bgContrast, in: Capsule())
        }
        .padding(.horizontal, 7)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(Capsule())
    }

    // MARK: - Bottom Utility Key Row (40 pt)

    private var utilityRow: some View {
        HStack(spacing: 6) {
            if model.needsGlobeKey {
                GlobeKeyRepresentable(controller: model.inputViewController)
                    .frame(width: 44, height: 40)
                    .background(
                        HexTheme.layer01,
                        in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                            .stroke(HexTheme.borderWeak, lineWidth: 1)
                    )
            }

            Button {
                model.onSpace?()
            } label: {
                Text("space")
                    .font(.system(size: 14, weight: .regular))
                    .foregroundStyle(HexTheme.textMuted)
                    .frame(maxWidth: .infinity)
                    .frame(height: 40)
                    .background(
                        HexTheme.layer01,
                        in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                            .stroke(HexTheme.borderWeak, lineWidth: 1)
                    )
                    .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }
            .buttonStyle(.plain)

            RepeatDeleteKey {
                model.onDelete?()
            }

            Button {
                model.onReturn?()
            } label: {
                Image(systemName: "return")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
                    .frame(width: 56, height: 40)
                    .background(
                        HexTheme.layer01,
                        in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                            .stroke(HexTheme.borderWeak, lineWidth: 1)
                    )
                    .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityLabel(model.returnKeyLabel)
        }
    }
}
