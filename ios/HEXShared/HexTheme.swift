import SwiftUI

enum HexTheme {
    // OpenCode dark palette — single cool-neutral temperature, tints via opacity.
    static let bgDeep = Color(red: 0x0A / 255.0, green: 0x0B / 255.0, blue: 0x0D / 255.0)
    static let bgBase = Color(red: 0x11 / 255.0, green: 0x13 / 255.0, blue: 0x16 / 255.0)
    static let layer01 = Color(red: 0x17 / 255.0, green: 0x19 / 255.0, blue: 0x1D / 255.0)
    static let layer02 = Color(red: 0x20 / 255.0, green: 0x23 / 255.0, blue: 0x29 / 255.0)

    static let textBase = Color.white.opacity(0.94)
    static let textMuted = Color.white.opacity(0.56)
    static let textFaint = Color.white.opacity(0.36)

    static let borderWeak = Color.white.opacity(0.06)
    static let borderStrong = Color.white.opacity(0.11)

    static let bgContrast = Color.white.opacity(0.96)
    static let textContrast = Color(red: 0x0A / 255.0, green: 0x0B / 255.0, blue: 0x0D / 255.0)

    static let danger = Color(red: 0xE5 / 255.0, green: 0x4D / 255.0, blue: 0x42 / 255.0)
    static let dangerSurface = Color(red: 0xE5 / 255.0, green: 0x4D / 255.0, blue: 0x42 / 255.0).opacity(0.14)
    static let warmMic = Color(red: 0xFF / 255.0, green: 0x95 / 255.0, blue: 0x00 / 255.0)
    static let success = Color(red: 0x34 / 255.0, green: 0xC7 / 255.0, blue: 0x59 / 255.0)

    static let cardRadius: CGFloat = 18
    static let controlRadius: CGFloat = 12

    static func formatClock(_ seconds: TimeInterval) -> String {
        let clamped = max(0, Int(seconds))
        return String(format: "%d:%02d", clamped / 60, clamped % 60)
    }

    static func formatRemaining(_ seconds: TimeInterval) -> String {
        let clamped = max(0, Int(ceil(seconds)))
        if clamped >= 3600 {
            let mins = (clamped % 3600) / 60
            return mins == 0 ? "\(clamped / 3600)h" : "\(clamped / 3600)h \(mins)m"
        }
        if clamped >= 90 {
            return "\((clamped + 59) / 60)m"
        }
        return formatClock(TimeInterval(clamped))
    }
}

struct HexStatusDot: View {
    var color: Color
    var size: CGFloat = 8

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: size, height: size)
    }
}

/// Minimal tactile bar meter inspired by Hex's desktop Dot Scope HUD.
struct DotScopeBar: View {
    var level: Float
    var barCount: Int = 18
    var active: Bool = true
    var height: CGFloat = 20

    private static let envelope: [CGFloat] = [
        0.22, 0.34, 0.52, 0.72, 0.90, 1.0, 0.84, 0.66, 0.94,
        0.78, 1.0, 0.86, 0.68, 0.92, 0.64, 0.48, 0.32, 0.20,
    ]

    var body: some View {
        let clamped = CGFloat(max(0, min(1, level)))
        HStack(spacing: 3) {
            ForEach(0..<barCount, id: \.self) { index in
                let weight = Self.envelope[index % Self.envelope.count]
                let signal = active ? max(0.16, min(1, clamped * (0.45 + 0.75 * weight) + 0.08 * weight)) : 0.16
                Capsule()
                    .fill(active ? HexTheme.textBase.opacity(0.28 + 0.72 * Double(signal)) : HexTheme.textFaint.opacity(0.5))
                    .frame(width: 2.5, height: max(3, height * signal))
            }
        }
        .frame(height: height)
        .animation(.easeOut(duration: 0.09), value: level)
        .accessibilityHidden(true)
    }
}
