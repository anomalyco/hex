import SwiftUI

/// Renders `KeyboardSurfaceView` docked beneath a dark composer stage
/// when `--snap-state keyboard-*` is passed on Simulator for deterministic UI proof.
struct KeyboardSnapshotStage: View {
    let scene: String
    @StateObject private var viewModel = KeyboardViewModel()

    static var requestedScene: String? {
        let args = ProcessInfo.processInfo.arguments
        guard let idx = args.firstIndex(of: "--snap-state"), idx + 1 < args.count else {
            return nil
        }
        let value = args[idx + 1]
        return value.hasPrefix("keyboard-") ? value : nil
    }

    var body: some View {
        VStack(spacing: 0) {
            Spacer()

            VStack(alignment: .leading, spacing: 10) {
                Text("Messages")
                    .font(.system(size: 13, weight: .medium))
                    .foregroundStyle(HexTheme.textMuted)

                Text(composerText)
                    .font(.system(size: 15))
                    .foregroundStyle(composerTextIsPlaceholder ? HexTheme.textFaint : HexTheme.textBase)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 10)
                    .background(HexTheme.bgBase, in: RoundedRectangle(cornerRadius: 18, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 18, style: .continuous)
                            .stroke(HexTheme.borderWeak, lineWidth: 1)
                    )
            }
            .padding(.horizontal, 16)
            .padding(.bottom, 14)

            Divider()
                .overlay(HexTheme.borderWeak)

            KeyboardSurfaceView(model: viewModel)
                .frame(height: 148)
        }
        .background(Color(red: 0x07 / 255.0, green: 0x08 / 255.0, blue: 0x09 / 255.0).ignoresSafeArea())
        .preferredColorScheme(.dark)
        .onAppear {
            configureScene()
        }
    }

    private var composerText: String {
        switch scene {
        case "keyboard-warm":
            "let's ship the iOS keyboard build tonight"
        case "keyboard-recording", "keyboard-transcribing":
            "Sending over the TestFlight invite…"
        default:
            "Message"
        }
    }

    private var composerTextIsPlaceholder: Bool {
        scene != "keyboard-warm" && scene != "keyboard-recording" && scene != "keyboard-transcribing"
    }

    private func configureScene() {
        let now: TimeInterval = 10_000
        viewModel.now = now
        viewModel.hasFullAccess = true
        viewModel.needsGlobeKey = true
        viewModel.isModelInstalled = true
        viewModel.returnAppID = "messages"

        switch scene {
        case "keyboard-cold":
            viewModel.snapshot = .offline
            viewModel.style = .polished

        case "keyboard-no-access":
            viewModel.hasFullAccess = false
            viewModel.snapshot = .offline

        case "keyboard-warm":
            viewModel.style = .casual
            viewModel.snapshot = KeyboardSnapshot(
                state: .ready,
                heartbeat: now - 0.1,
                expiresAt: now + 14 * 60 + 20
            )

        case "keyboard-recording":
            viewModel.style = .polished
            viewModel.snapshot = KeyboardSnapshot(
                state: .recording,
                heartbeat: now - 0.05,
                expiresAt: now + 15 * 60,
                recordingStartedAt: now - 5,
                audioLevel: 0.78,
                loudnessDB: -31.0,
                jobID: "snap-job"
            )

        case "keyboard-transcribing":
            viewModel.style = .polished
            viewModel.snapshot = KeyboardSnapshot(
                state: .transcribing,
                heartbeat: now - 0.05,
                expiresAt: now + 15 * 60,
                jobID: "snap-job"
            )

        case "keyboard-style":
            viewModel.style = .casual
            viewModel.activeTray = .style
            viewModel.snapshot = KeyboardSnapshot(
                state: .ready,
                heartbeat: now - 0.1,
                expiresAt: now + 14 * 60
            )

        default:
            break
        }
    }
}
