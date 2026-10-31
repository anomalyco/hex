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
        config.baseForegroundColor = UIColor(white: 0.92, alpha: 1)
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
            .background(isPressing ? HexTheme.layer02 : HexTheme.layer01, in: RoundedRectangle(cornerRadius: 10))
            .overlay(
                RoundedRectangle(cornerRadius: 10)
                    .stroke(HexTheme.borderWeak, lineWidth: 1)
            )
            .contentShape(RoundedRectangle(cornerRadius: 10))
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

extension View {
    /// iOS 26's soft scroll-edge effect blurs the whole content of a short
    /// scroll view (the 30 pt return-app tray), so turn it off there.
    /// Earlier versions have no edge effect and are left unchanged.
    @ViewBuilder
    func hexScrollEdgeEffectHidden() -> some View {
        if #available(iOS 26.0, *) {
            scrollEdgeEffectHidden(true, for: .all)
        } else {
            self
        }
    }
}

enum KeyboardSetupCopy {
    static let fullAccessPath = "Allow Full Access: Settings › Apps › Hex › Keyboards"
}

struct KeyboardSurfaceView: View {
    @ObservedObject var model: KeyboardViewModel

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
                            .frame(height: 28)
                            .background(active ? HexTheme.layer02 : HexTheme.layer01, in: Capsule())
                    }
                    .buttonStyle(.plain)
                }
                Spacer(minLength: 4)
                closeTrayButton
            }
            .frame(height: 30)

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
                .hexScrollEdgeEffectHidden()
                closeTrayButton
            }
            .frame(height: 30)

        case .none:
            HStack(spacing: 8) {
                statusLead
                Spacer(minLength: 4)

                if !model.isWarmAvailable && model.hasFullAccess {
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
                    .accessibilityLabel("Return app after cold start")
                }

                if model.hasFullAccess {
                    Button {
                        model.activeTray = .style
                    } label: {
                        HStack(spacing: 4) {
                            Text(model.style.label)
                                .font(.system(size: 12, weight: .medium))
                            Image(systemName: "chevron.down")
                                .font(.system(size: 9, weight: .semibold))
                        }
                        .foregroundStyle(HexTheme.textBase)
                        .padding(.horizontal, 10)
                        .frame(height: 26)
                        .background(HexTheme.layer01, in: Capsule())
                        .overlay(Capsule().stroke(HexTheme.borderWeak, lineWidth: 1))
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Dictation style: \(model.style.label)")
                }
            }
            .frame(height: 30)
        }
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
                .frame(height: 28)
                .background(active ? HexTheme.layer02 : HexTheme.layer01, in: Capsule())
        }
        .buttonStyle(.plain)
    }

    private var closeTrayButton: some View {
        Button {
            model.activeTray = .none
        } label: {
            Image(systemName: "xmark")
                .font(.system(size: 11, weight: .semibold))
                .foregroundStyle(HexTheme.textMuted)
                .frame(width: 28, height: 28)
                .background(HexTheme.layer01, in: Circle())
        }
        .buttonStyle(.plain)
    }

    @ViewBuilder
    private var statusLead: some View {
        if !model.hasFullAccess {
            HStack(spacing: 6) {
                HexStatusDot(color: HexTheme.warmMic, size: 7)
                Text(KeyboardSetupCopy.fullAccessPath)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
                    .lineLimit(1)
                    .minimumScaleFactor(0.7)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Full Access is off. \(KeyboardSetupCopy.fullAccessPath)")
        } else if model.isWarmAvailable {
            switch model.snapshot.state {
            case .recording:
                HStack(spacing: 6) {
                    HexStatusDot(color: HexTheme.danger, size: 8)
                    Text(HexTheme.formatClock(model.snapshot.recordingElapsed(at: model.now)))
                        .font(.system(size: 13, weight: .semibold))
                        .monospacedDigit()
                        .foregroundStyle(HexTheme.textBase)
                }
            case .transcribing:
                HStack(spacing: 6) {
                    ProgressView()
                        .controlSize(.mini)
                        .tint(HexTheme.textMuted)
                    Text("Transcribing…")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                }
            case .failed:
                HStack(spacing: 6) {
                    HexStatusDot(color: HexTheme.danger, size: 7)
                    Text(model.snapshot.message ?? "Hex needs attention")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                        .lineLimit(1)
                }
            default:
                HStack(spacing: 6) {
                    HexStatusDot(color: HexTheme.warmMic, size: 7)
                    Text(
                        model.snapshot.message
                            ?? "Warm · \(HexTheme.formatRemaining(model.snapshot.expiresAt - model.now))"
                    )
                    .font(.system(size: 12, weight: .medium))
                    .monospacedDigit()
                    .foregroundStyle(HexTheme.textMuted)
                    .lineLimit(1)
                }
            }
        } else {
            HStack(spacing: 6) {
                HexStatusDot(color: HexTheme.textFaint, size: 6)
                Text(
                    model.snapshot.message
                        ?? (model.isModelInstalled ? "Hex · mic asleep" : "Hex · setup needed")
                )
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(HexTheme.textFaint)
                .lineLimit(1)
            }
        }
    }

    @ViewBuilder
    private var primaryStage: some View {
        if !model.hasFullAccess {
            Button {
                model.onOpenURL?(KeyboardLaunchRequest.setup.url)
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "arrow.up.forward.app.fill")
                        .font(.system(size: 14, weight: .semibold))
                    Text("Open Hex setup")
                        .font(.system(size: 15, weight: .semibold))
                }
                .foregroundStyle(HexTheme.textContrast)
                .frame(maxWidth: .infinity, minHeight: 50)
                .background(HexTheme.bgContrast, in: Capsule())
            }
            .buttonStyle(.plain)
        } else if !model.isWarmAvailable || model.snapshot.state == .failed {
            Button {
                model.onOpenURL?(model.coldStartURL)
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "mic.fill")
                        .font(.system(size: 14, weight: .semibold))
                    Text(model.isModelInstalled ? "Start Hex" : "Set up Hex")
                        .font(.system(size: 15, weight: .semibold))
                }
                .foregroundStyle(HexTheme.textContrast)
                .frame(maxWidth: .infinity, minHeight: 50)
                .background(HexTheme.bgContrast, in: Capsule())
            }
            .buttonStyle(.plain)
        } else if model.snapshot.state == .recording {
            HStack(spacing: 10) {
                Button {
                    model.onCancelTap?()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(width: 40, height: 40)
                        .background(HexTheme.layer02, in: Circle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Cancel dictation")

                Spacer(minLength: 4)

                DotScopeBar(
                    level: model.snapshot.audioLevel,
                    barCount: 18,
                    active: !model.isOptimisticStopping,
                    height: 24
                )

                Spacer(minLength: 4)

                Button {
                    model.onStopTap?()
                } label: {
                    HStack(spacing: 6) {
                        if model.isOptimisticStopping {
                            ProgressView()
                                .controlSize(.mini)
                                .tint(HexTheme.textContrast)
                        } else {
                            Image(systemName: "checkmark")
                                .font(.system(size: 13, weight: .bold))
                        }
                        Text(model.isOptimisticStopping ? "Finishing" : "Done")
                            .font(.system(size: 14, weight: .semibold))
                    }
                    .foregroundStyle(HexTheme.textContrast)
                    .padding(.horizontal, 16)
                    .frame(height: 40)
                    .background(HexTheme.bgContrast, in: Capsule())
                }
                .buttonStyle(.plain)
                .disabled(model.isOptimisticStopping)
            }
            .padding(.horizontal, 6)
            .frame(maxWidth: .infinity, minHeight: 50)
            .background(HexTheme.bgBase, in: Capsule())
            .overlay(Capsule().stroke(HexTheme.borderStrong, lineWidth: 1))
        } else if model.snapshot.state == .transcribing || model.isOptimisticStarting {
            HStack(spacing: 8) {
                ProgressView()
                    .controlSize(.small)
                    .tint(HexTheme.textBase)
                Text(model.isOptimisticStarting ? "Opening mic…" : "Transcribing on iPhone…")
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
            }
            .frame(maxWidth: .infinity, minHeight: 50)
            .background(HexTheme.bgBase, in: Capsule())
            .overlay(Capsule().stroke(HexTheme.borderWeak, lineWidth: 1))
        } else {
            Button {
                model.onDictateTap?()
            } label: {
                HStack(spacing: 8) {
                    Image(systemName: "mic.fill")
                        .font(.system(size: 14, weight: .semibold))
                    Text("Dictate")
                        .font(.system(size: 15, weight: .semibold))
                }
                .foregroundStyle(HexTheme.textContrast)
                .frame(maxWidth: .infinity, minHeight: 50)
                .background(HexTheme.bgContrast, in: Capsule())
            }
            .buttonStyle(.plain)
        }
    }

    private var utilityRow: some View {
        HStack(spacing: 6) {
            if model.needsGlobeKey {
                GlobeKeyRepresentable(controller: model.inputViewController)
                    .frame(width: 44, height: 40)
                    .background(HexTheme.layer01, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10)
                            .stroke(HexTheme.borderWeak, lineWidth: 1)
                    )
            }

            Button {
                model.onSpace?()
            } label: {
                Text("space")
                    .font(.system(size: 14, weight: .regular))
                    .foregroundStyle(HexTheme.textBase)
                    .frame(maxWidth: .infinity)
                    .frame(height: 40)
                    .background(HexTheme.layer01, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10)
                            .stroke(HexTheme.borderWeak, lineWidth: 1)
                    )
                    .contentShape(RoundedRectangle(cornerRadius: 10))
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
                    .background(HexTheme.layer01, in: RoundedRectangle(cornerRadius: 10))
                    .overlay(
                        RoundedRectangle(cornerRadius: 10)
                            .stroke(HexTheme.borderWeak, lineWidth: 1)
                    )
                    .contentShape(RoundedRectangle(cornerRadius: 10))
            }
            .buttonStyle(.plain)
            .accessibilityLabel(model.returnKeyLabel)
        }
    }
}
