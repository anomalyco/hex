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

    /// nil when iOS doesn't say which keyboards are enabled.
    private(set) var keyboardAdded: Bool?
    private(set) var fullAccessOn = false
    private(set) var microphone = AVAudioApplication.shared.recordPermission

    init(bridge: KeyboardBridge = KeyboardBridge()) {
        self.bridge = bridge
        refresh()
    }

    var keyboardReady: Bool {
        keyboardAdded == true && fullAccessOn
    }

    func refresh() {
        let args = ProcessInfo.processInfo.arguments
        if let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count {
            switch args[idx + 1] {
            case "setup", "onboarding-try", "onboarding-try-off":
                keyboardAdded = true
                fullAccessOn = false
                microphone = .granted
            case "onboarding-welcome", "onboarding-allow":
                keyboardAdded = true
                fullAccessOn = false
                microphone = .undetermined
            case "onboarding-tour":
                let elapsed = OnboardingFixture.Tour.elapsed
                keyboardAdded = elapsed >= OnboardingFixture.Tour.keyboardAdded
                fullAccessOn = elapsed >= OnboardingFixture.Tour.fullAccess
                microphone = elapsed >= OnboardingFixture.Tour.microphoneAllowed ? .granted : .undetermined
            default:
                keyboardAdded = true
                fullAccessOn = true
                microphone = .granted
            }
            return
        }

        fullAccessOn = bridge.fullAccessSeenAt != nil
        if let keyboards = UserDefaults.standard.stringArray(forKey: "AppleKeyboards") {
            keyboardAdded = keyboards.contains(Self.keyboardBundleID) || fullAccessOn
        } else {
            keyboardAdded = fullAccessOn ? true : nil
        }
        microphone = AVAudioApplication.shared.recordPermission
    }

    func requestMicrophone() {
        AVAudioApplication.requestRecordPermission { _ in
            Task { @MainActor [weak self] in
                self?.refresh()
            }
        }
    }
}

struct KeyboardSetupCard: View {
    @Environment(\.openURL) private var openURL
    @Bindable var dictation: DictationController
    let monitor: KeyboardSetupMonitor
    @State private var checkText = ""
    @FocusState private var checkFocused: Bool

    /// iOS only tells Hex about Full Access from inside its keyboard, so until the keyboard
    /// has run once with it, the app can't tell "on" from "off".
    private var needsKeyboardCheck: Bool {
        monitor.keyboardAdded != false && !monitor.fullAccessOn
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Keyboard setup")
                .font(.system(size: 15, weight: .medium))
                .foregroundStyle(HexTheme.textBase)

            VStack(spacing: 0) {
                row(
                    title: "Hex Keyboard",
                    status: keyboardStatus,
                    done: monitor.keyboardAdded == true
                )
                divider
                row(
                    title: "Allow Full Access",
                    status: monitor.fullAccessOn ? "On" : needsKeyboardCheck ? "Not checked" : "Off",
                    done: monitor.fullAccessOn
                )
                divider
                row(
                    title: "Microphone",
                    status: microphoneStatus,
                    done: monitor.microphone == .granted
                ) {
                    if monitor.microphone == .undetermined {
                        smallButton("Allow") { monitor.requestMicrophone() }
                    }
                }
                divider
                row(
                    title: "Speech model",
                    status: modelStatus,
                    done: dictation.isModelReady
                )
            }

            if needsKeyboardCheck {
                VStack(alignment: .leading, spacing: 8) {
                    Text("Already turned it on? Tap the field and switch to Hex with \(Image(systemName: "globe")). Hex can only check Full Access from its keyboard.")
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .fixedSize(horizontal: false, vertical: true)
                    TextField("Tap here, then switch to Hex", text: $checkText)
                        .font(.system(size: 15))
                        .foregroundStyle(HexTheme.textBase)
                        .focused($checkFocused)
                        .padding(.horizontal, 14)
                        .frame(minHeight: 40)
                        .background(Color.white.opacity(0.06), in: Capsule())
                }
            }

            if !monitor.keyboardReady || monitor.microphone == .denied {
                Button {
                    if let url = URL(string: UIApplication.openSettingsURLString) {
                        openURL(url)
                    }
                } label: {
                    HStack(spacing: 7) {
                        Image(systemName: "gearshape.fill")
                            .font(.system(size: 13, weight: .medium))
                        Text("Open Settings")
                            .font(.system(size: 15, weight: .medium))
                    }
                    .foregroundStyle(HexTheme.textBase)
                    .frame(maxWidth: .infinity, minHeight: 40)
                    .background(Color.white.opacity(0.09), in: Capsule())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(16)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
        .onChange(of: monitor.fullAccessOn) { _, on in
            if on { checkFocused = false }
        }
    }

    private var keyboardStatus: String {
        switch monitor.keyboardAdded {
        case true?: "Added"
        case false?: "Not added"
        case nil: "Not verified"
        }
    }

    private var microphoneStatus: String {
        switch monitor.microphone {
        case .granted: "Allowed"
        case .denied: "Off"
        default: "Required"
        }
    }

    private var modelStatus: String {
        if dictation.isModelReady {
            return dictation.engineName
        }
        switch dictation.phase {
        case .preparingModel:
            return "\(Int(dictation.modelPreparation.fractionCompleted * 100))%"
        case .failed:
            return "Failed"
        default:
            return "Preparing"
        }
    }

    private func row(title: String, status: String, done: Bool) -> some View {
        row(title: title, status: status, done: done) { EmptyView() }
    }

    private func row<Trailing: View>(
        title: String,
        status: String,
        done: Bool,
        @ViewBuilder trailing: () -> Trailing
    ) -> some View {
        HStack(alignment: .center, spacing: 10) {
            Image(systemName: done ? "checkmark.circle.fill" : "circle")
                .font(.system(size: 16, weight: .medium))
                .foregroundStyle(done ? HexTheme.success : HexTheme.textFaint)

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
        .padding(.vertical, 9)
    }

    private func smallButton(_ title: String, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(HexTheme.textBase)
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(Color.white.opacity(0.09), in: Capsule())
            .buttonStyle(.plain)
    }

    private var divider: some View {
        Rectangle()
            .fill(HexTheme.borderWeak)
            .frame(height: 0.5)
    }
}
