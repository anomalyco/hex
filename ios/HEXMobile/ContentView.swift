import SwiftUI
import UIKit

struct ContentView: View {
    @Environment(\.openURL) private var openURL
    @Bindable var dictation: DictationController
    let openedFromKeyboard: Bool

    var body: some View {
        ZStack {
            Color(hex: 0x0B0C0D)
                .ignoresSafeArea()

            if openedFromKeyboard {
                keyboardHandoff
                    .transition(.opacity)
            } else {
                standaloneRecorder
                    .transition(.opacity)
            }
        }
        .preferredColorScheme(.dark)
        .animation(.easeOut(duration: 0.2), value: openedFromKeyboard)
    }

    private var standaloneRecorder: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header
                recorder

                if !dictation.transcript.isEmpty {
                    transcript
                }

                if case .failed(let message) = dictation.phase {
                    error(message)
                }

                keyboardStatus
                privacy
            }
            .frame(maxWidth: 560)
            .padding(.horizontal, 20)
            .padding(.vertical, 20)
            .frame(maxWidth: .infinity)
        }
    }

    private var header: some View {
        HStack(alignment: .firstTextBaseline) {
            Text("HEX")
                .font(.system(size: 20, weight: .bold, design: .rounded))
                .foregroundStyle(.white)

            Spacer()

            Text("\(dictation.engineName) · \(dictation.transcriptionLanguage)")
                .font(.system(size: 12, weight: .medium))
                .foregroundStyle(.white.opacity(0.42))
                .lineLimit(1)
        }
    }

    private var recorder: some View {
        Button(action: dictation.performPrimaryAction) {
            HStack(spacing: 16) {
                ZStack {
                    Circle()
                        .fill(recorderControlColor)
                        .frame(width: 58, height: 58)

                    if dictation.phase.isBusy {
                        ProgressView()
                            .tint(.black)
                    } else {
                        Image(systemName: primaryIcon)
                            .font(.system(size: 20, weight: .semibold))
                            .foregroundStyle(dictation.isRecording ? .white : .black)
                    }
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text(recorderTitle)
                        .font(.system(size: 18, weight: .semibold))
                        .foregroundStyle(.white)

                    Text(recorderDetail)
                        .font(.system(size: 14))
                        .foregroundStyle(.white.opacity(0.48))
                        .multilineTextAlignment(.leading)
                }

                Spacer(minLength: 0)
            }
            .padding(18)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(hex: 0x151719), in: RoundedRectangle(cornerRadius: 20))
            .overlay {
                RoundedRectangle(cornerRadius: 20)
                    .stroke(.white.opacity(0.06), lineWidth: 1)
            }
            .contentShape(RoundedRectangle(cornerRadius: 20))
        }
        .buttonStyle(.plain)
        .disabled(dictation.phase.isBusy)
        .accessibilityLabel(dictation.primaryTitle)
        .accessibilityHint(primaryHint)
    }

    private var transcript: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack {
                Text("Transcript")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.44))

                Spacer()

                Button(dictation.copied ? "Copied" : "Copy") {
                    dictation.copyTranscript()
                }
                .foregroundStyle(.white.opacity(0.82))

                Button("Clear") {
                    dictation.clearTranscript()
                }
                .foregroundStyle(.white.opacity(0.42))
            }
            .buttonStyle(.plain)
            .font(.system(size: 14, weight: .medium))

            Text(dictation.transcript)
                .font(.system(size: 19, design: .rounded))
                .foregroundStyle(.white.opacity(0.9))
                .textSelection(.enabled)
        }
        .padding(18)
        .background(Color(hex: 0x151719), in: RoundedRectangle(cornerRadius: 20))
    }

    private var keyboardStatus: some View {
        HStack(spacing: 14) {
            Image(systemName: "keyboard")
                .font(.system(size: 17, weight: .medium))
                .foregroundStyle(.white.opacity(0.58))
                .frame(width: 34, height: 34)
                .background(.white.opacity(0.055), in: Circle())

            VStack(alignment: .leading, spacing: 3) {
                Text("HEX Keyboard")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.white.opacity(0.9))

                Text(keyboardStatusDetail)
                    .font(.system(size: 13))
                    .foregroundStyle(.white.opacity(0.42))
            }

            Spacer()

            Button {
                if let url = URL(string: UIApplication.openSettingsURLString) {
                    openURL(url)
                }
            } label: {
                Image(systemName: "gearshape")
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(.white.opacity(0.55))
                    .frame(width: 44, height: 44)
                    .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityLabel("Open HEX Settings")
        }
        .padding(.horizontal, 4)
    }

    private var privacy: some View {
        Label("On-device. Temporary audio is deleted.", systemImage: "lock.fill")
            .font(.system(size: 12, weight: .medium))
            .foregroundStyle(.white.opacity(0.3))
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.top, 8)
    }

    private var keyboardHandoff: some View {
        VStack(spacing: 0) {
            HStack {
                Text("HEX")
                    .font(.system(size: 18, weight: .bold, design: .rounded))
                    .foregroundStyle(.white)
                Spacer()
            }

            Spacer()

            VStack(spacing: 22) {
                handoffSymbol

                VStack(spacing: 8) {
                    Text(handoffTitle)
                        .font(.system(size: 28, weight: .semibold, design: .rounded))
                        .foregroundStyle(.white)

                    Text(handoffDetail)
                        .font(.system(size: 16))
                        .foregroundStyle(.white.opacity(0.5))
                        .multilineTextAlignment(.center)
                        .fixedSize(horizontal: false, vertical: true)
                }

                if dictation.phase == .preparingModel {
                    ProgressView(value: dictation.modelPreparation.fractionCompleted)
                        .tint(.white)
                        .frame(maxWidth: 220)
                }

                if case .failed = dictation.phase {
                    Button("Try again") {
                        dictation.startKeyboardSessionFromKeyboard()
                    }
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(.black)
                    .padding(.horizontal, 18)
                    .padding(.vertical, 10)
                    .background(.white, in: Capsule())
                    .buttonStyle(.plain)
                }
            }
            .frame(maxWidth: 360)

            Spacer()

            if dictation.keyboardSessionEnabled {
                VStack(spacing: 10) {
                    Label("Swipe right along the bottom", systemImage: "arrow.right")
                        .font(.system(size: 16, weight: .semibold))
                        .foregroundStyle(.white)
                        .padding(.horizontal, 18)
                        .padding(.vertical, 13)
                        .background(Color(hex: 0x1A1C1E), in: Capsule())

                    Text("HEX stays ready for \(sessionDuration).")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(.white.opacity(0.32))
                        .monospacedDigit()
                }
            }
        }
        .padding(.horizontal, 24)
        .padding(.vertical, 20)
    }

    @ViewBuilder
    private var handoffSymbol: some View {
        if dictation.isRecording {
            Image(systemName: "mic.fill")
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 64, height: 64)
                .background(Color(hex: 0xA43B40), in: Circle())
        } else if dictation.keyboardSessionEnabled {
            Image(systemName: "checkmark")
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(.black)
                .frame(width: 64, height: 64)
                .background(.white, in: Circle())
        } else if case .failed = dictation.phase {
            Image(systemName: "exclamationmark")
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(.white)
                .frame(width: 64, height: 64)
                .background(Color(hex: 0x7A2E31), in: Circle())
        } else {
            ProgressView()
                .controlSize(.large)
                .tint(.white)
                .frame(width: 64, height: 64)
                .background(Color(hex: 0x151719), in: Circle())
        }
    }

    private func error(_ message: String) -> some View {
        Label(message, systemImage: "exclamationmark.triangle.fill")
            .font(.system(size: 14))
            .foregroundStyle(.white.opacity(0.72))
            .padding(16)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(Color(hex: 0x25191A), in: RoundedRectangle(cornerRadius: 16))
    }

    private var recorderTitle: String {
        return switch dictation.phase {
        case .modelRequired, .failed:
            "Prepare dictation"
        case .preparingModel:
            "Preparing dictation"
        case .ready:
            dictation.transcript.isEmpty ? "Start recording" : "Record again"
        case .requestingPermission:
            "Opening microphone"
        case .recording:
            "Stop recording"
        case .transcribing:
            "Transcribing"
        }
    }

    private var recorderDetail: String {
        return switch dictation.phase {
        case .modelRequired:
            "Set up private, on-device speech."
        case .preparingModel:
            dictation.modelPreparation.detail
        case .ready:
            "Tap once, speak, then tap again."
        case .requestingPermission:
            "Allow access when prompted."
        case .recording:
            duration(dictation.recordingDuration)
        case .transcribing:
            "Processing on this iPhone."
        case .failed:
            "Tap to try again."
        }
    }

    private var recorderControlColor: Color {
        dictation.isRecording ? Color(hex: 0xA43B40) : .white
    }

    private var primaryIcon: String {
        switch dictation.phase {
        case .modelRequired, .failed:
            "arrow.down"
        case .recording:
            "stop.fill"
        default:
            "mic.fill"
        }
    }

    private var primaryHint: String {
        switch dictation.phase {
        case .modelRequired, .failed:
            "Prepares the on-device speech model."
        case .ready:
            "Begins a microphone recording."
        case .recording:
            "Stops recording and begins local transcription."
        default:
            "HEX is working."
        }
    }

    private var keyboardStatusDetail: String {
        if dictation.keyboardSessionEnabled {
            return "Ready · \(sessionDuration) remaining"
        }
        return "Open HEX from the keyboard to connect."
    }

    private var handoffTitle: String {
        if dictation.isRecording {
            return "HEX is listening"
        }
        if dictation.keyboardSessionEnabled {
            return "HEX is ready"
        }

        return switch dictation.phase {
        case .failed:
            "Couldn’t start HEX"
        case .requestingPermission:
            "Allow microphone access"
        default:
            "Getting ready"
        }
    }

    private var handoffDetail: String {
        if dictation.isRecording {
            return "Swipe back and speak, then tap Stop and insert in the keyboard."
        }
        if dictation.keyboardSessionEnabled {
            return "Start near the bottom-left corner to return to the app you were using."
        }

        return switch dictation.phase {
        case .preparingModel:
            dictation.modelPreparation.detail
        case .requestingPermission:
            "HEX needs the microphone for keyboard dictation."
        case .failed(let message):
            message
        default:
            "Starting private, on-device dictation."
        }
    }

    private func duration(_ interval: TimeInterval) -> String {
        let seconds = Int(interval)
        return String(format: "%d:%02d", seconds / 60, seconds % 60)
    }

    private var sessionDuration: String {
        let seconds = max(0, Int(dictation.keyboardSessionRemaining))
        return String(format: "%d:%02d", seconds / 60, seconds % 60)
    }
}

private extension Color {
    init(hex: UInt32) {
        self.init(
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }
}

#Preview {
    ContentView(
        dictation: DictationController(),
        openedFromKeyboard: false
    )
}
