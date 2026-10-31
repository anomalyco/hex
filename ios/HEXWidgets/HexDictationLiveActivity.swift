import ActivityKit
import AppIntents
import SwiftUI
import WidgetKit

struct HexDictationLiveActivity: Widget {
    var body: some WidgetConfiguration {
        ActivityConfiguration(for: HexDictationActivityAttributes.self) { context in
            LockScreenBanner(state: context.state, isStale: context.isStale)
                .activityBackgroundTint(Color(red: 0x11 / 255.0, green: 0x13 / 255.0, blue: 0x16 / 255.0))
                .activitySystemActionForegroundColor(.white)
        } dynamicIsland: { context in
            let isRecording = !context.isStale && context.state.stage == .recording
            return DynamicIsland {
                DynamicIslandExpandedRegion(.bottom) {
                    LockScreenBanner(state: context.state, isStale: context.isStale)
                }
            } compactLeading: {
                Image(systemName: isRecording ? "mic.fill" : "waveform")
                    .font(.system(size: 13, weight: .semibold))
                    .foregroundStyle(isRecording ? HexTheme.danger : HexTheme.textMuted)
                    .padding(.leading, 3)
            } compactTrailing: {
                if isRecording {
                    Text(timerInterval: context.state.startedAt...Date.distantFuture, countsDown: false)
                        .font(.system(size: 14, weight: .semibold))
                        .monospacedDigit()
                        .foregroundStyle(.white)
                        .frame(maxWidth: 48, alignment: .trailing)
                        .padding(.trailing, 3)
                } else {
                    Text(context.state.stage == .transcribing ? "…" : "✓")
                        .font(.system(size: 13, weight: .semibold))
                        .foregroundStyle(HexTheme.textMuted)
                        .padding(.trailing, 4)
                }
            } minimal: {
                Image(systemName: "mic.fill")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundStyle(isRecording ? HexTheme.danger : HexTheme.textMuted)
            }
            .keylineTint(isRecording ? HexTheme.danger : .white)
        }
    }
}

private struct LockScreenBanner: View {
    let state: HexDictationActivityAttributes.ContentState
    let isStale: Bool

    private var activeStage: HexDictationActivityAttributes.Stage {
        isStale ? .copied : state.stage
    }

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(activeStage == .recording ? HexTheme.dangerSurface : HexTheme.layer02)
                    .frame(width: 40, height: 40)
                Image(systemName: activeStage == .copied ? "checkmark" : "mic.fill")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(activeStage == .recording ? HexTheme.danger : HexTheme.textBase)
            }

            VStack(alignment: .leading, spacing: 2) {
                Text(headline)
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(HexTheme.textBase)
                    .lineLimit(1)

                if activeStage == .recording {
                    Text(timerInterval: state.startedAt...Date.distantFuture, countsDown: false)
                        .font(.system(size: 13, weight: .medium))
                        .monospacedDigit()
                        .foregroundStyle(HexTheme.textMuted)
                } else {
                    Text(state.subtitle)
                        .font(.system(size: 13))
                        .foregroundStyle(HexTheme.textMuted)
                        .lineLimit(1)
                }
            }

            Spacer(minLength: 8)

            if activeStage == .recording {
                HStack(spacing: 6) {
                    Button(intent: CancelHexDictationIntent()) {
                        Image(systemName: "xmark")
                            .font(.system(size: 12, weight: .semibold))
                            .foregroundStyle(HexTheme.textMuted)
                            .frame(width: 36, height: 36)
                            .background(HexTheme.layer02, in: Circle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Cancel")

                    Button(intent: StopHexDictationIntent()) {
                        Text("Finish")
                            .font(.system(size: 13, weight: .semibold))
                            .foregroundStyle(HexTheme.textContrast)
                            .padding(.horizontal, 14)
                            .frame(height: 36)
                            .background(HexTheme.bgContrast, in: Capsule())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel("Finish dictation")
                }
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
    }

    private var headline: String {
        switch activeStage {
        case .recording: "Hex is listening"
        case .transcribing: "Transcribing…"
        case .copied: "Copied transcript"
        }
    }
}
