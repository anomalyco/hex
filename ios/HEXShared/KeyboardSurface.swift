import Combine
import SwiftUI
import UIKit

@MainActor
final class KeyboardClockModel: ObservableObject {
    @Published private(set) var recordingText: String = "0:00"
    @Published private(set) var warmRemainingText: String = "15m"

    func update(recordingElapsed: TimeInterval?, warmRemaining: TimeInterval?) {
        if let recordingElapsed {
            let formatted = HexTheme.formatClock(recordingElapsed)
            if formatted != recordingText {
                recordingText = formatted
            }
        }
        if let warmRemaining, warmRemaining > 0 {
            let formatted = HexTheme.formatRemaining(warmRemaining)
            if formatted != warmRemainingText {
                warmRemainingText = formatted
            }
        }
    }
}

@MainActor
final class KeyboardViewModel: ObservableObject {
    enum PickerTray: Equatable {
        case none
        case style
        case language
    }

    enum StagePhase: Equatable {
        case idle
        case recording
        case transcribing
        case failed
    }

    struct PendingCommand: Equatable {
        let kind: KeyboardCommandKind
        let jobID: String
        let sentAt: TimeInterval
    }

    static let commandAckTimeout: TimeInterval = 1.6

    let clock = KeyboardClockModel()

    @Published var hasFullAccess = true
    @Published var needsGlobeKey = true
    @Published var returnKeyLabel = "Return"
    @Published var isModelInstalled = false
    @Published private(set) var isWarmAvailable = false
    @Published private(set) var stagePhase: StagePhase = .idle
    @Published private(set) var statusMessage: String?
    @Published var style: DictationStyle = .polished
    @Published var activeTray: PickerTray = .none
    /// Settings › Dictation language (nil for Automatic), what Automatic resolves to, and the
    /// languages the keyboard's quick switch offers. Empty choices hide the switch.
    @Published var dictationLanguage: String?
    @Published var automaticLanguage: String?
    @Published var languageChoices: [String] = []
    /// Translate what I say's target (BCP 47) while it is on, else nil.
    @Published var translationTarget: String?
    /// The last dictation went in as spoken while Translate what I say was on: both keyboards
    /// say so for a few seconds, with the fix, which opens Hex's Settings.
    @Published var translationNote: TranslationFailure?
    /// Translate what I say's target whether it is on or off, for the keyboards' Translate to
    /// <language> switch; nil hides the switch (no Apple translation before iOS 26).
    @Published var translationLanguage: String?
    /// The targets the full keyboard's Hex key menu offers (`DictationTranslationSettings.targetChoices`).
    @Published var translationTargetChoices: [String] = []
    /// Record typing logs is recording this field: both keyboards show a small mark the
    /// whole time, so logging is never hidden.
    @Published var isLoggingTyping = false
    /// The dictation bar's emoji key: Settings › Keyboard › Emoji, in a field that takes emoji.
    @Published var showsEmojiKey = true
    /// A finished dictation the keyboard didn't insert by itself (it missed its moment, or
    /// the keyboard came back in another field): the full keyboard's strip offers Paste
    /// dictation, which calls `onPasteDictation`.
    @Published var offeredDictation: String?

    // Non-published live telemetry read directly by `DotScopeHostView` / button tap handlers
    // so 25–30 Hz polling never invalidates SwiftUI views mid-spring.
    var liveLoudnessDB: Float = -62.0
    var currentTimestamp: TimeInterval = Date().timeIntervalSince1970
    private(set) var currentSnapshot: KeyboardSnapshot = .offline
    var coldStartURL: URL = KeyboardLaunchRequest.startSession(requestedAt: nil).url
    private(set) var pendingCommand: PendingCommand?

    var onDictateTap: (() -> Void)?
    var onLocalDictateTap: (() -> Void)?
    /// The finger landed on the full keyboard's Hex key (wall clock), before it is a tap,
    /// a hold or a slide.
    var onHexKeyPress: ((TimeInterval) -> Void)?
    /// That press became a slide (the style menu) or was cancelled before it was a tap or a hold.
    var onHexKeyAbandon: (() -> Void)?
    var onStopTap: (() -> Void)?
    var onCancelTap: (() -> Void)?
    var onStyleSelected: ((DictationStyle) -> Void)?
    /// A language picked in the keyboard's switch; nil is Automatic.
    var onLanguageSelected: ((String?) -> Void)?
    /// Translate to <language> switched on (true) or off where you dictate.
    var onTranslationToggled: ((Bool) -> Void)?
    /// A target picked in the Hex key menu; it also turns translation on.
    var onTranslationTargetSelected: ((String) -> Void)?
    var onSpace: (() -> Void)?
    /// A punctuation key on the dictation bar.
    var onInsertText: ((String) -> Void)?
    /// The dictation bar's emoji key: the full keyboard's emoji panel opens in its place.
    var onEmojiTap: (() -> Void)?
    var onDelete: (() -> Void)?
    var onReturn: (() -> Void)?
    var onOpenURL: ((URL) -> Void)?
    var onPasteDictation: (() -> Void)?
    /// The dictation bar's light haptic for each press; the keyboard allows it with Full
    /// Access and Experiments › Haptics, as the full keyboard's keys.
    var onKeyFeedback: (() -> Void)?
    weak var inputViewController: UIInputViewController?

    /// The language dictation uses now, when known.
    var effectiveLanguage: String? {
        dictationLanguage ?? automaticLanguage
    }

    /// Its own short name ("Norsk", "English"), for the switch and the space bar.
    var languageLabel: String? {
        effectiveLanguage.map(DictationLanguageSwitch.nativeName(for:))
    }

    var showsLanguageSwitch: Bool {
        hasFullAccess && !languageChoices.isEmpty
    }

    /// "Paste in English" or "Paste in Spanish", when Translate what I say is on and you
    /// dictate in another language than its target.
    var pasteChoice: TranslationPasteChoice? {
        guard hasFullAccess, let translationTarget else { return nil }
        return TranslationPasteChoice(target: translationTarget, source: effectiveLanguage)
    }

    /// The keyboards' Translate to <language> switch can show: Full Access and a target.
    var showsTranslationSwitch: Bool {
        hasFullAccess && translationLanguage != nil
    }

    /// Dictations paste translated: Translate what I say is on. The Hex key wears the target's
    /// badge ("EN") the whole time.
    var isTranslating: Bool {
        hasFullAccess && translationTarget != nil
    }

    /// The target while `isTranslating`, for the badge beside the Hex key ("EN").
    var translatingTo: String? {
        isTranslating ? translationTarget : nil
    }

    /// The language label with where dictations go while they paste translated:
    /// "Español → English".
    var languageSummary: String? {
        guard let languageLabel else { return nil }
        guard let target = pasteChoice?.target else { return languageLabel }
        return "\(languageLabel) → \(DictationLanguageSwitch.nativeName(for: target))"
    }

    var dotScopePhase: DotScopePhase {
        switch stagePhase {
        case .idle:
            return .idle
        case .recording:
            return .recording
        case .transcribing:
            return .processing
        case .failed:
            return .failed
        }
    }

    func setActiveTray(_ tray: PickerTray, reduceMotion: Bool) {
        guard activeTray != tray else { return }
        withAnimation(HexMotion.trayAnimation(reduceMotion: reduceMotion)) {
            activeTray = tray
        }
    }

    func setPendingCommand(_ command: PendingCommand?, now: TimeInterval, animated: Bool) {
        currentTimestamp = now
        pendingCommand = command
        reconcileVisualState(
            hasFullAccess: hasFullAccess,
            snapshot: currentSnapshot,
            isModelInstalled: isModelInstalled,
            style: style,
            now: now,
            animated: animated
        )
    }

    func applyBridgeState(
        hasFullAccess newFullAccess: Bool,
        snapshot newSnapshot: KeyboardSnapshot,
        isModelInstalled newModelInstalled: Bool,
        style newStyle: DictationStyle,
        now: TimeInterval,
        clearPending: Bool = false,
        animated: Bool
    ) {
        // A job Hex stopped heartbeating for is over (Hex ended, crashed or was suspended
        // mid-dictation): without this its last snapshot held every keyboard in recording or
        // transcribing for good, with Done and Cancel sent to no one.
        let newSnapshot = newSnapshot.droppingOrphanedJob(at: now)
        currentTimestamp = now
        currentSnapshot = newSnapshot
        liveLoudnessDB = newSnapshot.state == .recording ? newSnapshot.effectiveLoudnessDB : -62.0

        if clearPending {
            pendingCommand = nil
        } else if let pending = pendingCommand {
            let expired = now - pending.sentAt >= Self.commandAckTimeout
            let acknowledged = (pending.kind == .start && newSnapshot.state == .recording)
                || (pending.kind == .stop && newSnapshot.state != .recording)
                || (pending.kind == .cancel && newSnapshot.state != .recording)
            if expired || acknowledged {
                pendingCommand = nil
            }
        }

        let recordingElapsed: TimeInterval? = if newSnapshot.state == .recording {
            newSnapshot.recordingElapsed(at: now)
        } else if let pending = pendingCommand, pending.kind == .start {
            max(0, now - pending.sentAt)
        } else {
            nil
        }
        let warmRemaining: TimeInterval? = newSnapshot.isAvailable(at: now)
            ? max(0, newSnapshot.expiresAt - now)
            : nil
        clock.update(recordingElapsed: recordingElapsed, warmRemaining: warmRemaining)

        reconcileVisualState(
            hasFullAccess: newFullAccess,
            snapshot: newSnapshot,
            isModelInstalled: newModelInstalled,
            style: newStyle,
            now: now,
            animated: animated
        )
    }

    /// Convenience for deterministic preview/snapshot fixtures (`KeyboardPreviewHost`).
    func configureFixture(
        snapshot: KeyboardSnapshot,
        now: TimeInterval,
        hasFullAccess: Bool = true,
        isModelInstalled: Bool = true,
        style: DictationStyle = .polished,
        activeTray: PickerTray = .none
    ) {
        self.activeTray = activeTray
        applyBridgeState(
            hasFullAccess: hasFullAccess,
            snapshot: snapshot,
            isModelInstalled: isModelInstalled,
            style: style,
            now: now,
            clearPending: true,
            animated: false
        )
    }

    private func reconcileVisualState(
        hasFullAccess newFullAccess: Bool,
        snapshot snap: KeyboardSnapshot,
        isModelInstalled newModelInstalled: Bool,
        style newStyle: DictationStyle,
        now: TimeInterval,
        animated: Bool
    ) {
        let nextWarm = newFullAccess && snap.isAvailable(at: now)
        let nextStage = computeStagePhase(snapshot: snap, now: now, hasFullAccess: newFullAccess)
        let trimmedMsg = snap.message?.trimmingCharacters(in: .whitespacesAndNewlines)
        let nextMessage = (trimmedMsg?.isEmpty == false) ? trimmedMsg : nil

        let changed = hasFullAccess != newFullAccess
            || isWarmAvailable != nextWarm
            || isModelInstalled != newModelInstalled
            || stagePhase != nextStage
            || statusMessage != nextMessage
            || style != newStyle

        guard changed else { return }

        let commit = {
            self.hasFullAccess = newFullAccess
            self.isWarmAvailable = nextWarm
            self.isModelInstalled = newModelInstalled
            self.stagePhase = nextStage
            self.statusMessage = nextMessage
            self.style = newStyle
        }

        if animated {
            withAnimation(HexMotion.stageAnimation(reduceMotion: UIAccessibility.isReduceMotionEnabled)) {
                commit()
            }
        } else {
            commit()
        }
    }

    private func computeStagePhase(
        snapshot snap: KeyboardSnapshot,
        now: TimeInterval,
        hasFullAccess: Bool
    ) -> StagePhase {
        guard hasFullAccess else { return .idle }

        if let pending = pendingCommand, now - pending.sentAt < Self.commandAckTimeout {
            switch pending.kind {
            case .cancel:
                return .idle
            case .stop where snap.state == .recording:
                return .transcribing
            case .start where snap.state != .recording:
                return .recording
            default:
                break
            }
        }

        switch snap.state {
        case .recording:
            return .recording
        case .transcribing:
            return .transcribing
        case .failed:
            return .failed
        case .offline, .ready:
            return .idle
        }
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
            ? UIColor(white: 0.92, alpha: 1)
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
            ? UIColor(white: 0.92, alpha: 1)
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

    var chipIdleFill: Color {
        isDark ? Color.white.opacity(0.07) : Color.black.opacity(0.055)
    }

    var chipActiveFill: Color {
        isDark ? Color.white.opacity(0.16) : Color.black.opacity(0.12)
    }

    var topBarPressedFill: Color {
        isDark ? Color.white.opacity(0.09) : Color.black.opacity(0.07)
    }

    var controlSubtleFill: Color {
        isDark ? Color.white.opacity(0.085) : Color.black.opacity(0.065)
    }

    var controlProminentFill: Color {
        isDark ? Color.white.opacity(0.15) : Color.black.opacity(0.11)
    }

    var idleTriggerFill: Color {
        isDark ? Color.white.opacity(0.09) : Color.black.opacity(0.07)
    }

    var keyFill: Color {
        if !isHosted { return Color.white.opacity(0.085) }
        return isDark ? Color.white.opacity(0.105) : Color.white
    }

    var keyPressedFill: Color {
        if !isHosted { return Color.white.opacity(0.16) }
        return isDark ? Color.white.opacity(0.19) : Color(white: 0.88)
    }

    var keyShadow: Color {
        Color.black.opacity(isDark ? 0.24 : 0.14)
    }
}

private struct KeyboardKeyButtonStyle: ButtonStyle {
    let palette: KeyboardChromePalette

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(
                configuration.isPressed ? palette.keyPressedFill : palette.keyFill,
                in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
            )
            .shadow(color: palette.keyShadow, radius: 0.5, x: 0, y: 1)
    }
}

private struct TopBarTriggerButtonStyle: ButtonStyle {
    let palette: KeyboardChromePalette

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(
                configuration.isPressed ? palette.topBarPressedFill : Color.clear,
                in: Capsule()
            )
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
            .frame(width: 44, height: 42)
            .background(
                isPressing ? palette.keyPressedFill : palette.keyFill,
                in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
            )
            .shadow(color: palette.keyShadow, radius: 0.5, x: 0, y: 1)
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

private struct KeyboardRecordingClockText: View {
    @ObservedObject var clock: KeyboardClockModel
    let color: Color

    var body: some View {
        Text(clock.recordingText)
            .font(.system(size: 13, weight: .medium))
            .monospacedDigit()
            .contentTransition(.numericText())
            .foregroundStyle(color)
            .animation(.easeOut(duration: 0.16), value: clock.recordingText)
    }
}

private struct KeyboardWarmRemainingText: View {
    @ObservedObject var clock: KeyboardClockModel
    let color: Color

    var body: some View {
        Text(clock.warmRemainingText)
            .font(.system(size: 13, weight: .medium))
            .monospacedDigit()
            .contentTransition(.numericText())
            .foregroundStyle(color)
    }
}

extension View {
    /// iOS 26's soft scroll-edge effect blurs the whole content of a short
    /// scroll view (the 26 pt return-app tray), so turn it off there.
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

/// Record typing logs is recording: a dot and "Logging", flat and small, in the dictation
/// bar's top row (the full keyboard draws `TypingLogMarkView`).
struct TypingLogMark: View {
    let textColor: Color

    var body: some View {
        HStack(spacing: 4) {
            // Muted, not orange: an orange dot would read as iOS's microphone indicator.
            Circle()
                .fill(textColor)
                .frame(width: 5, height: 5)
            Text(TypingLogMarkView.text)
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(textColor)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(TypingLogMarkView.accessibilityText)
    }
}

/// Beside the dictation bar's Hex mark while dictations paste translated: the target's code
/// ("EN"), so a dictation never comes out in another language by surprise.
private struct TranslationBadge: View {
    let target: String
    let palette: KeyboardChromePalette

    var body: some View {
        HStack(spacing: 2) {
            Image(systemName: "arrow.right")
                .font(.system(size: 8, weight: .bold))
            Text(TranslationPasteChoice.badge(target: target))
                .font(.system(size: 10, weight: .semibold))
        }
        .foregroundStyle(palette.textBase)
        .padding(.horizontal, 5)
        .frame(height: 16)
        .background(palette.chipActiveFill, in: Capsule())
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(TranslationPasteChoice.translatingLabel(target: target))
    }
}

enum KeyboardSetupCopy {
    static let fullAccessPath = "Allow Full Access: Settings › Apps › Hex › Keyboards"
}

/// Metal-free stand-in for the resting Dot Scope bar. Mounting it instead of
/// `DotScopeView` keeps the keyboard extension's launch path off the GPU: no
/// device, command queue, render pipelines, `CAMetalLayer`, display link, or
/// idle simulation steps before the first frame.
private struct KeyboardRestingDotBar: View {
    let isDark: Bool
    let leadingInset: CGFloat
    let trailingInset: CGFloat

    private static let pitch: CGFloat = 3.5
    private static let rows = 11
    private static let fade: CGFloat = 26
    private static let braidCycles: Double = 2.1
    private static let restingAmplitude: Double = 0.9

    var body: some View {
        let lead = Double(max(0, leadingInset))
        let trail = Double(max(0, trailingInset))
        let tint: (red: Double, green: Double, blue: Double) = isDark
            ? (red: 0.93, green: 0.91, blue: 0.86)
            : (red: 0.55, green: 0.46, blue: 0.33)
        return Canvas { context, size in
            let pitch = Double(Self.pitch)
            let firstX = lead + 3
            let lastX = Double(size.width) - trail - 3
            guard lastX - firstX > pitch * 2, size.height > 8 else { return }

            let columns = max(3, Int(((lastX - firstX) / pitch).rounded(.down)) + 1)
            let midY = Double(size.height) * 0.5
            let halfRows = Double(Self.rows - 1) * 0.5

            for column in 0..<columns {
                let x = firstX + Double(column) * pitch
                let xn = Double(column) / Double(max(1, columns - 1))
                let fadeIn = min(1.0, max(0.0, (x - lead) / Double(Self.fade)))
                let fadeOut = min(1.0, max(0.0, (Double(size.width) - trail - x) / Double(Self.fade)))
                let edge = min(fadeIn, fadeOut)
                guard edge > 0.004 else { continue }

                // The live scope flattens to a faint mirrored braid at rest.
                let wobble = sin(2.0 * .pi * Self.braidCycles * xn - 0.6) * Self.restingAmplitude
                let upper = midY - wobble
                let lower = midY + wobble

                for row in 0..<Self.rows {
                    let y = midY + (Double(row) - halfRows) * pitch
                    let litUpper = exp(-pow(y - upper, 2) / 2.2)
                    let litLower = exp(-pow(y - lower, 2) / 2.2)
                    let power = min(1.0, 1.0 - (1.0 - litUpper) * (1.0 - litLower))
                    let alpha = (0.05 + 0.40 * power) * edge
                    guard alpha > 0.01 else { continue }
                    let radius = (0.16 + 0.21 * power) * pitch
                    context.fill(
                        Path(
                            ellipseIn: CGRect(
                                x: x - radius,
                                y: y - radius,
                                width: radius * 2,
                                height: radius * 2
                            )
                        ),
                        with: .color(Color(red: tint.red, green: tint.green, blue: tint.blue, opacity: alpha))
                    )
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityHidden(true)
    }
}

struct KeyboardSurfaceView: View {
    @Environment(\.colorScheme) private var colorScheme
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @ObservedObject var model: KeyboardViewModel
    /// The keyboard extension sets this so its process never builds the Metal
    /// scope before a stage needs it. The app's docked keyboard preview keeps
    /// the live scope mounted at rest.
    var defersDotScopeUntilActive = false

    /// Push to talk on the well: the press, on the Hex key's state machine.
    @State private var hold = HexKeyGesture()
    @State private var holdTimer: Task<Void, Never>?
    @State private var pressBegan = false
    /// This press's hold started the dictation, so the Dictate button's tap action skips it.
    @State private var holdTookPress = false
    @GestureState private var isPressingWell = false

    private static let wellHeight: CGFloat = 44
    private static let controlHeight: CGFloat = 34
    private static let wellInset: CGFloat = 5
    private static let cancelWidth: CGFloat = 34
    private static let doneWidth: CGFloat = 66

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
        model.stagePhase == .recording || model.stagePhase == .transcribing
    }

    var body: some View {
        VStack(spacing: 8) {
            topBar
            primaryStage
            utilityRow
        }
        .padding(.horizontal, 10)
        .padding(.top, isHostedExtension ? 4 : 6)
        .padding(.bottom, 6)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(isHostedExtension ? Color.clear : HexTheme.bgDeep)
        .preferredColorScheme(isHostedExtension ? nil : .dark)
    }

    // MARK: - Top Bar (26 pt, permanently mounted ZStack)

    private var topBar: some View {
        let p = palette
        return ZStack {
            defaultTopBar(palette: p)
                .hexTrayReveal(model.activeTray == .none, reduceMotion: reduceMotion, offsetY: -3)

            styleTrayBar(palette: p)
                .hexTrayReveal(model.activeTray == .style, reduceMotion: reduceMotion, offsetY: 3.5)

            languageTrayBar(palette: p)
                .hexTrayReveal(model.activeTray == .language, reduceMotion: reduceMotion, offsetY: 3.5)
        }
        .frame(height: 26)
    }

    private func defaultTopBar(palette p: KeyboardChromePalette) -> some View {
        HStack(spacing: 8) {
            Button {
                model.onKeyFeedback?()
                if model.hasFullAccess, !model.isWarmAvailable, let local = model.onLocalDictateTap {
                    local()
                } else {
                    let target = model.hasFullAccess ? model.coldStartURL : KeyboardLaunchRequest.setup.url
                    model.onOpenURL?(target)
                }
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

            if let target = model.translatingTo {
                TranslationBadge(target: target, palette: p)
            }

            if model.isLoggingTyping {
                TypingLogMark(textColor: p.textMuted)
            }

            leadingAccessory(palette: p)

            Spacer(minLength: 4)

            // The translation note takes the bar's width for its few seconds.
            if model.showsLanguageSwitch || model.showsTranslationSwitch, model.translationNote == nil {
                languageTrigger(palette: p)
            }

            styleTrigger(palette: p)
                .hexTrayReveal(model.hasFullAccess && model.translationNote == nil, reduceMotion: reduceMotion, offsetY: 0)
        }
        .frame(height: 26)
    }

    private func styleTrayBar(palette p: KeyboardChromePalette) -> some View {
        HStack(spacing: 4) {
            ForEach(DictationStyle.allCases, id: \.self) { candidate in
                let active = candidate == model.style
                Button {
                    model.onKeyFeedback?()
                    model.onStyleSelected?(candidate)
                    model.setActiveTray(.none, reduceMotion: reduceMotion)
                } label: {
                    Text(candidate.label)
                        .font(.system(size: 13, weight: .medium))
                        .foregroundStyle(active ? p.textBase : p.textMuted)
                        .padding(.horizontal, 11)
                        .frame(height: 26)
                        .background(active ? p.chipActiveFill : p.chipIdleFill, in: Capsule())
                }
                .buttonStyle(.plain)
            }
            Spacer(minLength: 4)
            closeTrayButton(palette: p)
        }
        .frame(height: 26)
    }

    private func leadingAccessory(palette p: KeyboardChromePalette) -> some View {
        let showNoAccess = !model.hasFullAccess
        let showRecordingClock = model.hasFullAccess && model.stagePhase == .recording
        let showTranslationNote = model.hasFullAccess
            && model.translationNote != nil
            && model.stagePhase != .recording
            && model.stagePhase != .transcribing
        let showWarmStatus = model.hasFullAccess
            && model.isWarmAvailable
            && model.stagePhase != .recording
            && model.stagePhase != .transcribing
            && !showTranslationNote

        return ZStack(alignment: .leading) {
            HStack(spacing: 6) {
                HexStatusDot(color: HexTheme.warmMic, size: 6)
                Text(KeyboardSetupCopy.fullAccessPath)
                    .font(.system(size: 13, weight: .regular))
                    .foregroundStyle(p.textMuted)
                    .lineLimit(1)
                    .minimumScaleFactor(0.78)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel("Full Access is off. \(KeyboardSetupCopy.fullAccessPath)")
            .hexTrayReveal(showNoAccess, reduceMotion: reduceMotion, offsetY: 2)

            HStack(spacing: 6) {
                HexStatusDot(color: HexTheme.danger, size: 6.5)
                KeyboardRecordingClockText(clock: model.clock, color: p.textBase)
            }
            .hexTrayReveal(showRecordingClock, reduceMotion: reduceMotion, offsetY: 2)

            Group {
                if let message = model.statusMessage {
                    HStack(spacing: 6) {
                        if model.stagePhase == .failed {
                            HexStatusDot(color: HexTheme.danger, size: 6)
                        }
                        // A failure gets two smaller lines: it has to be read in full.
                        let failed = model.stagePhase == .failed
                        Text(message)
                            .font(.system(size: failed ? 11 : 13, weight: .regular))
                            .foregroundStyle(p.textMuted)
                            .lineLimit(failed ? 2 : 1)
                            .minimumScaleFactor(0.85)
                    }
                } else {
                    HStack(spacing: 5) {
                        HexStatusDot(color: HexTheme.warmMic, size: 6)
                        KeyboardWarmRemainingText(clock: model.clock, color: p.textMuted)
                    }
                }
            }
            .hexTrayReveal(showWarmStatus, reduceMotion: reduceMotion, offsetY: 2)

            Group {
                if let note = model.translationNote {
                    translationNote(note, palette: p)
                }
            }
            .hexTrayReveal(showTranslationNote, reduceMotion: reduceMotion, offsetY: 2)
        }
    }

    /// "Couldn't translate, pasted as spoken" over its fix; a tap opens Hex's Settings › Dictation.
    private func translationNote(_ note: TranslationFailure, palette p: KeyboardChromePalette) -> some View {
        Button {
            model.onOpenURL?(KeyboardLaunchRequest.translationSettings.url)
        } label: {
            VStack(alignment: .leading, spacing: 0) {
                Text(TranslationFailure.note)
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(p.textBase)
                Text(note.fix)
                    .font(.system(size: 11))
                    .foregroundStyle(p.textMuted)
            }
            .lineLimit(1)
            .minimumScaleFactor(0.8)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityElement(children: .combine)
        .accessibilityHint("Opens Hex's Settings")
    }

    private func styleTrigger(palette p: KeyboardChromePalette) -> some View {
        Button {
            model.onKeyFeedback?()
            model.setActiveTray(.style, reduceMotion: reduceMotion)
        } label: {
            HStack(spacing: 4) {
                Text(model.style.label)
                    .font(.system(size: 13, weight: .medium))
                Image(systemName: "chevron.down")
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(p.textMuted)
            }
            .foregroundStyle(p.textMuted)
            .padding(.horizontal, 8)
            .frame(height: 26)
            .contentShape(Capsule())
        }
        .buttonStyle(TopBarTriggerButtonStyle(palette: p))
        .accessibilityLabel("Style: \(model.style.label)")
    }

    private func languageTrigger(palette p: KeyboardChromePalette) -> some View {
        // While translating, the badge beside the Hex mark says where dictations go ("→ EN").
        let summary = model.translatingTo == nil ? model.languageSummary : model.languageLabel
        let label = summary ?? "Automatic"
        return Button {
            model.onKeyFeedback?()
            model.setActiveTray(.language, reduceMotion: reduceMotion)
        } label: {
            HStack(spacing: 4) {
                Text(label)
                    .font(.system(size: 13, weight: .medium))
                    .lineLimit(1)
                Image(systemName: "chevron.down")
                    .font(.system(size: 10, weight: .medium))
                    .foregroundStyle(p.textMuted)
            }
            .foregroundStyle(p.textMuted)
            .padding(.horizontal, 8)
            .frame(height: 26)
            .contentShape(Capsule())
        }
        .buttonStyle(TopBarTriggerButtonStyle(palette: p))
        // "Español → English" before the status beside the Hex mark, which is short whenever
        // the switch shows (it needs Full Access).
        .layoutPriority(1)
        .accessibilityLabel("Dictation language: \(label)")
    }

    private func languageTrayBar(palette p: KeyboardChromePalette) -> some View {
        let names = DictationLanguageSwitch.nativeNames(for: model.languageChoices)
        return HStack(spacing: 4) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 4) {
                    if model.showsLanguageSwitch {
                        languageChip(id: nil, label: "Auto", palette: p)
                        ForEach(model.languageChoices, id: \.self) { id in
                            languageChip(id: id, label: names[id] ?? id, palette: p)
                        }
                    }
                }
                .padding(.leading, 2)
                .padding(.trailing, 10)
            }
            .hexScrollEdgeEffectHidden()
            .hexHorizontalEdgeFade(leading: 4, trailing: 14)

            if model.showsTranslationSwitch, let target = model.translationLanguage {
                translationChip(target: target, palette: p)
            }
            closeTrayButton(palette: p)
        }
        .frame(height: 26)
    }

    /// Translate to <language>: lit while dictations paste translated; a tap turns
    /// Translate what I say on or off, the same App Group setting as Settings' switch.
    private func translationChip(target: String, palette p: KeyboardChromePalette) -> some View {
        let active = model.isTranslating
        let title = TranslationPasteChoice.toggleTitle(target: target)
        return Button {
            model.onKeyFeedback?()
            model.onTranslationToggled?(!active)
            model.setActiveTray(.none, reduceMotion: reduceMotion)
        } label: {
            Text(title)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(active ? p.textBase : p.textMuted)
                .lineLimit(1)
                .padding(.horizontal, 10)
                .frame(height: 26)
                .background(active ? p.chipActiveFill : p.chipIdleFill, in: Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(title)
        .accessibilityValue(active ? "On" : "Off")
        .accessibilityAddTraits(active ? .isSelected : [])
    }

    private func languageChip(id: String?, label: String, palette p: KeyboardChromePalette) -> some View {
        let active = model.dictationLanguage == id
        return Button {
            model.onKeyFeedback?()
            model.onLanguageSelected?(id)
            model.setActiveTray(.none, reduceMotion: reduceMotion)
        } label: {
            Text(label)
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(active ? p.textBase : p.textMuted)
                .padding(.horizontal, 10)
                .frame(height: 26)
                .background(active ? p.chipActiveFill : p.chipIdleFill, in: Capsule())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(id == nil ? "Automatic language" : label)
    }

    private func closeTrayButton(palette p: KeyboardChromePalette) -> some View {
        Button {
            model.onKeyFeedback?()
            model.setActiveTray(.none, reduceMotion: reduceMotion)
        } label: {
            Image(systemName: "xmark")
                .font(.system(size: 11, weight: .medium))
                .foregroundStyle(p.textMuted)
                .frame(width: 26, height: 26)
                .background(p.chipIdleFill, in: Circle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Close")
    }

    // MARK: - Unified 44 pt Recessed Stage Well (all controls permanently mounted)

    private var idleTriggerSpec: (icon: String, title: String, width: CGFloat) {
        if !model.hasFullAccess {
            return ("arrow.up.forward.app.fill", "Set up Hex", 112)
        }
        if !model.isModelInstalled {
            return ("mic.fill", "Set up", 88)
        }
        return ("mic.fill", "Dictate", 94)
    }

    private var primaryStage: some View {
        let p = palette
        let showRecordingControls = model.stagePhase == .recording
        let showIdleTrigger = model.stagePhase == .idle || model.stagePhase == .failed
        let spec = idleTriggerSpec

        let leadInset: Float = showRecordingControls
            ? Float(Self.wellInset + Self.cancelWidth + 6)
            : 0.0
        let trailInset: Float = if showRecordingControls {
            Float(Self.wellInset + Self.doneWidth + 6)
        } else if showIdleTrigger {
            Float(Self.wellInset + spec.width + 6)
        } else {
            0.0
        }

        return ZStack {
            HexRecessedWell(isDark: p.isDark)

            if defersDotScopeUntilActive && !isStageActive {
                KeyboardRestingDotBar(
                    isDark: p.isDark,
                    leadingInset: CGFloat(leadInset),
                    trailingInset: CGFloat(trailInset)
                )
                .clipShape(Capsule())
                .allowsHitTesting(false)
            } else {
                DotScopeView(
                    phase: model.dotScopePhase,
                    tuning: .bar(leadingInset: leadInset, trailingInset: trailInset, fade: 28.0, spacing: 3.5),
                    drawPlate: false,
                    loudnessProvider: { [weak model] in model?.liveLoudnessDB ?? -62.0 }
                )
                .clipShape(Capsule())
                .allowsHitTesting(false)
            }

            // Recording peer controls (`✕` and `Done`), always mounted so enter/exit springs stay continuous.
            HStack(spacing: 8) {
                Button {
                    model.onKeyFeedback?()
                    model.onCancelTap?()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(p.textMuted)
                        .frame(width: Self.cancelWidth, height: Self.controlHeight)
                        .background(p.controlSubtleFill, in: Circle())
                        .frame(minWidth: 44, minHeight: Self.wellHeight, alignment: .leading)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Cancel dictation")
                .hexControlReveal(
                    showRecordingControls,
                    reduceMotion: reduceMotion,
                    scale: 0.90,
                    blur: 2.5,
                    anchor: .leading
                )

                Spacer(minLength: 0)

                Button {
                    model.onKeyFeedback?()
                    model.onStopTap?()
                } label: {
                    Text("Done")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(p.textBase)
                        .frame(width: Self.doneWidth, height: Self.controlHeight)
                        .background(p.controlProminentFill, in: Capsule())
                        .frame(minHeight: Self.wellHeight, alignment: .trailing)
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
            .padding(.horizontal, Self.wellInset)

            // Single permanently mounted idle trigger (never swapped across 3 Button branches).
            Button {
                guard !holdTookPress else { return }
                model.onKeyFeedback?()
                handleIdleStageTap()
            } label: {
                HStack(spacing: 0) {
                    Spacer(minLength: 0)
                    HStack(spacing: 6) {
                        Image(systemName: spec.icon)
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(p.textMuted)
                        Text(spec.title)
                            .font(.system(size: 15, weight: .medium))
                            .foregroundStyle(p.textBase)
                    }
                    .frame(width: spec.width, height: Self.controlHeight)
                    .background(p.idleTriggerFill, in: Capsule())
                }
                .padding(.horizontal, Self.wellInset)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Capsule())
            }
            .buttonStyle(.plain)
            .accessibilityLabel(spec.title)
            .accessibilityHint(canHoldToTalk ? "Hold to talk, release to finish" : "")
            .hexControlReveal(
                showIdleTrigger,
                reduceMotion: reduceMotion,
                scale: 0.92,
                blur: 2.5,
                anchor: .trailing
            )
        }
        .frame(height: Self.wellHeight)
        .clipShape(Capsule())
        // On the well, not the Dictate button: the button stops taking touches once recording shows.
        .simultaneousGesture(pushToTalk)
        .onChange(of: isPressingWell) { _, pressing in
            guard !pressing else { return }
            // A cancelled press never reaches onEnded; an ended one already reset.
            DispatchQueue.main.async {
                guard pressBegan else { return }
                holdTimer?.cancel()
                hold.cancel()
                pressBegan = false
            }
        }
    }

    /// The well dictates in the keyboard (a warm session or in-keyboard dictation), so a
    /// hold there can be push to talk; opening Hex or setup stays a tap.
    private var canHoldToTalk: Bool {
        guard model.hasFullAccess, model.stagePhase == .idle || model.stagePhase == .failed else { return false }
        return (model.isWarmAvailable && model.stagePhase != .failed) || model.onLocalDictateTap != nil
    }

    /// Hold still on Dictate for `HexKeyGesture.holdDuration`: dictation starts with a light
    /// tap, and lifting the finger finishes it as Done does. A quick tap keeps tap to start.
    private var pushToTalk: some Gesture {
        DragGesture(minimumDistance: 0)
            .updating($isPressingWell) { _, pressing, _ in pressing = true }
            .onChanged { value in
                guard pressBegan else {
                    pressBegan = true
                    holdTookPress = false
                    guard canHoldToTalk else { return }
                    hold.touchDown(at: value.startLocation)
                    holdTimer?.cancel()
                    holdTimer = Task { @MainActor in
                        try? await Task.sleep(for: .seconds(HexKeyGesture.holdDuration))
                        guard !Task.isCancelled, hold.holdElapsed() == .hold else { return }
                        holdTookPress = true
                        model.onKeyFeedback?()
                        handleIdleStageTap()
                    }
                    return
                }
                if hold.touchMoved(to: value.location) == .openMenu {
                    // Moved away: not a hold.
                    holdTimer?.cancel()
                }
            }
            .onEnded { _ in
                holdTimer?.cancel()
                holdTimer = nil
                pressBegan = false
                if hold.touchUp() == .release {
                    model.onStopTap?()
                    // After the button's own touch-up action, which this press must not repeat.
                    DispatchQueue.main.async { holdTookPress = false }
                }
            }
    }

    private func handleIdleStageTap() {
        if !model.hasFullAccess {
            model.onOpenURL?(KeyboardLaunchRequest.setup.url)
        } else if !model.isWarmAvailable || model.stagePhase == .failed {
            if let local = model.onLocalDictateTap {
                local()
            } else {
                model.onOpenURL?(model.coldStartURL)
            }
        } else {
            model.onDictateTap?()
        }
    }

    // MARK: - Bottom Utility Key Row (42 pt, shadow-grounded keycaps with zero strokes)

    /// The bar's punctuation keys, the commonest first: as many as leave the space bar
    /// `minSpaceWidth` (five on a 402 pt iPhone, three on the SE beside its globe).
    private static let punctuation = [",", ".", "?", "!", "'", "\"", "@", "#"]
    private static let punctuationKeyWidth: CGFloat = 30
    private static let minSpaceWidth: CGFloat = 60

    private var utilityRow: some View {
        ViewThatFits(in: .horizontal) {
            utilityKeys(punctuation: 8)
            utilityKeys(punctuation: 6)
            utilityKeys(punctuation: 5)
            utilityKeys(punctuation: 4)
            utilityKeys(punctuation: 3)
            utilityKeys(punctuation: 2)
        }
    }

    private func utilityKeys(punctuation count: Int) -> some View {
        let p = palette
        return HStack(spacing: 5) {
            if model.needsGlobeKey {
                GlobeKeyRepresentable(controller: model.inputViewController, isDark: p.isDark)
                    .frame(width: 38, height: 42)
                    .background(
                        p.keyFill,
                        in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
                    )
                    .shadow(color: p.keyShadow, radius: 0.5, x: 0, y: 1)
            }

            if model.showsEmojiKey {
                Button {
                    model.onKeyFeedback?()
                    model.onEmojiTap?()
                } label: {
                    Image(systemName: "face.smiling")
                        .font(.system(size: 17, weight: .regular))
                        .foregroundStyle(p.textBase)
                        .frame(width: 38, height: 42)
                        .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
                }
                .buttonStyle(KeyboardKeyButtonStyle(palette: p))
                .accessibilityLabel("Emoji")
            }

            ForEach(Self.punctuation.prefix(count), id: \.self) { mark in
                Button {
                    model.onKeyFeedback?()
                    model.onInsertText?(mark)
                } label: {
                    Text(mark)
                        .font(.system(size: 19, weight: .regular))
                        .foregroundStyle(p.textBase)
                        .frame(width: Self.punctuationKeyWidth, height: 42)
                        .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
                }
                .buttonStyle(KeyboardKeyButtonStyle(palette: p))
            }

            Button {
                model.onKeyFeedback?()
                model.onSpace?()
            } label: {
                Text("space")
                    .font(.system(size: 15, weight: .regular))
                    .foregroundStyle(p.textMuted)
                    .lineLimit(1)
                    .frame(minWidth: Self.minSpaceWidth, maxWidth: .infinity)
                    .frame(height: 42)
                    .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }
            .buttonStyle(KeyboardKeyButtonStyle(palette: p))

            RepeatDeleteKey(palette: p) {
                model.onKeyFeedback?()
                model.onDelete?()
            }

            Button {
                model.onKeyFeedback?()
                model.onReturn?()
            } label: {
                Image(systemName: "return")
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(p.textBase)
                    .frame(width: 48, height: 42)
                    .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }
            .buttonStyle(KeyboardKeyButtonStyle(palette: p))
            .accessibilityLabel(model.returnKeyLabel)
        }
    }
}
