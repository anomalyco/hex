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

    static let warmPearl = Color(red: 0.96, green: 0.94, blue: 0.89)
    static let agentBlue = Color(red: 0.78, green: 0.86, blue: 0.97)

    static let cardRadius: CGFloat = 16
    static let controlRadius: CGFloat = 10

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
    var size: CGFloat = 7

    var body: some View {
        Circle()
            .fill(color)
            .frame(width: size, height: size)
    }
}

/// Static braided dot-matrix glyph for WidgetKit / Live Activity surfaces
/// where SpringBoard renders archived display lists rather than Metal drawables.
struct HexDotMatrixBadge: View {
    var columns: Int = 21
    var rows: Int = 7
    var pitch: CGFloat = 2.6
    var cooling: Bool = false

    var body: some View {
        Canvas { context, size in
            let midX = size.width * 0.5
            let midY = size.height * 0.5
            let halfRows = CGFloat(rows - 1) * 0.5
            let halfCols = CGFloat(columns - 1) * 0.5
            let tint = cooling
                ? SIMD3<Double>(0.76, 0.85, 0.98)
                : SIMD3<Double>(0.96, 0.93, 0.87)

            for i in 0..<columns {
                let xn = Double(i) / Double(max(1, columns - 1))
                let edge = sin(.pi * xn)
                let amp = (0.35 + 0.65 * sin(.pi * xn)) * Double(halfRows * 0.82)
                let yA = Double(halfRows) + amp * sin(2.0 * .pi * 1.45 * xn - 0.5)
                let yB = Double(halfRows) - amp * sin(2.0 * .pi * 1.45 * xn - 0.5)

                for j in 0..<rows {
                    let dA = abs(Double(j) - yA)
                    let dB = abs(Double(j) - yB)
                    let litA = exp(-(dA * dA) / 0.42)
                    let litB = exp(-(dB * dB) / 0.42)
                    let power = min(1.0, (1.0 - (1.0 - litA) * (1.0 - litB)) * (0.25 + 0.75 * edge))
                    let alpha = 0.07 * edge + 0.89 * pow(power, 0.9)
                    let r = (0.26 + 0.16 * pow(power, 0.6)) * Double(pitch)
                    let cx = midX + (CGFloat(i) - halfCols) * pitch
                    let cy = midY + (CGFloat(j) - halfRows) * pitch
                    let rect = CGRect(x: cx - r, y: cy - r, width: r * 2, height: r * 2)
                    context.fill(
                        Path(ellipseIn: rect),
                        with: .color(Color(red: tint.x, green: tint.y, blue: tint.z, opacity: alpha))
                    )
                }
            }
        }
        .frame(
            width: CGFloat(columns - 1) * pitch + 8,
            height: CGFloat(rows - 1) * pitch + 8
        )
        .accessibilityHidden(true)
    }
}
