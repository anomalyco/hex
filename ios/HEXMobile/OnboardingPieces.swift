import SwiftUI
import UIKit

// The pieces onboarding's pages share, in the app's flat style.

/// The desktop onboarding's motion (src/rune_ui/onboarding_kit/landing.rs and pill.rs): parts
/// roll in 8 pt on a 0.34 s zero-bounce spring, 0.05 s apart, and lift away on 0.26 s; the
/// download pill flies between its places on 0.42 s.
enum OnboardingMotion {
    static let travel: CGFloat = 8
    static let stagger: Double = 0.05
    static let enter = Animation.spring(duration: 0.34, bounce: 0)
    static let exit = Animation.spring(duration: 0.26, bounce: 0)
    static let fly = Animation.spring(duration: 0.42, bounce: 0)
}

extension EnvironmentValues {
    /// Where the download pill flies between pages; nil under Reduce Motion, where it fades.
    @Entry var onboardingPillSpace: Namespace.ID? = nil
}

extension View {
    /// Rolls in on arrival after "rank" stagger steps; a plain fade under Reduce Motion.
    func onboardingStagger(_ rank: Int) -> some View {
        modifier(OnboardingStagger(rank: rank))
    }
}

private struct OnboardingStagger: ViewModifier {
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    let rank: Int
    /// Stills show every part at once.
    @State private var shown = OnboardingFixture.isSnapshot && OnboardingFixture.current != .tour

    func body(content: Content) -> some View {
        content
            .opacity(shown ? 1 : 0)
            .offset(y: shown || reduceMotion ? 0 : OnboardingMotion.travel)
            .onAppear {
                guard !shown else { return }
                let animation = reduceMotion
                    ? HexMotion.reducedFade
                    : OnboardingMotion.enter.delay(Double(rank) * OnboardingMotion.stagger)
                withAnimation(animation) { shown = true }
            }
    }
}

private struct OnboardingPillFlight: ViewModifier {
    @Environment(\.onboardingPillSpace) private var space

    func body(content: Content) -> some View {
        if let space {
            content.matchedGeometryEffect(id: "download-pill", in: space)
        } else {
            content
        }
    }
}

@MainActor
func openSettings(_ openURL: OpenURLAction) {
    // Hex's own page in Settings (Apps › Hex), which lists Keyboards and Microphone.
    if let url = URL(string: UIApplication.openSettingsURLString) {
        openURL(url)
    }
}

struct OnboardingTitle: View {
    let title: LocalizedStringKey

    init(_ title: LocalizedStringKey) {
        self.title = title
    }

    var body: some View {
        Text(title)
            .font(.system(size: 28, weight: .semibold))
            .tracking(-0.4)
            .foregroundStyle(HexTheme.textBase)
            .multilineTextAlignment(.center)
    }
}

/// The top row: the model's download pill in the middle while it downloads, and a quiet Skip
/// on the optional pages, as on the desktop.
struct OnboardingTopBar: View {
    let model: OnboardingModel
    let onSkip: (() -> Void)?

    var body: some View {
        ZStack {
            if !model.isReady && !model.isFailed {
                OnboardingProgressPill(model: model, large: false)
                    .transition(.opacity)
            }
            HStack {
                Spacer()
                if let onSkip {
                    Button("Skip", action: onSkip)
                        .font(.system(size: 15, weight: .medium))
                        .foregroundStyle(HexTheme.textMuted)
                        .frame(minWidth: 44, minHeight: 44, alignment: .trailing)
                        .contentShape(Rectangle())
                        .buttonStyle(.plain)
                        .onboardingStagger(0)
                }
            }
        }
        .frame(height: 44)
        .animation(HexMotion.reducedFade, value: model.isReady)
    }
}

/// A hexagon outline that fills as the model downloads, beside its percentage. The large one
/// is Setting up Hex's; the small one sits in the top bar with the language.
struct OnboardingProgressPill: View {
    let model: OnboardingModel
    let large: Bool

    var body: some View {
        HStack(spacing: large ? 10 : 8) {
            ZStack {
                OnboardingHexagon()
                    .stroke(Color.white.opacity(0.18), lineWidth: 1.6)
                OnboardingHexagon()
                    .trim(from: 0, to: model.isReady ? 1 : max(0.02, model.fraction))
                    .stroke(model.isReady ? HexTheme.success : HexTheme.textBase, style: StrokeStyle(lineWidth: 1.6, lineCap: .round))
            }
            .frame(width: large ? 18 : 14, height: large ? 18 : 14)
            .animation(HexMotion.reducedFade, value: model.fraction)

            if !large {
                Text(model.language)
                    .foregroundStyle(HexTheme.textBase)
            }
            Group {
                if model.isReady {
                    Text("Ready")
                } else {
                    Text(model.fraction, format: .percent.precision(.fractionLength(0)))
                        .monospacedDigit()
                }
            }
            .foregroundStyle(large ? HexTheme.textBase : HexTheme.textMuted)
        }
        .font(.system(size: large ? 15 : 13, weight: .medium))
        .padding(.horizontal, large ? 14 : 11)
        .frame(height: large ? 36 : 30)
        .background(HexTheme.bgBase, in: Capsule())
        .modifier(OnboardingPillFlight())
        .accessibilityElement(children: .combine)
    }
}

struct OnboardingHexagon: Shape {
    func path(in rect: CGRect) -> Path {
        let r = min(rect.width, rect.height) / 2
        let c = CGPoint(x: rect.midX, y: rect.midY)
        var path = Path()
        for i in 0...6 {
            let angle = Double(i) * .pi / 3 - .pi / 2
            let point = CGPoint(x: c.x + r * cos(angle), y: c.y + r * sin(angle))
            if i == 0 { path.move(to: point) } else { path.addLine(to: point) }
        }
        return path
    }
}

struct OnboardingLanguageCapsule: View {
    let language: String

    var body: some View {
        Text(language)
            .font(.system(size: 14, weight: .medium))
            .foregroundStyle(HexTheme.textBase)
            .padding(.horizontal, 10)
            .frame(height: 26)
            .background(HexTheme.bgBase, in: Capsule())
            .overlay(Capsule().stroke(Color.white.opacity(0.1), lineWidth: 0.5))
    }
}

struct OnboardingFailure: View {
    let message: String
    let onRetry: () -> Void

    var body: some View {
        VStack(spacing: 12) {
            Text(message)
                .font(.system(size: 15))
                .foregroundStyle(HexTheme.textMuted)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)
            OnboardingSecondaryButton(title: "Try again", icon: "arrow.clockwise", action: onRetry)
                .frame(maxWidth: 200)
        }
    }
}

/// The lock line under Setting up Hex and the permissions.
struct OnboardingPrivacyLine: View {
    var body: some View {
        Label("Hex hears you on this iPhone. Your voice never leaves it.", systemImage: "lock")
            .font(.system(size: 13))
            .foregroundStyle(HexTheme.textMuted)
            .multilineTextAlignment(.center)
            .labelStyle(OnboardingInlineLabelStyle())
    }
}

private struct OnboardingInlineLabelStyle: LabelStyle {
    func makeBody(configuration: Configuration) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            configuration.icon.font(.system(size: 11, weight: .medium))
            configuration.title
        }
    }
}

/// A permission row: icon, title, a one-line detail that stays (so both rows keep one height),
/// and on the right either its action pill or, once done, only a check.
struct OnboardingPermissionRow: View {
    let icon: String
    let title: LocalizedStringKey
    let detail: LocalizedStringKey
    let done: Bool
    let actionTitle: LocalizedStringKey
    let action: () -> Void

    var body: some View {
        if done {
            content
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(Text(title))
                .accessibilityValue(Text("Done"))
        } else {
            Button(action: action) { content }
                .buttonStyle(OnboardingPressStyle())
                .accessibilityLabel(Text(title))
                .accessibilityHint(Text(actionTitle))
        }
    }

    private var content: some View {
        HStack(alignment: .center, spacing: 12) {
            Image(systemName: icon)
                .font(.system(size: 17, weight: .regular))
                .foregroundStyle(HexTheme.textBase)
                .frame(width: 24)
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.system(size: 16, weight: .medium))
                    .foregroundStyle(HexTheme.textBase)
                Text(detail)
                    .font(.system(size: 13))
                    .foregroundStyle(HexTheme.textMuted)
            }
            .lineLimit(1)
            .minimumScaleFactor(0.85)
            Spacer(minLength: 8)
            ZStack(alignment: .trailing) {
                OnboardingPill(title: actionTitle)
                    .opacity(done ? 0 : 1)
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 20, weight: .medium))
                    .foregroundStyle(HexTheme.success)
                    .opacity(done ? 1 : 0)
                    .scaleEffect(done ? 1 : 0.8)
            }
            .animation(HexMotion.settle, value: done)
        }
        .frame(minHeight: 64)
        .contentShape(Rectangle())
    }
}

/// The one filled action on a page.
struct OnboardingPrimaryButton: View {
    let title: LocalizedStringKey
    var enabled = true
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            Text(title)
                .font(.system(size: 17, weight: .semibold))
                .foregroundStyle(enabled ? HexTheme.textContrast : HexTheme.textFaint)
                .frame(maxWidth: .infinity, minHeight: 50)
                .background(enabled ? HexTheme.bgContrast : Color.white.opacity(0.07), in: Capsule())
                .contentShape(Capsule())
        }
        .buttonStyle(OnboardingPressStyle())
        .disabled(!enabled)
        .animation(HexMotion.reducedFade, value: enabled)
    }
}

/// A quiet text button: Skip's weight, for the second choice on a page.
struct OnboardingQuietButton: View {
    let title: LocalizedStringKey
    let action: () -> Void

    var body: some View {
        Button(title, action: action)
            .font(.system(size: 15, weight: .medium))
            .foregroundStyle(HexTheme.textBase)
            .frame(minHeight: 44)
            .contentShape(Rectangle())
            .buttonStyle(.plain)
    }
}

struct OnboardingSecondaryButton: View {
    let title: LocalizedStringKey
    let icon: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 7) {
                Image(systemName: icon)
                    .font(.system(size: 13, weight: .medium))
                Text(title)
                    .font(.system(size: 15, weight: .medium))
            }
            .foregroundStyle(HexTheme.textBase)
            .frame(maxWidth: .infinity, minHeight: 44)
            .background(Color.white.opacity(0.09), in: Capsule())
            .contentShape(Capsule())
        }
        .buttonStyle(OnboardingPressStyle())
    }
}

/// The small action pill on the right of a row; the row itself is the button.
struct OnboardingPill: View {
    let title: LocalizedStringKey

    var body: some View {
        Text(title)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle(HexTheme.textBase)
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(Color.white.opacity(0.09), in: Capsule())
            .fixedSize()
    }
}

struct OnboardingPressStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}
