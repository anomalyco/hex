import AVFAudio
import Observation
import SwiftUI
import UIKit

/// Live keyboard setup state, read from the system and the App Group.
@MainActor
@Observable
final class KeyboardSetupMonitor {
    static let keyboardBundleID = "anomaly.hex.mobile.keyboard"

    private let bridge: KeyboardBridge
    private let observers = NotificationObservers()

    /// nil when iOS doesn't say which keyboards are enabled.
    private(set) var keyboardAdded: Bool?
    private(set) var fullAccess = KeyboardFullAccess.unconfirmed
    private(set) var microphone = AVAudioApplication.shared.recordPermission
    /// When Hex last came to the foreground (KeyboardFullAccess.resolve).
    private(set) var activeSince: TimeInterval

    var fullAccessOn: Bool { fullAccess == .on }

    init(bridge: KeyboardBridge = KeyboardBridge(), activeSince: TimeInterval = Date().timeIntervalSince1970) {
        self.bridge = bridge
        self.activeSince = activeSince
        refresh()
        // The keyboard says when it reports, so the sheet changes as soon as it opens.
        KeyboardBridge.forwardFullAccessReports()
        observers.observe(KeyboardBridge.fullAccessReported) { [weak self] in self?.refresh() }
        observers.observe(UIApplication.willEnterForegroundNotification) { [weak self] in
            self?.cameToForeground()
        }
    }

    /// Back from Settings, or anywhere else Full Access might have changed.
    func cameToForeground(at now: TimeInterval = Date().timeIntervalSince1970) {
        activeSince = now
        refresh()
    }

    var keyboardReady: Bool {
        keyboardAdded == true && fullAccessOn
    }

    func refresh() {
        let args = ProcessInfo.processInfo.arguments
        if let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count {
            switch args[idx + 1] {
            case "setup", "setup-sheet", "onboarding-try", "onboarding-try-off", "onboarding-try-empty",
                 "onboarding-permissions-done", "onboarding-waiting", "onboarding-failed":
                keyboardAdded = true
                fullAccess = .unconfirmed
                microphone = .granted
            case "onboarding-setup", "onboarding-demo", "onboarding-permissions":
                keyboardAdded = false
                fullAccess = .unconfirmed
                microphone = .undetermined
            case "setup-sheet-microphone":
                keyboardAdded = true
                fullAccess = .unconfirmed
                microphone = .undetermined
            case _ where args[idx + 1].contains("-noaccess"):
                keyboardAdded = true
                fullAccess = .off
                microphone = .granted
            case "onboarding-tour":
                let elapsed = OnboardingFixture.Tour.elapsed
                keyboardAdded = elapsed >= OnboardingFixture.Tour.keyboardAdded
                fullAccess = elapsed >= OnboardingFixture.Tour.fullAccess ? .on : .unconfirmed
                microphone = elapsed >= OnboardingFixture.Tour.microphoneAllowed ? .granted : .undetermined
            default:
                keyboardAdded = true
                fullAccess = .on
                microphone = .granted
            }
            return
        }

        let latest = bridge.fullAccess(activeSince: activeSince)
        if fullAccess != latest { fullAccess = latest }
        if let keyboards = UserDefaults.standard.stringArray(forKey: "AppleKeyboards") {
            keyboardAdded = keyboards.contains(Self.keyboardBundleID) || fullAccessOn
        } else {
            keyboardAdded = fullAccessOn ? true : nil
        }
        microphone = AVAudioApplication.shared.recordPermission
        bridge.appMicrophoneAllowed = microphone == .granted
    }

    func requestMicrophone(then done: (@MainActor () -> Void)? = nil) {
        AVAudioApplication.requestRecordPermission { _ in
            Task { @MainActor [weak self] in
                self?.refresh()
                done?()
            }
        }
    }
}

/// Block observers on NotificationCenter, removed with their owner.
private final class NotificationObservers {
    private var tokens: [NSObjectProtocol] = []

    func observe(_ name: Notification.Name, _ handler: @escaping @MainActor () -> Void) {
        tokens.append(NotificationCenter.default.addObserver(forName: name, object: nil, queue: .main) { _ in
            MainActor.assumeIsolated { handler() }
        })
    }

    deinit {
        tokens.forEach(NotificationCenter.default.removeObserver)
    }
}

/// The four setup steps and how many are left. Full Access only counts as done once the
/// keyboard has run with it, so "not confirmed yet" is an ordinary step, not a failure.
extension KeyboardSetupMonitor {
    func stepsLeft(modelReady: Bool) -> Int {
        [keyboardAdded == true, fullAccessOn, microphone == .granted, modelReady]
            .filter { !$0 }
            .count
    }
}

/// Home's one status line under the Hex header: "Ready", or "Finish setup · 1 step" opening
/// the sheet.
struct SetupStatusRow: View {
    let stepsLeft: Int
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 9) {
                Group {
                    if stepsLeft == 0 {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(HexTheme.success)
                    } else {
                        Image(systemName: "circle.dotted")
                            .font(.system(size: 13, weight: .medium))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
                .frame(width: 20)

                HStack(spacing: 5) {
                    if stepsLeft == 0 {
                        Text("Ready")
                            .foregroundStyle(HexTheme.textMuted)
                    } else {
                        Text("Finish setup")
                            .foregroundStyle(HexTheme.textBase)
                        Text("· \(stepsLeft) steps")
                            .foregroundStyle(HexTheme.textMuted)
                        Image(systemName: "chevron.right")
                            .font(.system(size: 10, weight: .semibold))
                            .foregroundStyle(HexTheme.textMuted)
                    }
                }
            }
            .font(.system(size: 13, weight: .medium))
            .frame(minHeight: 30, alignment: .leading)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel(stepsLeft == 0 ? Text("Hex is ready") : Text("Finish setup, \(stepsLeft) left"))
    }
}

/// The setup checklist, opened from home's status line or the keyboard's Open Hex setup.
struct SetupSheet: View {
    @Environment(\.openURL) private var openURL
    @Environment(\.dismiss) private var dismiss
    @Environment(\.scenePhase) private var scenePhase
    @Bindable var dictation: DictationController
    let monitor: KeyboardSetupMonitor
    let keyboardSpeech: KeyboardSpeechPreparation
    @State private var checkText = ""

    /// Hex's model and the Hex key's own (KeyboardSpeechPreparation).
    private var modelReady: Bool {
        dictation.isModelReady && keyboardSpeech.isReady
    }
    @State private var contentHeight: CGFloat = 360
    @FocusState private var checkFocused: Bool

    /// iOS only tells Hex about Full Access from inside its keyboard, so until the keyboard
    /// has run once with it, the app can't tell "on" from "off".
    private var needsKeyboardCheck: Bool {
        monitor.keyboardAdded != false && !monitor.fullAccessOn
    }

    var body: some View {
        content
            .fixedSize(horizontal: false, vertical: true)
            .onGeometryChange(for: CGFloat.self) { $0.size.height } action: { contentHeight = $0 }
            .frame(maxHeight: .infinity, alignment: .top)
            .presentationDetents([.height(contentHeight)])
            .presentationDragIndicator(.visible)
            .presentationBackground(HexTheme.bgDeep)
            .preferredColorScheme(.dark)
            .onChange(of: monitor.fullAccessOn) { _, on in
                if on { checkFocused = false }
            }
            .onAppear { focusCheckSoon() }
            .onChange(of: scenePhase) { _, phase in
                if phase == .active { focusCheckSoon() }
            }
    }

    /// While Full Access is unconfirmed, the field takes focus so the keyboard comes up: Hex,
    /// if it was the last one, confirms at once; any other keyboard shows the switch hint.
    private func focusCheckSoon() {
        guard needsKeyboardCheck, monitor.fullAccess == .unconfirmed, !checkFocused else { return }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) {
            if needsKeyboardCheck, monitor.fullAccess == .unconfirmed { checkFocused = true }
        }
    }

    private var fullAccessStatus: String {
        switch monitor.fullAccess {
        case .on: String(localized: "On")
        case .off: String(localized: "Off")
        case .unconfirmed: needsKeyboardCheck ? String(localized: "Not confirmed yet") : String(localized: "Off")
        }
    }

    /// What to do next, under the row, once the keyboard is added.
    private var fullAccessNote: String? {
        guard needsKeyboardCheck else { return nil }
        return monitor.fullAccess == .off
            ? String(localized: "Turn on Allow Full Access in Settings for dictation, haptics and copy and paste in the keyboard.")
            : String(localized: "Switch to Hex below to confirm.")
    }

    private var content: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 12) {
                (monitor.stepsLeft(modelReady: modelReady) == 0 ? Text("Hex is ready") : Text("Finish setup"))
                    .font(.system(size: 17, weight: .semibold))
                    .tracking(-0.2)
                    .foregroundStyle(HexTheme.textBase)
                Spacer()
                Button {
                    dismiss()
                } label: {
                    Image(systemName: "xmark")
                        .font(.system(size: 12, weight: .semibold))
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(width: 30, height: 30)
                        .background(Color.white.opacity(0.07), in: Circle())
                        .frame(width: 44, height: 44, alignment: .trailing)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel("Close")
            }
            .frame(height: 30)

            VStack(spacing: 0) {
                row(
                    title: "Hex Keyboard",
                    status: keyboardStatus,
                    icon: monitor.keyboardAdded == true ? .done : .todo
                )
                SettingsDivider(leading: 42)
                row(
                    title: "Full Access",
                    status: fullAccessStatus,
                    icon: monitor.fullAccessOn ? .done : needsKeyboardCheck && monitor.fullAccess == .unconfirmed ? .unconfirmed : .todo,
                    note: fullAccessNote
                )
                SettingsDivider(leading: 42)
                row(
                    title: "Microphone",
                    status: microphoneStatus,
                    icon: monitor.microphone == .granted ? .done : .todo
                ) {
                    // iOS asks right after this tap, so Hex's own button says Continue, never
                    // Allow (App Review 5.1.1(iv)), as onboarding's Set up Hex page does.
                    if monitor.microphone == .undetermined {
                        smallButton("Continue") { monitor.requestMicrophone() }
                    }
                }
                SettingsDivider(leading: 42)
                row(
                    title: "Speech model",
                    status: modelStatus,
                    icon: modelReady ? .done : .todo
                )
            }
            .background(
                HexTheme.bgBase,
                in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
            )

            if needsKeyboardCheck {
                TextField(
                    "",
                    text: $checkText,
                    prompt: (checkFocused
                        ? Text("Switch to Hex with \(Image(systemName: "globe"))")
                        : Text("Tap here, then switch to Hex with \(Image(systemName: "globe"))"))
                        .foregroundStyle(HexTheme.textMuted)
                )
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textBase)
                .focused($checkFocused)
                .padding(.horizontal, 16)
                .frame(minHeight: 46)
                .background(
                    HexTheme.bgBase,
                    in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
                )
                .overlay {
                    RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
                        .strokeBorder(checkFocused ? HexTheme.borderStrong : .clear)
                }
            }

            if !monitor.keyboardReady || monitor.microphone == .denied {
                Button {
                    if let url = URL(string: UIApplication.openSettingsURLString) {
                        openURL(url)
                    }
                } label: {
                    Text("Open Settings")
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(HexTheme.textBase)
                        .frame(maxWidth: .infinity, minHeight: 44)
                        .background(Color.white.opacity(0.09), in: Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 20)
        .padding(.top, 24)
        .padding(.bottom, 20)
    }

    private enum RowIcon {
        case done
        case unconfirmed
        case todo
    }

    private var keyboardStatus: String {
        switch monitor.keyboardAdded {
        case true?: String(localized: "Added")
        case false?: String(localized: "Not added")
        case nil: String(localized: "Not verified")
        }
    }

    private var microphoneStatus: String {
        switch monitor.microphone {
        case .granted: String(localized: "Allowed")
        case .denied: String(localized: "Off")
        default: String(localized: "For dictation")
        }
    }

    private var modelStatus: String {
        if dictation.isModelReady && !keyboardSpeech.isReady {
            switch keyboardSpeech.state {
            case .downloading(let fraction): return "\(Int(fraction * 100))%"
            case .failed: return String(localized: "Failed")
            default: return String(localized: "Preparing")
            }
        }
        if dictation.isModelReady {
            // "Norwegian Bokmål · Apple": the language it really transcribes, short enough
            // for the row.
            let code = Locale(identifier: dictation.transcriptionLanguageID).language.languageCode?.identifier ?? "en"
            let language = Locale.current.localizedString(forLanguageCode: code).map {
                $0.prefix(1).uppercased() + $0.dropFirst()
            } ?? dictation.transcriptionLanguage
            let engine = dictation.engineName == "Apple Speech" ? "Apple" : dictation.engineName
            return "\(language) · \(engine)"
        }
        switch dictation.phase {
        case .preparingModel:
            return "\(Int(dictation.modelPreparation.fractionCompleted * 100))%"
        case .failed:
            return String(localized: "Failed")
        default:
            return String(localized: "Preparing")
        }
    }

    private func row(title: LocalizedStringKey, status: String, icon: RowIcon, note: String? = nil) -> some View {
        row(title: title, status: status, icon: icon, note: note) { EmptyView() }
    }

    private func row<Trailing: View>(
        title: LocalizedStringKey,
        status: String,
        icon: RowIcon,
        note: String? = nil,
        @ViewBuilder trailing: () -> Trailing
    ) -> some View {
        VStack(alignment: .leading, spacing: 3) {
            rowLine(title: title, status: status, icon: icon, trailing: trailing)
            if let note {
                Text(note)
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.leading, 26)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, note == nil ? 0 : 12)
        .frame(minHeight: 46)
    }

    private func rowLine<Trailing: View>(
        title: LocalizedStringKey,
        status: String,
        icon: RowIcon,
        @ViewBuilder trailing: () -> Trailing
    ) -> some View {
        HStack(alignment: .center, spacing: 10) {
            Group {
                switch icon {
                case .done:
                    Image(systemName: "checkmark.circle.fill")
                        .foregroundStyle(HexTheme.success)
                case .unconfirmed:
                    Image(systemName: "circle.dotted")
                        .foregroundStyle(HexTheme.textMuted)
                case .todo:
                    Image(systemName: "circle")
                        .foregroundStyle(HexTheme.textFaint)
                }
            }
            .font(.system(size: 16, weight: .medium))
            .frame(width: 16)

            Text(title)
                .font(.system(size: 15, weight: .regular))
                .foregroundStyle(HexTheme.textBase)

            Spacer(minLength: 8)

            Text(status)
                .font(.system(size: 13))
                .foregroundStyle(HexTheme.textMuted)
                .lineLimit(1)

            trailing()
        }
    }

    private func smallButton(_ title: LocalizedStringKey, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(HexTheme.textBase)
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(Color.white.opacity(0.09), in: Capsule())
            .buttonStyle(.plain)
    }
}
