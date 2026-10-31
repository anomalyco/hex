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
        VStack(alignment: .leading, spacing: 14) {
            Text("SET UP THE KEYBOARD")
                .font(.system(size: 11, weight: .semibold))
                .tracking(0.6)
                .foregroundStyle(HexTheme.textFaint)

            VStack(spacing: 0) {
                row(
                    title: "Hex Keyboard",
                    detail: keyboardDetail,
                    done: monitor.keyboardAdded == true
                )
                divider
                row(
                    title: "Allow Full Access",
                    detail: monitor.fullAccessOn
                        ? "On. The keyboard can reach Hex."
                        : "Lets the keyboard pass your words from Hex into the text field.",
                    done: monitor.fullAccessOn
                )
                divider
                row(title: "Microphone", detail: microphoneDetail, done: monitor.microphone == .granted) {
                    if monitor.microphone == .undetermined {
                        smallButton("Allow") { monitor.requestMicrophone() }
                    }
                }
                divider
                row(title: "Speech model", detail: modelDetail, done: dictation.isModelReady)
            }

            if !monitor.keyboardReady || monitor.microphone == .denied {
                VStack(alignment: .leading, spacing: 8) {
                    Button {
                        if let url = URL(string: UIApplication.openSettingsURLString) {
                            openURL(url)
                        }
                    } label: {
                        HStack(spacing: 7) {
                            Image(systemName: "gearshape.fill")
                                .font(.system(size: 13, weight: .semibold))
                            Text("Open Hex in Settings")
                                .font(.system(size: 15, weight: .semibold))
                        }
                        .foregroundStyle(HexTheme.textContrast)
                        .frame(maxWidth: .infinity, minHeight: 46)
                        .background(HexTheme.bgContrast, in: Capsule())
                    }
                    .buttonStyle(.plain)

                    Text(settingsInstruction)
                        .font(.system(size: 12))
                        .foregroundStyle(HexTheme.textMuted)
                        .fixedSize(horizontal: false, vertical: true)
                }
            } else if dictation.isModelReady {
                Text("Ready. In any text field, hold 🌐 and choose Hex Keyboard, then tap Start Hex.")
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textBase)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(16)
        .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: HexTheme.cardRadius))
        .overlay(
            RoundedRectangle(cornerRadius: HexTheme.cardRadius)
                .stroke(HexTheme.borderWeak, lineWidth: 1)
        )
    }

    private var keyboardDetail: String {
        switch monitor.keyboardAdded {
        case true?: "Added."
        case false?: "Not added yet."
        case nil: "Open the keyboard once to confirm it's added."
        }
    }

    private var microphoneDetail: String {
        switch monitor.microphone {
        case .granted: "Allowed."
        case .denied: "Off. Turn on Microphone in Settings."
        default: "Hex records only while you dictate."
        }
    }

    private var modelDetail: String {
        if dictation.isModelReady {
            return "\(dictation.engineName) is ready on this iPhone."
        }
        switch dictation.phase {
        case .preparingModel:
            let percent = Int(dictation.modelPreparation.fractionCompleted * 100)
            return "\(dictation.modelPreparation.detail) · \(percent)%"
        case .failed(let message):
            return message
        default:
            return "Runs on this iPhone. Nothing leaves it."
        }
    }

    private var settingsInstruction: String {
        var steps: [String] = []
        if !monitor.keyboardReady {
            steps.append("tap Keyboards, then turn on Hex Keyboard and Allow Full Access")
        }
        if monitor.microphone == .denied {
            steps.append("turn on Microphone")
        }
        return "In Settings, " + steps.joined(separator: ", and ") + ". Then come back here."
    }

    private func row(title: String, detail: String, done: Bool) -> some View {
        row(title: title, detail: detail, done: done) { EmptyView() }
    }

    private func row<Trailing: View>(
        title: String,
        detail: String,
        done: Bool,
        @ViewBuilder trailing: () -> Trailing
    ) -> some View {
        HStack(alignment: .center, spacing: 12) {
            Image(systemName: done ? "checkmark.circle.fill" : "circle")
                .font(.system(size: 18, weight: .medium))
                .foregroundStyle(done ? HexTheme.success : HexTheme.textFaint)

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
                Text(detail)
                    .font(.system(size: 12))
                    .foregroundStyle(HexTheme.textMuted)
                    .lineLimit(2)
            }

            Spacer(minLength: 8)
            trailing()
        }
        .padding(.vertical, 10)
    }

    private func smallButton(_ title: String, action: @escaping () -> Void) -> some View {
        Button(title, action: action)
            .font(.system(size: 13, weight: .semibold))
            .foregroundStyle(HexTheme.textBase)
            .padding(.horizontal, 12)
            .frame(height: 30)
            .background(HexTheme.layer02, in: Capsule())
            .buttonStyle(.plain)
    }

    private var divider: some View {
        Rectangle()
            .fill(HexTheme.borderWeak)
            .frame(height: 1)
    }
}
