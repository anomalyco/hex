import AVFAudio
import Observation
import SwiftUI
import UIKit

/// Live keyboard setup state, read from the system and the App Group.
@MainActor
@Observable
final class KeyboardSetupMonitor {
    static let keyboardBundleID = "com.kitlangton.hex.mobile.keyboard"

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
            let snapScene = args[idx + 1]
            if snapScene == "setup" {
                keyboardAdded = true
                fullAccessOn = false
                microphone = .granted
            } else {
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

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Keyboard setup")
                .font(.system(size: 14, weight: .semibold))
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
                    status: monitor.fullAccessOn ? "On" : "Off",
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

            if !monitor.keyboardReady || monitor.microphone == .denied {
                Button {
                    if let url = URL(string: UIApplication.openSettingsURLString) {
                        openURL(url)
                    }
                } label: {
                    HStack(spacing: 7) {
                        Image(systemName: "gearshape.fill")
                            .font(.system(size: 13, weight: .semibold))
                        Text("Open Settings")
                            .font(.system(size: 14, weight: .semibold))
                    }
                    .foregroundStyle(HexTheme.textBase)
                    .frame(maxWidth: .infinity, minHeight: 42)
                    .background(HexTheme.layer02, in: Capsule())
                    .overlay(Capsule().stroke(HexTheme.borderStrong, lineWidth: 1))
                }
                .buttonStyle(.plain)
            }
        }
        .padding(14)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: HexTheme.cardRadius, style: .continuous)
                .stroke(HexTheme.borderWeak, lineWidth: 1)
        )
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
                .font(.system(size: 14, weight: .medium))
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
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(HexTheme.textBase)
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(HexTheme.layer02, in: Capsule())
            .buttonStyle(.plain)
    }

    private var divider: some View {
        Rectangle()
            .fill(HexTheme.borderWeak)
            .frame(height: 1)
    }
}
