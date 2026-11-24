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
        if controller != nil {
            button.addTarget(context.coordinator, action: #selector(Coordinator.touch(_:with:)), for: .allTouchEvents)
        }
        return button
    }

    func makeCoordinator() -> Coordinator { Coordinator(controller: controller) }

    /// Holds back a tap where Apple's keyboard splits, as the full keyboard's globe does.
    @MainActor final class Coordinator: NSObject {
        weak var controller: UIInputViewController?
        private let inputModeListTouch = InputModeListTouch()

        init(controller: UIInputViewController?) { self.controller = controller }

        @objc func touch(_ sender: UIButton, with event: UIEvent) {
            guard let controller else { return }
            inputModeListTouch.forward(sender, with: event, to: controller)
        }
    }

    func updateUIView(_ uiView: UIButton, context: Context) {
        context.coordinator.controller = controller
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

    /// The return key's colours, as the full keyboard's (`FullKeyboardPalette`).
    var actionFill: Color { Color(red: 0, green: 0x7A / 255.0, blue: 1) }
    var actionPressedFill: Color { Color(red: 0, green: 0x62 / 255.0, blue: 0xCC / 255.0) }
    var actionDisabledFill: Color {
        isDark ? Color.white.opacity(0.39) : Color(red: 0xBF / 255.0, green: 0xC0 / 255.0, blue: 0xC5 / 255.0)
    }
    var returnDisabledInk: Color { textBase.opacity(0.1) }
}

private struct KeyboardKeyButtonStyle: ButtonStyle {
    let palette: KeyboardChromePalette
    /// Another keycap colour, idle and pressed: the return key's blue.
    var fill: (idle: Color, pressed: Color)?

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(
                configuration.isPressed ? fill?.pressed ?? palette.keyPressedFill : fill?.idle ?? palette.keyFill,
                in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
            )
            .shadow(color: palette.keyShadow, radius: 0.5, x: 0, y: 1)
    }
}

/// The top row's tap band: the 26 pt pills keep their size and centre in it.
private enum TopBarMetrics {
    static let height: CGFloat = 40
    static let hitWidth: CGFloat = 44
    /// The surface's side inset, owned by the end controls so taps reach the edges.
    static let sideInset: CGFloat = 10
    /// The Hex mark's label before its side inset.
    static let hexMarkWidth: CGFloat = 22
    /// How far the mark's tap area reaches into the status group's slot.
    static let hexMarkOverlap = hitWidth - sideInset - hexMarkWidth
}

/// The row's full height and at least 44 pt of width, without drawing any of it:
/// apply after the control's 26 pt visual so the visual keeps its size.
private struct TopBarHitTarget: ViewModifier {
    var alignment: Alignment = .center

    func body(content: Content) -> some View {
        content
            .frame(
                minWidth: TopBarMetrics.hitWidth,
                minHeight: TopBarMetrics.height,
                alignment: alignment
            )
            .contentShape(Rectangle())
    }
}

private extension View {
    func topBarHitTarget(alignment: Alignment = .center) -> some View {
        modifier(TopBarHitTarget(alignment: alignment))
    }
}

private struct TopBarTriggerButtonStyle: ButtonStyle {
    let palette: KeyboardChromePalette
    /// The side inset belongs to the tap area, drawn after the pressed capsule.
    var leadingHitInset: CGFloat = 0
    var trailingHitInset: CGFloat = 0

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .background(
                configuration.isPressed ? palette.topBarPressedFill : Color.clear,
                in: Capsule()
            )
            .padding(.leading, leadingHitInset)
            .padding(.trailing, trailingHitInset)
            .topBarHitTarget()
    }
}

/// A tray chip: its capsule keeps its size and the row's tap area surrounds it.
private struct TopBarChipButtonStyle: ButtonStyle {
    /// The row's side inset, owned by the first chip in a tray.
    var leadingHitInset: CGFloat = 0

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.leading, leadingHitInset)
            .topBarHitTarget(alignment: leadingHitInset == 0 ? .center : .leading)
    }
}

private struct RepeatDeleteKey: View {
    let palette: KeyboardChromePalette
    var glyphOpacity: Double = 1
    let onDelete: () -> Void
    @State private var isPressing = false
    @State private var repeatTask: Task<Void, Never>?

    var body: some View {
        Image(systemName: "delete.left")
            .font(.system(size: 15, weight: .medium))
            .foregroundStyle(palette.textBase)
            .opacity(glyphOpacity)
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

/// The dictation bar's space bar, a cursor trackpad like the full keyboard's
/// (`FullKeyboardEngine`): sliding `trackpadActivationDistance`, or holding still for
/// `trackpadHoldDelay`, starts it, and each `trackpadPointsPerCharacter` along moves the cursor
/// a character with a selection tick. Letting go of a trackpad types nothing; a tap types a space.
private struct TrackpadSpaceKey: View {
    let palette: KeyboardChromePalette
    let minWidth: CGFloat
    @Binding var isTrackpad: Bool
    let onPress: () -> Void
    let onSpace: () -> Void
    let onMove: (Int) -> Void
    let onStep: () -> Void

    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @GestureState private var isTouching = false
    @State private var isPressing = false
    @State private var startX: CGFloat = 0
    @State private var anchorX: CGFloat = 0
    @State private var lastX: CGFloat = 0
    @State private var holdTask: Task<Void, Never>?

    var body: some View {
        Text("space")
            .font(.system(size: 15, weight: .regular))
            .foregroundStyle(palette.textMuted)
            .lineLimit(1)
            .opacity(isTrackpad ? 0 : 1)
            .frame(minWidth: minWidth, maxWidth: .infinity)
            .frame(height: 42)
            .background(
                isPressing ? palette.keyPressedFill : palette.keyFill,
                in: RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous)
            )
            .shadow(color: palette.keyShadow, radius: 0.5, x: 0, y: 1)
            .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            .gesture(
                DragGesture(minimumDistance: 0)
                    .updating($isTouching) { _, touching, _ in touching = true }
                    .onChanged { value in moved(to: value.location.x, from: value.startLocation.x) }
                    .onEnded { _ in finish(typesSpace: true) }
            )
            .onChange(of: isTouching) { _, touching in
                guard !touching else { return }
                // A cancelled touch never reaches onEnded, and types nothing; an ended one already finished.
                DispatchQueue.main.async { finish(typesSpace: false) }
            }
            .accessibilityElement(children: .combine)
            .accessibilityAddTraits(.isButton)
            .accessibilityAction { onSpace() }
    }

    private func moved(to x: CGFloat, from start: CGFloat) {
        lastX = x
        if !isPressing {
            isPressing = true
            startX = start
            onPress()
            holdTask?.cancel()
            holdTask = Task { @MainActor in
                try? await Task.sleep(for: .seconds(FullKeyboardEngine.trackpadHoldDelay))
                guard !Task.isCancelled, isPressing, !isTrackpad else { return }
                begin(at: lastX)
            }
        }
        guard isTrackpad else {
            if abs(x - startX) >= FullKeyboardEngine.trackpadActivationDistance {
                begin(at: x)
            }
            return
        }
        let step = FullKeyboardEngine.trackpadPointsPerCharacter
        let steps = Int(((x - anchorX) / step).rounded(.towardZero))
        guard steps != 0 else { return }
        anchorX += CGFloat(steps) * step
        onMove(steps)
        onStep()
    }

    private func begin(at x: CGFloat) {
        holdTask?.cancel()
        holdTask = nil
        anchorX = x
        setTrackpad(true)
        onStep()
    }

    private func finish(typesSpace: Bool) {
        holdTask?.cancel()
        holdTask = nil
        guard isPressing else { return }
        isPressing = false
        if isTrackpad {
            setTrackpad(false)
        } else if typesSpace {
            onSpace()
        }
    }

    /// The glyphs blank at once under Reduce Motion, as the full keyboard's do, else fade quickly.
    private func setTrackpad(_ on: Bool) {
        withAnimation(reduceMotion ? nil : .easeOut(duration: 0.12)) {
            isTrackpad = on
        }
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
    static let fullAccessPath = String(localized: "Allow Full Access: Settings › Apps › Hex › Keyboards")
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
    /// That long press's own touch-up, so it doesn't also pick the language.
    @State private var removeTookPress = false
    /// The space bar slides the cursor: the keys' glyphs blank out, as on the full keyboard.
    @State private var isTrackpadActive = false

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
        model.stagePhase == .starting || model.stagePhase == .recording || model.stagePhase == .transcribing
    }

    var body: some View {
        let showsMore = model.activeTray == .moreLanguages
        // The top row spans the bar, its controls reaching the screen's edges;
        // everything under it keeps the surface's side inset.
        VStack(spacing: 2) {
            topBar
            ZStack {
                VStack(spacing: 8) {
                    primaryStage
                    utilityRow
                }
                .opacity(showsMore ? 0 : 1)
                .allowsHitTesting(!showsMore)
                .accessibilityHidden(showsMore)
                if showsMore {
                    moreLanguagesGrid(palette: palette)
                        .transition(.opacity)
                }
            }
            .padding(.horizontal, TopBarMetrics.sideInset)
        }
        .onChange(of: model.activeTray) { _, _ in
            removeTookPress = false
        }
        .padding(.top, 4)
        .padding(.bottom, 6)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(isHostedExtension ? Color.clear : HexTheme.bgDeep)
        .preferredColorScheme(isHostedExtension ? nil : .dark)
    }

    // MARK: - Top Bar (40 pt around 26 pt visuals, permanently mounted ZStack)

    private var topBar: some View {
        let p = palette
        return ZStack {
            defaultTopBar(palette: p)
                .hexTrayReveal(model.activeTray == .none, reduceMotion: reduceMotion, offsetY: -3)

            styleTrayBar(palette: p)
                .hexTrayReveal(model.activeTray == .style, reduceMotion: reduceMotion, offsetY: 3.5)

            languageTrayBar(palette: p)
                .hexTrayReveal(model.activeTray == .language, reduceMotion: reduceMotion, offsetY: 3.5)

            moreLanguagesBar(palette: p)
                .hexTrayReveal(model.activeTray == .moreLanguages, reduceMotion: reduceMotion, offsetY: 3.5)
        }
        .frame(height: TopBarMetrics.height)
    }

    private func defaultTopBar(palette p: KeyboardChromePalette) -> some View {
        HStack(spacing: 8) {
            Button {
                model.onKeyFeedback?()
                switch model.route {
                case .setUp:
                    model.onOpenURL?(KeyboardLaunchRequest.setup.url)
                case .warm:
                    // As Dictate, which shows only while nothing records.
                    if !isStageActive { model.onDictateTap?() }
                case .inKeyboard:
                    model.onLocalDictateTap?()
                case .needsModel, .opensHex:
                    model.dictateWhereUnavailable()
                }
            } label: {
                HexMarkView(
                    size: 16,
                    color: isStageActive && p.isDark ? HexTheme.warmPearl : p.textBase
                )
                .frame(width: TopBarMetrics.hexMarkWidth, height: 26, alignment: .leading)
                .padding(.leading, TopBarMetrics.sideInset)
                .topBarHitTarget(alignment: .leading)
            }
            .buttonStyle(.plain)
            .accessibilityLabel(hexMarkLabel)

            // The mark's tap area reaches past its pill into this group's slot: the
            // outdent keeps the badge and the status where they were.
            HStack(spacing: 8) {
                if let target = model.translatingTo {
                    TranslationBadge(target: target, palette: p)
                }

                leadingAccessory(palette: p)
            }
            .padding(.leading, -TopBarMetrics.hexMarkOverlap)

            Spacer(minLength: 4)

            if showsPasteDictation {
                pasteDictationChip(palette: p)
                Spacer(minLength: 4)
            }

            // A note (the translation's, or why a dictation tap can't dictate here) takes the
            // bar's width with Open Hex and ✕ for its few seconds.
            if let note = barNote {
                noteActions(note, palette: p)
            } else if model.showsLanguageSwitch || model.showsTranslationSwitch {
                languageTrigger(palette: p)
            }

            if barNote == nil {
                styleTrigger(palette: p)
                    .hexTrayReveal(model.hasFullAccess, reduceMotion: reduceMotion, offsetY: 0)
            }
        }
        .frame(height: TopBarMetrics.height)
    }

    /// What the Hex mark does, for VoiceOver: dictate as Dictate does, set up, or (before iOS 26
    /// where the keyboard can't dictate) open Hex to dictate.
    private var hexMarkLabel: String {
        if model.route == .setUp { return String(localized: "Set up Hex") }
        return model.dictationTapOpensHex ? String(localized: "Open Hex") : String(localized: "Dictate")
    }

    private enum BarNote {
        case translation
        case dictationBlock
    }

    private var barNote: BarNote? {
        switch model.statusSlot {
        case .translationNote: .translation
        case .dictationBlock: .dictationBlock
        default: nil
        }
    }

    /// The note's only controls: Open Hex, the one part that opens Hex (where Hex can help),
    /// and ✕, which puts the note away. A tap on the words does nothing.
    private func noteActions(_ note: BarNote, palette p: KeyboardChromePalette) -> some View {
        HStack(spacing: 6) {
            if note == .translation || model.dictationBlock?.opensHex == true {
                openHexChip(note, palette: p)
            }

            Button {
                model.onKeyFeedback?()
                switch note {
                case .translation: model.onDismissTranslationNote?()
                case .dictationBlock: model.dismissDictationBlock()
                }
            } label: {
                Image(systemName: "xmark")
                    .font(.system(size: 11, weight: .medium))
                    .foregroundStyle(p.textMuted)
                    .frame(width: 26, height: 26)
                    .background(p.chipIdleFill, in: Circle())
                    .padding(.trailing, TopBarMetrics.sideInset)
                    .topBarHitTarget(alignment: .trailing)
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Dismiss")
        }
        .layoutPriority(1)
    }

    private func openHexChip(_ note: BarNote, palette p: KeyboardChromePalette) -> some View {
        Button {
            model.onKeyFeedback?()
            switch note {
            case .translation: model.onOpenURL?(KeyboardLaunchRequest.translationSettings.url)
            case .dictationBlock: model.openHexForDictationBlock()
            }
        } label: {
            Text("Open Hex")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(p.textBase)
                .lineLimit(1)
                .fixedSize()
                .padding(.horizontal, 11)
                .frame(height: 26)
                .background(p.chipActiveFill, in: Capsule())
        }
        .buttonStyle(TopBarChipButtonStyle())
        .accessibilityHint(note == .translation ? Text("Opens Hex's Settings") : Text("Opens Hex to dictate there"))
    }

    /// A dictation that didn't go in by itself, as the full keyboard's strip offers it: the
    /// bar is the only keyboard on an iPad, and without this a late one was out of reach.
    private var showsPasteDictation: Bool {
        model.hasFullAccess && model.offeredDictation != nil && !isStageActive && barNote == nil
    }

    private func pasteDictationChip(palette p: KeyboardChromePalette) -> some View {
        Button {
            model.onKeyFeedback?()
            model.onPasteDictation?()
        } label: {
            Text("Paste dictation")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(p.textBase)
                .lineLimit(1)
                .fixedSize()
                .padding(.horizontal, 11)
                .frame(height: 26)
                .background(p.chipActiveFill, in: Capsule())
        }
        .buttonStyle(TopBarChipButtonStyle())
        .layoutPriority(1)
    }

    private func styleTrayBar(palette p: KeyboardChromePalette) -> some View {
        let firstStyle = DictationStyle.allCases.first
        return HStack(spacing: 4) {
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
                .buttonStyle(TopBarChipButtonStyle(
                    leadingHitInset: candidate == firstStyle ? TopBarMetrics.sideInset : 0
                ))
            }
            Spacer(minLength: 4)
            closeTrayButton(palette: p)
        }
        .frame(height: TopBarMetrics.height)
    }

    private func leadingAccessory(palette p: KeyboardChromePalette) -> some View {
        let slot = model.statusSlot
        let messageSlot: (text: String, isFailure: Bool)? = if case .message(let text, let isFailure) = slot {
            (text, isFailure)
        } else {
            nil
        }
        let noteSlot: TranslationFailure? = if case .translationNote(let note) = slot {
            note
        } else {
            nil
        }
        let blockSlot: KeyboardDictationBlock? = if case .dictationBlock(let block) = slot {
            block
        } else {
            nil
        }

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
            .hexTrayReveal(slot == .noAccess, reduceMotion: reduceMotion, offsetY: 2)

            Text("Starting…")
                .font(.system(size: 13, weight: .regular))
                .foregroundStyle(p.textMuted)
                .lineLimit(1)
                .hexTrayReveal(slot == .starting, reduceMotion: reduceMotion, offsetY: 2)

            HStack(spacing: 6) {
                HexStatusDot(color: HexTheme.danger, size: 6.5)
                KeyboardRecordingClockText(clock: model.clock, color: p.textBase)
            }
            .hexTrayReveal(slot == .recording, reduceMotion: reduceMotion, offsetY: 2)

            Group {
                if let messageSlot {
                    HStack(spacing: 6) {
                        if messageSlot.isFailure {
                            HexStatusDot(color: HexTheme.danger, size: 6)
                        }
                        // A failure gets two smaller lines: it has to be read in full.
                        Text(messageSlot.text)
                            .font(.system(size: messageSlot.isFailure ? 11 : 13, weight: .regular))
                            .foregroundStyle(p.textMuted)
                            .lineLimit(messageSlot.isFailure ? 2 : 1)
                            .minimumScaleFactor(0.85)
                    }
                }
            }
            .hexTrayReveal(messageSlot != nil, reduceMotion: reduceMotion, offsetY: 2)

            HStack(spacing: 5) {
                HexStatusDot(color: HexTheme.warmMic, size: 6)
                KeyboardWarmRemainingText(clock: model.clock, color: p.textMuted)
            }
            .hexTrayReveal(slot == .warm, reduceMotion: reduceMotion, offsetY: 2)

            Group {
                if let noteSlot {
                    translationNote(noteSlot, palette: p)
                }
            }
            .hexTrayReveal(noteSlot != nil, reduceMotion: reduceMotion, offsetY: 2)

            // Why a dictation tap can't dictate here; Open Hex and ✕ sit at the bar's end.
            Group {
                if let blockSlot {
                    Text(blockSlot.line)
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(p.textBase)
                        .lineLimit(2)
                        .minimumScaleFactor(0.8)
                }
            }
            .hexTrayReveal(blockSlot != nil, reduceMotion: reduceMotion, offsetY: 2)
        }
    }

    /// "Couldn't translate, pasted as spoken" over its fix. Only Open Hex at the bar's end opens
    /// Hex's Settings › Dictation; a tap on these words does nothing.
    private func translationNote(_ note: TranslationFailure, palette p: KeyboardChromePalette) -> some View {
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
        .accessibilityElement(children: .combine)
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
        }
        .buttonStyle(TopBarTriggerButtonStyle(palette: p, trailingHitInset: TopBarMetrics.sideInset))
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
        }
        .buttonStyle(TopBarTriggerButtonStyle(palette: p))
        // "Español → English" before the status beside the Hex mark, which is short whenever
        // the switch shows (it needs Full Access).
        .layoutPriority(1)
        .accessibilityLabel("Dictation language: \(label)")
    }

    private func languageTrayBar(palette p: KeyboardChromePalette) -> some View {
        // Apple Speech's languages here, as the Hex key menu names them: the chips and More
        // languages together name a language's region where it has several ("Español (US)").
        let names = DictationLanguageSwitch.nativeNames(
            for: model.languageChoices,
            supported: model.languageChoices + model.moreLanguages
        )
        // The close's 44 pt tap area ends on the screen's edge, so the spacing before
        // its pair keeps the scroll view's end at the same place either way.
        let hasTranslationChip = model.showsTranslationSwitch && model.translationLanguage != nil
        return HStack(spacing: hasTranslationChip ? 4 : -4) {
            ScrollView(.horizontal, showsIndicators: false) {
                HStack(spacing: 4) {
                    if model.showsLanguageSwitch {
                        languageChip(id: nil, label: String(localized: "Auto"), palette: p)
                        ForEach(model.languageChoices, id: \.self) { id in
                            languageChip(id: id, label: names[id] ?? id, palette: p)
                            if model.removingLanguage == id {
                                removeChip(id: id, label: names[id] ?? id, palette: p)
                            }
                        }
                        if !model.moreLanguages.isEmpty {
                            moreChip(palette: p)
                        }
                    }
                }
                .padding(.leading, 2)
                .padding(.trailing, 10)
            }
            .hexScrollEdgeEffectHidden()
            .hexHorizontalEdgeFade(leading: 4, trailing: 14)

            // The close overlaps 4 pt of what is beside it: with the chip, the pair
            // keeps the chip where it was and the chip wins its own pixels; without
            // it, the overlap is the scroll view's trailing fade.
            HStack(spacing: -4) {
                if hasTranslationChip, let target = model.translationLanguage {
                    translationChip(target: target, palette: p)
                        .zIndex(1)
                }
                closeTrayButton(palette: p)
            }
        }
        .padding(.leading, TopBarMetrics.sideInset)
        .frame(height: TopBarMetrics.height)
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
        .buttonStyle(TopBarChipButtonStyle())
        .accessibilityLabel(title)
        .accessibilityValue(active ? "On" : "Off")
        .accessibilityAddTraits(active ? .isSelected : [])
    }

    /// A language: a tap picks it; a long press offers Remove beside it (not Automatic).
    private func languageChip(id: String?, label: String, palette p: KeyboardChromePalette) -> some View {
        let active = model.dictationLanguage == id
        return Button {
            if let id, model.removingLanguage == id {
                // The long press's own release keeps Remove up; a later tap puts it away.
                if removeTookPress {
                    removeTookPress = false
                } else {
                    setRemovingLanguage(nil)
                }
                return
            }
            model.removingLanguage = nil
            removeTookPress = false
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
        .buttonStyle(TopBarChipButtonStyle())
        .simultaneousGesture(
            LongPressGesture(minimumDuration: Self.removeHoldDuration).onEnded { _ in
                guard let id, model.removingLanguage != id else { return }
                model.onSelectionStep?()
                removeTookPress = true
                setRemovingLanguage(id)
            },
            including: id == nil ? .subviews : .all
        )
        .accessibilityLabel(id == nil ? "Automatic language" : label)
        .accessibilityActions {
            if let id {
                Button("Remove") { model.onLanguageRemoved?(id) }
            }
        }
    }

    /// How long a language chip is held before Remove shows beside it.
    private static let removeHoldDuration: Double = 0.45

    private func setRemovingLanguage(_ id: String?) {
        withAnimation(HexMotion.trayAnimation(reduceMotion: reduceMotion)) {
            model.removingLanguage = id
        }
    }

    private func removeChip(id: String, label: String, palette p: KeyboardChromePalette) -> some View {
        Button {
            model.onKeyFeedback?()
            model.onLanguageRemoved?(id)
            removeTookPress = false
            setRemovingLanguage(nil)
        } label: {
            Text("Remove")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(HexTheme.danger)
                .padding(.horizontal, 10)
                .frame(height: 26)
                .background(p.chipIdleFill, in: Capsule())
        }
        .buttonStyle(TopBarChipButtonStyle())
        .transition(.opacity)
        .accessibilityLabel(Text("Remove \(label)"))
    }

    private func moreChip(palette p: KeyboardChromePalette) -> some View {
        Button {
            model.onKeyFeedback?()
            model.setActiveTray(.moreLanguages, reduceMotion: reduceMotion)
        } label: {
            Text("More languages…")
                .font(.system(size: 13, weight: .medium))
                .foregroundStyle(p.textMuted)
                .lineLimit(1)
                .padding(.horizontal, 10)
                .frame(height: 26)
                .background(p.chipIdleFill, in: Capsule())
        }
        .buttonStyle(TopBarChipButtonStyle())
    }

    /// More languages…'s bar: back to the language chips, and close.
    private func moreLanguagesBar(palette p: KeyboardChromePalette) -> some View {
        HStack(spacing: 8) {
            Button {
                model.onKeyFeedback?()
                model.setActiveTray(.language, reduceMotion: reduceMotion)
            } label: {
                HStack(spacing: 4) {
                    Image(systemName: "chevron.left")
                        .font(.system(size: 11, weight: .semibold))
                    Text("More languages")
                        .font(.system(size: 13, weight: .medium))
                        .lineLimit(1)
                }
                .foregroundStyle(p.textMuted)
                .padding(.horizontal, 8)
                .frame(height: 26)
            }
            .buttonStyle(TopBarTriggerButtonStyle(palette: p, leadingHitInset: TopBarMetrics.sideInset))
            .accessibilityLabel("Back")
            Spacer(minLength: 4)
            closeTrayButton(palette: p)
        }
        .frame(height: TopBarMetrics.height)
    }

    /// Every other language Apple Speech has, in place of the keys; a tap picks it, which puts
    /// it on the switch.
    private func moreLanguagesGrid(palette p: KeyboardChromePalette) -> some View {
        let names = DictationLanguageSwitch.nativeNames(for: model.languageChoices + model.moreLanguages)
        return ScrollView(.vertical, showsIndicators: false) {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 96), spacing: 6)], spacing: 6) {
                ForEach(model.moreLanguages, id: \.self) { id in
                    Button {
                        model.onKeyFeedback?()
                        model.onLanguageSelected?(id)
                        model.setActiveTray(.none, reduceMotion: reduceMotion)
                    } label: {
                        Text(names[id] ?? id)
                            .font(.system(size: 14, weight: .regular))
                            .foregroundStyle(p.textBase)
                            .lineLimit(1)
                            .minimumScaleFactor(0.75)
                            .padding(.horizontal, 8)
                            .frame(maxWidth: .infinity)
                            .frame(height: 34)
                            .background(p.chipIdleFill, in: Capsule())
                            .contentShape(Capsule())
                    }
                    .buttonStyle(.plain)
                }
            }
        }
        .hexScrollEdgeEffectHidden()
        .frame(height: Self.wellHeight + 8 + Self.keyHeight)
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
                .padding(.trailing, TopBarMetrics.sideInset)
                .topBarHitTarget(alignment: .trailing)
        }
        .buttonStyle(.plain)
        .accessibilityLabel("Close")
    }

    // MARK: - Unified 44 pt Recessed Stage Well (all controls permanently mounted)

    private var idleTriggerSpec: (icon: String, title: String, width: CGFloat) {
        switch model.route {
        case .setUp:
            return ("arrow.up.forward.app.fill", String(localized: "Set up Hex"), 112)
        case .needsModel:
            return ("mic.fill", String(localized: "Set up"), 88)
        case .warm, .inKeyboard, .opensHex:
            return ("mic.fill", String(localized: "Dictate"), 94)
        }
    }

    private var primaryStage: some View {
        let p = palette
        let showStarting = model.stagePhase == .starting
        let showRecordingControls = model.stagePhase == .recording
        let showCancelControl = showStarting || showRecordingControls
        let showIdleTrigger = model.stagePhase == .idle || model.stagePhase == .failed
        let spec = idleTriggerSpec

        let leadInset: Float = showCancelControl
            ? Float(Self.wellInset + Self.cancelWidth + 6)
            : 0.0
        let trailInset: Float = if showCancelControl {
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

            // Recording peer controls (`✕`, starting spinner, and `Done`), always mounted so enter/exit springs stay continuous.
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
                    showCancelControl,
                    reduceMotion: reduceMotion,
                    scale: 0.90,
                    blur: 2.5,
                    anchor: .leading
                )

                Spacer(minLength: 0)

                ZStack(alignment: .trailing) {
                    ProgressView()
                        .controlSize(.small)
                        .tint(p.textMuted)
                        .frame(width: Self.doneWidth, height: Self.controlHeight)
                        .background(p.controlSubtleFill, in: Capsule())
                        .accessibilityLabel("Starting microphone")
                        .hexControlReveal(
                            showStarting,
                            reduceMotion: reduceMotion,
                            scale: 0.90,
                            blur: 2.5,
                            anchor: .trailing
                        )

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
        model.canPushToTalk
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
        switch model.route {
        case .setUp:
            model.onOpenURL?(KeyboardLaunchRequest.setup.url)
        case .warm:
            model.onDictateTap?()
        case .inKeyboard:
            model.onLocalDictateTap?()
        case .needsModel, .opensHex:
            model.dictateWhereUnavailable()
        }
    }

    // MARK: - Bottom Utility Key Row (42 pt, shadow-grounded keycaps with zero strokes)

    /// The bar's punctuation keys, the commonest first: as many as leave the space bar
    /// `minSpaceWidth` (five on a 402 pt iPhone, three on the SE beside its globe).
    private static let punctuation = [",", ".", "?", "!", "'", "\"", "@", "#"]
    private static let punctuationKeyWidth: CGFloat = 30
    private static let minSpaceWidth: CGFloat = 60
    private static let keyHeight: CGFloat = 42

    /// The keys' glyphs while the space bar slides the cursor: blank, as on the full keyboard.
    private var glyphOpacity: Double { isTrackpadActive ? 0 : 1 }

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
                    .opacity(glyphOpacity)
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
                        .opacity(glyphOpacity)
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
                        .opacity(glyphOpacity)
                        .frame(width: Self.punctuationKeyWidth, height: 42)
                        .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
                }
                .buttonStyle(KeyboardKeyButtonStyle(palette: p))
            }

            TrackpadSpaceKey(
                palette: p,
                minWidth: Self.minSpaceWidth,
                isTrackpad: $isTrackpadActive,
                onPress: { model.onKeyFeedback?() },
                onSpace: { model.onSpace?() },
                onMove: { model.onMoveCursor?($0) },
                onStep: { model.onSelectionStep?() }
            )

            RepeatDeleteKey(palette: p, glyphOpacity: glyphOpacity) {
                model.onKeyFeedback?()
                model.onDelete?()
            }

            Button {
                model.onKeyFeedback?()
                model.onReturn?()
            } label: {
                Image(systemName: model.returnKey.symbol)
                    .font(.system(size: 15, weight: .medium))
                    .foregroundStyle(returnInk(p))
                    .opacity(glyphOpacity)
                    .frame(width: 48, height: 42)
                    .contentShape(RoundedRectangle(cornerRadius: HexTheme.controlRadius, style: .continuous))
            }
            .buttonStyle(KeyboardKeyButtonStyle(palette: p, fill: returnFill(p)))
            .disabled(!model.returnKeyEnabled)
            .accessibilityLabel(model.returnKey.accessibilityLabel)
        }
    }

    /// Send, go, search and the other acting types are blue, grey while the field is empty and
    /// the host turns them off; plain while the space bar is a trackpad, as on the full keyboard.
    private func returnFill(_ p: KeyboardChromePalette) -> (idle: Color, pressed: Color)? {
        guard model.returnKey.isAction, !isTrackpadActive else { return nil }
        return model.returnKeyEnabled
            ? (p.actionFill, p.actionPressedFill)
            : (p.actionDisabledFill, p.actionDisabledFill)
    }

    private func returnInk(_ p: KeyboardChromePalette) -> Color {
        if !model.returnKeyEnabled { return p.returnDisabledInk }
        return model.returnKey.isAction ? .white : p.textBase
    }
}
