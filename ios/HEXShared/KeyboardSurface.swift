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
    let isDark: Bool

    func makeUIView(context: Context) -> UIButton {
        let button = UIButton(type: .system)
        var config = UIButton.Configuration.plain()
        config.image = UIImage(
            systemName: "globe",
            withConfiguration: UIImage.SymbolConfiguration(pointSize: 15, weight: .medium)
        )
        config.baseForegroundColor = isDark
            ? UIColor(white: 0.90, alpha: 1)
            : UIColor(red: 0x14 / 255.0, green: 0x16 / 255.0, blue: 0x1B / 255.0, alpha: 1)
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

    func updateUIView(_ uiView: UIButton, context: Context) {
        uiView.configuration?.baseForegroundColor = isDark
            ? UIColor(white: 0.90, alpha: 1)
            : UIColor(red: 0x14 / 255.0, green: 0x16 / 255.0, blue: 0x1B / 255.0, alpha: 1)
    }
}

private struct KeyboardChromePalette {
    let isDark: Bool
    let isHosted: Bool

    var textBase: Color {
        isDark
            ? HexTheme.textBase
            : Color(red: 0x14 / 255.0, green: 0x16 / 255.0, blue: 0x1B / 255.0)
    }

    var textMuted: Color {
        isDark
            ? HexTheme.textMuted
            : Color(red: 0x4E / 255.0, green: 0x54 / 255.0, blue: 0x61 / 255.0)
    }

    var pillFill: Color {
        if !isHosted { return HexTheme.layer01 }
        return isDark ? Color.white.opacity(0.09) : Color.black.opacity(0.065)
    }

    var pillActiveFill: Color {
        if !isHosted { return HexTheme.layer02 }
        return isDark ? Color.white.opacity(0.17) : Color.black.opacity(0.13)
    }

    var keyFill: Color {
        if !isHosted { return HexTheme.layer01 }
        return isDark ? Color.white.opacity(0.10) : Color.white.opacity(0.88)
    }

    var keyPressedFill: Color {
        if !isHosted { return HexTheme.layer02 }
        return isDark ? Color.white.opacity(0.19) : Color.white.opacity(0.64)
    }

    var borderWeak: Color {
        if !isHosted { return HexTheme.borderWeak }
        return isDark ? Color.white.opacity(0.08) : Color.black.opacity(0.08)
    }

    var stageFill: Color {
        isDark
            ? Color(red: 0x0C / 255.0, green: 0x0E / 255.0, blue: 0x11 / 255.0)
            : Color(red: 0x12 / 255.0, green: 0x14 / 255.0, blue: 0x18 / 255.0)
    }

    func stageBorder(active: Bool) -> Color {
        if isDark {
            return active ? HexTheme.borderStrong : HexTheme.borderWeak
        }
        return Color.black.opacity(active ? 0.22 : 0.12)
    }
}

private struct RepeatDeleteKey: View {
    let palette: KeyboardChromePalette
    let onDelete: () -> Void
    @State private var isPressing = false
    @State private var repeatTask: Task<Void, Never>?

    var body: some View {
        Image(systemName: "delete.left")
            .font(.system(size: 15, weight: .medium))
            .foregroundStyle(palette.textBase)
            .frame(width: 52, height: 40)
            .background(
                isPressing ? palette.keyPressedFill : palette.keyFill,
                in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    .stroke(palette.borderWeak, lineWidth: 1)
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
    @Environment(\.colorScheme) private var colorScheme
    @ObservedObject var model: KeyboardViewModel
    @State private var idleTriggerWidth: CGFloat = 94
    @State private var doneButtonWidth: CGFloat = 68

    private var isHostedExtension: Bool {
        model.inputViewController != nil
    }

    private var palette: KeyboardChromePalette {
        KeyboardChromePalette(
            isDark: !isHostedExtension || colorScheme == .dark,
            isHosted: isHostedExtension
        )
    }

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
        .padding(.top, isHostedExtension ? 2 : 8)
        .padding(.bottom, isHostedExtension ? 4 : 6)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(isHostedExtension ? Color.clear : HexTheme.bgDeep)
        .preferredColorScheme(isHostedExtension ? nil : .dark)
    }

    // MARK: - Top Bar (26 pt)

    @ViewBuilder
    private var topBar: some View {
        let p = palette
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
                            .foregroundStyle(active ? p.textBase : p.textMuted)
                            .padding(.horizontal, 10)
                            .frame(height: 26)
                            .background(active ? p.pillActiveFill : p.pillFill, in: Capsule())
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
                .hexScrollEdgeEffectHidden()
                closeTrayButton
            }
            .frame(height: 26)

        case .none:
            HStack(spacing: 8) {
                Button {
                    let target = model.hasFullAccess ? model.coldStartURL : KeyboardLaunchRequest.setup.url
                    model.onOpenURL?(target)
                } label: {
                    HexMarkView(
                        size: 16,
                        color: isStageActive && p.isDark ? HexTheme.warmPearl : p.textBase
                    )
                    .frame(width: 22, height: 26, alignment: .leading)
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Open Hex")

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
        let p = palette
        if !model.hasFullAccess {
            HStack(spacing: 6) {
                HexStatusDot(color: HexTheme.warmMic, size: 6)
                Text(KeyboardSetupCopy.fullAccessPath)
                    .font(.system(size: 12, weight: .medium))
                    .foregroundStyle(p.textMuted)
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
                        .foregroundStyle(p.textBase)
                }
            case .transcribing:
                EmptyView()
            case .failed:
                if let message = model.snapshot.message, !message.isEmpty {
                    HStack(spacing: 6) {
                        HexStatusDot(color: HexTheme.danger, size: 6)
                        Text(message)
                            .font(.system(size: 12, weight: .medium))
                            .foregroundStyle(p.textMuted)
                            .lineLimit(1)
                    }
                }
            default:
                if let message = model.snapshot.message, !message.isEmpty {
                    Text(message)
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(p.textMuted)
                        .lineLimit(1)
                } else {
                    HStack(spacing: 5) {
                        HexStatusDot(color: HexTheme.warmMic, size: 6)
                        Text(HexTheme.formatRemaining(model.snapshot.expiresAt - model.now))
                            .font(.system(size: 12, weight: .medium))
                            .monospacedDigit()
                            .foregroundStyle(p.textMuted)
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
                .foregroundStyle(p.textMuted)
                .padding(.horizontal, 9)
                .frame(height: 26)
                .background(p.pillFill, in: Capsule())
                .overlay(Capsule().stroke(p.borderWeak, lineWidth: 1))
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Return app")
        }
    }

    private var stylePill: some View {
        let p = palette
        return Button {
            model.activeTray = .style
        } label: {
            HStack(spacing: 4) {
                Text(model.style.label)
                    .font(.system(size: 12, weight: .medium))
                Image(systemName: "chevron.down")
                    .font(.system(size: 9, weight: .semibold))
                    .foregroundStyle(p.textMuted)
            }
            .foregroundStyle(p.textBase)
            .padding(.horizontal, 10)
            .frame(height: 26)
            .background(p.pillFill, in: Capsule())
            .overlay(Capsule().stroke(p.borderWeak, lineWidth: 1))
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Style: \(model.style.label)")
    }

    private func returnChip(id: String?, label: String) -> some View {
        let p = palette
        let active = model.returnAppID == id
        return Button {
            model.onReturnAppSelected?(id)
            model.activeTray = .none
        } label: {
            Text(label)
                .font(.system(size: 12, weight: active ? .semibold : .medium))
                .foregroundStyle(active ? p.textBase : p.textMuted)
                .padding(.horizontal, 10)
                .frame(height: 26)
                .background(active ? p.pillActiveFill : p.pillFill, in: Capsule())
        }
        .buttonStyle(.plain)
    }

    private var closeTrayButton: some View {
        let p = palette
        return Button {
            model.activeTray = .none
        } label: {
            Image(systemName: "xmark")
                .font(.system(size: 10, weight: .semibold))
                .foregroundStyle(p.textMuted)
                .frame(width: 26, height: 26)
                .background(p.pillFill, in: Circle())
        }
        .buttonStyle(.plain)
    }

    // MARK: - Unified 52 pt Stage Well

    private var primaryStage: some View {
        let p = palette
        let showRecordingControls = (model.snapshot.state == .recording || model.isOptimisticStarting)
            && !model.isOptimisticStopping
        let leadInset: Float = (isStageActive && showRecordingControls) ? 49.0 : 0.0
        let trailInset: Float = if isStageActive {
            showRecordingControls ? Float(doneButtonWidth + 11.0) : 0.0
        } else {
            Float(idleTriggerWidth + 11.0)
        }

        return ZStack {
            Capsule()
                .fill(p.stageFill)
                .overlay(
                    Capsule()
                        .stroke(p.stageBorder(active: isStageActive), lineWidth: 1)
                )

            DotScopeView(
                phase: model.dotScopePhase,
                tuning: .bar(leadingInset: leadInset, trailingInset: trailInset, fade: 16.0, spacing: 3.2),
                drawPlate: false
            )
            .clipShape(Capsule())
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
                        .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { doneButtonWidth = $0 }
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
        .animation(.spring(duration: 0.28, bounce: 0.0), value: isStageActive)
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
            .onGeometryChange(for: CGFloat.self) { $0.size.width } action: { idleTriggerWidth = $0 }
        }
        .padding(.horizontal, 7)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .contentShape(Capsule())
    }

    // MARK: - Bottom Utility Key Row (40 pt)

    private var utilityRow: some View {
        let p = palette
        return HStack(spacing: 6) {
            if model.needsGlobeKey {
                GlobeKeyRepresentable(controller: model.inputViewController, isDark: p.isDark)
                    .frame(width: 44, height: 40)
                    .background(
                        p.keyFill,
                        in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                            .stroke(p.borderWeak, lineWidth: 1)
                    )
            }

            Button {
                model.onSpace?()
            } label: {
                Text("space")
                    .font(.system(size: 14, weight: .regular))
                    .foregroundStyle(p.textMuted)
                    .frame(maxWidth: .infinity)
                    .frame(height: 40)
                    .background(
                        p.keyFill,
                        in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                            .stroke(p.borderWeak, lineWidth: 1)
                    )
                    .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }
            .buttonStyle(.plain)

            RepeatDeleteKey(palette: p) {
                model.onDelete?()
            }

            Button {
                model.onReturn?()
            } label: {
                Image(systemName: "return")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(p.textBase)
                    .frame(width: 56, height: 40)
                    .background(
                        p.keyFill,
                        in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    )
                    .overlay(
                        RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                            .stroke(p.borderWeak, lineWidth: 1)
                    )
                    .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }
            .buttonStyle(.plain)
            .accessibilityLabel(model.returnKeyLabel)
        }
    }
}
